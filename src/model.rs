use crate::ast::{
    Action, ActionCall, Assignment, BinaryOperator, CallArguments, Condition, Expr, Fairness,
    PropertyKind, Quantifier, ScenarioStep, Spec, TargetKey, TypeExpr, UnaryOperator, Value,
    display_call,
};
use crate::symmetry::Symmetry;
use crate::{Error, Result, State};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(crate) const MAX_ARGUMENT_COMBINATIONS: usize = 10_000;
const MAX_CALLS: usize = 100_000;
const MAX_SLOTS: usize = 4_096;
const MAX_QUANTIFIER_VALUES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Domain {
    Integer { min: i64, max: i64 },
    Boolean,
    Enum(Vec<u32>),
}

impl Domain {
    #[must_use]
    pub fn size(&self) -> usize {
        match self {
            Self::Integer { min, max } => {
                usize::try_from(i128::from(*max) - i128::from(*min) + 1).unwrap_or(usize::MAX)
            }
            Self::Boolean => 2,
            Self::Enum(ids) => ids.len(),
        }
    }

    pub(crate) fn position(&self, raw: i64) -> Option<usize> {
        match self {
            Self::Integer { min, max } => {
                if raw < *min || raw > *max {
                    None
                } else {
                    usize::try_from(i128::from(raw) - i128::from(*min)).ok()
                }
            }
            Self::Boolean => usize::try_from(raw).ok().filter(|position| *position < 2),
            Self::Enum(ids) => ids.iter().position(|id| i64::from(*id) == raw),
        }
    }

    pub(crate) fn raw_at(&self, position: usize) -> Option<i64> {
        match self {
            Self::Integer { min, max } => i64::try_from(position)
                .ok()
                .and_then(|offset| min.checked_add(offset))
                .filter(|raw| raw <= max),
            Self::Boolean => i64::try_from(position).ok().filter(|raw| *raw < 2),
            Self::Enum(ids) => ids.get(position).map(|id| i64::from(*id)),
        }
    }

    pub(crate) fn raws(&self) -> Vec<i64> {
        (0..self.size())
            .filter_map(|position| self.raw_at(position))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    Scalar(Domain),
    Map {
        key_type: String,
        keys: Domain,
        value: Domain,
    },
}

#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: String,
    pub kind: FieldType,
    pub(crate) offset: usize,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub domain: Domain,
}

#[derive(Debug, Clone)]
pub struct ActionInfo {
    pub name: String,
    pub fairness: Fairness,
    pub params: Vec<Param>,
    pub line: usize,
    pub(crate) requires: Vec<CompiledCondition>,
    pub(crate) assignments: Vec<CompiledAssignment>,
}

#[derive(Debug, Clone)]
pub(crate) enum Ir {
    Const(i64),
    Slot(usize),
    Local(usize),
    Index {
        offset: usize,
        keys: Domain,
        key: Box<Ir>,
        field: String,
    },
    Not(Box<Ir>),
    Negate(Box<Ir>),
    Binary(BinaryOperator, Box<Ir>, Box<Ir>),
    If(Box<Ir>, Box<Ir>, Box<Ir>),
    Quantified {
        quantifier: Quantifier,
        local: usize,
        values: Vec<i64>,
        body: Box<Ir>,
    },
}

impl Ir {
    /// In an ordered existential/universal traversal a different witness can
    /// hide an error. Integer arithmetic and integer map indexing are partial.
    fn evaluation(&self) -> (bool, bool) {
        let combine = |(a, b), (c, d)| (a || c, b || d);
        match self {
            Self::Const(_) | Self::Slot(_) | Self::Local(_) => (false, false),
            Self::Index { keys, key, .. } => combine(
                key.evaluation(),
                (matches!(keys, Domain::Integer { .. }), false),
            ),
            Self::Not(x) => x.evaluation(),
            Self::Negate(x) => combine(x.evaluation(), (true, false)),
            Self::Binary(op, a, b) => {
                let partial = matches!(
                    op,
                    BinaryOperator::Add
                        | BinaryOperator::Subtract
                        | BinaryOperator::Multiply
                        | BinaryOperator::Divide
                );
                combine(combine(a.evaluation(), b.evaluation()), (partial, false))
            }
            Self::If(c, a, b) => combine(c.evaluation(), combine(a.evaluation(), b.evaluation())),
            Self::Quantified {
                quantifier, body, ..
            } => {
                let (partial, sensitive) = body.evaluation();
                (
                    partial,
                    sensitive || (partial && *quantifier != Quantifier::Count),
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledCondition {
    pub ir: Ir,
    pub source: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledProperty {
    pub kind: PropertyKind,
    pub premise: Option<CompiledCondition>,
    pub condition: CompiledCondition,
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum Target {
    Slot(usize),
    Indexed {
        offset: usize,
        keys: Domain,
        key: Ir,
        field: String,
    },
    Every {
        offset: usize,
        count: usize,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledAssignment {
    pub target: Target,
    pub value: Ir,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum CompiledStep {
    Call { call: u32, line: usize },
    Reject { call: u32, line: usize },
    Expect(CompiledCondition),
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledScenario {
    pub name: String,
    pub steps: Vec<CompiledStep>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Call {
    pub action: usize,
    pub args: Vec<i64>,
}

#[derive(Debug, Clone)]
pub(crate) struct Slot {
    pub domain: Domain,
    pub label: String,
}

#[derive(Debug, Clone)]
pub(crate) struct Fault {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    pub fingerprint: String,
    pub sources: BTreeMap<String, String>,
    pub component: Option<String>,
    pub(crate) source_map: Option<std::sync::Arc<crate::source::SourceMap>>,
    pub state_limit: usize,
    pub fields: Vec<FieldInfo>,
    pub actions: Vec<ActionInfo>,
    pub(crate) symbols: Vec<String>,
    pub(crate) slots: Vec<Slot>,
    pub(crate) initial: Box<[i64]>,
    pub(crate) properties: Vec<CompiledProperty>,
    pub(crate) terminal: Option<CompiledCondition>,
    pub(crate) scenarios: Vec<CompiledScenario>,
    pub(crate) calls: Vec<Call>,
    pub(crate) call_ids: HashMap<Call, u32>,
    pub(crate) locals: usize,
    pub(crate) symmetry: Option<Symmetry>,
    pub(crate) interchangeable: Vec<Vec<i64>>,
}

pub(crate) fn index32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(crate) fn at(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

impl Model {
    pub fn compile(spec: &Spec) -> Result<Self> {
        let result = Compiler::new(spec).compile();
        match &spec.source_map {
            Some(map) => result.map_err(|e| map.locate(e)),
            None => result,
        }
    }

    pub(crate) fn order_sensitive_evaluation(&self) -> bool {
        let sensitive = |ir: &Ir| ir.evaluation().1;
        self.properties.iter().any(|p| {
            sensitive(&p.condition.ir) || p.premise.as_ref().is_some_and(|c| sensitive(&c.ir))
        }) || self.terminal.as_ref().is_some_and(|c| sensitive(&c.ir))
            || self.actions.iter().any(|a| {
                a.requires.iter().any(|c| sensitive(&c.ir))
                    || a.assignments.iter().any(|w| {
                        sensitive(&w.value)
                            || matches!(&w.target, Target::Indexed { key, .. } if sensitive(key))
                    })
            })
    }

    #[must_use]
    pub fn symbol(&self, raw: i64) -> &str {
        usize::try_from(raw)
            .ok()
            .and_then(|index| self.symbols.get(index))
            .map_or("?", String::as_str)
    }

    #[must_use]
    pub fn value(&self, domain: &Domain, raw: i64) -> Value {
        match domain {
            Domain::Integer { .. } => Value::Integer(raw),
            Domain::Boolean => Value::Boolean(raw != 0),
            Domain::Enum(_) => Value::Symbol(self.symbol(raw).to_string()),
        }
    }

    #[must_use]
    pub fn encode(&self, domain: &Domain, value: &Value) -> Option<i64> {
        let raw = match (domain, value) {
            (Domain::Integer { .. }, Value::Integer(raw)) => *raw,
            (Domain::Boolean, Value::Boolean(flag)) => i64::from(*flag),
            (Domain::Enum(ids), Value::Symbol(name)) => ids
                .iter()
                .find(|id| self.symbol(i64::from(**id)) == name)
                .map(|id| i64::from(*id))?,
            _ => return None,
        };
        domain.position(raw).map(|_| raw)
    }

    #[must_use]
    pub fn describe(&self, domain: &Domain) -> String {
        match domain {
            Domain::Integer { min, max } => format!("an integer from {min} to {max}"),
            Domain::Boolean => "a boolean".to_string(),
            Domain::Enum(ids) => format!(
                "one of {}",
                ids.iter()
                    .map(|id| Value::Symbol(self.symbol(i64::from(*id)).to_string()).source())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    pub(crate) fn decode(&self, state: &[i64]) -> State {
        let mut decoded = State::new();
        for field in &self.fields {
            let value = match &field.kind {
                FieldType::Scalar(domain) => {
                    self.value(domain, state.get(field.offset).copied().unwrap_or(0))
                }
                FieldType::Map { keys, value, .. } => Value::Map(
                    keys.raws()
                        .into_iter()
                        .enumerate()
                        .map(|(position, key)| {
                            let raw = state.get(field.offset + position).copied().unwrap_or(0);
                            (self.value(keys, key).to_string(), self.value(value, raw))
                        })
                        .collect(),
                ),
            };
            decoded.insert(field.name.clone(), value);
        }
        decoded
    }

    #[must_use]
    pub fn action_index(&self, name: &str) -> Option<usize> {
        self.actions.iter().position(|action| action.name == name)
    }

    /// How many distinct calls (an action with concrete arguments) exist.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.calls.len()
    }

    pub(crate) fn call_values(&self, call: u32) -> Vec<Value> {
        let Some(call) = self.calls.get(at(call)) else {
            return Vec::new();
        };
        let Some(action) = self.actions.get(call.action) else {
            return Vec::new();
        };
        action
            .params
            .iter()
            .zip(&call.args)
            .map(|(param, raw)| self.value(&param.domain, *raw))
            .collect()
    }

    pub(crate) fn call_action(&self, call: u32) -> &str {
        self.calls
            .get(at(call))
            .and_then(|call| self.actions.get(call.action))
            .map_or("?", |action| action.name.as_str())
    }

    pub(crate) fn call_text(&self, call: u32) -> String {
        display_call(self.call_action(call), &self.call_values(call))
    }

    pub(crate) fn new_locals(&self) -> Vec<i64> {
        vec![0; self.locals]
    }

    pub(crate) fn holds(
        &self,
        condition: &CompiledCondition,
        state: &[i64],
        locals: &mut [i64],
    ) -> std::result::Result<bool, Fault> {
        eval(&condition.ir, state, locals)
            .map(|raw| raw != 0)
            .map_err(|message| Fault {
                line: condition.line,
                message: format!("{message} while evaluating '{}'", condition.source),
            })
    }

    pub(crate) fn step(
        &self,
        call: u32,
        state: &[i64],
        locals: &mut [i64],
    ) -> std::result::Result<Option<Box<[i64]>>, Fault> {
        let missing = || Fault {
            line: 0,
            message: "internal error: unknown call".to_string(),
        };
        let call = self.calls.get(at(call)).ok_or_else(missing)?;
        let action = self.actions.get(call.action).ok_or_else(missing)?;
        for (local, raw) in locals.iter_mut().zip(&call.args) {
            *local = *raw;
        }
        for condition in &action.requires {
            if !self.holds(condition, state, locals)? {
                return Ok(None);
            }
        }
        let mut next: Box<[i64]> = state.into();
        for write in collect_writes(&action.assignments, state, locals, &self.slots)? {
            let slot = self.slots.get(write.slot).ok_or_else(missing)?;
            if slot.domain.position(write.raw).is_none() {
                return Err(Fault {
                    line: write.line,
                    message: format!(
                        "{} would become {}, but it is {}; add a 'requires' that rules this out",
                        slot.label,
                        self.value(&slot.domain, write.raw),
                        self.describe(&slot.domain)
                    ),
                });
            }
            if let Some(cell) = next.get_mut(write.slot) {
                *cell = write.raw;
            }
        }
        Ok(Some(next))
    }
}

pub(crate) struct Write {
    pub slot: usize,
    pub raw: i64,
    pub line: usize,
}

fn key_slot(
    offset: usize,
    keys: &Domain,
    raw: i64,
    field: &str,
) -> std::result::Result<usize, String> {
    keys.position(raw)
        .map(|position| offset + position)
        .ok_or_else(|| format!("{raw} is not a key of {field}"))
}

pub(crate) fn collect_writes(
    assignments: &[CompiledAssignment],
    state: &[i64],
    locals: &mut [i64],
    slots: &[Slot],
) -> std::result::Result<Vec<Write>, Fault> {
    let mut writes = Vec::new();
    for assignment in assignments {
        if let Target::Every { offset, count } = &assignment.target {
            let raw = eval(&assignment.value, state, locals).map_err(|message| Fault {
                line: assignment.line,
                message,
            })?;
            writes.extend((0..*count).map(|position| Write {
                slot: offset + position,
                raw,
                line: assignment.line,
            }));
        }
    }
    let broad = writes.len();
    for assignment in assignments {
        let fault = |message: String| Fault {
            line: assignment.line,
            message,
        };
        let slot = match &assignment.target {
            Target::Every { .. } => continue,
            Target::Slot(slot) => *slot,
            Target::Indexed {
                offset,
                keys,
                key,
                field,
            } => {
                let key = eval(key, state, locals).map_err(fault)?;
                key_slot(*offset, keys, key, field).map_err(fault)?
            }
        };
        if writes.iter().skip(broad).any(|write| write.slot == slot) {
            let label = slots
                .get(slot)
                .map_or("a field", |slot| slot.label.as_str());
            return Err(fault(format!("{label} is assigned twice in one step")));
        }
        let raw = eval(&assignment.value, state, locals).map_err(fault)?;
        writes.push(Write {
            slot,
            raw,
            line: assignment.line,
        });
    }
    Ok(writes)
}

fn fetch(values: &[i64], index: usize) -> std::result::Result<i64, String> {
    values
        .get(index)
        .copied()
        .ok_or_else(|| "internal error: value out of range".to_string())
}

pub(crate) fn eval(ir: &Ir, state: &[i64], locals: &mut [i64]) -> std::result::Result<i64, String> {
    Ok(match ir {
        Ir::Const(raw) => *raw,
        Ir::Slot(slot) => fetch(state, *slot)?,
        Ir::Local(local) => fetch(locals, *local)?,
        Ir::Index {
            offset,
            keys,
            key,
            field,
        } => {
            let key = eval(key, state, locals)?;
            fetch(state, key_slot(*offset, keys, key, field)?)?
        }
        Ir::Not(value) => i64::from(eval(value, state, locals)? == 0),
        Ir::Negate(value) => eval(value, state, locals)?
            .checked_neg()
            .ok_or("integer overflow")?,
        Ir::If(condition, then, otherwise) => {
            if eval(condition, state, locals)? != 0 {
                eval(then, state, locals)?
            } else {
                eval(otherwise, state, locals)?
            }
        }
        Ir::Quantified {
            quantifier,
            local,
            values,
            body,
        } => {
            let mut count: i64 = 0;
            for value in values {
                if let Some(cell) = locals.get_mut(*local) {
                    *cell = *value;
                }
                if eval(body, state, locals)? != 0 {
                    count += 1;
                    if *quantifier == Quantifier::Some {
                        return Ok(1);
                    }
                } else if *quantifier == Quantifier::Every {
                    return Ok(0);
                }
            }
            match quantifier {
                Quantifier::Every => 1,
                Quantifier::Some => 0,
                Quantifier::Count => count,
            }
        }
        Ir::Binary(operator, left, right) => {
            let left = eval(left, state, locals)?;
            match operator {
                BinaryOperator::And if left == 0 => return Ok(0),
                BinaryOperator::Or if left != 0 => return Ok(1),
                BinaryOperator::Implies if left == 0 => return Ok(1),
                _ => {}
            }
            let right = eval(right, state, locals)?;
            match operator {
                BinaryOperator::And | BinaryOperator::Or | BinaryOperator::Implies => right,
                BinaryOperator::Equal => i64::from(left == right),
                BinaryOperator::NotEqual => i64::from(left != right),
                BinaryOperator::Less => i64::from(left < right),
                BinaryOperator::LessEqual => i64::from(left <= right),
                BinaryOperator::Greater => i64::from(left > right),
                BinaryOperator::GreaterEqual => i64::from(left >= right),
                BinaryOperator::Add => left.checked_add(right).ok_or("integer overflow")?,
                BinaryOperator::Subtract => left.checked_sub(right).ok_or("integer overflow")?,
                BinaryOperator::Multiply => left.checked_mul(right).ok_or("integer overflow")?,
                BinaryOperator::Divide => {
                    if right == 0 {
                        return Err("division by zero".to_string());
                    }
                    left.checked_div(right).ok_or("integer overflow")?
                }
            }
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ty {
    Int,
    Bool,
    Enum(BTreeSet<u32>),
}

impl Ty {
    fn of(domain: &Domain) -> Self {
        match domain {
            Domain::Integer { .. } => Self::Int,
            Domain::Boolean => Self::Bool,
            Domain::Enum(ids) => Self::Enum(ids.iter().copied().collect()),
        }
    }

    fn noun(&self) -> &'static str {
        match self {
            Self::Int => "a number",
            Self::Bool => "a true/false value",
            Self::Enum(_) => "an enum value",
        }
    }
}

enum Resolved {
    Scalar(Domain),
    Map {
        key_type: String,
        keys: Domain,
        value: Domain,
    },
}

struct Compiler<'a> {
    spec: &'a Spec,
    symbols: Vec<String>,
    symbol_ids: HashMap<String, u32>,
    types: HashMap<String, Domain>,
    fields: Vec<FieldInfo>,
    field_ids: HashMap<String, usize>,
    slots: Vec<Slot>,
    scope: Vec<(String, Ty)>,
    locals: usize,
    constant_only: bool,
    /// Interchangeable values, which the specification may not name, and their type.
    unnamed: HashMap<u32, String>,
    /// Every enumeration written in the specification, with its line.
    enums: Vec<(Vec<u32>, usize)>,
}

impl<'a> Compiler<'a> {
    fn new(spec: &'a Spec) -> Self {
        Self {
            spec,
            symbols: Vec::new(),
            symbol_ids: HashMap::new(),
            types: HashMap::new(),
            fields: Vec::new(),
            field_ids: HashMap::new(),
            slots: Vec::new(),
            scope: Vec::new(),
            locals: 0,
            constant_only: false,
            unnamed: HashMap::new(),
            enums: Vec::new(),
        }
    }

    fn compile(mut self) -> Result<Model> {
        let spec = self.spec;
        for declaration in &spec.types {
            let Resolved::Scalar(domain) = self.resolve(&declaration.kind, declaration.line)?
            else {
                return Err(Error::at(
                    declaration.line,
                    "a named type cannot be a map; declare the map in state",
                ));
            };
            if self
                .types
                .insert(declaration.name.clone(), domain)
                .is_some()
            {
                return Err(Error::at(
                    declaration.line,
                    format!("type {} is declared twice", declaration.name),
                ));
            }
        }
        let groups = self.interchangeable()?;
        for field in &spec.state {
            self.declare_field(&field.name, &field.kind, field.line)?;
        }
        let mut signatures = Vec::new();
        for action in &spec.actions {
            signatures.push(self.signature(action)?);
        }
        self.check_partial_groups(&groups)?;
        let initial = self.initial_state()?;

        let mut properties = Vec::new();
        for property in &spec.properties {
            let premise = match &property.premise {
                Some(premise) => Some(self.condition(premise)?),
                None => None,
            };
            properties.push(CompiledProperty {
                kind: property.kind,
                premise,
                condition: self.condition(&property.condition)?,
                text: property.display(),
                line: property.line,
            });
        }
        let terminal = match &spec.terminal {
            Some(condition) => Some(self.condition(condition)?),
            None => None,
        };

        let mut actions = Vec::new();
        for (action, params) in spec.actions.iter().zip(signatures) {
            actions.push(self.action(action, params)?);
        }
        let (calls, call_ids) = enumerate_calls(&actions)?;

        let mut model = Model {
            name: spec.name.clone(),
            fingerprint: spec.fingerprint.clone(),
            sources: spec.sources.clone(),
            component: None,
            source_map: spec.source_map.clone(),
            state_limit: spec.state_limit,
            fields: self.fields,
            actions,
            symbols: self.symbols,
            slots: self.slots,
            initial,
            properties,
            terminal,
            scenarios: Vec::new(),
            calls,
            call_ids,
            locals: self.locals.max(1),
            symmetry: None,
            interchangeable: groups
                .into_iter()
                .map(|(_, ids, _)| ids.into_iter().map(i64::from).collect())
                .collect(),
        };
        crate::symmetry::validate(&model)?;
        let mut scenarios = Compiler {
            spec,
            symbols: model.symbols.clone(),
            symbol_ids: self.symbol_ids,
            types: self.types,
            fields: model.fields.clone(),
            field_ids: self.field_ids,
            slots: Vec::new(),
            scope: Vec::new(),
            locals: model.locals,
            constant_only: false,
            unnamed: HashMap::new(),
            enums: Vec::new(),
        };
        for scenario in &spec.scenarios {
            let mut steps = Vec::new();
            for step in &scenario.steps {
                steps.push(match step {
                    ScenarioStep::Call(call) => CompiledStep::Call {
                        call: resolve_call(&model, call)?,
                        line: call.line,
                    },
                    ScenarioStep::Reject(call) => CompiledStep::Reject {
                        call: resolve_call(&model, call)?,
                        line: call.line,
                    },
                    ScenarioStep::Expect(condition) => {
                        CompiledStep::Expect(scenarios.condition(condition)?)
                    }
                });
            }
            model.scenarios.push(CompiledScenario {
                name: scenario.name.clone(),
                steps,
                line: scenario.line,
            });
        }
        model.locals = model.locals.max(scenarios.locals);
        Ok(model)
    }

    fn symbol(&self, id: u32, line: usize) -> Result<(Ir, Ty)> {
        if let Some(kind) = self.unnamed.get(&id) {
            let name = self.symbols.get(at(id)).map_or("?", String::as_str);
            return Err(Error::at(
                line,
                format!(
                    "{name} is an interchangeable {kind}, so the specification cannot single it \
                     out; refer to it through a parameter or a quantifier"
                ),
            ));
        }
        Ok((Ir::Const(i64::from(id)), Ty::Enum(BTreeSet::from([id]))))
    }

    /// The interchangeable types: name, values and line.
    fn interchangeable(&mut self) -> Result<Vec<(String, Vec<u32>, usize)>> {
        let mut groups = Vec::new();
        for declaration in self.spec.types.iter().filter(|type_| type_.interchangeable) {
            let Some(Domain::Enum(ids)) = self.types.get(&declaration.name) else {
                return Err(Error::at(
                    declaration.line,
                    format!(
                        "only an enumeration can be interchangeable, and {} is not one",
                        declaration.name
                    ),
                ));
            };
            for id in ids {
                self.unnamed.insert(*id, declaration.name.clone());
            }
            groups.push((declaration.name.clone(), ids.clone(), declaration.line));
        }
        Ok(groups)
    }

    /// Relabeling one interchangeable value must relabel it everywhere, so an
    /// enumeration holds all of an interchangeable type's values or none.
    fn check_partial_groups(&self, groups: &[(String, Vec<u32>, usize)]) -> Result<()> {
        for (ids, line) in &self.enums {
            for (name, group, _) in groups {
                let shared = group.iter().filter(|id| ids.contains(id)).count();
                let other_group = groups
                    .iter()
                    .any(|(other, values, _)| other != name && values == ids);
                if shared == 0 || (shared == group.len() && !other_group) {
                    continue;
                }
                return Err(Error::at(
                    *line,
                    format!(
                        "this enumeration shares values with the interchangeable type {name}; \
                         list all of its values ({}) or none of them",
                        group
                            .iter()
                            .map(|id| self.symbols.get(at(*id)).map_or("?", String::as_str))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
            }
        }
        Ok(())
    }

    fn intern(&mut self, name: &str) -> u32 {
        if let Some(id) = self.symbol_ids.get(name) {
            return *id;
        }
        let id = index32(self.symbols.len());
        self.symbols.push(name.to_string());
        self.symbol_ids.insert(name.to_string(), id);
        id
    }

    fn scalar(&mut self, kind: &TypeExpr, line: usize) -> Result<Domain> {
        match self.resolve(kind, line)? {
            Resolved::Scalar(domain) => Ok(domain),
            Resolved::Map { .. } => Err(Error::at(line, "a map is only allowed as a state field")),
        }
    }

    fn resolve(&mut self, kind: &TypeExpr, line: usize) -> Result<Resolved> {
        Ok(Resolved::Scalar(match kind {
            TypeExpr::Integer { min, max } => Domain::Integer {
                min: *min,
                max: *max,
            },
            TypeExpr::Boolean => Domain::Boolean,
            TypeExpr::Enum { values } => {
                let mut ids = Vec::new();
                for value in values {
                    if value.is_empty() {
                        return Err(Error::at(line, "an enum value cannot be empty"));
                    }
                    let id = self.intern(value);
                    if ids.contains(&id) {
                        return Err(Error::at(line, format!("duplicate enum value {value}")));
                    }
                    ids.push(id);
                }
                self.enums.push((ids.clone(), line));
                Domain::Enum(ids)
            }
            TypeExpr::Named(name) => self.types.get(name).cloned().ok_or_else(|| {
                Error::at(
                    line,
                    format!("unknown type {name}; declare it in a types block"),
                )
            })?,
            TypeExpr::Map { key, value } => {
                let keys = self.types.get(key).cloned().ok_or_else(|| {
                    Error::at(
                        line,
                        format!("unknown key type {key}; declare it in a types block"),
                    )
                })?;
                if keys.size() > MAX_SLOTS {
                    return Err(Error::at(
                        line,
                        format!("a map may have at most {MAX_SLOTS} keys"),
                    ));
                }
                let value = self.scalar(value, line).map_err(|error| {
                    if error.message.starts_with("a map is only") {
                        Error::at(line, "map values must be integers, booleans, or enums")
                    } else {
                        error
                    }
                })?;
                return Ok(Resolved::Map {
                    key_type: key.clone(),
                    keys,
                    value,
                });
            }
        }))
    }

    fn declare_field(&mut self, name: &str, kind: &TypeExpr, line: usize) -> Result<()> {
        if self.field_ids.contains_key(name) {
            return Err(Error::at(line, format!("duplicate state field {name}")));
        }
        let offset = self.slots.len();
        let kind = match self.resolve(kind, line)? {
            Resolved::Scalar(domain) => {
                self.slots.push(Slot {
                    domain: domain.clone(),
                    label: name.to_string(),
                });
                FieldType::Scalar(domain)
            }
            Resolved::Map {
                key_type,
                keys,
                value,
            } => {
                for key in keys.raws() {
                    let key = self.raw_value(&keys, key);
                    self.slots.push(Slot {
                        domain: value.clone(),
                        label: format!("{name} of {key}"),
                    });
                }
                FieldType::Map {
                    key_type,
                    keys,
                    value,
                }
            }
        };
        if self.slots.len() > MAX_SLOTS {
            return Err(Error::at(
                line,
                format!("the state may hold at most {MAX_SLOTS} values"),
            ));
        }
        self.field_ids.insert(name.to_string(), self.fields.len());
        self.fields.push(FieldInfo {
            name: name.to_string(),
            kind,
            offset,
        });
        Ok(())
    }

    fn raw_value(&self, domain: &Domain, raw: i64) -> Value {
        match domain {
            Domain::Integer { .. } => Value::Integer(raw),
            Domain::Boolean => Value::Boolean(raw != 0),
            Domain::Enum(_) => Value::Symbol(
                usize::try_from(raw)
                    .ok()
                    .and_then(|index| self.symbols.get(index))
                    .cloned()
                    .unwrap_or_default(),
            ),
        }
    }

    fn signature(&mut self, action: &Action) -> Result<Vec<Param>> {
        if self
            .spec
            .actions
            .iter()
            .any(|other| other.name == action.name && other.line < action.line)
        {
            return Err(Error::at(
                action.line,
                format!("duplicate action {}", action.name),
            ));
        }
        let mut params: Vec<Param> = Vec::new();
        for param in &action.params {
            if params.iter().any(|other| other.name == param.name) {
                return Err(Error::at(
                    param.line,
                    format!("duplicate parameter {}", param.name),
                ));
            }
            if self.field_ids.contains_key(&param.name) {
                return Err(Error::at(
                    param.line,
                    format!(
                        "parameter {} has the same name as a state field",
                        param.name
                    ),
                ));
            }
            params.push(Param {
                name: param.name.clone(),
                domain: self.scalar(&param.kind, param.line)?,
            });
        }
        Ok(params)
    }

    fn initial_state(&mut self) -> Result<Box<[i64]>> {
        let spec = self.spec;
        self.constant_only = true;
        let mut assignments = Vec::new();
        for assignment in &spec.initially {
            assignments.push(self.assignment(assignment)?);
        }
        self.constant_only = false;
        check_duplicate_targets(&assignments)?;
        let mut locals = vec![0; self.locals.max(1)];
        let writes = collect_writes(&assignments, &[], &mut locals, &self.slots)
            .map_err(|fault| Error::at(fault.line, fault.message))?;
        let mut state: Vec<Option<i64>> = vec![None; self.slots.len()];
        for write in writes {
            let Some(slot) = self.slots.get(write.slot) else {
                continue;
            };
            if slot.domain.position(write.raw).is_none() {
                return Err(Error::at(
                    write.line,
                    format!(
                        "the initial value of {} is {}, but it must be {}",
                        slot.label,
                        self.raw_value(&slot.domain, write.raw),
                        self.describe(&slot.domain)
                    ),
                ));
            }
            if let Some(cell) = state.get_mut(write.slot) {
                *cell = Some(write.raw);
            }
        }
        let mut initial = Vec::with_capacity(state.len());
        for (cell, slot) in state.iter().zip(&self.slots) {
            initial.push(cell.ok_or_else(|| {
                Error::new(format!(
                    "{} has no initial value; set it in the initially block",
                    slot.label
                ))
            })?);
        }
        Ok(initial.into_boxed_slice())
    }

    fn describe(&self, domain: &Domain) -> String {
        match domain {
            Domain::Integer { min, max } => format!("an integer from {min} to {max}"),
            Domain::Boolean => "a boolean".to_string(),
            Domain::Enum(ids) => format!(
                "one of {}",
                ids.iter()
                    .map(|id| self.raw_value(domain, i64::from(*id)).source())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    fn action(&mut self, action: &Action, params: Vec<Param>) -> Result<ActionInfo> {
        self.scope = params
            .iter()
            .map(|param| (param.name.clone(), Ty::of(&param.domain)))
            .collect();
        self.locals = self.locals.max(self.scope.len());
        let mut requires = Vec::new();
        for condition in &action.requires {
            requires.push(self.condition(condition)?);
        }
        let mut assignments = Vec::new();
        for assignment in &action.assignments {
            assignments.push(self.assignment(assignment)?);
        }
        check_duplicate_targets(&assignments)?;
        self.scope.clear();
        Ok(ActionInfo {
            name: action.name.clone(),
            fairness: action.fairness,
            params,
            line: action.line,
            requires,
            assignments,
        })
    }

    fn condition(&mut self, condition: &Condition) -> Result<CompiledCondition> {
        let (ir, ty) = self.expr(&condition.expr, condition.line)?;
        if ty != Ty::Bool {
            return Err(Error::at(
                condition.line,
                format!(
                    "'{}' must be true or false, but it is {}",
                    condition.source,
                    ty.noun()
                ),
            )
            .diagnosed("type.mismatch", Some(&condition.source)));
        }
        Ok(CompiledCondition {
            ir,
            source: condition.source.clone(),
            line: condition.line,
        })
    }

    fn assignment(&mut self, assignment: &Assignment) -> Result<CompiledAssignment> {
        let line = assignment.line;
        let field = self
            .field_ids
            .get(&assignment.field)
            .and_then(|index| self.fields.get(*index))
            .cloned()
            .ok_or_else(|| Error::at(line, format!("unknown state field {}", assignment.field)))?;
        let (target, domain) = match (&field.kind, &assignment.key) {
            (FieldType::Scalar(domain), TargetKey::Whole) => {
                (Target::Slot(field.offset), domain.clone())
            }
            (FieldType::Scalar(_), _) => {
                return Err(Error::at(line, format!("{} is not a map", field.name)));
            }
            (FieldType::Map { key_type, .. }, TargetKey::Whole) => {
                return Err(Error::at(
                    line,
                    format!(
                        "{name} is a map; assign '{name} of <{key_type}>' or '{name} of every {key_type}'",
                        name = field.name
                    ),
                ));
            }
            (
                FieldType::Map {
                    key_type,
                    keys,
                    value,
                },
                TargetKey::Every(named),
            ) => {
                if named != key_type {
                    return Err(Error::at(
                        line,
                        format!("{} is keyed by {key_type}, not {named}", field.name),
                    ));
                }
                (
                    Target::Every {
                        offset: field.offset,
                        count: keys.size(),
                    },
                    value.clone(),
                )
            }
            (FieldType::Map { keys, value, .. }, TargetKey::At(key)) => {
                let key = self.key(key, keys, &field.name, line)?;
                let target = match key {
                    Ir::Const(raw) => Target::Slot(
                        key_slot(field.offset, keys, raw, &field.name)
                            .map_err(|message| Error::at(line, message))?,
                    ),
                    key => Target::Indexed {
                        offset: field.offset,
                        keys: keys.clone(),
                        key,
                        field: field.name.clone(),
                    },
                };
                (target, value.clone())
            }
        };
        let (value, ty) = self.expr(&assignment.value, line)?;
        if !assignable(&ty, &domain) {
            return Err(Error::at(
                line,
                format!(
                    "{} is {}, but '{}' assigns {}",
                    assignment.field,
                    self.describe(&domain),
                    assignment.source,
                    self.describe_ty(&ty)
                ),
            )
            .diagnosed("type.mismatch", Some(&assignment.field)));
        }
        Ok(CompiledAssignment {
            target,
            value,
            line,
        })
    }

    fn describe_ty(&self, ty: &Ty) -> String {
        match ty {
            Ty::Enum(ids) => ids
                .iter()
                .map(|id| {
                    self.symbols
                        .get(at(*id))
                        .map_or_else(String::new, |name| Value::Symbol(name.clone()).source())
                })
                .collect::<Vec<_>>()
                .join(" or "),
            other => other.noun().to_string(),
        }
    }

    fn key(&mut self, key: &Expr, keys: &Domain, field: &str, line: usize) -> Result<Ir> {
        let (ir, ty) = self.expr(key, line)?;
        if !assignable(&ty, keys) {
            return Err(Error::at(
                line,
                format!(
                    "{field} is keyed by {}, but the key here is {}",
                    self.describe(keys),
                    self.describe_ty(&ty)
                ),
            ));
        }
        Ok(ir)
    }

    fn expr(&mut self, expr: &Expr, line: usize) -> Result<(Ir, Ty)> {
        match expr {
            Expr::Literal(Value::Integer(raw)) => Ok((Ir::Const(*raw), Ty::Int)),
            Expr::Literal(Value::Boolean(flag)) => Ok((Ir::Const(i64::from(*flag)), Ty::Bool)),
            Expr::Literal(Value::Symbol(name)) => match self.symbol_ids.get(name).copied() {
                Some(id) => self.symbol(id, line),
                None => Err(Error::at(line, format!("unknown enum value \"{name}\""))),
            },
            Expr::Literal(Value::Map(_)) => Err(Error::at(line, "map literals are not supported")),
            Expr::Name(name) => self.name(name, line),
            Expr::Index { map, key } => self.index(map, key, line),
            Expr::Unary { operator, value } => {
                let (value, ty) = self.expr(value, line)?;
                match operator {
                    UnaryOperator::Not => {
                        expect(&ty, &Ty::Bool, "'not'", line)?;
                        Ok((Ir::Not(Box::new(value)), Ty::Bool))
                    }
                    UnaryOperator::Negate => {
                        expect(&ty, &Ty::Int, "'-'", line)?;
                        Ok((Ir::Negate(Box::new(value)), Ty::Int))
                    }
                }
            }
            Expr::Binary {
                operator,
                left,
                right,
            } => self.binary(*operator, left, right, line),
            Expr::If {
                condition,
                then,
                otherwise,
            } => {
                let (condition, ty) = self.expr(condition, line)?;
                expect(&ty, &Ty::Bool, "the condition of 'if'", line)?;
                let (then, then_ty) = self.expr(then, line)?;
                let (otherwise, otherwise_ty) = self.expr(otherwise, line)?;
                let ty = match (then_ty, otherwise_ty) {
                    (Ty::Enum(mut left), Ty::Enum(right)) => {
                        left.extend(right);
                        Ty::Enum(left)
                    }
                    (left, right) if left == right => left,
                    (left, right) => {
                        return Err(Error::at(
                            line,
                            format!(
                                "both branches of 'if' must have the same kind of value, but one is {} and the other {}",
                                left.noun(),
                                right.noun()
                            ),
                        ));
                    }
                };
                Ok((
                    Ir::If(Box::new(condition), Box::new(then), Box::new(otherwise)),
                    ty,
                ))
            }
            Expr::Quantified {
                quantifier,
                variable,
                domain,
                body,
            } => self.quantified(*quantifier, variable, domain, body, line),
        }
    }

    fn name(&mut self, name: &str, line: usize) -> Result<(Ir, Ty)> {
        if let Some(local) = self.scope.iter().rposition(|(local, _)| local == name) {
            let ty = self.scope.get(local).map_or(Ty::Int, |(_, ty)| ty.clone());
            return Ok((Ir::Local(local), ty));
        }
        if let Some(field) = self
            .field_ids
            .get(name)
            .and_then(|index| self.fields.get(*index))
        {
            if self.constant_only {
                return Err(Error::at(
                    line,
                    format!("initial values must be constants, but this reads {name}"),
                ));
            }
            return match &field.kind {
                FieldType::Scalar(domain) => Ok((Ir::Slot(field.offset), Ty::of(domain))),
                FieldType::Map { key_type, .. } => Err(Error::at(
                    line,
                    format!("{name} is a map; read one entry with '{name} of <{key_type}>'"),
                )),
            };
        }
        if let Some(id) = self.symbol_ids.get(name).copied() {
            return self.symbol(id, line);
        }
        Err(Error::at(
            line,
            format!("unknown name {name}; declare it or use a quoted enum value"),
        )
        .diagnosed("name.unknown", Some(name)))
    }

    fn index(&mut self, map: &str, key: &Expr, line: usize) -> Result<(Ir, Ty)> {
        let field = self
            .field_ids
            .get(map)
            .and_then(|index| self.fields.get(*index))
            .cloned()
            .ok_or_else(|| {
                Error::at(line, format!("unknown map {map}")).diagnosed("name.unknown", Some(map))
            })?;
        let FieldType::Map { keys, value, .. } = &field.kind else {
            return Err(Error::at(line, format!("{map} is not a map")));
        };
        if self.constant_only {
            return Err(Error::at(
                line,
                format!("initial values must be constants, but this reads {map}"),
            ));
        }
        let ir = match self.key(key, keys, map, line)? {
            Ir::Const(raw) => Ir::Slot(
                key_slot(field.offset, keys, raw, map)
                    .map_err(|message| Error::at(line, message))?,
            ),
            key => Ir::Index {
                offset: field.offset,
                keys: keys.clone(),
                key: Box::new(key),
                field: field.name.clone(),
            },
        };
        Ok((ir, Ty::of(value)))
    }

    fn binary(
        &mut self,
        operator: BinaryOperator,
        left: &Expr,
        right: &Expr,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        let (left, left_ty) = self.expr(left, line)?;
        let (right, right_ty) = self.expr(right, line)?;
        let ty = match operator {
            BinaryOperator::And | BinaryOperator::Or | BinaryOperator::Implies => {
                let word = match operator {
                    BinaryOperator::And => "'and'",
                    BinaryOperator::Or => "'or'",
                    _ => "'implies'",
                };
                expect(&left_ty, &Ty::Bool, word, line)?;
                expect(&right_ty, &Ty::Bool, word, line)?;
                Ty::Bool
            }
            BinaryOperator::Equal | BinaryOperator::NotEqual => {
                match (&left_ty, &right_ty) {
                    (Ty::Int, Ty::Int) | (Ty::Bool, Ty::Bool) => {}
                    (Ty::Enum(left), Ty::Enum(right)) => {
                        if left.is_disjoint(right) {
                            return Err(Error::at(
                                line,
                                format!(
                                    "this comparison can never be true: {} is never {}",
                                    self.describe_ty(&left_ty),
                                    self.describe_ty(&right_ty)
                                ),
                            ));
                        }
                    }
                    _ => {
                        return Err(Error::at(
                            line,
                            format!("cannot compare {} with {}", left_ty.noun(), right_ty.noun()),
                        )
                        .diagnosed("type.mismatch", None));
                    }
                }
                Ty::Bool
            }
            BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual => {
                expect(&left_ty, &Ty::Int, "an ordering comparison", line)?;
                expect(&right_ty, &Ty::Int, "an ordering comparison", line)?;
                Ty::Bool
            }
            BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide => {
                expect(&left_ty, &Ty::Int, "arithmetic", line)?;
                expect(&right_ty, &Ty::Int, "arithmetic", line)?;
                Ty::Int
            }
        };
        Ok((Ir::Binary(operator, Box::new(left), Box::new(right)), ty))
    }

    fn quantified(
        &mut self,
        quantifier: Quantifier,
        variable: &str,
        domain: &str,
        body: &Expr,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        let domain = self.types.get(domain).cloned().ok_or_else(|| {
            Error::at(
                line,
                format!("unknown type {domain}; declare it in a types block"),
            )
        })?;
        if domain.size() > MAX_QUANTIFIER_VALUES {
            return Err(Error::at(
                line,
                format!("a quantifier may range over at most {MAX_QUANTIFIER_VALUES} values"),
            ));
        }
        if self.field_ids.contains_key(variable) {
            return Err(Error::at(
                line,
                format!("{variable} has the same name as a state field"),
            ));
        }
        let local = self.scope.len();
        self.scope.push((variable.to_string(), Ty::of(&domain)));
        self.locals = self.locals.max(self.scope.len());
        let body = self.expr(body, line);
        self.scope.pop();
        let (body, ty) = body?;
        expect(&ty, &Ty::Bool, "the condition of a quantifier", line)?;
        Ok((
            Ir::Quantified {
                quantifier,
                local,
                values: domain.raws(),
                body: Box::new(body),
            },
            if quantifier == Quantifier::Count {
                Ty::Int
            } else {
                Ty::Bool
            },
        ))
    }
}

fn expect(actual: &Ty, wanted: &Ty, what: &str, line: usize) -> Result<()> {
    if actual == wanted {
        Ok(())
    } else {
        Err(Error::at(
            line,
            format!("{what} needs {}, found {}", wanted.noun(), actual.noun()),
        )
        .diagnosed("type.mismatch", None))
    }
}

fn assignable(ty: &Ty, domain: &Domain) -> bool {
    match (ty, domain) {
        (Ty::Int, Domain::Integer { .. }) | (Ty::Bool, Domain::Boolean) => true,
        (Ty::Enum(ids), Domain::Enum(allowed)) => ids.iter().all(|id| allowed.contains(id)),
        _ => false,
    }
}

fn check_duplicate_targets(assignments: &[CompiledAssignment]) -> Result<()> {
    let mut slots = BTreeSet::new();
    let mut broad = BTreeSet::new();
    for assignment in assignments {
        let fresh = match &assignment.target {
            Target::Slot(slot) => slots.insert(*slot),
            Target::Every { offset, .. } => broad.insert(*offset),
            Target::Indexed { .. } => true,
        };
        if !fresh {
            return Err(Error::at(
                assignment.line,
                "this value is already assigned in the same block",
            ));
        }
    }
    Ok(())
}

type Calls = (Vec<Call>, HashMap<Call, u32>);

fn enumerate_calls(actions: &[ActionInfo]) -> Result<Calls> {
    let mut calls = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        let mut combinations: usize = 1;
        for param in &action.params {
            combinations = combinations.saturating_mul(param.domain.size());
        }
        if combinations > MAX_ARGUMENT_COMBINATIONS {
            return Err(Error::of(
                crate::ErrorKind::Limit,
                Some(action.line),
                format!(
                    "action {} has {combinations} argument combinations; the limit is {MAX_ARGUMENT_COMBINATIONS}",
                    action.name
                ),
            ));
        }
        let mut pending = vec![Vec::new()];
        for param in &action.params {
            let values = param.domain.raws();
            pending = pending
                .into_iter()
                .flat_map(|prefix: Vec<i64>| {
                    values.iter().map(move |value| {
                        let mut args = prefix.clone();
                        args.push(*value);
                        args
                    })
                })
                .collect();
        }
        calls.extend(pending.into_iter().map(|args| Call {
            action: index,
            args,
        }));
        if calls.len() > MAX_CALLS {
            return Err(Error::of(
                crate::ErrorKind::Limit,
                Some(action.line),
                format!("the specification has more than {MAX_CALLS} distinct action calls"),
            ));
        }
    }
    let ids = calls
        .iter()
        .enumerate()
        .map(|(index, call)| (call.clone(), index32(index)))
        .collect();
    Ok((calls, ids))
}

pub(crate) fn resolve_call(model: &Model, call: &ActionCall) -> Result<u32> {
    let line = call.line;
    let index = model
        .action_index(&call.name)
        .ok_or_else(|| Error::at(line, format!("unknown action {}", call.name)))?;
    let action = model
        .actions
        .get(index)
        .ok_or_else(|| Error::at(line, "unknown action"))?;
    let values: Vec<&Value> = match &call.arguments {
        CallArguments::Positional(values) => {
            if values.len() != action.params.len() {
                return Err(Error::at(
                    line,
                    format!(
                        "{} takes {} argument(s), but {} were given",
                        action.name,
                        action.params.len(),
                        values.len()
                    ),
                ));
            }
            values.iter().collect()
        }
        CallArguments::Named(named) => {
            if let Some(unknown) = named
                .keys()
                .find(|name| !action.params.iter().any(|param| &param.name == *name))
            {
                return Err(Error::at(
                    line,
                    format!("{} has no parameter {unknown}", action.name),
                ));
            }
            let mut values = Vec::new();
            for param in &action.params {
                values.push(named.get(&param.name).ok_or_else(|| {
                    Error::at(
                        line,
                        format!("{} is missing argument {}", action.name, param.name),
                    )
                })?);
            }
            values
        }
    };
    let mut args = Vec::new();
    for (param, value) in action.params.iter().zip(values) {
        args.push(model.encode(&param.domain, value).ok_or_else(|| {
            Error::at(
                line,
                format!(
                    "argument {} of {} must be {}, but {} was given",
                    param.name,
                    action.name,
                    model.describe(&param.domain),
                    value.source()
                ),
            )
        })?);
    }
    model
        .call_ids
        .get(&Call {
            action: index,
            args,
        })
        .copied()
        .ok_or_else(|| Error::at(line, "internal error: call is not enumerated"))
}

pub(crate) fn named_values(model: &Model, call: u32) -> BTreeMap<String, Value> {
    let values = model.call_values(call);
    model
        .calls
        .get(at(call))
        .and_then(|call| model.actions.get(call.action))
        .map(|action| {
            action
                .params
                .iter()
                .map(|param| param.name.clone())
                .zip(values)
                .collect()
        })
        .unwrap_or_default()
}
