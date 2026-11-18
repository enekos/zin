use std::fmt::Write as _;

const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    #[must_use]
    pub fn str(value: impl Into<String>) -> Self {
        Self::Str(value.into())
    }

    #[must_use]
    pub fn count(value: usize) -> Self {
        Self::Int(i64::try_from(value).unwrap_or(i64::MAX))
    }

    #[must_use]
    pub fn object<const N: usize>(entries: [(&str, Json); N]) -> Self {
        Self::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(value) => Some(value),
            _ => None,
        }
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        let mut reader = Reader {
            bytes: source.as_bytes(),
            source,
            index: 0,
        };
        reader.skip_whitespace();
        let value = reader.value(0)?;
        reader.skip_whitespace();
        if reader.index == reader.bytes.len() {
            Ok(value)
        } else {
            Err(format!("unexpected text at column {}", reader.index + 1))
        }
    }

    #[must_use]
    pub fn to_compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Int(value) => {
                let _ = write!(out, "{value}");
            }
            Self::Float(value) => {
                if value.is_finite() {
                    let _ = write!(out, "{value}");
                } else {
                    out.push_str("null");
                }
            }
            Self::Str(value) => write_string(value, out),
            Self::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Self::Object(entries) => {
                out.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_string(value: &str, out: &mut String) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(control));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

struct Reader<'a> {
    bytes: &'a [u8],
    source: &'a str,
    index: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.index).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.index += 1;
        }
    }

    fn fail<T>(&self, message: &str) -> Result<T, String> {
        Err(format!("{message} at column {}", self.index + 1))
    }

    fn literal(&mut self, text: &str, value: Json) -> Result<Json, String> {
        let end = self.index + text.len();
        if self.bytes.get(self.index..end) == Some(text.as_bytes()) {
            self.index = end;
            Ok(value)
        } else {
            self.fail("unexpected value")
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return self.fail("JSON nests too deeply");
        }
        match self.peek() {
            Some(b'n') => self.literal("null", Json::Null),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => self.fail("expected a JSON value"),
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.index += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.index += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b']') => {
                    self.index += 1;
                    return Ok(Json::Array(items));
                }
                _ => return self.fail("expected ',' or ']'"),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.index += 1;
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.index += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return self.fail("expected a string key");
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return self.fail("expected ':'");
            }
            self.index += 1;
            self.skip_whitespace();
            let value = self.value(depth + 1)?;
            entries.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.index += 1,
                Some(b'}') => {
                    self.index += 1;
                    return Ok(Json::Object(entries));
                }
                _ => return self.fail("expected ',' or '}'"),
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.index;
        let mut integral = true;
        if self.peek() == Some(b'-') {
            self.index += 1;
        }
        while let Some(byte) = self.peek() {
            match byte {
                b'0'..=b'9' => {}
                b'.' | b'e' | b'E' | b'+' | b'-' => integral = false,
                _ => break,
            }
            self.index += 1;
        }
        let text = self.source.get(start..self.index).unwrap_or("");
        if integral && let Ok(value) = text.parse::<i64>() {
            return Ok(Json::Int(value));
        }
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => Ok(Json::Float(value)),
            _ => self.fail("invalid number"),
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let end = self.index + 4;
        let digits = self.source.get(self.index..end).unwrap_or("");
        match u32::from_str_radix(digits, 16) {
            Ok(value) if digits.len() == 4 => {
                self.index = end;
                Ok(value)
            }
            _ => self.fail("invalid \\u escape"),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.index += 1;
        let mut value = String::new();
        loop {
            let start = self.index;
            while !matches!(self.peek(), Some(b'"' | b'\\') | None) {
                self.index += 1;
            }
            value.push_str(self.source.get(start..self.index).unwrap_or(""));
            match self.peek() {
                Some(b'"') => {
                    self.index += 1;
                    return Ok(value);
                }
                Some(b'\\') => {
                    self.index += 1;
                    let escape = self.peek();
                    self.index += 1;
                    match escape {
                        Some(b'"') => value.push('"'),
                        Some(b'\\') => value.push('\\'),
                        Some(b'/') => value.push('/'),
                        Some(b'n') => value.push('\n'),
                        Some(b'r') => value.push('\r'),
                        Some(b't') => value.push('\t'),
                        Some(b'b') => value.push('\u{8}'),
                        Some(b'f') => value.push('\u{c}'),
                        Some(b'u') => value.push(self.unicode_escape()?),
                        _ => return self.fail("invalid escape"),
                    }
                }
                _ => return self.fail("unterminated string"),
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let first = self.hex4()?;
        let code = if (0xd800..0xdc00).contains(&first) {
            if self.bytes.get(self.index..self.index + 2) != Some(b"\\u") {
                return self.fail("unpaired surrogate");
            }
            self.index += 2;
            let second = self.hex4()?;
            if !(0xdc00..0xe000).contains(&second) {
                return self.fail("unpaired surrogate");
            }
            0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00)
        } else {
            first
        };
        match char::from_u32(code) {
            Some(character) => Ok(character),
            None => self.fail("invalid \\u escape"),
        }
    }
}
