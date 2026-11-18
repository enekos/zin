use crate::ast::PropertyKind;
use crate::model::{Fault, Model, at, index32};
use crate::{Error, ErrorKind, Result, Trace, TraceStep};
use std::collections::HashMap;

const MAX_TRANSITIONS: usize = 20_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub call: u32,
    pub to: u32,
}

#[derive(Debug, Clone)]
pub struct Graph {
    pub(crate) states: Vec<Box<[i64]>>,
    pub(crate) parents: Vec<Option<(u32, u32)>>,
    pub(crate) edges: Vec<Edge>,
    pub(crate) first_edge: Vec<usize>,
    pub(crate) index: HashMap<Box<[i64]>, u32>,
}

impl Graph {
    #[must_use]
    pub fn state_count(&self) -> usize {
        self.states.len()
    }

    #[must_use]
    pub fn transition_count(&self) -> usize {
        self.edges.len()
    }

    #[must_use]
    pub fn out(&self, state: u32) -> &[Edge] {
        let state = at(state);
        let start = self.first_edge.get(state).copied().unwrap_or(0);
        let end = self.first_edge.get(state + 1).copied().unwrap_or(start);
        self.edges.get(start..end).unwrap_or(&[])
    }

    #[must_use]
    pub fn target(&self, state: u32, call: u32) -> Option<u32> {
        self.out(state)
            .iter()
            .find(|edge| edge.call == call)
            .map(|edge| edge.to)
    }

    pub(crate) fn state(&self, state: u32) -> &[i64] {
        self.states.get(at(state)).map_or(&[], |state| state)
    }

    pub(crate) fn path_to(&self, state: u32) -> Vec<Edge> {
        path_to(&self.parents, state)
    }

    pub(crate) fn trace(&self, model: &Model, path: &[Edge], loop_from: Option<usize>) -> Trace {
        trace(model, &self.states, path, loop_from)
    }

    pub(crate) fn trace_to(&self, model: &Model, state: u32) -> Trace {
        self.trace(model, &self.path_to(state), None)
    }

    pub(crate) fn predecessors(&self) -> Vec<Vec<u32>> {
        let mut predecessors = vec![Vec::new(); self.states.len()];
        for from in 0..self.states.len() {
            let from = index32(from);
            for edge in self.out(from) {
                if let Some(list) = predecessors.get_mut(at(edge.to)) {
                    list.push(from);
                }
            }
        }
        predecessors
    }
}

fn path_to(parents: &[Option<(u32, u32)>], state: u32) -> Vec<Edge> {
    let mut path = Vec::new();
    let mut current = state;
    while let Some(Some((from, call))) = parents.get(at(current)) {
        path.push(Edge {
            call: *call,
            to: current,
        });
        current = *from;
    }
    path.reverse();
    path
}

fn trace(model: &Model, states: &[Box<[i64]>], path: &[Edge], loop_from: Option<usize>) -> Trace {
    let decode = |state: u32| model.decode(states.get(at(state)).map_or(&[], |state| state));
    Trace {
        initial: decode(0),
        steps: path
            .iter()
            .map(|edge| TraceStep {
                call: model.call_text(edge.call),
                state: decode(edge.to),
            })
            .collect(),
        loop_from,
    }
}

struct Explorer<'a> {
    model: &'a Model,
    graph: Graph,
    locals: Vec<i64>,
}

impl Explorer<'_> {
    fn fail(&self, kind: ErrorKind, line: usize, message: String, state: u32) -> Error {
        Error::of(kind, Some(line).filter(|line| *line > 0), message).with_trace(trace(
            self.model,
            &self.graph.states,
            &path_to(&self.graph.parents, state),
            None,
        ))
    }

    fn fault(&self, fault: Fault, state: u32) -> Error {
        self.fail(ErrorKind::Model, fault.line, fault.message, state)
    }

    fn check_safety(&mut self, state: u32) -> Result<()> {
        let model = self.model;
        for property in &model.properties {
            let wanted = match property.kind {
                PropertyKind::Always => true,
                PropertyKind::Never => false,
                _ => continue,
            };
            let values = self.graph.states.get(at(state)).map_or(&[][..], |s| s);
            let holds = model
                .holds(&property.condition, values, &mut self.locals)
                .map_err(|fault| self.fault(fault, state))?;
            if holds != wanted {
                return Err(self.fail(
                    ErrorKind::Violation,
                    property.line,
                    format!("'{}' is violated", property.text),
                    state,
                ));
            }
        }
        Ok(())
    }

    fn add(&mut self, values: Box<[i64]>, parent: Option<(u32, u32)>) -> Result<u32> {
        if let Some(existing) = self.graph.index.get(&values) {
            return Ok(*existing);
        }
        if self.graph.states.len() >= self.model.state_limit {
            return Err(Error::of(
                ErrorKind::Limit,
                None,
                format!(
                    "more than {} states are reachable; tighten the ranges or raise the limit with 'explore at most N states'",
                    self.model.state_limit
                ),
            ));
        }
        let id = index32(self.graph.states.len());
        self.graph.index.insert(values.clone(), id);
        self.graph.states.push(values);
        self.graph.parents.push(parent);
        self.check_safety(id)?;
        Ok(id)
    }
}

pub(crate) fn explore(model: &Model) -> Result<Graph> {
    let mut explorer = Explorer {
        model,
        graph: Graph {
            states: Vec::new(),
            parents: Vec::new(),
            edges: Vec::new(),
            first_edge: Vec::new(),
            index: HashMap::new(),
        },
        locals: model.new_locals(),
    };
    explorer.add(model.initial.clone(), None)?;
    let calls = index32(model.calls.len());
    let mut cursor = 0;
    while cursor < explorer.graph.states.len() {
        let from = index32(cursor);
        explorer.graph.first_edge.push(explorer.graph.edges.len());
        let Some(current) = explorer.graph.states.get(cursor).cloned() else {
            break;
        };
        for call in 0..calls {
            let next = match model.step(call, &current, &mut explorer.locals) {
                Ok(Some(next)) => next,
                Ok(None) => continue,
                Err(fault) => {
                    let message = format!("{}: {}", model.call_text(call), fault.message);
                    return Err(explorer.fail(ErrorKind::Model, fault.line, message, from));
                }
            };
            if explorer.graph.edges.len() >= MAX_TRANSITIONS {
                return Err(Error::of(
                    ErrorKind::Limit,
                    None,
                    format!("more than {MAX_TRANSITIONS} transitions are reachable"),
                ));
            }
            let to = explorer.add(next, Some((from, call)))?;
            explorer.graph.edges.push(Edge { call, to });
        }
        cursor += 1;
    }
    explorer.graph.first_edge.push(explorer.graph.edges.len());
    Ok(explorer.graph)
}
