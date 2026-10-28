//! `.html` files as Tera templates, and the parsed result.
//!
//! The loader compiles each file as a Tera 2 template, so syntax errors fail the
//! load. The template's name is its asset path, so `.html` files get Tera's
//! HTML autoescaping of `{{ }}` values. A plain `.html` file with no Tera
//! syntax is a template that renders to itself.
//!
//! The crate registers a `json` filter ([`new_tera`]): argument attributes
//! (`data-l10n-args`, `data-with`, `data-tooltip-args`) print their value
//! with it, `'{{ {"name": player} | json }}'`.
//!
//! Templates compose. `{% extends "base.html" %}` and `{% include
//! "part.html" %}` name other templates by path relative to the template's
//! own file (like a URL: `../lib/ui.html`, `/ui/lib.html` from the asset
//! root; `embedded://` templates resolve within their source). The loader
//! loads them with it — with Bevy's file watcher, editing one reloads every
//! template using it (a manual `AssetServer::reload` must name the using
//! template: Bevy reloads only the given path) — and
//! renders them as one Tera set, so `{% component %}`s defined in any of
//! them (Tera 2's macros) can be called from all: include a component
//! library (it renders nothing) and use `{{ <ui.button label="Go" /> }}`.
//! A template that extends another may only hold blocks at its top level,
//! so include libraries inside a block there.

use std::fmt::Write;

use bevy::asset::{AssetLoader, AssetPath, LoadContext, io::Reader};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::l10n::LocalizedText;
use crate::md;

/// A compiled Tera template loaded from an `.html`/`.htm` file — or a
/// Markdown `.md`/`.markdown` file ([ADR 0014]), whose Markdown converts
/// to HTML once, at asset load (Tera syntax passes through; ADR 0020).
/// Use it via [`HtmlUi`](crate::html::HtmlUi).
///
/// [ADR 0014]: docs/agents/adr/0014-markdown-via-pulldown-cmark.md
#[derive(Asset, TypePath)]
pub struct HtmlTemplate {
    pub(crate) tera: tera::Tera,
    /// Template name inside `tera`: the asset path.
    pub(crate) name: String,
}

impl HtmlTemplate {
    /// Renders with `context`, then parses the output into a DOM.
    pub fn render(
        &self,
        context: &tera::Context,
    ) -> Result<HtmlDocument, Box<dyn std::error::Error + Send + Sync>> {
        let source = self.render_source(context)?;
        Ok(HtmlDocument::parse(source)?)
    }

    /// Renders with `context` to HTML text, without parsing it.
    pub fn render_source(&self, context: &tera::Context) -> tera::TeraResult<String> {
        self.tera.render(&self.name, context)
    }

    /// The template's name (its asset path).
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Parsed HTML. Owns its source text.
pub struct HtmlDocument {
    /// Boxed: a `VDomGuard` is ~200 bytes, and `RenderedHtml` holds this.
    dom: Box<tl::VDomGuard>,
    /// The HTML text `dom` was parsed from (`dom` borrows its own leaked copy).
    source: String,
}

impl HtmlDocument {
    pub fn parse(html: String) -> Result<Self, tl::ParseError> {
        let source = html.clone();
        // SAFETY: `parse_owned` leaks `html` and frees it when the returned
        // `VDomGuard` drops; `VDomGuard::get_ref` ties borrows to the guard.
        let dom = unsafe { tl::parse_owned(html, tl::ParserOptions::default()) }?;
        Ok(Self {
            dom: Box::new(dom),
            source,
        })
    }

    /// The HTML text this document was parsed from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The parsed DOM.
    pub fn dom(&self) -> &tl::VDom<'_> {
        self.dom.get_ref()
    }

    /// Indented outline of the DOM: one line per element (with attributes),
    /// text node, and comment. Whitespace-only text is skipped. Elements in
    /// `localized` show their translation (`l10n "…"`) instead of children.
    pub fn outline(&self, localized: &LocalizedText) -> String {
        let dom = self.dom();
        let parser = dom.parser();
        let mut out = String::new();
        for handle in dom.children() {
            write_node(&mut out, parser, &localized.0, *handle);
        }
        out
    }
}

/// `tl` leaves character references as written. Decode the named (WHATWG's
/// table, semicolon forms — ADR 0027) and numeric ones, so text and
/// attribute values read as authored. One level: a decoded `&` is not
/// re-scanned (`&amp;lt;` reads back `&lt;`); an unknown reference is kept
/// verbatim, as in a browser.
#[path = "entity_data.rs"]
mod entity_data;

pub(crate) fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        if !text[at..].starts_with('&') {
            let character = text[at..].chars().next().expect("at a char boundary");
            out.push(character);
            at += character.len_utf8();
            continue;
        }
        // Numeric: decimal or hex, terminated by `;`. Invalid code points
        // (0, surrogates, past the top) read U+FFFD, and the C1 controls
        // read their Windows-1252 replacements (HTML §13.4's table).
        let numeric = text[at..]
            .strip_prefix("&#x")
            .or_else(|| text[at..].strip_prefix("&#X"))
            .and_then(|rest| terminated_digits(rest, 16, 1, 8, 3))
            .or_else(|| {
                text[at..]
                    .strip_prefix("&#")
                    .and_then(|rest| terminated_digits(rest, 10, 1, 10, 2))
            });
        if let Some((consumed, code)) = numeric {
            let code = match code {
                0x00 => 0xFFFD,
                0x80..=0x9F => u32::from(C1_REPLACEMENTS[(code - 0x80) as usize]),
                code => code,
            };
            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            at += consumed;
            continue;
        }
        // Named: the longest `&name;` in the table wins.
        let limit = text.len().min(at + entity_data::MAX_REFERENCE);
        let mut decoded = false;
        for end in (at + 3..=limit).rev() {
            // Byte-level: `end - 1` may land inside a multi-byte char.
            if text.as_bytes()[end - 1] != b';' {
                continue;
            }
            let name = &text[at + 1..end];
            match entity_data::ENTITIES.binary_search_by(|(known, _)| (*known).cmp(name)) {
                Ok(index) => {
                    for code in entity_data::ENTITIES[index].1 {
                        out.push(char::from_u32(*code).expect("a table code point"));
                    }
                    at = end;
                    decoded = true;
                    break;
                }
                // Keep going: a shorter name may still match (the
                // longest-match rule).
                Err(_) => continue,
            }
        }
        if !decoded {
            out.push('&');
            at += 1;
        }
    }
    out
}

/// The digits up to the reference's `;`: `(consumed including the `;`,
/// value)`, or `None` without the terminator, past the digit bound, or
/// non-parsing (the reference stays verbatim).
fn terminated_digits(
    rest: &str,
    radix: u32,
    min: usize,
    max: usize,
    prefix: usize,
) -> Option<(usize, u32)> {
    // Only the digit run, at most `max` long (review_0005 I04: finding
    // the `;` first scanned the rest of the text per `&#`, O(n²) on a
    // text of unterminated references). Digits only: `from_str_radix`
    // would accept a sign (`&#+5;` isn't a reference).
    let length = rest
        .bytes()
        .take(max + 1)
        .take_while(|byte| char::from(*byte).is_digit(radix))
        .count();
    let digits = &rest[..length];
    (length >= min && length <= max && rest[length..].starts_with(';')).then_some(())?;
    // The consumed span includes the terminator and the `&#`/`&#x`
    // prefix (the caller's `at` still sits on the `&`).
    Some((
        digits.len() + 1 + prefix,
        u32::from_str_radix(digits, radix).ok()?,
    ))
}

/// The C1 controls' Windows-1252 replacements, indexed from 0x80.
#[rustfmt::skip]
static C1_REPLACEMENTS: &[u16] = &[
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021,
    0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008D, 0x017D, 0x008F,
    0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x009D, 0x017E, 0x0178,
];
/// A Tera environment as the template loader builds it (public so tools
/// can compile templates exactly as the loader does): autoescaping for
/// the markup extensions and the crate's filters — `json`, which prints a
/// value as JSON for the argument attributes (`data-l10n-args`,
/// `data-with`, `data-tooltip-args`): `'{{ {"name": player} | json }}'`.
/// Tera prints a value without it in Rust's `Debug` style, which isn't
/// JSON once a string holds an escape (`\u{200d}` in an emoji sequence,
/// bug_0128).
pub fn new_tera() -> tera::Tera {
    let mut tera = tera::Tera::new();
    tera.autoescape_on([".html", ".htm", ".xml", ".md", ".markdown"]);
    tera.register_filter("json", json_filter);
    tera
}

/// The `json` filter: the value as compact JSON.
fn json_filter(value: tera::Value, _: tera::Kwargs, _: &tera::State) -> tera::TeraResult<String> {
    serde_json::to_string(&value).map_err(|err| tera::Error::message(format!("json: {err}")))
}

/// Parses an argument attribute's JSON (`data-l10n-args`, `data-with`,
/// `data-tooltip-args`), as printed by the `json` filter.
pub(crate) fn parse_attribute_json(text: &str) -> serde_json::Result<serde_json::Value> {
    serde_json::from_str(text)
}

/// `err: source: source…` — Tera puts the useful detail in the sources.
pub(crate) fn error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(err) = source {
        let _ = write!(message, ": {err}");
        source = err.source();
    }
    message
}

/// The DOM outline (depth-first, document order), iterative: the
/// document's depth costs heap, not stack.
fn write_node(
    out: &mut String,
    parser: &tl::Parser,
    localized: &HashMap<tl::NodeHandle, Result<String, String>>,
    handle: tl::NodeHandle,
) {
    // The indent is carried per stack entry (a child's is its parent's
    // plus two spaces): "  ".repeat(depth) per node recomputed the
    // prefix over and over.
    let mut stack = vec![(handle, String::new())];
    while let Some((handle, indent)) = stack.pop() {
        let Some(node) = handle.get(parser) else {
            continue;
        };
        match node {
            tl::Node::Tag(tag) => {
                let _ = write!(out, "{indent}{}", tag.name().as_utf8_str());
                for (key, value) in tag.attributes().iter() {
                    match value {
                        Some(value) => {
                            let _ = write!(out, " {key}=\"{value}\"");
                        }
                        None => {
                            let _ = write!(out, " {key}");
                        }
                    }
                }
                out.push('\n');
                let child_indent = format!("{indent}  ");
                match localized.get(&handle) {
                    Some(Ok(text)) => {
                        let _ = writeln!(out, "{child_indent}l10n {text:?}");
                    }
                    Some(Err(err)) => {
                        let _ = writeln!(out, "{child_indent}l10n error: {err}");
                    }
                    None => {
                        // Reversed: the leftmost child pops first.
                        let children: Vec<tl::NodeHandle> =
                            tag.children().top().iter().copied().collect();
                        for child in children.into_iter().rev() {
                            stack.push((child, child_indent.clone()));
                        }
                    }
                }
            }
            tl::Node::Raw(text) => {
                let text = text.as_utf8_str();
                let text = text.trim();
                if !text.is_empty() {
                    let _ = writeln!(out, "{indent}{text:?}");
                }
            }
            tl::Node::Comment(text) => {
                let _ = writeln!(out, "{indent}{}", text.as_utf8_str());
            }
        }
    }
}

/// Whether `path` names one of Bevy's network asset sources (`http://`,
/// `https://`, with Bevy's `http`/`https` features): content must not
/// fetch remote files — a template include, a Fluent resource, a
/// stylesheet or 9-slice image from a mod or a user would reach the
/// network (review_0003 G19). `<img>` already refused any `://`; other
/// sources (`embedded://`, an app's own named sources) stay usable.
pub(crate) fn is_remote(path: &AssetPath<'_>) -> bool {
    match path.source() {
        bevy::asset::io::AssetSourceId::Name(name) => {
            name.eq_ignore_ascii_case("http") || name.eq_ignore_ascii_case("https")
        }
        bevy::asset::io::AssetSourceId::Default => false,
    }
}

/// The keywords of the tags naming another template.
const REFERENCE_TAGS: [&str; 2] = ["include", "extends"];

/// The index just past the `{% endraw %}` group closing the raw block
/// whose opener ends at `after`; `None` when there is none.
fn tera_raw_end(source: &str, after: usize) -> Option<usize> {
    // Plain scan: raw content is verbatim (no strings to respect — a
    // quote inside used to swallow the endraw and drop every later
    // include, review_0003 G13).
    let mut cursor = after;
    while let Some(next) = source[cursor..].find("{%") {
        let at = cursor + next;
        let end = source[at..]
            .find("%}")
            .map_or(source.len(), |offset| at + offset + 2);
        let body = source[at + 2..end - 2]
            .trim()
            .trim_matches(['-', '+'])
            .trim();
        if body == "endraw" {
            return Some(end);
        }
        cursor = end;
    }
    None
}

/// The index just past the closer (`%}` for a tag, `}}` for an
/// expression) of the Tera span at `source` (which starts with its
/// two-byte opener); quoted strings inside are skipped.
fn tera_span_end(source: &str, closer: [u8; 2]) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut cursor = 2;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' | b'\'' => {
                let quote = bytes[cursor];
                cursor += 1;
                while cursor < bytes.len() && bytes[cursor] != quote {
                    cursor += if bytes[cursor] == b'\\' { 2 } else { 1 };
                }
                cursor += 1;
            }
            first
                if first == closer[0]
                    && cursor + 1 < bytes.len()
                    && bytes[cursor + 1] == closer[1] =>
            {
                return Some(cursor + 2);
            }
            _ => cursor += 1,
        }
    }
    None
}

/// Finds the templates `source` (the template at `path`) includes or
/// extends — `{% include "x.html" %}`, `{% extends "base.html" %}`, with
/// either quote — resolves each relative to `path`'s directory (like a
/// URL; `/x` is from the asset root, `embedded://…` keeps its source),
/// and rewrites the reference to the resolved path, which is the name
/// it's registered under. Returns the rewritten source and the resolved
/// paths. In a Markdown template, code (as its conversion reads it:
/// [`md::template_code_ranges`]) is literal text, so a reference there
/// is none.
#[hotpath::measure]
pub(crate) fn resolve_references(
    path: &AssetPath<'static>,
    source: &str,
    allow_unapproved: bool,
) -> Result<(String, Vec<AssetPath<'static>>), BevyError> {
    let mut out = String::with_capacity(source.len());
    let mut references = Vec::new();
    let mut rest = source;
    // Markdown code (code spans, fenced and indented blocks) protects its
    // content: a code sample showing `{% include %}` syntax isn't a
    // reference. pulldown decides what is code (review_0003 G13/G17,
    // review_0004 H04 — the hand-written fence, indent and backtick rules
    // disagreed with it both ways).
    let code = if md::is_markdown(&path.to_string()) {
        md::template_code_ranges(source)
    } else {
        Vec::new()
    };
    let mut code_index = 0;
    // The three stop positions, cached in absolute coordinates: `rest` is
    // always a suffix of `source`, and re-finding a needle that was
    // already located (or one that never occurs) is what made this loop
    // quadratic — 40k `{{ x }}` lines scanned 320 KB twenty times over
    // (review_0003 G14). A cached position is valid until the cursor
    // passes it; a `None` stays `None` (the needle is absent from every
    // suffix too).
    let mut tag_at = rest.find("{%");
    let mut expression_at = rest.find("{{");
    let mut comment_at = rest.find("{#");
    while !rest.is_empty() {
        let base = source.len() - rest.len();
        let refresh = |cached: &mut Option<usize>, needle: &str| {
            if cached.is_some_and(|at| at < base) {
                *cached = rest.find(needle).map(|at| base + at);
            }
        };
        refresh(&mut tag_at, "{%");
        refresh(&mut expression_at, "{{");
        refresh(&mut comment_at, "{#");
        let tag_start = tag_at.map(|at| at - base);
        let expression_start = expression_at.map(|at| at - base);
        let comment_start = comment_at.map(|at| at - base);
        // The earliest of: a comment (its examples are commented out, not
        // references), or a Tera tag/expression — copied as a unit (its
        // quoted strings can contain `{#`, `{%` or `{{`), with a
        // reference tag resolved and a raw block verbatim.
        let start = [tag_start, expression_start, comment_start]
            .into_iter()
            .flatten()
            .min();
        let Some(start) = start else {
            out.push_str(rest);
            return Ok((out, references));
        };
        // A stop inside code: copied verbatim through the code's end.
        while code
            .get(code_index)
            .is_some_and(|range| range.end <= base + start)
        {
            code_index += 1;
        }
        if let Some(range) = code
            .get(code_index)
            .filter(|range| range.start <= base + start)
        {
            let end = range.end - base;
            out.push_str(&rest[..end]);
            rest = &rest[end..];
            continue;
        }
        if Some(start) == comment_start {
            // A `{# … #}` comment: copied verbatim, its contents skipped.
            let Some(end) = rest[start..].find("#}") else {
                out.push_str(rest);
                return Ok((out, references));
            };
            let end = start + end + 2;
            out.push_str(&rest[..end]);
            rest = &rest[end..];
            continue;
        }
        // A Tera tag or expression; the text before it is copied as is.
        out.push_str(&rest[..start]);
        if Some(start) == expression_start {
            // A `{{ … }}` expression: copied verbatim (its strings can
            // contain anything).
            let Some(span_len) = tera_span_end(&rest[start..], *b"}}") else {
                out.push_str(&rest[start..start + 2]);
                rest = &rest[start + 2..];
                continue;
            };
            out.push_str(&rest[start..start + span_len]);
            rest = &rest[start + span_len..];
            continue;
        }
        let Some(tag_len) = tera_span_end(&rest[start..], *b"%}") else {
            // A lone `{%` without its closer: text; keep scanning after it.
            out.push_str(&rest[start..start + 2]);
            rest = &rest[start + 2..];
            continue;
        };
        let tag = &rest[start..start + tag_len];
        let body = tag[2..tag.len() - 2]
            .trim()
            .trim_start_matches(['-', '+'])
            .trim_start();
        let keyword = REFERENCE_TAGS.into_iter().find(|keyword| {
            body.strip_prefix(keyword)
                .is_some_and(|after| after.starts_with(char::is_whitespace))
        });
        let literal = keyword.and_then(|keyword| {
            let after = body[keyword.len()..].trim_start();
            let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let end = after[1..].find(quote)?;
            Some((after, &after[1..1 + end], quote))
        });
        let Some((after, literal, quote)) = literal else {
            let raw_word = body.split_whitespace().next().unwrap_or("");
            let keyword_word = raw_word.trim_matches(['-', '+']);
            if keyword_word == "raw" {
                // `{% raw %} … {% endraw %}`: verbatim through the endraw
                // group (whitespace-control variants included).
                let Some(end) = tera_raw_end(rest, start + tag_len) else {
                    out.push_str(&rest[start..]);
                    return Ok((out, references));
                };
                out.push_str(&rest[start..end]);
                rest = &rest[end..];
            } else {
                out.push_str(tag);
                rest = &rest[start + tag_len..];
            }
            continue;
        };
        let resolved = path.resolve_embed_str(literal)?;
        if is_remote(&resolved) {
            return Err(format!("template include names a remote source: {literal}").into());
        }
        // `read_asset_bytes` skips Bevy's own unapproved-path check
        // (`../..` out of the asset root would load): enforce the app's
        // `AssetPlugin::unapproved_path_mode` here (bug_0087). `Deny` and
        // `Forbid` refuse alike — an include can't ask for Bevy's
        // `override_unapproved`; only `Allow` reads outside the root.
        if resolved.is_unapproved() && !allow_unapproved {
            return Err(format!("template include escapes the asset root: {literal}").into());
        }
        // The resolved path replaces the literal; everything else in the
        // tag (opening, closing quote, whitespace control, `%}`) keeps
        // its original bytes.
        let quote_offset = after.as_ptr() as usize - tag.as_ptr() as usize;
        out.push_str(&tag[..quote_offset]);
        out.push(quote);
        out.push_str(&resolved.to_string());
        out.push_str(&tag[quote_offset + 1 + literal.len()..]);
        rest = &rest[start + tag_len..];
        references.push(resolved);
    }
    out.push_str(rest);
    Ok((out, references))
}

/// `allow_unapproved`: the app's `AssetPlugin::unapproved_path_mode` is
/// `Allow` (read by `BevyMarkupPlugin::build`; bug_0087).
#[derive(Default, TypePath)]
pub(crate) struct HtmlTemplateLoader {
    pub(crate) allow_unapproved: bool,
}

impl AssetLoader for HtmlTemplateLoader {
    type Asset = HtmlTemplate;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<HtmlTemplate, BevyError> {
        load_template(reader, self.allow_unapproved, load_context).await
    }

    fn extensions(&self) -> &[&str] {
        &["html", "htm"]
    }
}

/// Loads Markdown templates: the same Tera machinery (includes, extends,
/// one Tera set per template); each file's Markdown converts to HTML at
/// load ([`md::to_html_template`](crate::md::to_html_template)), so
/// [`HtmlTemplate::render`] is Tera-only.
#[derive(Default, TypePath)]
pub(crate) struct MarkdownTemplateLoader {
    pub(crate) allow_unapproved: bool,
}

impl AssetLoader for MarkdownTemplateLoader {
    type Asset = HtmlTemplate;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<HtmlTemplate, BevyError> {
        load_template(reader, self.allow_unapproved, load_context).await
    }

    fn extensions(&self) -> &[&str] {
        &["md", "markdown"]
    }
}

/// The shared loader body: the template and everything it includes, extends
/// or imports, by resolved asset path — one Tera instance renders them
/// together. Reading them through the load context makes them dependencies:
/// editing one reloads every template that uses it. Autoescaping is forced
/// on Markdown too (Tera's default list is `.html`/`.htm`/`.xml` only):
/// without it, `{{ value }}` in a `.md` template would inject raw text into
/// the HTML.
#[hotpath::measure]
async fn load_template(
    reader: &mut dyn Reader,
    allow_unapproved: bool,
    load_context: &mut LoadContext<'_>,
) -> Result<HtmlTemplate, BevyError> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await?;
    let source = String::from_utf8(bytes)?;

    let root = load_context.path().clone_owned();
    let name = root.to_string();
    let mut templates: Vec<(String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pending = vec![(root, source)];
    while let Some((path, source)) = pending.pop() {
        if !seen.insert(path.to_string()) {
            continue;
        }
        let (source, references) = resolve_references(&path, &source, allow_unapproved)?;
        // ADR 0020: a `.md` template's Markdown converts once, at load —
        // Tera syntax survives verbatim (`to_html_template`), so the
        // per-frame path is Tera-only. Per file: an `.html` partial
        // included from a `.md` page stays HTML, and a `.md` partial
        // included from an `.html` page converts.
        let source = if md::is_markdown(&path.to_string()) {
            md::to_html_template(&source)
        } else {
            source
        };
        for reference in references {
            if !seen.contains(&reference.to_string()) {
                let bytes = load_context.read_asset_bytes(reference.clone()).await?;
                pending.push((reference, String::from_utf8(bytes)?));
            }
        }
        templates.push((path.to_string(), source));
    }
    let mut tera = new_tera();
    tera.add_raw_templates(templates)?;
    Ok(HtmlTemplate { tera, name })
}

/// Includes naming a network source fail the load (review_0003 G19:
/// with Bevy's `http`/`https` features a template reached the network);
/// other sources stay usable.
#[test]
fn remote_includes_are_refused() {
    let path = AssetPath::from("page.html");
    for remote in ["https://evil.example/x.html", "HTTP://evil.example/x.html"] {
        let source = format!(r#"{{% include "{remote}" %}}"#);
        assert!(
            resolve_references(&path, &source, false).is_err(),
            "{remote}"
        );
    }
    let (_, references) = resolve_references(
        &path,
        r#"{% include "embedded://crate/part.html" %}"#,
        false,
    )
    .unwrap();
    assert_eq!(references[0].to_string(), "embedded://crate/part.html");
}

/// The `json` filter prints JSON that survives autoescaping and the
/// attribute's entity decoding unchanged, whatever the string holds
/// (bug_0128: emoji sequences, combining accents, control characters).
/// Without the filter Tera prints Rust `Debug` escapes, which aren't JSON:
/// the attribute fails to parse (the removed `\u{…}` rewrite accepted
/// them; templates add `| json` instead).
#[test]
fn json_filter_round_trips_any_string() {
    let render = |template: &str, value: &str| {
        let mut tera = new_tera();
        tera.add_raw_template("t.html", template).unwrap();
        let context = tera::Context::from_serialize(&serde_json::json!({ "s": value })).unwrap();
        decode_entities(&tera.render("t.html", &context).unwrap())
    };
    for value in [
        "Ada \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
        "Jose\u{301}",
        "bell\u{7} nul\u{0}",
        "quotes \" ' and <tags> & \\u{41}",
        "",
    ] {
        let json = render(r#"{{ {"v": s, "n": 2} | json }}"#, value);
        let parsed = parse_attribute_json(&json).unwrap_or_else(|err| panic!("{json:?}: {err}"));
        assert_eq!(parsed["v"], value, "{json:?}");
        assert_eq!(parsed["n"], 2, "numbers stay numbers (plural selectors)");
    }
    let debug = render(r#"{{ {"v": s} }}"#, "a\u{200D}b");
    assert!(parse_attribute_json(&debug).is_err(), "{debug:?}");
}

/// The scanner's code protection is pulldown's: the shapes the
/// hand-written fence/indent/backtick rules got wrong (review_0003 G17,
/// review_0004 H04) — a dropped real include fails the render ("Unknown
/// template"), a code sample taken for a reference fails the load
/// (NotFound). Each case states what CommonMark makes of the include.
#[test]
fn scanner_code_is_commonmark_code() {
    let path = AssetPath::from("page.md");
    let resolve = |source: &str| {
        resolve_references(&path, source, false)
            .map(|(_, refs)| refs.iter().map(|r| r.to_string()).collect::<Vec<_>>())
            .unwrap()
    };
    let real = vec!["real.md".to_owned()];
    let none = Vec::<String>::new();
    for (shape, source, expected) in [
        (
            "lazy continuation",
            "Intro\n    {% include \"real.md\" %}\n",
            &real,
        ),
        (
            "list item paragraph",
            "- item\n\n    {% include \"real.md\" %}\n",
            &real,
        ),
        (
            "tab-indented code",
            "Intro\n\n\t{% include \"gone.md\" %}\n",
            &none,
        ),
        (
            "escaped backticks",
            "\\`{% include \"real.md\" %}\\`\n",
            &real,
        ),
        (
            "longer fence closer",
            "```\ncode\n````\n\n{% include \"real.md\" %}\n",
            &real,
        ),
        (
            "shorter fence closer",
            "````\n```\n{% include \"gone.md\" %}\n````\n",
            &none,
        ),
        (
            "fence in a quote",
            "> ```\n> {% include \"gone.md\" %}\n> ```\n",
            &none,
        ),
        (
            "fence in a list",
            "- item\n\n  ```\n  {% include \"gone.md\" %}\n  ```\n",
            &none,
        ),
        (
            "double-backtick span",
            "``a ` {% include \"gone.md\" %} ``\n",
            &none,
        ),
        (
            "unclosed span",
            "a ` b\n\n{% include \"real.md\" %}\n",
            &real,
        ),
        // Indentation inside a Tera block isn't Markdown (review_0004
        // H11): this body line is a paragraph, so the include runs.
        (
            "indented block body",
            "{% if a %}\n    see {% include \"real.md\" %}\n{% endif %}\n",
            &real,
        ),
    ] {
        assert_eq!(&resolve(source), expected, "{shape}");
    }
}

/// Malformed attribute JSON is an error, not a crash (a backslash before
/// a non-ASCII character once panicked the removed escape rewrite).
#[test]
fn malformed_attribute_json_is_an_error() {
    assert!(parse_attribute_json("{\"a\": \"\\u{41}\\é\"}").is_err());
    assert!(parse_attribute_json("{\"a\": ").is_err());
}

/// The scanner's code protection agrees with pulldown on the shapes G13
/// found: a fence after prose, indented code, whitespace-control raw
/// variants, code spans across blank lines (their content is scanned),
/// and raw blocks holding quotes (the endraw scan is plain). Fails when
/// the scanner's protection is reverted.
#[test]
fn scanner_protects_code_like_pulldown() {
    let path = AssetPath::from("page.md");
    let resolve = |source: &str| {
        resolve_references(&path, source, false)
            .map(|(_, refs)| refs.iter().map(|r| r.to_string()).collect::<Vec<_>>())
    };
    // Protected content: the include inside is not a reference.
    assert_eq!(
        resolve("Intro\n\n~~~\n{% include \"gone.md\" %}\n~~~").unwrap(),
        Vec::<String>::new(),
        "a fence after prose protects"
    );
    assert_eq!(
        resolve("Intro\n\n    {% include \"gone.md\" %}\n").unwrap(),
        Vec::<String>::new(),
        "indented code protects"
    );
    assert_eq!(
        resolve("{%raw-%}\n{% include \"gone.md\" %}\n{% endraw -%}").unwrap(),
        Vec::<String>::new(),
        "the whitespace-control raw variant protects"
    );
    assert_eq!(
        resolve("{% raw %}{% it's %}{% endraw %}{% include \"real.html\" %}").unwrap(),
        vec!["real.html".to_owned()],
        "a quote inside a raw block doesn't swallow the endraw"
    );
    // Unprotected: the span's content is scanned.
    assert_eq!(
        resolve("a `\n\n b {% include \"gone.md\" %} c ` d").unwrap(),
        vec!["gone.md".to_owned()],
        "a code span can't span a blank line"
    );
}
#[cfg(test)]
mod tests {
    /// The scanners are linear (review_0003 G14: they used to re-find each
    /// stop needle, or re-scan the line, per statement — a 320 KB template
    /// cost seconds to load). Asserted as *scaling*, not wall time (review_0004
    /// H09: a 2 s bound passed a quadratic rewrite at 1.25 s): 4× the input
    /// may cost at most 8× the time — linear is ~4×, quadratic ~16× — best of
    /// three runs each, so machine speed and load cancel out.
    /// review_0005 I04: each `&#` scanned to the next `;` (or the end of the
    /// text) before checking the digit bound, so a text of `&#` references
    /// without terminators decoded in O(n²) — on every render. Scaling, not
    /// wall time: 4× the input may cost at most 8× (best of three).
    #[test]
    fn entity_decoding_stays_linear_without_terminators() {
        use std::time::{Duration, Instant};
        let best = |text: &str| {
            (0..3)
                .map(|_| {
                    let start = Instant::now();
                    std::hint::black_box(decode_entities(text));
                    start.elapsed()
                })
                .min()
                .unwrap()
        };
        for (shape, unit) in [("numeric", "&#1"), ("hex", "&#x1"), ("named", "&am")] {
            let (small, large) = (best(&unit.repeat(10_000)), best(&unit.repeat(40_000)));
            assert!(
                large < small * 8 + Duration::from_millis(5),
                "decode_entities scales superlinearly on {shape}: 10k {small:?}, 40k {large:?}"
            );
        }
    }

    #[test]
    fn scanners_stay_linear_on_pathological_input() {
        use std::time::{Duration, Instant};
        let path = AssetPath::from("p.md");
        let best = |text: &str, run: &dyn Fn(&str)| -> Duration {
            (0..3)
                .map(|_| {
                    let start = Instant::now();
                    run(text);
                    start.elapsed()
                })
                .min()
                .unwrap()
        };
        let resolve = |text: &str| drop(resolve_references(&path, text, false));
        let separate = |text: &str| drop(md::separate_statements(text));
        // Deeply nested, indented blocks: the dedent re-reads each body per
        // level (review_0004 H11), bounded by its depth cap.
        let nested = |count: usize| {
            let open: String = (0..count)
                .map(|depth| format!("{}{{% if a %}}\n", " ".repeat(depth % 40)))
                .collect();
            let close: String = (0..count)
                .rev()
                .map(|depth| format!("{}{{% endif %}}\n", " ".repeat(depth % 40)))
                .collect();
            format!("{open}text\n{close}")
        };
        for (shape, small, large) in [
            (
                "expressions",
                "{{ x }}\n".repeat(10_000),
                "{{ x }}\n".repeat(40_000),
            ),
            ("code spans", "`c`\n".repeat(10_000), "`c`\n".repeat(40_000)),
            (
                "own-line statements",
                "{% if a %}x{% endif %}\n".repeat(10_000),
                "{% if a %}x{% endif %}\n".repeat(40_000),
            ),
            (
                "one line of inline statements",
                "{% if a %}x{% endif %} ".repeat(10_000),
                "{% if a %}x{% endif %} ".repeat(40_000),
            ),
            (
                "deeply nested indented blocks",
                nested(1_000),
                nested(4_000),
            ),
        ] {
            for (scanner, run) in [
                ("resolve_references", &resolve as &dyn Fn(&str)),
                ("separate_statements", &separate),
            ] {
                let (small_time, large_time) = (best(&small, run), best(&large, run));
                assert!(
                    large_time < small_time * 8 + Duration::from_millis(20),
                    "{scanner} scales superlinearly on {shape}: 10k {small_time:?}, 40k {large_time:?}"
                );
            }
        }
    }

    use super::*;
    use proptest::prelude::*;

    /// Text dense in markup characters and entity look-alikes (`&lt;`
    /// typed as text), plus arbitrary characters.
    const MARKUP_TEXT: &str = "(?s)(&lt;|&gt;|&amp;|&quot;|&#39;|[<>&\"';# a-z]|.){0,24}";

    /// References resolve like URLs from the template's directory and are
    /// rewritten to the resolved path; other tags and text stay as written.
    #[test]
    fn references_resolve_relative_to_the_template() {
        let path = AssetPath::parse("ui/pages/main.html").clone_owned();
        let source = r#"{% extends "base.html" %}{%- include '../lib/ui.html' -%}
{% include "/shared/foot.html" %}{% if include %}{{ "include" }}{% endif %}{% block x %}{% endblock %}"#;
        let (out, references) = resolve_references(&path, source, false).unwrap();
        assert_eq!(
            references
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["ui/pages/base.html", "ui/lib/ui.html", "shared/foot.html"]
        );
        assert_eq!(
            out,
            r#"{% extends "ui/pages/base.html" %}{%- include 'ui/lib/ui.html' -%}
{% include "shared/foot.html" %}{% if include %}{{ "include" }}{% endif %}{% block x %}{% endblock %}"#
        );
        let embedded = AssetPath::parse("embedded://app/ui/main.html").clone_owned();
        let (_, references) =
            resolve_references(&embedded, r#"{% include "part.html" %}"#, false).unwrap();
        assert_eq!(references[0].to_string(), "embedded://app/ui/part.html");
    }

    /// The scanner's edge cases: an unmatched backtick run is text (it
    /// used to skip to EOF, dropping later includes), a fence at the
    /// file's start closes (it never did), and a `"{#"` inside a Tera
    /// string — in a tag or an expression — is not a comment (it
    /// swallowed everything to the next `#}` or to EOF).
    #[test]
    fn scanner_edge_cases_keep_later_references() {
        let markdown = AssetPath::parse("main.md").clone_owned();
        let (_, references) =
            resolve_references(&markdown, "It`s here\n{% include \"a.md\" %}\n", false).unwrap();
        assert_eq!(references.len(), 1);

        let (_, references) = resolve_references(
            &markdown,
            "```rust\nx\n```\n{% include \"a.md\" %}\n",
            false,
        )
        .unwrap();
        assert_eq!(references.len(), 1);

        let html = AssetPath::parse("main.html").clone_owned();
        let (out, references) = resolve_references(
            &html,
            "{% set x = \"{#\" %}\n{% include \"a.html\" %}\n",
            false,
        )
        .unwrap();
        assert_eq!(references.len(), 1);
        assert!(out.contains("x = \"{#\""), "the tag is verbatim: {out}");

        let (out, references) = resolve_references(
            &html,
            "{% include \"a.html\" %}{{ \"{#\" }}{% include \"b.html\" %}\n",
            false,
        )
        .unwrap();
        assert_eq!(references.len(), 2, "{references:?}");
        assert!(out.contains("{{ \"{#\" }}"), "{out}");
    }

    /// Each of the five references Tera's autoescaping emits decodes to its
    /// character. Catches a dropped or misspelled entry (e.g. `&#x27;`
    /// instead of `&#39;`), which would show raw entities in the UI.
    #[test]
    fn decodes_the_five_escapes() {
        assert_eq!(
            decode_entities("&lt;b&gt; &quot;x&quot; &#39;y&#39; &amp;"),
            "<b> \"x\" 'y' &"
        );
    }

    /// Decoding undoes exactly one level of escaping: an escaped entity
    /// (`&amp;lt;`, what Tera emits for a literal `&lt;`) becomes the entity
    /// text, not the character. Catches decoding `&amp;` before the others
    /// (or decoding repeatedly), which turns authored `&lt;` into `<`.
    #[test]
    fn decodes_one_level_only() {
        assert_eq!(decode_entities("&amp;lt;"), "&lt;");
        assert_eq!(decode_entities("&amp;amp;"), "&amp;");
        assert_eq!(decode_entities("&amp;quot;&amp;#39;"), "&quot;&#39;");
    }

    /// Known references decode (ADR 0027); unknown, unterminated, or
    /// non-semicolon legacy ones stay verbatim, as in a browser; and the
    /// one-level rule keeps a partial decode from re-decoding
    /// (`&amp;nbsp;` reads `&nbsp;`, not the space).
    #[test]
    fn other_references_decode_or_stay_verbatim() {
        for (text, expected) in [
            ("&nbsp;", "\u{a0}"),
            ("&#169;", "©"),
            ("&#x3C;", "<"),
            ("&copy;", "©"),
            ("&NotEqualTilde;", "\u{2242}\u{338}"), // two code points
            ("a & b", "a & b"),
            // The legacy non-`;` aliases are unsupported (the table holds
            // the semicolon forms only).
            ("&lt", "&lt"),
            ("&nbsp", "&nbsp"),
            ("&;", "&;"),
            // Unterminated numerics stay verbatim.
            ("&#39", "&#39"),
            // Invalid code points read U+FFFD (0, surrogates, past the top).
            ("&#0;", "\u{FFFD}"),
            ("&#xD800;", "\u{FFFD}"),
            ("&#x110000;", "\u{FFFD}"),
            // The C1 controls read their Windows-1252 replacements
            // (0x97 = U+2014, 0x92 = U+2019).
            ("&#151;", "\u{2014}"),
            ("&#146;", "\u{2019}"),
            // Digits only: a sign is no reference.
            ("&#+65;", "&#+65;"),
            ("&#x+41;", "&#x+41;"),
            ("&#x41;&#66;", "AB"),
            // One level: a decoded `&` is not re-scanned.
            ("&amp;lt;", "&lt;"),
        ] {
            assert_eq!(decode_entities(text), expected);
        }
    }

    /// Concatenated text of every raw node in `document`, decoded.
    fn decoded_text(document: &HtmlDocument) -> String {
        let dom = document.dom();
        dom.nodes()
            .iter()
            .filter_map(|node| match node {
                tl::Node::Raw(text) => Some(decode_entities(&text.as_utf8_str())),
                _ => None,
            })
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// Any string from a Tera variable (`.html` name → autoescaping)
        /// reads back verbatim after `tl` parsing and `decode_entities`:
        /// Tera's escape set and ours must match exactly (a Tera upgrade
        /// emitting `&#x27;`/`&apos;`, or a missing decode, shows entities
        /// in the UI; a missing escape lets data inject markup).
        #[test]
        fn rendered_variables_round_trip(s in MARKUP_TEXT) {
            let mut tera = tera::Tera::new();
            tera.add_raw_template("t.html", "<p>{{ s }}</p>").unwrap();
            let template = HtmlTemplate { tera, name: "t.html".to_owned() };
            let mut context = tera::Context::new();
            context.insert("s", &s);
            let document = template.render(&context).unwrap();
            prop_assert_eq!(document.dom().query_selector("p").unwrap().count(), 1);
            prop_assert_eq!(decoded_text(&document), s);
        }
    }

    /// The DOM outline indents each level by two spaces, skips blank text
    /// (formatting whitespace between tags), quotes text, and shows a
    /// translated element's translation (or error) instead of its children.
    #[test]
    fn outline_indents_levels_and_shows_translations() {
        let document = HtmlDocument::parse(
            "<div class=\"a\">\n  <p>Hi <b>there</b></p>\n  <p data-l10n-id=\"t\">x</p>\n  <p data-l10n-id=\"e\">y</p>\n</div>".to_owned(),
        )
        .unwrap();
        let paragraphs: Vec<tl::NodeHandle> = document.dom().query_selector("p").unwrap().collect();
        let mut localized = LocalizedText::default();
        localized
            .0
            .insert(paragraphs[1], Ok("Translated".to_owned()));
        localized.0.insert(paragraphs[2], Err("missing".to_owned()));
        let expected = "\
div class=\"a\"
  p
    \"Hi\"
    b
      \"there\"
  p data-l10n-id=\"t\"
    l10n \"Translated\"
  p data-l10n-id=\"e\"
    l10n error: missing
";
        assert_eq!(document.outline(&localized), expected);
    }

    /// `aria-*` (and every other) attribute shows verbatim in the outline:
    /// the debug view surfaces accessibility attributes without the
    /// pipeline interpreting them. (Attribute order in `tl` is a map
    /// iteration, so assert per attribute, not on the whole line.)
    #[test]
    fn outline_shows_aria_attributes() {
        let document = HtmlDocument::parse(
            "<button aria-label=\"Close\" aria-disabled=\"true\" data-on-click=\"close\">X</button>"
                .to_owned(),
        )
        .unwrap();
        let outline = document.outline(&LocalizedText::default());
        let line = outline.lines().next().unwrap();
        assert!(line.starts_with("button"), "{line}");
        assert!(line.contains("aria-label=\"Close\""), "{line}");
        assert!(line.contains("aria-disabled=\"true\""), "{line}");
        assert!(line.contains("data-on-click=\"close\""), "{line}");
        assert_eq!(outline.lines().nth(1).unwrap(), "  \"X\"");
    }

    /// Error messages include every `source()` in the chain (Tera puts the
    /// useful detail — the missing variable — in a source).
    #[test]
    fn error_chain_includes_all_sources() {
        #[derive(Debug)]
        struct Layer(&'static str, Option<Box<Layer>>);
        impl std::fmt::Display for Layer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0)
            }
        }
        impl std::error::Error for Layer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                self.1.as_deref().map(|layer| layer as _)
            }
        }
        let error = Layer(
            "render failed",
            Some(Box::new(Layer(
                "in t.html",
                Some(Box::new(Layer("unknown variable `x`", None))),
            ))),
        );
        assert_eq!(
            error_chain(&error),
            "render failed: in t.html: unknown variable `x`"
        );
    }

    /// Comments, raw blocks and — in Markdown — fenced code blocks and
    /// inline code spans protect their content: an include inside them is
    /// copied verbatim and not resolved (a commented-out include of a
    /// deleted file must not fail the load; bug_0067). In an `.html` file
    /// backticks are text: an include between them is still a reference.
    #[test]
    fn comments_raw_and_code_are_not_references() {
        let markdown = AssetPath::parse("page.md").clone_owned();
        for source in [
            r#"{# {%- include "gone.html" -%} #}"#,
            r#"{% raw %}{% include "gone.html" %}{% endraw %}"#,
            "```html\n{% include \"gone.html\" %}\n```",
            r#"`{% include "gone.html" %}`"#,
        ] {
            let (out, references) = resolve_references(&markdown, source, false).unwrap();
            assert!(references.is_empty(), "{source:?}: {references:?}");
            assert_eq!(out, source, "{source:?}: rewritten");
        }
        let html = AssetPath::parse("page.html").clone_owned();
        let (_, references) =
            resolve_references(&html, r#"`{% include "part.html" %}`"#, false).unwrap();
        assert_eq!(references.len(), 1, "{references:?}");
    }
}
/// bug_0087: an include escaping the asset root is refused unless the
/// app's `AssetPlugin::unapproved_path_mode` is `Allow`.
#[test]
fn includes_outside_the_root_follow_the_unapproved_mode() {
    let path = AssetPath::from("pages/page.html");
    let source = r#"{% include "../../outside.html" %}"#;
    assert!(resolve_references(&path, source, false).is_err());
    let (out, references) = resolve_references(&path, source, true).unwrap();
    assert_eq!(references.len(), 1, "{references:?}");
    assert!(references[0].is_unapproved(), "{out}");
}
