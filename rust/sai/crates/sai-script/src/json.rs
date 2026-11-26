//! JSON as `sai_script.cpp` reads it with `nlohmann::json`: objects with sorted keys (`std::map`; a
//! repeated key: the last), integers as `int64` / `uint64`, and the accessors' rules and error texts
//! (`at`, `value`, `get<T>`: what nlohmann 3.11 throws, so a script's error line reads the same).
#![forbid(unsafe_code)]

use std::collections::BTreeMap;

/// A JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// `null`
    Null,
    /// `true` / `false`
    Bool(bool),
    /// a negative or small integer (`number_integer`)
    Int(i64),
    /// an integer above `i64::MAX` (`number_unsigned`)
    UInt(u64),
    /// a number with a fraction or exponent
    Float(f64),
    /// a string
    Str(String),
    /// an array
    Arr(Vec<Json>),
    /// an object
    Obj(BTreeMap<String, Json>),
}

/// An accessor's failure: nlohmann's exception text.
pub type Error = String;

static NULL: Json = Json::Null;

impl Json {
    /// nlohmann's `type_name()`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Int(_) | Self::UInt(_) | Self::Float(_) => "number",
            Self::Str(_) => "string",
            Self::Arr(_) => "array",
            Self::Obj(_) => "object",
        }
    }

    /// `is_number_integer()`
    pub fn is_integer(&self) -> bool {
        matches!(self, Self::Int(_) | Self::UInt(_))
    }

    /// `is_number()`
    pub fn is_number(&self) -> bool {
        matches!(self, Self::Int(_) | Self::UInt(_) | Self::Float(_))
    }

    /// `contains(key)` (false for a non-object).
    pub fn contains(&self, key: &str) -> bool {
        matches!(self, Self::Obj(m) if m.contains_key(key))
    }

    /// `at(key)`.
    pub fn at(&self, key: &str) -> Result<&Self, Error> {
        match self {
            Self::Obj(m) => m
                .get(key)
                .ok_or_else(|| format!("[json.exception.out_of_range.403] key '{key}' not found")),
            o => Err(format!(
                "[json.exception.type_error.304] cannot use at() with {}",
                o.type_name()
            )),
        }
    }

    /// `at(index)` of an array.
    pub fn at_index(&self, i: usize) -> Result<&Self, Error> {
        match self {
            Self::Arr(a) => a
                .get(i)
                .ok_or_else(|| format!("[json.exception.out_of_range.401] array index {i} is out of range")),
            o => Err(format!(
                "[json.exception.type_error.304] cannot use at() with {}",
                o.type_name()
            )),
        }
    }

    /// `operator[](key)` of a const object, for keys the caller checked (`contains`); null else.
    pub fn get_key(&self, key: &str) -> &Self {
        match self {
            Self::Obj(m) => m.get(key).unwrap_or(&NULL),
            _ => &NULL,
        }
    }

    /// `value(key, default)`: the member converted, or `None` (the default) when absent.
    pub fn member(&self, key: &str) -> Result<Option<&Self>, Error> {
        match self {
            Self::Obj(m) => Ok(m.get(key)),
            o => Err(format!(
                "[json.exception.type_error.306] cannot use value() with {}",
                o.type_name()
            )),
        }
    }

    /// `get<std::string>()`.
    pub fn str(&self) -> Result<&str, Error> {
        match self {
            Self::Str(s) => Ok(s),
            o => Err(format!(
                "[json.exception.type_error.302] type must be string, but is {}",
                o.type_name()
            )),
        }
    }

    /// `get<bool>()`.
    pub fn boolean(&self) -> Result<bool, Error> {
        match self {
            Self::Bool(b) => Ok(*b),
            o => Err(format!(
                "[json.exception.type_error.302] type must be boolean, but is {}",
                o.type_name()
            )),
        }
    }

    /// `get<int64_t>()` (also of a boolean or a float, as nlohmann converts them).
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_possible_truncation,
        reason = "C++ conversions"
    )]
    pub fn i64(&self) -> Result<i64, Error> {
        match self {
            Self::Int(v) => Ok(*v),
            Self::UInt(v) => Ok(*v as i64),
            Self::Float(v) => Ok(*v as i64),
            Self::Bool(b) => Ok(i64::from(*b)),
            o => Err(format!(
                "[json.exception.type_error.302] type must be number, but is {}",
                o.type_name()
            )),
        }
    }

    /// `get<uint64_t>()`.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation, reason = "C++ conversions")]
    pub fn u64(&self) -> Result<u64, Error> {
        match self {
            Self::UInt(v) => Ok(*v),
            Self::Float(v) => Ok(*v as u64),
            o => self.i64().map(|v| v as u64).map_err(|_| {
                format!(
                    "[json.exception.type_error.302] type must be number, but is {}",
                    o.type_name()
                )
            }),
        }
    }

    /// The elements a range-for visits: an array's, an object's values (key order), a primitive
    /// once, null none.
    pub fn items(&self) -> Vec<&Self> {
        match self {
            Self::Arr(a) => a.iter().collect(),
            Self::Obj(m) => m.values().collect(),
            Self::Null => Vec::new(),
            o => vec![o],
        }
    }

    /// `size()`: elements of an array / object, 0 for null, 1 for a primitive.
    pub fn len(&self) -> usize {
        self.items().len()
    }

    /// `size() == 0`
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Parses the first JSON value of `text` (what follows it is ignored, as `operator>>`).
pub fn parse(text: &str) -> Result<Json, Error> {
    let mut p = Parser {
        s: text.as_bytes(),
        at: 0,
    };
    p.ws();
    if p.at >= p.s.len() {
        return Err("[json.exception.parse_error.101] parse error at line 1, column 1: attempting to parse an empty input; check that your input string or stream contains the expected JSON".to_owned());
    }
    p.value(0)
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.at).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn err(&self, what: &str) -> Error {
        let before = self.s.get(..self.at).unwrap_or(self.s);
        let line = before.split(|c| *c == b'\n').count();
        let col = before.iter().rev().take_while(|c| **c != b'\n').count() + 1;
        format!("[json.exception.parse_error.101] parse error at line {line}, column {col}: syntax error - {what}")
    }

    fn lit(&mut self, word: &str, v: Json) -> Result<Json, Error> {
        if self.s.get(self.at..self.at + word.len()) == Some(word.as_bytes()) {
            self.at += word.len();
            Ok(v)
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, Error> {
        if depth > 512 {
            return Err(self.err("nesting too deep"));
        }
        self.ws();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.lit("true", Json::Bool(true)),
            Some(b'f') => self.lit("false", Json::Bool(false)),
            Some(b'n') => self.lit("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.err("unexpected character")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, Error> {
        self.at += 1;
        let mut m = BTreeMap::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Obj(m));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a string key"));
            }
            let k = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.at += 1;
            let v = self.value(depth + 1)?;
            m.insert(k, v);
            self.ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Obj(m));
                }
                _ => return Err(self.err("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, Error> {
        self.at += 1;
        let mut a = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Arr(a));
        }
        loop {
            a.push(self.value(depth + 1)?);
            self.ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Arr(a));
                }
                _ => return Err(self.err("expected ',' or ']'")),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let h = self
            .s
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.err("bad \\u escape"))?;
        let t = std::str::from_utf8(h).map_err(|_| self.err("bad \\u escape"))?;
        let v = u32::from_str_radix(t, 16).map_err(|_| self.err("bad \\u escape"))?;
        self.at += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, Error> {
        self.at += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self.err("missing closing quote"));
            };
            self.at += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|_| self.err("invalid UTF-8")),
                b'\\' => {
                    let e = self.peek().ok_or_else(|| self.err("bad escape"))?;
                    self.at += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut v = self.hex4()?;
                            if (0xd800..0xdc00).contains(&v) {
                                if self.s.get(self.at..self.at + 2) != Some(b"\\u") {
                                    return Err(self.err("bad surrogate"));
                                }
                                self.at += 2;
                                let lo = self.hex4()?;
                                if !(0xdc00..0xe000).contains(&lo) {
                                    return Err(self.err("bad surrogate"));
                                }
                                v = 0x10000 + ((v - 0xd800) << 10) + (lo - 0xdc00);
                            }
                            char::from_u32(v).ok_or_else(|| self.err("bad code point"))?
                        }
                        _ => return Err(self.err("bad escape")),
                    };
                    let mut b = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                }
                c if c < 0x20 => return Err(self.err("control character in a string")),
                c => out.push(c),
            }
        }
    }

    fn number(&mut self) -> Result<Json, Error> {
        let start = self.at;
        let mut float = false;
        while let Some(c) = self.peek() {
            match c {
                b'0'..=b'9' | b'-' | b'+' => {}
                b'.' | b'e' | b'E' => float = true,
                _ => break,
            }
            self.at += 1;
        }
        let t = std::str::from_utf8(self.s.get(start..self.at).unwrap_or_default()).unwrap_or("");
        if !float {
            if let Ok(v) = t.parse::<i64>() {
                return Ok(Json::Int(v));
            }
            if let Ok(v) = t.parse::<u64>() {
                return Ok(Json::UInt(v));
            }
        }
        t.parse::<f64>()
            .map(Json::Float)
            .map_err(|_| self.err("invalid number"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_and_the_accessors_errors() {
        let j = parse(r#"{"b": [1, -2, 18446744073709551615, 1.5, "x\u00e9", true, null], "a": {}, "a": 3}"#).unwrap();
        assert_eq!(j.at("a").unwrap(), &Json::Int(3));
        let b = j.at("b").unwrap();
        assert_eq!(b.at_index(2).unwrap(), &Json::UInt(u64::MAX));
        assert_eq!(b.at_index(4).unwrap().str().unwrap(), "x\u{e9}");
        assert_eq!(b.at_index(3).unwrap().i64().unwrap(), 1);
        assert_eq!(
            j.at("zz").unwrap_err(),
            "[json.exception.out_of_range.403] key 'zz' not found"
        );
        assert_eq!(
            b.at_index(0).unwrap().str().unwrap_err(),
            "[json.exception.type_error.302] type must be string, but is number"
        );
        assert_eq!(b.len(), 7);
        assert!(parse("   ").unwrap_err().contains("attempting to parse an empty input"));
        assert!(parse("[1,]").is_err());
        assert_eq!(parse("[1] trailing").unwrap(), Json::Arr(vec![Json::Int(1)]));
    }
}
