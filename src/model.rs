use crate::ast::{
    Action, ActionCall, Assignment, BinaryOperator, CallArguments, Condition, Expr, Fairness,
    PropertyKind, Quantifier, ScenarioStep, Spec, TypeDecl, TypeExpr, UnaryOperator, Value,
    display_call,
};
use crate::explain::{self, Code};
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
        value: Box<FieldType>,
    },
    /// Named members, laid out one after another in declaration order.
    Record(Vec<(String, FieldType)>),
    /// A presence slot (0 for nothing, 1 for a value), then the value's
    /// slots. When the value is nothing they hold each slot's first value.
    Optional(Box<FieldType>),
    /// One boolean slot per value of the element type: whether the set holds it.
    Set {
        element_type: String,
        elements: Domain,
    },
    /// A length slot from 0 to `capacity`, then `capacity` items. Items at
    /// or past the length hold their first values.
    List {
        capacity: usize,
        element: Box<FieldType>,
    },
    /// A tag slot holding the case, then each case's members in turn. The
    /// members of the cases not taken hold their first values.
    Variant {
        tags: Domain,
        cases: Vec<(String, Vec<(String, FieldType)>)>,
    },
}

impl FieldType {
    /// How many state slots a value of this type occupies.
    #[must_use]
    pub fn width(&self) -> usize {
        match self {
            Self::Scalar(_) => 1,
            Self::Map { keys, value, .. } => keys.size().saturating_mul(value.width()),
            Self::Record(members) => members
                .iter()
                .fold(0, |width, (_, kind)| width.saturating_add(kind.width())),
            Self::Optional(value) => value.width().saturating_add(1),
            Self::Set { elements, .. } => elements.size(),
            Self::List { capacity, element } => {
                capacity.saturating_mul(element.width()).saturating_add(1)
            }
            Self::Variant { cases, .. } => cases
                .iter()
                .flat_map(|(_, members)| members)
                .fold(1, |width, (_, kind)| width.saturating_add(kind.width())),
        }
    }

    /// The slot values of a value of this type that nothing has set: the
    /// first value of every slot. A value that is nothing holds these.
    pub(crate) fn defaults(&self) -> Vec<i64> {
        match self {
            Self::Scalar(domain) => vec![domain.raw_at(0).unwrap_or(0)],
            Self::Map { keys, value, .. } => value.defaults().repeat(keys.size()),
            Self::Record(members) => members
                .iter()
                .flat_map(|(_, kind)| kind.defaults())
                .collect(),
            Self::Optional(value) => {
                let mut out = vec![0];
                out.extend(value.defaults());
                out
            }
            Self::Set { elements, .. } => vec![0; elements.size()],
            Self::List { capacity, element } => {
                let mut out = vec![0];
                out.extend(element.defaults().repeat(*capacity));
                out
            }
            Self::Variant { tags, cases } => {
                let mut out = vec![tags.raw_at(0).unwrap_or(0)];
                for (_, members) in cases {
                    out.extend(members.iter().flat_map(|(_, kind)| kind.defaults()));
                }
                out
            }
        }
    }

    /// Whether `plan/1` can carry this type: a scalar, or a map of scalars.
    pub(crate) fn is_classic(&self) -> bool {
        match self {
            Self::Scalar(_) => true,
            Self::Map { value, .. } => matches!(value.as_ref(), Self::Scalar(_)),
            Self::Record(_)
            | Self::Optional(_)
            | Self::Set { .. }
            | Self::List { .. }
            | Self::Variant { .. } => false,
        }
    }

    /// The member `name` of a record: its first slot, counted from the
    /// record's, and its type.
    pub(crate) fn member(&self, name: &str) -> Option<(usize, &FieldType)> {
        let Self::Record(members) = self else {
            return None;
        };
        let mut offset = 0;
        for (member, kind) in members {
            if member == name {
                return Some((offset, kind));
            }
            offset += kind.width();
        }
        None
    }
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

/// One dynamic map key on the way to a slot: the key's position in `keys`,
/// times `stride`, is added to the slot.
#[derive(Debug, Clone)]
pub(crate) struct KeyStep {
    pub key: Ir,
    pub keys: Domain,
    pub stride: usize,
    pub field: String,
}

#[derive(Debug, Clone)]
pub(crate) enum Ir {
    Const(i64),
    Slot(usize),
    Local(usize),
    Index {
        offset: usize,
        steps: Box<[KeyStep]>,
    },
    /// A compound value, one expression per slot. Compiled code takes it
    /// apart; it is never evaluated as a whole.
    Tuple(Vec<Ir>),
    /// A model fault with this message, such as appending to a full list.
    Fail(String),
    /// A model fault because a value is absent: the value of nothing, a
    /// member of a case that does not have it, or an item of an empty list.
    Absent(String),
    /// `value`, or `default` when evaluating `probe` and then `value` finds
    /// an absent value. `probe` holds the other slots of a compound read, so
    /// every slot of the value falls back together.
    Recover {
        probe: Vec<Ir>,
        value: Box<Ir>,
        default: Box<Ir>,
    },
    /// The option at `key`'s position in `keys`; `what` names the keys in
    /// the fault when `key` is not one of them.
    Select {
        key: Box<Ir>,
        keys: Domain,
        options: Vec<Ir>,
        what: String,
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
        steps: Box<[KeyStep]>,
    },
    Every {
        offset: usize,
        count: usize,
        stride: usize,
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
    /// Included files and their fingerprints; see [`Spec::sources`].
    pub sources: BTreeMap<String, String>,
    /// The component a model was isolated to by [`crate::verify_component`].
    pub component: Option<String>,
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
    /// Raw values of each `interchangeable` type; empty when there are none.
    pub(crate) interchangeable: Vec<Vec<i64>>,
}

pub(crate) fn index32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(crate) fn at(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

impl Model {
    /// Compiles a parsed specification. Errors locate their source file.
    pub fn compile(spec: &Spec) -> Result<Self> {
        Self::compile_unlocated(spec).map_err(|e| spec.locate(e))
    }

    /// As [`Model::compile`], with errors still in the parser's coordinates.
    pub(crate) fn compile_unlocated(spec: &Spec) -> Result<Self> {
        Compiler::new(spec).compile()
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
            let slots = state
                .get(field.offset..field.offset + field.kind.width())
                .unwrap_or_default();
            decoded.insert(field.name.clone(), self.decode_value(&field.kind, slots));
        }
        decoded
    }

    /// The value held in `slots`, laid out as `kind` lays it out.
    pub(crate) fn decode_value(&self, kind: &FieldType, slots: &[i64]) -> Value {
        match kind {
            FieldType::Scalar(domain) => self.value(domain, slots.first().copied().unwrap_or(0)),
            FieldType::Set { elements, .. } => Value::List(
                elements
                    .raws()
                    .into_iter()
                    .zip(slots)
                    .filter(|(_, held)| **held != 0)
                    .map(|(raw, _)| self.value(elements, raw))
                    .collect(),
            ),
            FieldType::Variant { tags, cases } => {
                let (tag, mut rest) = slots.split_first().unwrap_or((&0, &[]));
                let taken = tags.position(*tag).unwrap_or(0);
                for (_, members) in cases.iter().take(taken) {
                    let width: usize = members.iter().map(|(_, kind)| kind.width()).sum();
                    rest = rest.get(width..).unwrap_or_default();
                }
                let Some((name, members)) = cases.get(taken) else {
                    return Value::Nothing;
                };
                if members.is_empty() {
                    return Value::Symbol(name.clone());
                }
                let record = FieldType::Record(members.clone());
                let Value::Map(members) = self.decode_value(&record, rest) else {
                    return Value::Nothing;
                };
                Value::Tagged {
                    tag: name.clone(),
                    members,
                }
            }
            FieldType::List { element, .. } => {
                let width = element.width();
                let (length, items) = slots.split_first().unwrap_or((&0, &[]));
                Value::List(
                    items
                        .chunks(width.max(1))
                        .take(usize::try_from(*length).unwrap_or(0))
                        .map(|item| self.decode_value(element, item))
                        .collect(),
                )
            }
            FieldType::Optional(value) => match slots.split_first() {
                Some((1, rest)) => self.decode_value(value, rest),
                _ => Value::Nothing,
            },
            FieldType::Record(members) => {
                let mut offset = 0;
                Value::Map(
                    members
                        .iter()
                        .map(|(name, kind)| {
                            let width = kind.width();
                            let entry = slots.get(offset..offset + width).unwrap_or_default();
                            offset += width;
                            (name.clone(), self.decode_value(kind, entry))
                        })
                        .collect(),
                )
            }
            FieldType::Map { keys, value, .. } => {
                let width = value.width();
                Value::Map(
                    keys.raws()
                        .into_iter()
                        .enumerate()
                        .map(|(position, key)| {
                            let start = position * width;
                            let entry = slots.get(start..start + width).unwrap_or_default();
                            (
                                self.value(keys, key).to_string(),
                                self.decode_value(value, entry),
                            )
                        })
                        .collect(),
                )
            }
        }
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

/// Whether an observed value agrees with the specification's value of type
/// `kind`. Maps and records may list a subset of their keys; a set matches
/// in any order.
pub(crate) fn observation_matches(kind: &FieldType, expected: &Value, actual: &Value) -> bool {
    match (kind, expected, actual) {
        (FieldType::Map { value, .. }, Value::Map(wanted), Value::Map(observed)) => {
            observed.iter().all(|(key, item)| {
                wanted
                    .get(key)
                    .is_some_and(|wanted| observation_matches(value, wanted, item))
            })
        }
        (FieldType::Record(members), Value::Map(wanted), Value::Map(observed)) => {
            observed.iter().all(|(key, item)| {
                members
                    .iter()
                    .find(|(member, _)| member == key)
                    .zip(wanted.get(key))
                    .is_some_and(|((_, kind), wanted)| observation_matches(kind, wanted, item))
            })
        }
        (FieldType::Optional(value), wanted, observed) if *wanted != Value::Nothing => {
            observation_matches(value, wanted, observed)
        }
        (
            FieldType::Variant { cases, .. },
            Value::Tagged {
                tag: wanted_tag,
                members: wanted,
            },
            Value::Tagged { tag, members },
        ) => {
            wanted_tag == tag
                && cases
                    .iter()
                    .find(|(name, _)| name == tag)
                    .is_some_and(|(_, kinds)| {
                        observation_matches(
                            &FieldType::Record(kinds.clone()),
                            &Value::Map(wanted.clone()),
                            &Value::Map(members.clone()),
                        )
                    })
        }
        (FieldType::Set { .. }, Value::List(wanted), Value::List(observed)) => {
            observed.len() == wanted.len() && observed.iter().all(|item| wanted.contains(item))
        }
        (FieldType::List { element, .. }, Value::List(wanted), Value::List(observed)) => {
            observed.len() == wanted.len()
                && wanted
                    .iter()
                    .zip(observed)
                    .all(|(wanted, item)| observation_matches(element, wanted, item))
        }
        _ => expected == actual,
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
    stride: usize,
    raw: i64,
    field: &str,
) -> std::result::Result<usize, String> {
    keys.position(raw)
        .map(|position| offset + position * stride)
        .ok_or_else(|| format!("{raw} is not a key of {field}"))
}

/// The slot `offset` plus each key step, evaluated in order.
fn locate(
    offset: usize,
    steps: &[KeyStep],
    state: &[i64],
    locals: &mut [i64],
) -> std::result::Result<usize, String> {
    find(offset, steps, state, locals).map_err(Stop::message)
}

fn find(
    offset: usize,
    steps: &[KeyStep],
    state: &[i64],
    locals: &mut [i64],
) -> std::result::Result<usize, Stop> {
    let mut slot = offset;
    for step in steps {
        let key = run(&step.key, state, locals)?;
        slot = key_slot(slot, &step.keys, step.stride, key, &step.field)?;
    }
    Ok(slot)
}

pub(crate) fn collect_writes(
    assignments: &[CompiledAssignment],
    state: &[i64],
    locals: &mut [i64],
    slots: &[Slot],
) -> std::result::Result<Vec<Write>, Fault> {
    let mut writes = Vec::new();
    for assignment in assignments {
        if let Target::Every {
            offset,
            count,
            stride,
        } = &assignment.target
        {
            let raw = eval(&assignment.value, state, locals).map_err(|message| Fault {
                line: assignment.line,
                message,
            })?;
            writes.extend((0..*count).map(|position| Write {
                slot: offset + position * stride,
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
            Target::Indexed { offset, steps } => {
                locate(*offset, steps, state, locals).map_err(fault)?
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

/// Why evaluation stopped.
enum Stop {
    Absent(String),
    Fault(String),
}

impl Stop {
    fn message(self) -> String {
        match self {
            Self::Absent(message) | Self::Fault(message) => message,
        }
    }
}

impl From<String> for Stop {
    fn from(message: String) -> Self {
        Self::Fault(message)
    }
}

impl From<&str> for Stop {
    fn from(message: &str) -> Self {
        Self::Fault(message.to_string())
    }
}

pub(crate) fn eval(ir: &Ir, state: &[i64], locals: &mut [i64]) -> std::result::Result<i64, String> {
    run(ir, state, locals).map_err(Stop::message)
}

fn run(ir: &Ir, state: &[i64], locals: &mut [i64]) -> std::result::Result<i64, Stop> {
    Ok(match ir {
        Ir::Const(raw) => *raw,
        Ir::Slot(slot) => fetch(state, *slot)?,
        Ir::Local(local) => fetch(locals, *local)?,
        Ir::Index { offset, steps } => fetch(state, find(*offset, steps, state, locals)?)?,
        Ir::Tuple(_) => return Err("internal error: a compound value was evaluated".into()),
        Ir::Fail(message) => return Err(Stop::Fault(message.clone())),
        Ir::Absent(message) => return Err(Stop::Absent(message.clone())),
        Ir::Recover {
            probe,
            value,
            default,
        } => {
            let found = probe
                .iter()
                .try_for_each(|part| run(part, state, locals).map(drop))
                .and_then(|()| run(value, state, locals));
            match found {
                Err(Stop::Absent(_)) => run(default, state, locals)?,
                other => other?,
            }
        }
        Ir::Select {
            key,
            keys,
            options,
            what,
        } => {
            let key = run(key, state, locals)?;
            let option = keys
                .position(key)
                .and_then(|position| options.get(position))
                .ok_or_else(|| format!("{key} is not {what}"))?;
            run(option, state, locals)?
        }
        Ir::Not(value) => i64::from(run(value, state, locals)? == 0),
        Ir::Negate(value) => run(value, state, locals)?
            .checked_neg()
            .ok_or("integer overflow")?,
        Ir::If(condition, then, otherwise) => {
            if run(condition, state, locals)? != 0 {
                run(then, state, locals)?
            } else {
                run(otherwise, state, locals)?
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
                if run(body, state, locals)? != 0 {
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
            let left = run(left, state, locals)?;
            match operator {
                BinaryOperator::And if left == 0 => return Ok(0),
                BinaryOperator::Or if left != 0 => return Ok(1),
                BinaryOperator::Implies if left == 0 => return Ok(1),
                _ => {}
            }
            let right = run(right, state, locals)?;
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
                        return Err("division by zero".into());
                    }
                    left.checked_div(right).ok_or("integer overflow")?
                }
                BinaryOperator::Has | BinaryOperator::With | BinaryOperator::Without => {
                    return Err("internal error: a set operator was evaluated".into());
                }
            }
        }
    })
}

/// The static type of an expression. Compound types are structural: two
/// records agree when their members agree name by name, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ty {
    Int,
    Bool,
    Enum(BTreeSet<u32>),
    Record(Vec<(String, Ty)>),
    Map {
        keys: Domain,
        value: Box<Ty>,
    },
    Optional(Box<Ty>),
    /// The literal `nothing`, before the context says what it is nothing of.
    Nothing,
    Set(Domain),
    List {
        capacity: usize,
        element: Box<Ty>,
    },
    /// Each case's tag and members, in declaration order.
    Variant(Vec<(u32, Vec<(String, Ty)>)>),
}

impl Ty {
    fn of(domain: &Domain) -> Self {
        match domain {
            Domain::Integer { .. } => Self::Int,
            Domain::Boolean => Self::Bool,
            Domain::Enum(ids) => Self::Enum(ids.iter().copied().collect()),
        }
    }

    fn of_kind(kind: &FieldType) -> Self {
        match kind {
            FieldType::Scalar(domain) => Self::of(domain),
            FieldType::Map { keys, value, .. } => Self::Map {
                keys: keys.clone(),
                value: Box::new(Self::of_kind(value)),
            },
            FieldType::Record(members) => Self::Record(
                members
                    .iter()
                    .map(|(name, kind)| (name.clone(), Self::of_kind(kind)))
                    .collect(),
            ),
            FieldType::Optional(value) => Self::Optional(Box::new(Self::of_kind(value))),
            FieldType::Set { elements, .. } => Self::Set(elements.clone()),
            FieldType::List { capacity, element } => Self::List {
                capacity: *capacity,
                element: Box::new(Self::of_kind(element)),
            },
            FieldType::Variant { tags, cases } => Self::Variant(
                tags.raws()
                    .into_iter()
                    .zip(cases)
                    .map(|(tag, (_, members))| {
                        (
                            u32::try_from(tag).unwrap_or(u32::MAX),
                            members
                                .iter()
                                .map(|(name, kind)| (name.clone(), Self::of_kind(kind)))
                                .collect(),
                        )
                    })
                    .collect(),
            ),
        }
    }

    fn noun(&self) -> &'static str {
        match self {
            Self::Int => "a number",
            Self::Bool => "a true/false value",
            Self::Enum(_) => "an enum value",
            Self::Record(_) => "a record",
            Self::Map { .. } => "a map",
            Self::Optional(_) => "a value that may be nothing",
            Self::Nothing => "nothing",
            Self::Set(_) => "a set",
            Self::List { .. } => "a list",
            Self::Variant(_) => "an enum value with members",
        }
    }

    fn is_scalar(&self) -> bool {
        matches!(self, Self::Int | Self::Bool | Self::Enum(_))
    }

    /// The type of a value that may be either `self` or `other`, if they agree.
    fn join(&self, other: &Self) -> Option<Self> {
        Some(match (self, other) {
            (Self::Int, Self::Int) => Self::Int,
            (Self::Bool, Self::Bool) => Self::Bool,
            (Self::Enum(left), Self::Enum(right)) => Self::Enum(left | right),
            (Self::Record(left), Self::Record(right)) if left.len() == right.len() => {
                let mut members = Vec::with_capacity(left.len());
                for ((name, left), (other, right)) in left.iter().zip(right) {
                    if name != other {
                        return None;
                    }
                    members.push((name.clone(), left.join(right)?));
                }
                Self::Record(members)
            }
            (
                Self::Map { keys, value },
                Self::Map {
                    keys: other_keys,
                    value: other_value,
                },
            ) if keys == other_keys => Self::Map {
                keys: keys.clone(),
                value: Box::new(value.join(other_value)?),
            },
            (Self::Set(left), Self::Set(right)) if left == right => Self::Set(left.clone()),
            (Self::Variant(left), Self::Variant(right)) if left.len() == right.len() => {
                let mut cases = Vec::with_capacity(left.len());
                for ((tag, left), (other, right)) in left.iter().zip(right) {
                    if tag != other {
                        return None;
                    }
                    let Self::Record(members) =
                        Self::Record(left.clone()).join(&Self::Record(right.clone()))?
                    else {
                        return None;
                    };
                    cases.push((*tag, members));
                }
                Self::Variant(cases)
            }
            (Self::Variant(cases), Self::Enum(ids)) | (Self::Enum(ids), Self::Variant(cases))
                if bare_tags(cases, ids) =>
            {
                Self::Variant(cases.clone())
            }
            (
                Self::List { capacity, element },
                Self::List {
                    capacity: other,
                    element: other_element,
                },
            ) if capacity == other => Self::List {
                capacity: *capacity,
                element: Box::new(element.join(other_element)?),
            },
            (Self::Nothing, Self::Nothing) => Self::Nothing,
            (Self::Nothing, Self::Optional(value)) | (Self::Optional(value), Self::Nothing) => {
                Self::Optional(value.clone())
            }
            (Self::Nothing, value) | (value, Self::Nothing) => {
                Self::Optional(Box::new(value.clone()))
            }
            (Self::Optional(left), Self::Optional(right)) => {
                Self::Optional(Box::new(left.join(right)?))
            }
            (Self::Optional(left), right) | (right, Self::Optional(left)) => {
                Self::Optional(Box::new(left.join(right)?))
            }
            _ => return None,
        })
    }

    fn member(&self, name: &str) -> Option<(usize, &Ty)> {
        let Self::Record(members) = self else {
            return None;
        };
        let mut offset = 0;
        for (member, ty) in members {
            if member == name {
                return Some((offset, ty));
            }
            offset += ty.width();
        }
        None
    }

    fn width(&self) -> usize {
        match self {
            Self::Int | Self::Bool | Self::Enum(_) => 1,
            Self::Record(members) => members.iter().map(|(_, ty)| ty.width()).sum(),
            Self::Map { keys, value } => keys.size().saturating_mul(value.width()),
            Self::Optional(value) => value.width() + 1,
            Self::Nothing => 1,
            Self::Set(elements) => elements.size(),
            Self::List { capacity, element } => capacity * element.width() + 1,
            Self::Variant(cases) => {
                1 + cases
                    .iter()
                    .flat_map(|(_, members)| members)
                    .map(|(_, ty)| ty.width())
                    .sum::<usize>()
            }
        }
    }
}

/// Whether every id in `ids` is the tag of a case without members.
fn bare_tags(cases: &[(u32, Vec<(String, Ty)>)], ids: &BTreeSet<u32>) -> bool {
    ids.iter().all(|id| {
        cases
            .iter()
            .any(|(tag, members)| tag == id && members.is_empty())
    })
}

/// The parts of a variant value: its tag, then each case's members.
fn split_variant(parts: Vec<Ir>, cases: &[(u32, Vec<(String, Ty)>)]) -> (Ir, Vec<Vec<Ir>>) {
    let mut parts = parts.into_iter();
    let tag = parts.next().unwrap_or(Ir::Const(0));
    let payloads = cases
        .iter()
        .map(|(_, members)| {
            let width = members.iter().map(|(_, ty)| ty.width()).sum();
            parts.by_ref().take(width).collect()
        })
        .collect();
    (tag, payloads)
}

/// `parts` of a value of type `from`, laid out as the wider type `to`: a value
/// becomes present, `nothing` becomes an absent value of the right width.
fn coerce(parts: Vec<Ir>, from: &Ty, to: &Ty) -> Vec<Ir> {
    match (from, to) {
        (Ty::Nothing, Ty::Optional(value)) => std::iter::repeat_with(|| Ir::Const(0))
            .take(1 + value.width())
            .collect(),
        (Ty::Optional(from), Ty::Optional(to)) => {
            let mut parts = parts.into_iter();
            let tag = parts.next().unwrap_or(Ir::Const(0));
            let mut out = vec![tag];
            out.extend(coerce(parts.collect(), from, to));
            out
        }
        (from, Ty::Optional(to)) => {
            let mut out = vec![Ir::Const(1)];
            out.extend(coerce(parts, from, to));
            out
        }
        (Ty::Record(from), Ty::Record(to)) => {
            let mut parts = parts.into_iter();
            let mut out = Vec::new();
            for ((_, from), (_, to)) in from.iter().zip(to) {
                let member = parts.by_ref().take(from.width()).collect();
                out.extend(coerce(member, from, to));
            }
            out
        }
        (
            Ty::List { element: from, .. },
            Ty::List {
                capacity,
                element: to,
            },
        ) => {
            let mut parts = parts.into_iter();
            let mut out = vec![parts.next().unwrap_or(Ir::Const(0))];
            for _ in 0..*capacity {
                let item = parts.by_ref().take(from.width()).collect();
                out.extend(coerce(item, from, to));
            }
            out
        }
        (Ty::Enum(_), Ty::Variant(cases)) => {
            let mut parts = parts;
            let width = Ty::Variant(cases.clone()).width();
            parts.extend(std::iter::repeat_with(|| Ir::Const(0)).take(width - 1));
            parts
        }
        (Ty::Variant(from), Ty::Variant(to)) => {
            let (tag, payloads) = split_variant(parts, from);
            let mut out = vec![tag];
            for (payload, ((_, from), (_, to))) in payloads.into_iter().zip(from.iter().zip(to)) {
                out.extend(coerce(
                    payload,
                    &Ty::Record(from.clone()),
                    &Ty::Record(to.clone()),
                ));
            }
            out
        }
        (Ty::Map { value: from, .. }, Ty::Map { keys, value: to }) => {
            let mut parts = parts.into_iter();
            let mut out = Vec::new();
            for _ in 0..keys.size() {
                let entry = parts.by_ref().take(from.width()).collect();
                out.extend(coerce(entry, from, to));
            }
            out
        }
        _ => parts,
    }
}

/// Whether two values laid out as `ty` are equal. An absent optional equals
/// another absent one whatever its slots hold.
fn equal_as(left: Vec<Ir>, right: Vec<Ir>, ty: &Ty) -> Ir {
    let all = |checks: Vec<Ir>| {
        checks
            .into_iter()
            .reduce(|all, next| Ir::Binary(BinaryOperator::And, Box::new(all), Box::new(next)))
            .unwrap_or(Ir::Const(1))
    };
    match ty {
        Ty::Int | Ty::Bool | Ty::Enum(_) => {
            match (left.into_iter().next(), right.into_iter().next()) {
                (Some(left), Some(right)) => {
                    Ir::Binary(BinaryOperator::Equal, Box::new(left), Box::new(right))
                }
                _ => Ir::Const(1),
            }
        }
        Ty::Nothing => Ir::Const(1),
        Ty::Variant(cases) => {
            let (left_tag, left) = split_variant(left, cases);
            let (right_tag, right) = split_variant(right, cases);
            let mut checks = vec![Ir::Binary(
                BinaryOperator::Equal,
                Box::new(left_tag.clone()),
                Box::new(right_tag),
            )];
            for (((tag, members), left), right) in cases.iter().zip(left).zip(right) {
                if members.is_empty() {
                    continue;
                }
                checks.push(Ir::Binary(
                    BinaryOperator::Implies,
                    Box::new(is_raw(&left_tag, i64::from(*tag))),
                    Box::new(equal_as(left, right, &Ty::Record(members.clone()))),
                ));
            }
            all(checks)
        }
        Ty::List { capacity, element } => {
            let (mut left, mut right) = (left.into_iter(), right.into_iter());
            let (Some(length), Some(other)) = (left.next(), right.next()) else {
                return Ir::Const(1);
            };
            let width = element.width();
            let mut checks = vec![Ir::Binary(
                BinaryOperator::Equal,
                Box::new(length.clone()),
                Box::new(other),
            )];
            for position in 0..*capacity {
                let same = equal_as(
                    left.by_ref().take(width).collect(),
                    right.by_ref().take(width).collect(),
                    element,
                );
                checks.push(Ir::Binary(
                    BinaryOperator::Implies,
                    Box::new(longer(&length, position)),
                    Box::new(same),
                ));
            }
            all(checks)
        }
        Ty::Set(_) => all(left
            .into_iter()
            .zip(right)
            .map(|(left, right)| Ir::Binary(BinaryOperator::Equal, Box::new(left), Box::new(right)))
            .collect()),
        Ty::Optional(value) => {
            let (mut left, mut right) = (left.into_iter(), right.into_iter());
            let (Some(left_tag), Some(right_tag)) = (left.next(), right.next()) else {
                return Ir::Const(1);
            };
            let same_tag = Ir::Binary(
                BinaryOperator::Equal,
                Box::new(left_tag.clone()),
                Box::new(right_tag),
            );
            let values = equal_as(left.collect(), right.collect(), value);
            let absent_or_equal = Ir::Binary(
                BinaryOperator::Or,
                Box::new(Ir::Not(Box::new(left_tag))),
                Box::new(values),
            );
            Ir::Binary(
                BinaryOperator::And,
                Box::new(same_tag),
                Box::new(absent_or_equal),
            )
        }
        Ty::Record(members) => {
            let (mut left, mut right) = (left.into_iter(), right.into_iter());
            all(members
                .iter()
                .map(|(_, member)| {
                    let width = member.width();
                    equal_as(
                        left.by_ref().take(width).collect(),
                        right.by_ref().take(width).collect(),
                        member,
                    )
                })
                .collect())
        }
        Ty::Map { keys, value } => {
            let (mut left, mut right) = (left.into_iter(), right.into_iter());
            let width = value.width();
            all((0..keys.size())
                .map(|_| {
                    equal_as(
                        left.by_ref().take(width).collect(),
                        right.by_ref().take(width).collect(),
                        value,
                    )
                })
                .collect())
        }
    }
}

/// `parts`, to be written to a place of type `kind`, with every absent
/// optional's slots set to their first values, so each value has one encoding.
fn canonical(parts: Vec<Ir>, kind: &FieldType) -> Vec<Ir> {
    match kind {
        FieldType::Scalar(_) | FieldType::Set { .. } => parts,
        FieldType::Optional(value) => {
            let mut parts = parts.into_iter();
            let tag = parts.next().unwrap_or(Ir::Const(0));
            let inner = canonical(parts.collect(), value);
            let mut out = vec![tag.clone()];
            out.extend(
                inner
                    .into_iter()
                    .zip(value.defaults())
                    .map(|(part, default)| match part {
                        Ir::Const(raw) if raw == default => part,
                        part => Ir::If(
                            Box::new(tag.clone()),
                            Box::new(part),
                            Box::new(Ir::Const(default)),
                        ),
                    }),
            );
            out
        }
        FieldType::Record(members) => {
            let mut parts = parts.into_iter();
            members
                .iter()
                .flat_map(|(_, member)| {
                    canonical(parts.by_ref().take(member.width()).collect(), member)
                })
                .collect()
        }
        FieldType::Map { keys, value, .. } => {
            let mut parts = parts.into_iter();
            (0..keys.size())
                .flat_map(|_| canonical(parts.by_ref().take(value.width()).collect(), value))
                .collect()
        }
        FieldType::Variant { tags, cases } => {
            let mut parts = parts.into_iter();
            let tag = parts.next().unwrap_or(Ir::Const(0));
            let mut out = vec![tag.clone()];
            for (raw, (_, members)) in tags.raws().into_iter().zip(cases) {
                let record = FieldType::Record(members.clone());
                let payload = canonical(parts.by_ref().take(record.width()).collect(), &record);
                let taken = is_raw(&tag, raw);
                out.extend(payload.into_iter().zip(record.defaults()).map(
                    |(part, default)| match (&taken, part) {
                        (Ir::Const(0), _) => Ir::Const(default),
                        (Ir::Const(_), part) => part,
                        (_, Ir::Const(raw)) if raw == default => Ir::Const(raw),
                        (taken, part) => Ir::If(
                            Box::new(taken.clone()),
                            Box::new(part),
                            Box::new(Ir::Const(default)),
                        ),
                    },
                ));
            }
            out
        }
        FieldType::List { capacity, element } => {
            let mut parts = parts.into_iter();
            let length = parts.next().unwrap_or(Ir::Const(0));
            let mut out = vec![length.clone()];
            for position in 0..*capacity {
                let item = canonical(parts.by_ref().take(element.width()).collect(), element);
                let shown = longer(&length, position);
                out.extend(
                    item.into_iter()
                        .zip(element.defaults())
                        .map(|(part, default)| match (&shown, part) {
                            (Ir::Const(0), _) => Ir::Const(default),
                            (Ir::Const(_), part) => part,
                            (_, Ir::Const(raw)) if raw == default => Ir::Const(raw),
                            (shown, part) => Ir::If(
                                Box::new(shown.clone()),
                                Box::new(part),
                                Box::new(Ir::Const(default)),
                            ),
                        }),
                );
            }
            out
        }
    }
}

/// Whether a list of length `length` has an item at `position`.
fn longer(length: &Ir, position: usize) -> Ir {
    let position = i64::try_from(position).unwrap_or(i64::MAX);
    match length {
        Ir::Const(length) => Ir::Const(i64::from(*length > position)),
        length => Ir::Binary(
            BinaryOperator::Greater,
            Box::new(length.clone()),
            Box::new(Ir::Const(position)),
        ),
    }
}

/// The value inside an optional, faulting with `message` when it is nothing.
fn unwrap(ir: Ir, ty: Ty, message: &str) -> (Ir, Ty) {
    let Ty::Optional(value) = ty else {
        return (ir, ty);
    };
    let mut parts = parts(ir).into_iter();
    let tag = parts.next().unwrap_or(Ir::Const(0));
    let inner = parts
        .map(|part| {
            Ir::If(
                Box::new(tag.clone()),
                Box::new(part),
                Box::new(Ir::Absent(message.to_string())),
            )
        })
        .collect();
    unwrap(tuple(inner), *value, message)
}

/// Whether evaluating `ir` can find an absent value.
fn can_be_absent(ir: &Ir) -> bool {
    match ir {
        Ir::Absent(_) => true,
        Ir::Const(_) | Ir::Slot(_) | Ir::Local(_) | Ir::Fail(_) => false,
        Ir::Index { steps, .. } => steps.iter().any(|step| can_be_absent(&step.key)),
        Ir::Tuple(parts) => parts.iter().any(can_be_absent),
        Ir::Select { key, options, .. } => can_be_absent(key) || options.iter().any(can_be_absent),
        Ir::Not(value) | Ir::Negate(value) => can_be_absent(value),
        Ir::Binary(_, left, right) => can_be_absent(left) || can_be_absent(right),
        Ir::If(condition, then, otherwise) => {
            can_be_absent(condition) || can_be_absent(then) || can_be_absent(otherwise)
        }
        Ir::Quantified { body, .. } => can_be_absent(body),
        Ir::Recover { default, .. } => can_be_absent(default),
    }
}

/// How a fault message names the value `expr` reads.
fn describe_expr(expr: &Expr) -> String {
    match expr {
        Expr::Name { name, .. } => name.clone(),
        Expr::Index { map, key, .. } => format!("{map} of {}", describe_expr(key)),
        Expr::Subscript { base, key } => format!("{}[{}]", describe_expr(base), describe_expr(key)),
        Expr::Literal(value) => value.source(),
        Expr::OrElse { value, default } => {
            format!(
                "{} or else {}",
                describe_expr(value),
                describe_expr(default)
            )
        }
        _ => "a value".to_string(),
    }
}

/// A state location: a first slot, the dynamic keys that move it, and the
/// type found there.
#[derive(Debug, Clone)]
struct Place {
    offset: usize,
    steps: Vec<KeyStep>,
    /// Set by `of every <type>`: the number of entries and the slots between them.
    every: Option<(usize, usize)>,
    kind: FieldType,
    label: String,
}

impl Place {
    fn targets(&self) -> Vec<Target> {
        (0..self.kind.width())
            .map(|position| {
                let offset = self.offset + position;
                match self.every {
                    Some((count, stride)) => Target::Every {
                        offset,
                        count,
                        stride,
                    },
                    None if self.steps.is_empty() => Target::Slot(offset),
                    None => Target::Indexed {
                        offset,
                        steps: self.steps.clone().into(),
                    },
                }
            })
            .collect()
    }

    fn member(&self, name: &str) -> Option<Self> {
        let (offset, kind) = self.kind.member(name)?;
        Some(Self {
            offset: self.offset + offset,
            steps: self.steps.clone(),
            every: self.every,
            kind: kind.clone(),
            label: format!("{name} of {}", self.label),
        })
    }
}

/// A compiled expression: a state location, which can still be indexed,
/// assigned or read, or a value.
enum Operand {
    Place(Place),
    Value(Ir, Ty),
}

/// The per-slot expressions of a compiled value.
fn parts(ir: Ir) -> Vec<Ir> {
    match ir {
        Ir::Tuple(parts) => parts,
        ir => vec![ir],
    }
}

fn tuple(mut parts: Vec<Ir>) -> Ir {
    if parts.len() == 1 {
        parts.pop().unwrap_or(Ir::Const(0))
    } else {
        Ir::Tuple(parts)
    }
}

/// Whether `expr` is a literal that takes its type from the other side.
fn record_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Record(_) | Expr::Items(_) | Expr::Literal(Value::Nothing)
    )
}

struct Compiler<'a> {
    spec: &'a Spec,
    symbols: Vec<String>,
    symbol_ids: HashMap<String, u32>,
    types: HashMap<String, FieldType>,
    fields: Vec<FieldInfo>,
    field_ids: HashMap<String, usize>,
    slots: Vec<Slot>,
    scope: Vec<(String, Ty)>,
    /// Every enumeration with members, for building one from a case name.
    variants: Vec<Ty>,
    /// Quantifier variables that stand for a list item: the local holding
    /// the position, and the item read at it.
    views: Vec<(usize, Ir, Ty)>,
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
            views: Vec::new(),
            variants: Vec::new(),
            locals: 0,
            constant_only: false,
            unnamed: HashMap::new(),
            enums: Vec::new(),
        }
    }

    fn compile(mut self) -> Result<Model> {
        let spec = self.spec;
        for declaration in &spec.types {
            self.declare_type(declaration)
                .map_err(|e| e.within(declaration.span))?;
        }
        let groups = self.interchangeable()?;
        for field in &spec.state {
            self.declare_field(&field.name, &field.kind, field.line)
                .map_err(|e| e.within(field.span))?;
        }
        let mut signatures = Vec::new();
        for action in &spec.actions {
            signatures.push(self.signature(action).map_err(|e| e.within(action.span))?);
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
            views: Vec::new(),
            variants: self.variants,
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
                        call: resolve_call(&model, call).map_err(|e| e.within(call.span))?,
                        line: call.line,
                    },
                    ScenarioStep::Reject(call) => CompiledStep::Reject {
                        call: resolve_call(&model, call).map_err(|e| e.within(call.span))?,
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
            let Some(FieldType::Scalar(Domain::Enum(ids))) = self.types.get(&declaration.name)
            else {
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
            FieldType::Scalar(domain) => Ok(domain),
            FieldType::Map { .. } => Err(Error::at(line, "a map is only allowed as a state field")),
            FieldType::Record(_) => Err(Error::at(
                line,
                "a record is only allowed in state; parameters are integers, booleans or enums",
            )),
            FieldType::Optional(_) => Err(Error::at(
                line,
                "'or nothing' is only allowed in state; parameters are integers, booleans or enums",
            )),
            FieldType::Set { .. } => Err(Error::at(
                line,
                "a set is only allowed in state; parameters are integers, booleans or enums",
            )),
            FieldType::List { .. } => Err(Error::at(
                line,
                "a list is only allowed in state; parameters are integers, booleans or enums",
            )),
            FieldType::Variant { .. } => Err(Error::at(
                line,
                "an enum value with members is only allowed in state; parameters are integers, booleans or enums",
            )),
        }
    }

    fn resolve(&mut self, kind: &TypeExpr, line: usize) -> Result<FieldType> {
        Ok(FieldType::Scalar(match kind {
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
            TypeExpr::Named(name) => {
                return self.types.get(name).cloned().ok_or_else(|| {
                    Error::at(
                        line,
                        format!("unknown type {name}; declare it in a types block"),
                    )
                    .coded(Code::NameUnknown)
                    .suggest_closest(name, self.types.keys().map(String::as_str))
                });
            }
            TypeExpr::Optional(value) => {
                return Ok(FieldType::Optional(Box::new(self.resolve(value, line)?)));
            }
            TypeExpr::Variant(cases) => {
                let mut ids = Vec::with_capacity(cases.len());
                let mut resolved = Vec::with_capacity(cases.len());
                for (name, members) in cases {
                    let id = self.intern(name);
                    if ids.contains(&id) {
                        return Err(Error::at(line, format!("duplicate enum value {name}")));
                    }
                    ids.push(id);
                    let mut fields = Vec::with_capacity(members.len());
                    for member in members {
                        fields.push((
                            member.name.clone(),
                            self.resolve(&member.kind, member.line)
                                .map_err(|e| e.within(member.span))?,
                        ));
                    }
                    resolved.push((name.clone(), fields));
                }
                self.enums.push((ids.clone(), line));
                let kind = FieldType::Variant {
                    tags: Domain::Enum(ids),
                    cases: resolved,
                };
                self.variants.push(Ty::of_kind(&kind));
                return Ok(kind);
            }
            TypeExpr::List { capacity, element } => {
                let element = self.resolve(element, line)?;
                if capacity.saturating_mul(element.width()) > MAX_SLOTS {
                    return Err(Error::at(
                        line,
                        format!("a list may hold at most {MAX_SLOTS} values"),
                    ));
                }
                return Ok(FieldType::List {
                    capacity: *capacity,
                    element: Box::new(element),
                });
            }
            TypeExpr::Set(element) => {
                let elements = match self.types.get(element) {
                    Some(FieldType::Scalar(domain)) => domain.clone(),
                    Some(_) => {
                        return Err(Error::at(
                            line,
                            format!(
                                "set elements must be integers, booleans or enums, and {element} is not"
                            ),
                        ));
                    }
                    None => {
                        return Err(Error::at(
                            line,
                            format!("unknown element type {element}; declare it in a types block"),
                        )
                        .coded(Code::NameUnknown)
                        .suggest_closest(element, self.scalar_types()));
                    }
                };
                if elements.size() > MAX_SLOTS {
                    return Err(Error::at(
                        line,
                        format!("a set may hold at most {MAX_SLOTS} values"),
                    ));
                }
                return Ok(FieldType::Set {
                    element_type: element.clone(),
                    elements,
                });
            }
            TypeExpr::Record(members) => {
                let mut resolved = Vec::with_capacity(members.len());
                for member in members {
                    resolved.push((
                        member.name.clone(),
                        self.resolve(&member.kind, member.line)
                            .map_err(|e| e.within(member.span))?,
                    ));
                }
                return Ok(FieldType::Record(resolved));
            }
            TypeExpr::Map { key, value } => {
                let keys = match self.types.get(key) {
                    Some(FieldType::Scalar(domain)) => domain.clone(),
                    Some(_) => {
                        return Err(Error::at(
                            line,
                            format!(
                                "map keys must be integers, booleans or enums, and {key} is not"
                            ),
                        ));
                    }
                    None => {
                        return Err(Error::at(
                            line,
                            format!("unknown key type {key}; declare it in a types block"),
                        )
                        .coded(Code::NameUnknown)
                        .suggest_closest(key, self.scalar_types()));
                    }
                };
                if keys.size() > MAX_SLOTS {
                    return Err(Error::at(
                        line,
                        format!("a map may have at most {MAX_SLOTS} keys"),
                    ));
                }
                let value = self.resolve(value, line)?;
                return Ok(FieldType::Map {
                    key_type: key.clone(),
                    keys,
                    value: Box::new(value),
                });
            }
        }))
    }

    fn declare_field(&mut self, name: &str, kind: &TypeExpr, line: usize) -> Result<()> {
        if self.field_ids.contains_key(name) {
            return Err(Error::at(line, format!("duplicate state field {name}")));
        }
        let offset = self.slots.len();
        let kind = self.resolve(kind, line)?;
        if offset.saturating_add(kind.width()) > MAX_SLOTS {
            return Err(Error::at(
                line,
                format!("the state may hold at most {MAX_SLOTS} values"),
            ));
        }
        self.lay_out(&kind, name);
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

    /// Appends the slots of a value of type `kind`, labeled from `label`.
    fn lay_out(&mut self, kind: &FieldType, label: &str) {
        match kind {
            FieldType::Scalar(domain) => self.slots.push(Slot {
                domain: domain.clone(),
                label: label.to_string(),
            }),
            FieldType::Map { keys, value, .. } => {
                for key in keys.raws() {
                    let key = self.raw_value(keys, key);
                    self.lay_out(value, &format!("{label} of {key}"));
                }
            }
            FieldType::Record(members) => {
                for (name, kind) in members {
                    self.lay_out(kind, &format!("{name} of {label}"));
                }
            }
            FieldType::Optional(value) => {
                self.slots.push(Slot {
                    domain: Domain::Boolean,
                    label: label.to_string(),
                });
                self.lay_out(value, label);
            }
            FieldType::Set { elements, .. } => {
                for element in elements.raws() {
                    let element = self.raw_value(elements, element);
                    self.slots.push(Slot {
                        domain: Domain::Boolean,
                        label: format!("{label} has {element}"),
                    });
                }
            }
            FieldType::Variant { tags, cases } => {
                self.slots.push(Slot {
                    domain: tags.clone(),
                    label: label.to_string(),
                });
                for (_, members) in cases {
                    for (member, kind) in members {
                        self.lay_out(kind, &format!("{member} of {label}"));
                    }
                }
            }
            FieldType::List { capacity, element } => {
                self.slots.push(Slot {
                    domain: Domain::Integer {
                        min: 0,
                        max: i64::try_from(*capacity).unwrap_or(i64::MAX),
                    },
                    label: format!("length of {label}"),
                });
                for position in 0..*capacity {
                    self.lay_out(element, &format!("item {} of {label}", position + 1));
                }
            }
        }
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

    fn declare_type(&mut self, declaration: &TypeDecl) -> Result<()> {
        let domain = self.resolve(&declaration.kind, declaration.line)?;
        if matches!(domain, FieldType::Map { .. }) {
            return Err(Error::at(
                declaration.line,
                "a named type cannot be a map; declare the map in state",
            ));
        }
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
        Ok(())
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
                return Err(
                    Error::at(param.line, format!("duplicate parameter {}", param.name))
                        .spanned(param.span),
                );
            }
            if self.field_ids.contains_key(&param.name) {
                return Err(Error::at(
                    param.line,
                    format!(
                        "parameter {} has the same name as a state field",
                        param.name
                    ),
                )
                .spanned(param.span));
            }
            params.push(Param {
                name: param.name.clone(),
                domain: self
                    .scalar(&param.kind, param.line)
                    .map_err(|e| e.within(param.span))?,
            });
        }
        Ok(params)
    }

    fn initial_state(&mut self) -> Result<Box<[i64]>> {
        let spec = self.spec;
        self.constant_only = true;
        let mut assignments = Vec::new();
        for assignment in &spec.initially {
            assignments.extend(self.assignment(assignment)?);
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
        self.locals = self.locals.max(params.len());
        let mut requires = Vec::new();
        for condition in &action.requires {
            self.enter_action(&params, condition.params.as_deref());
            requires.push(self.condition(condition)?);
        }
        let mut assignments = Vec::new();
        for assignment in &action.assignments {
            self.enter_action(&params, assignment.params.as_deref());
            assignments.extend(self.assignment(assignment)?);
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

    /// Puts the action's parameters in scope at their call positions. A
    /// parameter outside `visible` keeps its slot but cannot be named.
    fn enter_action(&mut self, params: &[Param], visible: Option<&[String]>) {
        self.scope = params
            .iter()
            .map(|param| {
                let name = match visible {
                    Some(visible) if !visible.contains(&param.name) => String::new(),
                    _ => param.name.clone(),
                };
                (name, Ty::of(&param.domain))
            })
            .collect();
    }

    fn condition(&mut self, condition: &Condition) -> Result<CompiledCondition> {
        let (ir, ty) = self
            .expr(&condition.expr, condition.line)
            .map_err(|e| e.within(condition.span))?;
        if ty != Ty::Bool {
            return Err(Error::at(
                condition.line,
                format!(
                    "'{}' must be true or false, but it is {}",
                    condition.source,
                    ty.noun()
                ),
            )
            .coded(Code::TypeMismatch)
            .spanned(condition.span));
        }
        Ok(CompiledCondition {
            ir,
            source: condition.source.clone(),
            line: condition.line,
        })
    }

    fn assignment(&mut self, assignment: &Assignment) -> Result<Vec<CompiledAssignment>> {
        self.assignment_at(assignment)
            .map_err(|e| e.within(assignment.target))
    }

    fn assignment_at(&mut self, assignment: &Assignment) -> Result<Vec<CompiledAssignment>> {
        let line = assignment.line;
        if let Some((Ir::Slot(slot), Ty::Int | Ty::Bool | Ty::Enum(_))) =
            self.scalar_field_at(&assignment.place)
        {
            let (value, ty) = self.expr_as(&assignment.value, None, line)?;
            if ty.is_scalar() && assignable(&ty, &self.slot_kind(slot)) {
                return Ok(vec![CompiledAssignment {
                    target: Target::Slot(slot),
                    value,
                    line,
                }]);
            }
        }
        let place = self.target(&assignment.place, line)?;
        let want = Ty::of_kind(&place.kind);
        let (value, ty) = self.expr_as(&assignment.value, Some(&want), line)?;
        if !assignable(&ty, &place.kind) {
            if let (Ty::Enum(ids), Ty::Variant(cases)) = (&ty, &want)
                && let Some((tag, members)) = cases
                    .iter()
                    .find(|(tag, members)| ids.contains(tag) && !members.is_empty())
            {
                let name = self.symbols.get(at(*tag)).cloned().unwrap_or_default();
                let example = members
                    .iter()
                    .map(|(member, _)| format!("{member}: ..."))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(Error::at(
                    line,
                    format!("{name} carries members; write '{name} with {{{example}}}'"),
                )
                .coded(Code::TypeMismatch));
            }
            if let (FieldType::Map { key_type, .. }, true) = (&place.kind, ty.is_scalar()) {
                return Err(Error::at(
                    line,
                    format!(
                        "{name} is a map; assign '{name} of <{key_type}>' or '{name} of every {key_type}'",
                        name = place.label
                    ),
                ));
            }
            return Err(Error::at(
                line,
                format!(
                    "{} is {}, but '{}' assigns {}",
                    place.label,
                    self.describe_kind(&place.kind),
                    assignment.source,
                    self.describe_ty(&ty)
                ),
            )
            .coded(Code::TypeMismatch));
        }
        let value = canonical(coerce(parts(value), &ty, &want), &place.kind);
        Ok(place
            .targets()
            .into_iter()
            .zip(value)
            .map(|(target, value)| CompiledAssignment {
                target,
                value,
                line,
            })
            .collect())
    }

    /// The place an assignment writes.
    fn target(&mut self, expr: &Expr, line: usize) -> Result<Place> {
        match self.operand(expr, None, line)? {
            Operand::Place(place) => Ok(place),
            Operand::Value(..) => Err(Error::at(
                line,
                "only a state field, or an entry or member of one, can be assigned",
            )),
        }
    }

    fn describe_kind(&self, kind: &FieldType) -> String {
        match kind {
            FieldType::Scalar(domain) => self.describe(domain),
            FieldType::Map {
                key_type, value, ..
            } => format!("a map from {key_type} to {}", self.describe_kind(value)),
            FieldType::Record(members) => format!(
                "a record with ({})",
                members
                    .iter()
                    .map(|(name, kind)| format!("{name} is {}", self.describe_kind(kind)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            FieldType::Optional(value) => format!("{} or nothing", self.describe_kind(value)),
            FieldType::Set { element_type, .. } => format!("a set of {element_type}"),
            FieldType::List { capacity, element } => {
                format!(
                    "a list of at most {capacity} {}",
                    self.describe_kind(element)
                )
            }
            FieldType::Variant { cases, .. } => format!(
                "one of {}",
                cases
                    .iter()
                    .map(|(name, members)| if members.is_empty() {
                        name.clone()
                    } else {
                        format!(
                            "{name} with ({})",
                            members
                                .iter()
                                .map(|(member, kind)| format!(
                                    "{member} is {}",
                                    self.describe_kind(kind)
                                ))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
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
            Ty::Record(members) => format!(
                "{{{}}}",
                members
                    .iter()
                    .map(|(name, ty)| format!("{name}: {}", self.describe_ty(ty)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Ty::Optional(value) => format!("{} or nothing", self.describe_ty(value)),
            Ty::Set(elements) => format!("a set of {}", self.describe(elements)),
            Ty::List { capacity, element } => {
                format!("a list of at most {capacity} {}", self.describe_ty(element))
            }
            Ty::Variant(cases) => cases
                .iter()
                .map(|(tag, _)| self.symbols.get(at(*tag)).cloned().unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" or "),
            other => other.noun().to_string(),
        }
    }

    fn key(&mut self, key: &Expr, keys: &Domain, field: &str, line: usize) -> Result<Ir> {
        let (ir, ty) = self.expr(key, line)?;
        if !assignable(&ty, &FieldType::Scalar(keys.clone())) {
            return Err(Error::at(
                line,
                format!(
                    "{field} is keyed by {}, but the key here is {}",
                    self.describe(keys),
                    self.describe_ty(&ty)
                ),
            )
            .coded(Code::TypeMismatch));
        }
        Ok(ir)
    }

    /// Compiles `expr` where an integer, boolean or enum is needed: the value
    /// of an optional is read, and reading it when it is nothing is a fault.
    fn expr(&mut self, expr: &Expr, line: usize) -> Result<(Ir, Ty)> {
        let (ir, ty) = self.expr_as(expr, None, line)?;
        if !matches!(ty, Ty::Optional(_)) {
            return Ok((ir, ty));
        }
        Ok(unwrap(
            ir,
            ty,
            &format!("{} is nothing", describe_expr(expr)),
        ))
    }

    /// Compiles `expr` to a value. `want` is the type the context expects,
    /// which a record literal needs to order its members.
    fn expr_as(&mut self, expr: &Expr, want: Option<&Ty>, line: usize) -> Result<(Ir, Ty)> {
        if let Some(read) = self.scalar_field(expr) {
            return Ok(read);
        }
        match self.operand(expr, want, line)? {
            Operand::Place(place) => self.read(&place, line),
            Operand::Value(ir, ty) => Ok((ir, ty)),
        }
    }

    /// A read of a scalar state field named by `expr`, without building a
    /// place: the common case, kept as cheap as before compound types.
    fn scalar_field(&self, expr: &Expr) -> Option<(Ir, Ty)> {
        if self.constant_only {
            return None;
        }
        self.scalar_field_at(expr)
    }

    /// The slot and type of the scalar state field named by `expr`.
    fn scalar_field_at(&self, expr: &Expr) -> Option<(Ir, Ty)> {
        let Expr::Name { name, .. } = expr else {
            return None;
        };
        if self.scope.iter().any(|(local, _)| local == name) {
            return None;
        }
        let field = self.fields.get(*self.field_ids.get(name)?)?;
        let FieldType::Scalar(domain) = &field.kind else {
            return None;
        };
        Some((Ir::Slot(field.offset), Ty::of(domain)))
    }

    fn slot_kind(&self, slot: usize) -> FieldType {
        FieldType::Scalar(
            self.slots
                .get(slot)
                .map_or(Domain::Boolean, |slot| slot.domain.clone()),
        )
    }

    fn read(&self, place: &Place, line: usize) -> Result<(Ir, Ty)> {
        if self.constant_only {
            return Err(Error::at(
                line,
                format!(
                    "initial values must be constants, but this reads {}",
                    place.label
                ),
            ));
        }
        if place.every.is_some() {
            return Err(Error::at(
                line,
                "'of every' names all entries at once, so it can only be assigned",
            ));
        }
        let read = (0..place.kind.width())
            .map(|position| {
                let offset = place.offset + position;
                if place.steps.is_empty() {
                    Ir::Slot(offset)
                } else {
                    Ir::Index {
                        offset,
                        steps: place.steps.clone().into(),
                    }
                }
            })
            .collect();
        Ok((tuple(read), Ty::of_kind(&place.kind)))
    }

    fn operand(&mut self, expr: &Expr, want: Option<&Ty>, line: usize) -> Result<Operand> {
        match expr {
            Expr::Name { name, span } => self
                .name(name, line)
                .map_err(|e| self.suggest_name(e, name, want).within(*span)),
            Expr::Index { map, key, span } => self.of(map, key, line).map_err(|e| e.within(*span)),
            Expr::Subscript { base, key } => match self.operand(base, None, line)? {
                Operand::Place(place) => Ok(Operand::Place(self.index(place, key, line)?)),
                Operand::Value(_, ty) => Err(Error::at(
                    line,
                    format!("only a map in the state can be indexed, not {}", ty.noun()),
                )),
            },
            other => {
                let (ir, ty) = self.value(other, want, line)?;
                Ok(Operand::Value(ir, ty))
            }
        }
    }

    fn value(&mut self, expr: &Expr, want: Option<&Ty>, line: usize) -> Result<(Ir, Ty)> {
        match expr {
            Expr::Literal(Value::Integer(raw)) => Ok((Ir::Const(*raw), Ty::Int)),
            Expr::Literal(Value::Boolean(flag)) => Ok((Ir::Const(i64::from(*flag)), Ty::Bool)),
            Expr::Literal(Value::Symbol(name)) => match self.symbol_ids.get(name).copied() {
                Some(id) => self.symbol(id, line),
                None => Err(Error::at(line, format!("unknown enum value \"{name}\""))
                    .coded(Code::NameUnknown)
                    .suggest(
                        &format!("\"{name}\""),
                        explain::closest(name, self.symbol_candidates(want))
                            .into_iter()
                            .map(|value| format!("\"{value}\""))
                            .collect(),
                    )),
            },
            Expr::Literal(Value::Map(_)) => Err(Error::at(line, "map literals are not supported")),
            Expr::Literal(Value::Nothing) => Ok((Ir::Const(0), Ty::Nothing)),
            Expr::Literal(Value::List(_)) => Err(Error::at(line, "list values are written [a, b]")),
            Expr::Literal(Value::Tagged { .. }) => Err(Error::at(
                line,
                "enum values with members are written 'name with {member: value}'",
            )),
            Expr::Binary {
                operator: BinaryOperator::With,
                left,
                right,
            } if matches!(right.as_ref(), Expr::Record(_)) && self.case_name(left).is_some() => {
                let (Some(tag), Expr::Record(members)) = (self.case_name(left), right.as_ref())
                else {
                    return Err(Error::at(line, "internal error: not a case"));
                };
                self.construct(&tag, members, want, line)
            }
            Expr::Name { .. } | Expr::Index { .. } | Expr::Subscript { .. } => {
                self.expr_as(expr, want, line)
            }
            Expr::Every(domain) => Err(Error::at(
                line,
                format!("'every {domain}' names all keys at once, so it can only be assigned to"),
            )),
            Expr::Record(members) => self.record(members, want, line),
            Expr::Items(items) => self.items(items, want, line),
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
                let ((then, then_ty), (otherwise, otherwise_ty)) =
                    self.pair(then, otherwise, want, line)?;
                let Some(ty) = then_ty.join(&otherwise_ty) else {
                    return Err(Error::at(
                        line,
                        format!(
                            "both branches of 'if' must have the same kind of value, but one is {} and the other {}",
                            then_ty.noun(),
                            otherwise_ty.noun()
                        ),
                    ));
                };
                let then = coerce(parts(then), &then_ty, &ty);
                let otherwise = coerce(parts(otherwise), &otherwise_ty, &ty);
                let branches = then
                    .into_iter()
                    .zip(otherwise)
                    .map(|(then, otherwise)| {
                        Ir::If(
                            Box::new(condition.clone()),
                            Box::new(then),
                            Box::new(otherwise),
                        )
                    })
                    .collect();
                Ok((tuple(branches), ty))
            }
            Expr::Quantified {
                quantifier,
                variable,
                domain,
                body,
            } => self.quantified(*quantifier, variable, domain, body, line),
            Expr::OrElse { value, default } => self.or_else(value, default, line),
        }
    }

    /// Two operands that must agree in type: a record literal on one side
    /// takes its member order from the other.
    fn pair(
        &mut self,
        left: &Expr,
        right: &Expr,
        want: Option<&Ty>,
        line: usize,
    ) -> Result<((Ir, Ty), (Ir, Ty))> {
        if record_literal(left) && !record_literal(right) {
            let right = self.expr_as(right, want, line)?;
            let left = self.expr_as(left, Some(&right.1), line)?;
            Ok((left, right))
        } else {
            let left = self.expr_as(left, want, line)?;
            let right = self.expr_as(right, want.or(Some(&left.1)), line)?;
            Ok((left, right))
        }
    }

    fn record(
        &mut self,
        members: &[(String, Expr)],
        want: Option<&Ty>,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        if let Some(Ty::Map { keys, value }) = want {
            return self.map_literal(members, keys, value, line);
        }
        let order: Vec<(String, Option<Ty>)> = match want {
            Some(Ty::Record(wanted)) => {
                if let Some((extra, _)) = members
                    .iter()
                    .find(|(name, _)| !wanted.iter().any(|(member, _)| member == name))
                {
                    return Err(
                        Error::at(line, format!("this record has no member {extra}"))
                            .coded(Code::NameUnknown)
                            .suggest_closest(
                                extra,
                                wanted.iter().map(|(member, _)| member.as_str()),
                            ),
                    );
                }
                wanted
                    .iter()
                    .map(|(name, ty)| (name.clone(), Some(ty.clone())))
                    .collect()
            }
            _ => members
                .iter()
                .map(|(name, _)| (name.clone(), None))
                .collect(),
        };
        let mut irs = Vec::new();
        let mut tys = Vec::with_capacity(order.len());
        for (name, want) in order {
            let Some((_, value)) = members.iter().find(|(member, _)| *member == name) else {
                return Err(Error::at(
                    line,
                    format!("this record needs a value for {name}"),
                ));
            };
            let (ir, ty) = self.expr_as(value, want.as_ref(), line)?;
            irs.extend(parts(ir));
            tys.push((name, ty));
        }
        Ok((tuple(irs), Ty::Record(tys)))
    }

    /// `{key: value, ...}` where a map is expected: one value for every key.
    fn map_literal(
        &mut self,
        members: &[(String, Expr)],
        keys: &Domain,
        value: &Ty,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        let names: Vec<String> = keys
            .raws()
            .into_iter()
            .map(|raw| self.raw_value(keys, raw).to_string())
            .collect();
        if let Some((extra, _)) = members.iter().find(|(name, _)| !names.contains(name)) {
            return Err(Error::at(line, format!("{extra} is not a key of this map"))
                .coded(Code::NameUnknown)
                .suggest_closest(extra, names.iter().map(String::as_str)));
        }
        let mut irs = Vec::new();
        let mut joined = value.clone();
        for name in names {
            let Some((_, entry)) = members.iter().find(|(member, _)| *member == name) else {
                return Err(Error::at(
                    line,
                    format!("this map needs a value for every key, and {name} has none"),
                ));
            };
            let (ir, ty) = self.expr_as(entry, Some(value), line)?;
            joined = joined.join(&ty).unwrap_or(ty);
            irs.extend(parts(ir));
        }
        Ok((
            tuple(irs),
            Ty::Map {
                keys: keys.clone(),
                value: Box::new(joined),
            },
        ))
    }

    fn name(&mut self, name: &str, line: usize) -> Result<Operand> {
        if let Some(local) = self.scope.iter().rposition(|(local, _)| local == name) {
            if let Some((_, item, ty)) = self.views.iter().rev().find(|(at, ..)| *at == local) {
                return Ok(Operand::Value(item.clone(), ty.clone()));
            }
            let ty = self.scope.get(local).map_or(Ty::Int, |(_, ty)| ty.clone());
            return Ok(Operand::Value(Ir::Local(local), ty));
        }
        if let Some(place) = self.field(name) {
            return Ok(Operand::Place(place));
        }
        if let Some(id) = self.symbol_ids.get(name).copied() {
            let (ir, ty) = self.symbol(id, line)?;
            return Ok(Operand::Value(ir, ty));
        }
        Err(Error::at(
            line,
            format!("unknown name {name}; declare it or use a quoted enum value"),
        )
        .coded(Code::NameUnknown))
    }

    /// Suggest names close to `name` that could stand where a value of type
    /// `want` is needed.
    fn suggest_name(&self, error: Error, name: &str, want: Option<&Ty>) -> Error {
        if error.code() != Code::NameUnknown.name() {
            return error;
        }
        let fits = |ty: &Ty| want.is_none_or(|want| want.join(ty).is_some());
        let locals = self
            .scope
            .iter()
            .filter(|(_, ty)| fits(ty))
            .map(|(local, _)| local.as_str());
        let fields = self
            .fields
            .iter()
            .filter(|field| fits(&Ty::of_kind(&field.kind)))
            .map(|field| field.name.as_str());
        let literals = match want {
            None => ["true", "false", "nothing"].as_slice(),
            Some(Ty::Bool) => ["true", "false"].as_slice(),
            Some(Ty::Optional(_)) => ["nothing"].as_slice(),
            Some(_) => [].as_slice(),
        };
        let candidates = locals
            .chain(fields)
            .chain(self.symbol_candidates(want))
            .chain(literals.iter().copied());
        error.suggest_closest(name, candidates.collect::<Vec<_>>())
    }

    /// The enum values a specification may name where a value of type
    /// `want` is needed: not interchangeable ones.
    fn symbol_candidates(&self, want: Option<&Ty>) -> Vec<&str> {
        let allowed = |id: u32| match want {
            None => true,
            Some(Ty::Enum(ids)) => ids.contains(&id),
            Some(Ty::Variant(cases)) => cases.iter().any(|(tag, _)| *tag == id),
            Some(_) => false,
        };
        self.symbol_ids
            .iter()
            .filter(|(_, id)| !self.unnamed.contains_key(id) && allowed(**id))
            .map(|(name, _)| name.as_str())
            .collect()
    }

    fn map_fields(&self) -> impl Iterator<Item = &str> {
        self.fields
            .iter()
            .filter(|field| matches!(field.kind, FieldType::Map { .. }))
            .map(|field| field.name.as_str())
    }

    /// Named integer, boolean and enum types, which quantifiers, sets and
    /// map keys range over.
    fn scalar_types(&self) -> impl Iterator<Item = &str> {
        self.types
            .iter()
            .filter(|(_, kind)| matches!(kind, FieldType::Scalar(_)))
            .map(|(name, _)| name.as_str())
    }

    fn field(&self, name: &str) -> Option<Place> {
        let field = self
            .field_ids
            .get(name)
            .and_then(|index| self.fields.get(*index))?;
        Some(Place {
            offset: field.offset,
            steps: Vec::new(),
            every: None,
            kind: field.kind.clone(),
            label: field.name.clone(),
        })
    }

    /// `name of key`: the member `name` of the record `key`, or else the
    /// entry `key` of the map field `name`. A record that may be nothing is
    /// read first; reading a member of nothing is a fault.
    fn of(&mut self, name: &str, key: &Expr, line: usize) -> Result<Operand> {
        if !matches!(key, Expr::Every(_)) {
            let base = match self.operand(key, None, line) {
                Ok(Operand::Place(place)) if matches!(place.kind, FieldType::Optional(_)) => self
                    .read(&place, line)
                    .map(|(ir, ty)| Operand::Value(ir, ty)),
                other => other,
            };
            let base = match base {
                Ok(Operand::Value(ir, ty @ Ty::Optional(_))) => {
                    let (ir, ty) = unwrap(ir, ty, &format!("{} is nothing", describe_expr(key)));
                    Ok(Operand::Value(ir, ty))
                }
                Ok(Operand::Place(place))
                    if matches!(
                        place.kind,
                        FieldType::Set { .. } | FieldType::List { .. } | FieldType::Variant { .. }
                    ) =>
                {
                    self.read(&place, line)
                        .map(|(ir, ty)| Operand::Value(ir, ty))
                }
                other => other,
            };
            if let Ok(Operand::Value(ir, ty @ (Ty::Set(_) | Ty::List { .. } | Ty::Variant(_)))) =
                &base
                && let Some(result) = collection_property(name, ir, ty, &describe_expr(key), line)
                    .or_else(|| self.case_member(name, ir, ty, &describe_expr(key), line))
            {
                return result.map(|(ir, ty)| Operand::Value(ir, ty));
            }
            match base {
                Ok(Operand::Place(place)) if place.kind.member(name).is_some() => {
                    return Ok(Operand::Place(place.member(name).unwrap_or(place)));
                }
                Ok(Operand::Value(ir, ty)) if ty.member(name).is_some() => {
                    let (offset, member) = ty.member(name).unwrap_or((0, &ty));
                    let member = member.clone();
                    let width = member.width();
                    let slice = parts(ir).into_iter().skip(offset).take(width).collect();
                    return Ok(Operand::Value(tuple(slice), member));
                }
                Ok(Operand::Place(place)) if self.field(name).is_none() => {
                    let error = match &place.kind {
                        FieldType::Record(members) => {
                            Error::at(line, format!("{} has no member {name}", place.label))
                                .suggest_closest(name, members.iter().map(|(m, _)| m.as_str()))
                        }
                        _ => Error::at(line, format!("unknown map {name}"))
                            .suggest_closest(name, self.map_fields()),
                    };
                    return Err(error.coded(Code::NameUnknown));
                }
                Err(error) if self.field(name).is_none() => return Err(error),
                _ => {}
            }
        }
        let place = self.field(name).ok_or_else(|| {
            Error::at(line, format!("unknown map {name}"))
                .coded(Code::NameUnknown)
                .suggest_closest(name, self.map_fields())
        })?;
        Ok(Operand::Place(self.index(place, key, line)?))
    }

    /// The entry `key` of the map at `place`.
    fn index(&mut self, place: Place, key: &Expr, line: usize) -> Result<Place> {
        let FieldType::Map {
            key_type,
            keys,
            value,
        } = &place.kind
        else {
            return Err(Error::at(line, format!("{} is not a map", place.label)));
        };
        let stride = value.width();
        let mut entry = Place {
            kind: value.as_ref().clone(),
            ..place.clone()
        };
        if let Expr::Every(named) = key {
            if named != key_type {
                return Err(Error::at(
                    line,
                    format!("{} is keyed by {key_type}, not {named}", place.label),
                ));
            }
            if place.every.is_some() || !place.steps.is_empty() {
                return Err(Error::at(
                    line,
                    "'of every' cannot follow a key that is not a constant, or another 'of every'",
                ));
            }
            entry.every = Some((keys.size(), stride));
            return Ok(entry);
        }
        match self.key(key, keys, &place.label, line)? {
            Ir::Const(raw) => {
                entry.offset = key_slot(place.offset, keys, stride, raw, &place.label)
                    .map_err(|message| Error::at(line, message))?;
            }
            key => {
                if place.every.is_some() {
                    return Err(Error::at(
                        line,
                        "after 'of every', every key must be a constant",
                    ));
                }
                entry.steps.push(KeyStep {
                    key,
                    keys: keys.clone(),
                    stride,
                    field: place.label.clone(),
                });
            }
        }
        Ok(entry)
    }

    /// `[a, b]` where a set is expected.
    fn items(&mut self, items: &[Expr], want: Option<&Ty>, line: usize) -> Result<(Ir, Ty)> {
        if let Some(Ty::List { capacity, element }) = want {
            return self.list_literal(items, *capacity, element, line);
        }
        let Some(Ty::Set(elements)) = want else {
            return Err(Error::at(
                line,
                "a [...] literal needs a set on the other side of 'is' or 'becomes'",
            ));
        };
        let elements = elements.clone();
        let mut members = Vec::with_capacity(items.len());
        for item in items {
            members.push(self.element(item, &elements, line)?);
        }
        let held = elements
            .raws()
            .into_iter()
            .map(|raw| {
                members
                    .iter()
                    .map(|member| is_raw(member, raw))
                    .reduce(or)
                    .unwrap_or(Ir::Const(0))
            })
            .collect();
        Ok((tuple(held), Ty::Set(elements)))
    }

    /// `expr` as a value of `elements`, the element type of a set.
    fn element(&mut self, expr: &Expr, elements: &Domain, line: usize) -> Result<Ir> {
        let (ir, ty) = self.expr(expr, line)?;
        if !assignable(&ty, &FieldType::Scalar(elements.clone())) {
            return Err(Error::at(
                line,
                format!(
                    "this set holds {}, not {}",
                    self.describe(elements),
                    self.describe_ty(&ty)
                ),
            )
            .coded(Code::TypeMismatch));
        }
        Ok(ir)
    }

    /// `s has x`, `s with x` and `s without x`, where `x` is a value or a set.
    fn set_operator(
        &mut self,
        operator: BinaryOperator,
        left: &Expr,
        right: &Expr,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        let (set, set_ty) = self.expr_as(left, None, line)?;
        if let Ty::List { capacity, element } = &set_ty {
            let what = describe_expr(left);
            return self.list_operator(operator, set, *capacity, element, right, &what, line);
        }
        let Ty::Set(elements) = set_ty.clone() else {
            let word = match operator {
                BinaryOperator::Has => "'has'",
                BinaryOperator::With => "'with'",
                _ => "'without'",
            };
            return Err(Error::at(
                line,
                format!("{word} needs a set on its left, found {}", set_ty.noun()),
            )
            .coded(Code::TypeMismatch));
        };
        let held = parts(set);
        if operator == BinaryOperator::Has {
            let element = self.element(right, &elements, line)?;
            let what = format!("one of {}", self.describe(&elements));
            return Ok((select(element, &elements, held, what), Ty::Bool));
        }
        let with = operator == BinaryOperator::With;
        let (other, other_ty) = self.expr_as(right, Some(&set_ty), line)?;
        let combined = if other_ty == set_ty {
            held.into_iter()
                .zip(parts(other))
                .map(|(mine, theirs)| {
                    if with {
                        or(mine, theirs)
                    } else {
                        and(mine, Ir::Not(Box::new(theirs)))
                    }
                })
                .collect()
        } else {
            let element = self.element(right, &elements, line)?;
            held.into_iter()
                .zip(elements.raws())
                .map(|(mine, raw)| {
                    let hit = is_raw(&element, raw);
                    match (hit, with) {
                        (Ir::Const(0), _) => mine,
                        (Ir::Const(_), true) => Ir::Const(1),
                        (Ir::Const(_), false) => Ir::Const(0),
                        (hit, true) => or(hit, mine),
                        (hit, false) => and(Ir::Not(Box::new(hit)), mine),
                    }
                })
                .collect()
        };
        Ok((tuple(combined), set_ty))
    }

    fn binary(
        &mut self,
        operator: BinaryOperator,
        left: &Expr,
        right: &Expr,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        if matches!(
            operator,
            BinaryOperator::Has | BinaryOperator::With | BinaryOperator::Without
        ) {
            return self.set_operator(operator, left, right, line);
        }
        let ((left, left_ty), (right, right_ty)) =
            if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
                self.pair(left, right, None, line)?
            } else {
                (self.expr(left, line)?, self.expr(right, line)?)
            };
        if !left_ty.is_scalar() || !right_ty.is_scalar() {
            return self.compare_compound(operator, (left, left_ty), (right, right_ty), line);
        }
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
                        .coded(Code::TypeMismatch));
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
            BinaryOperator::Has | BinaryOperator::With | BinaryOperator::Without => {
                return Err(Error::at(
                    line,
                    "internal error: a set operator was not compiled",
                ));
            }
        };
        Ok((Ir::Binary(operator, Box::new(left), Box::new(right)), ty))
    }

    /// `=` and `!=` on records and maps: every slot equal. Encodings are
    /// canonical, so equal values have equal slots.
    fn compare_compound(
        &self,
        operator: BinaryOperator,
        (left, left_ty): (Ir, Ty),
        (right, right_ty): (Ir, Ty),
        line: usize,
    ) -> Result<(Ir, Ty)> {
        let negate = match operator {
            BinaryOperator::Equal => false,
            BinaryOperator::NotEqual => true,
            _ => {
                let culprit = if left_ty.is_scalar() {
                    &right_ty
                } else {
                    &left_ty
                };
                return Err(Error::at(
                    line,
                    format!("{} cannot be used with this operator", culprit.noun()),
                )
                .coded(Code::TypeMismatch));
            }
        };
        if let (Ty::Variant(_), Ty::Enum(ids)) | (Ty::Enum(ids), Ty::Variant(_)) =
            (&left_ty, &right_ty)
        {
            let tag = if left_ty.is_scalar() { right } else { left };
            let tag = parts(tag).into_iter().next().unwrap_or(Ir::Const(0));
            let is_case = ids
                .iter()
                .map(|id| is_raw(&tag, i64::from(*id)))
                .reduce(or)
                .unwrap_or(Ir::Const(0));
            return Ok((
                if negate {
                    Ir::Not(Box::new(is_case))
                } else {
                    is_case
                },
                Ty::Bool,
            ));
        }
        let Some(joined) = left_ty.join(&right_ty) else {
            return Err(Error::at(
                line,
                format!(
                    "cannot compare {} with {}",
                    self.describe_ty(&left_ty),
                    self.describe_ty(&right_ty)
                ),
            )
            .coded(Code::TypeMismatch));
        };
        let equal = equal_as(
            coerce(parts(left), &left_ty, &joined),
            coerce(parts(right), &right_ty, &joined),
            &joined,
        );
        let ir = if negate {
            Ir::Not(Box::new(equal))
        } else {
            equal
        };
        Ok((ir, Ty::Bool))
    }

    /// `every x in s satisfies ...` over the members of the set field `s`.
    fn over_set(
        &mut self,
        quantifier: Quantifier,
        variable: &str,
        place: &Place,
        body: &Expr,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        let FieldType::Set { elements, .. } = &place.kind else {
            return Err(Error::at(
                line,
                format!(
                    "a quantifier ranges over a type or a set, and {} is neither",
                    place.label
                ),
            ));
        };
        let elements = elements.clone();
        let (held, _) = self.read(place, line)?;
        let local = self.scope.len();
        self.scope.push((variable.to_string(), Ty::of(&elements)));
        self.locals = self.locals.max(self.scope.len());
        let body = self.expr(body, line);
        self.scope.pop();
        let (body, ty) = body?;
        expect(&ty, &Ty::Bool, "the condition of a quantifier", line)?;
        let what = format!("one of {}", self.describe(&elements));
        let member = select(Ir::Local(local), &elements, parts(held), what);
        let body = match quantifier {
            Quantifier::Every => {
                Ir::Binary(BinaryOperator::Implies, Box::new(member), Box::new(body))
            }
            Quantifier::Some | Quantifier::Count => and(member, body),
        };
        Ok((
            Ir::Quantified {
                quantifier,
                local,
                values: elements.raws(),
                body: Box::new(body),
            },
            if quantifier == Quantifier::Count {
                Ty::Int
            } else {
                Ty::Bool
            },
        ))
    }

    /// `[a, b]` where a list is expected.
    fn list_literal(
        &mut self,
        items: &[Expr],
        capacity: usize,
        element: &Ty,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        if items.len() > capacity {
            return Err(Error::at(
                line,
                format!(
                    "this list holds at most {capacity} items, and {} are given",
                    items.len()
                ),
            ));
        }
        let mut out = vec![Ir::Const(i64::try_from(items.len()).unwrap_or(i64::MAX))];
        for item in items {
            out.extend(self.item(item, element, line)?);
        }
        out.extend(
            std::iter::repeat_with(|| Ir::Const(0))
                .take((capacity - items.len()) * element.width()),
        );
        Ok((
            tuple(out),
            Ty::List {
                capacity,
                element: Box::new(element.clone()),
            },
        ))
    }

    /// `expr` laid out as an item of type `element`.
    fn item(&mut self, expr: &Expr, element: &Ty, line: usize) -> Result<Vec<Ir>> {
        let (ir, ty) = self.expr_as(expr, Some(element), line)?;
        if ty.join(element).as_ref() != Some(element) {
            return Err(Error::at(
                line,
                format!(
                    "this list holds {}, not {}",
                    self.describe_ty(element),
                    self.describe_ty(&ty)
                ),
            )
            .coded(Code::TypeMismatch));
        }
        Ok(coerce(parts(ir), &ty, element))
    }

    /// `l has x`, `l with x` (append) and `l without x` (remove every `x`).
    #[allow(clippy::too_many_arguments)]
    fn list_operator(
        &mut self,
        operator: BinaryOperator,
        list: Ir,
        capacity: usize,
        element: &Ty,
        right: &Expr,
        what: &str,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        let list_ty = Ty::List {
            capacity,
            element: Box::new(element.clone()),
        };
        let (length, items) = split_list(list, element.width());
        let value = self.item(right, element, line)?;
        let same = |item: &[Ir]| equal_as(item.to_vec(), value.clone(), element);
        match operator {
            BinaryOperator::Has => Ok((
                items
                    .iter()
                    .enumerate()
                    .map(|(position, item)| and(longer(&length, position), same(item)))
                    .reduce(or)
                    .unwrap_or(Ir::Const(0)),
                Ty::Bool,
            )),
            BinaryOperator::With => {
                let room = Ir::Binary(
                    BinaryOperator::Less,
                    Box::new(length.clone()),
                    Box::new(Ir::Const(i64::try_from(capacity).unwrap_or(i64::MAX))),
                );
                let grown = Ir::Binary(
                    BinaryOperator::Add,
                    Box::new(length.clone()),
                    Box::new(Ir::Const(1)),
                );
                let full = Ir::Fail(format!("{what} is full"));
                let mut out = vec![Ir::If(Box::new(room), Box::new(grown), Box::new(full))];
                for (position, item) in items.into_iter().enumerate() {
                    let here = is_raw(&length, i64::try_from(position).unwrap_or(i64::MAX));
                    out.extend(item.into_iter().zip(&value).map(|(old, new)| match &here {
                        Ir::Const(0) => old,
                        Ir::Const(_) => new.clone(),
                        here => {
                            Ir::If(Box::new(here.clone()), Box::new(new.clone()), Box::new(old))
                        }
                    }));
                }
                Ok((tuple(out), list_ty))
            }
            _ => {
                if capacity
                    .saturating_mul(capacity)
                    .saturating_mul(element.width())
                    > MAX_SLOTS
                {
                    return Err(Error::at(
                        line,
                        format!("'without' needs a smaller list than {what}"),
                    ));
                }
                let kept: Vec<Ir> = items
                    .iter()
                    .enumerate()
                    .map(|(position, item)| {
                        and(longer(&length, position), Ir::Not(Box::new(same(item))))
                    })
                    .collect();
                let add = |left: Ir, right: Ir| {
                    Ir::Binary(BinaryOperator::Add, Box::new(left), Box::new(right))
                };
                let before: Vec<Ir> = (0..capacity)
                    .map(|position| {
                        kept.iter()
                            .take(position)
                            .cloned()
                            .reduce(add)
                            .unwrap_or(Ir::Const(0))
                    })
                    .collect();
                let mut out = vec![kept.iter().cloned().reduce(add).unwrap_or(Ir::Const(0))];
                for target in 0..capacity {
                    let target_raw = i64::try_from(target).unwrap_or(i64::MAX);
                    let mut chosen = vec![Ir::Const(0); element.width()];
                    for source in (target..capacity).rev() {
                        let lands = and(kept[source].clone(), is_raw(&before[source], target_raw));
                        for (slot, part) in chosen.iter_mut().zip(&items[source]) {
                            *slot = Ir::If(
                                Box::new(lands.clone()),
                                Box::new(part.clone()),
                                Box::new(std::mem::replace(slot, Ir::Const(0))),
                            );
                        }
                    }
                    out.extend(chosen);
                }
                Ok((tuple(out), list_ty))
            }
        }
    }

    /// `every m in l satisfies ...` over the items of the list field `l`.
    fn over_list(
        &mut self,
        quantifier: Quantifier,
        variable: &str,
        place: &Place,
        body: &Expr,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        let FieldType::List { capacity, element } = &place.kind else {
            return Err(Error::at(line, "internal error: not a list"));
        };
        let (capacity, element) = (*capacity, Ty::of_kind(element));
        let (list, _) = self.read(place, line)?;
        let (length, items) = split_list(list, element.width());
        let local = self.scope.len();
        let at = positions(capacity);
        let item = (0..element.width())
            .map(|part| {
                select(
                    Ir::Local(local),
                    &at,
                    items.iter().map(|item| item[part].clone()).collect(),
                    format!("a position in {}", place.label),
                )
            })
            .collect();
        self.scope.push((variable.to_string(), element.clone()));
        self.views.push((local, tuple(item), element));
        self.locals = self.locals.max(self.scope.len());
        let body = self.expr(body, line);
        self.views.pop();
        self.scope.pop();
        let (body, ty) = body?;
        expect(&ty, &Ty::Bool, "the condition of a quantifier", line)?;
        let present = Ir::Binary(
            BinaryOperator::Greater,
            Box::new(length),
            Box::new(Ir::Local(local)),
        );
        let body = match quantifier {
            Quantifier::Every => {
                Ir::Binary(BinaryOperator::Implies, Box::new(present), Box::new(body))
            }
            Quantifier::Some | Quantifier::Count => and(present, body),
        };
        Ok((
            Ir::Quantified {
                quantifier,
                local,
                values: at.raws(),
                body: Box::new(body),
            },
            if quantifier == Quantifier::Count {
                Ty::Int
            } else {
                Ty::Bool
            },
        ))
    }

    /// The case named by `expr` when it is a bare name of a case with members.
    fn case_name(&self, expr: &Expr) -> Option<String> {
        let name = match expr {
            Expr::Name { name, .. } if self.field(name).is_none() => name,
            Expr::Literal(Value::Symbol(name)) => name,
            _ => return None,
        };
        if self.scope.iter().any(|(local, _)| local == name) {
            return None;
        }
        let id = *self.symbol_ids.get(name)?;
        self.variants
            .iter()
            .any(|ty| matches!(ty, Ty::Variant(cases) if cases.iter().any(|(tag, members)| *tag == id && !members.is_empty())))
            .then(|| name.clone())
    }

    /// `running with {attempts: 1}`: a value of the enumeration that has the
    /// case, the one the context expects when several do.
    fn construct(
        &mut self,
        tag: &str,
        members: &[(String, Expr)],
        want: Option<&Ty>,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        let id = self.symbol_ids.get(tag).copied().unwrap_or(u32::MAX);
        let has =
            |ty: &Ty| matches!(ty, Ty::Variant(cases) if cases.iter().any(|(case, _)| *case == id));
        let ty = match want {
            Some(want) if has(want) => want.clone(),
            _ => {
                let mut candidates = self.variants.iter().filter(|ty| has(ty));
                let (Some(first), None) = (candidates.next(), candidates.next()) else {
                    return Err(Error::at(
                        line,
                        format!(
                            "several enumerations have the value {tag}; compare or assign it to say which"
                        ),
                    ));
                };
                first.clone()
            }
        };
        let Ty::Variant(cases) = &ty else {
            return Err(Error::at(
                line,
                "internal error: not an enumeration with members",
            ));
        };
        let mut out = vec![Ir::Const(i64::from(id))];
        for (case, fields) in cases {
            let record = Ty::Record(fields.clone());
            if *case == id {
                let (ir, value) = self.record(members, Some(&record), line)?;
                if value.join(&record).is_none() {
                    return Err(Error::at(
                        line,
                        format!(
                            "{tag} has members {}, but this gives {}",
                            self.describe_ty(&record),
                            self.describe_ty(&value)
                        ),
                    )
                    .coded(Code::TypeMismatch));
                }
                out.extend(coerce(parts(ir), &value, &record));
            } else {
                out.extend(std::iter::repeat_with(|| Ir::Const(0)).take(record.width()));
            }
        }
        Ok((tuple(out), ty))
    }

    /// `attempts of phase`: a member of the case `phase` holds. Reading it
    /// while `phase` holds a case without that member is a fault.
    fn case_member(
        &self,
        name: &str,
        ir: &Ir,
        ty: &Ty,
        what: &str,
        line: usize,
    ) -> Option<Result<(Ir, Ty)>> {
        let Ty::Variant(cases) = ty else {
            return None;
        };
        let (tag, payloads) = split_variant(parts(ir.clone()), cases);
        let mut holders = Vec::new();
        let mut member_ty: Option<Ty> = None;
        for ((case, members), payload) in cases.iter().zip(&payloads) {
            let record = Ty::Record(members.clone());
            let Some((offset, member)) = record.member(name) else {
                continue;
            };
            member_ty = match member_ty {
                None => Some(member.clone()),
                Some(seen) => match seen.join(member) {
                    Some(joined) => Some(joined),
                    None => {
                        return Some(Err(Error::at(
                            line,
                            format!("{name} has a different type in different cases of {what}"),
                        )));
                    }
                },
            };
            let slice: Vec<Ir> = payload
                .iter()
                .skip(offset)
                .take(member.width())
                .cloned()
                .collect();
            holders.push((*case, slice));
        }
        let Some(member_ty) = member_ty else {
            return Some(Err(Error::at(
                line,
                format!("no value of {what} has a member {name}"),
            )));
        };
        let names: Vec<String> = holders
            .iter()
            .map(|(case, _)| self.symbols.get(at(*case)).cloned().unwrap_or_default())
            .collect();
        let missing = Ir::Absent(format!(
            "{what} is not {}, so it has no {name}",
            names.join(" or ")
        ));
        let read = (0..member_ty.width())
            .map(|part| {
                holders
                    .iter()
                    .rev()
                    .fold(missing.clone(), |otherwise, (case, slice)| {
                        Ir::If(
                            Box::new(is_raw(&tag, i64::from(*case))),
                            Box::new(slice.get(part).cloned().unwrap_or(Ir::Const(0))),
                            Box::new(otherwise),
                        )
                    })
            })
            .collect();
        Some(Ok((tuple(read), member_ty)))
    }

    /// `value or else default`: every slot of `value`, or of `default` where
    /// reading `value` finds an absent value.
    fn or_else(&mut self, value: &Expr, default: &Expr, line: usize) -> Result<(Ir, Ty)> {
        let (read, ty) = self.expr(value, line)?;
        if !can_be_absent(&read) {
            return Err(Error::at(
                line,
                format!(
                    "'or else' gives a default for a value that can be absent: the value of an optional, a member of an enumeration case, or first, last or rest of a list; {} is always there",
                    describe_expr(value)
                ),
            ));
        }
        let (fallback, fallback_ty) = self.expr_as(default, Some(&ty), line)?;
        let (fallback, fallback_ty) = unwrap(
            fallback,
            fallback_ty,
            &format!("{} is nothing", describe_expr(default)),
        );
        let Some(joined) = ty.join(&fallback_ty) else {
            return Err(Error::at(
                line,
                format!(
                    "the default after 'or else' must be {}, like {}, not {}",
                    ty.noun(),
                    describe_expr(value),
                    fallback_ty.noun()
                ),
            )
            .coded(Code::TypeMismatch));
        };
        let read = coerce(parts(read), &ty, &joined);
        let fallback = coerce(parts(fallback), &fallback_ty, &joined);
        let probe = if read.len() > 1 {
            read.clone()
        } else {
            Vec::new()
        };
        let slots = read
            .into_iter()
            .zip(fallback)
            .map(|(value, default)| Ir::Recover {
                probe: probe.clone(),
                value: Box::new(value),
                default: Box::new(default),
            })
            .collect();
        Ok((tuple(slots), joined))
    }

    fn quantified(
        &mut self,
        quantifier: Quantifier,
        variable: &str,
        domain: &str,
        body: &Expr,
        line: usize,
    ) -> Result<(Ir, Ty)> {
        if !self.types.contains_key(domain)
            && let Some(place) = self.field(domain)
        {
            if matches!(place.kind, FieldType::List { .. }) {
                return self.over_list(quantifier, variable, &place, body, line);
            }
            return self.over_set(quantifier, variable, &place, body, line);
        }
        let domain = match self.types.get(domain) {
            Some(FieldType::Scalar(domain)) => domain.clone(),
            Some(_) => {
                return Err(Error::at(
                    line,
                    format!(
                        "a quantifier ranges over integers, booleans or enums, and {domain} is not one"
                    ),
                ));
            }
            None => {
                let collections = self.fields.iter().filter(|field| {
                    matches!(field.kind, FieldType::Set { .. } | FieldType::List { .. })
                });
                return Err(Error::at(
                    line,
                    format!("unknown type {domain}; declare it in a types block"),
                )
                .coded(Code::NameUnknown)
                .suggest_closest(
                    domain,
                    self.scalar_types()
                        .chain(collections.map(|field| field.name.as_str())),
                ));
            }
        };
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

fn or(left: Ir, right: Ir) -> Ir {
    Ir::Binary(BinaryOperator::Or, Box::new(left), Box::new(right))
}

fn and(left: Ir, right: Ir) -> Ir {
    Ir::Binary(BinaryOperator::And, Box::new(left), Box::new(right))
}

/// Whether `value` is `raw`, folded when `value` is a constant.
fn is_raw(value: &Ir, raw: i64) -> Ir {
    match value {
        Ir::Const(constant) => Ir::Const(i64::from(*constant == raw)),
        value => Ir::Binary(
            BinaryOperator::Equal,
            Box::new(value.clone()),
            Box::new(Ir::Const(raw)),
        ),
    }
}

/// The option for `key`, chosen while compiling when `key` is a constant.
fn select(key: Ir, keys: &Domain, mut options: Vec<Ir>, what: String) -> Ir {
    if let Ir::Const(raw) = key
        && let Some(position) = keys.position(raw)
    {
        return options.swap_remove(position);
    }
    Ir::Select {
        key: Box::new(key),
        keys: keys.clone(),
        options,
        what,
    }
}

/// `size of s`, and `length`, `first`, `last` and `rest of` a list. `None`
/// when `ty` is not a collection. `what` names the collection in faults.
fn collection_property(
    name: &str,
    ir: &Ir,
    ty: &Ty,
    what: &str,
    line: usize,
) -> Option<Result<(Ir, Ty)>> {
    match ty {
        Ty::Set(_) => Some(match name {
            "size" => Ok((
                parts(ir.clone())
                    .into_iter()
                    .reduce(|sum, next| {
                        Ir::Binary(BinaryOperator::Add, Box::new(sum), Box::new(next))
                    })
                    .unwrap_or(Ir::Const(0)),
                Ty::Int,
            )),
            _ => Err(Error::at(
                line,
                format!("a set has no {name}; 'size of' counts its members"),
            )),
        }),
        Ty::List { capacity, element } => {
            let (length, items) = split_list(ir.clone(), element.width());
            let empty = format!("{what} is empty");
            let guarded = |part: Ir| {
                Ir::If(
                    Box::new(longer(&length, 0)),
                    Box::new(part),
                    Box::new(Ir::Absent(empty.clone())),
                )
            };
            Some(match name {
                "length" => Ok((length.clone(), Ty::Int)),
                "first" => Ok((
                    tuple(
                        items
                            .first()
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .map(guarded)
                            .collect(),
                    ),
                    (**element).clone(),
                )),
                "last" => {
                    let at = positions(*capacity);
                    let last = Ir::Binary(
                        BinaryOperator::Subtract,
                        Box::new(length.clone()),
                        Box::new(Ir::Const(1)),
                    );
                    let item = (0..element.width())
                        .map(|part| {
                            guarded(select(
                                last.clone(),
                                &at,
                                items.iter().map(|item| item[part].clone()).collect(),
                                format!("a position in {what}"),
                            ))
                        })
                        .collect();
                    Ok((tuple(item), (**element).clone()))
                }
                "rest" => {
                    let mut out = vec![guarded(Ir::Binary(
                        BinaryOperator::Subtract,
                        Box::new(length.clone()),
                        Box::new(Ir::Const(1)),
                    ))];
                    for item in items.iter().skip(1) {
                        out.extend(item.iter().cloned());
                    }
                    out.extend(std::iter::repeat_with(|| Ir::Const(0)).take(element.width()));
                    Ok((tuple(out), ty.clone()))
                }
                _ => Err(Error::at(
                    line,
                    format!("a list has no {name}; use length, first, last or rest"),
                )),
            })
        }
        _ => None,
    }
}

/// The positions of a list: 0 up to `capacity - 1`.
fn positions(capacity: usize) -> Domain {
    Domain::Integer {
        min: 0,
        max: i64::try_from(capacity).unwrap_or(i64::MAX) - 1,
    }
}

/// A list value's length and the parts of each item.
fn split_list(ir: Ir, width: usize) -> (Ir, Vec<Vec<Ir>>) {
    let mut parts = parts(ir).into_iter();
    let length = parts.next().unwrap_or(Ir::Const(0));
    let rest: Vec<Ir> = parts.collect();
    (
        length,
        rest.chunks(width.max(1)).map(<[Ir]>::to_vec).collect(),
    )
}

fn expect(actual: &Ty, wanted: &Ty, what: &str, line: usize) -> Result<()> {
    if actual == wanted {
        Ok(())
    } else {
        Err(Error::at(
            line,
            format!("{what} needs {}, found {}", wanted.noun(), actual.noun()),
        )
        .coded(Code::TypeMismatch))
    }
}

/// Whether a value of type `ty` can be stored as `kind`. Integer ranges are
/// checked when the value is written.
fn assignable(ty: &Ty, kind: &FieldType) -> bool {
    match (ty, kind) {
        (Ty::Int, FieldType::Scalar(Domain::Integer { .. }))
        | (Ty::Bool, FieldType::Scalar(Domain::Boolean)) => true,
        (Ty::Enum(ids), FieldType::Scalar(Domain::Enum(allowed))) => {
            ids.iter().all(|id| allowed.contains(id))
        }
        (Ty::Record(members), FieldType::Record(wanted)) => {
            members.len() == wanted.len()
                && members
                    .iter()
                    .zip(wanted)
                    .all(|((name, ty), (other, kind))| name == other && assignable(ty, kind))
        }
        (
            Ty::Map { keys, value },
            FieldType::Map {
                keys: wanted_keys,
                value: wanted,
                ..
            },
        ) => keys == wanted_keys && assignable(value, wanted),
        (
            Ty::Set(elements),
            FieldType::Set {
                elements: wanted, ..
            },
        ) => elements == wanted,
        (
            Ty::List { capacity, element },
            FieldType::List {
                capacity: wanted_capacity,
                element: wanted,
            },
        ) => capacity == wanted_capacity && assignable(element, wanted),
        (Ty::Variant(cases), FieldType::Variant { cases: wanted, .. }) => {
            cases.len() == wanted.len()
                && cases.iter().zip(wanted).all(|((_, members), (_, wanted))| {
                    assignable(
                        &Ty::Record(members.clone()),
                        &FieldType::Record(wanted.clone()),
                    )
                })
        }
        (Ty::Enum(ids), FieldType::Variant { .. }) => {
            let Ty::Variant(cases) = Ty::of_kind(kind) else {
                return false;
            };
            bare_tags(&cases, ids)
        }
        (Ty::Nothing, FieldType::Optional(_)) => true,
        (Ty::Optional(value), FieldType::Optional(wanted)) => assignable(value, wanted),
        (value, FieldType::Optional(wanted)) => assignable(value, wanted),
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
    let index = model.action_index(&call.name).ok_or_else(|| {
        Error::at(line, format!("unknown action {}", call.name))
            .coded(Code::NameUnknown)
            .suggest_closest(
                &call.name,
                model.actions.iter().map(|action| action.name.as_str()),
            )
    })?;
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
                return Err(
                    Error::at(line, format!("{} has no parameter {unknown}", action.name))
                        .coded(Code::NameUnknown)
                        .suggest_closest(unknown, action.params.iter().map(|p| p.name.as_str())),
                );
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
