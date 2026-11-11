//! Tera 2.4's parser, ported (`parsing/parser.rs`): the same tree from the
//! same tokens, operator precedence included, since which operand a filter
//! applies to decides a bound. It runs only on templates Tera compiled, so
//! it doesn't repeat Tera's validation (reserved names, where blocks may
//! go, duplicate arguments): an input it can't read is an error, never a
//! guess.

use super::lexer::{Spanned, Tok};

/// A literal value.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Lit {
    None,
    Bool,
    /// A number's magnitude (a float rounded up; NaN and infinity are
    /// `u64::MAX`).
    Number(u64),
    Str(usize),
    Array(Vec<Lit>),
    Map(Vec<(usize, Lit)>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Unary {
    Not,
    Minus,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Binary {
    Mul,
    Div,
    Mod,
    Plus,
    Minus,
    FloorDiv,
    Power,
    LessThan,
    GreaterThan,
    LessThanOrEqual,
    GreaterThanOrEqual,
    Equal,
    NotEqual,
    And,
    Or,
    StrConcat,
    In,
    Is,
    Pipe,
}

pub(super) type Kwargs = Vec<(String, Expr)>;

/// A map or component-attribute entry.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Entry {
    /// The key's length (for a map's size) and the value.
    Pair(String, Expr),
    Spread(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Expr {
    Lit(Lit),
    /// `[a, ...b]`: each item with whether it's spread.
    Array(Vec<(bool, Expr)>),
    Map(Vec<Entry>),
    Var(String),
    Attr(Box<Expr>, String),
    Item(Box<Expr>, Box<Expr>),
    Slice(Box<Expr>, Vec<Expr>),
    Filter(Box<Expr>, String, Kwargs),
    Test(Box<Expr>, Kwargs),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Comprehension {
        expr: Box<Expr>,
        key: Option<String>,
        value: String,
        target: Box<Expr>,
        condition: Option<Box<Expr>>,
    },
    Component {
        name: String,
        attrs: Vec<Entry>,
        body: Vec<Node>,
    },
    Call(String, Kwargs),
    Unary(Unary, Box<Expr>),
    Binary(Binary, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Node {
    Content(usize),
    Expr(Expr),
    Set {
        name: String,
        value: Expr,
        global: bool,
    },
    BlockSet {
        name: String,
        filters: Vec<(String, Kwargs)>,
        body: Vec<Node>,
        global: bool,
    },
    Include(String),
    Block(String, Vec<Node>),
    For {
        key: Option<String>,
        value: String,
        target: Expr,
        body: Vec<Node>,
        else_body: Vec<Node>,
    },
    Break,
    Continue,
    If(Expr, Vec<Node>, Vec<Node>),
    FilterSection(Vec<(String, Kwargs)>, Vec<Node>),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Param {
    pub name: String,
    pub default: Option<Lit>,
    pub implicit: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ComponentDef {
    pub name: String,
    pub params: Vec<Param>,
    pub rest: Option<String>,
    pub body: Vec<Node>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Parsed {
    pub parent: Option<String>,
    pub nodes: Vec<Node>,
    pub components: Vec<ComponentDef>,
}

/// Where parsing stopped: a byte offset in the source.
pub(super) type ParseError = usize;

pub(super) fn parse(tokens: &[Spanned<'_>], len: usize) -> Result<Parsed, ParseError> {
    let mut parser = Parser {
        tokens,
        pos: 0,
        len,
        in_loop: 0,
        output: Parsed::default(),
    };
    let nodes = parser.until(|_| false)?;
    parser.output.nodes = nodes;
    Ok(parser.output)
}

struct Parser<'t, 'a> {
    tokens: &'t [Spanned<'a>],
    pos: usize,
    len: usize,
    /// How many `for` bodies enclose the position (`loop.index` & co).
    in_loop: usize,
    output: Parsed,
}

const TERNARY_L_BP: u8 = 0;

/// A float's magnitude, rounded up (`as` saturates; NaN is the maximum).
fn magnitude(f: f64) -> u64 {
    if f.is_nan() {
        u64::MAX
    } else {
        f.abs().ceil() as u64
    }
}

fn binding_power(op: Binary) -> (u8, u8) {
    use Binary::*;
    match op {
        Or => (1, 2),
        And => (3, 4),
        In | Is => (5, 6),
        Equal | NotEqual | LessThan | LessThanOrEqual | GreaterThan | GreaterThanOrEqual => (7, 8),
        Plus | Minus => (11, 12),
        Mul | Div | Mod | StrConcat | FloorDiv => (13, 14),
        Power => (16, 15),
        Pipe => (17, 18),
    }
}

impl<'a> Parser<'_, 'a> {
    fn peek(&self) -> Option<&Tok<'a>> {
        self.tokens.get(self.pos).map(|(token, _)| token)
    }

    fn here(&self) -> usize {
        self.tokens.get(self.pos).map_or(self.len, |(_, at)| *at)
    }

    fn next(&mut self) -> Result<Tok<'a>, ParseError> {
        let token = self.tokens.get(self.pos).ok_or(self.len)?.0.clone();
        self.pos += 1;
        Ok(token)
    }

    fn expect(&mut self, want: &Tok<'_>) -> Result<(), ParseError> {
        let at = self.here();
        if self.next()? == *want {
            Ok(())
        } else {
            Err(at)
        }
    }

    fn ident(&mut self) -> Result<&'a str, ParseError> {
        let at = self.here();
        match self.next()? {
            Tok::Ident(name) => Ok(name),
            _ => Err(at),
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        let at = self.here();
        match self.next()? {
            Tok::Str(s) => Ok(s),
            _ => Err(at),
        }
    }

    fn is(&self, token: &Tok<'_>) -> bool {
        self.peek() == Some(token)
    }

    // ---- expressions -------------------------------------------------

    fn expr(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let at = self.here();
        let mut lhs = match self.next()? {
            Tok::Integer(i) => Expr::Lit(Lit::Number(i.unsigned_abs())),
            Tok::Float(f) => Expr::Lit(Lit::Number(magnitude(f))),
            Tok::Str(s) => Expr::Lit(Lit::Str(s.len())),
            Tok::Bool(_) => Expr::Lit(Lit::Bool),
            Tok::Ident("none" | "None" | "null") => Expr::Lit(Lit::None),
            token @ (Tok::Minus | Tok::Ident("not")) => {
                let (op, r_bp) = if token == Tok::Minus {
                    (Unary::Minus, 20)
                } else {
                    (Unary::Not, 5)
                };
                let expr = self.expr(r_bp)?;
                Expr::Unary(op, Box::new(expr))
            }
            Tok::Ident(name) => self.ident_expr(name)?,
            Tok::LessThan => {
                let name = self.component_name()?;
                let attrs = self.component_attrs()?;
                self.expect(&Tok::Div)?;
                self.expect(&Tok::GreaterThan)?;
                Expr::Component {
                    name,
                    attrs,
                    body: Vec::new(),
                }
            }
            Tok::LeftBrace => self.map()?,
            Tok::LeftBracket => self.array()?,
            Tok::LeftParen => {
                let inner = self.expr(0)?;
                self.expect(&Tok::RightParen)?;
                inner
            }
            _ => return Err(at),
        };

        let mut negated = false;
        while let Some(token) = self.peek() {
            let op = match token {
                Tok::Mul => Binary::Mul,
                Tok::Div => Binary::Div,
                Tok::FloorDiv => Binary::FloorDiv,
                Tok::Mod => Binary::Mod,
                Tok::Plus => Binary::Plus,
                Tok::Minus => Binary::Minus,
                Tok::Power => Binary::Power,
                Tok::LessThan => Binary::LessThan,
                Tok::LessThanOrEqual => Binary::LessThanOrEqual,
                Tok::GreaterThan => Binary::GreaterThan,
                Tok::GreaterThanOrEqual => Binary::GreaterThanOrEqual,
                Tok::Equal => Binary::Equal,
                Tok::NotEqual => Binary::NotEqual,
                Tok::Tilde => Binary::StrConcat,
                Tok::Ident("not") => {
                    // Only `not in`, checked against `min_bp` like `in`.
                    if binding_power(Binary::In).0 < min_bp {
                        break;
                    }
                    let at = self.here();
                    self.pos += 1;
                    if !self.is(&Tok::Ident("in")) {
                        return Err(at);
                    }
                    negated = true;
                    continue;
                }
                Tok::Ident("in") => Binary::In,
                Tok::Ident("and") => Binary::And,
                Tok::Ident("or") => Binary::Or,
                Tok::Ident("is") => Binary::Is,
                Tok::LeftBracket => {
                    lhs = self.subscript(lhs)?;
                    continue;
                }
                Tok::Ident("if") => {
                    if TERNARY_L_BP < min_bp {
                        break;
                    }
                    self.pos += 1;
                    let condition = self.expr(0)?;
                    self.expect(&Tok::Ident("else"))?;
                    let otherwise = self.expr(0)?;
                    return Ok(Expr::Ternary(
                        Box::new(condition),
                        Box::new(lhs),
                        Box::new(otherwise),
                    ));
                }
                Tok::Pipe => Binary::Pipe,
                _ => break,
            };
            let (l_bp, r_bp) = binding_power(op);
            if l_bp < min_bp {
                break;
            }
            self.pos += 1;
            if op == Binary::Is && self.is(&Tok::Ident("not")) {
                self.pos += 1;
                negated = true;
            }
            lhs = match op {
                Binary::Is => {
                    self.ident()?;
                    let kwargs = if self.is(&Tok::LeftParen) {
                        self.kwargs()?
                    } else {
                        Vec::new()
                    };
                    Expr::Test(Box::new(lhs), kwargs)
                }
                Binary::Pipe => {
                    let name = self.ident()?;
                    let kwargs = if self.is(&Tok::LeftParen) {
                        self.kwargs()?
                    } else {
                        Vec::new()
                    };
                    Expr::Filter(Box::new(lhs), name.to_owned(), kwargs)
                }
                _ => {
                    let rhs = self.expr(r_bp)?;
                    Expr::Binary(op, Box::new(lhs), Box::new(rhs))
                }
            };
            if negated {
                lhs = Expr::Unary(Unary::Not, Box::new(lhs));
                negated = false;
            }
        }
        Ok(lhs)
    }

    /// An identifier: a function call, `loop.*` in a loop, or a variable
    /// with `.`/`?.`/`[…]` access.
    fn ident_expr(&mut self, name: &str) -> Result<Expr, ParseError> {
        if self.is(&Tok::LeftParen) {
            let kwargs = self.kwargs()?;
            return Ok(Expr::Call(name.to_owned(), kwargs));
        }
        let mut expr = Expr::Var(name.to_owned());
        loop {
            match self.peek() {
                Some(Tok::Dot | Tok::QuestionMarkDot) => {
                    self.pos += 1;
                    let attr = self.ident()?;
                    expr = if name == "loop" && self.in_loop > 0 && expr == Expr::Var("loop".into())
                    {
                        // `loop.index` & co: a number below the loop's length.
                        Expr::Var(format!("__tera_loop_{attr}"))
                    } else {
                        Expr::Attr(Box::new(expr), attr.to_owned())
                    };
                }
                Some(Tok::LeftBracket | Tok::QuestionMarkLeftBracket) => {
                    expr = self.subscript(expr)?;
                }
                Some(Tok::LeftParen) => return Err(self.here()),
                _ => return Ok(expr),
            }
        }
    }

    fn subscript(&mut self, expr: Expr) -> Result<Expr, ParseError> {
        self.pos += 1; // `[` or `?[`
        let mut parts = Vec::new();
        let mut slice = false;
        if !self.is(&Tok::Colon) {
            parts.push(self.expr(0)?);
        }
        if self.is(&Tok::Colon) {
            self.pos += 1;
            slice = true;
            if !matches!(self.peek(), Some(Tok::Colon | Tok::RightBracket)) {
                parts.push(self.expr(0)?);
            }
            if self.is(&Tok::Colon) {
                self.pos += 1;
                parts.push(self.expr(0)?);
            }
        }
        self.expect(&Tok::RightBracket)?;
        Ok(if slice {
            Expr::Slice(Box::new(expr), parts)
        } else {
            let index = parts.pop().ok_or(self.here())?;
            Expr::Item(Box::new(expr), Box::new(index))
        })
    }

    fn kwargs(&mut self) -> Result<Kwargs, ParseError> {
        self.expect(&Tok::LeftParen)?;
        let mut kwargs = Vec::new();
        loop {
            if self.is(&Tok::RightParen) {
                break;
            }
            if !kwargs.is_empty() {
                self.expect(&Tok::Comma)?;
            }
            if self.is(&Tok::RightParen) {
                break;
            }
            let name = self.ident()?;
            self.expect(&Tok::Assign)?;
            kwargs.push((name.to_owned(), self.expr(0)?));
        }
        self.expect(&Tok::RightParen)?;
        Ok(kwargs)
    }

    fn component_name(&mut self) -> Result<String, ParseError> {
        let mut name = self.ident()?.to_owned();
        while self.is(&Tok::Dot) {
            self.pos += 1;
            name.push('.');
            name.push_str(self.ident()?);
        }
        Ok(name)
    }

    fn component_attrs(&mut self) -> Result<Vec<Entry>, ParseError> {
        let mut attrs = Vec::new();
        loop {
            match self.peek() {
                Some(Tok::Ident(name)) => {
                    let name = name.to_string();
                    self.pos += 1;
                    let value = if self.is(&Tok::Assign) {
                        self.pos += 1;
                        let at = self.here();
                        match self.next()? {
                            Tok::Str(s) => Expr::Lit(Lit::Str(s.len())),
                            Tok::LeftBrace => {
                                let value = self.expr(0)?;
                                self.expect(&Tok::RightBrace)?;
                                value
                            }
                            _ => return Err(at),
                        }
                    } else {
                        Expr::Var(name.clone())
                    };
                    attrs.push(Entry::Pair(name, value));
                }
                Some(Tok::LeftBrace) => {
                    self.pos += 1;
                    self.expect(&Tok::Spread)?;
                    let value = self.expr(0)?;
                    self.expect(&Tok::RightBrace)?;
                    attrs.push(Entry::Spread(value));
                }
                _ => return Ok(attrs),
            }
        }
    }

    fn map(&mut self) -> Result<Expr, ParseError> {
        let mut entries = Vec::new();
        loop {
            if self.is(&Tok::RightBrace) {
                break;
            }
            if !entries.is_empty() {
                self.expect(&Tok::Comma)?;
            }
            if self.is(&Tok::RightBrace) {
                break;
            }
            if self.is(&Tok::Spread) {
                self.pos += 1;
                entries.push(Entry::Spread(self.expr(0)?));
                continue;
            }
            let at = self.here();
            let key = match self.next()? {
                Tok::Str(s) => s,
                Tok::Integer(i) => i.to_string(),
                Tok::Bool(b) => b.to_string(),
                _ => return Err(at),
            };
            self.expect(&Tok::Colon)?;
            entries.push(Entry::Pair(key, self.expr(0)?));
        }
        self.expect(&Tok::RightBrace)?;
        Ok(Expr::Map(entries))
    }

    fn array(&mut self) -> Result<Expr, ParseError> {
        let mut items = Vec::new();
        loop {
            if self.is(&Tok::RightBracket) {
                break;
            }
            if !items.is_empty() {
                self.expect(&Tok::Comma)?;
            }
            if self.is(&Tok::RightBracket) {
                break;
            }
            if self.is(&Tok::Spread) {
                self.pos += 1;
                items.push((true, self.expr(0)?));
                continue;
            }
            let item = self.expr(0)?;
            if items.is_empty() && self.is(&Tok::Ident("for")) {
                return self.comprehension(item);
            }
            items.push((false, item));
        }
        self.expect(&Tok::RightBracket)?;
        Ok(Expr::Array(items))
    }

    fn comprehension(&mut self, expr: Expr) -> Result<Expr, ParseError> {
        self.expect(&Tok::Ident("for"))?;
        let mut value = self.ident()?.to_owned();
        let mut key = None;
        if self.is(&Tok::Comma) {
            self.pos += 1;
            key = Some(value);
            value = self.ident()?.to_owned();
        }
        self.expect(&Tok::Ident("in"))?;
        let target = self.expr(TERNARY_L_BP + 1)?;
        let condition = if self.is(&Tok::Ident("if")) {
            self.pos += 1;
            Some(Box::new(self.expr(TERNARY_L_BP + 1)?))
        } else {
            None
        };
        self.expect(&Tok::RightBracket)?;
        Ok(Expr::Comprehension {
            expr: Box::new(expr),
            key,
            value,
            target: Box::new(target),
            condition,
        })
    }

    /// A literal for a component argument's default.
    fn literal(&mut self) -> Result<Lit, ParseError> {
        let at = self.here();
        match self.expr(0)? {
            Expr::Lit(lit) => Ok(lit),
            Expr::Unary(Unary::Minus, inner) if matches!(*inner, Expr::Lit(Lit::Number(_))) => {
                match *inner {
                    Expr::Lit(lit) => Ok(lit),
                    _ => unreachable!(),
                }
            }
            Expr::Array(items) => Ok(Lit::Array(
                items
                    .into_iter()
                    .map(|(spread, item)| match (spread, item) {
                        (false, Expr::Lit(lit)) => Ok(lit),
                        _ => Err(at),
                    })
                    .collect::<Result<_, _>>()?,
            )),
            Expr::Map(entries) => Ok(Lit::Map(
                entries
                    .into_iter()
                    .map(|entry| match entry {
                        Entry::Pair(key, Expr::Lit(lit)) => Ok((key.len(), lit)),
                        _ => Err(at),
                    })
                    .collect::<Result<_, _>>()?,
            )),
            _ => Err(at),
        }
    }

    // ---- statements --------------------------------------------------

    /// Nodes up to a tag whose first token `end` accepts (left unread
    /// after the `{%`), or the end of the input.
    fn until(&mut self, end: impl Fn(&Tok<'_>) -> bool) -> Result<Vec<Node>, ParseError> {
        let mut nodes = Vec::new();
        while self.pos < self.tokens.len() {
            match self.next()? {
                Tok::Content(len) => nodes.push(Node::Content(len)),
                Tok::VariableStart => {
                    let expr = self.expr(0)?;
                    self.expect(&Tok::VariableEnd)?;
                    nodes.push(Node::Expr(expr));
                }
                Tok::TagStart => {
                    if self.peek().is_some_and(&end) {
                        return Ok(nodes);
                    }
                    let node = self.tag()?;
                    self.expect(&Tok::TagEnd)?;
                    if let Some(node) = node {
                        nodes.push(node);
                    }
                }
                _ => return Err(self.here()),
            }
        }
        Ok(nodes)
    }

    /// The end tag's name (after its `{%`, which `until` stopped at); the
    /// caller reads the `%}`.
    fn end_tag(&mut self, name: &str) -> Result<(), ParseError> {
        self.expect(&Tok::Ident(name))
    }

    fn filters(&mut self) -> Result<Vec<(String, Kwargs)>, ParseError> {
        let mut filters = Vec::new();
        loop {
            let name = self.ident()?.to_owned();
            let kwargs = if self.is(&Tok::LeftParen) {
                self.kwargs()?
            } else {
                Vec::new()
            };
            filters.push((name, kwargs));
            if self.is(&Tok::Pipe) {
                self.pos += 1;
            } else {
                return Ok(filters);
            }
        }
    }

    fn tag(&mut self) -> Result<Option<Node>, ParseError> {
        let at = self.here();
        match self.next()? {
            Tok::Ident(word @ ("set" | "set_global")) => {
                let global = word == "set_global";
                let name = self.ident()?.to_owned();
                if self.is(&Tok::Assign) {
                    self.pos += 1;
                    let value = self.expr(0)?;
                    return Ok(Some(Node::Set {
                        name,
                        value,
                        global,
                    }));
                }
                let mut filters = Vec::new();
                while self.is(&Tok::Pipe) {
                    self.pos += 1;
                    let filter = self.ident()?.to_owned();
                    let kwargs = if self.is(&Tok::LeftParen) {
                        self.kwargs()?
                    } else {
                        Vec::new()
                    };
                    filters.push((filter, kwargs));
                }
                self.expect(&Tok::TagEnd)?;
                let body = self.until(|t| *t == Tok::Ident("endset"))?;
                self.end_tag("endset")?;
                Ok(Some(Node::BlockSet {
                    name,
                    filters,
                    body,
                    global,
                }))
            }
            Tok::Ident("include") => Ok(Some(Node::Include(self.string()?))),
            Tok::Ident("extends") => {
                self.output.parent = Some(self.string()?);
                Ok(None)
            }
            Tok::Ident("block") => {
                let name = self.ident()?.to_owned();
                self.expect(&Tok::TagEnd)?;
                let body = self.until(|t| *t == Tok::Ident("endblock"))?;
                self.end_tag("endblock")?;
                if matches!(self.peek(), Some(Tok::Ident(_))) {
                    self.pos += 1;
                }
                Ok(Some(Node::Block(name, body)))
            }
            Tok::Ident("for") => {
                let mut value = self.ident()?.to_owned();
                let mut key = None;
                if self.is(&Tok::Comma) {
                    self.pos += 1;
                    key = Some(value);
                    value = self.ident()?.to_owned();
                }
                self.expect(&Tok::Ident("in"))?;
                let target = self.expr(0)?;
                self.expect(&Tok::TagEnd)?;
                self.in_loop += 1;
                let body = self.until(|t| matches!(t, Tok::Ident("endfor" | "else")));
                self.in_loop -= 1;
                let body = body?;
                let mut else_body = Vec::new();
                if self.is(&Tok::Ident("else")) {
                    self.pos += 1;
                    self.expect(&Tok::TagEnd)?;
                    else_body = self.until(|t| *t == Tok::Ident("endfor"))?;
                }
                self.end_tag("endfor")?;
                Ok(Some(Node::For {
                    key,
                    value,
                    target,
                    body,
                    else_body,
                }))
            }
            Tok::Ident("if") => {
                let node = self.if_rest()?;
                self.end_tag("endif")?;
                Ok(Some(node))
            }
            Tok::Ident("filter") => {
                let filters = self.filters()?;
                self.expect(&Tok::TagEnd)?;
                let body = self.until(|t| *t == Tok::Ident("endfilter"))?;
                self.end_tag("endfilter")?;
                Ok(Some(Node::FilterSection(filters, body)))
            }
            Tok::Ident("component") => {
                let def = self.component_def()?;
                self.output.components.push(def);
                Ok(None)
            }
            Tok::Ident("break") => Ok(Some(Node::Break)),
            Tok::Ident("continue") => Ok(Some(Node::Continue)),
            Tok::LessThan => {
                let name = self.component_name()?;
                let attrs = self.component_attrs()?;
                self.expect(&Tok::GreaterThan)?;
                self.expect(&Tok::TagEnd)?;
                let body = self.until(|t| *t == Tok::ClosingTagStart)?;
                self.expect(&Tok::ClosingTagStart)?;
                if self.component_name()? != name {
                    return Err(self.here());
                }
                self.expect(&Tok::GreaterThan)?;
                Ok(Some(Node::Expr(Expr::Component { name, attrs, body })))
            }
            _ => Err(at),
        }
    }

    /// After `if`/`elif`: the condition, the body and what follows, up to
    /// (not including) the `endif`.
    fn if_rest(&mut self) -> Result<Node, ParseError> {
        let condition = self.expr(0)?;
        self.expect(&Tok::TagEnd)?;
        let body = self.until(|t| matches!(t, Tok::Ident("endif" | "else" | "elif")))?;
        let at = self.here();
        let otherwise = match self.peek() {
            Some(Tok::Ident("elif")) => {
                self.pos += 1;
                vec![self.if_rest()?]
            }
            Some(Tok::Ident("else")) => {
                self.pos += 1;
                self.expect(&Tok::TagEnd)?;
                self.until(|t| *t == Tok::Ident("endif"))?
            }
            Some(Tok::Ident("endif")) => Vec::new(),
            _ => return Err(at),
        };
        Ok(Node::If(condition, body, otherwise))
    }

    fn component_def(&mut self) -> Result<ComponentDef, ParseError> {
        let name = self.component_name()?;
        self.expect(&Tok::LeftParen)?;
        let mut params = Vec::new();
        let mut rest = None;
        loop {
            if self.is(&Tok::RightParen) {
                self.pos += 1;
                break;
            }
            if !params.is_empty() {
                self.expect(&Tok::Comma)?;
            }
            if self.is(&Tok::RightParen) {
                self.pos += 1;
                break;
            }
            if self.is(&Tok::Spread) {
                self.pos += 1;
                rest = Some(self.ident()?.to_owned());
                self.expect(&Tok::RightParen)?;
                break;
            }
            let implicit = self.is(&Tok::At);
            if implicit {
                self.pos += 1;
            }
            let param = self.ident()?.to_owned();
            if self.is(&Tok::Colon) {
                self.pos += 1;
                self.ident()?;
            }
            let default = if self.is(&Tok::Assign) {
                self.pos += 1;
                Some(self.literal()?)
            } else {
                None
            };
            params.push(Param {
                name: param,
                default,
                implicit,
            });
        }
        // Metadata: a literal map.
        if self.is(&Tok::LeftBrace) {
            self.pos += 1;
            self.map()?;
        }
        self.expect(&Tok::TagEnd)?;
        let body = self.until(|t| *t == Tok::Ident("endcomponent"))?;
        self.end_tag("endcomponent")?;
        // `{% endcomponent name %}`: the name may repeat (dotted).
        if matches!(self.peek(), Some(Tok::Ident(_))) {
            self.component_name()?;
        }
        Ok(ComponentDef {
            name,
            params,
            rest,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::lexer::tokenize;
    use super::*;

    fn parse_str(source: &str) -> Parsed {
        let tokens = tokenize(source).unwrap();
        parse(&tokens, source.len()).unwrap()
    }

    fn expr(source: &str) -> Expr {
        match parse_str(&format!("{{{{ {source} }}}}")).nodes.remove(0) {
            Node::Expr(expr) => expr,
            other => panic!("{other:?}"),
        }
    }

    fn var(name: &str) -> Box<Expr> {
        Box::new(Expr::Var(name.into()))
    }

    /// Precedence as Tera's: a filter binds tighter than `~` and `**`,
    /// looser than unary minus; `not in`; the ternary takes the rest.
    #[test]
    fn precedence_matches_tera() {
        assert_eq!(
            expr("a ~ b | upper"),
            Expr::Binary(
                Binary::StrConcat,
                var("a"),
                Box::new(Expr::Filter(var("b"), "upper".into(), vec![]))
            )
        );
        assert_eq!(
            expr("a not in b"),
            Expr::Unary(
                Unary::Not,
                Box::new(Expr::Binary(Binary::In, var("a"), var("b")))
            )
        );
        assert_eq!(
            expr("a if b else c ~ d"),
            Expr::Ternary(
                var("b"),
                var("a"),
                Box::new(Expr::Binary(Binary::StrConcat, var("c"), var("d")))
            )
        );
        assert_eq!(
            expr("a ** b ** c"),
            Expr::Binary(
                Binary::Power,
                var("a"),
                Box::new(Expr::Binary(Binary::Power, var("b"), var("c")))
            )
        );
    }

    #[test]
    fn statements_and_components() {
        let parsed = parse_str(
            "{% component ui.button(label, @theme, n=3, ...rest) %}<b>{{ body }}</b>{% endcomponent ui.button %}\
             {% for k, v in m %}{{ loop.index }}{% else %}none{% endfor %}\
             {% if a %}1{% elif b %}2{% else %}3{% endif %}\
             {% set x %}{{ 1 }}{% endset %}{% set_global y = [1, ...z] %}\
             {% filter upper %}t{% endfilter %}{% <ui.button label=\"x\"> %}in{% </ui.button> %}",
        );
        assert_eq!(parsed.components.len(), 1);
        assert_eq!(parsed.components[0].params.len(), 3);
        assert_eq!(parsed.components[0].rest.as_deref(), Some("rest"));
        assert_eq!(parsed.nodes.len(), 6);
        let Node::For { body, .. } = &parsed.nodes[0] else {
            panic!()
        };
        assert_eq!(
            body,
            &vec![Node::Expr(Expr::Var("__tera_loop_index".into()))]
        );
    }
}
