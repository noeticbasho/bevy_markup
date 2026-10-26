//! Markdown as a source syntax ([ADR 0014]): `.md` templates and Markdown
//! Fluent values convert here into the HTML the rest of the pipeline
//! already parses — Markdown is sugar for documents, never a second DOM
//! model.
//!
//! Dialect: CommonMark plus the GFM extensions (tables, strikethrough,
//! task lists) and footnotes. Raw inline/block HTML passes through
//! unchanged, so a Markdown page can embed real UI — `data-on-click`
//! buttons, containers — like an HTML template can.
//!
//! The conversion is small enough to run per re-render: streaming events
//! into `push_html`, no AST, no allocation beyond the output string.
//!
//! [ADR 0014]: docs/agents/adr/0014-markdown-via-pulldown-cmark.md

use std::collections::VecDeque;
use std::ops::Range;

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd};

/// The class injected into every element Markdown compilation constructs
/// (paragraphs, headings, images, links, emphasis, tables, …), so a
/// document styles its Markdown content apart from the raw-HTML `<img>`
/// tags it embeds — which pass through untouched, as in any HTML template,
/// and can add the class by hand if they want it.
pub(crate) const MARKDOWN_CLASS: &str = "markdown";

/// Emitted before every constructed start tag (see [`MARKDOWN_CLASS`]);
/// `to_html` rewrites `marker + <tag>` into the classed tag and drops the
/// marker before the HTML reaches `tl`.
const MARKER: &str = "\u{1}bevy-markup\u{1}";

/// The dialect: CommonMark plus tables, strikethrough, task lists and
/// footnotes. Shared by the conversion and the code-range scans
/// ([`code_ranges`]), which must agree on what is code.
const OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_FOOTNOTES);

/// Converts Markdown `source` to the HTML `tl` parses. Not a round trip:
/// the output is one `<p>` per paragraph etc., which the walk reads
/// transparently (a `<p>` inside a translated block's fragment becomes
/// plain runs).
#[hotpath::measure]
pub(crate) fn to_html(source: &str) -> String {
    convert(source, false)
}

/// [`to_html`](Self), for a `.md` *template* (ADR 0020): Tera syntax
/// survives verbatim — a paragraph of only Tera tags passes through
/// unwrapped, inline Tera runs inside text pass raw (unescaped) — so the
/// conversion runs once at load and the per-frame path is Tera-only.
/// Text inside code blocks and code spans is Tera-literal: it stays
/// escaped text, like any Markdown code content.
#[hotpath::measure]
pub(crate) fn to_html_template(source: &str) -> String {
    convert(&separate_statements(&dedent_blocks(source).0), true)
}

/// The byte ranges of `text` that pulldown renders as code: inline code
/// spans (backticks included) and code blocks — fenced (fence lines
/// included) and indented — in document order. CommonMark's own rules
/// decide (fence lengths, lazy continuations, tabs, escaped backticks,
/// blocks inside lists and quotes), not a re-implementation of them
/// (review_0003 G13/G17, review_0004 H04).
fn code_ranges(text: &str) -> Vec<Range<usize>> {
    Parser::new_ext(text, OPTIONS)
        .into_offset_iter()
        .filter(|(event, _)| matches!(event, Event::Code(_) | Event::Start(Tag::CodeBlock(_))))
        .map(|(_, range)| range)
        .collect()
}

/// The byte ranges of a `.md` template's `source` that its conversion
/// ([`to_html_template`]) renders as code — where Tera is literal text,
/// so an `{% include %}` there is no reference. Computed on the text the
/// conversion actually parses (after [`dedent_blocks`] and
/// [`separate_statements`], which change indentation and blank lines)
/// and mapped back to `source` offsets.
pub(crate) fn template_code_ranges(source: &str) -> Vec<Range<usize>> {
    let (dedented, lines) = dedent_blocks(source);
    let (separated, inserted) = separate_statements_mapped(&dedented);
    // `inserted` holds the separated text's offsets of the added newlines.
    let to_source =
        |at: usize| dedented_to_source(&lines, at - inserted.partition_point(|&added| added < at));
    code_ranges(&separated)
        .into_iter()
        .map(|range| to_source(range.start)..to_source(range.end))
        .collect()
}

/// Block bodies nested deeper than this keep their indentation: each
/// level re-parses its body to tell code from structure, so the pass is
/// O(size × depth) — 16 is far past authored nesting and keeps 600 KB of
/// pathologically nested blocks at ~50 ms (release).
const MAX_DEDENT_DEPTH: usize = 16;

/// One source line during [`dedent_blocks`]: the text (its `\n`
/// included) and the columns removed from its indentation so far.
struct DedentLine<'a> {
    text: &'a str,
    cut: usize,
}

impl DedentLine<'_> {
    /// The leading whitespace's width in columns (a tab advances to the
    /// next multiple of four, as CommonMark counts it) and its length in
    /// bytes.
    fn lead(&self) -> (usize, usize) {
        let mut width = 0;
        for (at, byte) in self.text.bytes().enumerate() {
            match byte {
                b' ' => width += 1,
                b'\t' => width += 4 - width % 4,
                _ => return (width, at),
            }
        }
        (width, self.text.len())
    }

    /// Whitespace only (or empty).
    fn blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// The indentation as the current level sees it (outer levels' cuts
    /// applied).
    fn indent(&self) -> usize {
        self.lead().0 - self.cut
    }

    /// The line as the current level sees it.
    fn render(&self, out: &mut String) {
        let (width, bytes) = self.lead();
        if self.cut == 0 {
            out.push_str(self.text);
        } else {
            out.extend(std::iter::repeat_n(' ', width - self.cut));
            out.push_str(&self.text[bytes..]);
        }
    }
}

/// Which block an own-line opener or closer belongs to: a Tera statement
/// keyword, or an HTML element name (lowercased).
#[derive(Clone, PartialEq, Eq)]
enum BlockKey<'a> {
    Tera(&'a str),
    Html(String),
}

/// What an own-line Tera statement or HTML tag does to the block structure.
#[derive(Clone, PartialEq, Eq)]
enum BlockTag<'a> {
    Open(BlockKey<'a>),
    /// `elif`/`else`: splits the innermost Tera block's body.
    Middle,
    Close(BlockKey<'a>),
}

impl BlockKey<'_> {
    /// How the block's body is treated: `Some(true)` dedented and searched
    /// for nested blocks, `Some(false)` dedented only (`{% raw %}`: Tera
    /// reads its statements as text), `None` left verbatim (`<pre>`,
    /// `<textarea>`, `<script>`, `<style>`: their whitespace is content).
    fn body(&self) -> Option<bool> {
        match self {
            BlockKey::Tera("raw") => Some(false),
            BlockKey::Tera(_) => Some(true),
            BlockKey::Html(name) => {
                (!matches!(name.as_str(), "pre" | "textarea" | "script" | "style")).then_some(true)
            }
        }
    }

    /// Whether only this block's own closer means anything inside it.
    fn opaque(&self) -> bool {
        self.body() != Some(true)
    }
}

/// HTML elements that have no closing tag.
const VOID_ELEMENTS: [&str; 14] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// The structural role of `line`: a Tera statement alone on its line
/// (`if`/`for`/`block`/`component`/`filter`/`raw` and a block
/// `set`/`set_global` — no `=` — open, `elif`/`else` split, `end…` close),
/// or an HTML tag alone on its line (`<div class="card">` opens, `</div>`
/// closes; void and self-closing tags, comments and lines holding more
/// than one tag don't). Anything else (inline statements, `include`,
/// `set x = …`, expressions, text, inline HTML) is content.
fn block_tag(line: &str) -> Option<BlockTag<'_>> {
    let trimmed = line.trim();
    if trimmed.starts_with('<') {
        return html_tag(trimmed);
    }
    let inner = trimmed.strip_prefix("{%")?.strip_suffix("%}")?;
    // One statement: no second closer inside.
    if inner.contains("%}") {
        return None;
    }
    let inner = inner.trim_matches(['-', '+']).trim();
    let keyword = inner
        .split(|c: char| c.is_whitespace() || c == '(')
        .next()?;
    match keyword {
        "if" | "for" | "block" | "component" | "filter" | "raw" => {
            Some(BlockTag::Open(BlockKey::Tera(keyword)))
        }
        "set" | "set_global" if !inner.contains('=') => Some(BlockTag::Open(BlockKey::Tera("set"))),
        "elif" | "else" => Some(BlockTag::Middle),
        _ => {
            let closed = keyword.strip_prefix("end")?;
            let closed = if closed == "set_global" {
                "set"
            } else {
                closed
            };
            matches!(
                closed,
                "if" | "for" | "block" | "component" | "filter" | "raw" | "set"
            )
            .then_some(BlockTag::Close(BlockKey::Tera(closed)))
        }
    }
}

/// [`block_tag`] for a trimmed line starting with `<`.
fn html_tag(trimmed: &str) -> Option<BlockTag<'static>> {
    let valid = |name: &str| {
        name.starts_with(|c: char| c.is_ascii_alphabetic())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    if let Some(rest) = trimmed.strip_prefix("</") {
        let name = rest.strip_suffix('>')?.trim_end();
        return valid(name).then(|| BlockTag::Close(BlockKey::Html(name.to_ascii_lowercase())));
    }
    let rest = trimmed.strip_prefix('<')?;
    // One opening tag: it ends the line, and nothing else opens a tag.
    if !rest.ends_with('>') || rest.ends_with("/>") || rest.contains('<') {
        return None;
    }
    let name_end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(rest.len());
    let (name, after) = rest.split_at(name_end);
    if !valid(name) || !after.starts_with(|c: char| c.is_whitespace() || c == '>') {
        return None;
    }
    let name = name.to_ascii_lowercase();
    (!VOID_ELEMENTS.contains(&name.as_str())).then(|| BlockTag::Open(BlockKey::Html(name)))
}

/// Removes, from the body of every own-line Tera block (`{% if %}`,
/// `{% for %}`, `{% block %}`, … each segment between its opener, its
/// `elif`/`else` and its closer) and every own-line HTML container
/// (`<div>` … `</div>`; review_0004's H11 follow-up: an indented
/// `<div>` body after a blank line was an indented code block too — not
/// `<pre>`/`<textarea>`/`<script>`/`<style>`, whose whitespace is
/// content), the indentation the body's lines share —
/// recursively, outermost first, so nested bodies line up with their
/// statements. Tera indentation is for the author's readability, not
/// Markdown: without this, a body indented four spaces became an indented
/// code block once separation put a blank line after its statement
/// (review_0004 H11). A body's *relative* indentation (a nested list, an
/// indented code block inside it) is kept. Statement lines that Markdown
/// reads as code (a Tera sample in a fence) are content, not structure;
/// a `{% raw %}` body is dedented but not searched for statements.
///
/// Returns the text and, per line, where its content starts in the
/// output and in `source`, for mapping output offsets back.
fn dedent_blocks(source: &str) -> (String, Vec<LineMap>) {
    let mut lines: Vec<DedentLine<'_>> = source
        .split_inclusive('\n')
        .map(|text| DedentLine { text, cut: 0 })
        .collect();
    if source.contains("{%") || source.contains('<') {
        dedent_level(&mut lines, 0);
    }
    let mut out = String::with_capacity(source.len());
    let mut map = Vec::with_capacity(lines.len());
    let mut source_at = 0;
    for line in &lines {
        let line_start = out.len();
        line.render(&mut out);
        let (_, lead_bytes) = line.lead();
        let content_len = line.text.len() - lead_bytes;
        map.push(LineMap {
            out_start: line_start,
            out_content: out.len() - content_len,
            source_start: source_at,
            source_content: source_at + lead_bytes,
        });
        source_at += line.text.len();
    }
    (out, map)
}

/// Where one line's content starts in [`dedent_blocks`]' output and in
/// its source (the removed part is leading whitespace only).
struct LineMap {
    out_start: usize,
    out_content: usize,
    source_start: usize,
    source_content: usize,
}

/// A [`dedent_blocks`] output offset as a source offset.
fn dedented_to_source(map: &[LineMap], at: usize) -> usize {
    let Some(line) = map[..map.partition_point(|line| line.out_start <= at)].last() else {
        return at;
    };
    if at >= line.out_content {
        line.source_content + (at - line.out_content)
    } else {
        // Inside the (shortened) indentation: the same distance before the
        // content, within the source line's own indentation.
        line.source_content
            .saturating_sub(line.out_content - at)
            .max(line.source_start)
    }
}

/// One level of [`dedent_blocks`]: finds the top-level blocks among
/// `lines`, dedents each body segment, and recurses into it.
fn dedent_level(lines: &mut [DedentLine<'_>], depth: usize) {
    if depth > MAX_DEDENT_DEPTH {
        return;
    }
    // The level as Markdown reads it, to tell code from structure.
    let mut text = String::new();
    let mut starts = Vec::with_capacity(lines.len());
    for line in lines.iter() {
        line.render(&mut text);
        // Where the line's content starts (after its rendered indentation).
        starts.push(text.len() - (line.text.len() - line.lead().1));
    }
    let code = code_ranges(&text);
    let mut code_index = 0;
    let mut in_code = |index: usize| {
        let at = starts[index];
        while code.get(code_index).is_some_and(|range| range.end <= at) {
            code_index += 1;
        }
        code.get(code_index).is_some_and(|range| range.start <= at)
    };

    // The top-level blocks: their key, opener, `elif`/`else` and closer
    // lines.
    let mut blocks: Vec<(BlockKey<'_>, Vec<usize>)> = Vec::new();
    let mut stack: Vec<(BlockKey<'_>, Vec<usize>)> = Vec::new();
    // Inside an opaque block (`{% raw %}`, `<pre>`, …) only its own closer
    // means anything.
    let mut opaque: Option<BlockKey<'_>> = None;
    for index in 0..lines.len() {
        if lines[index].blank() || in_code(index) {
            continue;
        }
        let Some(tag) = block_tag(lines[index].text) else {
            continue;
        };
        if let Some(key) = &opaque {
            if tag != BlockTag::Close(key.clone()) {
                continue;
            }
        }
        match tag {
            BlockTag::Open(key) => {
                if key.opaque() {
                    opaque = Some(key.clone());
                }
                stack.push((key, vec![index]));
            }
            BlockTag::Middle => {
                if let Some((BlockKey::Tera(_), marks)) = stack.last_mut() {
                    marks.push(index);
                }
            }
            BlockTag::Close(key) => {
                opaque = None;
                // An unclosed inner block is dropped (Tera would reject the
                // template; HTML would close it implicitly); a closer with no
                // opener is content.
                let Some(open) = stack.iter().rposition(|(open, _)| *open == key) else {
                    continue;
                };
                stack.truncate(open + 1);
                let (key, mut marks) = stack.pop().expect("found above");
                if stack.is_empty() {
                    marks.push(index);
                    blocks.push((key, marks));
                }
            }
        }
    }

    for (key, marks) in blocks {
        let Some(search) = key.body() else {
            continue;
        };
        for pair in marks.windows(2) {
            let body = &mut lines[pair[0] + 1..pair[1]];
            dedent_segment(body);
            if search {
                dedent_level(body, depth + 1);
            }
        }
    }
}

/// Removes the indentation `body`'s lines share. Blank lines don't count,
/// nor do a fenced block's inner lines — their indentation is the code's,
/// and CommonMark strips at most the fence's own indent from them — so a
/// fence's content at column 0 doesn't pin the segment.
fn dedent_segment(body: &mut [DedentLine<'_>]) {
    let mut fence: Option<(u8, usize)> = None;
    let mut shared = usize::MAX;
    for line in body.iter() {
        if line.blank() {
            continue;
        }
        let trimmed = line.text.trim_start();
        let run = |marker: u8| trimmed.bytes().take_while(|&byte| byte == marker).count();
        match fence {
            Some((marker, length)) => {
                // The closer: the same marker, at least as long, nothing after.
                if run(marker) >= length && trimmed[run(marker)..].trim().is_empty() {
                    fence = None;
                    shared = shared.min(line.indent());
                }
            }
            None => {
                shared = shared.min(line.indent());
                for marker in [b'`', b'~'] {
                    if run(marker) >= 3 {
                        fence = Some((marker, run(marker)));
                    }
                }
            }
        }
    }
    if shared == 0 || shared == usize::MAX {
        return;
    }
    for line in body.iter_mut() {
        line.cut += line.indent().min(shared);
    }
}

/// Puts Tera *statements* (`{% … %}`, `{# … #}`, including the `-`
/// whitespace-control variants) on their own line when they have a line
/// to themselves, so they cannot merge with neighbouring Markdown
/// content into one block. A statement *inside* a line (an attribute
/// value, a list item, a heading, a table cell, or after other content
/// on its line — inline Tera included, review_0003 G18) stays inline —
/// the Tera rewrite (ADR 0020) handles inline runs. Statements inside
/// code (code spans, fenced and indented code blocks, as pulldown reads
/// the source) are left verbatim. Interpolations (`{{ … }}`) are inline
/// content and stay where they are.
pub(crate) fn separate_statements(source: &str) -> String {
    separate_statements_mapped(source).0
}

/// [`separate_statements`], plus the output offsets of the newlines it
/// inserted (ascending), for mapping output offsets back to `source`.
fn separate_statements_mapped(source: &str) -> (String, Vec<usize>) {
    if !source.contains("{%") && !source.contains("{#") {
        return (source.to_owned(), Vec::new());
    }
    // Separation never moves code: its blank lines go around whole lines
    // outside code, which can't open, close or split a fence or a span.
    let code = code_ranges(source);
    let mut code_index = 0;
    let mut out = String::with_capacity(source.len() + 16);
    let mut inserted = Vec::new();
    let bytes = source.as_bytes();
    // Every scan moves forward from the cursor or covers only the bytes
    // between the previous statement and this one, so the loop is linear
    // (review_0003 G14: re-scanning a long line per statement was
    // quadratic).
    let mut cursor = 0;
    // The start of the line holding the current statement.
    let mut line_start = 0;
    // The first newline at or after the current statement, else
    // `source.len()` (cached for the statements sharing one long line: a
    // missing newline must be remembered too, or every statement on a
    // last line re-scans to the end).
    let mut next_newline: Option<usize> = None;
    let mut search = 0;
    while let Some(found) = source[search..].find('{') {
        let start = search + found;
        let closer = match bytes.get(start + 1) {
            Some(b'%') => "%}",
            Some(b'#') => "#}",
            _ => {
                search = start + 1;
                continue;
            }
        };
        // Inside code: skip to the code's end.
        while code.get(code_index).is_some_and(|range| range.end <= start) {
            code_index += 1;
        }
        if let Some(range) = code.get(code_index).filter(|range| range.start <= start) {
            search = range.end;
            continue;
        }
        // A lone `{%`/`{#` without its closer is text; so is the rest.
        let Some(inner) = source[start + 2..].find(closer) else {
            break;
        };
        let end = start + 2 + inner + closer.len();
        // The statement's line: everything before it on its line counts —
        // an inline statement earlier on the line included (G18: the old
        // line start was the cursor, mid-line after such a statement).
        if let Some(newline) = source[cursor..start].rfind('\n') {
            line_start = cursor + newline + 1;
        }
        if next_newline.is_none_or(|at| at < start) {
            next_newline = Some(
                source[start..]
                    .find('\n')
                    .map_or(source.len(), |at| start + at),
            );
        }
        let line_end = next_newline.unwrap_or(source.len());
        let before = &source[line_start..start];
        // A multi-line statement's closer is past its first line's end:
        // nothing follows it on that line.
        let after = if end <= line_end {
            &source[end..line_end]
        } else {
            ""
        };
        out.push_str(&source[cursor..start]);
        if before.trim().is_empty() && after.trim().is_empty() {
            inserted.push(out.len());
            out.push('\n');
            out.push_str(&source[start..end]);
            inserted.push(out.len());
            out.push('\n');
        } else {
            out.push_str(&source[start..end]);
        }
        cursor = end;
        search = end;
    }
    out.push_str(&source[cursor..]);
    (out, inserted)
}

/// The shared conversion: `tera` selects whether Tera syntax passes
/// through verbatim (ADR 0020) or converts as Markdown text.
fn convert(source: &str, tera: bool) -> String {
    let mut html = String::with_capacity(source.len() * 2);
    let parser = Parser::new_ext(source, OPTIONS);
    let marked = Marked {
        parser,
        pending: VecDeque::new(),
    };
    let events: Vec<Event<'_>> = marked.collect();
    let events = decode_code_entities(events);
    let events = if tera { rewrite(events) } else { events };
    pulldown_cmark::html::push_html(&mut html, &mut events.into_iter());
    inject_class(&html)
}

/// Decodes the five entities in code spans and code blocks, once: code
/// content is the one place pulldown doesn't resolve character
/// references (`&amp;` stays in the event text), so a value escaped by
/// `parse_args` was escaped a second time by `push_html` there and a
/// code span showed `&amp;` for `&`. Text events need no decode:
/// pulldown decodes references into them, and `push_html` re-escapes —
/// which is also what keeps an escaped value's `<` text instead of
/// markup (review_0003 G01). The build decodes the result once more, so
/// a pattern's own documented `&lt;` still shows as `<`.
fn decode_code_entities(events: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut code_block = false;
    events
        .into_iter()
        .map(|event| match event {
            Event::Start(Tag::CodeBlock(_)) => {
                code_block = true;
                event
            }
            Event::End(TagEnd::CodeBlock) => {
                code_block = false;
                event
            }
            Event::Code(code) => Event::Code(crate::template::decode_entities(&code).into()),
            Event::Text(text) if code_block => {
                Event::Text(crate::template::decode_entities(&text).into())
            }
            event => event,
        })
        .collect()
}

/// An event stream that emits [`MARKER`] before every constructed start
/// tag. Raw HTML (`Html`/`InlineHtml` events) is passed through without
/// one — that is the whole distinction: pulldown-constructed versus
/// handwritten. Raw HTML *blocks* skip the marker too: pulldown writes
/// their content verbatim, so a marker there would class the author's own
/// first tag.
struct Marked<'a> {
    parser: Parser<'a>,
    /// The start tags waiting behind their markers.
    pending: VecDeque<Event<'a>>,
}

impl<'a> Iterator for Marked<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Event<'a>> {
        if let Some(event) = self.pending.pop_front() {
            return Some(event);
        }
        match self.parser.next()? {
            // Not an element of its own: content only.
            Event::Start(Tag::HtmlBlock) => Some(Event::Start(Tag::HtmlBlock)),
            Event::Start(tag) => {
                // The marker must not be treated as text; it is an `Html`
                // event because `push_html` writes those verbatim.
                self.pending.push_back(Event::Start(tag));
                Some(Event::Html(CowStr::from(MARKER)))
            }
            event => Some(event),
        }
    }
}

/// Adjusts the raw-block depth for the Tera `raw`/`endraw` tags in
/// `text` (any `{% … %}` whose body is `raw`/`endraw`; Tera scans raw
/// content textually, so a tag inside a code fence still ends the
/// block). Returns the depth BEFORE the tags.
fn raw_depth_before(text: &str, raw: &mut usize) -> usize {
    let before = *raw;
    for (tera, part) in tera_runs(text) {
        if !tera {
            continue;
        }
        let body = part
            .trim_start_matches("{%")
            .trim_start_matches("{%-")
            .trim_end_matches("%}")
            .trim_matches(['-', ' ']);
        if body == "raw" {
            *raw += 1;
        } else if body == "endraw" {
            *raw = raw.saturating_sub(1);
        }
    }
    before
}

/// The Tera syntax runs of `text`: `(tera, text)` pairs in order — every
/// `{{ … }}`, `{% … %}` and `{# … #}` (quoted strings inside a tag skip
/// their own closing pair). `tera` also swallows a trailing unclosed tag
/// (the source is mid-template-expansion).
fn tera_runs(text: &str) -> Vec<(bool, &str)> {
    let bytes = text.as_bytes();
    let mut runs: Vec<(bool, &str)> = Vec::new();
    let mut start = 0;
    let mut scan = 0;
    while scan < bytes.len() {
        if bytes[scan] == b'{' && scan + 1 < bytes.len() {
            let opener = bytes[scan + 1];
            if opener == b'{' || opener == b'%' || opener == b'#' {
                let closer = match opener {
                    b'{' => b'}',
                    b'%' => b'%',
                    _ => b'#',
                };
                // Scan to the closer, skipping quoted strings.
                let mut cursor = scan + 2;
                let mut end = None;
                while cursor < bytes.len() {
                    if bytes[cursor] == b'"' || bytes[cursor] == b'\'' {
                        let quote = bytes[cursor];
                        cursor += 1;
                        while cursor < bytes.len() && bytes[cursor] != quote {
                            cursor += if bytes[cursor] == b'\\' { 2 } else { 1 };
                        }
                        cursor += 1;
                        continue;
                    }
                    if bytes[cursor] == closer
                        && cursor + 1 < bytes.len()
                        && bytes[cursor + 1] == b'}'
                    {
                        end = Some(cursor + 2);
                        break;
                    }
                    cursor += 1;
                }
                let end = end.unwrap_or(bytes.len());
                if scan > start {
                    runs.push((false, &text[start..scan]));
                }
                runs.push((true, &text[scan..end]));
                scan = end;
                start = end;
                continue;
            }
        }
        scan += 1;
    }
    if start < text.len() {
        runs.push((false, &text[start..]));
    }
    if runs.is_empty() {
        runs.push((false, text));
    }
    runs
}

/// The Tera-aware rewrite (ADR 0020): text splits into plain and Tera
/// runs — Tera as raw `Html`, which `push_html` writes unescaped — and a
/// paragraph whose text is only Tera passes through without its `<p>`
/// wrapper and marker (no element is constructed). Code blocks and code
/// spans are Tera-literal: their text stays escaped, like any Markdown
/// code content.
fn rewrite(events: Vec<Event<'_>>) -> Vec<Event<'_>> {
    let mut out = Vec::with_capacity(events.len());
    let mut code_block = false;
    // Inside a `{% raw %}` block: Tera passes its content through
    // verbatim, so code there must NOT be Tera-literalized — the escape
    // braces would pass through raw and display as `{{ "{" }}` garbage
    // (review_0003 G07).
    let mut raw = 0usize;
    let mut index = 0;
    while index < events.len() {
        match &events[index] {
            // The marker precedes a constructed start tag. A paragraph
            // start begins a pure-Tera inspection: the paragraph's text up
            // to its end, its marker and its wrapper dropped when every
            // text run in it is Tera.
            Event::Html(text) if text.as_ref() == MARKER => {
                let starts_paragraph =
                    matches!(events.get(index + 1), Some(Event::Start(Tag::Paragraph)));
                if starts_paragraph {
                    // The paragraph's text as one string: a Tera span can
                    // span a softbreak (a multi-line `{# comment #}` —
                    // per-piece scanning read each line separately and
                    // left the comment visible as a paragraph).
                    let mut whole = String::new();
                    let mut pure = true;
                    let mut cursor = index + 2;
                    while cursor < events.len()
                        && !matches!(events[cursor], Event::End(TagEnd::Paragraph))
                    {
                        match &events[cursor] {
                            Event::Text(text) => whole.push_str(text),
                            Event::SoftBreak => whole.push('\n'),
                            _ => pure = false,
                        }
                        cursor += 1;
                    }
                    let ends = cursor < events.len();
                    if pure && ends && tera_runs(&whole).iter().all(|(tera, _)| *tera) {
                        // The paragraph's Tera tags pass through verbatim;
                        // their raw tags still count (Tera scans them).
                        raw_depth_before(&whole, &mut raw);
                        out.push(Event::Html(CowStr::Boxed(whole.into_boxed_str())));
                        index = cursor + 1;
                        continue;
                    }
                }
                out.push(Event::Html(CowStr::from(MARKER)));
                index += 1;
            }
            Event::Start(Tag::CodeBlock(_)) => {
                code_block = true;
                out.push(events[index].clone());
                index += 1;
            }
            Event::End(TagEnd::CodeBlock) => {
                code_block = false;
                out.push(events[index].clone());
                index += 1;
            }
            // Inline code: literal, escaped by `push_html` — and
            // Tera-literal: Tera never sees an original opener in it —
            // unless a raw block wraps it: Tera scans raw content
            // textually (its raw ends even inside a code fence), so the
            // verbatim content's own raw tags still count.
            Event::Code(code) => {
                let was_raw = raw > 0;
                if was_raw {
                    raw_depth_before(code, &mut raw);
                    out.push(Event::Code(code.clone()));
                } else {
                    out.push(Event::Code(tera_literal(code)));
                }
                index += 1;
            }
            Event::Text(text) if code_block => {
                let was_raw = raw > 0;
                if was_raw {
                    raw_depth_before(text, &mut raw);
                    out.push(Event::Text(text.clone()));
                } else {
                    out.push(Event::Text(tera_literal(text)));
                }
                index += 1;
            }
            // Text outside code: the raw tags in it count (Tera sees
            // them); the runs pass through as before.
            Event::Text(text) | Event::Html(text)
                if text.contains("raw") && text.contains("%}") =>
            {
                raw_depth_before(text, &mut raw);
                let was_raw = raw > 0;
                for (tera, part) in tera_runs(text) {
                    if tera {
                        out.push(Event::Html(CowStr::Boxed(part.to_owned().into_boxed_str())));
                    } else {
                        out.push(Event::Text(CowStr::Boxed(part.to_owned().into_boxed_str())));
                    }
                }
                let _ = was_raw;
                index += 1;
            }
            Event::Text(text) => {
                for (tera, part) in tera_runs(text) {
                    if tera {
                        out.push(Event::Html(CowStr::Boxed(part.to_owned().into_boxed_str())));
                    } else {
                        out.push(Event::Text(CowStr::Boxed(part.to_owned().into_boxed_str())));
                    }
                }
                index += 1;
            }
            event => {
                out.push(event.clone());
                index += 1;
            }
        }
    }
    out
}

/// Code text that Tera cannot execute: every `{` becomes an expression
/// printing a literal brace, so no original `{{`/`{%`/`{#` opener
/// survives (Tera sees only balanced expressions it runs). Code spans
/// and blocks keep their content verbatim — ADR 0020 — whether the
/// content is a variable reference, a raw block or an unbalanced string.
fn tera_literal(code: &CowStr<'_>) -> CowStr<'static> {
    let code: &str = code;
    // Owned: `out` must not borrow the event vector.
    CowStr::from(if code.contains('{') {
        code.replace('{', "{{ \"{\" }}")
    } else {
        code.to_owned()
    })
}

/// Rewrites each `MARKER + <tag …>` into `<tag … class="markdown">`
/// (appending to a tag that already has a class) and drops the markers.
fn inject_class(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(index) = rest.find(MARKER) {
        out.push_str(&rest[..index]);
        rest = &rest[index + MARKER.len()..];
        // The constructed start tag follows (pulldown may put a newline
        // between, since the marker leaves the output mid-line).
        let whitespace = rest.len() - rest.trim_start().len();
        out.push_str(&rest[..whitespace]);
        rest = &rest[whitespace..];
        let Some(tag) = rest.strip_prefix('<') else {
            // No tag after the marker (nothing to class): drop the marker.
            continue;
        };
        let Some(close) = tag.find('>') else {
            break;
        };
        let (inside, after) = tag.split_at(close);
        let name_end = inside
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .unwrap_or(inside.len());
        let (name, attrs) = inside.split_at(name_end);
        let classed = match attrs.find(" class=\"") {
            // Append inside the existing class list.
            Some(at) => {
                let value_at = at + " class=\"".len();
                match attrs[value_at..].find('"') {
                    // A class-like text inside another attribute's value
                    // with no closing `"`: leave the tag verbatim (the
                    // old `unwrap_or(0)` sliced at `value_end + 1`,
                    // past the end or inside a multi-byte character —
                    // content could panic the loader: review_0003 G04;
                    // `MARKER` is plain text nothing strips).
                    None => {
                        out.push('<');
                        out.push_str(&tag.split_at(close).0);
                        out.push('>');
                        rest = &after[1..];
                        continue;
                    }
                    Some(value_rel) => {
                        let value_end = value_at + value_rel;
                        format!(
                            "{name}{before} {MARKDOWN_CLASS}\"{after_tail}",
                            before = &attrs[..value_end],
                            after_tail = &attrs[value_end + 1..],
                        )
                    }
                }
            }
            None => format!("{name} class=\"{MARKDOWN_CLASS}\"{attrs}"),
        };
        out.push('<');
        out.push_str(&classed);
        out.push('>');
        rest = &after[1..];
    }
    out.push_str(rest);
    out
}

/// Whether an asset path is a Markdown template (see the
/// `MarkdownTemplateLoader`).
pub(crate) fn is_markdown(name: &str) -> bool {
    name.ends_with(".md") || name.ends_with(".markdown")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fenced block with an info string closes again: the closing
    /// check sat behind `fence.is_none()`, so everything after a
    /// ```` ```rust ```` fence stayed verbatim and own-line statements
    /// merged into one paragraph (the a=false render left an empty
    /// paragraph behind). A bare fence opens too, so its statements stay
    /// *inside* the code instead of executing.
    #[test]
    fn fences_close_and_statements_after_them_separate() {
        let render = |source: &str, json: &str| -> String {
            let html = to_html_template(source);
            let value: serde_json::Value = serde_json::from_str(json).unwrap();
            let mut tera = tera::Tera::default();
            tera.add_raw_template("md", &html).unwrap();
            let context = tera::Context::from_serialize(&value).unwrap();
            tera.render("md", &context).unwrap()
        };

        // After the closed fence, the statement is separated: `Shown`
        // keeps its own paragraph (no merged `<p>{% if %}…{% endif %}</p>`,
        // which renders an empty paragraph when `a` is false).
        let source = "```rust\nx\n```\n\n{% if a %}\nShown\n{% endif %}\n";
        let html = render(source, r#"{"a":true}"#);
        assert!(html.contains("<p class=\"markdown\">Shown</p>"), "{html}");
        let html = render(source, r#"{"a":false}"#);
        assert!(
            !html.contains("<p class=\"markdown\"></p>"),
            "the merged paragraph renders empty: {html}"
        );

        // A fence after prose, before the statement: the scanner jumps
        // to the first statement, so the fence line must be found on the
        // way (it used to be skipped, and the statements separated
        // inside the code). Rendered: the statements stay code (Tera
        // doesn't execute them), so "Shown" never becomes a paragraph.
        let source = "Intro\n\n```html\n{% if a %}\nShown\n{% endif %}\n```\n";
        let html = to_html_template(source);
        let code_at = html.find("<pre").expect("the fenced block: {html}");
        assert!(
            html[code_at..].contains("% if a %"),
            "the statements left the code: {html}"
        );
        let value: serde_json::Value = serde_json::from_str(r#"{"a":true}"#).unwrap();
        let mut tera = tera::Tera::default();
        tera.add_raw_template("md", &html).unwrap();
        let context = tera::Context::from_serialize(&value).unwrap();
        let rendered = tera.render("md", &context).unwrap();
        assert!(
            !rendered.contains("<p class=\"markdown\">Shown</p>"),
            "the statements executed outside the code: {rendered}"
        );

        // A bare fence opens too (Markdown semantics): its statements are
        // code, not executed — and no blank lines were inserted around
        // them (the old behavior split them, adding empty lines to the
        // code).
        let source = "```\ncode\n{% if a %}\nShown\n{% endif %}\n```\n";
        let html = render(source, r#"{"a":true}"#);
        assert!(!html.contains("<p class=\"markdown\">Shown</p>"), "{html}");
        let code_at = html.find("<code").expect("the code block: {html}");
        assert!(html[code_at..].contains("Shown"), "{html}");
        assert!(
            !html[code_at..].contains("code\n\n"),
            "blank lines were inserted into the code: {html}"
        );
    }

    /// A `{# … #}` comment with no backtick after it used to spin
    /// forever: the code-span branch consumed nothing. Every shape
    /// terminates, and the comment/text survive verbatim.
    #[test]
    fn a_comment_without_a_backtick_terminates() {
        let out = separate_statements("{# note #}\nHello\n");
        assert!(out.contains("{# note #}\n"), "{out}");
        assert!(out.contains("Hello"), "{out}");

        let out = separate_statements("Hi {# c #} there");
        assert!(out.contains("Hi {# c #} there"), "{out}");

        let out = separate_statements("{##}");
        assert!(out.contains("{##}"), "{out}");
    }

    /// A statement inside a code span still stays verbatim, whether or
    /// not the span closes after it (an unterminated span runs to the
    /// statement's end so the rest is processed normally).
    #[test]
    fn a_statement_between_backticks_stays_verbatim() {
        let out = separate_statements("use `{# c #}` here");
        assert!(out.contains("use `{# c #}` here"), "{out}");

        let out = separate_statements("use `{# c #} tail");
        assert!(out.contains("use `{# c #}"), "{out}");
        assert!(out.contains("tail"), "{out}");
    }

    /// CommonMark basics: headings, emphasis, lists; code blocks; images.
    /// Every constructed element carries [`MARKDOWN_CLASS`].
    #[test]
    fn converts_the_document_basics() {
        let html = to_html("# Title\n\nplain *slanted* and **strong** text\n");
        assert!(html.contains("<h1 class=\"markdown\">Title</h1>"), "{html}");
        assert!(
            html.contains(
                "<p class=\"markdown\">plain <em class=\"markdown\">slanted</em> \
                 and <strong class=\"markdown\">strong</strong> text</p>"
            ),
            "{html}"
        );
        let html = to_html("- one\n- two\n");
        assert!(html.contains("<li class=\"markdown\">one</li>"), "{html}");
        let html = to_html("```\ncode\n```\n");
        assert!(
            html.contains("<pre class=\"markdown\"><code>code\n</code></pre>"),
            "{html}"
        );
    }

    /// The enabled GFM extensions: tables and strikethrough.
    #[test]
    fn converts_tables_and_strikethrough() {
        let html = to_html("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(html.contains("<table class=\"markdown\">"), "{html}");
        assert!(html.contains("<td class=\"markdown\">2</td>"), "{html}");
        let html = to_html("~~gone~~\n");
        assert!(
            html.contains("<del class=\"markdown\">gone</del>"),
            "{html}"
        );
    }

    /// Raw HTML passes through, inline and (with blank lines around it)
    /// block — a Markdown page may embed real UI, unclassed. (The blank
    /// line ends the raw block, so the following Markdown paragraph is
    /// constructed and classed.)
    #[test]
    fn passes_raw_html_through() {
        let html = to_html("before <span class=\"hl\">marked</span> after\n");
        assert!(
            html.contains("before <span class=\"hl\">marked</span> after"),
            "{html}"
        );
        let html =
            to_html("text\n\n<div data-on-click=\"buy\" class=\"action\">Buy</div>\n\ntail\n");
        assert!(
            html.contains("<div data-on-click=\"buy\" class=\"action\">Buy</div>"),
            "{html}"
        );
        assert!(html.contains("<p class=\"markdown\">tail</p>"), "{html}");
    }

    /// Statement separation reads code and lines as CommonMark does
    /// (review_0003 G17/G18): a longer fence closer closes (the statements
    /// after it separate instead of merging into a `<p>`), and a statement
    /// after inline Tera on its line stays inline (it was split off, cutting
    /// the paragraph in two).
    #[test]
    fn separation_follows_fences_and_lines() {
        let html = to_html_template("```\ncode\n````\n{% if a %}\nShown\n{% endif %}\n");
        assert!(
            html.contains("{% if a %}\n<p class=\"markdown\">Shown</p>"),
            "{html}"
        );
        let html = to_html_template("````\n```\n{% if a %}\n````\n");
        assert!(
            html.contains("<code>```\n{{ \"{\" }}% if a %}"),
            "a shorter closer doesn't close: {html}"
        );
        let html = to_html_template("Hello {# c #} {% if a %}\nShown\n{% endif %}\n");
        assert!(
            html.contains("<p class=\"markdown\">Hello {# c #} {% if a %}\nShown</p>"),
            "{html}"
        );
    }

    /// Indentation inside Tera blocks is the author's, not Markdown's
    /// (review_0004 H11): an indented body converts like an unindented one —
    /// it became an indented code block once separation put a blank line
    /// after its statement. Relative indentation inside a body is kept, and
    /// Tera shown inside a fence is code, not structure.
    #[test]
    fn indented_tera_bodies_are_not_code() {
        let html = to_html_template("{% if a %}\n    Some *text*\n{% endif %}\n");
        assert!(
            html.contains(r#"<p class="markdown">Some <em class="markdown">text</em></p>"#),
            "{html}"
        );
        assert!(!html.contains("<pre"), "{html}");
        // Nested blocks line up with their own statements.
        let html = to_html_template(
            "{% for x in xs %}\n    {% if x %}\n        - item {{ x }}\n    {% endif %}\n{% endfor %}\n",
        );
        assert!(
            html.contains(r#"<li class="markdown">item {{ x }}</li>"#),
            "{html}"
        );
        // Each `elif`/`else` segment dedents on its own.
        let html = to_html_template(
            "{% if a %}\n    A\n{% elif b %}\n      B\n{% else %}\n\tC\n{% endif %}\n",
        );
        for letter in ["A", "B", "C"] {
            assert!(
                html.contains(&format!(r#"<p class="markdown">{letter}</p>"#)),
                "{letter}: {html}"
            );
        }
        // Relative indentation inside the body is still Markdown's.
        let html = to_html_template("{% if a %}\n    Example:\n\n        code here\n{% endif %}\n");
        assert!(html.contains("<code>code here\n</code>"), "{html}");
        // A fence's content at column 0 doesn't pin the body's indentation.
        let html = to_html_template("{% if a %}\n    ```\ncode at 0\n    ```\n{% endif %}\n");
        assert!(html.contains("<code>code at 0\n</code>"), "{html}");
        // Tera inside a fence is a sample: untouched, still literal.
        let sample = "```\n{% if a %}\n    sample\n{% endif %}\n```\n";
        assert_eq!(dedent_blocks(sample).0, sample);
        // A raw body dedents, but its statements are text.
        let raw = "{% raw %}\n    {% if a %}\n        x\n    {% endif %}\n{% endraw %}\n";
        assert_eq!(
            dedent_blocks(raw).0,
            "{% raw %}\n{% if a %}\n    x\n{% endif %}\n{% endraw %}\n"
        );
    }

    /// Random token soup of Tera blocks, fences, backticks, tabs and
    /// multi-byte text never panics the dedent or the conversion, and every
    /// mapped code range is an ordered, in-bounds slice of the source on
    /// character boundaries.
    #[test]
    fn dedent_survives_token_soup() {
        const TOKENS: [&str; 23] = [
            "{% if a %}",
            "{% endif %}",
            "{% else %}",
            "{% for x in y %}",
            "{% endfor %}",
            "{% raw %}",
            "{% endraw %}",
            "{% set x %}",
            "{% endset %}",
            "```",
            "~~~",
            "`",
            "    ",
            "\t",
            "\n",
            "\n\n",
            "é x",
            "{{ x }}",
            "<div class=\"a\">",
            "</div>",
            "<pre>",
            "</pre>",
            "<br>",
        ];
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..3000 {
            let length = (next() % 40) as usize;
            let source: String = (0..length)
                .map(|_| TOKENS[(next() % 23) as usize])
                .collect();
            let _ = to_html_template(&source);
            let mut last = 0;
            for range in template_code_ranges(&source) {
                assert!(
                    range.start <= range.end && range.end <= source.len(),
                    "{source:?}"
                );
                assert!(range.start >= last, "unordered: {source:?}");
                assert!(
                    source.is_char_boundary(range.start) && source.is_char_boundary(range.end),
                    "{source:?}"
                );
                last = range.start;
            }
        }
    }

    /// Raw HTML containers dedent like Tera blocks (review_0004 H11
    /// follow-up, ADR 0025): an indented `<div>` body after a blank line was
    /// an indented code block. Containers and Tera blocks nest in any order;
    /// `<pre>`-like elements, void and self-closing tags, and lines holding
    /// more than one tag aren't containers.
    #[test]
    fn indented_html_container_bodies_are_not_code() {
        let html = to_html_template("<div class=\"card\">\n\n    Some **text**\n\n</div>\n");
        assert!(
            html.contains(r#"<p class="markdown">Some <strong class="markdown">text</strong></p>"#),
            "{html}"
        );
        let html = to_html_template(
            "<section>\n\n    {% if a %}\n        <DIV class=\"x\">\n\n            - item\n\n        </div>\n    {% endif %}\n\n</section>\n",
        );
        assert!(html.contains(r#"<li class="markdown">item</li>"#), "{html}");
        assert!(!html.contains("<pre"), "{html}");
        // Whitespace is content in a <pre> (and its lines aren't structure).
        let pre = "<pre>\n    <div>\n        keep\n    </div>\n</pre>\n";
        assert_eq!(dedent_blocks(pre).0, pre);
        // Not containers: void, self-closing, comment, two tags on a line.
        for line in [
            "<br>",
            "<img src=\"a.png\">",
            "<x-icon />",
            "<!-- note -->",
            "<p>hi</p>",
        ] {
            assert!(block_tag(line).is_none(), "{line}");
        }
    }

    /// Offsets map back through the dedent: a code range found in the
    /// dedented text lands on the same characters of the source.
    #[test]
    fn dedent_maps_offsets_back() {
        let source = "{% if a %}\n    text `code` here\n\t\tdeep `x`\n{% endif %}\n";
        let ranges = template_code_ranges(source);
        let texts: Vec<&str> = ranges.iter().map(|range| &source[range.clone()]).collect();
        assert_eq!(texts, ["`code`", "`x`"]);
    }

    /// Content can supply `MARKER` (plain U+0001-delimited text): a class-like
    /// run inside another attribute's value with no closing `"` used to slice
    /// past the end or inside a multi-byte character and panicked the loader
    /// (review_0003 G04). The marker is dropped and the tag stays verbatim.
    #[test]
    fn crafted_markers_cannot_panic_inject_class() {
        let cases = [
            "\u{1}bevy-markup\u{1}<b data-x=' class=\"\u{e9}'>x</b>",
            "\u{1}bevy-markup\u{1}<b data-x=' class=\"'>x</b>",
            "\u{1}bevy-markup\u{1}<b data-x=' class=\"y\">x</b>",
        ];
        for case in cases {
            let html = std::panic::catch_unwind(|| to_html(case)).expect("no panic");
            assert!(
                html.contains("x</b>") || html.contains("</b>"),
                "{case}: {html}"
            );
            // The unclassed tag carries no injected class.
            assert!(
                !html.contains("class=\"markdown markdown\""),
                "{case}: {html}"
            );
        }
    }

    /// Entities survive: escaped text is not re-interpreted as markup
    /// (`&lt;` stays an entity for `tl`/`decode_entities`).
    #[test]
    fn keeps_entities_as_entities() {
        let html = to_html("a &lt;b&gt; c\n");
        assert!(
            html.contains("<p class=\"markdown\">a &lt;b&gt; c</p>"),
            "{html}"
        );
    }

    /// A one-line value gains a `<p>` wrapper: the walk reads fragments
    /// through their content, so this is transparent.
    #[test]
    fn wraps_a_paragraph() {
        // The marker leaves the output mid-line, so pulldown prefixes the
        // first block with a newline.
        assert!(to_html("Hello").contains("<p class=\"markdown\">Hello</p>"));
    }

    /// Every constructed element carries [`MARKDOWN_CLASS`] — including
    /// images (`img.markdown`) and links — while a raw-HTML `<img>` in the
    /// same document passes through unclassed.
    #[test]
    fn constructed_elements_carry_the_class() {
        let html = to_html("![cat](../ui/cat.png)");
        assert!(
            html.contains("<img class=\"markdown\" src=\"../ui/cat.png\" alt=\"cat\" />"),
            "{html}"
        );

        let html = to_html("[text](page.md)");
        assert!(
            html.contains("<a class=\"markdown\" href=\"page.md\">text</a>"),
            "{html}"
        );

        let html = to_html("text\n\n<img class=\"seal\" src=\"cat.png\">\n");
        assert!(
            html.contains("<img class=\"seal\" src=\"cat.png\">"),
            "{html}"
        );
        assert!(
            !html.contains("class=\"markdown markdown\""),
            "no double class: {html}"
        );
    }

    /// A footnote definition already carries a class: `markdown` is
    /// appended to the list, not a second attribute.
    #[test]
    fn appends_to_an_existing_class() {
        let html = to_html("text[^1]\n\n[^1]: the note\n");
        assert!(
            html.contains("class=\"footnote-definition markdown\""),
            "{html}"
        );
    }

    /// ADR 0020: a template's Tera syntax survives verbatim — a
    /// pure-Tera paragraph passes unwrapped (no `<p>`, no class), inline
    /// Tera inside text passes raw while the surrounding Markdown still
    /// constructs and classes, and code is Tera-literal. Tera statements
    /// sit on their own lines (like after a Tera expansion), so they never
    /// shape Markdown structure.
    #[test]
    fn template_mode_passes_tera_through() {
        // A pure-Tera paragraph (Tera lines and the whitespace between
        // them): no wrapper, no class, raw syntax. Plain text between
        // statements becomes its own paragraph, as after a Tera expansion.
        let html = to_html_template("{% if score > 3 %}\n{% endif %}\n");
        assert!(!html.contains("<p"), "PURE = {html:?}");
        assert!(html.contains("{% if score > 3 %}"), "{html}");
        assert!(html.contains("{% endif %}"), "{html}");
        let html = to_html_template("{% if score > 3 %}\nshow it\n{% endif %}\n");
        assert!(html.contains("{% if score > 3 %}"), "MIXED = {html:?}");
        assert!(
            html.contains("<p class=\"markdown\">show it</p>"),
            "MIXED = {html:?}"
        );
        assert!(html.contains("{% endif %}"), "MIXED = {html:?}");

        // A block-level loop: the wrapper-free Tera lines bracket the
        // constructed Markdown.
        let html =
            to_html_template("{% for i in range(end=2) %}\n## Section {{ i }}\n{% endfor %}\n");
        assert!(html.contains("{% for i in range(end=2) %}"), "{html}");
        assert!(html.contains("{% endfor %}"), "{html}");
        assert!(
            html.contains("<h2 class=\"markdown\">Section {{ i }}</h2>"),
            "{html}"
        );

        // Inline Tera inside text: raw alongside constructed emphasis.
        let html = to_html_template("hello {{ name }}, *slanted*\n");
        assert!(
            html.contains("hello {{ name }}, <em class=\"markdown\">slanted</em>"),
            "INLINE = {html:?}"
        );

        // Code stays Tera-literal: a code span's `{{ x }}` is escaped
        // text, not passed through.
        let html = to_html_template("a `{{ x }}` span\n");
        assert!(html.contains("<code>"), "{html}");
        assert!(
            !html.contains("<p class=\"markdown\">{{ x }}</p>"),
            "{html}"
        );

        // The `to_html` path (values, not templates) converts Tera
        // looking text as text, as before.
        let html = to_html("hello {{ name }}\n");
        assert!(html.contains("hello {{ name }}"), "{html}");
        assert!(html.contains("<p class=\"markdown\">"), "{html}");
    }

    /// bug_0178 (UPSTREAM.md U13): pulldown-cmark 0.13.4's offset iterator
    /// (`code_ranges`, `dedent_blocks`) panicked on a list item holding
    /// only a link reference definition followed by a whitespace-only line
    /// (`\u{0B}`); fixed upstream in 0.13.5 (pulldown-cmark#1021). A `.md`
    /// template is user content: its conversion must never panic.
    #[test]
    fn a_refdef_only_list_item_before_a_blank_line_does_not_panic() {
        for source in [
            "- [a]: b\n\u{0B}",
            "+\t[<]:\u{FFFD}\n\u{0B}",
            "* [a]: b\n\u{0C}",
        ] {
            let html = to_html_template(source);
            assert!(
                !html.contains("[a]: b"),
                "the definition is not text: {html}"
            );
        }
    }
}

/// Tera in a template (ADR 0020): a paragraph of only statements
/// passes through unwrapped; inline statements (inside a line, an
/// attribute value, a list item, a heading) stay inline and render
/// where they are; a code span/block is Tera-literal; and a raw
/// `{% raw %}` example in text doesn't break the template.
#[test]
fn tera_templates_survive_the_conversion() {
    // A pure-Tera paragraph: unwrapped.
    let html = to_html_template("{% if hd %}<p>hd</p>{% endif %}\n");
    assert!(html.contains("<p>hd</p>"), "{html}");
    // An inline statement inside an attribute value: no newlines.
    let html = to_html_template("<img src=\"{% if hd %}hd{% else %}sd{% endif %}.png\">\n");
    assert!(
        html.contains("src=\"{% if hd %}hd{% else %}sd{% endif %}.png\""),
        "{html}"
    );
    // A statement inside a list item, a heading and a table cell.
    let html = to_html_template("# Hello {% if a %}world{% endif %}\n");
    assert!(
        html.contains("<h1 class=\"markdown\">Hello {% if a %}world{% endif %}</h1>"),
        "{html}"
    );
    let html = to_html_template("* {% if a %}one{% endif %} item\n");
    assert!(
        html.contains("<li class=\"markdown\">{% if a %}one{% endif %} item</li>"),
        "{html}"
    );
    let html = to_html_template("| a | b |\n|---|---|\n| {% if a %}1{% endif %} | 2 |\n");
    assert!(html.contains("{% if a %}1{% endif %}"), "{html}");
    // A code span and a fenced block are Tera-literal: the converted
    // template contains no executable Tera from the code (`code_is_
    // teraliteral_when_rendered` pins the rendered result).
    let html = to_html_template("a `{{ x }}` span\n");
    assert!(html.contains("<code>"), "{html}");
    assert!(
        !html.contains("<code>{{ x }}</code>"),
        "the code's opener must not survive the conversion: {html}"
    );
    let html = to_html_template("```\nlet s = \"{%\"; // %}\n```\n");
    assert!(
        !html.contains("let s = \"{%\";"),
        "the fenced code's opener must not survive the conversion: {html}"
    );
    // A literal `{% raw %}` in text: Tera-literal via the code span...
    let html = to_html_template("text `{% raw %}` tail\n");
    assert!(
        !html.contains("<code>{% raw %}</code>"),
        "the raw opener must not survive the conversion: {html}"
    );
}

/// Code content renders verbatim (ADR 0020; R34): Tera never sees an
/// opener inside a code span or block, so `{{ x }}` stays `{{ x }}`
/// (not the variable), a fenced `{%`/`endraw`/unbalanced string is
/// literal, and prose Tera still executes.
#[test]
fn code_is_teraliteral_when_rendered() {
    let render = |source: &str, json: &str| -> String {
        let html = to_html_template(source);
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut tera = tera::Tera::default();
        tera.add_raw_template("md", &html).unwrap();
        let context = tera::Context::from_serialize(&value).unwrap();
        tera.render("md", &context).unwrap()
    };

    let html = render("a `{{ x }}` span\n", r#"{"x":"VALUE"}"#);
    assert!(html.contains("<code>{{ x }}</code>"), "{html}");
    assert!(!html.contains("VALUE"), "{html}");

    let html = render("```\nlet s = \"{%\"; // %}\n{% endraw %}\n```\n", "{}");
    assert!(html.contains("let s = \"{%\"; // %}"), "{html}");
    assert!(html.contains("{% endraw %}"), "{html}");

    let html = render("text `{% raw %}` tail\n", "{}");
    assert!(html.contains("<code>{% raw %}</code>"), "{html}");

    // Inside a raw block Tera passes content through verbatim: the
    // code is NOT Tera-literalized (the escape braces would show as
    // `{{ "{" }}` garbage — review_0003 G07); Tera's raw ends even
    // inside a code fence (its scan is textual), so the code after
    // the endraw is literalized again.
    let html = render(
        "{% raw %}\n\n```\n{{ x }}\n```\n\n{% endraw %}\n\n`{{ x }}`\n",
        r#"{"x":"VALUE"}"#,
    );
    assert!(html.contains("<code>{{ x }}</code>"), "{html}");
    assert!(!html.contains("{{ \"\""), "{html}");
    assert!(!html.contains("VALUE"), "{html}");
    assert_eq!(html.matches("{{ x }}").count(), 2, "{html}");

    // Prose Tera still executes; only code is literal.
    let html = render("hello {{ name }}, code `{{ name }}`\n", r#"{"name":"Ada"}"#);
    assert!(html.contains("hello Ada"), "{html}");
    assert!(html.contains("<code>{{ name }}</code>"), "{html}");
}
