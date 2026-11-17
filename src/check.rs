use crate::analysis::validate_types;
use crate::ast::{
    Action, ActionCall, BinaryOperator, CallArguments, Condition, Expr, Field, FieldKind, Scenario,
    ScenarioStep, Spec, UnaryOperator, Value,
};
use crate::{Error, Result};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_ARGUMENT_COMBINATIONS: usize = 10_000;
const MAX_TRANSITIONS: usize = 200_000;

pub type State = BTreeMap<String, Value>;
type Parameters = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub from: usize,
    pub to: usize,
    pub call: ActionCall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioResult {
    pub name: String,
    pub states: Vec<State>,
    pub calls: Vec<ActionCall>,
    pub expectations: usize,
}

#[derive(Debug, Clone)]
pub struct Verification {
    pub initial: State,
    pub states: Vec<State>,
    pub transitions: Vec<Transition>,
    pub scenarios: Vec<ScenarioResult>,
    pub promises: usize,
}

impl Verification {
    #[must_use]
    pub fn action_coverage(&self) -> BTreeMap<String, usize> {
        let mut coverage = BTreeMap::new();
        for transition in &self.transitions {
            *coverage.entry(transition.call.name.clone()).or_insert(0) += 1;
        }
        coverage
    }

    #[must_use]
    pub fn terminal_states(&self) -> usize {
        let outgoing: BTreeSet<usize> = self
            .transitions
            .iter()
            .map(|transition| transition.from)
            .collect();
        (0..self.states.len())
            .filter(|index| !outgoing.contains(index))
            .count()
    }
}

pub fn verify(spec: &Spec) -> Result<Verification> {
    validate_structure(spec)?;
    validate_types(spec)?;
    let initial = initial_state(spec)?;
    check_promises(spec, &initial, &[])?;

    let argument_sets = spec
        .actions
        .iter()
        .map(argument_combinations)
        .collect::<Result<Vec<_>>>()?;
    let mut states = vec![initial.clone()];
    let mut known = BTreeMap::from([(initial, 0)]);
    let mut parents = vec![None];
    let mut transitions = Vec::new();
    let mut queue = VecDeque::from([0]);

    while let Some(from) = queue.pop_front() {
        let state = states
            .get(from)
            .cloned()
            .ok_or_else(|| Error::new("internal state index is invalid"))?;
        for (action, all_arguments) in spec.actions.iter().zip(&argument_sets) {
            for arguments in all_arguments {
                let call = positional_call(action, arguments.clone(), action.line);
                let Some(next) = execute(spec, action, &state, arguments, call.line)? else {
                    continue;
                };
                let existing = known.get(&next).copied();
                let to = if let Some(existing) = existing {
                    existing
                } else {
                    if states.len() >= spec.state_limit {
                        return Err(Error::new(format!(
                            "exploration reached the {} state limit; tighten the model or use 'explore at most ... states'",
                            spec.state_limit
                        )));
                    }
                    let index = states.len();
                    known.insert(next.clone(), index);
                    states.push(next);
                    parents.push(Some((from, transitions.len())));
                    queue.push_back(index);
                    index
                };
                if transitions.len() >= MAX_TRANSITIONS {
                    return Err(Error::new(format!(
                        "exploration reached the {MAX_TRANSITIONS} transition limit; reduce action parameter ranges"
                    )));
                }
                transitions.push(Transition { from, to, call });
                if existing.is_none() {
                    let trace = trace_to(to, &parents, &transitions);
                    check_promises(spec, &states[to], &trace)?;
                }
            }
        }
    }

    let scenarios = spec
        .scenarios
        .iter()
        .map(|scenario| run_scenario(spec, &states[0], scenario))
        .collect::<Result<_>>()?;
    Ok(Verification {
        initial: states[0].clone(),
        states,
        transitions,
        scenarios,
        promises: spec.invariants.len(),
    })
}

fn validate_structure(spec: &Spec) -> Result<()> {
    if spec.name.trim().is_empty() {
        return Err(Error::new("the system needs a name"));
    }
    if spec.state_limit == 0 {
        return Err(Error::new("the state limit must be greater than zero"));
    }
    let mut fields = BTreeSet::new();
    let mut enum_values = BTreeSet::new();
    for field in &spec.state {
        if !fields.insert(field.name.clone()) {
            return Err(Error::at(
                field.line,
                format!("duplicate state field {}", field.name),
            ));
        }
        validate_field(field, &mut enum_values)?;
    }

    let mut initialized = BTreeSet::new();
    for assignment in &spec.initially {
        if !fields.contains(&assignment.field) {
            return Err(Error::at(
                assignment.line,
                format!("unknown initial field {}", assignment.field),
            ));
        }
        if !initialized.insert(assignment.field.clone()) {
            return Err(Error::at(
                assignment.line,
                format!("{} is initialized more than once", assignment.field),
            ));
        }
        validate_names(
            &assignment.value,
            &BTreeSet::new(),
            &enum_values,
            assignment.line,
        )?;
    }
    for field in &spec.state {
        if !initialized.contains(&field.name) {
            return Err(Error::at(
                field.line,
                format!("{} has no initial value", field.name),
            ));
        }
    }

    for promise in &spec.invariants {
        validate_names(&promise.expr, &fields, &enum_values, promise.line)?;
    }

    let mut action_names = BTreeSet::new();
    for action in &spec.actions {
        if action.name.trim().is_empty() {
            return Err(Error::at(action.line, "an action name cannot be empty"));
        }
        if !action_names.insert(action.name.clone()) {
            return Err(Error::at(
                action.line,
                format!("duplicate action {}", action.name),
            ));
        }
        let mut variables = fields.clone();
        let mut action_enums = enum_values.clone();
        let mut parameters = BTreeSet::new();
        for parameter in &action.params {
            if !parameters.insert(parameter.name.clone()) || fields.contains(&parameter.name) {
                return Err(Error::at(
                    parameter.line,
                    format!("duplicate or shadowing parameter {}", parameter.name),
                ));
            }
            variables.insert(parameter.name.clone());
            validate_field(parameter, &mut action_enums)?;
        }
        for condition in &action.requires {
            validate_names(&condition.expr, &variables, &action_enums, condition.line)?;
        }
        let mut assigned = BTreeSet::new();
        for assignment in &action.assignments {
            if !fields.contains(&assignment.field) {
                return Err(Error::at(
                    assignment.line,
                    format!("unknown assignment field {}", assignment.field),
                ));
            }
            if !assigned.insert(assignment.field.clone()) {
                return Err(Error::at(
                    assignment.line,
                    format!("{} is assigned twice in {}", assignment.field, action.name),
                ));
            }
            validate_names(
                &assignment.value,
                &variables,
                &action_enums,
                assignment.line,
            )?;
        }
    }

    let mut scenario_names = BTreeSet::new();
    for scenario in &spec.scenarios {
        if !scenario_names.insert(scenario.name.clone()) {
            return Err(Error::at(
                scenario.line,
                format!("duplicate scenario {}", scenario.name),
            ));
        }
        for step in &scenario.steps {
            match step {
                ScenarioStep::Call(call) if !action_names.contains(&call.name) => {
                    return Err(Error::at(
                        call.line,
                        format!("scenario calls unknown action {}", call.name),
                    ));
                }
                ScenarioStep::Expect(condition) => {
                    validate_names(&condition.expr, &fields, &enum_values, condition.line)?;
                }
                ScenarioStep::Call(_) => {}
            }
        }
    }
    Ok(())
}

fn validate_field(field: &Field, enum_values: &mut BTreeSet<String>) -> Result<()> {
    if let FieldKind::Enum { values } = &field.kind {
        if values.is_empty() {
            return Err(Error::at(
                field.line,
                format!("{} needs at least one enum value", field.name),
            ));
        }
        let mut local = BTreeSet::new();
        for value in values {
            if !local.insert(value) {
                return Err(Error::at(
                    field.line,
                    format!("{} repeats enum value {value}", field.name),
                ));
            }
            enum_values.insert(value.clone());
        }
    }
    Ok(())
}

fn validate_names(
    expression: &Expr,
    variables: &BTreeSet<String>,
    enum_values: &BTreeSet<String>,
    line: usize,
) -> Result<()> {
    match expression {
        Expr::Literal(_) => Ok(()),
        Expr::Name(name) if variables.contains(name) || enum_values.contains(name) => Ok(()),
        Expr::Name(name) => Err(Error::at(
            line,
            format!("unknown name {name}; declare it or use a quoted enum value"),
        )),
        Expr::Unary { value, .. } => validate_names(value, variables, enum_values, line),
        Expr::Binary { left, right, .. } => {
            validate_names(left, variables, enum_values, line)?;
            validate_names(right, variables, enum_values, line)
        }
    }
}

fn initial_state(spec: &Spec) -> Result<State> {
    let empty = State::new();
    let parameters = Parameters::new();
    let mut state = State::new();
    for assignment in &spec.initially {
        let value = evaluate(&assignment.value, &empty, &parameters)
            .map_err(|error| Error::at(assignment.line, error.message))?;
        let field = spec
            .state
            .iter()
            .find(|field| field.name == assignment.field)
            .ok_or_else(|| Error::at(assignment.line, "unknown initial field"))?;
        if !value_fits(&field.kind, &value) {
            return Err(Error::at(
                assignment.line,
                format!(
                    "initial value {value} does not satisfy {} is {}",
                    field.name, field.kind
                ),
            ));
        }
        state.insert(assignment.field.clone(), value);
    }
    Ok(state)
}

fn argument_combinations(action: &Action) -> Result<Vec<Vec<Value>>> {
    let mut combinations = vec![Vec::new()];
    for parameter in &action.params {
        let domain = domain_values(parameter)?;
        if combinations.len().saturating_mul(domain.len()) > MAX_ARGUMENT_COMBINATIONS {
            return Err(Error::at(
                parameter.line,
                format!(
                    "{} has more than {MAX_ARGUMENT_COMBINATIONS} parameter combinations; narrow its domains",
                    action.name
                ),
            ));
        }
        let mut expanded = Vec::with_capacity(combinations.len() * domain.len());
        for combination in &combinations {
            for value in &domain {
                let mut next = combination.clone();
                next.push(value.clone());
                expanded.push(next);
            }
        }
        combinations = expanded;
    }
    Ok(combinations)
}

fn domain_values(field: &Field) -> Result<Vec<Value>> {
    match &field.kind {
        FieldKind::Boolean => Ok(vec![Value::Boolean(false), Value::Boolean(true)]),
        FieldKind::Enum { values } => Ok(values.iter().cloned().map(Value::Text).collect()),
        FieldKind::Integer { min, max } => {
            let size = i128::from(*max) - i128::from(*min) + 1;
            if size > i128::try_from(MAX_ARGUMENT_COMBINATIONS).unwrap_or(i128::MAX) {
                return Err(Error::at(
                    field.line,
                    format!("{} has too many values", field.name),
                ));
            }
            let mut values = Vec::new();
            let mut current = *min;
            loop {
                values.push(Value::Integer(current));
                if current == *max {
                    break;
                }
                current = current
                    .checked_add(1)
                    .ok_or_else(|| Error::at(field.line, "integer range overflow"))?;
            }
            Ok(values)
        }
    }
}

fn execute(
    spec: &Spec,
    action: &Action,
    state: &State,
    arguments: &[Value],
    line: usize,
) -> Result<Option<State>> {
    let parameters = bind_parameters(action, arguments, line)?;
    for condition in &action.requires {
        if !condition_value(condition, state, &parameters)? {
            return Ok(None);
        }
    }
    let mut next = state.clone();
    for assignment in &action.assignments {
        let value = evaluate(&assignment.value, state, &parameters)
            .map_err(|error| Error::at(assignment.line, error.message))?;
        let field = spec
            .state
            .iter()
            .find(|field| field.name == assignment.field)
            .ok_or_else(|| Error::at(assignment.line, "unknown assignment field"))?;
        if !value_fits(&field.kind, &value) {
            return Err(Error::at(
                assignment.line,
                format!(
                    "action {} would assign {value} to {}, but {} is {}",
                    action.name, assignment.field, field.name, field.kind
                ),
            ));
        }
        next.insert(assignment.field.clone(), value);
    }
    Ok(Some(next))
}

fn bind_parameters(action: &Action, arguments: &[Value], line: usize) -> Result<Parameters> {
    if arguments.len() != action.params.len() {
        return Err(Error::at(
            line,
            format!(
                "{} expects {} arguments, received {}",
                action.name,
                action.params.len(),
                arguments.len()
            ),
        ));
    }
    let mut parameters = Parameters::new();
    for (parameter, value) in action.params.iter().zip(arguments) {
        if !value_fits(&parameter.kind, value) {
            return Err(Error::at(
                line,
                format!(
                    "{value} is not valid for {}; expected {}",
                    parameter.name, parameter.kind
                ),
            ));
        }
        parameters.insert(parameter.name.clone(), value.clone());
    }
    Ok(parameters)
}

fn run_scenario(spec: &Spec, initial: &State, scenario: &Scenario) -> Result<ScenarioResult> {
    let mut state = initial.clone();
    let mut states = vec![state.clone()];
    let mut calls = Vec::new();
    let mut expectations = 0;
    for step in &scenario.steps {
        match step {
            ScenarioStep::Call(call) => {
                let action = spec
                    .actions
                    .iter()
                    .find(|action| action.name == call.name)
                    .ok_or_else(|| Error::at(call.line, format!("unknown action {}", call.name)))?;
                let arguments = resolve_call_arguments(action, call)?;
                let canonical = positional_call(action, arguments.clone(), call.line);
                let Some(next) = execute(spec, action, &state, &arguments, call.line)? else {
                    return Err(Error::at(
                        call.line,
                        format!(
                            "scenario '{}': {} is not enabled\nstate: {}",
                            scenario.name,
                            canonical.display(),
                            display_state(&state)
                        ),
                    ));
                };
                let trace = vec![canonical.display()];
                check_promises(spec, &next, &trace).map_err(|error| {
                    Error::new(format!("scenario '{}': {}", scenario.name, error))
                })?;
                state = next;
                states.push(state.clone());
                calls.push(canonical);
            }
            ScenarioStep::Expect(condition) => {
                if !condition_value(condition, &state, &Parameters::new())? {
                    return Err(Error::at(
                        condition.line,
                        format!(
                            "scenario '{}': expectation failed: {}\nstate: {}",
                            scenario.name,
                            condition.source,
                            display_state(&state)
                        ),
                    ));
                }
                expectations += 1;
            }
        }
    }
    Ok(ScenarioResult {
        name: scenario.name.clone(),
        states,
        calls,
        expectations,
    })
}

fn resolve_call_arguments(action: &Action, call: &ActionCall) -> Result<Vec<Value>> {
    match &call.arguments {
        CallArguments::Positional(arguments) => {
            if arguments.len() != action.params.len() {
                return Err(Error::at(
                    call.line,
                    format!(
                        "{} expects {} arguments, received {}",
                        action.name,
                        action.params.len(),
                        arguments.len()
                    ),
                ));
            }
            Ok(arguments.clone())
        }
        CallArguments::Named(arguments) => {
            for name in arguments.keys() {
                if !action
                    .params
                    .iter()
                    .any(|parameter| parameter.name == *name)
                {
                    return Err(Error::at(
                        call.line,
                        format!("{} has no parameter {name}", action.name),
                    ));
                }
            }
            let mut arguments = arguments.clone();
            action
                .params
                .iter()
                .map(|parameter| {
                    arguments.remove(&parameter.name).ok_or_else(|| {
                        Error::at(
                            call.line,
                            format!("{} is missing argument {}", action.name, parameter.name),
                        )
                    })
                })
                .collect()
        }
    }
}

fn positional_call(action: &Action, arguments: Vec<Value>, line: usize) -> ActionCall {
    ActionCall {
        name: action.name.clone(),
        arguments: CallArguments::Positional(arguments),
        line,
    }
}

fn check_promises(spec: &Spec, state: &State, trace: &[String]) -> Result<()> {
    for promise in &spec.invariants {
        if !condition_value(promise, state, &Parameters::new())? {
            let mut message = format!(
                "promise failed: {}\nstate: {}",
                promise.source,
                display_state(state)
            );
            if !trace.is_empty() {
                message.push_str("\ncounterexample:");
                for (index, call) in trace.iter().enumerate() {
                    message.push_str(&format!("\n  {}. {call}", index + 1));
                }
            }
            return Err(Error::at(promise.line, message));
        }
    }
    Ok(())
}

fn trace_to(
    mut state: usize,
    parents: &[Option<(usize, usize)>],
    transitions: &[Transition],
) -> Vec<String> {
    let mut trace = Vec::new();
    while let Some(Some((previous, transition))) = parents.get(state) {
        if let Some(step) = transitions.get(*transition) {
            trace.push(step.call.display());
        }
        state = *previous;
    }
    trace.reverse();
    trace
}

fn condition_value(condition: &Condition, state: &State, parameters: &Parameters) -> Result<bool> {
    match evaluate(&condition.expr, state, parameters)
        .map_err(|error| Error::at(condition.line, error.message))?
    {
        Value::Boolean(value) => Ok(value),
        other => Err(Error::at(
            condition.line,
            format!(
                "condition '{}' must be boolean, found {other}",
                condition.source
            ),
        )),
    }
}

fn evaluate(expression: &Expr, state: &State, parameters: &Parameters) -> Result<Value> {
    match expression {
        Expr::Literal(value) => Ok(value.clone()),
        Expr::Name(name) => Ok(parameters
            .get(name)
            .or_else(|| state.get(name))
            .cloned()
            .unwrap_or_else(|| Value::Text(name.clone()))),
        Expr::Unary { operator, value } => {
            let value = evaluate(value, state, parameters)?;
            match (operator, value) {
                (UnaryOperator::Not, Value::Boolean(value)) => Ok(Value::Boolean(!value)),
                (UnaryOperator::Negate, Value::Integer(value)) => value
                    .checked_neg()
                    .map(Value::Integer)
                    .ok_or_else(|| Error::new("integer negation overflow")),
                (UnaryOperator::Not, other) => {
                    Err(Error::new(format!("not expects a boolean, found {other}")))
                }
                (UnaryOperator::Negate, other) => Err(Error::new(format!(
                    "negation expects an integer, found {other}"
                ))),
            }
        }
        Expr::Binary {
            operator: BinaryOperator::And,
            left,
            right,
        } => {
            let left = boolean(evaluate(left, state, parameters)?)?;
            if !left {
                return Ok(Value::Boolean(false));
            }
            Ok(Value::Boolean(boolean(evaluate(
                right, state, parameters,
            )?)?))
        }
        Expr::Binary {
            operator: BinaryOperator::Or,
            left,
            right,
        } => {
            let left = boolean(evaluate(left, state, parameters)?)?;
            if left {
                return Ok(Value::Boolean(true));
            }
            Ok(Value::Boolean(boolean(evaluate(
                right, state, parameters,
            )?)?))
        }
        Expr::Binary {
            operator: BinaryOperator::Implies,
            left,
            right,
        } => {
            let left = boolean(evaluate(left, state, parameters)?)?;
            if !left {
                return Ok(Value::Boolean(true));
            }
            Ok(Value::Boolean(boolean(evaluate(
                right, state, parameters,
            )?)?))
        }
        Expr::Binary {
            operator,
            left,
            right,
        } => {
            let left = evaluate(left, state, parameters)?;
            let right = evaluate(right, state, parameters)?;
            evaluate_binary(*operator, left, right)
        }
    }
}

fn evaluate_binary(operator: BinaryOperator, left: Value, right: Value) -> Result<Value> {
    match operator {
        BinaryOperator::Equal | BinaryOperator::NotEqual => {
            if std::mem::discriminant(&left) != std::mem::discriminant(&right) {
                return Err(Error::new(format!("cannot compare {left} with {right}")));
            }
            let equal = left == right;
            Ok(Value::Boolean(if operator == BinaryOperator::Equal {
                equal
            } else {
                !equal
            }))
        }
        BinaryOperator::Less
        | BinaryOperator::LessEqual
        | BinaryOperator::Greater
        | BinaryOperator::GreaterEqual => {
            let left = integer(left)?;
            let right = integer(right)?;
            Ok(Value::Boolean(match operator {
                BinaryOperator::Less => left < right,
                BinaryOperator::LessEqual => left <= right,
                BinaryOperator::Greater => left > right,
                BinaryOperator::GreaterEqual => left >= right,
                _ => unreachable!("comparison operator was matched above"),
            }))
        }
        BinaryOperator::Add
        | BinaryOperator::Subtract
        | BinaryOperator::Multiply
        | BinaryOperator::Divide => {
            let left = integer(left)?;
            let right = integer(right)?;
            let result = match operator {
                BinaryOperator::Add => left.checked_add(right),
                BinaryOperator::Subtract => left.checked_sub(right),
                BinaryOperator::Multiply => left.checked_mul(right),
                BinaryOperator::Divide if right == 0 => return Err(Error::new("division by zero")),
                BinaryOperator::Divide => left.checked_div(right),
                _ => unreachable!("arithmetic operator was matched above"),
            };
            result
                .map(Value::Integer)
                .ok_or_else(|| Error::new("integer arithmetic overflow"))
        }
        BinaryOperator::And | BinaryOperator::Or | BinaryOperator::Implies => {
            Err(Error::new("logical operator reached arithmetic evaluation"))
        }
    }
}

fn boolean(value: Value) -> Result<bool> {
    if let Value::Boolean(value) = value {
        Ok(value)
    } else {
        Err(Error::new(format!("expected a boolean, found {value}")))
    }
}

fn integer(value: Value) -> Result<i64> {
    if let Value::Integer(value) = value {
        Ok(value)
    } else {
        Err(Error::new(format!("expected an integer, found {value}")))
    }
}

fn value_fits(kind: &FieldKind, value: &Value) -> bool {
    match (kind, value) {
        (FieldKind::Integer { min, max }, Value::Integer(value)) => value >= min && value <= max,
        (FieldKind::Enum { values }, Value::Text(value)) => values.contains(value),
        (FieldKind::Boolean, Value::Boolean(_)) => true,
        _ => false,
    }
}

pub fn display_state(state: &State) -> String {
    state
        .iter()
        .map(|(name, value)| format!("{name} = {}", value.source()))
        .collect::<Vec<_>>()
        .join(", ")
}
