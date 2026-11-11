//! Tera 2.4's lexer, ported (`parsing/lexer.rs`): the same tokens from the
//! same text, so the analysis reads the template Tera compiled. Whitespace
//! control (`{%-`, `-%}`) only trims text, which the analysis may
//! over-count, so it's recognized and dropped. `{% raw %}` bodies are
//! text; comments are nothing.

use nom::Parser;
use nom::bytes::complete::{tag, take_while};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Tok<'a> {
    /// Template text (and `{% raw %}` bodies): only its length matters.
    Content(usize),
    VariableStart,
    VariableEnd,
    TagStart,
    TagEnd,
    Ident(&'a str),
    Str(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
    Mul,
    Div,
    FloorDiv,
    Mod,
    Plus,
    Minus,
    Power,
    LessThan,
    ClosingTagStart,
    GreaterThan,
    LessThanOrEqual,
    GreaterThanOrEqual,
    Equal,
    NotEqual,
    Tilde,
    Pipe,
    Assign,
    Dot,
    QuestionMarkDot,
    QuestionMarkLeftBracket,
    Comma,
    Colon,
    Bang,
    LeftBracket,
    RightBracket,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Spread,
    At,
}

/// A token and the byte offset it starts at (for error messages).
pub(super) type Spanned<'a> = (Tok<'a>, usize);

#[derive(Clone, Copy, PartialEq)]
enum State {
    Template,
    Variable,
    Tag,
}

/// `-? ws* name ws* -? %}` at the start of `text`: the length up to and
/// including `%}` (Tera's `skip_tag`).
fn skip_tag(text: &str, name: &str) -> Option<usize> {
    let mut rest = text.strip_prefix('-').unwrap_or(text);
    rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace());
    rest = rest.strip_prefix(name)?;
    rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace());
    rest = rest.strip_prefix('-').unwrap_or(rest);
    rest = rest.strip_prefix("%}")?;
    Some(text.len() - rest.len())
}

/// The next `{{`, `{%` or `{#` (Tera's `find_start_marker`).
fn find_start_marker(text: &str) -> Option<usize> {
    text.as_bytes()
        .windows(2)
        .position(|pair| pair == b"{{" || pair == b"{%" || pair == b"{#")
}

/// Tokenizes `source` as Tera does, or the byte offset of what it can't.
pub(super) fn tokenize(source: &str) -> Result<Vec<Spanned<'_>>, usize> {
    let mut tokens = Vec::new();
    let mut rest = source;
    let mut state = State::Template;
    let offset = |rest: &str| source.len() - rest.len();
    while !rest.is_empty() {
        let at = offset(rest);
        match state {
            State::Template => {
                // `{{`/`{%`/`{#`, then a `-` for whitespace control.
                fn after(rest: &str) -> &str {
                    if rest.as_bytes().get(2) == Some(&b'-') {
                        &rest[3..]
                    } else {
                        &rest[2..]
                    }
                }
                if rest.starts_with("{{") {
                    rest = after(rest);
                    state = State::Variable;
                    tokens.push((Tok::VariableStart, at));
                } else if rest.starts_with("{%") {
                    rest = after(rest);
                    if let Some(open) = skip_tag(rest, "raw") {
                        // The body runs to the first `{% endraw %}`.
                        let body = &rest[open..];
                        let mut search = 0;
                        let end = loop {
                            let Some(found) = body[search..].find("{%") else {
                                return Err(at);
                            };
                            let tag_start = search + found + 2;
                            if let Some(close) = skip_tag(&body[tag_start..], "endraw") {
                                break (search + found, tag_start + close);
                            }
                            search = tag_start;
                        };
                        tokens.push((Tok::Content(end.0), at));
                        rest = &body[end.1..];
                    } else {
                        state = State::Tag;
                        tokens.push((Tok::TagStart, at));
                    }
                } else if rest.starts_with("{#") {
                    let body = after(rest);
                    let Some(end) = body.find("#}") else {
                        return Err(at);
                    };
                    rest = &body[end + 2..];
                } else {
                    let len = find_start_marker(rest).unwrap_or(rest.len());
                    tokens.push((Tok::Content(len), at));
                    rest = &rest[len..];
                }
            }
            State::Variable | State::Tag => {
                let trimmed = rest.trim_start_matches(|c: char| c.is_ascii_whitespace());
                if trimmed.len() != rest.len() {
                    rest = trimmed;
                    continue;
                }
                let at = offset(rest);
                let end = if state == State::Tag { "%}" } else { "}}" };
                if let Some(after) = rest.strip_prefix('-').and_then(|r| r.strip_prefix(end)) {
                    tokens.push((closing(state), at));
                    state = State::Template;
                    rest = after;
                    continue;
                }
                if let Some(after) = rest.strip_prefix(end) {
                    tokens.push((closing(state), at));
                    state = State::Template;
                    rest = after;
                    continue;
                }
                let (after, token) = token(rest).map_err(|()| at)?;
                tokens.push((token, at));
                rest = after;
            }
        }
    }
    // An unclosed tag fails Tera's parser; the analysis never sees it, but
    // don't hand it a half tag either.
    if state != State::Template {
        return Err(source.len());
    }
    Ok(tokens)
}

fn closing(state: State) -> Tok<'static> {
    if state == State::Tag {
        Tok::TagEnd
    } else {
        Tok::VariableEnd
    }
}

/// One token inside a tag: operators longest first, then strings, numbers
/// and identifiers, in Tera's order.
fn token(rest: &str) -> Result<(&str, Tok<'_>), ()> {
    const TWO: [(&str, Tok<'static>); 9] = [
        ("//", Tok::FloorDiv),
        ("**", Tok::Power),
        ("==", Tok::Equal),
        ("!=", Tok::NotEqual),
        (">=", Tok::GreaterThanOrEqual),
        ("<=", Tok::LessThanOrEqual),
        ("</", Tok::ClosingTagStart),
        ("?.", Tok::QuestionMarkDot),
        ("?[", Tok::QuestionMarkLeftBracket),
    ];
    if let Ok((after, _)) = tag::<_, _, ()>("...").parse(rest) {
        return Ok((after, Tok::Spread));
    }
    for (text, token) in TWO {
        if let Some(after) = rest.strip_prefix(text) {
            return Ok((after, token));
        }
    }
    let first = rest.as_bytes()[0];
    let one = match first {
        b'+' => Some(Tok::Plus),
        b'-' => Some(Tok::Minus),
        b'*' => Some(Tok::Mul),
        b'/' => Some(Tok::Div),
        b'%' => Some(Tok::Mod),
        b'!' => Some(Tok::Bang),
        b'.' => Some(Tok::Dot),
        b',' => Some(Tok::Comma),
        b':' => Some(Tok::Colon),
        b'~' => Some(Tok::Tilde),
        b'|' => Some(Tok::Pipe),
        b'=' => Some(Tok::Assign),
        b'>' => Some(Tok::GreaterThan),
        b'<' => Some(Tok::LessThan),
        b'(' => Some(Tok::LeftParen),
        b')' => Some(Tok::RightParen),
        b'[' => Some(Tok::LeftBracket),
        b']' => Some(Tok::RightBracket),
        b'{' => Some(Tok::LeftBrace),
        b'}' => Some(Tok::RightBrace),
        b'@' => Some(Tok::At),
        _ => None,
    };
    if let Some(token) = one {
        return Ok((&rest[1..], token));
    }
    if matches!(first, b'\'' | b'"' | b'`') {
        return string(rest, first);
    }
    if first.is_ascii_digit() {
        return number(rest);
    }
    // `_` anywhere, a letter first, letters and digits after.
    if !(first == b'_' || first.is_ascii_alphabetic()) {
        return Err(());
    }
    let (after, ident) = take_while::<_, _, ()>(|c: char| c == '_' || c.is_ascii_alphanumeric())
        .parse(rest)
        .map_err(|_| ())?;
    match ident {
        "" => Err(()),
        "true" | "True" => Ok((after, Tok::Bool(true))),
        "false" | "False" => Ok((after, Tok::Bool(false))),
        _ => Ok((after, Tok::Ident(ident))),
    }
}

/// A quoted string with Tera's escapes (`\"`, `\'`, `\/`, `\\`, `\n`,
/// `\t`, `\r`; any other escape is an error).
fn string(rest: &str, quote: u8) -> Result<(&str, Tok<'_>), ()> {
    let bytes = rest.as_bytes();
    let mut i = 1;
    let mut escaped = false;
    while i < bytes.len() {
        if escaped {
            escaped = false;
        } else if bytes[i] == b'\\' {
            escaped = true;
        } else if bytes[i] == quote {
            break;
        }
        i += 1;
    }
    if bytes.get(i) != Some(&quote) {
        return Err(());
    }
    let body = &rest[1..i];
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(match chars.next().ok_or(())? {
                c @ ('"' | '\'' | '/' | '\\') => c,
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                _ => return Err(()),
            });
        } else {
            out.push(c);
        }
    }
    Ok((&rest[i + 1..], Tok::Str(out)))
}

/// Digits with at most one `.` (a float), as Tera reads them.
fn number(rest: &str) -> Result<(&str, Tok<'_>), ()> {
    let mut dot = false;
    let len = rest
        .bytes()
        .take_while(|&c| {
            if !dot && c == b'.' {
                dot = true;
                true
            } else {
                c.is_ascii_digit()
            }
        })
        .count();
    let (text, after) = rest.split_at(len);
    if dot {
        Ok((after, Tok::Float(text.parse().map_err(|_| ())?)))
    } else {
        Ok((after, Tok::Integer(text.parse().map_err(|_| ())?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<Tok<'_>> {
        tokenize(source)
            .unwrap()
            .into_iter()
            .map(|(t, _)| t)
            .collect()
    }

    #[test]
    fn tags_text_and_operators() {
        assert_eq!(
            kinds("a{{ x | f(n=1.5) }}b{%- if not y -%}c"),
            vec![
                Tok::Content(1),
                Tok::VariableStart,
                Tok::Ident("x"),
                Tok::Pipe,
                Tok::Ident("f"),
                Tok::LeftParen,
                Tok::Ident("n"),
                Tok::Assign,
                Tok::Float(1.5),
                Tok::RightParen,
                Tok::VariableEnd,
                Tok::Content(1),
                Tok::TagStart,
                Tok::Ident("if"),
                Tok::Ident("not"),
                Tok::Ident("y"),
                Tok::TagEnd,
                Tok::Content(1),
            ]
        );
        assert_eq!(
            kinds("{{ a ~ 'it\\'s' // 2 ** 3 }}"),
            vec![
                Tok::VariableStart,
                Tok::Ident("a"),
                Tok::Tilde,
                Tok::Str("it's".into()),
                Tok::FloorDiv,
                Tok::Integer(2),
                Tok::Power,
                Tok::Integer(3),
                Tok::VariableEnd,
            ]
        );
    }

    #[test]
    fn raw_bodies_and_comments() {
        assert_eq!(
            kinds("{% raw %}{{ not a tag }}{% endraw %}{# gone #}x"),
            vec![Tok::Content(15), Tok::Content(1)]
        );
        assert!(tokenize("{% raw %}never closed").is_err());
        assert!(tokenize("{{ 'unterminated }}").is_err());
        assert!(tokenize("{{ x").is_err());
    }
}
