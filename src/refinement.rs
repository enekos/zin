//! Finite deterministic observational refinement over two verified models.
use crate::ast::{ActionCall, CallArguments};
use crate::model::{at, index32, named_values, resolve_call};
use crate::{Error, ErrorKind, Result, Spec, Trace, TraceStep, Verification, verify};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone)]
pub struct RefinementOptions {
    /// Abstract to concrete names. Unlisted names match by identity.
    pub fields: BTreeMap<String, String>,
    pub actions: BTreeMap<String, String>,
    pub max_pairs: usize,
}
impl Default for RefinementOptions {
    fn default() -> Self {
        Self {
            fields: BTreeMap::new(),
            actions: BTreeMap::new(),
            max_pairs: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refinement {
    pub state_pairs: usize,
}

struct Visit {
    state: (u32, u32),
    parent: Option<(usize, u32)>,
}

fn mismatch(concrete: &Verification, seen: &[Visit], mut here: usize, message: String) -> Error {
    let mut steps = Vec::new();
    while let Some((parent, call)) = seen[here].parent {
        steps.push(TraceStep {
            call: concrete.call_text(call),
            state: concrete.state(seen[here].state.1),
        });
        here = parent;
    }
    steps.reverse();
    Error::of(ErrorKind::Violation, None, message)
        .diagnosed("refinement.mismatch", None)
        .with_trace(Trace {
            initial: concrete.state(0),
            steps,
            loop_from: None,
        })
}

fn correspondence<'a>(
    kind: &str,
    abstract_names: impl Iterator<Item = &'a str>,
    concrete_names: impl Iterator<Item = &'a str>,
    mapping: &'a BTreeMap<String, String>,
) -> Result<BTreeMap<&'a str, &'a str>> {
    let available: HashSet<_> = concrete_names.collect();
    let mut used = HashSet::new();
    let mut result = BTreeMap::new();
    for name in abstract_names {
        let wanted = mapping.get(name).map_or(name, String::as_str);
        if !available.contains(wanted) {
            return Err(Error::new(format!(
                "abstract {kind} {name} has no matching concrete {kind} {wanted}"
            )));
        }
        if !used.insert(wanted) {
            return Err(Error::new(format!(
                "several abstract {kind}s map to {wanted}"
            )));
        }
        result.insert(name, wanted);
    }
    if let Some(key) = mapping
        .keys()
        .find(|key| !result.contains_key(key.as_str()))
    {
        return Err(Error::new(format!(
            "unknown abstract {kind} in mapping: {key}"
        )));
    }
    Ok(result)
}

/// Verify both specs, then compare reachable pairs breadth first. Each visible
/// call must agree on acceptance and refusal; concrete-only calls are private
/// and must preserve the abstract observations. Liveness is checked on each
/// input independently.
pub fn refine(
    abstract_spec: &Spec,
    concrete_spec: &Spec,
    options: &RefinementOptions,
) -> Result<Refinement> {
    if options.max_pairs == 0 {
        return Err(Error::usage("refinement requires a positive pair limit"));
    }
    let contract = verify(abstract_spec)?;
    let concrete = verify(concrete_spec)?;
    let fields = correspondence(
        "field",
        contract.model.fields.iter().map(|f| f.name.as_str()),
        concrete.model.fields.iter().map(|f| f.name.as_str()),
        &options.fields,
    )?;
    let actions = correspondence(
        "action",
        contract.model.actions.iter().map(|a| a.name.as_str()),
        concrete.model.actions.iter().map(|a| a.name.as_str()),
        &options.actions,
    )?;
    let mut forward = Vec::with_capacity(contract.model.call_count());
    let mut reverse = vec![None; concrete.model.call_count()];
    for i in 0..contract.model.call_count() {
        let i = index32(i);
        let name = actions[contract.model.call_action(i)];
        let call = resolve_call(
            &concrete.model,
            &ActionCall {
                name: name.into(),
                line: 0,
                arguments: CallArguments::Named(named_values(&contract.model, i)),
            },
        )
        .map_err(|_| {
            Error::new(format!(
                "{} has no matching call in concrete action {name}",
                contract.call_text(i)
            ))
        })?;
        forward.push(call);
        reverse[at(call)] = Some(i);
    }
    let visible: HashSet<_> = actions.values().copied().collect();
    for (i, mapped) in reverse.iter().enumerate() {
        if mapped.is_none() && visible.contains(concrete.model.call_action(index32(i))) {
            return Err(Error::new(format!(
                "concrete call {} is outside the abstract action signature",
                concrete.call_text(index32(i))
            )));
        }
    }

    let mut seen = vec![Visit {
        state: (0, 0),
        parent: None,
    }];
    let mut visited = HashSet::from([(0, 0)]);
    let mut here = 0;
    while here < seen.len() {
        let (a, b) = seen[here].state;
        let expected = contract.state(a);
        let actual = concrete.state(b);
        for (abstract_name, concrete_name) in &fields {
            if expected[*abstract_name] != actual[*concrete_name] {
                return Err(mismatch(
                    &concrete,
                    &seen,
                    here,
                    format!(
                        "concrete {concrete_name} is {}, but abstract {abstract_name} is {}",
                        actual[*concrete_name], expected[*abstract_name]
                    ),
                ));
            }
        }
        let abstract_edges = contract.graph.out(a);
        let concrete_edges = concrete.graph.out(b);
        for edge in abstract_edges {
            let call = forward[at(edge.call)];
            if concrete_edges
                .binary_search_by_key(&call, |e| e.call)
                .is_err()
            {
                return Err(mismatch(
                    &concrete,
                    &seen,
                    here,
                    format!(
                        "concrete rejects {}, but abstract accepts {}",
                        concrete.call_text(call),
                        contract.call_text(edge.call)
                    ),
                ));
            }
        }
        for edge in concrete_edges {
            let next_a = match reverse[at(edge.call)] {
                None => a,
                Some(call) => abstract_edges
                    .binary_search_by_key(&call, |e| e.call)
                    .ok()
                    .map(|i| abstract_edges[i].to)
                    .ok_or_else(|| {
                        mismatch(
                            &concrete,
                            &seen,
                            here,
                            format!(
                                "concrete accepts {}, but abstract rejects {}",
                                concrete.call_text(edge.call),
                                contract.call_text(call)
                            ),
                        )
                    })?,
            };
            let pair = (next_a, edge.to);
            if visited.insert(pair) {
                if seen.len() >= options.max_pairs {
                    return Err(Error::of(
                        ErrorKind::Limit,
                        None,
                        format!("refinement exceeds {} state pairs", options.max_pairs),
                    ));
                }
                seen.push(Visit {
                    state: pair,
                    parent: Some((here, edge.call)),
                });
            }
        }
        here += 1;
    }
    Ok(Refinement {
        state_pairs: seen.len(),
    })
}
