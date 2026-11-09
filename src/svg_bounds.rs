//! What usvg 0.49 will do with an SVG document, bounded from its XML before
//! usvg runs (ADR 0038 §5, ADR 0040): how deeply its converter recurses
//! (`depth`, against a stack overflow) and how much it builds (`work`,
//! against a conversion that takes seconds or never ends).
//!
//! Both come from one graph over the document's elements. Its edges are
//! parent → child, element → element it references (`url(#id)`, `#id`
//! hrefs, in attributes, `style` and matched `<style>` rules), and element →
//! paint server or marker it *inherits* (`fill`, `stroke`, `marker-*` set on
//! an ancestor or a `<use>` it is copied under). usvg converts a reference's
//! target once per reference (`<use>` content is copied, masks/clip paths/
//! filters with `objectBoundingBox` units are converted per element, an
//! `objectBoundingBox` pattern is cloned per shape, a gradient's stops are
//! copied per shape), a marker once per vertex of each path that carries it,
//! and splits each elliptical arc into cubics by its radius (kurbo, at an
//! absolute tolerance of 0.1 user units): an element's count of instances is
//! the number of paths to it from the root, each edge weighted by how many
//! times it instantiates its target. The work is that count times the
//! element's own cost (its conversion, its path segments and its text),
//! summed. It over-approximates (a cached gradient counts per use, an
//! inherited `fill` counts even where the shape overrides it) and never
//! misses a reference usvg follows.

use std::collections::HashMap;

use resvg::usvg;
use usvg::roxmltree;

use crate::svg::SvgError;

/// The deepest recursion [`bounds`] lets through, in its units of roughly 3
/// KB of debug-build stack (about 1 KB in release): at most ~800 KB, inside
/// the 2 MB of the threads Bevy loads assets on, with room for the caller's
/// frames. Measured per construct in the probe report §6;
/// `accepted_depths_fit_a_two_megabyte_stack` checks it.
pub(crate) const MAX_DEPTH: u32 = 256;
/// Weight of one nested element (usvg's converter: ~3.0 KB per `<g>` level
/// in debug).
const DEPTH_ELEMENT: u32 = 1;
/// Weight of one nested `<svg>` (a new viewport: ~5.7 KB in debug).
const DEPTH_SVG: u32 = 2;
/// Weight of following one reference into its target (paint servers,
/// masks, clip paths, markers, filters, `<use>`, `feImage`: up to ~18.5 KB
/// per hop in debug, counting the elements the hop passes through).
const DEPTH_REFERENCE: u32 = 6;
/// More reference edges than this (CSS rules × the elements they match,
/// plus attributes, plus inherited paint servers and markers × the shapes
/// inheriting them) fail the load: the graph's memory stays bounded.
pub(crate) const MAX_REFERENCES: usize = 1 << 20;

/// Work of converting one element instance (a group, a shape):
/// ~0.86 µs in release (a `<use>` fan-out of 10⁵ rects: 0.69 s). One unit
/// of work is ~60 ns (ADR 0040's calibration).
const ELEMENT_WORK: u64 = 16;
/// Work of one path segment usvg builds (and bounds, transforms, copies):
/// ~0.24 µs for a parsed line, ~0.08 µs for a circle's cubic.
const SEGMENT_WORK: u64 = 4;
/// Work per (cubics of one path arc)²: svgtypes splits an arc into a buffer
/// of all its cubics, then hands them out with `Vec::remove(0)`, each
/// moving the rest (UPSTREAM U22): ~0.18 ns per square, a fifth of this.
const ARC_SQUARE_WORK: f64 = 1.0 / 64.0;
/// Work of one gradient stop instance (~2 ns in release when an inherited
/// gradient's stops are copied per shape; one unit, generously).
const STOP_WORK: u64 = 1;
/// Work per byte of text content: as much as an element, for shaping with
/// `svg-text` (without it usvg skips text).
const TEXT_WORK: u64 = 16;
/// usvg's default `font-size` (`Options::font_size`), the base of `em`.
const DEFAULT_FONT_SIZE: f64 = 12.0;
/// usvg's viewport when the root has no size: 100 × 100.
const DEFAULT_VIEWPORT: f64 = 100.0;
/// usvg's resolution for absolute units (`Options::dpi`).
const DPI: f64 = 96.0;

/// What [`bounds`] found.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bounds {
    /// The longest weighted path through the graph, in units of
    /// [`DEPTH_ELEMENT`].
    pub depth: u32,
    /// The conversion's work, in units of ~60 ns of release-build conversion
    /// (an element instance is [`ELEMENT_WORK`]); saturating.
    pub work: u64,
}

/// Bounds the recursion depth and the work of usvg's conversion of
/// `document`, in time linear in its elements and references. Iterative
/// throughout (Kahn's algorithm), so the check itself can't overflow. A
/// reference cycle (direct, or through an inherited paint server or
/// marker) is an error: usvg expands some cycles until its depth limit,
/// past the stack, and recurses through others without end.
#[hotpath::measure]
pub(crate) fn bounds(
    document: &roxmltree::Document,
    max_references: usize,
) -> Result<Bounds, SvgError> {
    check_values(document)?;
    let mut graph = Graph::new(document, max_references)?;
    let order = graph.order().ok_or(SvgError::ReferenceCycle)?;
    graph.inherit(&order, max_references)?;
    graph.measure()
}

/// Refuses values usvg would panic on while converting. Only an element's
/// own attributes count: neither is a presentation attribute, so CSS can't
/// set them.
///
/// - `feTurbulence`'s `baseFrequency` (UPSTREAM U26): usvg reads it as an
///   `f32` list, where a number past `f32::MAX` becomes infinity, checks
///   only its sign and unwraps `PositiveF32::new`, which is `None` for
///   infinity (release builds panic too). Any non-finite number refuses,
///   also in a list usvg would ignore (wrong length, a bad entry): an
///   over-refusal of documents nobody writes.
/// - `feConvolveMatrix`'s `order` (UPSTREAM U29): usvg multiplies the
///   columns and rows as `u32` unchecked, which panics in debug builds and
///   wraps in release (where a wrapped kernel is then refused by the cost
///   estimate, which multiplies in `f64`). Read as usvg reads it: the
///   first number saturated to `i32`, the second defaulting to the first;
///   a non-positive side keeps usvg's 3 × 3.
fn check_values(document: &roxmltree::Document) -> Result<(), SvgError> {
    let refuse = |element, attribute, value: &str, reason| SvgError::OutOfRange {
        element,
        attribute,
        value: value.chars().take(64).collect(),
        reason,
    };
    for element in document.descendants().filter(|node| node.is_element()) {
        match element.tag_name().name() {
            "feTurbulence" => {
                let Some(value) = element.attribute("baseFrequency") else {
                    continue;
                };
                let finite = svgtypes::NumberListParser::from(value)
                    .all(|number| number.map_or(true, |number| (number as f32).is_finite()));
                if !finite {
                    return Err(refuse(
                        "feTurbulence",
                        "baseFrequency",
                        value,
                        "its numbers must be finite",
                    ));
                }
            }
            "feConvolveMatrix" => {
                let Some(value) = element.attribute("order") else {
                    continue;
                };
                let mut numbers = svgtypes::NumberListParser::from(value);
                let columns = numbers.next().and_then(Result::ok).map_or(3, |n| n as i32);
                let rows = numbers
                    .next()
                    .and_then(Result::ok)
                    .map_or(columns, |n| n as i32);
                if columns > 0 && rows > 0 && (columns as u32).checked_mul(rows as u32).is_none() {
                    return Err(refuse(
                        "feConvolveMatrix",
                        "order",
                        value,
                        "its columns × rows must fit in 32 bits",
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// What an element is, for the work model.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Path,
    Polyline,
    Line,
    Rect,
    Circle,
    Ellipse,
    /// `text`, `tspan`, `textPath`, `tref`: paint is applied per span.
    Text,
    /// A target instantiated per vertex of each path carrying it.
    Marker,
    /// A target instantiated per shape whose `fill`/`stroke` names it.
    PaintServer,
    /// Starts a viewport: `%` lengths below resolve against it.
    Viewport,
    /// A gradient stop: copied per shape the gradient paints, which is a
    /// copy of a small struct, not a conversion.
    Stop,
    Use,
    Other,
}

impl Kind {
    fn of(element: &roxmltree::Node) -> Kind {
        match element.tag_name().name() {
            "path" => Kind::Path,
            "polyline" | "polygon" => Kind::Polyline,
            "line" => Kind::Line,
            "rect" => Kind::Rect,
            "circle" => Kind::Circle,
            "ellipse" => Kind::Ellipse,
            "text" | "tspan" | "textPath" | "tref" => Kind::Text,
            "marker" => Kind::Marker,
            "linearGradient" | "radialGradient" | "pattern" => Kind::PaintServer,
            "svg" | "symbol" => Kind::Viewport,
            "stop" => Kind::Stop,
            "use" => Kind::Use,
            _ => Kind::Other,
        }
    }

    fn is_shape(self) -> bool {
        matches!(
            self,
            Kind::Path | Kind::Polyline | Kind::Line | Kind::Rect | Kind::Circle | Kind::Ellipse
        )
    }
}

#[derive(Clone, Copy)]
struct Edge {
    to: u32,
    /// Depth added by following it.
    depth: u32,
    /// Instances of the target per instance of the source.
    times: u64,
    /// The target inherits the source's properties: a child, or a `<use>`'s
    /// referenced content (usvg copies it under the `<use>`).
    inherits: bool,
}

struct Graph<'a, 'input> {
    elements: Vec<roxmltree::Node<'a, 'input>>,
    kinds: Vec<Kind>,
    /// Every declaration that applies to the element: its attributes, its
    /// `style` attribute's declarations, the matched `<style>` rules'.
    declarations: Vec<Vec<(&'a str, &'a str)>>,
    /// The paint servers and markers the element names itself.
    own_targets: Vec<Vec<u32>>,
    edges: Vec<Vec<Edge>>,
    edge_count: usize,
    /// Per element, filled by [`Graph::inherit`]: the work of one instance.
    work: Vec<u64>,
}

impl<'a, 'input> Graph<'a, 'input> {
    fn new(
        document: &'a roxmltree::Document<'input>,
        max_references: usize,
    ) -> Result<Self, SvgError> {
        let elements: Vec<roxmltree::Node> = document
            .descendants()
            .filter(|node| node.is_element())
            .collect();
        let mut index = vec![
            u32::MAX;
            document
                .descendants()
                .map(|node| node.id().get_usize() + 1)
                .max()
                .unwrap_or(0)
        ];
        for (i, element) in elements.iter().enumerate() {
            index[element.id().get_usize()] = i as u32;
        }
        let mut ids: HashMap<&str, Vec<u32>> = HashMap::new();
        for (i, element) in elements.iter().enumerate() {
            if let Some(id) = element.attribute("id") {
                ids.entry(id).or_default().push(i as u32);
            }
        }
        // The rules that can attach a reference or a length the model
        // reads, as usvg collects them (`svgtree/parse.rs` `resolve_css`).
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
        let rules: Vec<(&simplecss::Selector, Vec<(&str, &str)>)> = sheet
            .rules
            .iter()
            .filter_map(|rule| {
                let declarations: Vec<(&str, &str)> = rule
                    .declarations
                    .iter()
                    .filter(|declaration| {
                        is_length_property(declaration.name) || has_reference(declaration.value)
                    })
                    .map(|declaration| (declaration.name, declaration.value))
                    .collect();
                (!declarations.is_empty()).then_some((&rule.selector, declarations))
            })
            .collect();

        let kinds: Vec<Kind> = elements.iter().map(Kind::of).collect();
        let mut graph = Graph {
            declarations: vec![Vec::new(); elements.len()],
            own_targets: vec![Vec::new(); elements.len()],
            edges: vec![Vec::new(); elements.len()],
            work: vec![0; elements.len()],
            edge_count: 0,
            kinds,
            elements,
        };
        let mut targets = Vec::new();
        for i in 0..graph.elements.len() {
            let element = graph.elements[i];
            let mut declarations: Vec<(&'a str, &'a str)> = Vec::new();
            for attribute in element.attributes() {
                declarations.push((attribute.name(), attribute.value()));
                if attribute.name() == "style" {
                    for declaration in simplecss::DeclarationTokenizer::from(attribute.value()) {
                        declarations.push((declaration.name, declaration.value));
                    }
                }
            }
            for (selector, rule_declarations) in &rules {
                if selector.matches(&XmlNode(element)) {
                    declarations.extend(rule_declarations.iter().copied());
                }
            }

            for child in element.children().filter(|child| child.is_element()) {
                let to = index[child.id().get_usize()];
                graph.edges[i].push(Edge {
                    to,
                    depth: depth_weight(graph.kinds[to as usize], &child),
                    times: 1,
                    inherits: true,
                });
            }
            targets.clear();
            let mut hrefs = Vec::new();
            for &(name, value) in &declarations {
                let before = targets.len();
                reference_targets(value, &mut targets);
                if graph.kinds[i] == Kind::Use && name == "href" {
                    hrefs.extend_from_slice(&targets[before..]);
                }
            }
            targets.sort_unstable();
            targets.dedup();
            for target in &targets {
                for &to in ids.get(target).into_iter().flatten() {
                    let kind = graph.kinds[to as usize];
                    graph.edges[i].push(Edge {
                        to,
                        depth: DEPTH_REFERENCE + depth_weight(kind, &graph.elements[to as usize]),
                        times: 1,
                        inherits: hrefs.contains(target),
                    });
                    if matches!(kind, Kind::Marker | Kind::PaintServer) {
                        graph.own_targets[i].push(to);
                    }
                }
            }
            graph.own_targets[i].sort_unstable();
            graph.own_targets[i].dedup();
            graph.edge_count += graph.edges[i].len() + declarations.len();
            if graph.edge_count > max_references {
                return Err(SvgError::TooManyReferences {
                    max: max_references,
                });
            }
            graph.declarations[i] = declarations;
        }
        Ok(graph)
    }

    /// A topological order (Kahn's algorithm), or `None` on a cycle.
    fn order(&self) -> Option<Vec<u32>> {
        let mut incoming = vec![0u32; self.elements.len()];
        for list in &self.edges {
            for edge in list {
                incoming[edge.to as usize] += 1;
            }
        }
        let mut ready: Vec<u32> = (0..self.elements.len() as u32)
            .filter(|&i| incoming[i as usize] == 0)
            .collect();
        let mut order = Vec::with_capacity(self.elements.len());
        while let Some(from) = ready.pop() {
            order.push(from);
            for edge in &self.edges[from as usize] {
                let to = edge.to as usize;
                incoming[to] -= 1;
                if incoming[to] == 0 {
                    ready.push(to as u32);
                }
            }
        }
        (order.len() == self.elements.len()).then_some(order)
    }

    /// Walks `order` (parents and `<use>`s before what inherits from them),
    /// carrying the inherited `font-size`, viewport size and paint
    /// servers/markers down; fills each element's own work and adds the
    /// edges from shapes and text to what they inherit.
    ///
    /// A marker is converted again per vertex, its references included: its
    /// edge carries that count. A paint server is converted once and
    /// cached; a shape it paints clones its content shallowly (nested paint
    /// servers stay shared `Arc`s): its edge carries no instances (it is
    /// there for the depth and the cycle check), and the shape's own work
    /// gains the server's content, as many times as it has paints.
    fn inherit(&mut self, order: &[u32], max_references: usize) -> Result<(), SvgError> {
        let len = self.elements.len();
        let mut font = vec![DEFAULT_FONT_SIZE; len];
        let mut view = vec![DEFAULT_VIEWPORT; len];
        let mut inherited: Vec<Vec<u32>> = vec![Vec::new(); len];
        let mut painted: Vec<(u32, u32)> = Vec::new();
        let mut entries = 0usize;
        for &i in order {
            let i = i as usize;
            let declarations = &self.declarations[i];
            let parent_font = font[i];
            for &(name, value) in declarations {
                let size = match name {
                    "font-size" => font_size(value, parent_font),
                    "font" => svgtypes::FontShorthand::from_str(value)
                        .map_or(0.0, |font| font_size(font.font_size, parent_font)),
                    _ => continue,
                };
                font[i] = font[i].max(size);
            }
            if self.kinds[i] == Kind::Viewport {
                for &(name, value) in declarations {
                    let size = match name {
                        "viewBox" => value
                            .parse::<svgtypes::ViewBox>()
                            .map_or(0.0, |vb| vb.w.abs().max(vb.h.abs())),
                        "width" | "height" => length(value, font[i], view[i]),
                        _ => continue,
                    };
                    view[i] = view[i].max(size);
                }
            }

            let mut context = std::mem::take(&mut inherited[i]);
            context.extend_from_slice(&self.own_targets[i]);
            context.sort_unstable();
            context.dedup();

            let kind = self.kinds[i];
            let (segments, own) = self.shape_work(i, font[i], view[i]);
            self.work[i] = own.saturating_add(self.text_work(i));
            if kind.is_shape() || kind == Kind::Text {
                for &to in &context {
                    let target = self.kinds[to as usize];
                    let times = match target {
                        // Start, every vertex, end.
                        Kind::Marker if kind.is_shape() => segments.saturating_add(2),
                        Kind::Marker => continue,
                        _ => {
                            painted.push((i as u32, to));
                            0
                        }
                    };
                    self.edges[i].push(Edge {
                        to,
                        depth: DEPTH_REFERENCE + depth_weight(target, &self.elements[to as usize]),
                        times,
                        inherits: false,
                    });
                    self.edge_count += 1;
                }
            }
            for edge in &self.edges[i] {
                if !edge.inherits {
                    continue;
                }
                let to = edge.to as usize;
                font[to] = font[to].max(font[i]);
                view[to] = view[to].max(view[i]);
                if !context.is_empty() {
                    let into = &mut inherited[to];
                    entries += context.len();
                    into.extend_from_slice(&context);
                    into.sort_unstable();
                    into.dedup();
                }
            }
            if self.edge_count + entries > max_references {
                return Err(SvgError::TooManyReferences {
                    max: max_references,
                });
            }
        }
        // A paint server's content: its subtree through children and
        // `<use>` copies (what a clone copies), deepest first.
        let mut content = self.work.clone();
        for &i in order.iter().rev() {
            let i = i as usize;
            for edge in &self.edges[i] {
                if edge.inherits {
                    content[i] = content[i].saturating_add(content[edge.to as usize]);
                }
            }
        }
        for (shape, server) in painted {
            // Fill and stroke.
            let clones = content[server as usize].saturating_mul(2);
            self.work[shape as usize] = self.work[shape as usize].saturating_add(clones);
        }
        Ok(())
    }

    /// The path segments of shape `i` (0 for anything else), and the work
    /// of one instance of it.
    fn shape_work(&self, i: usize, font: f64, view: f64) -> (u64, u64) {
        let element = &self.elements[i];
        let largest = |names: &[&str]| {
            self.declarations[i]
                .iter()
                .filter(|(name, _)| names.contains(name))
                .map(|&(_, value)| length(value, font, view))
                .fold(0.0, f64::max)
        };
        let (segments, squares) = match self.kinds[i] {
            Kind::Path => element.attribute("d").map_or((0.0, 0.0), path_segments),
            Kind::Polyline => {
                let points = element
                    .attribute("points")
                    .map_or(0, |points| svgtypes::PointsParser::from(points).count());
                (points as f64 + 1.0, 0.0)
            }
            Kind::Line => (2.0, 0.0),
            // Four quarter arcs, built with tiny-skia's `PathBuilder` (no
            // svgtypes buffer): linear in the curves.
            Kind::Rect => (5.0 + 4.0 * quarter_arc(largest(&["rx", "ry"])), 0.0),
            Kind::Circle => (4.0 * quarter_arc(largest(&["r"])), 0.0),
            Kind::Ellipse => (4.0 * quarter_arc(largest(&["rx", "ry"])), 0.0),
            _ => (0.0, 0.0),
        };
        // `as` saturates (and maps NaN to 0, which the arcs never produce:
        // `quarter_arc` and `arc_curves` turn NaN into infinity).
        let segments = segments.ceil() as u64;
        if self.kinds[i] == Kind::Stop {
            return (0, STOP_WORK);
        }
        let work = ELEMENT_WORK
            .saturating_add(segments.saturating_mul(SEGMENT_WORK))
            .saturating_add((squares * ARC_SQUARE_WORK).ceil() as u64);
        (segments, work)
    }

    fn text_work(&self, i: usize) -> u64 {
        self.elements[i]
            .children()
            .filter(|child| child.is_text())
            .map(|child| child.text().map_or(0, str::len) as u64)
            .sum::<u64>()
            .saturating_mul(TEXT_WORK)
    }

    /// The longest weighted path and the total work, over a topological
    /// order of the graph with the inherited edges.
    fn measure(&self) -> Result<Bounds, SvgError> {
        let len = self.elements.len();
        let mut incoming = vec![0u32; len];
        for list in &self.edges {
            for edge in list {
                incoming[edge.to as usize] += 1;
            }
        }
        let mut depth: Vec<u32> = self
            .elements
            .iter()
            .zip(&self.kinds)
            .map(|(element, &kind)| depth_weight(kind, element))
            .collect();
        let mut instances = vec![0u64; len];
        let mut ready: Vec<u32> = (0..len as u32)
            .filter(|&i| incoming[i as usize] == 0)
            .collect();
        for &root in &ready {
            instances[root as usize] = 1;
        }
        let mut done = 0usize;
        let mut deepest = 0;
        let mut work = 0u64;
        while let Some(from) = ready.pop() {
            let from = from as usize;
            done += 1;
            deepest = deepest.max(depth[from]);
            work = work.saturating_add(instances[from].saturating_mul(self.work[from]));
            for edge in &self.edges[from] {
                let to = edge.to as usize;
                depth[to] = depth[to].max(depth[from].saturating_add(edge.depth));
                instances[to] =
                    instances[to].saturating_add(instances[from].saturating_mul(edge.times));
                incoming[to] -= 1;
                if incoming[to] == 0 {
                    ready.push(to as u32);
                }
            }
        }
        if done < len {
            return Err(SvgError::ReferenceCycle);
        }
        Ok(Bounds {
            depth: deepest,
            work,
        })
    }
}

fn depth_weight(kind: Kind, element: &roxmltree::Node) -> u32 {
    if kind == Kind::Viewport && element.has_tag_name("svg") {
        DEPTH_SVG
    } else {
        DEPTH_ELEMENT
    }
}

/// Properties whose lengths the model reads (radii, `em` and `%` bases).
fn is_length_property(name: &str) -> bool {
    matches!(
        name,
        "font-size" | "font" | "r" | "rx" | "ry" | "width" | "height"
    )
}

fn has_reference(value: &str) -> bool {
    let mut targets = Vec::new();
    reference_targets(value, &mut targets);
    !targets.is_empty()
}

/// A length's magnitude in px, as usvg converts it (`units.rs`): `em`/`ex`
/// against `font`, `%` against `view` (the larger viewport side bounds
/// every `%` base usvg uses). Anything unparsable is 0 (usvg uses the
/// default, which is never larger).
fn length(value: &str, font: f64, view: f64) -> f64 {
    use svgtypes::LengthUnit;
    let Ok(length) = value.trim().parse::<svgtypes::Length>() else {
        return 0.0;
    };
    let factor = match length.unit {
        LengthUnit::None | LengthUnit::Px => 1.0,
        LengthUnit::Em => font,
        LengthUnit::Ex => font / 2.0,
        LengthUnit::In => DPI,
        LengthUnit::Cm => DPI / 2.54,
        LengthUnit::Mm => DPI / 25.4,
        LengthUnit::Pt => DPI / 72.0,
        LengthUnit::Pc => DPI / 6.0,
        LengthUnit::Percent => view / 100.0,
    };
    (length.number * factor).abs()
}

/// A `font-size` value's magnitude, against the parent's (usvg's
/// `resolve_font_size`; keywords scale the parent or the default by at
/// most 1.2³).
fn font_size(value: &str, parent: f64) -> f64 {
    let value = value.trim();
    match value {
        "xx-small" | "x-small" | "small" | "medium" | "large" | "x-large" | "xx-large"
        | "smaller" | "larger" => parent.max(DEFAULT_FONT_SIZE) * 1.728,
        _ => length(value, parent, parent),
    }
}

/// kurbo's subdivisions of a whole ellipse of radius `radius` at usvg's
/// tolerance 0.1 (`Arc::append_iter`): an arc of sweep `s` gets
/// `ceil(n × s / 2π)` cubics. Infinite for a non-finite radius.
fn subdivisions(radius: f64) -> f64 {
    let n = (1.1163 * (radius / 0.1)).powf(1.0 / 6.0).max(3.999_999);
    if n.is_finite() { n } else { f64::INFINITY }
}

/// The cubics of one quarter arc of `radius` (rounded rects, circles,
/// ellipses: four each). None for a radius kurbo draws as a line (a rect
/// without `rx`/`ry` has square corners).
fn quarter_arc(radius: f64) -> f64 {
    if radius <= 1e-5 {
        return 0.0;
    }
    whole(subdivisions(radius) * 0.25)
}

/// The segments svgtypes' `SimplifyingPathParser` makes of path data `d`
/// (what usvg's `convert_path` builds), and the sum of each arc's cubics
/// squared. Walks the raw segments, tracking the current point as the
/// simplifier does, and counts each arc's cubics with kurbo's own formula
/// instead of generating them (an arc can make millions).
fn path_segments(d: &str) -> (f64, f64) {
    let (mut x, mut y) = (0.0f64, 0.0f64);
    let (mut start_x, mut start_y) = (0.0f64, 0.0f64);
    let mut segments = 0.0f64;
    let mut squares = 0.0f64;
    for segment in svgtypes::PathParser::from(d) {
        // usvg stops at the first error.
        let Ok(segment) = segment else { break };
        use svgtypes::PathSegment as S;
        let to = |abs: bool, to_x: f64, to_y: f64| {
            if abs {
                (to_x, to_y)
            } else {
                (x + to_x, y + to_y)
            }
        };
        match segment {
            S::MoveTo {
                abs,
                x: to_x,
                y: to_y,
            } => {
                (x, y) = to(abs, to_x, to_y);
                (start_x, start_y) = (x, y);
                segments += 1.0;
            }
            S::LineTo {
                abs,
                x: to_x,
                y: to_y,
            }
            | S::CurveTo {
                abs,
                x: to_x,
                y: to_y,
                ..
            }
            | S::SmoothCurveTo {
                abs,
                x: to_x,
                y: to_y,
                ..
            }
            | S::Quadratic {
                abs,
                x: to_x,
                y: to_y,
                ..
            }
            | S::SmoothQuadratic {
                abs,
                x: to_x,
                y: to_y,
            } => {
                (x, y) = to(abs, to_x, to_y);
                segments += 1.0;
            }
            S::HorizontalLineTo { abs, x: to_x } => {
                x = if abs { to_x } else { x + to_x };
                segments += 1.0;
            }
            S::VerticalLineTo { abs, y: to_y } => {
                y = if abs { to_y } else { y + to_y };
                segments += 1.0;
            }
            S::ClosePath { .. } => {
                (x, y) = (start_x, start_y);
                // The close, and the move the simplifier inserts before a
                // segment that follows it.
                segments += 2.0;
            }
            S::EllipticalArc {
                abs,
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
                x: to_x,
                y: to_y,
            } => {
                let (end_x, end_y) = to(abs, to_x, to_y);
                let curves = arc_curves(
                    (x, y),
                    (end_x, end_y),
                    (rx, ry),
                    x_axis_rotation.to_radians(),
                    large_arc,
                    sweep,
                );
                segments += curves.max(1.0);
                squares += curves * curves;
                (x, y) = (end_x, end_y);
            }
        }
        if !segments.is_finite() || segments > 1e18 {
            return (f64::INFINITY, f64::INFINITY);
        }
    }
    (segments, squares)
}

/// The cubics kurbo 0.13 makes of an SVG arc (`Arc::from_svg_arc`, then
/// `to_cubic_beziers(0.1, …)`): the same arithmetic, without generating
/// them, rounded up. 0 for an arc kurbo draws as a line; infinite where
/// kurbo's count isn't finite (it would loop).
///
/// The simplifier starts each segment where kurbo's last cubic ended, a
/// rounding error away from the exact endpoint tracked here: an arc whose
/// endpoints coincide here may be a whole turn there, so it counts as one,
/// and a count a hair under a whole number counts the next one.
fn arc_curves(
    from: (f64, f64),
    to: (f64, f64),
    radii: (f64, f64),
    x_rotation: f64,
    large_arc: bool,
    sweep: bool,
) -> f64 {
    use std::f64::consts::PI;
    // `SvgArc::is_straight_line`.
    if radii.0.abs() <= 1e-5 || radii.1.abs() <= 1e-5 {
        return 0.0;
    }
    if from == to {
        return whole(subdivisions(radii.0.abs().max(radii.1.abs())));
    }
    // The simplifier starts the arc where kurbo's previous cubic ended, a
    // rounding error from `from`. The sweep angle below is computed from
    // the exact point, and a radius large against the chord amplifies that
    // error into any sweep up to a full turn: `A1e92 1e9 0 1 1 0 0` after
    // an arc ending at `(1e9, 0)` is one cubic from the exact point and
    // 3·10¹⁵ from the simplifier's (`(1e9 + 5e-6, -2.9e-6)`). So a radius
    // (as kurbo scales it to the chord) past `EXACT_SWEEP_SUBDIVISIONS`
    // counts the full turn, which bounds every sweep. Below it, a
    // misjudged sweep is off by at most that many cubics: like a few line
    // segments.
    let (mut rx, mut ry) = (radii.0.abs(), radii.1.abs());
    let xr = x_rotation % (2.0 * PI);
    let (sin_phi, cos_phi) = xr.sin_cos();
    let hd_x = (from.0 - to.0) * 0.5;
    let hd_y = (from.1 - to.1) * 0.5;
    let p_x = cos_phi * hd_x + sin_phi * hd_y;
    let p_y = -sin_phi * hd_x + cos_phi * hd_y;
    let rf = p_x * p_x / (rx * rx) + p_y * p_y / (ry * ry);
    if rf > 1.0 {
        let scale = rf.sqrt();
        rx *= scale;
        ry *= scale;
    }
    let full_turn = subdivisions(rx.max(ry));
    if !(full_turn <= EXACT_SWEEP_SUBDIVISIONS) {
        return whole(full_turn);
    }
    let rxry = rx * ry;
    let rxpy = rx * p_y;
    let rypx = ry * p_x;
    let sum_of_sq = rxpy * rxpy + rypx * rypx;
    let sign_coe = if large_arc == sweep { -1.0 } else { 1.0 };
    let coe = sign_coe * ((rxry * rxry - sum_of_sq) / sum_of_sq).abs().sqrt();
    let transformed_cx = coe * rxpy / ry;
    let transformed_cy = -coe * rypx / rx;
    let start = ((p_y - transformed_cy) / ry).atan2((p_x - transformed_cx) / rx);
    let end = ((-p_y - transformed_cy) / ry).atan2((-p_x - transformed_cx) / rx);
    let mut sweep_angle = (end - start) % (2.0 * PI);
    if sweep && sweep_angle < 0.0 {
        sweep_angle += 2.0 * PI;
    } else if !sweep && sweep_angle > 0.0 {
        sweep_angle -= 2.0 * PI;
    }
    // `Arc::append_iter`.
    let n_err = (1.1163 * (rx.max(ry) / 0.1)).powf(1.0 / 6.0).max(3.999_999);
    let n = n_err * sweep_angle.abs() * (1.0 / (2.0 * PI));
    if n.is_nan() {
        // `as usize` makes NaN 0 cubics.
        0.0
    } else {
        whole(n)
    }
}

/// The largest full-turn subdivision count for which [`arc_curves`]
/// trusts the exact sweep angle (a radius up to about 1.5·10⁶).
const EXACT_SWEEP_SUBDIVISIONS: f64 = 16.0;

/// `n` rounded up, with a margin for the simplifier's rounding (see
/// [`arc_curves`]); infinite past any count usvg could finish.
fn whole(n: f64) -> f64 {
    let n = (n + 1e-6 * n.max(1.0)).ceil();
    if n.is_finite() { n } else { f64::INFINITY }
}

/// Adds the ids `value` may reference: a value that is `#id` (an `href`),
/// and every `url(#id)` in it (quoted or not; `url(` in any case).
pub(crate) fn reference_targets<'a>(value: &'a str, targets: &mut Vec<&'a str>) {
    let id = |rest: &'a str| {
        let end = rest
            .find(|c: char| c == ')' || c == '"' || c == '\'' || c.is_ascii_whitespace())
            .unwrap_or(rest.len());
        &rest[..end]
    };
    if let Some(rest) = value.trim_start().strip_prefix('#') {
        targets.push(id(rest));
    }
    let bytes = value.as_bytes();
    let mut at = 0;
    while let Some(found) = bytes[at..]
        .windows(4)
        .position(|window| window.eq_ignore_ascii_case(b"url("))
    {
        let rest = value[at + found + 4..]
            .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '"' || c == '\'');
        if let Some(rest) = rest.strip_prefix('#') {
            targets.push(id(rest));
        }
        at += found + 4;
    }
}

/// usvg's `simplecss::Element` for roxmltree nodes (`svgtree/parse.rs`
/// `XmlNode`), so `<style>` selectors match exactly what usvg applies.
struct XmlNode<'a, 'input>(roxmltree::Node<'a, 'input>);

impl simplecss::Element for XmlNode<'_, '_> {
    fn parent_element(&self) -> Option<Self> {
        self.0.parent_element().map(XmlNode)
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.0.prev_sibling_element().map(XmlNode)
    }

    fn has_local_name(&self, local_name: &str) -> bool {
        self.0.tag_name().name() == local_name
    }

    fn attribute_matches(&self, local_name: &str, operator: simplecss::AttributeOperator) -> bool {
        self.0
            .attribute(local_name)
            .is_some_and(|value| operator.matches(value))
    }

    fn pseudo_class_matches(&self, class: simplecss::PseudoClass) -> bool {
        match class {
            simplecss::PseudoClass::FirstChild => self.prev_sibling_element().is_none(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What usvg builds: svgtypes' simplifier (kurbo's arcs) run for real.
    fn simplified(d: &str) -> usize {
        svgtypes::SimplifyingPathParser::from(d)
            .take_while(Result::is_ok)
            .count()
    }

    /// The counted segments are the simplifier's, arc by arc: radii from
    /// tiny (scaled up to reach the endpoint) to large, every flag,
    /// rotations, relative and absolute endpoints, after a close.
    #[test]
    fn path_segments_match_the_simplifier() {
        let mut checked = 0;
        let mut exact = 0;
        let mut singles = 0;
        for radius in ["0.5", "3", "40", "1e3", "1e6", "1e9", "1e12"] {
            for (large, sweep) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                for rotation in ["0", "30", "-120"] {
                    for (command, end) in [("A", "7 3"), ("a", "-5 9"), ("A", "1e4 -2e3")] {
                        let arc =
                            format!("{command}{radius} {radius} {rotation} {large} {sweep} {end}");
                        // The repeated arc ends where it starts: a whole
                        // turn here, not always there.
                        for (d, single) in [
                            (format!("M1 2 {arc}"), true),
                            (format!("M1 2 L4 4 Z {arc} {arc}"), false),
                            (format!("M1 2 H5 V7 Q1 1 3 3 T6 6 {arc}"), true),
                        ] {
                            let (segments, _) = path_segments(&d);
                            let built = simplified(&d);
                            // Never fewer; more only by the rounding
                            // margins (an arc that ends where it starts
                            // counts a whole turn: 4 cubics up to radius
                            // 40, more for larger ones).
                            let small = radius.parse::<f64>().unwrap() <= 40.0;
                            assert!(
                                segments as usize >= built
                                    && (!small || segments as usize <= built + 8),
                                "{d}: counted {segments}, built {built}"
                            );
                            // Past `EXACT_SWEEP_SUBDIVISIONS` (radius ~1.5e6)
                            // an arc counts a whole turn.
                            let exact_sweep = radius.parse::<f64>().unwrap() <= 1e6;
                            if single && exact_sweep {
                                // The margin rounds a whole count (a half
                                // turn: exactly 2) up by one.
                                assert!(segments as usize <= built + 1, "{d}");
                                exact += usize::from(segments as usize == built);
                                singles += 1;
                            }
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 700);
        // 276 of 360 (radii up to 1e6; the rest is the `+ 1` margin).
        assert!(
            exact * 4 > singles * 3,
            "{exact} of {singles} single arcs exact"
        );
    }

    #[test]
    fn the_largest_arcs_count_without_being_built() {
        // ~7·10¹¹ cubics: counted, never generated.
        let (segments, squares) = path_segments("M0 0 A1e70 1e70 0 1 1 1e70 0");
        assert!(segments > 1e11 && squares > 1e22, "{segments} {squares}");
        // Past ~1e77, kurbo's `rx·ry` squared overflows f64 and its
        // arithmetic can turn NaN (no cubics); counted as a whole turn
        // anyway, which is infinite: refused.
        let d = "M0 0 A1e120 1e120 0 1 1 1e120 0";
        assert!(path_segments(d).0 as usize >= simplified(d));
        assert!(path_segments(d).0.is_infinite());
        // The simplifier starts an arc where the previous one's last cubic
        // ended, `(1e9 + 5e-6, -2.9e-6)` here; with `rx = 1e92` that error
        // makes the sweep ~3·10¹⁵ cubics, where the exact start gives one
        // (a fuzz find: a 30 GB allocation). Large radii count a whole turn.
        let (segments, _) = path_segments("M0 0 A1e9 1e9 0 1 1 1e9 0 A1e92 1e9 0 1 1 0 0");
        assert!(segments > 3e15, "{segments}");
        // A zero radius is a line, as in kurbo.
        assert_eq!(path_segments("M0 0 A0 5 0 1 1 9 9").0, 2.0);
    }

    #[test]
    fn lengths_resolve_as_usvg_does() {
        assert_eq!(length("12", 16.0, 100.0), 12.0);
        assert_eq!(length("2em", 16.0, 100.0), 32.0);
        assert_eq!(length("2ex", 16.0, 100.0), 16.0);
        assert_eq!(length("1in", 16.0, 100.0), 96.0);
        assert_eq!(length("50%", 16.0, 300.0), 150.0);
        assert_eq!(length("-4px", 16.0, 100.0), 4.0);
        assert_eq!(length("auto", 16.0, 100.0), 0.0);
        assert_eq!(font_size("150%", 10.0), 15.0);
        assert!(font_size("larger", 10.0) >= 12.0);
    }

    #[test]
    fn square_corners_have_no_arcs() {
        assert_eq!(quarter_arc(0.0), 0.0);
        // kurbo makes one; the rounding margin may count two.
        assert!((1.0..=2.0).contains(&quarter_arc(10.0)));
        assert!(quarter_arc(1e30) > 1e4);
    }
}
