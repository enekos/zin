use crate::ast::{
    Action, ActionCall, Assignment, BinaryOperator, CallArguments, Condition, Expr, Fairness,
    PropertyKind, Quantifier, ScenarioStep, Spec, TypeDecl, TypeExpr, UnaryOperator, Value,
    display_call,
};
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
        }
    }

    /// Whether `plan/1` can carry this type: a scalar, or a map of scalars.
    pub(crate) fn is_classic(&self) -> bool {
        match self {
            Self::Scalar(_) => true,
            Self::Map { value, .. } => matches!(value.as_ref(), Self::Scalar(_)),
            Self::Record(_) => false,
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
    let mut slot = offset;
    for step in steps {
        let key = eval(&step.key, state, locals)?;
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

pub(crate) fn eval(ir: &Ir, state: &[i64], locals: &mut [i64]) -> std::result::Result<i64, String> {
    Ok(match ir {
        Ir::Const(raw) => *raw,
        Ir::Slot(slot) => fetch(state, *slot)?,
        Ir::Local(local) => fetch(locals, *local)?,
        Ir::Index { offset, steps } => fetch(state, locate(*offset, steps, state, locals)?)?,
        Ir::Tuple(_) => return Err("internal error: a compound value was evaluated".to_string()),
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

/// The static type of an expression. Compound types are structural: two
/// records agree when their members agree name by name, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ty {
    Int,
    Bool,
    Enum(BTreeSet<u32>),
    Record(Vec<(String, Ty)>),
    Map { keys: Domain, value: Box<Ty> },
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
        }
    }

    fn noun(&self) -> &'static str {
        match self {
            Self::Int => "a number",
            Self::Bool => "a true/false value",
            Self::Enum(_) => "an enum value",
            Self::Record(_) => "a record",
            Self::Map { .. } => "a map",
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
        }
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

fn record_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Record(_))
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
                        ));
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
            .coded("type.mismatch")
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
            .coded("type.mismatch"));
        }
        Ok(place
            .targets()
            .into_iter()
            .zip(parts(value))
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
            .coded("type.mismatch"));
        }
        Ok(ir)
    }

    fn expr(&mut self, expr: &Expr, line: usize) -> Result<(Ir, Ty)> {
        self.expr_as(expr, None, line)
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
            Expr::Name { name, span } => self.name(name, line).map_err(|e| e.within(*span)),
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
                None => Err(Error::at(line, format!("unknown enum value \"{name}\""))),
            },
            Expr::Literal(Value::Map(_)) => Err(Error::at(line, "map literals are not supported")),
            Expr::Name { .. } | Expr::Index { .. } | Expr::Subscript { .. } => {
                self.expr_as(expr, want, line)
            }
            Expr::Every(domain) => Err(Error::at(
                line,
                format!("'every {domain}' names all keys at once, so it can only be assigned to"),
            )),
            Expr::Record(members) => self.record(members, want, line),
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
                let branches = parts(then)
                    .into_iter()
                    .zip(parts(otherwise))
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
                    return Err(Error::at(
                        line,
                        format!("this record has no member {extra}"),
                    ));
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
            return Err(Error::at(line, format!("{extra} is not a key of this map")));
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
        .coded("name.unknown"))
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
    /// entry `key` of the map field `name`.
    fn of(&mut self, name: &str, key: &Expr, line: usize) -> Result<Operand> {
        if !matches!(key, Expr::Every(_)) {
            let base = self.operand(key, None, line);
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
                    return Err(Error::at(
                        line,
                        match place.kind {
                            FieldType::Record(_) => format!("{} has no member {name}", place.label),
                            _ => format!("unknown map {name}"),
                        },
                    )
                    .coded("name.unknown"));
                }
                Err(error) if self.field(name).is_none() => return Err(error),
                _ => {}
            }
        }
        let place = self
            .field(name)
            .ok_or_else(|| Error::at(line, format!("unknown map {name}")).coded("name.unknown"))?;
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

    fn binary(
        &mut self,
        operator: BinaryOperator,
        left: &Expr,
        right: &Expr,
        line: usize,
    ) -> Result<(Ir, Ty)> {
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
                        .coded("type.mismatch"));
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
                .coded("type.mismatch"));
            }
        };
        if left_ty.join(&right_ty).is_none() {
            return Err(Error::at(
                line,
                format!(
                    "cannot compare {} with {}",
                    self.describe_ty(&left_ty),
                    self.describe_ty(&right_ty)
                ),
            )
            .coded("type.mismatch"));
        }
        let equal = parts(left)
            .into_iter()
            .zip(parts(right))
            .map(|(left, right)| Ir::Binary(BinaryOperator::Equal, Box::new(left), Box::new(right)))
            .reduce(|all, next| Ir::Binary(BinaryOperator::And, Box::new(all), Box::new(next)))
            .unwrap_or(Ir::Const(1));
        let ir = if negate {
            Ir::Not(Box::new(equal))
        } else {
            equal
        };
        Ok((ir, Ty::Bool))
    }

    fn quantified(
        &mut self,
        quantifier: Quantifier,
        variable: &str,
        domain: &str,
        body: &Expr,
        line: usize,
    ) -> Result<(Ir, Ty)> {
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
                return Err(Error::at(
                    line,
                    format!("unknown type {domain}; declare it in a types block"),
                )
                .coded("name.unknown"));
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

fn expect(actual: &Ty, wanted: &Ty, what: &str, line: usize) -> Result<()> {
    if actual == wanted {
        Ok(())
    } else {
        Err(Error::at(
            line,
            format!("{what} needs {}, found {}", wanted.noun(), actual.noun()),
        )
        .coded("type.mismatch"))
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
