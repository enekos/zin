//! Composition: several `system` blocks in one file, checked as one system.
//!
//! Components share no state. An action declared in several components is
//! one synchronized step: it is enabled when every component's `requires`
//! holds, and it applies every component's assignments at once. Parameters
//! with the same name are the same value, so a shared action passes data
//! between components. An action declared in one component interleaves with
//! everything else.

use crate::ast::{Action, BinaryOperator, Condition, Expr, Field, Span, Spec, TargetKey, TypeDecl};
use crate::{Error, Result};
use std::collections::{HashMap, HashSet};

/// Merges parsed components into one specification.
pub(crate) fn compose(components: Vec<Spec>) -> Result<Spec> {
    let owners = owners(&components)?;
    for component in &components {
        for action in &component.actions {
            confine(action, &component.name, &owners)?;
        }
        for assignment in &component.initially {
            if let Some((owner, _)) = owners
                .get(assignment.field.as_str())
                .filter(|(owner, _)| *owner != component.name)
            {
                return Err(Error::at(
                    assignment.line,
                    format!(
                        "{} sets the initial value of {}, which belongs to {owner}",
                        component.name, assignment.field
                    ),
                )
                .coded("component.foreign_field")
                .spanned(assignment.target));
            }
        }
    }
    if owners.is_empty() {
        return Err(Error::new("declare at least one field in a state block"));
    }
    drop(owners);
    let mut spec = Spec::empty();
    let observers: Vec<&Spec> = components
        .iter()
        .filter(|component| component.state.is_empty())
        .collect();
    spec.name = match observers.as_slice() {
        [observer] => observer.name.clone(),
        _ => components
            .iter()
            .map(|component| component.name.as_str())
            .collect::<Vec<_>>()
            .join(" + "),
    };
    spec.components = components
        .iter()
        .map(|component| component.name.clone())
        .collect();
    let mut limits = Vec::new();
    let mut terminal: Vec<Condition> = Vec::new();
    for component in components {
        if component.state_limit != crate::ast::DEFAULT_STATE_LIMIT {
            limits.push(component.state_limit);
        }
        for declaration in component.types {
            merge_type(&mut spec.types, declaration, &component.name)?;
        }
        spec.state.extend(component.state);
        spec.initially.extend(component.initially);
        spec.properties.extend(component.properties);
        terminal.extend(component.terminal);
        for action in component.actions {
            merge_action(&mut spec.actions, action, &component.name)?;
        }
        spec.scenarios.extend(component.scenarios);
    }
    if let Some(limit) = limits.into_iter().max() {
        spec.state_limit = limit;
    }
    spec.terminal = terminal.into_iter().reduce(|left, right| Condition {
        source: format!("({}) and ({})", left.source, right.source),
        line: left.line,
        span: left.span,
        expr: Expr::Binary {
            operator: BinaryOperator::And,
            left: Box::new(left.expr),
            right: Box::new(right.expr),
        },
    });
    Ok(spec)
}

/// Which component declares each state field, and where.
fn owners(components: &[Spec]) -> Result<HashMap<&str, (&str, Span)>> {
    let mut owners: HashMap<&str, (&str, Span)> = HashMap::new();
    let mut names = HashSet::new();
    for component in components {
        if !names.insert(component.name.as_str()) {
            return Err(Error::new(format!(
                "two components are both called \"{}\"",
                component.name
            )));
        }
        for field in &component.state {
            if let Some((owner, first)) = owners.get(field.name.as_str()) {
                return Err(Error::at(
                    field.line,
                    format!(
                        "{} is already a state field of {owner}; components share no state, \
                         so rename one and pass values through a shared action",
                        field.name
                    ),
                )
                .coded("component.field_conflict")
                .spanned(field.span)
                .related(*first));
            }
            owners.insert(&field.name, (&component.name, field.span));
        }
        let mut seen: HashMap<&str, Span> = HashMap::new();
        for action in &component.actions {
            if let Some(first) = seen.insert(&action.name, action.span) {
                return Err(Error::at(
                    action.line,
                    format!("duplicate action {} in {}", action.name, component.name),
                )
                .coded("component.duplicate_action")
                .spanned(action.span)
                .related(first));
            }
        }
    }
    Ok(owners)
}

/// An action reads and writes only its own component's state.
fn confine(action: &Action, component: &str, owners: &HashMap<&str, (&str, Span)>) -> Result<()> {
    let foreign = |name: &str| {
        owners
            .get(name)
            .filter(|(owner, _)| *owner != component)
            .map(|(owner, _)| *owner)
    };
    let fail = |span: Span, field: &str, owner: &str, verb: &str| {
        Error::at(
            span.line,
            format!(
                "action {} of {component} {verb} {field}, which belongs to {owner}; a component \
                 reaches another only through shared actions",
                action.name
            ),
        )
        .coded("component.foreign_field")
        .spanned(span)
    };
    for condition in &action.requires {
        if let Some((field, owner, span)) = foreign_name(&condition.expr, &foreign) {
            return Err(fail(span, &field, owner, "reads"));
        }
    }
    for assignment in &action.assignments {
        if let Some(owner) = foreign(&assignment.field) {
            return Err(fail(assignment.target, &assignment.field, owner, "assigns"));
        }
        let key = match &assignment.key {
            TargetKey::At(key) => foreign_name(key, &foreign),
            TargetKey::Whole | TargetKey::Every(_) => None,
        };
        if let Some((field, owner, span)) =
            key.or_else(|| foreign_name(&assignment.value, &foreign))
        {
            return Err(fail(span, &field, owner, "reads"));
        }
    }
    Ok(())
}

/// The first name in `expr` that another component owns.
fn foreign_name<'a>(
    expr: &Expr,
    foreign: &impl Fn(&str) -> Option<&'a str>,
) -> Option<(String, &'a str, Span)> {
    let mut pending = vec![expr];
    while let Some(expr) = pending.pop() {
        match expr {
            Expr::Literal(_) => {}
            Expr::Name { name, span } => {
                if let Some(owner) = foreign(name) {
                    return Some((name.clone(), owner, *span));
                }
            }
            Expr::Index { map, key, span } => {
                if let Some(owner) = foreign(map) {
                    return Some((map.clone(), owner, *span));
                }
                pending.push(key);
            }
            Expr::Unary { value, .. } => pending.push(value),
            Expr::Binary { left, right, .. } => {
                pending.push(left);
                pending.push(right);
            }
            Expr::If {
                condition,
                then,
                otherwise,
            } => {
                pending.push(condition);
                pending.push(then);
                pending.push(otherwise);
            }
            Expr::Quantified { body, .. } => pending.push(body),
        }
    }
    None
}

fn merge_type(types: &mut Vec<TypeDecl>, declaration: TypeDecl, component: &str) -> Result<()> {
    match types.iter().find(|other| other.name == declaration.name) {
        None => types.push(declaration),
        Some(other)
            if other.kind == declaration.kind
                && other.interchangeable == declaration.interchangeable => {}
        Some(other) => {
            return Err(Error::at(
                declaration.line,
                format!(
                    "{component} declares type {} differently; components that share a type \
                     must declare it the same way",
                    declaration.name
                ),
            )
            .coded("component.type_conflict")
            .spanned(declaration.span)
            .related(other.span));
        }
    }
    Ok(())
}

fn merge_action(actions: &mut Vec<Action>, action: Action, component: &str) -> Result<()> {
    let Some(shared) = actions.iter_mut().find(|other| other.name == action.name) else {
        actions.push(action);
        return Ok(());
    };
    for param in action.params {
        match shared.params.iter().find(|other| other.name == param.name) {
            None => shared.params.push(param),
            Some(Field { kind, .. }) if *kind == param.kind => {}
            Some(other) => {
                return Err(Error::at(
                    param.line,
                    format!(
                        "{component} gives parameter {} of the shared action {} the type {}, \
                         but another component gives it {}",
                        param.name, action.name, param.kind, other.kind
                    ),
                )
                .coded("component.parameter_conflict")
                .spanned(param.span)
                .related(other.span));
            }
        }
    }
    shared.fairness = shared.fairness.max(action.fairness);
    shared.requires.extend(action.requires);
    shared.assignments.extend(action.assignments);
    Ok(())
}
