use crate::ast::{
    Action, ActionCall, Assignment, BinaryOperator, CallArguments, Condition, Expr, Fairness,
    Field, Property, PropertyKind, Quantifier, Scenario, ScenarioStep, Span, Spec, TypeDecl,
    TypeExpr, UnaryOperator, Value, is_reserved_word,
};
use crate::{Error, Result};
use std::collections::BTreeMap;

const I64_MIN_MAGNITUDE: u64 = 9_223_372_036_854_775_808;
const MAX_NESTING: usize = 64;
const MAX_TOKENS_PER_LINE: usize = 1024;
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Integer(u64),
    Text(String),
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Colon,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Equal,
    EqualEqual,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Debug)]
struct Line {
    number: usize,
    indent: usize,
    /// The column where `content` starts.
    column: usize,
    content: String,
}

impl Line {
    /// The position of `part`, a slice of `content`.
    fn at(&self, part: &str) -> At {
        let offset = part
            .as_ptr()
            .addr()
            .checked_sub(self.content.as_ptr().addr())
            .and_then(|offset| self.content.get(..offset))
            .map_or(0, |before| before.chars().count());
        At {
            line: self.number,
            column: self.column + offset,
        }
    }
}

/// Where a piece of source text starts.
#[derive(Debug, Clone, Copy)]
struct At {
    line: usize,
    column: usize,
}

impl At {
    fn span(self, text: &str) -> Span {
        Span {
            line: self.line,
            column: self.column,
            end_column: self.column + text.trim_end().chars().count(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Section {
    None,
    Types,
    State,
    Initially,
    Properties(PropertyKind),
    Action(usize),
    Scenario(usize),
}

#[derive(Default)]
struct Seen {
    system: bool,
    state: bool,
    initially: bool,
}

pub(crate) fn fingerprint(source: &str) -> String {
    let mut hash = FNV_OFFSET;
    for byte in source.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

/// Parses a specification. Several `system` blocks in one file make a
/// composition: see [`crate::compose`].
///
/// # Errors
///
/// Returns the first syntax error, with its line, or a composition error.
pub fn parse(source: &str) -> Result<Spec> {
    let map = crate::source::SourceMap::inline(source);
    let mut spec = parse_unlocated(source).map_err(|e| map.locate(e))?;
    spec.source_map = Some(map);
    Ok(spec)
}

/// As [`parse`], with errors in the coordinates of `source`.
pub(crate) fn parse_unlocated(source: &str) -> Result<Spec> {
    let lines = logical_lines(source)?;
    let mut components: Vec<Vec<Line>> = Vec::new();
    for line in lines {
        let starts_system = line.indent == 0 && after_keyword(&line.content, "system").is_some();
        match components.last_mut() {
            Some(component) if !starts_system => component.push(line),
            _ => components.push(vec![line]),
        }
    }
    let composed = components.len() > 1;
    let mut specs = Vec::with_capacity(components.len());
    for lines in components {
        specs.push(parse_component(lines, composed)?);
    }
    let mut spec = if specs.len() == 1 {
        specs.pop().unwrap_or_else(Spec::empty)
    } else if specs.is_empty() {
        parse_component(Vec::new(), false)?
    } else {
        crate::compose::compose(specs)?
    };
    spec.fingerprint = fingerprint(source);
    Ok(spec)
}

/// One `system` block. In a composition a block may leave out state, to hold
/// properties and scenarios about the whole.
fn parse_component(lines: Vec<Line>, composed: bool) -> Result<Spec> {
    let mut spec = Spec::empty();
    let mut section = Section::None;
    let mut seen = Seen::default();

    for line in lines {
        if line.indent == 0 {
            section = parse_top_level(&mut spec, &line, &mut seen)?;
            continue;
        }
        parse_block_line(&mut spec, section, &line)?;
    }

    if !seen.system {
        return Err(Error::new("a specification must start with system \"...\""));
    }
    if composed && !seen.state && !seen.initially {
        return Ok(spec);
    }
    let fail = |message: &str| match composed {
        true => Error::new(format!("{}: {message}", spec.name)),
        false => Error::new(message),
    };
    if !seen.state || spec.state.is_empty() {
        return Err(fail("declare at least one field in a state block"));
    }
    if !seen.initially {
        return Err(fail("declare an initially block"));
    }
    Ok(spec)
}

fn parse_block_line(spec: &mut Spec, section: Section, line: &Line) -> Result<()> {
    let content = line.content.as_str();
    match section {
        Section::Types => {
            let (interchangeable, content) = match after_keyword(content, "interchangeable") {
                Some(rest) => (true, rest),
                None => (false, content),
            };
            let field = parse_field(content, line.at(content))?;
            spec.types.push(TypeDecl {
                name: field.name,
                kind: field.kind,
                interchangeable,
                line: field.line,
                span: field.span,
            });
        }
        Section::State => spec.state.push(parse_field(content, line.at(content))?),
        Section::Initially => {
            spec.initially
                .push(parse_assignment(content, "is", line.at(content))?);
        }
        Section::Properties(kind) => spec.properties.push(Property {
            kind,
            premise: None,
            condition: parse_condition(content, line.at(content))?,
            line: line.number,
        }),
        Section::Action(index) => {
            let action = spec
                .actions
                .get_mut(index)
                .ok_or_else(|| Error::at(line.number, "missing action"))?;
            if let Some(source) = after_keyword(content, "requires") {
                if source.is_empty() {
                    return Err(Error::at(line.number, "requires needs a condition"));
                }
                action
                    .requires
                    .push(parse_condition(source, line.at(source))?);
            } else {
                action
                    .assignments
                    .push(parse_assignment(content, "becomes", line.at(content))?);
            }
        }
        Section::Scenario(index) => {
            let scenario = spec
                .scenarios
                .get_mut(index)
                .ok_or_else(|| Error::at(line.number, "missing scenario"))?;
            let step = if let Some(source) = after_keyword(content, "expect") {
                if source.is_empty() {
                    return Err(Error::at(line.number, "expect needs a condition"));
                }
                ScenarioStep::Expect(parse_condition(source, line.at(source))?)
            } else if let Some(source) = after_keyword(content, "reject") {
                if source.is_empty() {
                    return Err(Error::at(line.number, "reject needs an action call"));
                }
                ScenarioStep::Reject(parse_call(source, line.at(source))?)
            } else {
                ScenarioStep::Call(parse_call(content, line.at(content))?)
            };
            scenario.steps.push(step);
        }
        Section::None => {
            return Err(Error::at(
                line.number,
                "this indented line does not belong to a block",
            ));
        }
    }
    Ok(())
}

fn parse_top_level(spec: &mut Spec, line: &Line, seen: &mut Seen) -> Result<Section> {
    let content = line.content.as_str();
    if let Some(rest) = after_keyword(content, "system") {
        if seen.system {
            return Err(Error::at(line.number, "system may only be declared once"));
        }
        spec.name = parse_standalone_name(rest, line.at(rest))?;
        seen.system = true;
        return Ok(Section::None);
    }
    if after_keyword(content, "include").is_some() {
        return Err(Error::at(
            line.number,
            "include needs a file: load the specification with parse_file or the zin command",
        ));
    }
    if !seen.system {
        return Err(Error::at(
            line.number,
            "the first declaration must be system \"...\"",
        ));
    }
    match content {
        "types" => return Ok(Section::Types),
        "state" => {
            if seen.state {
                return Err(Error::at(line.number, "state may only be declared once"));
            }
            seen.state = true;
            return Ok(Section::State);
        }
        "initially" => {
            if seen.initially {
                return Err(Error::at(
                    line.number,
                    "initially may only be declared once",
                ));
            }
            seen.initially = true;
            return Ok(Section::Initially);
        }
        _ => {}
    }
    if let Some(section) = parse_property_declaration(spec, line)? {
        return Ok(section);
    }
    if let Some(rest) = after_keyword(content, "terminal") {
        let source = after_keyword(rest, "when")
            .filter(|source| !source.is_empty())
            .ok_or_else(|| Error::at(line.number, "expected 'terminal when <condition>'"))?;
        if spec.terminal.is_some() {
            return Err(Error::at(line.number, "terminal may only be declared once"));
        }
        spec.terminal = Some(parse_condition(source, line.at(source))?);
        return Ok(Section::None);
    }
    let fairness = if let Some(rest) = after_keyword(content, "strongly") {
        Some((Fairness::Strong, after_keyword(rest, "fair")))
    } else {
        after_keyword(content, "fair").map(|rest| (Fairness::Weak, Some(rest)))
    };
    if let Some((fairness, rest)) = fairness {
        let rest = rest
            .and_then(|rest| after_keyword(rest, "action"))
            .ok_or_else(|| {
                Error::at(
                    line.number,
                    format!("expected '{} action <name>'", fairness.keyword()),
                )
            })?;
        let mut action = parse_action(rest, line.at(rest))?;
        action.fairness = fairness;
        spec.actions.push(action);
        return Ok(Section::Action(spec.actions.len() - 1));
    }
    if let Some(rest) = after_keyword(content, "action") {
        spec.actions.push(parse_action(rest, line.at(rest))?);
        return Ok(Section::Action(spec.actions.len() - 1));
    }
    if let Some(rest) = after_keyword(content, "scenario") {
        let name = parse_standalone_name(rest, line.at(rest))?;
        spec.scenarios.push(Scenario {
            name,
            steps: Vec::new(),
            line: line.number,
        });
        return Ok(Section::Scenario(spec.scenarios.len() - 1));
    }
    if after_keyword(content, "explore").is_some() {
        let mut tokens = Tokens::new(content, line.at(content))?;
        tokens.expect_word("explore")?;
        tokens.expect_word("at")?;
        tokens.expect_word("most")?;
        let limit = tokens.take_nonnegative_integer()?;
        if limit == 0 {
            return Err(Error::at(
                line.number,
                "the state limit must be greater than zero",
            ));
        }
        tokens.expect_word("states")?;
        tokens.finish()?;
        spec.state_limit = limit;
        return Ok(Section::None);
    }
    Err(Error::at(
        line.number,
        format!("unrecognized declaration: {content}"),
    ))
}

fn parse_property_declaration(spec: &mut Spec, line: &Line) -> Result<Option<Section>> {
    let content = line.content.as_str();
    if let Some(rest) = after_keyword(content, "whenever") {
        let (premise, condition) = split_at_word(rest, "eventually")
            .filter(|(premise, condition)| !premise.is_empty() && !condition.is_empty())
            .ok_or_else(|| {
                Error::at(
                    line.number,
                    "expected 'whenever <condition> eventually <condition>'",
                )
            })?;
        spec.properties.push(Property {
            kind: PropertyKind::LeadsTo,
            premise: Some(parse_condition(premise, line.at(premise))?),
            condition: parse_condition(condition, line.at(condition))?,
            line: line.number,
        });
        return Ok(Some(Section::None));
    }
    let (kind, source) = if let Some(rest) = after_keyword(content, "always") {
        match after_keyword(rest, "possible") {
            Some(rest) => (PropertyKind::AlwaysPossible, rest),
            None => (PropertyKind::Always, rest),
        }
    } else if let Some(rest) = after_keyword(content, "never") {
        (PropertyKind::Never, rest)
    } else if let Some(rest) = after_keyword(content, "sometimes") {
        (PropertyKind::Sometimes, rest)
    } else if let Some(rest) = after_keyword(content, "eventually") {
        (PropertyKind::Eventually, rest)
    } else {
        return Ok(None);
    };
    if source.is_empty() {
        return Ok(Some(Section::Properties(kind)));
    }
    spec.properties.push(Property {
        kind,
        premise: None,
        condition: parse_condition(source, line.at(source))?,
        line: line.number,
    });
    Ok(Some(Section::None))
}

fn logical_lines(source: &str) -> Result<Vec<Line>> {
    let mut result = Vec::new();
    for (index, raw) in source.lines().enumerate() {
        let number = index + 1;
        let mut indent = 0;
        for character in raw.chars() {
            match character {
                ' ' => indent += 1,
                '\t' => return Err(Error::at(number, "use spaces, not tabs, for indentation")),
                _ => break,
            }
        }
        let content = strip_comment(raw).trim();
        if content.is_empty() {
            continue;
        }
        let before = raw.len() - raw.trim_start().len();
        result.push(Line {
            number,
            indent,
            column: 1 + raw
                .get(..before)
                .map_or(indent, |text| text.chars().count()),
            content: content.to_string(),
        });
    }
    Ok(result)
}

pub(crate) fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
        } else if character == '#' && !quoted {
            return line.get(..index).unwrap_or(line);
        }
    }
    line
}

fn split_at_word<'a>(source: &'a str, word: &str) -> Option<(&'a str, &'a str)> {
    let mut quoted = false;
    let mut escaped = false;
    let mut boundary = true;
    for (index, character) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted {
            match character {
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
            boundary = false;
            continue;
        }
        if character == '"' {
            quoted = true;
            boundary = false;
            continue;
        }
        let is_word_character = character.is_ascii_alphanumeric() || character == '_';
        if boundary && is_word_character {
            let rest = source.get(index..)?;
            if let Some(after) = rest.strip_prefix(word) {
                let ends = after
                    .chars()
                    .next()
                    .is_none_or(|next| !(next.is_ascii_alphanumeric() || next == '_'));
                if ends {
                    return Some((source.get(..index)?.trim(), after.trim()));
                }
            }
        }
        boundary = !is_word_character;
    }
    None
}

pub(crate) fn after_keyword<'a>(source: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = source.strip_prefix(keyword)?;
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix(char::is_whitespace).map(str::trim_start)
}

fn parse_standalone_name(source: &str, at: At) -> Result<String> {
    let mut tokens = Tokens::new(source, at)?;
    let (name, span) = tokens.take_name()?;
    tokens.finish()?;
    if name.is_empty() {
        return Err(Error::at(at.line, "a name cannot be empty").spanned(span));
    }
    Ok(name)
}

fn parse_condition(source: &str, at: At) -> Result<Condition> {
    let mut tokens = Tokens::new(source, at)?;
    let expr = parse_expression(&mut tokens)?;
    tokens.finish()?;
    Ok(Condition {
        expr,
        source: source.trim().to_string(),
        line: at.line,
        span: at.span(source),
        params: None,
    })
}

fn parse_assignment(source: &str, keyword: &str, at: At) -> Result<Assignment> {
    let mut tokens = Tokens::new(source, at)?;
    let target = tokens.span(0);
    let place = tokens.nested(parse_target)?;
    tokens.expect_word(keyword)?;
    let value = parse_expression(&mut tokens)?;
    tokens.finish()?;
    Ok(Assignment {
        place,
        value,
        source: source.trim().to_string(),
        line: at.line,
        target,
        params: None,
    })
}

fn parse_field(source: &str, at: At) -> Result<Field> {
    let mut tokens = Tokens::new(source, at)?;
    let field = tokens.take_field(false)?;
    tokens.finish()?;
    Ok(field)
}

fn parse_action(source: &str, at: At) -> Result<Action> {
    let mut tokens = Tokens::new(source, at)?;
    let (name, span) = tokens.take_name()?;
    let mut params = Vec::new();
    if tokens.eat(&Token::LeftParen) && !tokens.eat(&Token::RightParen) {
        loop {
            params.push(tokens.take_field(true)?);
            if tokens.eat(&Token::Comma) {
                continue;
            }
            tokens.expect(&Token::RightParen)?;
            break;
        }
    }
    tokens.finish()?;
    Ok(Action {
        name,
        fairness: Fairness::Unfair,
        params,
        requires: Vec::new(),
        assignments: Vec::new(),
        line: at.line,
        span,
    })
}

fn parse_call(source: &str, at: At) -> Result<ActionCall> {
    let mut tokens = Tokens::new(source, at)?;
    let (name, span) = tokens.take_name()?;
    let arguments = if tokens.eat(&Token::LeftParen) {
        let mut values = Vec::new();
        if !tokens.eat(&Token::RightParen) {
            loop {
                values.push(tokens.take_literal()?);
                if tokens.eat(&Token::Comma) {
                    continue;
                }
                tokens.expect(&Token::RightParen)?;
                break;
            }
        }
        CallArguments::Positional(values)
    } else if tokens.eat_word("with") {
        let mut values = BTreeMap::new();
        loop {
            let (parameter, parameter_span) = tokens.take_word()?;
            tokens.expect(&Token::Equal)?;
            let value = tokens.take_literal()?;
            if values.insert(parameter.clone(), value).is_some() {
                return Err(
                    Error::at(at.line, format!("duplicate argument {parameter}"))
                        .spanned(parameter_span),
                );
            }
            if tokens.eat(&Token::Comma) || tokens.eat_word("and") {
                continue;
            }
            break;
        }
        CallArguments::Named(values)
    } else {
        CallArguments::Positional(Vec::new())
    };
    tokens.finish()?;
    Ok(ActionCall {
        name,
        arguments,
        line: at.line,
        span,
    })
}

struct Tokens {
    values: Vec<Token>,
    spans: Vec<Span>,
    index: usize,
    at: At,
    depth: usize,
}

impl Tokens {
    fn new(source: &str, at: At) -> Result<Self> {
        let (values, spans) = lex(source, at)?;
        if values.len() > MAX_TOKENS_PER_LINE {
            return Err(Error::at(
                at.line,
                format!("a line may hold at most {MAX_TOKENS_PER_LINE} tokens"),
            ));
        }
        Ok(Self {
            values,
            spans,
            index: 0,
            at,
            depth: 0,
        })
    }

    /// The token at `index`, or the last token when the line ends first.
    fn span(&self, index: usize) -> Span {
        self.spans
            .get(index)
            .or_else(|| self.spans.last())
            .copied()
            .unwrap_or(Span {
                line: self.at.line,
                column: self.at.column,
                end_column: self.at.column,
            })
    }

    fn previous_span(&self) -> Span {
        self.span(self.index.saturating_sub(1))
    }

    fn nested<T>(&mut self, parse: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        if self.depth == MAX_NESTING {
            return Err(self.error(format!(
                "expressions may nest at most {MAX_NESTING} levels deep"
            )));
        }
        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;
        result
    }

    fn peek(&self) -> Option<&Token> {
        self.values.get(self.index)
    }

    fn peek_word(&self, offset: usize) -> Option<&str> {
        match self.values.get(self.index + offset) {
            Some(Token::Word(word)) => Some(word),
            _ => None,
        }
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.values.get(self.index).cloned();
        if token.is_some() {
            self.index += 1;
        }
        token
    }

    fn eat(&mut self, expected: &Token) -> bool {
        if self.peek() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(self.error(format!(
                "expected {}, found {}",
                token_name(expected),
                self.describe_next()
            )))
        }
    }

    fn eat_word(&mut self, expected: &str) -> bool {
        if self.peek_word(0) == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn expect_word(&mut self, expected: &str) -> Result<()> {
        if self.eat_word(expected) {
            Ok(())
        } else {
            Err(self.error(format!(
                "expected '{expected}', found {}",
                self.describe_next()
            )))
        }
    }

    fn take_word(&mut self) -> Result<(String, Span)> {
        match self.next() {
            Some(Token::Word(word)) => Ok((word, self.previous_span())),
            _ => Err(self.previous_error(format!(
                "expected a name, found {}",
                self.describe_previous()
            ))),
        }
    }

    fn take_name(&mut self) -> Result<(String, Span)> {
        match self.next() {
            Some(Token::Word(word) | Token::Text(word)) => Ok((word, self.previous_span())),
            _ => Err(self.previous_error(format!(
                "expected a name, found {}",
                self.describe_previous()
            ))),
        }
    }

    fn take_nonnegative_integer(&mut self) -> Result<usize> {
        match self.next() {
            Some(Token::Integer(value)) => usize::try_from(value).map_err(|_| {
                self.previous_error(format!("{value} does not fit this platform's size limit"))
            }),
            _ => Err(self.previous_error("expected a non-negative integer")),
        }
    }

    fn take_signed_integer(&mut self) -> Result<i64> {
        let negative = self.eat(&Token::Minus);
        match self.next() {
            Some(Token::Integer(value)) => self.convert_integer(value, negative),
            _ => Err(self.previous_error("expected an integer")),
        }
    }

    fn take_literal(&mut self) -> Result<Value> {
        if matches!(self.peek(), Some(Token::Minus | Token::Integer(_))) {
            return self.take_signed_integer().map(Value::Integer);
        }
        match self.next() {
            Some(Token::Text(value)) => Ok(Value::Symbol(value)),
            Some(Token::Word(value)) if value == "true" => Ok(Value::Boolean(true)),
            Some(Token::Word(value)) if value == "false" => Ok(Value::Boolean(false)),
            Some(Token::Word(value)) => Ok(Value::Symbol(value)),
            _ => Err(self.previous_error("expected an integer, boolean, or enum value")),
        }
    }

    fn convert_integer(&self, value: u64, negative: bool) -> Result<i64> {
        if negative && value == I64_MIN_MAGNITUDE {
            return Ok(i64::MIN);
        }
        let value = i64::try_from(value)
            .map_err(|_| self.previous_error("integer is outside the signed 64-bit range"))?;
        if negative {
            value
                .checked_neg()
                .ok_or_else(|| self.previous_error("integer is below the signed 64-bit range"))
        } else {
            Ok(value)
        }
    }

    fn take_field(&mut self, parameter_list: bool) -> Result<Field> {
        let (name, span) = self.take_word()?;
        if is_reserved_word(&name) {
            return Err(self.previous_error(format!(
                "{name} is reserved for expressions; choose another name"
            )));
        }
        self.expect_word("is")?;
        let kind = self.take_type(&name, parameter_list)?;
        Ok(Field {
            name,
            kind,
            line: self.at.line,
            span,
        })
    }

    fn take_type(&mut self, name: &str, parameter_list: bool) -> Result<TypeExpr> {
        let kind = self.take_plain_type(name, parameter_list)?;
        if self.peek_word(0) == Some("or") && self.peek_word(1) == Some("nothing") {
            self.index += 2;
            return Ok(TypeExpr::Optional(Box::new(kind)));
        }
        Ok(kind)
    }

    fn take_plain_type(&mut self, name: &str, parameter_list: bool) -> Result<TypeExpr> {
        if !self.eat_word("a") {
            self.eat_word("an");
        }
        if self.eat_word("integer") {
            self.expect_word("from")?;
            let min = self.take_signed_integer()?;
            self.expect_word("to")?;
            let max = self.take_signed_integer()?;
            if min > max {
                return Err(self.previous_error(format!("{name} has an empty range")));
            }
            return Ok(TypeExpr::Integer { min, max });
        }
        if self.eat_word("boolean") {
            return Ok(TypeExpr::Boolean);
        }
        if self.eat_word("one") {
            self.expect_word("of")?;
            let mut values = vec![self.take_name()?.0];
            loop {
                if self.peek_word(0) == Some("or") && self.peek_word(1) == Some("nothing") {
                    break;
                }
                if self.eat_word("or") {
                    values.push(self.take_name()?.0);
                    continue;
                }
                if parameter_list && self.parameter_boundary() {
                    break;
                }
                if self.eat(&Token::Comma) {
                    values.push(self.take_name()?.0);
                    continue;
                }
                break;
            }
            return Ok(TypeExpr::Enum { values });
        }
        if self.eat_word("set") {
            self.expect_word("of")?;
            if !self.eat_word("a") {
                self.eat_word("an");
            }
            return Ok(TypeExpr::Set(self.take_word()?.0));
        }
        if self.eat_word("record") {
            self.expect_word("with")?;
            self.expect(&Token::LeftParen)?;
            let mut members: Vec<Field> = Vec::new();
            loop {
                let member = self.nested(|tokens| tokens.take_field(true))?;
                if members.iter().any(|other| other.name == member.name) {
                    return Err(Error::at(
                        self.at.line,
                        format!("{name} has two members named {}", member.name),
                    )
                    .spanned(member.span));
                }
                members.push(member);
                if self.eat(&Token::Comma) {
                    continue;
                }
                self.expect(&Token::RightParen)?;
                break;
            }
            return Ok(TypeExpr::Record(members));
        }
        if self.eat_word("map") {
            self.expect_word("from")?;
            if !self.eat_word("a") {
                self.eat_word("an");
            }
            let (key, _) = self.take_word()?;
            self.expect_word("to")?;
            let value = self.take_type(name, parameter_list)?;
            return Ok(TypeExpr::Map {
                key,
                value: Box::new(value),
            });
        }
        match self.next() {
            Some(Token::Word(word)) if !is_reserved_word(&word) => Ok(TypeExpr::Named(word)),
            _ => Err(self.previous_error(
                "expected 'an integer from ... to ...', 'a boolean', 'one of ...', 'a map from ... to ...', 'a record with (...)', or a declared type",
            )),
        }
    }

    fn parameter_boundary(&self) -> bool {
        self.peek() == Some(&Token::Comma)
            && self.peek_word(1).is_some()
            && self.peek_word(2) == Some("is")
    }

    fn finish(&self) -> Result<()> {
        if self.index == self.values.len() {
            Ok(())
        } else {
            Err(self.error(format!("unexpected {}", self.describe_next())))
        }
    }

    fn describe_next(&self) -> String {
        self.peek()
            .map_or_else(|| "end of line".to_string(), token_name)
    }

    fn describe_previous(&self) -> String {
        self.index
            .checked_sub(1)
            .and_then(|index| self.values.get(index))
            .map_or_else(|| "end of line".to_string(), token_name)
    }

    /// An error at the next token.
    fn error(&self, message: impl Into<String>) -> Error {
        Error::at(self.at.line, message).spanned(self.span(self.index))
    }

    /// An error at the token just taken.
    fn previous_error(&self, message: impl Into<String>) -> Error {
        Error::at(self.at.line, message).spanned(self.previous_span())
    }
}

fn token_name(token: &Token) -> String {
    let symbol = match token {
        Token::Word(value) => return format!("'{value}'"),
        Token::Integer(value) => return format!("'{value}'"),
        Token::Text(_) => return "a quoted string".to_string(),
        Token::LeftParen => "(",
        Token::RightParen => ")",
        Token::LeftBracket => "[",
        Token::RightBracket => "]",
        Token::LeftBrace => "{",
        Token::RightBrace => "}",
        Token::Colon => ":",
        Token::Comma => ",",
        Token::Plus => "+",
        Token::Minus => "-",
        Token::Star => "*",
        Token::Slash => "/",
        Token::Equal => "=",
        Token::EqualEqual => "==",
        Token::NotEqual => "!=",
        Token::Less => "<",
        Token::LessEqual => "<=",
        Token::Greater => ">",
        Token::GreaterEqual => ">=",
    };
    format!("'{symbol}'")
}

fn lex(source: &str, at: At) -> Result<(Vec<Token>, Vec<Span>)> {
    let characters: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut spans = Vec::new();
    let mut index = 0;
    let span = |start: usize, width: usize| Span {
        line: at.line,
        column: at.column + start,
        end_column: at.column + start + width,
    };
    let fail = |start: usize, width: usize, message: String| {
        Error::at(at.line, message).spanned(span(start, width))
    };
    while let Some(character) = characters.get(index).copied() {
        if character.is_whitespace() {
            index += 1;
            continue;
        }
        let followed_by_equal = characters.get(index + 1) == Some(&'=');
        let (token, width) = match character {
            '(' => (Token::LeftParen, 1),
            ')' => (Token::RightParen, 1),
            '[' => (Token::LeftBracket, 1),
            ']' => (Token::RightBracket, 1),
            '{' => (Token::LeftBrace, 1),
            '}' => (Token::RightBrace, 1),
            ':' => (Token::Colon, 1),
            ',' => (Token::Comma, 1),
            '+' => (Token::Plus, 1),
            '-' => (Token::Minus, 1),
            '*' => (Token::Star, 1),
            '/' => (Token::Slash, 1),
            '=' if followed_by_equal => (Token::EqualEqual, 2),
            '=' => (Token::Equal, 1),
            '!' if followed_by_equal => (Token::NotEqual, 2),
            '<' if followed_by_equal => (Token::LessEqual, 2),
            '<' => (Token::Less, 1),
            '>' if followed_by_equal => (Token::GreaterEqual, 2),
            '>' => (Token::Greater, 1),
            '"' => {
                let rest = characters.get(index + 1..).unwrap_or(&[]);
                let (text, width) =
                    lex_text(rest).map_err(|(width, message)| fail(index, width + 1, message))?;
                (Token::Text(text), width + 1)
            }
            '0'..='9' => {
                let digits: String = characters
                    .iter()
                    .skip(index)
                    .take_while(|next| next.is_ascii_digit())
                    .collect();
                let value = digits.parse().map_err(|_| {
                    fail(
                        index,
                        digits.len(),
                        "integer is above the supported range".into(),
                    )
                })?;
                (Token::Integer(value), digits.len())
            }
            character if character.is_ascii_alphabetic() || character == '_' => {
                let word: String = characters
                    .iter()
                    .skip(index)
                    .take_while(|next| next.is_ascii_alphanumeric() || **next == '_')
                    .collect();
                let width = word.len();
                (Token::Word(word), width)
            }
            other => return Err(fail(index, 1, format!("unexpected character '{other}'"))),
        };
        spans.push(span(index, width));
        index += width;
        tokens.push(token);
    }
    Ok((tokens, spans))
}

/// A quoted string's value and width, or how far it got and why it failed.
fn lex_text(characters: &[char]) -> std::result::Result<(String, usize), (usize, String)> {
    let mut value = String::new();
    let mut index = 0;
    while let Some(next) = characters.get(index).copied() {
        index += 1;
        match next {
            '"' => return Ok((value, index)),
            '\\' => {
                let Some(escaped) = characters.get(index).copied() else {
                    return Err((index, "unfinished escape in quoted string".into()));
                };
                index += 1;
                value.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    other => return Err((index, format!("unsupported escape \\{other}"))),
                });
            }
            other => value.push(other),
        }
    }
    Err((index, "unterminated quoted string".into()))
}

fn parse_expression(tokens: &mut Tokens) -> Result<Expr> {
    tokens.nested(parse_implication)
}

fn parse_implication(tokens: &mut Tokens) -> Result<Expr> {
    let left = parse_or(tokens)?;
    if tokens.eat_word("implies") {
        let right = parse_expression(tokens)?;
        Ok(binary(BinaryOperator::Implies, left, right))
    } else {
        Ok(left)
    }
}

fn parse_or(tokens: &mut Tokens) -> Result<Expr> {
    let mut expression = parse_and(tokens)?;
    while tokens.eat_word("or") {
        expression = binary(BinaryOperator::Or, expression, parse_and(tokens)?);
    }
    Ok(expression)
}

fn parse_and(tokens: &mut Tokens) -> Result<Expr> {
    let mut expression = parse_not(tokens)?;
    while tokens.eat_word("and") {
        expression = binary(BinaryOperator::And, expression, parse_not(tokens)?);
    }
    Ok(expression)
}

fn parse_not(tokens: &mut Tokens) -> Result<Expr> {
    if tokens.eat_word("not") {
        Ok(Expr::Unary {
            operator: UnaryOperator::Not,
            value: Box::new(tokens.nested(parse_not)?),
        })
    } else {
        parse_comparison(tokens)
    }
}

fn parse_comparison(tokens: &mut Tokens) -> Result<Expr> {
    let left = parse_combination(tokens)?;
    let operator =
        if tokens.eat(&Token::Equal) || tokens.eat(&Token::EqualEqual) || tokens.eat_word("equals")
        {
            Some(BinaryOperator::Equal)
        } else if tokens.eat(&Token::NotEqual) {
            Some(BinaryOperator::NotEqual)
        } else if tokens.eat(&Token::LessEqual) {
            Some(BinaryOperator::LessEqual)
        } else if tokens.eat(&Token::GreaterEqual) {
            Some(BinaryOperator::GreaterEqual)
        } else if tokens.eat(&Token::Less) {
            Some(BinaryOperator::Less)
        } else if tokens.eat(&Token::Greater) {
            Some(BinaryOperator::Greater)
        } else if tokens.eat_word("is") {
            Some(parse_english_comparison(tokens)?)
        } else if tokens.eat_word("has") {
            Some(BinaryOperator::Has)
        } else {
            None
        };
    if let Some(operator) = operator {
        Ok(binary(operator, left, parse_combination(tokens)?))
    } else {
        Ok(left)
    }
}

/// `s with x` and `s without x`, between comparisons and arithmetic.
fn parse_combination(tokens: &mut Tokens) -> Result<Expr> {
    let mut expression = parse_additive(tokens)?;
    loop {
        let operator = if tokens.eat_word("with") {
            BinaryOperator::With
        } else if tokens.eat_word("without") {
            BinaryOperator::Without
        } else {
            break;
        };
        expression = binary(operator, expression, parse_additive(tokens)?);
    }
    Ok(expression)
}

fn parse_english_comparison(tokens: &mut Tokens) -> Result<BinaryOperator> {
    if tokens.eat_word("at") {
        if tokens.eat_word("least") {
            Ok(BinaryOperator::GreaterEqual)
        } else if tokens.eat_word("most") {
            Ok(BinaryOperator::LessEqual)
        } else {
            Err(tokens.error("expected 'least' or 'most' after 'is at'"))
        }
    } else if tokens.eat_word("less") {
        tokens.expect_word("than")?;
        Ok(BinaryOperator::Less)
    } else if tokens.eat_word("greater") || tokens.eat_word("more") {
        tokens.expect_word("than")?;
        Ok(BinaryOperator::Greater)
    } else if tokens.eat_word("not") {
        if tokens.eat_word("equal") {
            tokens.eat_word("to");
        }
        Ok(BinaryOperator::NotEqual)
    } else if tokens.eat_word("different") {
        tokens.expect_word("from")?;
        Ok(BinaryOperator::NotEqual)
    } else {
        if tokens.eat_word("equal") {
            tokens.eat_word("to");
        }
        Ok(BinaryOperator::Equal)
    }
}

fn parse_additive(tokens: &mut Tokens) -> Result<Expr> {
    let mut expression = parse_multiplicative(tokens)?;
    loop {
        let operator = if tokens.eat(&Token::Plus) || tokens.eat_word("plus") {
            BinaryOperator::Add
        } else if tokens.eat(&Token::Minus) || tokens.eat_word("minus") {
            BinaryOperator::Subtract
        } else {
            break;
        };
        expression = binary(operator, expression, parse_multiplicative(tokens)?);
    }
    Ok(expression)
}

fn parse_multiplicative(tokens: &mut Tokens) -> Result<Expr> {
    let mut expression = parse_unary(tokens)?;
    loop {
        let operator = if tokens.eat(&Token::Star) || tokens.eat_word("times") {
            BinaryOperator::Multiply
        } else if tokens.eat(&Token::Slash) {
            BinaryOperator::Divide
        } else if tokens.eat_word("divided") {
            tokens.expect_word("by")?;
            BinaryOperator::Divide
        } else {
            break;
        };
        expression = binary(operator, expression, parse_unary(tokens)?);
    }
    Ok(expression)
}

fn parse_unary(tokens: &mut Tokens) -> Result<Expr> {
    if tokens.peek() == Some(&Token::Minus)
        && matches!(tokens.values.get(tokens.index + 1), Some(Token::Integer(_)))
    {
        Ok(Expr::Literal(Value::Integer(tokens.take_signed_integer()?)))
    } else if tokens.eat(&Token::Minus) {
        Ok(Expr::Unary {
            operator: UnaryOperator::Negate,
            value: Box::new(tokens.nested(parse_unary)?),
        })
    } else {
        parse_primary(tokens)
    }
}

fn parse_primary(tokens: &mut Tokens) -> Result<Expr> {
    if tokens.peek_word(0) == Some("number")
        && tokens.peek_word(1) == Some("of")
        && tokens.peek_word(2).is_some()
        && tokens.peek_word(3) == Some("in")
    {
        tokens.index += 2;
        return parse_quantified(tokens, Quantifier::Count, "where");
    }
    match tokens.next() {
        Some(Token::Integer(value)) => Ok(Expr::Literal(Value::Integer(
            tokens.convert_integer(value, false)?,
        ))),
        Some(Token::Text(value)) => Ok(Expr::Literal(Value::Symbol(value))),
        Some(Token::Word(word)) => match word.as_str() {
            "true" => Ok(Expr::Literal(Value::Boolean(true))),
            "false" => Ok(Expr::Literal(Value::Boolean(false))),
            "nothing" => Ok(Expr::Literal(Value::Nothing)),
            "if" => {
                let condition = parse_expression(tokens)?;
                tokens.expect_word("then")?;
                let then = parse_expression(tokens)?;
                tokens.expect_word("else")?;
                let otherwise = parse_expression(tokens)?;
                Ok(Expr::If {
                    condition: Box::new(condition),
                    then: Box::new(then),
                    otherwise: Box::new(otherwise),
                })
            }
            "every" => parse_quantified(tokens, Quantifier::Every, "satisfies"),
            "some" => parse_quantified(tokens, Quantifier::Some, "satisfies"),
            _ if is_reserved_word(&word) => {
                Err(tokens.previous_error(format!("expected a value or name, found '{word}'")))
            }
            _ => {
                let span = tokens.previous_span();
                parse_name(tokens, word, span)
            }
        },
        Some(Token::LeftParen) => {
            let expression = parse_expression(tokens)?;
            tokens.expect(&Token::RightParen)?;
            parse_subscripts(tokens, expression)
        }
        Some(Token::LeftBrace) => parse_record(tokens),
        Some(Token::LeftBracket) => {
            let mut items = Vec::new();
            if !tokens.eat(&Token::RightBracket) {
                loop {
                    items.push(parse_expression(tokens)?);
                    if tokens.eat(&Token::Comma) {
                        continue;
                    }
                    tokens.expect(&Token::RightBracket)?;
                    break;
                }
            }
            Ok(Expr::Items(items))
        }
        _ => Err(tokens.previous_error("expected a value, name, or parenthesized expression")),
    }
}

/// The left side of an assignment: `name`, `name[key]...`, `name of every
/// <type>`, or `name of <target>`, which may itself be a path.
fn parse_target(tokens: &mut Tokens) -> Result<Expr> {
    let (name, span) = tokens.take_word()?;
    let place = if tokens.eat(&Token::LeftBracket) {
        let key = parse_expression(tokens)?;
        tokens.expect(&Token::RightBracket)?;
        Expr::Index {
            map: name,
            key: Box::new(key),
            span,
        }
    } else if tokens.eat_word("of") {
        let key = if tokens.eat_word("every") {
            Expr::Every(tokens.take_word()?.0)
        } else if matches!(tokens.peek(), Some(Token::Word(word)) if !is_reserved_word(word)) {
            tokens.nested(parse_target)?
        } else {
            tokens.nested(parse_primary)?
        };
        Expr::Index {
            map: name,
            key: Box::new(key),
            span,
        }
    } else {
        Expr::Name { name, span }
    };
    parse_subscripts(tokens, place)
}

fn parse_subscripts(tokens: &mut Tokens, mut base: Expr) -> Result<Expr> {
    while tokens.eat(&Token::LeftBracket) {
        let key = parse_expression(tokens)?;
        tokens.expect(&Token::RightBracket)?;
        base = Expr::Subscript {
            base: Box::new(base),
            key: Box::new(key),
        };
    }
    Ok(base)
}

fn parse_record(tokens: &mut Tokens) -> Result<Expr> {
    let mut members: Vec<(String, Expr)> = Vec::new();
    loop {
        let (member, span) = tokens.take_word()?;
        if members.iter().any(|(other, _)| *other == member) {
            return Err(
                Error::at(tokens.at.line, format!("{member} is given twice")).spanned(span),
            );
        }
        tokens.expect(&Token::Colon)?;
        members.push((member, parse_expression(tokens)?));
        if tokens.eat(&Token::Comma) {
            continue;
        }
        tokens.expect(&Token::RightBrace)?;
        return Ok(Expr::Record(members));
    }
}

fn parse_name(tokens: &mut Tokens, name: String, span: Span) -> Result<Expr> {
    if tokens.eat(&Token::LeftBracket) {
        let key = parse_expression(tokens)?;
        tokens.expect(&Token::RightBracket)?;
        return parse_subscripts(
            tokens,
            Expr::Index {
                map: name,
                key: Box::new(key),
                span,
            },
        );
    }
    if tokens.eat_word("of") {
        let key = tokens.nested(parse_primary)?;
        return Ok(Expr::Index {
            map: name,
            key: Box::new(key),
            span,
        });
    }
    Ok(Expr::Name { name, span })
}

fn parse_quantified(tokens: &mut Tokens, quantifier: Quantifier, link: &str) -> Result<Expr> {
    let (variable, _) = tokens.take_word()?;
    if is_reserved_word(&variable) {
        return Err(tokens.previous_error(format!(
            "{variable} is reserved for expressions; choose another name"
        )));
    }
    tokens.expect_word("in")?;
    let (domain, _) = tokens.take_word()?;
    tokens.expect_word(link)?;
    let body = parse_expression(tokens)?;
    Ok(Expr::Quantified {
        quantifier,
        variable,
        domain,
        body: Box::new(body),
    })
}

fn binary(operator: BinaryOperator, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        operator,
        left: Box::new(left),
        right: Box::new(right),
    }
}
