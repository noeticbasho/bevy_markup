//! usvg's own object bounding boxes, measured before usvg uses them
//! (ADR 0042, bug_0197, UPSTREAM U31).
//!
//! usvg 0.49 maps every `objectBoundingBox` region onto an element's box
//! with tiny-skia-path's `NonZeroRect::bbox_transform`, which unwraps the
//! product: a filter region (−10%, 120%), a mask region, a pattern tile,
//! an `feImage` or primitive subregion, a CSS filter function's region. A
//! finite box whose product overflows `f32` panics, in every build. The
//! box comes from the geometry and every transform below the element
//! (`scale(1e20)` in a filtered group grows it without any big number), so
//! the only faithful source is usvg's own conversion.
//!
//! [`largest_box`] converts a copy of the document with nothing that calls
//! `bbox_transform`, and reads the boxes usvg computed:
//!
//! - every element's CSS is inlined into its `style` attribute, in the
//!   order usvg applies it (`<style>` rules by specificity, then the
//!   element's own `style`), so renaming elements can't change what a
//!   selector matches;
//! - each `style` starts with `filter`, `mask` and `clip-path` set to
//!   `none`, `fill` to black and `stroke` to `none`, all `!important`:
//!   usvg keeps the first important declaration, so these win over the
//!   document's. No filter, mask or pattern is converted, and every shape
//!   stays visible (a fill naming a pattern that isn't converted would
//!   otherwise drop it);
//! - `defs`, `mask`, `clipPath` and `pattern` become `g`, so their content
//!   (which can carry filters of its own) is converted in place. A group's
//!   box doesn't depend on where its content is used: transforms above it
//!   don't count.
//!
//! The copy converts a superset of the real document's elements, so the
//! largest box is at least the largest the real conversion computes.
//! Groups usvg doesn't keep (no transform, no filter left) fold into their
//! parent's box, in the same coordinates.

use resvg::usvg;
use usvg::roxmltree;

use crate::svg::SvgError;
use crate::svg_bounds::XmlNode;

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Declarations that win over the document's (usvg keeps the first
/// `!important` one it sees).
const NEUTRAL: &str = "filter:none!important;mask:none!important;clip-path:none!important;\
                       fill:#000!important;stroke:none!important";

/// The largest coordinate magnitude of any object bounding box usvg
/// computes for `document` (0 for a document without boxes).
#[hotpath::measure]
pub(crate) fn largest_box(
    document: &roxmltree::Document,
    options: &usvg::Options,
) -> Result<f64, SvgError> {
    let copy = rewrite(document);
    let xml_options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    let parsed = roxmltree::Document::parse_with_options(&copy, xml_options)
        .map_err(|err| SvgError::Parse(usvg::Error::ParsingFailed(err)))?;
    let tree = usvg::Tree::from_xmltree(&parsed, options).map_err(SvgError::Parse)?;
    let mut largest = 0.0f64;
    group_boxes(tree.root(), &mut largest);
    Ok(largest)
}

/// Folds every group's box (and those of the groups below it, and of the
/// sub-trees usvg hangs off its nodes) into `largest`.
fn group_boxes(group: &usvg::Group, largest: &mut f64) {
    let rect = group.bounding_box();
    for side in [rect.left(), rect.top(), rect.right(), rect.bottom()] {
        *largest = largest.max(f64::from(side).abs());
    }
    for node in group.children() {
        if let usvg::Node::Group(child) = node {
            group_boxes(child, largest);
        }
        node.subroots(|subroot| group_boxes(subroot, largest));
    }
}

/// The document as described in the module docs: SVG elements only (usvg
/// skips other namespaces with their subtrees), CSS inlined, no DTD
/// (entities are already expanded in `document`).
fn rewrite(document: &roxmltree::Document) -> String {
    let mut sheet = simplecss::StyleSheet::new();
    for node in document
        .descendants()
        .filter(|node| node.has_tag_name("style"))
    {
        match node.attribute("type") {
            Some("text/css") | None => {}
            Some(_) => continue,
        }
        if let Some(text) = node.text() {
            sheet.parse_more(text);
        }
    }

    let mut out = String::with_capacity(document.input_text().len() * 2);
    // Iterative: open each node, and close it once its children are out.
    let mut stack = vec![(document.root_element(), false)];
    while let Some((node, opened)) = stack.pop() {
        if node.is_text() {
            escape(&mut out, node.text().unwrap_or(""), false);
            continue;
        }
        if opened {
            out.push_str("</");
            out.push_str(name(node));
            out.push('>');
            continue;
        }
        // usvg skips other namespaces with their subtrees; the `<style>`
        // rules are inlined.
        if node.tag_name().namespace() != Some(SVG_NS) || node.tag_name().name() == "style" {
            continue;
        }
        open(&mut out, node, &sheet);
        stack.push((node, true));
        let children: Vec<_> = node
            .children()
            .filter(|child| child.is_element() || child.is_text())
            .collect();
        stack.extend(children.into_iter().rev().map(|child| (child, false)));
    }
    out
}

/// The tag usvg converts `node` as in the copy.
fn name<'a>(node: roxmltree::Node<'a, '_>) -> &'a str {
    match node.tag_name().name() {
        "defs" | "mask" | "clipPath" | "pattern" => "g",
        other => other,
    }
}

fn open(out: &mut String, node: roxmltree::Node, sheet: &simplecss::StyleSheet) {
    out.push('<');
    out.push_str(name(node));
    if node.parent().is_some_and(|parent| parent.is_root()) {
        out.push_str(" xmlns=\"");
        out.push_str(SVG_NS);
        out.push_str("\" xmlns:xlink=\"");
        out.push_str(XLINK_NS);
        out.push('"');
    }
    let mut written: Vec<String> = Vec::new();
    for attribute in node.attributes() {
        let qualified = match attribute.namespace() {
            None | Some(SVG_NS) => attribute.name().to_string(),
            Some(XLINK_NS) => format!("xlink:{}", attribute.name()),
            Some(XML_NS) => format!("xml:{}", attribute.name()),
            Some(_) => continue,
        };
        // Composed below; a name twice would be malformed XML.
        if qualified == "style" || written.contains(&qualified) {
            continue;
        }
        out.push(' ');
        out.push_str(&qualified);
        out.push_str("=\"");
        escape(out, attribute.value(), true);
        out.push('"');
        written.push(qualified);
    }

    let mut style = String::from(NEUTRAL);
    if name(node) != node.tag_name().name() {
        // A renamed element's content must be converted even where the
        // original element wouldn't be displayed itself.
        style.push_str(";display:inline!important");
    }
    let element = XmlNode(node);
    for rule in &sheet.rules {
        if rule.selector.matches(&element) {
            for declaration in &rule.declarations {
                style.push(';');
                style.push_str(declaration.name);
                style.push(':');
                style.push_str(declaration.value);
                if declaration.important {
                    style.push_str("!important");
                }
            }
        }
    }
    if let Some(own) = node.attribute("style") {
        style.push(';');
        style.push_str(own);
    }
    out.push_str(" style=\"");
    escape(out, &style, true);
    out.push_str("\">");
}

/// XML-escapes `text`; in an attribute, also the characters attribute
/// value normalization would turn into spaces.
fn escape(out: &mut String, text: &str, attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\t' if attribute => out.push_str("&#9;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c => out.push(c),
        }
    }
}
