use crate::ast::{
    Action, ActionCall, Assignment, BinaryOperator, CallArguments, Condition, Expr, Field,
    FieldKind, Scenario, ScenarioStep, Spec, UnaryOperator, Value,
};
use crate::{Error, Result};
use std::collections::BTreeMap;

const I64_MIN_MAGNITUDE: u64 = 9_223_372_036_854_775_808;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Integer(u64),
    Text(String),
    LeftParen,
    RightParen,
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
    content: String,
}

#[derive(Debug, Clone, Copy)]
enum Section {
    None,
    State,
    Initially,
    Always,
    Action(usize),
    Scenario(usize),
}

pub fn parse(source: &str) -> Result<Spec> {
    let lines = logical_lines(source)?;
    let mut spec = Spec::empty();
    let mut section = Section::None;
    let mut has_system = false;
    let mut has_state = false;
    let mut has_initially = false;

    for line in lines {
        if line.indent == 0 {
            section = parse_top_level(
                &mut spec,
                &line,
                &mut has_system,
                &mut has_state,
                &mut has_initially,
            )?;
            continue;
        }

        match section {
            Section::State => spec.state.push(parse_field(&line.content, line.number)?),
            Section::Initially => {
                spec.initially
                    .push(parse_assignment(&line.content, "is", line.number)?)
            }
            Section::Always => spec
                .invariants
                .push(parse_condition(&line.content, line.number)?),
            Section::Action(index) => {
                let action = spec
                    .actions
                    .get_mut(index)
                    .ok_or_else(|| Error::at(line.number, "missing action"))?;
                if let Some(source) = after_keyword(&line.content, "requires") {
                    if source.is_empty() {
                        return Err(Error::at(line.number, "requires needs a condition"));
                    }
                    action.requires.push(parse_condition(source, line.number)?);
                } else {
                    action.assignments.push(parse_assignment(
                        &line.content,
                        "becomes",
                        line.number,
                    )?);
                }
            }
            Section::Scenario(index) => {
                let scenario = spec
                    .scenarios
                    .get_mut(index)
                    .ok_or_else(|| Error::at(line.number, "missing scenario"))?;
                if let Some(source) = after_keyword(&line.content, "expect") {
                    if source.is_empty() {
                        return Err(Error::at(line.number, "expect needs a condition"));
                    }
                    scenario
                        .steps
                        .push(ScenarioStep::Expect(parse_condition(source, line.number)?));
                } else {
                    scenario
                        .steps
                        .push(ScenarioStep::Call(parse_call(&line.content, line.number)?));
                }
            }
            Section::None => {
                return Err(Error::at(
                    line.number,
                    "this indented line does not belong to a block",
                ));
            }
        }
    }

    if !has_system {
        return Err(Error::new("a specification must start with system \"...\""));
    }
    if !has_state || spec.state.is_empty() {
        return Err(Error::new("declare at least one field in a state block"));
    }
    if !has_initially {
        return Err(Error::new("declare an initially block"));
    }
    Ok(spec)
}

fn parse_top_level(
    spec: &mut Spec,
    line: &Line,
    has_system: &mut bool,
    has_state: &mut bool,
    has_initially: &mut bool,
) -> Result<Section> {
    let content = line.content.as_str();
    if let Some(rest) = after_keyword(content, "system") {
        if *has_system {
            return Err(Error::at(line.number, "system may only be declared once"));
        }
        spec.name = parse_standalone_name(rest, line.number)?;
        *has_system = true;
        return Ok(Section::None);
    }
    if !*has_system {
        return Err(Error::at(
            line.number,
            "the first declaration must be system \"...\"",
        ));
    }
    if content == "state" {
        if *has_state {
            return Err(Error::at(line.number, "state may only be declared once"));
        }
        *has_state = true;
        return Ok(Section::State);
    }
    if content == "initially" {
        if *has_initially {
            return Err(Error::at(
                line.number,
                "initially may only be declared once",
            ));
        }
        *has_initially = true;
        return Ok(Section::Initially);
    }
    if content == "always" {
        return Ok(Section::Always);
    }
    if let Some(source) = after_keyword(content, "always") {
        if source.is_empty() {
            return Ok(Section::Always);
        }
        spec.invariants.push(parse_condition(source, line.number)?);
        return Ok(Section::None);
    }
    if let Some(rest) = after_keyword(content, "action") {
        let action = parse_action(rest, line.number)?;
        spec.actions.push(action);
        return Ok(Section::Action(spec.actions.len() - 1));
    }
    if let Some(rest) = after_keyword(content, "scenario") {
        let name = parse_standalone_name(rest, line.number)?;
        spec.scenarios.push(Scenario {
            name,
            steps: Vec::new(),
            line: line.number,
        });
        return Ok(Section::Scenario(spec.scenarios.len() - 1));
    }
    if after_keyword(content, "explore").is_some() {
        let mut tokens = Tokens::new(content, line.number)?;
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
        let without_comment = strip_comment(raw);
        let content = without_comment.trim();
        if content.is_empty() {
            continue;
        }
        result.push(Line {
            number,
            indent,
            content: content.to_string(),
        });
    }
    Ok(result)
}

fn strip_comment(line: &str) -> &str {
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
            return &line[..index];
        }
    }
    line
}

fn after_keyword<'a>(source: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = source.strip_prefix(keyword)?;
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix(char::is_whitespace).map(str::trim_start)
}

fn parse_standalone_name(source: &str, line: usize) -> Result<String> {
    let mut tokens = Tokens::new(source, line)?;
    let name = tokens.take_name()?;
    tokens.finish()?;
    if name.is_empty() {
        return Err(Error::at(line, "a name cannot be empty"));
    }
    Ok(name)
}

fn parse_condition(source: &str, line: usize) -> Result<Condition> {
    let mut tokens = Tokens::new(source, line)?;
    let expr = parse_expression_tokens(&mut tokens)?;
    tokens.finish()?;
    Ok(Condition {
        expr,
        source: source.trim().to_string(),
        line,
    })
}

fn parse_assignment(source: &str, keyword: &str, line: usize) -> Result<Assignment> {
    let mut tokens = Tokens::new(source, line)?;
    let field = tokens.take_word()?;
    tokens.expect_word(keyword)?;
    let value = parse_expression_tokens(&mut tokens)?;
    tokens.finish()?;
    Ok(Assignment {
        field,
        value,
        source: source.trim().to_string(),
        line,
    })
}

fn parse_field(source: &str, line: usize) -> Result<Field> {
    let mut tokens = Tokens::new(source, line)?;
    let field = tokens.take_field(false)?;
    tokens.finish()?;
    Ok(field)
}

fn parse_action(source: &str, line: usize) -> Result<Action> {
    let mut tokens = Tokens::new(source, line)?;
    let name = tokens.take_name()?;
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
        params,
        requires: Vec::new(),
        assignments: Vec::new(),
        line,
    })
}

fn parse_call(source: &str, line: usize) -> Result<ActionCall> {
    let mut tokens = Tokens::new(source, line)?;
    let name = tokens.take_name()?;
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
            let parameter = tokens.take_word()?;
            tokens.expect(&Token::Equal)?;
            let value = tokens.take_literal()?;
            if values.insert(parameter.clone(), value).is_some() {
                return Err(Error::at(line, format!("duplicate argument {parameter}")));
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
        line,
    })
}

struct Tokens {
    values: Vec<Token>,
    index: usize,
    line: usize,
}

impl Tokens {
    fn new(source: &str, line: usize) -> Result<Self> {
        Ok(Self {
            values: lex(source, line)?,
            index: 0,
            line,
        })
    }

    fn peek(&self) -> Option<&Token> {
        self.values.get(self.index)
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
        if matches!(self.peek(), Some(Token::Word(word)) if word == expected) {
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

    fn take_word(&mut self) -> Result<String> {
        match self.next() {
            Some(Token::Word(word)) => Ok(word),
            _ => Err(self.error(format!(
                "expected a name, found {}",
                self.describe_previous()
            ))),
        }
    }

    fn take_name(&mut self) -> Result<String> {
        match self.next() {
            Some(Token::Word(word) | Token::Text(word)) => Ok(word),
            _ => Err(self.error(format!(
                "expected a name, found {}",
                self.describe_previous()
            ))),
        }
    }

    fn take_nonnegative_integer(&mut self) -> Result<usize> {
        match self.next() {
            Some(Token::Integer(value)) => usize::try_from(value).map_err(|_| {
                self.error(format!("{value} does not fit this platform's size limit"))
            }),
            _ => Err(self.error("expected a non-negative integer")),
        }
    }

    fn take_signed_integer(&mut self) -> Result<i64> {
        let negative = self.eat(&Token::Minus);
        match self.next() {
            Some(Token::Integer(value)) => self.convert_integer(value, negative),
            _ => Err(self.error("expected an integer")),
        }
    }

    fn take_literal(&mut self) -> Result<Value> {
        if matches!(self.peek(), Some(Token::Minus | Token::Integer(_))) {
            return self.take_signed_integer().map(Value::Integer);
        }
        match self.next() {
            Some(Token::Text(value)) => Ok(Value::Text(value)),
            Some(Token::Word(value)) if value == "true" => Ok(Value::Boolean(true)),
            Some(Token::Word(value)) if value == "false" => Ok(Value::Boolean(false)),
            Some(Token::Word(value)) => Ok(Value::Text(value)),
            _ => Err(self.error("expected an integer, boolean, or enum value")),
        }
    }

    fn convert_integer(&self, value: u64, negative: bool) -> Result<i64> {
        if negative && value == I64_MIN_MAGNITUDE {
            return Ok(i64::MIN);
        }
        let value = i64::try_from(value)
            .map_err(|_| self.error("integer is outside the signed 64-bit range"))?;
        if negative {
            value
                .checked_neg()
                .ok_or_else(|| self.error("integer is below the signed 64-bit range"))
        } else {
            Ok(value)
        }
    }

    fn take_field(&mut self, parameter_list: bool) -> Result<Field> {
        let name = self.take_word()?;
        if is_reserved_identifier(&name) {
            return Err(self.error(format!(
                "{name} is reserved for expressions; choose another field or parameter name"
            )));
        }
        self.expect_word("is")?;
        self.eat_word("a");
        self.eat_word("an");

        let kind = if self.eat_word("integer") {
            self.expect_word("from")?;
            let min = self.take_signed_integer()?;
            self.expect_word("to")?;
            let max = self.take_signed_integer()?;
            if min > max {
                return Err(self.error(format!("{name} has an empty range")));
            }
            FieldKind::Integer { min, max }
        } else if self.eat_word("boolean") {
            FieldKind::Boolean
        } else if self.eat_word("one") {
            self.expect_word("of")?;
            let mut values = vec![self.take_name()?];
            loop {
                if self.eat_word("or") {
                    values.push(self.take_name()?);
                    continue;
                }
                if parameter_list && self.parameter_boundary() {
                    break;
                }
                if self.eat(&Token::Comma) {
                    values.push(self.take_name()?);
                    continue;
                }
                break;
            }
            FieldKind::Enum { values }
        } else {
            return Err(
                self.error("expected 'an integer from ... to ...', 'a boolean', or 'one of ...'")
            );
        };
        Ok(Field {
            name,
            kind,
            line: self.line,
        })
    }

    fn parameter_boundary(&self) -> bool {
        if self.peek() != Some(&Token::Comma) {
            return false;
        }
        matches!(
            (self.values.get(self.index + 1), self.values.get(self.index + 2)),
            (Some(Token::Word(_)), Some(Token::Word(keyword))) if keyword == "is"
        )
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

    fn error(&self, message: impl Into<String>) -> Error {
        Error::at(self.line, message)
    }
}

fn is_reserved_identifier(name: &str) -> bool {
    matches!(
        name,
        "true"
            | "false"
            | "not"
            | "and"
            | "or"
            | "implies"
            | "is"
            | "equals"
            | "plus"
            | "minus"
            | "times"
            | "divided"
    )
}

fn token_name(token: &Token) -> String {
    match token {
        Token::Word(value) => format!("'{value}'"),
        Token::Integer(value) => format!("'{value}'"),
        Token::Text(_) => "a quoted string".to_string(),
        Token::LeftParen => "'('".to_string(),
        Token::RightParen => "')'".to_string(),
        Token::Comma => "','".to_string(),
        Token::Plus => "'+'".to_string(),
        Token::Minus => "'-'".to_string(),
        Token::Star => "'*'".to_string(),
        Token::Slash => "'/'".to_string(),
        Token::Equal => "'='".to_string(),
        Token::EqualEqual => "'=='".to_string(),
        Token::NotEqual => "'!='".to_string(),
        Token::Less => "'<'".to_string(),
        Token::LessEqual => "'<='".to_string(),
        Token::Greater => "'>'".to_string(),
        Token::GreaterEqual => "'>='".to_string(),
    }
}

fn lex(source: &str, line: usize) -> Result<Vec<Token>> {
    let characters: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while let Some(character) = characters.get(index).copied() {
        if character.is_whitespace() {
            index += 1;
            continue;
        }
        let token = match character {
            '(' => {
                index += 1;
                Token::LeftParen
            }
            ')' => {
                index += 1;
                Token::RightParen
            }
            ',' => {
                index += 1;
                Token::Comma
            }
            '+' => {
                index += 1;
                Token::Plus
            }
            '-' => {
                index += 1;
                Token::Minus
            }
            '*' => {
                index += 1;
                Token::Star
            }
            '/' => {
                index += 1;
                Token::Slash
            }
            '=' => {
                index += 1;
                if characters.get(index) == Some(&'=') {
                    index += 1;
                    Token::EqualEqual
                } else {
                    Token::Equal
                }
            }
            '!' if characters.get(index + 1) == Some(&'=') => {
                index += 2;
                Token::NotEqual
            }
            '<' => {
                index += 1;
                if characters.get(index) == Some(&'=') {
                    index += 1;
                    Token::LessEqual
                } else {
                    Token::Less
                }
            }
            '>' => {
                index += 1;
                if characters.get(index) == Some(&'=') {
                    index += 1;
                    Token::GreaterEqual
                } else {
                    Token::Greater
                }
            }
            '"' => {
                index += 1;
                let mut value = String::new();
                let mut closed = false;
                while let Some(next) = characters.get(index).copied() {
                    index += 1;
                    match next {
                        '"' => {
                            closed = true;
                            break;
                        }
                        '\\' => {
                            let Some(escaped) = characters.get(index).copied() else {
                                return Err(Error::at(line, "unfinished escape in quoted string"));
                            };
                            index += 1;
                            value.push(match escaped {
                                'n' => '\n',
                                'r' => '\r',
                                't' => '\t',
                                '"' => '"',
                                '\\' => '\\',
                                other => {
                                    return Err(Error::at(
                                        line,
                                        format!("unsupported escape \\{other}"),
                                    ));
                                }
                            });
                        }
                        other => value.push(other),
                    }
                }
                if !closed {
                    return Err(Error::at(line, "unterminated quoted string"));
                }
                Token::Text(value)
            }
            '0'..='9' => {
                let start = index;
                index += 1;
                while characters.get(index).is_some_and(char::is_ascii_digit) {
                    index += 1;
                }
                let value: String = characters[start..index].iter().collect();
                Token::Integer(
                    value
                        .parse()
                        .map_err(|_| Error::at(line, "integer is above the supported range"))?,
                )
            }
            character if character.is_ascii_alphabetic() || character == '_' => {
                let start = index;
                index += 1;
                while characters
                    .get(index)
                    .is_some_and(|next| next.is_ascii_alphanumeric() || *next == '_')
                {
                    index += 1;
                }
                Token::Word(characters[start..index].iter().collect())
            }
            other => return Err(Error::at(line, format!("unexpected character '{other}'"))),
        };
        tokens.push(token);
    }
    Ok(tokens)
}

fn parse_expression_tokens(tokens: &mut Tokens) -> Result<Expr> {
    parse_implication(tokens)
}

fn parse_implication(tokens: &mut Tokens) -> Result<Expr> {
    let left = parse_or(tokens)?;
    if tokens.eat_word("implies") {
        let right = parse_implication(tokens)?;
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
            value: Box::new(parse_not(tokens)?),
        })
    } else {
        parse_comparison(tokens)
    }
}

fn parse_comparison(tokens: &mut Tokens) -> Result<Expr> {
    let left = parse_additive(tokens)?;
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
            if tokens.eat_word("at") {
                if tokens.eat_word("least") {
                    Some(BinaryOperator::GreaterEqual)
                } else if tokens.eat_word("most") {
                    Some(BinaryOperator::LessEqual)
                } else {
                    return Err(tokens.error("expected 'least' or 'most' after 'is at'"));
                }
            } else if tokens.eat_word("less") {
                tokens.expect_word("than")?;
                Some(BinaryOperator::Less)
            } else if tokens.eat_word("greater") || tokens.eat_word("more") {
                tokens.expect_word("than")?;
                Some(BinaryOperator::Greater)
            } else if tokens.eat_word("not") {
                if tokens.eat_word("equal") {
                    tokens.eat_word("to");
                }
                Some(BinaryOperator::NotEqual)
            } else if tokens.eat_word("different") {
                tokens.expect_word("from")?;
                Some(BinaryOperator::NotEqual)
            } else {
                if tokens.eat_word("equal") {
                    tokens.eat_word("to");
                }
                Some(BinaryOperator::Equal)
            }
        } else {
            None
        };
    if let Some(operator) = operator {
        Ok(binary(operator, left, parse_additive(tokens)?))
    } else {
        Ok(left)
    }
}

fn parse_additive(tokens: &mut Tokens) -> Result<Expr> {
    let mut expression = parse_multiplicative(tokens)?;
    loop {
        let operator = if tokens.eat(&Token::Plus) || tokens.eat_word("plus") {
            Some(BinaryOperator::Add)
        } else if tokens.eat(&Token::Minus) || tokens.eat_word("minus") {
            Some(BinaryOperator::Subtract)
        } else {
            None
        };
        let Some(operator) = operator else { break };
        expression = binary(operator, expression, parse_multiplicative(tokens)?);
    }
    Ok(expression)
}

fn parse_multiplicative(tokens: &mut Tokens) -> Result<Expr> {
    let mut expression = parse_unary(tokens)?;
    loop {
        let operator = if tokens.eat(&Token::Star) || tokens.eat_word("times") {
            Some(BinaryOperator::Multiply)
        } else if tokens.eat(&Token::Slash) || tokens.eat_word("divided") {
            if tokens.values.get(tokens.index.wrapping_sub(1))
                == Some(&Token::Word("divided".to_string()))
            {
                tokens.expect_word("by")?;
            }
            Some(BinaryOperator::Divide)
        } else {
            None
        };
        let Some(operator) = operator else { break };
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
            value: Box::new(parse_unary(tokens)?),
        })
    } else {
        parse_primary(tokens)
    }
}

fn parse_primary(tokens: &mut Tokens) -> Result<Expr> {
    match tokens.next() {
        Some(Token::Integer(value)) => Ok(Expr::Literal(Value::Integer(
            tokens.convert_integer(value, false)?,
        ))),
        Some(Token::Text(value)) => Ok(Expr::Literal(Value::Text(value))),
        Some(Token::Word(value)) if value == "true" => Ok(Expr::Literal(Value::Boolean(true))),
        Some(Token::Word(value)) if value == "false" => Ok(Expr::Literal(Value::Boolean(false))),
        Some(Token::Word(value)) => Ok(Expr::Name(value)),
        Some(Token::LeftParen) => {
            let expression = parse_expression_tokens(tokens)?;
            tokens.expect(&Token::RightParen)?;
            Ok(expression)
        }
        _ => Err(tokens.error("expected a value, name, or parenthesized expression")),
    }
}

fn binary(operator: BinaryOperator, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        operator,
        left: Box::new(left),
        right: Box::new(right),
    }
}
