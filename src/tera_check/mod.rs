//! The load-time safety check of templates (ADR 0044, bug_0220): a static
//! bound on what a template can make Tera do, refused before it ever
//! renders.
//!
//! Templates are untrusted input. Tera 2.4 bounds a `range` (10⁵ items),
//! component recursion and its parser, but not the work or memory of a
//! render: nested loops that print nothing run for hours, and
//! `{% set_global s = s ~ s %}` forty times asks for a terabyte, which
//! aborts the process. Tera's parser is private, so this module carries
//! its own (a port of Tera's lexer and parser) and runs an abstract render
//! over bounds (ADR 0044). It runs once per load (and hot reload),
//! after Tera has compiled the template set, over the entry template with
//! its includes, `extends` parents and components. A template is refused
//! when the check can't read it, or when its bounds pass
//! [`MAX_TEMPLATE_STEPS`], [`MAX_TEMPLATE_VALUE_BYTES`] or
//! [`MAX_TEMPLATE_APP_COPIES`].
//!
//! The bounds count the template's own work per piece of the app's data:
//! a loop over a `TemplateContext` collection counts one iteration, since
//! the app decides how much data it sends (a template looping 10⁵ times
//! inside it is refused for its own 10⁵).

use std::collections::HashMap;
use std::fmt;

mod analysis;
mod lexer;
mod parser;

/// The most a template may do per piece of app data, in steps (roughly one
/// Tera instruction each: 2²⁵ took 0.45 s of release-build rendering).
pub const MAX_TEMPLATE_STEPS: u64 = 1 << 25;

/// The largest value a template may build (a string, a list, a captured
/// `{% set %}` block, a component's output), in bytes as written: 16 MiB.
pub const MAX_TEMPLATE_VALUE_BYTES: u64 = 16 << 20;

/// The most copies of one app value a template's value may hold: past it
/// a small context value would make a large one (`name ~ name ~ …`).
pub const MAX_TEMPLATE_APP_COPIES: u64 = 1 << 12;

/// Why a template was refused.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum CheckError {
    /// The check couldn't read the template at this byte offset.
    Unreadable(String, usize),
    TooManySteps,
    ValueTooLarge,
    TooManyCopies,
    /// A value grows with the square of the app's data.
    AppSquared,
    /// A `set_global` variable in a loop grows faster than linearly.
    Superlinear(String),
    Recursive(String),
    /// A loop over context data inside another, not over its element.
    NestedAppLoops,
    /// A template, component, filter or function the check doesn't know.
    Unknown(String),
    TooComplex,
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable(name, line) => write!(
                f,
                "{name}: the template safety check couldn't read line {line}"
            ),
            Self::TooManySteps => write!(
                f,
                "the template could run more than MAX_TEMPLATE_STEPS ({MAX_TEMPLATE_STEPS}) \
                 steps (nested loops?)"
            ),
            Self::ValueTooLarge => write!(
                f,
                "the template could build a value larger than MAX_TEMPLATE_VALUE_BYTES \
                 ({MAX_TEMPLATE_VALUE_BYTES} bytes)"
            ),
            Self::TooManyCopies => write!(
                f,
                "the template could copy one context value more than MAX_TEMPLATE_APP_COPIES \
                 ({MAX_TEMPLATE_APP_COPIES}) times into a value"
            ),
            Self::AppSquared => write!(
                f,
                "the template could build a value that grows with the square of the context's \
                 data (a filter combining context values)"
            ),
            Self::Superlinear(name) => write!(
                f,
                "`{name}`, set with `set_global` in a loop, could grow faster than linearly \
                 with the loop (it depends on itself more than once, or through a filter)"
            ),
            Self::NestedAppLoops => write!(
                f,
                "a loop over context data runs inside another loop over context data: the \
                 work would grow with their product (loop over the outer loop's own data \
                 instead, as in `for card in group.cards`)"
            ),
            Self::Recursive(name) => {
                write!(f, "`{name}` includes, extends or calls itself")
            }
            Self::Unknown(what) => write!(f, "the template safety check doesn't know {what}"),
            Self::TooComplex => write!(f, "the template is too complex for the safety check"),
        }
    }
}

/// The 1-based line of byte `offset` in `source`.
fn line_of(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        + 1
}

/// Checks `entry` of a compiled template set (`(name, source)` pairs, as
/// Tera got them).
pub(crate) fn check(
    entry: &str,
    sources: &[(String, String)],
) -> Result<analysis::Report, CheckError> {
    let mut parsed = HashMap::new();
    for (name, source) in sources {
        let unreadable = |offset| CheckError::Unreadable(name.clone(), line_of(source, offset));
        let tokens = lexer::tokenize(source).map_err(unreadable)?;
        let tree = parser::parse(&tokens, source.len()).map_err(unreadable)?;
        parsed.insert(name.clone(), tree);
    }
    analysis::Analyzer::new(&parsed).run(entry)
}

#[cfg(test)]
mod tests;
