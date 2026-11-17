use crate::ast::{BinaryOperator, Expr, Field, FieldKind, Spec, UnaryOperator, Value};
use crate::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Type {
    Integer,
    Boolean,
    Enum(BTreeSet<String>),
}

impl Type {
    fn field(field: &Field) -> Self {
        match &field.kind {
            FieldKind::Integer { .. } => Self::Integer,
            FieldKind::Boolean => Self::Boolean,
            FieldKind::Enum { values } => Self::Enum(values.iter().cloned().collect()),
        }
    }

    fn literal(value: &Value) -> Self {
        match value {
            Value::Integer(_) => Self::Integer,
            Value::Boolean(_) => Self::Boolean,
            Value::Text(value) => Self::Enum(BTreeSet::from([value.clone()])),
        }
    }

    fn description(&self) -> String {
        match self {
            Self::Integer => "an integer".to_string(),
            Self::Boolean => "a boolean".to_string(),
            Self::Enum(values) => {
                format!(
                    "enum {{{}}}",
                    values.iter().cloned().collect::<Vec<_>>().join(", ")
                )
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Scope {
    variables: BTreeMap<String, Type>,
    enum_values: BTreeMap<String, Type>,
}

impl Scope {
    fn new(fields: &[Field]) -> Self {
        let mut scope = Self {
            variables: BTreeMap::new(),
            enum_values: BTreeMap::new(),
        };
        for field in fields {
            scope.add_field(field);
        }
        scope
    }

    fn add_field(&mut self, field: &Field) {
        self.variables
            .insert(field.name.clone(), Type::field(field));
        if let FieldKind::Enum { values } = &field.kind {
            for value in values {
                self.enum_values
                    .insert(value.clone(), Type::literal(&Value::Text(value.clone())));
            }
        }
    }

    fn without_variables(&self) -> Self {
        Self {
            variables: BTreeMap::new(),
            enum_values: self.enum_values.clone(),
        }
    }
}

pub(crate) fn validate_types(spec: &Spec) -> Result<()> {
    let scope = Scope::new(&spec.state);
    let initial_scope = scope.without_variables();
    for assignment in &spec.initially {
        let field = spec
            .state
            .iter()
            .find(|field| field.name == assignment.field)
            .ok_or_else(|| Error::at(assignment.line, "unknown initial field"))?;
        let actual = infer(&assignment.value, &initial_scope, assignment.line)?;
        ensure_assignable(
            &Type::field(field),
            &actual,
            &assignment.field,
            assignment.line,
        )?;
    }
    for promise in &spec.invariants {
        expect_boolean(&promise.expr, &scope, &promise.source, promise.line)?;
    }
    for action in &spec.actions {
        let mut action_scope = scope.clone();
        for parameter in &action.params {
            action_scope.add_field(parameter);
        }
        for condition in &action.requires {
            expect_boolean(
                &condition.expr,
                &action_scope,
                &condition.source,
                condition.line,
            )?;
        }
        for assignment in &action.assignments {
            let field = spec
                .state
                .iter()
                .find(|field| field.name == assignment.field)
                .ok_or_else(|| Error::at(assignment.line, "unknown assignment field"))?;
            let actual = infer(&assignment.value, &action_scope, assignment.line)?;
            ensure_assignable(
                &Type::field(field),
                &actual,
                &assignment.field,
                assignment.line,
            )?;
        }
    }
    for scenario in &spec.scenarios {
        for step in &scenario.steps {
            if let crate::ast::ScenarioStep::Expect(condition) = step {
                expect_boolean(&condition.expr, &scope, &condition.source, condition.line)?;
            }
        }
    }
    Ok(())
}

fn infer(expression: &Expr, scope: &Scope, line: usize) -> Result<Type> {
    match expression {
        Expr::Literal(value) => Ok(Type::literal(value)),
        Expr::Name(name) => scope
            .variables
            .get(name)
            .or_else(|| scope.enum_values.get(name))
            .cloned()
            .ok_or_else(|| Error::at(line, format!("unknown name {name}"))),
        Expr::Unary {
            operator: UnaryOperator::Not,
            value,
        } => expect_boolean(value, scope, "not", line),
        Expr::Unary {
            operator: UnaryOperator::Negate,
            value,
        } => {
            expect_integer(value, scope, line)?;
            Ok(Type::Integer)
        }
        Expr::Binary {
            operator: BinaryOperator::And | BinaryOperator::Or | BinaryOperator::Implies,
            left,
            right,
        } => {
            expect_boolean(left, scope, "logical operator", line)?;
            expect_boolean(right, scope, "logical operator", line)
        }
        Expr::Binary {
            operator: BinaryOperator::Equal | BinaryOperator::NotEqual,
            left,
            right,
        } => {
            let left = infer(left, scope, line)?;
            let right = infer(right, scope, line)?;
            if comparable(&left, &right) {
                Ok(Type::Boolean)
            } else {
                Err(Error::at(
                    line,
                    format!(
                        "cannot compare {} with {}",
                        left.description(),
                        right.description()
                    ),
                ))
            }
        }
        Expr::Binary {
            operator,
            left,
            right,
        } => {
            let left_type = infer(left, scope, line)?;
            let right_type = infer(right, scope, line)?;
            match operator {
                BinaryOperator::Less
                | BinaryOperator::LessEqual
                | BinaryOperator::Greater
                | BinaryOperator::GreaterEqual => {
                    ensure_integer(&left_type, line)?;
                    ensure_integer(&right_type, line)?;
                    Ok(Type::Boolean)
                }
                BinaryOperator::Add
                | BinaryOperator::Subtract
                | BinaryOperator::Multiply
                | BinaryOperator::Divide => {
                    ensure_integer(&left_type, line)?;
                    ensure_integer(&right_type, line)?;
                    Ok(Type::Integer)
                }
                BinaryOperator::And
                | BinaryOperator::Or
                | BinaryOperator::Implies
                | BinaryOperator::Equal
                | BinaryOperator::NotEqual => {
                    Err(Error::at(line, "internal logical operator typing error"))
                }
            }
        }
    }
}

fn expect_boolean(expression: &Expr, scope: &Scope, source: &str, line: usize) -> Result<Type> {
    let actual = infer(expression, scope, line)?;
    if actual == Type::Boolean {
        Ok(Type::Boolean)
    } else {
        Err(Error::at(
            line,
            format!("'{source}' must be boolean, found {}", actual.description()),
        ))
    }
}

fn expect_integer(expression: &Expr, scope: &Scope, line: usize) -> Result<Type> {
    let actual = infer(expression, scope, line)?;
    ensure_integer(&actual, line)?;
    Ok(Type::Integer)
}

fn ensure_integer(actual: &Type, line: usize) -> Result<()> {
    if *actual == Type::Integer {
        Ok(())
    } else {
        Err(Error::at(
            line,
            format!("expected an integer, found {}", actual.description()),
        ))
    }
}

fn comparable(left: &Type, right: &Type) -> bool {
    match (left, right) {
        (Type::Integer, Type::Integer) | (Type::Boolean, Type::Boolean) => true,
        (Type::Enum(left), Type::Enum(right)) => !left.is_disjoint(right),
        _ => false,
    }
}

fn ensure_assignable(expected: &Type, actual: &Type, field: &str, line: usize) -> Result<()> {
    let assignable = match (expected, actual) {
        (Type::Integer, Type::Integer) | (Type::Boolean, Type::Boolean) => true,
        (Type::Enum(expected), Type::Enum(actual)) => actual.is_subset(expected),
        _ => false,
    };
    if assignable {
        Ok(())
    } else {
        Err(Error::at(
            line,
            format!(
                "cannot assign {} to {field}, which expects {}",
                actual.description(),
                expected.description()
            ),
        ))
    }
}
