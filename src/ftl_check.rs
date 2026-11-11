//! The load-time safety check of Fluent resources (ADR 0045, replacing
//! the fluent-syntax fork of bug_0005 and bug_0015).
//!
//! FTL is untrusted (ADR 0044), and fluent-syntax 0.11.1 has two defects
//! reachable from any resource (UPSTREAM U1, U2):
//!
//! - an invalid `\u`/`\U` escape cut off by a multi-byte character panics
//!   (the error quotes one *byte* of that character). The check refuses a
//!   `\u` or `\U` followed by fewer hex digits than it needs and then a
//!   non-ASCII byte, anywhere in the file: the only way to reach that
//!   slice, and a context-free scan can't miss it;
//! - expressions nest by recursion without a limit (placeables, select
//!   variants, call arguments), and a few KB overflow an asset thread's
//!   stack. The check runs a port of the parser's control flow
//!   (`parse_runtime` and everything it calls, each error at the same
//!   position, so entries start and end where fluent-syntax's do) that
//!   counts the active `{` and `(` levels, and refuses a resource past
//!   [`MAX_FTL_NESTING`].
//!
//! It runs once per resource load (and hot reload), before
//! `FluentResource::try_new`.

use std::fmt;

/// The deepest nesting of placeables and call arguments a resource may
/// hold. fluent-syntax recurses per level; a debug build overflowed a
/// 2 MB thread past ~200 (UPSTREAM U2), so 64 leaves room, and no
/// translation nests more than a few.
pub const MAX_FTL_NESTING: usize = 64;

/// Why a resource was refused (with its 1-based line).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FtlCheckError {
    TooDeep(usize),
    BadEscape(usize),
}

impl fmt::Display for FtlCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooDeep(line) => write!(
                f,
                "line {line}: placeables or function calls nest deeper than MAX_FTL_NESTING \
                 ({MAX_FTL_NESTING})"
            ),
            Self::BadEscape(line) => write!(
                f,
                "line {line}: an incomplete \\u or \\U escape before a non-ASCII character"
            ),
        }
    }
}

fn line_of(source: &str, offset: usize) -> usize {
    source.as_bytes()[..offset.min(source.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

/// Checks an FTL resource before fluent-syntax parses it.
pub(crate) fn check(source: &str) -> Result<(), FtlCheckError> {
    if let Some(at) = bad_escape(source.as_bytes()) {
        return Err(FtlCheckError::BadEscape(line_of(source, at)));
    }
    let mut parser = Parser::new(source.as_bytes());
    match parser.parse() {
        Ok(()) => Ok(()),
        Err(at) => Err(FtlCheckError::TooDeep(line_of(source, at))),
    }
}

/// A `\u` (4 hex digits) or `\U` (6) whose digits stop at a non-ASCII byte:
/// where fluent-syntax's `skip_unicode_escape_sequence` slices one byte
/// into a character (UPSTREAM U1). Anywhere in the file: text never runs
/// that code, so a hit there only refuses a file that's fine.
fn bad_escape(bytes: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'\\' && matches!(bytes[i + 1], b'u' | b'U') {
            let length = if bytes[i + 1] == b'u' { 4 } else { 6 };
            let digits = &bytes[i + 2..];
            let hex = digits
                .iter()
                .take(length)
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
            if hex < length && digits.get(hex).is_some_and(|b| !b.is_ascii()) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// How a parse step stopped: a syntax error (the entry becomes Junk, as in
/// fluent-syntax) or the nesting limit (the whole check stops).
#[derive(Debug)]
enum Stop {
    Error,
    TooDeep(usize),
}

type Step<T> = std::result::Result<T, Stop>;

/// What `get_inline_expression` produced: what its callers branch on.
#[derive(Clone, Copy, PartialEq)]
enum Inline {
    String,
    Number,
    Variable,
    Function,
    Placeable,
    Term { attribute: bool },
    Message { attribute: bool, id: (usize, usize) },
}

/// A parsed entry, for the differential test against fluent-syntax.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum Entry {
    Message(String),
    Term(String),
    Junk(usize, usize),
}

/// fluent-syntax 0.11.1's `Parser` (`parser/core.rs`, `pattern.rs`,
/// `expression.rs`, `helper.rs`, `comment.rs`, `runtime.rs`), ported:
/// the same pointer moves and the same errors, without the tree.
pub(crate) struct Parser<'a> {
    source: &'a [u8],
    ptr: usize,
    length: usize,
    depth: usize,
    pub(crate) entries: Vec<Entry>,
}

macro_rules! fail {
    () => {
        return Err(Stop::Error)
    };
}

impl<'a> Parser<'a> {
    pub(crate) fn new(source: &'a [u8]) -> Self {
        Parser {
            source,
            ptr: 0,
            length: source.len(),
            depth: 0,
            entries: Vec::new(),
        }
    }

    fn byte(&self, at: usize) -> Option<u8> {
        self.source.get(at).copied()
    }

    fn current(&self) -> Option<u8> {
        self.byte(self.ptr)
    }

    fn text(&self, start: usize, end: usize) -> String {
        String::from_utf8_lossy(&self.source[start.min(self.length)..end.min(self.length)])
            .into_owned()
    }

    /// Enters a nesting level (a placeable or call arguments).
    fn enter(&mut self) -> Step<()> {
        self.depth += 1;
        if self.depth > MAX_FTL_NESTING {
            return Err(Stop::TooDeep(self.ptr));
        }
        Ok(())
    }

    // ---- runtime.rs, core.rs -------------------------------------------

    /// `parse_runtime`; `Err` = the offset where the nesting limit was
    /// passed.
    pub(crate) fn parse(&mut self) -> std::result::Result<(), usize> {
        self.skip_blank_block();
        while self.ptr < self.length {
            let entry_start = self.ptr;
            match self.get_entry_runtime() {
                Ok(()) => {}
                Err(Stop::TooDeep(at)) => return Err(at),
                Err(Stop::Error) => {
                    self.depth = 0;
                    self.skip_to_next_entry_start();
                    self.entries.push(Entry::Junk(entry_start, self.ptr));
                }
            }
            self.skip_blank_block();
        }
        Ok(())
    }

    fn get_entry_runtime(&mut self) -> Step<()> {
        match self.current() {
            Some(b'#') => self.skip_comment(),
            Some(b'-') => {
                let id = self.get_term()?;
                self.entries.push(Entry::Term(id));
            }
            _ => {
                let id = self.get_message()?;
                self.entries.push(Entry::Message(id));
            }
        }
        Ok(())
    }

    fn get_message(&mut self) -> Step<String> {
        let (start, end) = self.get_identifier()?;
        self.skip_blank_inline();
        self.expect_byte(b'=')?;
        let pattern = self.get_pattern()?;
        self.skip_blank_block();
        let attributes = self.get_attributes()?;
        if !pattern && attributes == 0 {
            fail!();
        }
        Ok(self.text(start, end))
    }

    fn get_term(&mut self) -> Step<String> {
        self.expect_byte(b'-')?;
        let (start, end) = self.get_identifier()?;
        self.skip_blank_inline();
        self.expect_byte(b'=')?;
        self.skip_blank_inline();
        let value = self.get_pattern()?;
        self.skip_blank_block();
        self.get_attributes()?;
        if value {
            Ok(self.text(start, end))
        } else {
            fail!()
        }
    }

    /// The number of attributes. A failed attribute ends the list where it
    /// started (fluent-syntax's `get_attributes` is infallible), but the
    /// nesting limit still stops the check.
    fn get_attributes(&mut self) -> Step<usize> {
        let mut count = 0;
        loop {
            let line_start = self.ptr;
            self.skip_blank_inline();
            if !self.take_byte_if(b'.') {
                self.ptr = line_start;
                break;
            }
            let depth = self.depth;
            match self.get_attribute() {
                Ok(()) => count += 1,
                Err(Stop::Error) => {
                    self.depth = depth;
                    self.ptr = line_start;
                    break;
                }
                Err(too_deep) => return Err(too_deep),
            }
        }
        Ok(count)
    }

    fn get_attribute(&mut self) -> Step<()> {
        self.get_identifier()?;
        self.skip_blank_inline();
        self.expect_byte(b'=')?;
        if self.get_pattern()? { Ok(()) } else { fail!() }
    }

    /// The identifier after its first byte (`self.ptr - 1`): its span.
    fn get_identifier_unchecked(&mut self) -> (usize, usize) {
        let mut ptr = self.ptr;
        while matches!(self.byte(ptr), Some(b) if b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            ptr += 1;
        }
        let span = (self.ptr - 1, ptr);
        self.ptr = ptr;
        span
    }

    fn get_identifier(&mut self) -> Step<(usize, usize)> {
        if !self.is_identifier_start() {
            fail!();
        }
        self.ptr += 1;
        Ok(self.get_identifier_unchecked())
    }

    fn get_attribute_accessor(&mut self) -> Step<bool> {
        if self.take_byte_if(b'.') {
            self.get_identifier()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get_variant_key(&mut self) -> Step<()> {
        self.skip_blank();
        if self.is_number_start() {
            self.get_number_literal()?;
        } else {
            self.get_identifier()?;
        }
        self.skip_blank();
        self.expect_byte(b']')
    }

    fn get_variants(&mut self) -> Step<()> {
        let mut has_default = false;
        loop {
            let default = self.take_byte_if(b'*');
            if default {
                if has_default {
                    fail!();
                }
                has_default = true;
            }
            if !self.take_byte_if(b'[') {
                break;
            }
            self.get_variant_key()?;
            if self.get_pattern()? {
                self.skip_blank();
            } else {
                fail!();
            }
        }
        if has_default { Ok(()) } else { fail!() }
    }

    /// After a `{`: one nesting level.
    fn get_placeable(&mut self) -> Step<()> {
        self.enter()?;
        self.skip_blank();
        let expression = self.get_expression()?;
        self.skip_blank_inline();
        self.expect_byte(b'}')?;
        if matches!(expression, Some(Inline::Term { attribute: true })) {
            fail!();
        }
        self.depth -= 1;
        Ok(())
    }

    // ---- pattern.rs ----------------------------------------------------

    /// Whether the pattern has a value (`Some` in fluent-syntax).
    fn get_pattern(&mut self) -> Step<bool> {
        #[derive(PartialEq)]
        enum Role {
            InitialLineStart,
            LineStart,
            Continuation,
        }
        let mut has_value = false;
        self.skip_blank_inline();
        let mut role = if self.skip_eol() {
            self.skip_blank_block();
            Role::LineStart
        } else {
            Role::InitialLineStart
        };
        while self.ptr < self.length {
            if self.take_byte_if(b'{') {
                self.get_placeable()?;
                has_value = true;
                role = Role::Continuation;
            } else {
                let slice_start = self.ptr;
                let mut indent = 0;
                if role == Role::LineStart {
                    indent = self.skip_blank_inline();
                    match self.current() {
                        Some(b) if indent == 0 => {
                            if b != b'\r' && b != b'\n' {
                                break;
                            }
                        }
                        Some(b) => {
                            if matches!(b, b'.' | b'}' | b'[' | b'*') {
                                self.ptr = slice_start;
                                break;
                            }
                        }
                        None => break,
                    }
                }
                let (start, end, non_blank, termination) = self.get_text_slice()?;
                let _ = indent;
                if start != end && non_blank {
                    has_value = true;
                }
                role = match termination {
                    Termination::LineFeed | Termination::Crlf => Role::LineStart,
                    Termination::PlaceableStart | Termination::Eof => Role::Continuation,
                };
            }
        }
        Ok(has_value)
    }

    fn get_text_slice(&mut self) -> Step<(usize, usize, bool, Termination)> {
        let start = self.ptr;
        let mut non_blank = false;
        while let Some(b) = self.current() {
            match b {
                b' ' => self.ptr += 1,
                b'\n' => {
                    self.ptr += 1;
                    return Ok((start, self.ptr, non_blank, Termination::LineFeed));
                }
                b'\r' if self.byte(self.ptr + 1) == Some(b'\n') => {
                    self.ptr += 1;
                    return Ok((start, self.ptr - 1, non_blank, Termination::Crlf));
                }
                b'{' => return Ok((start, self.ptr, non_blank, Termination::PlaceableStart)),
                b'}' => fail!(),
                _ => {
                    non_blank = true;
                    self.ptr += 1;
                }
            }
        }
        Ok((start, self.ptr, non_blank, Termination::Eof))
    }

    // ---- expression.rs ---------------------------------------------------

    /// `None`: a select expression.
    fn get_expression(&mut self) -> Step<Option<Inline>> {
        let expression = self.get_inline_expression(false)?;
        self.skip_blank();
        if !self.is_current_byte(b'-') || self.byte(self.ptr + 1) != Some(b'>') {
            if matches!(expression, Inline::Term { attribute: true }) {
                fail!();
            }
            return Ok(Some(expression));
        }
        match expression {
            Inline::Message { .. } => fail!(),
            Inline::Term { attribute: false } => fail!(),
            Inline::Term { attribute: true }
            | Inline::String
            | Inline::Number
            | Inline::Variable
            | Inline::Function => {}
            Inline::Placeable => fail!(),
        }
        self.ptr += 2; // ->
        self.skip_blank_inline();
        if !self.skip_eol() {
            fail!();
        }
        self.skip_blank();
        self.get_variants()?;
        Ok(None)
    }

    fn get_inline_expression(&mut self, only_literal: bool) -> Step<Inline> {
        match self.current() {
            Some(b'"') => {
                self.ptr += 1;
                while let Some(b) = self.current() {
                    match b {
                        b'\\' => match self.byte(self.ptr + 1) {
                            Some(b'\\' | b'{' | b'"') => self.ptr += 2,
                            Some(b'u') => {
                                self.ptr += 2;
                                self.skip_unicode_escape_sequence(4)?;
                            }
                            Some(b'U') => {
                                self.ptr += 2;
                                self.skip_unicode_escape_sequence(6)?;
                            }
                            _ => fail!(),
                        },
                        b'"' => break,
                        b'\n' => fail!(),
                        _ => self.ptr += 1,
                    }
                }
                self.expect_byte(b'"')?;
                Ok(Inline::String)
            }
            Some(b) if b.is_ascii_digit() => {
                self.get_number_literal()?;
                Ok(Inline::Number)
            }
            Some(b'-') if !only_literal => {
                self.ptr += 1;
                if self.is_identifier_start() {
                    self.ptr += 1;
                    self.get_identifier_unchecked();
                    let attribute = self.get_attribute_accessor()?;
                    self.get_call_arguments()?;
                    Ok(Inline::Term { attribute })
                } else {
                    self.ptr -= 1;
                    self.get_number_literal()?;
                    Ok(Inline::Number)
                }
            }
            Some(b'$') if !only_literal => {
                self.ptr += 1;
                self.get_identifier()?;
                Ok(Inline::Variable)
            }
            Some(b) if b.is_ascii_alphabetic() => {
                self.ptr += 1;
                let id = self.get_identifier_unchecked();
                if self.get_call_arguments()? {
                    let callee = self.source[id.0..id.1].iter().all(|c| {
                        c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-'
                    });
                    if !callee {
                        fail!();
                    }
                    Ok(Inline::Function)
                } else {
                    let attribute = self.get_attribute_accessor()?;
                    Ok(Inline::Message { attribute, id })
                }
            }
            Some(b'{') if !only_literal => {
                self.ptr += 1;
                self.get_placeable()?;
                Ok(Inline::Placeable)
            }
            _ => fail!(),
        }
    }

    /// Whether there were arguments; one nesting level while inside them.
    fn get_call_arguments(&mut self) -> Step<bool> {
        self.skip_blank();
        if !self.take_byte_if(b'(') {
            return Ok(false);
        }
        self.enter()?;
        let mut names: Vec<&[u8]> = Vec::new();
        self.skip_blank();
        while self.ptr < self.length {
            if self.is_current_byte(b')') {
                break;
            }
            let expression = self.get_inline_expression(false)?;
            if let Inline::Message {
                attribute: false,
                id,
            } = expression
            {
                self.skip_blank();
                if self.is_current_byte(b':') {
                    let name = &self.source[id.0..id.1];
                    if names.contains(&name) {
                        fail!();
                    }
                    self.ptr += 1;
                    self.skip_blank();
                    self.get_inline_expression(true)?;
                    names.push(name);
                } else if !names.is_empty() {
                    fail!();
                }
            } else if !names.is_empty() {
                fail!();
            }
            self.skip_blank();
            self.take_byte_if(b',');
            self.skip_blank();
        }
        self.expect_byte(b')')?;
        self.depth -= 1;
        Ok(true)
    }

    // ---- helper.rs, comment.rs -------------------------------------------

    fn is_current_byte(&self, b: u8) -> bool {
        self.current() == Some(b)
    }

    fn skip_to_next_entry_start(&mut self) {
        while let Some(b) = self.current() {
            let new_line = self.ptr == 0 || self.byte(self.ptr - 1) == Some(b'\n');
            if new_line && (b.is_ascii_alphabetic() || b == b'-' || b == b'#') {
                break;
            }
            self.ptr += 1;
        }
    }

    fn skip_eol(&mut self) -> bool {
        match self.current() {
            Some(b'\n') => {
                self.ptr += 1;
                true
            }
            Some(b'\r') if self.byte(self.ptr + 1) == Some(b'\n') => {
                self.ptr += 2;
                true
            }
            _ => false,
        }
    }

    fn skip_unicode_escape_sequence(&mut self, length: usize) -> Step<()> {
        let start = self.ptr;
        for _ in 0..length {
            match self.current() {
                Some(b) if b.is_ascii_hexdigit() => self.ptr += 1,
                _ => break,
            }
        }
        if self.ptr - start != length {
            fail!();
        }
        Ok(())
    }

    fn is_identifier_start(&self) -> bool {
        matches!(self.current(), Some(b) if b.is_ascii_alphabetic())
    }

    fn take_byte_if(&mut self, b: u8) -> bool {
        if self.is_current_byte(b) {
            self.ptr += 1;
            true
        } else {
            false
        }
    }

    fn skip_blank_block(&mut self) -> usize {
        let mut count = 0;
        loop {
            let start = self.ptr;
            self.skip_blank_inline();
            if !self.skip_eol() {
                self.ptr = start;
                break;
            }
            count += 1;
        }
        count
    }

    fn skip_blank(&mut self) {
        loop {
            match self.current() {
                Some(b' ' | b'\n') => self.ptr += 1,
                Some(b'\r') if self.byte(self.ptr + 1) == Some(b'\n') => self.ptr += 2,
                _ => break,
            }
        }
    }

    fn skip_blank_inline(&mut self) -> usize {
        let start = self.ptr;
        while self.current() == Some(b' ') {
            self.ptr += 1;
        }
        self.ptr - start
    }

    fn expect_byte(&mut self, b: u8) -> Step<()> {
        if !self.is_current_byte(b) {
            fail!();
        }
        self.ptr += 1;
        Ok(())
    }

    fn is_number_start(&self) -> bool {
        matches!(self.current(), Some(b) if b.is_ascii_digit() || b == b'-')
    }

    fn is_eol(&self) -> bool {
        match self.current() {
            Some(b'\n') | None => true,
            Some(b'\r') => self.byte(self.ptr + 1) == Some(b'\n'),
            _ => false,
        }
    }

    fn skip_digits(&mut self) -> Step<()> {
        let start = self.ptr;
        while matches!(self.current(), Some(b) if b.is_ascii_digit()) {
            self.ptr += 1;
        }
        if start == self.ptr { fail!() } else { Ok(()) }
    }

    fn get_number_literal(&mut self) -> Step<()> {
        self.take_byte_if(b'-');
        self.skip_digits()?;
        if self.take_byte_if(b'.') {
            self.skip_digits()?;
        }
        Ok(())
    }

    fn skip_comment(&mut self) {
        loop {
            while self.ptr < self.length && !self.is_eol() {
                self.ptr += 1;
            }
            self.ptr += 1;
            if self.is_current_byte(b'#') {
                self.ptr += 1;
            } else {
                break;
            }
        }
    }
}

#[derive(PartialEq)]
enum Termination {
    LineFeed,
    Crlf,
    PlaceableStart,
    Eof,
}

#[cfg(test)]
mod tests;
