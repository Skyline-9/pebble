use std::collections::BTreeMap;

pub(super) enum Json {
    Object(BTreeMap<String, Self>),
    Array(Vec<Self>),
    String(String),
    Number(String),
    Bool(bool),
    Null,
}

impl Json {
    pub(super) const fn as_object(&self) -> Option<&BTreeMap<String, Self>> {
        if let Self::Object(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(super) fn as_array(&self) -> Option<&[Self]> {
        if let Self::Array(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(super) fn as_string(&self) -> Option<&str> {
        if let Self::String(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(super) fn as_number(&self) -> Option<&str> {
        if let Self::Number(value) = self {
            Some(value)
        } else {
            None
        }
    }

    pub(super) const fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(value) = self {
            Some(*value)
        } else {
            None
        }
    }
}

pub(super) struct JsonParser<'a> {
    input: &'a [u8],
    index: usize,
}

impl<'a> JsonParser<'a> {
    pub(super) const fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            index: 0,
        }
    }

    pub(super) fn parse(mut self) -> Result<Json, String> {
        let value = self.value()?;
        self.whitespace();
        if self.index == self.input.len() {
            Ok(value)
        } else {
            Err(String::from("trailing data"))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.whitespace();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal(b"true", Json::Bool(true)),
            Some(b'f') => self.literal(b"false", Json::Bool(false)),
            Some(b'n') => self.literal(b"null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(String::from("invalid JSON value")),
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.take(b'{')?;
        let mut values = BTreeMap::new();
        self.whitespace();
        if self.consume(b'}') {
            return Ok(Json::Object(values));
        }
        loop {
            self.whitespace();
            let key = self.string()?;
            self.whitespace();
            self.take(b':')?;
            let value = self.value()?;
            if values.insert(key.clone(), value).is_some() {
                return Err(format!("duplicate JSON key: {key}"));
            }
            self.whitespace();
            if self.consume(b'}') {
                return Ok(Json::Object(values));
            }
            self.take(b',')?;
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.take(b'[')?;
        let mut values = Vec::new();
        self.whitespace();
        if self.consume(b']') {
            return Ok(Json::Array(values));
        }
        loop {
            values.push(self.value()?);
            self.whitespace();
            if self.consume(b']') {
                return Ok(Json::Array(values));
            }
            self.take(b',')?;
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.take(b'"')?;
        let mut value = String::new();
        loop {
            let byte = self
                .next()
                .ok_or_else(|| String::from("unterminated string"))?;
            match byte {
                b'"' => return Ok(value),
                b'\\' => value.push(self.escape()?),
                0..=31 => return Err(String::from("control character in string")),
                _ => value.push(char::from(byte)),
            }
        }
    }

    fn escape(&mut self) -> Result<char, String> {
        match self.next() {
            Some(b'"') => Ok('"'),
            Some(b'\\') => Ok('\\'),
            Some(b'/') => Ok('/'),
            Some(b'b') => Ok('\u{0008}'),
            Some(b'f') => Ok('\u{000c}'),
            Some(b'n') => Ok('\n'),
            Some(b'r') => Ok('\r'),
            Some(b't') => Ok('\t'),
            Some(b'u') => {
                let digits = (0..4)
                    .map(|_| {
                        self.next()
                            .ok_or_else(|| String::from("invalid unicode escape"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let digits = std::str::from_utf8(&digits)
                    .map_err(|_| String::from("invalid unicode escape"))?;
                let code = u32::from_str_radix(digits, 16)
                    .map_err(|_| String::from("invalid unicode escape"))?;
                char::from_u32(code).ok_or_else(|| String::from("invalid unicode escape"))
            }
            _ => Err(String::from("invalid string escape")),
        }
    }

    fn literal(&mut self, literal: &[u8], value: Json) -> Result<Json, String> {
        if self.input.get(self.index..self.index + literal.len()) == Some(literal) {
            self.index += literal.len();
            Ok(value)
        } else {
            Err(String::from("invalid JSON literal"))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.index;
        while self.peek().is_some_and(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.' | b'e' | b'E')
        }) {
            self.index += 1;
        }
        let number = std::str::from_utf8(&self.input[start..self.index])
            .map_err(|_| String::from("invalid number"))?;
        if number.parse::<f64>().is_ok() {
            Ok(Json::Number(number.to_owned()))
        } else {
            Err(String::from("invalid number"))
        }
    }

    fn whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.index += 1;
        }
    }

    fn take(&mut self, expected: u8) -> Result<(), String> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(format!("expected {}", char::from(expected)))
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.index).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.index += 1;
        Some(byte)
    }
}
