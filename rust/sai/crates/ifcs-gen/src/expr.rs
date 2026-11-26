//! A parser of Python expressions (`ast.parse(s, mode="eval")`) for the right-hand sides of a
//! ctypesgen file. The reference model stores every line that parses, whatever it holds, and
//! skips the ones that do not (`SyntaxError`), so this parser accepts the expression grammar,
//! not only the shapes the model can evaluate: names, numbers, strings, calls, attributes,
//! subscripts and slices, unary, binary, boolean and comparison operators, conditional
//! expressions, tuples, lists, sets, dicts, comprehensions, `lambda` and `await`. Outside it
//! (read as unparsable): `yield`, `:=`, nested quotes inside f-string replacement fields, line
//! continuations (one line is one expression here).
//!
//! The tree compares structurally ([`PartialEq`]) as `ast.dump` does: integer literals by value
//! (`0x10` equals `16`), no positions.

use crate::py;

/// An integer literal: Python's are unbounded, these are exact up to `i128`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum IntLit {
    /// The value.
    Small(i128),
    /// A literal outside `i128`, as its digits (a value the generator refuses to use).
    Big(String),
}

/// Unary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnOp {
    Neg,
    Pos,
    Invert,
    Not,
}

/// Binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mult,
    MatMult,
    Div,
    FloorDiv,
    Mod,
    Pow,
    LShift,
    RShift,
    BitOr,
    BitXor,
    BitAnd,
}

/// A parsed expression (`ast` node).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Expr {
    Int(IntLit),
    /// A float literal, by its bits.
    Float(u64),
    /// An imaginary literal, as written.
    Imag(String),
    /// A string literal's text between the quotes (adjacent literals joined).
    Str(String),
    /// A bytes literal's text.
    Bytes(String),
    /// An f-string or t-string, as written.
    Formatted(String),
    /// `None`.
    NoneConst,
    /// `True` / `False`.
    Bool(bool),
    /// `...`.
    Ellipsis,
    Name(String),
    Unary(UnOp, Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    /// `and` (true) / `or` (false) over the operands.
    BoolOp(bool, Vec<Expr>),
    /// A comparison chain: the first operand, then (operator, operand) pairs.
    Compare(Box<Expr>, Vec<(&'static str, Expr)>),
    IfExp(Box<Expr>, Box<Expr>, Box<Expr>),
    /// A call: function, positional arguments (`*x` as [`Expr::Starred`]), keywords (`None`: `**x`).
    Call(Box<Expr>, Vec<Expr>, Vec<(Option<String>, Expr)>),
    Starred(Box<Expr>),
    Attribute(Box<Expr>, String),
    Subscript(Box<Expr>, Box<Expr>),
    Slice(Option<Box<Expr>>, Option<Box<Expr>>, Option<Box<Expr>>),
    Tuple(Vec<Expr>),
    List(Vec<Expr>),
    Set(Vec<Expr>),
    /// Dict items (`None` key: `**x`).
    Dict(Vec<(Option<Expr>, Expr)>),
    /// A comprehension or generator expression (not evaluated: no further structure kept).
    Comprehension,
    /// A `lambda` (not evaluated).
    Lambda,
    Await(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Name(String),
    Int(IntLit),
    Float(f64),
    Imag(String),
    /// kind: 's' str, 'b' bytes, 'f' formatted; the text between the quotes (or the literal)
    Str(char, String),
    Op(&'static str),
}

const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
    "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal",
    "not", "or", "pass", "raise", "return", "try", "while", "with", "yield",
];

const OPS: &[&str] = &[
    "**=", "//=", ">>=", "<<=", "...", "->", ":=", "**", "//", "<<", ">>", "<=", ">=", "==", "!=", "+=", "-=", "*=",
    "/=", "%=", "&=", "|=", "^=", "@=", "+", "-", "*", "/", "%", "@", "&", "|", "^", "~", "<", ">", "(", ")", "[", "]",
    "{", "}", ",", ":", ".", ";", "=",
];

const STRING_PREFIXES: &[&str] = &["r", "u", "b", "br", "rb", "f", "fr", "rf", "t", "tr", "rt"];

/// Scans a number at `s[i..]` (a digit, or `.` and a digit). `None`: not a valid literal.
fn number(cs: &[char], i: &mut usize) -> Option<Tok> {
    let at = |k: usize| cs.get(k).copied();
    let start = *i;
    // a run of digits of radix with single underscores between them; lead: one may come first
    let run = |i: &mut usize, radix: u32, lead: bool| -> Option<String> {
        let mut out = String::new();
        let mut last_us = !lead;
        let mut first = true;
        while let Some(c) = at(*i) {
            if c == '_' {
                if last_us && !(first && lead) {
                    return None;
                }
                last_us = true;
            } else if c.is_digit(radix) {
                out.push(c);
                last_us = false;
            } else {
                break;
            }
            first = false;
            *i += 1;
        }
        (!last_us && !out.is_empty()).then_some(out)
    };
    if at(*i) == Some('0') && matches!(at(*i + 1), Some('x' | 'X' | 'o' | 'O' | 'b' | 'B')) {
        let radix = match at(*i + 1) {
            Some('x' | 'X') => 16,
            Some('o' | 'O') => 8,
            _ => 2,
        };
        *i += 2;
        let ds = run(i, radix, true)?;
        if at(*i).is_some_and(py::is_word) {
            return None; // "invalid hexadecimal literal" and the like
        }
        return Some(Tok::Int(match i128::from_str_radix(&ds, radix) {
            Ok(v) => IntLit::Small(v),
            Err(_) => IntLit::Big(format!("0{}{ds}", cs.get(start + 1).copied().unwrap_or('x'))),
        }));
    }
    let mut text = String::new();
    let mut is_float = false;
    if at(*i) == Some('.') {
        is_float = true;
    } else {
        text.push_str(&run(i, 10, false)?);
    }
    if at(*i) == Some('.') {
        is_float = true;
        text.push('.');
        *i += 1;
        if at(*i).is_some_and(|c| c.is_ascii_digit()) {
            text.push_str(&run(i, 10, false)?);
        } else if text == "." {
            return None;
        }
    }
    if matches!(at(*i), Some('e' | 'E')) {
        let save = *i;
        let mut k = *i + 1;
        let mut exp = String::from("e");
        if let Some(c @ ('+' | '-')) = at(k) {
            exp.push(c);
            k += 1;
        }
        if at(k).is_some_and(|c| c.is_ascii_digit()) {
            *i = k;
            exp.push_str(&run(i, 10, false)?);
            text.push_str(&exp);
            is_float = true;
        } else {
            *i = save;
            return None; // "1e" / "1ex": invalid decimal literal
        }
    }
    if matches!(at(*i), Some('j' | 'J')) {
        *i += 1;
        if at(*i).is_some_and(py::is_word) {
            return None;
        }
        return Some(Tok::Imag(cs.get(start..*i)?.iter().collect()));
    }
    if at(*i).is_some_and(py::is_word) {
        return None; // "invalid decimal literal"
    }
    if is_float {
        return text.parse::<f64>().ok().map(Tok::Float);
    }
    if text.starts_with('0') && text.chars().any(|c| c != '0') {
        return None; // leading zeros in a decimal integer
    }
    Some(Tok::Int(match text.parse::<i128>() {
        Ok(v) => IntLit::Small(v),
        Err(_) => IntLit::Big(text),
    }))
}

/// Scans a string literal whose quote is at `cs[*i]`; returns its text between the quotes.
fn string(cs: &[char], i: &mut usize) -> Option<String> {
    let q = *cs.get(*i)?;
    let triple = cs.get(*i + 1) == Some(&q) && cs.get(*i + 2) == Some(&q);
    let open = if triple { 3 } else { 1 };
    *i += open;
    let start = *i;
    loop {
        let c = *cs.get(*i)?;
        if c == '\\' {
            *i += 2;
            continue;
        }
        if c == '\n' && !triple {
            return None;
        }
        if c == q && (!triple || (cs.get(*i + 1) == Some(&q) && cs.get(*i + 2) == Some(&q))) {
            let text: String = cs.get(start..*i)?.iter().collect();
            *i += open;
            return Some(text);
        }
        *i += 1;
    }
}

fn tokenize(src: &str) -> Option<Vec<Tok>> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut level = 0usize;
    while let Some(&c) = cs.get(i) {
        if matches!(c, '(' | '[' | '{') {
            // the tokenizer's MAXLEVEL: "too many nested parentheses"
            if level >= 200 {
                return None;
            }
            level += 1;
        } else if matches!(c, ')' | ']' | '}') {
            level = level.saturating_sub(1);
        }
        if matches!(c, ' ' | '\t' | '\u{c}') {
            i += 1;
        } else if c == '#' {
            break;
        } else if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).is_some_and(char::is_ascii_digit)) {
            out.push(number(&cs, &mut i)?);
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while cs.get(i).copied().is_some_and(py::is_word) {
                i += 1;
            }
            let word: String = cs.get(start..i)?.iter().collect();
            let lower = word.to_ascii_lowercase();
            if matches!(cs.get(i), Some('\'' | '"')) && STRING_PREFIXES.contains(&lower.as_str()) {
                let text = string(&cs, &mut i)?;
                let kind = if lower.contains('b') {
                    'b'
                } else if lower.contains('f') || lower.contains('t') {
                    'f'
                } else {
                    's'
                };
                out.push(Tok::Str(kind, text));
            } else {
                out.push(Tok::Name(word));
            }
        } else if c == '\'' || c == '"' {
            out.push(Tok::Str('s', string(&cs, &mut i)?));
        } else {
            let op = OPS
                .iter()
                .find(|op| op.chars().enumerate().all(|(k, oc)| cs.get(i + k) == Some(&oc)))?;
            i += op.chars().count();
            out.push(Tok::Op(op));
        }
    }
    Some(out)
}

/// Parses one expression as `ast.parse(src, mode="eval")` does; `None` where Python raises
/// `SyntaxError` (or where the expression is outside the grammar above).
pub(crate) fn parse(src: &str) -> Option<Expr> {
    let toks = tokenize(src)?;
    let mut p = Parser { toks, pos: 0, depth: 0 };
    let e = p.expressions()?;
    (p.pos == p.toks.len()).then_some(e)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
}

type P<T> = Option<T>;

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn peek_at(&self, k: usize) -> Option<&Tok> {
        self.toks.get(self.pos + k)
    }
    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek(), Some(Tok::Op(o)) if *o == op)
    }
    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(Tok::Name(n)) if n == kw)
    }
    fn eat_op(&mut self, op: &str) -> bool {
        let yes = self.is_op(op);
        if yes {
            self.pos += 1;
        }
        yes
    }
    fn eat_kw(&mut self, kw: &str) -> bool {
        let yes = self.is_kw(kw);
        if yes {
            self.pos += 1;
        }
        yes
    }
    fn expect_op(&mut self, op: &str) -> P<()> {
        self.eat_op(op).then_some(())
    }
    /// A bound on the parser's recursion (the tokenizer's 200 bracket levels come first).
    fn enter(&mut self) -> P<()> {
        self.depth += 1;
        (self.depth < 450).then_some(())
    }

    /// `expressions`: one expression, or a tuple of them separated by commas.
    fn expressions(&mut self) -> P<Expr> {
        let first = self.expression()?;
        if !self.eat_op(",") {
            return Some(first);
        }
        let mut items = vec![first];
        while self.peek().is_some() && !self.is_op(")") {
            items.push(self.expression()?);
            if !self.eat_op(",") {
                break;
            }
        }
        Some(Expr::Tuple(items))
    }

    fn expression(&mut self) -> P<Expr> {
        self.enter()?;
        let r = self.expression_inner();
        self.depth -= 1;
        r
    }

    fn expression_inner(&mut self) -> P<Expr> {
        if self.eat_kw("lambda") {
            return self.lambda();
        }
        let body = self.disjunction()?;
        if !self.eat_kw("if") {
            return Some(body);
        }
        let test = self.disjunction()?;
        if !self.eat_kw("else") {
            return None;
        }
        let orelse = self.expression()?;
        Some(Expr::IfExp(Box::new(test), Box::new(body), Box::new(orelse)))
    }

    /// `lambda [params]: expression` (the parameters only checked for shape).
    fn lambda(&mut self) -> P<Expr> {
        loop {
            if self.eat_op(":") {
                break;
            }
            if !(self.eat_op("*") | self.eat_op("**") | self.eat_op("/")) {
                match self.peek() {
                    Some(Tok::Name(n)) if !KEYWORDS.contains(&n.as_str()) => self.pos += 1,
                    _ => return None,
                }
                if self.eat_op("=") {
                    self.expression()?;
                }
            } else if let Some(Tok::Name(n)) = self.peek() {
                if !KEYWORDS.contains(&n.as_str()) {
                    self.pos += 1;
                }
            }
            if !self.eat_op(",") {
                self.expect_op(":")?;
                break;
            }
        }
        self.expression()?;
        Some(Expr::Lambda)
    }

    fn disjunction(&mut self) -> P<Expr> {
        let first = self.conjunction()?;
        if !self.is_kw("or") {
            return Some(first);
        }
        let mut items = vec![first];
        while self.eat_kw("or") {
            items.push(self.conjunction()?);
        }
        Some(Expr::BoolOp(false, items))
    }

    fn conjunction(&mut self) -> P<Expr> {
        let first = self.inversion()?;
        if !self.is_kw("and") {
            return Some(first);
        }
        let mut items = vec![first];
        while self.eat_kw("and") {
            items.push(self.inversion()?);
        }
        Some(Expr::BoolOp(true, items))
    }

    fn inversion(&mut self) -> P<Expr> {
        if self.eat_kw("not") {
            self.enter()?;
            let e = self.inversion();
            self.depth -= 1;
            return Some(Expr::Unary(UnOp::Not, Box::new(e?)));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> P<Expr> {
        let first = self.bitor()?;
        let mut rest = Vec::new();
        loop {
            let op = match self.peek() {
                Some(&Tok::Op(o)) if matches!(o, "==" | "!=" | "<" | "<=" | ">" | ">=") => {
                    self.pos += 1;
                    o
                }
                Some(Tok::Name(n)) if n == "in" => {
                    self.pos += 1;
                    "in"
                }
                Some(Tok::Name(n)) if n == "not" && matches!(self.peek_at(1), Some(Tok::Name(m)) if m == "in") => {
                    self.pos += 2;
                    "not in"
                }
                Some(Tok::Name(n)) if n == "is" => {
                    self.pos += 1;
                    if self.eat_kw("not") { "is not" } else { "is" }
                }
                _ => break,
            };
            rest.push((op, self.bitor()?));
        }
        Some(if rest.is_empty() {
            first
        } else {
            Expr::Compare(Box::new(first), rest)
        })
    }

    /// One level of left-associative binary operators over `next`.
    fn binary(&mut self, ops: &[(&str, BinOp)], next: fn(&mut Self) -> P<Expr>) -> P<Expr> {
        let mut left = next(self)?;
        'outer: loop {
            for (tok, op) in ops {
                if self.eat_op(tok) {
                    let right = next(self)?;
                    left = Expr::Bin(*op, Box::new(left), Box::new(right));
                    continue 'outer;
                }
            }
            return Some(left);
        }
    }
    fn bitor(&mut self) -> P<Expr> {
        self.binary(&[("|", BinOp::BitOr)], Self::bitxor)
    }
    fn bitxor(&mut self) -> P<Expr> {
        self.binary(&[("^", BinOp::BitXor)], Self::bitand)
    }
    fn bitand(&mut self) -> P<Expr> {
        self.binary(&[("&", BinOp::BitAnd)], Self::shift)
    }
    fn shift(&mut self) -> P<Expr> {
        self.binary(&[("<<", BinOp::LShift), (">>", BinOp::RShift)], Self::sum)
    }
    fn sum(&mut self) -> P<Expr> {
        self.binary(&[("+", BinOp::Add), ("-", BinOp::Sub)], Self::term)
    }
    fn term(&mut self) -> P<Expr> {
        self.binary(
            &[
                ("*", BinOp::Mult),
                ("/", BinOp::Div),
                ("//", BinOp::FloorDiv),
                ("%", BinOp::Mod),
                ("@", BinOp::MatMult),
            ],
            Self::factor,
        )
    }

    fn factor(&mut self) -> P<Expr> {
        let op = match self.peek() {
            Some(Tok::Op("+")) => UnOp::Pos,
            Some(Tok::Op("-")) => UnOp::Neg,
            Some(Tok::Op("~")) => UnOp::Invert,
            _ => return self.power(),
        };
        self.pos += 1;
        self.enter()?;
        let e = self.factor();
        self.depth -= 1;
        Some(Expr::Unary(op, Box::new(e?)))
    }

    fn power(&mut self) -> P<Expr> {
        let base = if self.eat_kw("await") {
            Expr::Await(Box::new(self.primary()?))
        } else {
            self.primary()?
        };
        if self.eat_op("**") {
            self.enter()?;
            let exp = self.factor();
            self.depth -= 1;
            return Some(Expr::Bin(BinOp::Pow, Box::new(base), Box::new(exp?)));
        }
        Some(base)
    }

    fn primary(&mut self) -> P<Expr> {
        let mut e = self.atom()?;
        loop {
            if self.eat_op(".") {
                match self.peek().cloned() {
                    Some(Tok::Name(n)) if !KEYWORDS.contains(&n.as_str()) => {
                        self.pos += 1;
                        e = Expr::Attribute(Box::new(e), n);
                    }
                    _ => return None,
                }
            } else if self.eat_op("(") {
                e = self.call(e)?;
            } else if self.eat_op("[") {
                let index = self.slices()?;
                self.expect_op("]")?;
                e = Expr::Subscript(Box::new(e), Box::new(index));
            } else {
                return Some(e);
            }
        }
    }

    /// The arguments of a call after its `(`.
    fn call(&mut self, func: Expr) -> P<Expr> {
        let (mut args, mut kws) = (Vec::new(), Vec::new());
        while !self.is_op(")") {
            if self.eat_op("**") {
                kws.push((None, self.expression()?));
            } else if self.eat_op("*") {
                args.push(Expr::Starred(Box::new(self.expression()?)));
            } else if let (Some(Tok::Name(n)), Some(Tok::Op("="))) = (self.peek().cloned(), self.peek_at(1)) {
                if KEYWORDS.contains(&n.as_str()) {
                    return None;
                }
                self.pos += 2;
                kws.push((Some(n), self.expression()?));
            } else {
                if !kws.is_empty() {
                    return None; // positional argument follows keyword argument
                }
                let a = self.expression()?;
                if self.is_kw("for") || self.is_kw("async") {
                    // a generator expression: the only argument
                    self.comprehension()?;
                    if !args.is_empty() || !self.is_op(")") {
                        return None;
                    }
                    args.push(Expr::Comprehension);
                    break;
                }
                args.push(a);
            }
            if !self.eat_op(",") {
                break;
            }
        }
        self.expect_op(")")?;
        Some(Expr::Call(Box::new(func), args, kws))
    }

    /// `for targets in iter [if cond]...` clauses after a comprehension's element.
    fn comprehension(&mut self) -> P<()> {
        let mut any = false;
        loop {
            let asynchronous = self.eat_kw("async");
            if !self.eat_kw("for") {
                return (any && !asynchronous).then_some(());
            }
            any = true;
            loop {
                self.eat_op("*");
                self.bitor()?;
                if !self.eat_op(",") || self.is_kw("in") {
                    break;
                }
            }
            if !self.eat_kw("in") {
                return None;
            }
            self.disjunction()?;
            while self.eat_kw("if") {
                self.disjunction()?;
            }
        }
    }

    /// The inside of `[...]` after a primary.
    fn slices(&mut self) -> P<Expr> {
        let first = self.slice()?;
        if !self.eat_op(",") {
            return Some(first);
        }
        let mut items = vec![first];
        while !self.is_op("]") {
            items.push(self.slice()?);
            if !self.eat_op(",") {
                break;
            }
        }
        Some(Expr::Tuple(items))
    }

    fn slice(&mut self) -> P<Expr> {
        if self.eat_op("*") {
            return Some(Expr::Starred(Box::new(self.bitor()?)));
        }
        let lower = if self.is_op(":") {
            None
        } else {
            Some(Box::new(self.expression()?))
        };
        if !self.eat_op(":") {
            return lower.map(|b| *b);
        }
        let part = |p: &mut Self| -> P<Option<Box<Expr>>> {
            if p.is_op(":") || p.is_op("]") || p.is_op(",") {
                Some(None)
            } else {
                Some(Some(Box::new(p.expression()?)))
            }
        };
        let upper = part(self)?;
        let step = if self.eat_op(":") { part(self)? } else { None };
        Some(Expr::Slice(lower, upper, step))
    }

    /// An element of a tuple, list or set display: an expression or `*expr`.
    fn element(&mut self) -> P<Expr> {
        if self.eat_op("*") {
            return Some(Expr::Starred(Box::new(self.bitor()?)));
        }
        self.expression()
    }

    /// Elements after the first of a display up to `close`.
    fn rest_of_display(&mut self, first: Expr, close: &str) -> P<Vec<Expr>> {
        let mut items = vec![first];
        while self.eat_op(",") {
            if self.is_op(close) {
                break;
            }
            items.push(self.element()?);
        }
        self.expect_op(close)?;
        Some(items)
    }

    fn atom(&mut self) -> P<Expr> {
        self.enter()?;
        let r = self.atom_inner();
        self.depth -= 1;
        r
    }

    fn atom_inner(&mut self) -> P<Expr> {
        let tok = self.peek().cloned()?;
        self.pos += 1;
        match tok {
            Tok::Name(n) => match n.as_str() {
                "None" => Some(Expr::NoneConst),
                "True" => Some(Expr::Bool(true)),
                "False" => Some(Expr::Bool(false)),
                k if KEYWORDS.contains(&k) => None,
                _ => Some(Expr::Name(n)),
            },
            Tok::Int(v) => Some(Expr::Int(v)),
            Tok::Float(f) => Some(Expr::Float(f.to_bits())),
            Tok::Imag(s) => Some(Expr::Imag(s)),
            Tok::Str(kind, text) => self.strings(kind, text),
            Tok::Op("...") => Some(Expr::Ellipsis),
            Tok::Op("(") => self.paren(),
            Tok::Op("[") => {
                if self.eat_op("]") {
                    return Some(Expr::List(Vec::new()));
                }
                let first = self.element()?;
                if self.is_kw("for") || self.is_kw("async") {
                    self.comprehension()?;
                    self.expect_op("]")?;
                    return Some(Expr::Comprehension);
                }
                Some(Expr::List(self.rest_of_display(first, "]")?))
            }
            Tok::Op("{") => self.brace(),
            Tok::Op(_) => None,
        }
    }

    /// Adjacent string literals are one constant; str and bytes do not mix.
    fn strings(&mut self, kind: char, text: String) -> P<Expr> {
        let (mut kind, mut text) = (kind, text);
        while let Some(Tok::Str(k, t)) = self.peek().cloned() {
            if (k == 'b') != (kind == 'b') {
                return None;
            }
            if k == 'f' {
                kind = 'f';
            }
            text.push_str(&t);
            self.pos += 1;
        }
        Some(match kind {
            'b' => Expr::Bytes(text),
            'f' => Expr::Formatted(text),
            _ => Expr::Str(text),
        })
    }

    /// After `(`: the empty tuple, a group, a tuple or a generator expression.
    fn paren(&mut self) -> P<Expr> {
        if self.eat_op(")") {
            return Some(Expr::Tuple(Vec::new()));
        }
        let first = self.element()?;
        if self.is_kw("for") || self.is_kw("async") {
            self.comprehension()?;
            self.expect_op(")")?;
            return Some(Expr::Comprehension);
        }
        if self.eat_op(")") {
            return (!matches!(first, Expr::Starred(_))).then_some(first);
        }
        let mut items = vec![first];
        while self.eat_op(",") {
            if self.is_op(")") {
                break;
            }
            items.push(self.element()?);
        }
        self.expect_op(")")?;
        Some(Expr::Tuple(items))
    }

    /// After `{`: a dict or a set display or comprehension.
    fn brace(&mut self) -> P<Expr> {
        if self.eat_op("}") {
            return Some(Expr::Dict(Vec::new()));
        }
        let first_key = if self.eat_op("**") { None } else { Some(self.element()?) };
        let Some(key) = first_key else {
            let v = self.bitor()?;
            return self.dict_rest(vec![(None, v)]);
        };
        if self.eat_op(":") {
            let v = self.expression()?;
            if self.is_kw("for") || self.is_kw("async") {
                self.comprehension()?;
                self.expect_op("}")?;
                return Some(Expr::Comprehension);
            }
            return self.dict_rest(vec![(Some(key), v)]);
        }
        if self.is_kw("for") || self.is_kw("async") {
            self.comprehension()?;
            self.expect_op("}")?;
            return Some(Expr::Comprehension);
        }
        Some(Expr::Set(self.rest_of_display(key, "}")?))
    }

    fn dict_rest(&mut self, mut items: Vec<(Option<Expr>, Expr)>) -> P<Expr> {
        while self.eat_op(",") {
            if self.is_op("}") {
                break;
            }
            if self.eat_op("**") {
                items.push((None, self.bitor()?));
            } else {
                let k = self.expression()?;
                self.expect_op(":")?;
                items.push((Some(k), self.expression()?));
            }
        }
        self.expect_op("}")?;
        Some(Expr::Dict(items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(v: i128) -> Expr {
        Expr::Int(IntLit::Small(v))
    }
    fn name(n: &str) -> Expr {
        Expr::Name(n.into())
    }

    #[test]
    fn ctypesgen_shapes() {
        assert_eq!(parse("9"), Some(int(9)));
        assert_eq!(parse("0x1F"), Some(int(31)));
        assert_eq!(parse("  c_uint8  "), Some(name("c_uint8")));
        assert_eq!(parse("(-1)"), Some(Expr::Unary(UnOp::Neg, Box::new(int(1)))));
        assert_eq!(
            parse("(1 << 3) - 1"),
            Some(Expr::Bin(
                BinOp::Sub,
                Box::new(Expr::Bin(BinOp::LShift, Box::new(int(1)), Box::new(int(3)))),
                Box::new(int(1))
            ))
        );
        assert_eq!(
            parse("POINTER(c_int)"),
            Some(Expr::Call(Box::new(name("POINTER")), vec![name("c_int")], vec![]))
        );
        assert_eq!(
            parse("c_uint8 * int(6)"),
            Some(Expr::Bin(
                BinOp::Mult,
                Box::new(name("c_uint8")),
                Box::new(Expr::Call(Box::new(name("int")), vec![int(6)], vec![]))
            ))
        );
        // the comment ctypesgen appends to a constant
        assert_eq!(parse("9# /net/x/y.h: 127"), Some(int(9)));
        assert!(matches!(
            parse("(c_uint32).in_dll(_libs[\"x.so\"], \"y\")"),
            Some(Expr::Call(..))
        ));
        assert!(matches!(
            parse("((c_uint32 (ord_if_char(X))).value << S)"),
            Some(Expr::Bin(BinOp::LShift, ..))
        ));
        assert!(matches!(
            parse("(__BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__)"),
            Some(Expr::Compare(..))
        ));
        assert_eq!(parse("100.0"), Some(Expr::Float(100.0f64.to_bits())));
        assert_eq!(parse("'/tmp/x'"), Some(Expr::Str("/tmp/x".into())));
        assert_eq!(parse("None"), Some(Expr::NoneConst));
        assert_eq!(parse("True"), Some(Expr::Bool(true)));
    }

    #[test]
    fn grammar_beyond_the_model() {
        for ok in [
            "a if b else c",
            "not a and b or c",
            "a < b <= c",
            "a is not b",
            "x not in y",
            "f(a, *b, k=1, **d)",
            "f(x for x in y)",
            "[x for x in y if x]",
            "{a: b for a, b in c}",
            "{1, 2}",
            "{}",
            "{**a, 'b': 1}",
            "lambda x, y=1: x + y",
            "lambda: 0",
            "a[1:2, ::3]",
            "a, b",
            "(a,)",
            "()",
            "-x ** -y",
            "'a' 'b'",
            "b'x'",
            "f'{x}'",
            "r'\\d'",
            "...",
            "1j",
            "1e5",
            ".5",
            "1_000",
            "0o17",
            "await x",
            "a @ b",
            "a // b % c",
        ] {
            assert!(parse(ok).is_some(), "{ok}");
        }
        for bad in [
            "",
            "0123",
            "1__0",
            "a b",
            "(a",
            "a)",
            "class",
            "x = 1",
            "a +",
            "f(a=1, b)",
            "'abc",
            "1x",
            "0xg",
            "a.1",
            "x := 1",
            "yield x",
            "*a",
            "(*a)",
            "'a' b'b'",
            "\\",
            "a ! b",
            "$",
            "if",
            "lambda x x: 1",
        ] {
            assert!(parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn big_and_deep() {
        assert_eq!(
            parse("340282366920938463463374607431768211456"),
            Some(Expr::Int(IntLit::Big("340282366920938463463374607431768211456".into())))
        );
        // 200 levels parse (Python's limit; the thread's stack, not the grammar, bounds a test's depth)
        let deep = format!("{}1{}", "(".repeat(200), ")".repeat(200));
        let ok = std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn(move || parse(&deep).is_some());
        assert!(ok.expect("thread").join().expect("join"));
        assert!(parse(&format!("{}1{}", "(".repeat(201), ")".repeat(201))).is_none());
        assert!(parse(&format!("{}1", "-".repeat(2000))).is_none());
    }
}
