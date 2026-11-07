//! SVG images, behind the `svg` cargo feature (ADR 0038).
//!
//! With the feature on, [`BevyMarkupPlugin`](crate::BevyMarkupPlugin)
//! registers an asset loader that turns `.svg` files into ordinary Bevy
//! [`Image`]s, drawn on the CPU by [resvg](https://docs.rs/resvg). Anything
//! that loads an image by path accepts an SVG:
//!
//! ```html
//! <p>Gold <img src="icons/coin.svg"> 120</p>
//! ```
//!
//! ```css
//! .panel { border-image: url(frame.svg) 8 stretch; }
//! .hero  { background-image: url(banner.svg); }
//! ```
//!
//! The image is drawn at the document's intrinsic size: its `width` and
//! `height` in CSS px, else its `viewBox`. `border-image-slice` numbers are
//! in those px, as in a browser. [`SvgLoaderSettings::scale`] draws it larger
//! (`asset_server.load_with_settings::<Image, SvgLoaderSettings>(path, |s|
//! s.scale = 2.0)`); CSS-loaded images always use `1.0`.
//!
//! An `<img>` stays sharp at any size: once its box has been laid out, the
//! SVG is drawn again at the pixels it covers (the box × the scale factor,
//! `UiScale` included), off the main thread, and swapped in when ready.
//! One drawing per document and size is shared by every `<img>` showing
//! it. CSS `border-image`/`background-image` SVGs are drawn at `1.0` only.
//!
//! Static SVG only: no animation, no scripts. Text inside an SVG (`<text>`)
//! isn't drawn (the feature builds resvg without its font stack).
//!
//! # Limits
//!
//! SVG content may come from mods or players, so the loader treats every
//! file as untrusted:
//!
//! - **No external resources.** `<image href>` is never resolved: not file
//!   paths (usvg would read any file the process can), not `data:` URLs.
//!   Such an image is left out.
//! - **Size.** Neither side exceeds [`MAX_SVG_SIDE`] px; a larger document
//!   is drawn scaled down to fit, with a warning.
//! - **Render cost.** Filters, masks, clip paths and group opacity each draw
//!   offscreen layers, and some filter parameters (`numOctaves`, morphology
//!   radius, convolution order) multiply the work per pixel. Before drawing,
//!   the loader estimates the cost from the parsed document; one over
//!   [`MAX_SVG_COST`] fails to load, with an error naming the estimate.
//!   resvg can't stop a render once it starts, so the estimate is the only
//!   bound. Structural bombs (entity expansion, deep nesting, more than a
//!   million nodes after `<use>` expansion) are rejected by usvg's parser.
//!
//! # Another `.svg` loader in the app
//!
//! Bevy picks a loader for a *typed* load (`load::<Image>("x.svg")`) by the
//! asset type, so this loader and another crate's (bevy_resvg's
//! `SvgAsset`, say) coexist: each typed load reaches its own. An *untyped*
//! load, or a path with a `#label`, takes whichever loader was registered
//! last. bevy_markup makes only typed loads.

use std::fmt;

use bevy::asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader};
use bevy::image::Image;
use bevy::prelude::*;
use resvg::{tiny_skia, usvg};
// usvg's own roxmltree: the document handed to `Tree::from_xmltree` is
// parsed by the parser usvg was built with, so the two can't be different
// versions. (The direct `roxmltree` dependency is only a version floor.)
use serde::{Deserialize, Serialize};
use usvg::roxmltree;

/// The largest width or height an SVG is drawn at, in px. A larger
/// document (or a larger [`SvgLoaderSettings::scale`]) is scaled down to fit,
/// keeping its aspect ratio. Also keeps the texture within every GPU's
/// default texture-size limit.
pub const MAX_SVG_SIDE: u32 = 4096;

/// The largest estimated render cost an SVG may have, in units of one
/// pixel drawn once. A plain 4096×4096 document costs about 17 million;
/// each offscreen layer (opacity, mask, clip path, filter) adds its area
/// again, and each filter primitive its area times a weight for its kind.
/// Calibrated in `docs/agents/performance/adr-0038-probe.md` §7: at most
/// ~3.8 ns of release-build drawing per unit (0.5 s at the limit).
pub const MAX_SVG_COST: u64 = 1 << 27;

/// Settings for loading an `.svg` as an [`Image`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SvgLoaderSettings {
    /// Pixels per CSS px: `2.0` draws a 24×24 icon into a 48×48 image.
    /// Finite and above zero; the result is still capped at
    /// [`MAX_SVG_SIDE`].
    pub scale: f32,
}

impl Default for SvgLoaderSettings {
    fn default() -> Self {
        Self { scale: 1.0 }
    }
}

/// Loads `.svg` files as [`Image`]s.
#[derive(Default, TypePath)]
pub(crate) struct SvgLoader;

impl AssetLoader for SvgLoader {
    type Asset = Image;
    type Settings = SvgLoaderSettings;
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        settings: &SvgLoaderSettings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Image, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let raster = rasterize(&bytes, settings.scale, Limits::DEFAULT)
            .map_err(|err| format!("svg {}: {err}", load_context.path()))?;
        debug!(
            "svg {}: {}x{}, estimated cost {} of {MAX_SVG_COST}",
            load_context.path(),
            raster.width,
            raster.height,
            raster.cost
        );
        if raster.downscaled {
            warn!(
                "svg {}: drawn at {}x{}, scaled down to fit {MAX_SVG_SIDE} px",
                load_context.path(),
                raster.width,
                raster.height
            );
        }
        Ok(raster.image)
    }

    fn extensions(&self) -> &[&str] {
        &["svg"]
    }
}

/// Why an SVG didn't load.
#[derive(Debug)]
pub(crate) enum SvgError {
    Parse(usvg::Error),
    Scale(f32),
    TooExpensive {
        estimate: u64,
        max: u64,
    },
    Allocation {
        width: u32,
        height: u32,
    },
    TooDeep {
        depth: u32,
        max: u32,
    },
    ReferenceCycle,
    TooManyReferences {
        max: usize,
    },
    /// usvg or resvg panicked (caught by [`guarded`]).
    Panicked(String),
}

impl fmt::Display for SvgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(err) => write!(f, "{err}"),
            Self::Scale(scale) => write!(f, "scale {scale} must be finite and above zero"),
            Self::TooExpensive { estimate, max } => write!(
                f,
                "too expensive to draw (estimated cost {estimate}, limit {max}): \
                 reduce its filters, masks, clip paths or group opacity, or its size"
            ),
            Self::Allocation { width, height } => {
                write!(f, "couldn't allocate a {width}x{height} image")
            }
            Self::TooDeep { depth, max } => write!(
                f,
                "nests too deeply (depth {depth}, limit {max}; nested elements count 1, \
                 nested <svg> 2, each followed url(#…)/href reference 6 more)"
            ),
            Self::ReferenceCycle => {
                write!(
                    f,
                    "a url(#…)/href reference leads back to an element it came from"
                )
            }
            Self::TooManyReferences { max } => {
                write!(f, "more than {max} url(#…)/href references")
            }
            Self::Panicked(message) => write!(f, "the SVG renderer panicked: {message}"),
        }
    }
}

impl std::error::Error for SvgError {}

/// The bounds one rasterization works within.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub side: u32,
    pub cost: u64,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        side: MAX_SVG_SIDE,
        cost: MAX_SVG_COST,
    };
}

/// A drawn SVG.
pub(crate) struct Raster {
    pub image: Image,
    pub width: u32,
    pub height: u32,
    /// The requested size exceeded [`Limits::side`].
    pub downscaled: bool,
    /// The cost estimate the draw was allowed on.
    pub cost: u64,
}

/// usvg options that resolve no `<image href>`: usvg's default resolver
/// reads any file path the process can (absolute, cwd-relative, `..`) and
/// embeds `data:` URLs (probe report §2).
fn options() -> usvg::Options<'static> {
    usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    }
}

/// Parses an SVG the way `usvg::Tree::from_data` does, with one step in
/// between: the XML is parsed once (roxmltree, configured as usvg's
/// `from_str` configures it), [`recursion_depth`] bounds what usvg would
/// recurse through, and only then does usvg convert that same document
/// (`Tree::from_xmltree`). One parse, so the check and usvg can't disagree
/// about the document's structure (a DTD entity expanding into markup, say).
pub(crate) fn parse(bytes: &[u8]) -> Result<usvg::Tree, SvgError> {
    guarded(|| parse_unguarded(bytes))
}

/// Runs usvg or resvg work, turning a panic into [`SvgError::Panicked`].
/// Both still panic on some inputs where overflow checks are on (debug
/// builds, fuzzing): resvg 0.49's `feTurbulence` adds past `i32::MAX`
/// after enough octaves (resvg #1185, UPSTREAM U20). Bevy already turns a
/// panicking asset loader into a failed load (`AssetLoaderPanic`), but the
/// redraws of [`rasterize_svg_images`] run as `AsyncComputeTaskPool`
/// tasks, and polling a task that panicked re-raises the panic in the
/// poller: the app's main thread (checked with a probe, ADR 0038 probe
/// report §8). Here it's an error: the size isn't redrawn. (With `panic =
/// "abort"` nothing can catch it, but such builds normally have overflow
/// checks off, and the arithmetic wraps.)
fn guarded<T>(work: impl FnOnce() -> Result<T, SvgError>) -> Result<T, SvgError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| message.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_string());
        Err(SvgError::Panicked(message))
    })
}

fn parse_unguarded(bytes: &[u8]) -> Result<usvg::Tree, SvgError> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return Err(SvgError::Parse(usvg::Error::SvgzFeatureNotEnabled));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| SvgError::Parse(usvg::Error::NotAnUtf8Str))?;
    let xml_options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    let document = roxmltree::Document::parse_with_options(text, xml_options)
        .map_err(|err| SvgError::Parse(usvg::Error::ParsingFailed(err)))?;
    let depth = recursion_depth(&document, MAX_REFERENCES)?;
    if depth > MAX_DEPTH {
        return Err(SvgError::TooDeep {
            depth,
            max: MAX_DEPTH,
        });
    }
    usvg::Tree::from_xmltree(&document, &options()).map_err(SvgError::Parse)
}

/// The deepest recursion [`recursion_depth`] lets through, in its units of
/// roughly 3 KB of debug-build stack (about 1 KB in release): at most ~800
/// KB, inside the 2 MB of the threads Bevy loads assets on, with room for
/// the caller's frames. Measured per construct in the probe report §6;
/// `accepted_depths_fit_a_two_megabyte_stack` checks it.
const MAX_DEPTH: u32 = 256;
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
/// plus attributes) fail the load: the graph's memory stays bounded.
const MAX_REFERENCES: usize = 1 << 20;

/// An upper bound on how deeply usvg 0.49 (converting) and resvg (drawing)
/// recurse through `document`, in units of [`DEPTH_ELEMENT`].
///
/// usvg's converter recurses once per nested element and once per
/// reference it follows into a target's content; neither is bounded by
/// anything but its 1024-level nesting check and a million nodes, and a
/// few KB of references overflow a 2 MB stack (probe report §6, upstream
/// UPSTREAM.md U17). This walks the graph whose edges are parent → child
/// and element → referenced element, and returns its longest weighted
/// path. A reference is any `url(#id)` or `#id` value in any attribute
/// (`style` included) or in the declarations of a `<style>` rule whose
/// selector matches the element — matched with simplecss, as usvg applies
/// them (`svgtree/parse.rs` `resolve_css`). Over-approximating is safe (a
/// `fill="#fff"` beside an element `id="fff"` adds an edge); missing a
/// reference usvg follows is not.
///
/// A cycle is an error: usvg drops some cyclic links but expands `<use>`
/// cycles until its depth limit, which is past the stack. Iterative
/// throughout (Kahn's algorithm), so the check itself can't overflow.
pub(crate) fn recursion_depth(
    document: &roxmltree::Document,
    max_references: usize,
) -> Result<u32, SvgError> {
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
    let mut ids: std::collections::HashMap<&str, Vec<u32>> = std::collections::HashMap::new();
    for (i, element) in elements.iter().enumerate() {
        if let Some(id) = element.attribute("id") {
            ids.entry(id).or_default().push(i as u32);
        }
    }
    // The rules that can attach a reference, as usvg collects them.
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
    let rules: Vec<(&simplecss::Selector, Vec<&str>)> = sheet
        .rules
        .iter()
        .filter_map(|rule| {
            let mut targets = Vec::new();
            for declaration in &rule.declarations {
                reference_targets(declaration.value, &mut targets);
            }
            (!targets.is_empty()).then_some((&rule.selector, targets))
        })
        .collect();

    let weight = |element: &roxmltree::Node| {
        if element.has_tag_name("svg") {
            DEPTH_SVG
        } else {
            DEPTH_ELEMENT
        }
    };
    let mut edges: Vec<Vec<(u32, u32)>> = vec![Vec::new(); elements.len()];
    let mut edge_count = 0usize;
    let mut targets = Vec::new();
    for (i, element) in elements.iter().enumerate() {
        for child in element.children().filter(|child| child.is_element()) {
            edges[i].push((index[child.id().get_usize()], weight(&child)));
        }
        targets.clear();
        for attribute in element.attributes() {
            reference_targets(attribute.value(), &mut targets);
        }
        for (selector, rule_targets) in &rules {
            if selector.matches(&XmlNode(*element)) {
                targets.extend(rule_targets.iter().copied());
            }
        }
        targets.sort_unstable();
        targets.dedup();
        for target in &targets {
            for &to in ids.get(target).into_iter().flatten() {
                edges[i].push((to, DEPTH_REFERENCE + weight(&elements[to as usize])));
            }
        }
        edge_count += edges[i].len();
        if edge_count > max_references {
            return Err(SvgError::TooManyReferences {
                max: max_references,
            });
        }
    }

    // Longest path over a topological order; a node left over is on a cycle.
    let mut incoming = vec![0u32; elements.len()];
    for list in &edges {
        for &(to, _) in list {
            incoming[to as usize] += 1;
        }
    }
    let mut depth: Vec<u32> = elements.iter().map(weight).collect();
    let mut ready: Vec<u32> = (0..elements.len() as u32)
        .filter(|&i| incoming[i as usize] == 0)
        .collect();
    let mut done = 0usize;
    let mut deepest = 0;
    while let Some(from) = ready.pop() {
        done += 1;
        let at = depth[from as usize];
        deepest = deepest.max(at);
        for &(to, step) in &edges[from as usize] {
            let to = to as usize;
            depth[to] = depth[to].max(at.saturating_add(step));
            incoming[to] -= 1;
            if incoming[to] == 0 {
                ready.push(to as u32);
            }
        }
    }
    if done < elements.len() {
        return Err(SvgError::ReferenceCycle);
    }
    Ok(deepest)
}

/// Adds the ids `value` may reference: a value that is `#id` (an `href`),
/// and every `url(#id)` in it (quoted or not; `url(` in any case).
fn reference_targets<'a>(value: &'a str, targets: &mut Vec<&'a str>) {
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

/// The pixel size to draw `tree` at: intrinsic size × `scale`, rounded up,
/// scaled down uniformly to fit `max_side`. `true` when it was scaled down.
pub(crate) fn raster_size(
    tree: &usvg::Tree,
    scale: f32,
    max_side: u32,
) -> Result<(u32, u32, bool), SvgError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(SvgError::Scale(scale));
    }
    let size = tree.size();
    // f64: a huge intrinsic size times the scale stays finite.
    let width = f64::from(size.width()) * f64::from(scale);
    let height = f64::from(size.height()) * f64::from(scale);
    let max = f64::from(max_side.max(1));
    let fit = (max / width.max(height)).min(1.0);
    let side = |len: f64| ((len * fit).ceil() as u32).clamp(1, max_side.max(1));
    Ok((side(width), side(height), fit < 1.0))
}

/// Parses, budgets and draws an SVG.
pub(crate) fn rasterize(bytes: &[u8], scale: f32, limits: Limits) -> Result<Raster, SvgError> {
    let tree = parse(bytes)?;
    let (width, height, downscaled) = raster_size(&tree, scale, limits.side)?;
    let (image, cost) = draw(&tree, width, height, limits)?;
    Ok(Raster {
        image,
        width,
        height,
        downscaled,
        cost,
    })
}

/// Budgets and draws a parsed SVG into a `width`×`height` image (both at
/// most `limits.side`); the image and the cost estimate it was allowed on.
pub(crate) fn draw(
    tree: &usvg::Tree,
    width: u32,
    height: u32,
    limits: Limits,
) -> Result<(Image, u64), SvgError> {
    guarded(|| draw_unguarded(tree, width, height, limits))
}

fn draw_unguarded(
    tree: &usvg::Tree,
    width: u32,
    height: u32,
    limits: Limits,
) -> Result<(Image, u64), SvgError> {
    let (width, height) = (
        width.clamp(1, limits.side.max(1)),
        height.clamp(1, limits.side.max(1)),
    );
    let cost = estimate_cost(tree, width, height, limits.cost);
    if cost > limits.cost {
        return Err(SvgError::TooExpensive {
            estimate: cost,
            max: limits.cost,
        });
    }
    // Checked before the allocation: tiny-skia aborts the process on a
    // failed one rather than returning `None` (probe §4).
    let mut pixmap =
        tiny_skia::Pixmap::new(width, height).ok_or(SvgError::Allocation { width, height })?;
    let size = tree.size();
    let transform = tiny_skia::Transform::from_scale(
        width as f32 / size.width(),
        height as f32 / size.height(),
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    // tiny-skia stores premultiplied alpha; Bevy UI blends straight alpha.
    // The channels are sRGB-encoded (tiny-skia blends in sRGB, as browsers
    // do), hence `Rgba8UnormSrgb`.
    let data = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    let image = Image::new(
        wgpu_types::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        wgpu_types::TextureDimension::D2,
        data,
        wgpu_types::TextureFormat::Rgba8UnormSrgb,
        // Kept in the main world too: `<img>` sizing and `%` border-image
        // slices read the image's size.
        RenderAssetUsages::default(),
    );
    Ok((image, cost))
}

/// A parsed `.svg`, for drawing it again at another size (ADR 0038 §3).
/// Its own asset type behind its own loader, loaded typed: a labeled
/// sub-asset of the `Image` load would be found by extension alone, and
/// another crate's `.svg` loader registered after ours would take it
/// (probe report §1).
#[derive(Asset, TypePath)]
pub(crate) struct SvgDocument {
    tree: std::sync::Arc<usvg::Tree>,
}

/// Loads `.svg` files as [`SvgDocument`]s, with the same checks as
/// [`SvgLoader`] (the depth bound, the refusing resolver).
#[derive(Default, TypePath)]
pub(crate) struct SvgDocumentLoader;

impl AssetLoader for SvgDocumentLoader {
    type Asset = SvgDocument;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<SvgDocument, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let tree = parse(&bytes).map_err(|err| format!("svg {}: {err}", load_context.path()))?;
        Ok(SvgDocument {
            tree: std::sync::Arc::new(tree),
        })
    }

    fn extensions(&self) -> &[&str] {
        &["svg"]
    }
}

/// On an `<img>` span showing an SVG: what the build put there and what
/// is drawn instead (ADR 0038 §3). The build writes the 1× `source`
/// whenever it updates the span; [`rasterize_svg_images`] puts the raster
/// back in the same frame, before Bevy's UI systems read it.
#[derive(Component)]
pub(crate) struct SvgRaster {
    /// The 1× image the build writes (`Handle<Image>` of the `.svg`).
    source: Handle<Image>,
    document: Handle<SvgDocument>,
    /// The source's size: the SVG's intrinsic size in CSS px. `size_images`
    /// keeps the aspect ratio from this, not from the raster's rounded size.
    pub intrinsic: Vec2,
    /// The raster on screen and its pixel size.
    shown: Option<(UVec2, Handle<Image>)>,
    /// The size asked for last frame: a size is drawn once it holds for a
    /// frame, not on every step of a resize.
    wanted: Option<UVec2>,
}

/// Rasters of [`SvgDocument`]s by pixel size: drawn ones (shared by every
/// span showing that document at that size), draws in flight, and sizes
/// the budget refused. Entries no span uses are dropped each frame.
#[derive(Resource, Default)]
pub(crate) struct SvgRasters {
    ready: std::collections::HashMap<(AssetId<SvgDocument>, UVec2), Handle<Image>>,
    drawing: std::collections::HashMap<
        (AssetId<SvgDocument>, UVec2),
        bevy::tasks::Task<Result<Image, String>>,
    >,
    refused: std::collections::HashSet<(AssetId<SvgDocument>, UVec2)>,
}

/// Draws each SVG `<img>` at the pixel size it's shown at (its inline
/// box × the target's scale factor, `UiScale` included), so it stays sharp
/// under HiDPI, `UiScale` and CSS sizes larger than its own. Draws run on
/// the `AsyncComputeTaskPool`; until one finishes the previous image stays.
/// The swapped image keeps the box's logical size in `InlineImage`'s
/// `width`/`height`, or Bevy would size the box from the raster's pixels.
/// Runs after the build and `size_images` and before `UiSystems::Prepare`,
/// so a build that writes the 1× source back is undone the same frame.
#[allow(clippy::type_complexity)]
pub(crate) fn rasterize_svg_images(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    documents: Res<Assets<SvgDocument>>,
    mut rasters: ResMut<SvgRasters>,
    mut spans: Query<(
        Entity,
        &ChildOf,
        &mut bevy::ui::widget::InlineImage,
        &bevy::text::InlineBox,
        Option<&mut SvgRaster>,
    )>,
    blocks: Query<&ComputedNode>,
) {
    // Finished draws.
    let finished: Vec<_> = rasters
        .drawing
        .iter_mut()
        .filter_map(|(key, task)| {
            bevy::tasks::futures::check_ready(task).map(|result| (*key, result))
        })
        .collect();
    for (key, result) in finished {
        rasters.drawing.remove(&key);
        match result {
            Ok(image) => {
                let handle = images.add(image);
                rasters.ready.insert(key, handle);
            }
            Err(err) => {
                debug!("svg: not redrawn at {}x{}: {err}", key.1.x, key.1.y);
                rasters.refused.insert(key);
            }
        }
    }

    let mut in_use = std::collections::HashSet::new();
    for (entity, parent, mut image, inline_box, raster) in &mut spans {
        // What the build put there: the raster's source if the span still
        // shows (or was reset to) it, else whatever it holds now.
        let source = match &raster {
            Some(raster)
                if image.image == raster.source
                    || raster
                        .shown
                        .as_ref()
                        .is_some_and(|(_, shown)| image.image == *shown) =>
            {
                raster.source.clone()
            }
            _ => image.image.clone(),
        };
        let is_svg = source
            .path()
            .and_then(|path| path.path().extension())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
        if !is_svg {
            if raster.is_some() {
                commands.entity(entity).remove::<SvgRaster>();
            }
            continue;
        }
        let Some(intrinsic) = images.get(&source).map(|image| image.size().as_vec2()) else {
            continue;
        };
        let mut raster = match raster {
            Some(raster) if raster.source == source => raster,
            _ => {
                // A new `<img>`, or its `src` changed: start over.
                let path = source.path().expect("svg path").clone_owned();
                commands.entity(entity).insert(SvgRaster {
                    source,
                    document: server.load(path),
                    intrinsic,
                    shown: None,
                    wanted: None,
                });
                continue;
            }
        };
        let logical = inline_box.size;
        if !(logical.cmpgt(Vec2::ZERO).all() && logical.is_finite()) {
            continue;
        }
        let inverse_scale = blocks
            .get(parent.parent())
            .map_or(1.0, |node| node.inverse_scale_factor())
            .max(f32::EPSILON);
        let physical = logical / inverse_scale;
        let target = UVec2::new(
            (physical.x.ceil() as u32).clamp(1, MAX_SVG_SIDE),
            (physical.y.ceil() as u32).clamp(1, MAX_SVG_SIDE),
        );
        let document = raster.document.id();
        // Put the shown raster back if the build reset the span.
        if let Some((size, shown)) = &raster.shown {
            in_use.insert((document, *size));
            let pinned = (Some(logical.x), Some(logical.y));
            if image.image != *shown || (image.width, image.height) != pinned {
                image.image = shown.clone();
                image.width = pinned.0;
                image.height = pinned.1;
            }
        }
        // Drawn at its own size already: the 1× image serves.
        if target == intrinsic.as_uvec2() && raster.shown.is_none() {
            continue;
        }
        if raster
            .shown
            .as_ref()
            .is_some_and(|(size, _)| *size == target)
        {
            continue;
        }
        let key = (document, target);
        in_use.insert(key);
        if raster.wanted != Some(target) {
            raster.wanted = Some(target);
            continue;
        }
        if let Some(handle) = rasters.ready.get(&key).cloned() {
            image.image = handle.clone();
            image.width = Some(logical.x);
            image.height = Some(logical.y);
            raster.shown = Some((target, handle));
        } else if !rasters.refused.contains(&key) && !rasters.drawing.contains_key(&key) {
            let Some(document) = documents.get(&raster.document) else {
                continue;
            };
            let tree = document.tree.clone();
            let task = bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                draw(&tree, target.x, target.y, Limits::DEFAULT)
                    .map(|(image, _)| image)
                    .map_err(|err| err.to_string())
            });
            rasters.drawing.insert(key, task);
        }
    }
    rasters.ready.retain(|key, _| in_use.contains(key));
    rasters.refused.retain(|key| in_use.contains(key));
}

/// Cost of visiting one node (resvg's per-node setup), in pixel units. Also
/// bounds the walk: a document whose references multiply (a mask used twice
/// by the next one, and so on) adds at least this per visit, so the walk
/// stops after `max / NODE_COST` visits.
const NODE_COST: u64 = 64;
/// Cost of one path segment (tessellation; 10 million segments took 0.87 s,
/// probe §3).
const SEGMENT_COST: u64 = 24;
/// Nesting deeper than this (groups, masks, clip paths, patterns) is
/// refused: resvg recurses once per level.
const MAX_COST_DEPTH: u32 = 512;

/// Estimates the cost of drawing `tree` into a `width`×`height` image, in
/// units of one pixel drawn once, stopping early once it exceeds `max`
/// (the result is then some value above `max`).
///
/// The model follows resvg 0.49's renderer (`render.rs`, `mask.rs`,
/// `clip.rs`, `filter/mod.rs`): an isolated group (opacity, filter, mask,
/// clip path, blend mode) draws into a layer the size of its bounding box,
/// at most 5× the canvas on each side (`max_filter_bbox`); filters run over
/// that layer; a clip path or mask allocates another layer of the same
/// size and draws its content into it. The weights per filter primitive
/// are the measured scaling laws of probe §4.
pub(crate) fn estimate_cost(tree: &usvg::Tree, width: u32, height: u32, max: u64) -> u64 {
    let size = tree.size();
    let mut walk = CostWalk {
        total: 0,
        max,
        scale_x: f64::from(width) / f64::from(size.width()),
        scale_y: f64::from(height) / f64::from(size.height()),
        max_width: f64::from(width) * 5.0,
        max_height: f64::from(height) * 5.0,
    };
    // The canvas itself: cleared, drawn into, read back.
    let canvas = f64::from(width) * f64::from(height);
    if walk.add_area(canvas, 2.0).is_ok() {
        let _ = walk.group(tree.root(), 0);
    }
    walk.total
}

/// The walk stopped: the estimate is over the limit.
struct Over;

struct CostWalk {
    total: u64,
    max: u64,
    scale_x: f64,
    scale_y: f64,
    max_width: f64,
    max_height: f64,
}

impl CostWalk {
    fn add(&mut self, cost: u64) -> Result<(), Over> {
        self.total = self.total.saturating_add(cost);
        if self.total > self.max {
            Err(Over)
        } else {
            Ok(())
        }
    }

    fn add_area(&mut self, area: f64, weight: f64) -> Result<(), Over> {
        // Saturating: `as` maps NaN to 0 and anything past u64::MAX to it.
        self.add((area * weight).ceil() as u64)
    }

    /// The pixel area of a layer over `rect` (user space), clamped as resvg
    /// clamps layers.
    fn area(&self, width: f32, height: f32) -> f64 {
        let w = (f64::from(width) * self.scale_x).clamp(1.0, self.max_width.max(1.0));
        let h = (f64::from(height) * self.scale_y).clamp(1.0, self.max_height.max(1.0));
        w * h
    }

    fn group(&mut self, group: &usvg::Group, depth: u32) -> Result<(), Over> {
        if depth > MAX_COST_DEPTH {
            self.total = self.max.saturating_add(1);
            return Err(Over);
        }
        self.add(NODE_COST)?;
        if group.should_isolate() {
            let bbox = group.abs_layer_bounding_box();
            let layer = self.area(bbox.width(), bbox.height());
            // Allocated, then composited onto its parent.
            self.add_area(layer, 2.0)?;
            for filter in group.filters() {
                for primitive in filter.primitives() {
                    self.primitive(primitive.kind(), layer, depth)?;
                }
            }
            if let Some(clip) = group.clip_path() {
                self.clip(clip, layer, depth + 1)?;
            }
            if let Some(mask) = group.mask() {
                self.mask(mask, layer, depth + 1)?;
            }
        }
        for child in group.children() {
            self.node(child, depth + 1)?;
        }
        Ok(())
    }

    fn node(&mut self, node: &usvg::Node, depth: u32) -> Result<(), Over> {
        match node {
            usvg::Node::Group(group) => self.group(group, depth),
            usvg::Node::Path(path) => self.path(path, depth),
            usvg::Node::Image(image) => {
                self.add(NODE_COST)?;
                // Never produced with the refusing resolver; counted anyway.
                if let usvg::ImageKind::SVG(tree) = image.kind() {
                    self.group(tree.root(), depth + 1)?;
                }
                Ok(())
            }
            usvg::Node::Text(_) => self.add(NODE_COST),
        }
    }

    fn path(&mut self, path: &usvg::Path, depth: u32) -> Result<(), Over> {
        self.add(NODE_COST)?;
        let segments = path.data().len() as u64;
        self.add(segments.saturating_mul(SEGMENT_COST))?;
        let bbox = path.abs_stroke_bounding_box();
        let area = self.area(bbox.width(), bbox.height());
        let fill = path.fill().map(|fill| fill.paint());
        let stroke = path.stroke().map(|stroke| stroke.paint());
        for paint in fill.into_iter().chain(stroke) {
            self.add_area(area, 1.0)?;
            if let usvg::Paint::Pattern(pattern) = paint {
                // The tile is drawn per use, then shaded over the path.
                self.group(pattern.root(), depth + 1)?;
            }
        }
        Ok(())
    }

    fn clip(&mut self, clip: &usvg::ClipPath, layer: f64, depth: u32) -> Result<(), Over> {
        if depth > MAX_COST_DEPTH {
            self.total = self.max.saturating_add(1);
            return Err(Over);
        }
        // A layer-sized pixmap, filled, drawn into, applied.
        self.add_area(layer, 3.0)?;
        self.group(clip.root(), depth + 1)?;
        if let Some(inner) = clip.clip_path() {
            self.clip(inner, layer, depth + 1)?;
        }
        Ok(())
    }

    fn mask(&mut self, mask: &usvg::Mask, layer: f64, depth: u32) -> Result<(), Over> {
        if depth > MAX_COST_DEPTH {
            self.total = self.max.saturating_add(1);
            return Err(Over);
        }
        // A layer-sized pixmap and an alpha mask, drawn, converted, applied.
        self.add_area(layer, 4.0)?;
        self.group(mask.root(), depth + 1)?;
        if let Some(inner) = mask.mask() {
            self.mask(inner, layer, depth + 1)?;
        }
        Ok(())
    }

    fn primitive(&mut self, kind: &usvg::filter::Kind, layer: f64, depth: u32) -> Result<(), Over> {
        use usvg::filter::Kind;
        self.add(NODE_COST)?;
        // Every primitive converts its inputs' color space and writes a
        // result buffer: a base of 4 passes over the layer.
        let weight = 4.0
            + match kind {
                Kind::GaussianBlur(_) => 12.0,
                Kind::DropShadow(_) => 16.0,
                Kind::Morphology(fe) => {
                    // Grows faster than linearly in the radius (calibration:
                    // r 2 → 0.03 µs/px, 10 → 0.23, 40 → 3.5).
                    let rx = f64::from(fe.radius_x().get()) * self.scale_x;
                    let ry = f64::from(fe.radius_y().get()) * self.scale_y;
                    2.0 * rx * ry + 8.0 * (rx + ry)
                }
                Kind::ConvolveMatrix(fe) => {
                    let matrix = fe.matrix();
                    f64::from(matrix.columns()) * f64::from(matrix.rows())
                }
                Kind::Turbulence(fe) => 16.0 * f64::from(fe.num_octaves().max(1)),
                Kind::DiffuseLighting(_) | Kind::SpecularLighting(_) => 32.0,
                Kind::DisplacementMap(_) => 8.0,
                Kind::Merge(fe) => 2.0 * fe.inputs().len() as f64,
                Kind::Image(fe) => {
                    self.group(fe.root(), depth + 1)?;
                    0.0
                }
                Kind::Blend(_)
                | Kind::ColorMatrix(_)
                | Kind::ComponentTransfer(_)
                | Kind::Composite(_)
                | Kind::Flood(_)
                | Kind::Offset(_)
                | Kind::Tile(_) => 0.0,
            };
        self.add_area(layer, weight)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svg(width: u32, height: u32, body: &str) -> String {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}">{body}</svg>"#
        )
    }

    fn draw(source: &str) -> Result<Raster, SvgError> {
        rasterize(source.as_bytes(), 1.0, Limits::DEFAULT)
    }

    fn pixel(raster: &Raster, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * raster.width + x) * 4) as usize;
        let data = raster.image.data.as_ref().expect("CPU data kept");
        data[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn intrinsic_size_comes_from_width_height_else_the_view_box() {
        let sized = draw(&svg(20, 10, "")).unwrap();
        assert_eq!((sized.width, sized.height), (20, 10));
        let view_box =
            draw(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 30 12"/>"#).unwrap();
        assert_eq!((view_box.width, view_box.height), (30, 12));
        // A fractional size rounds up: no edge pixel is lost.
        let fractional =
            draw(r#"<svg xmlns="http://www.w3.org/2000/svg" width="10.2" height="3.5"/>"#).unwrap();
        assert_eq!((fractional.width, fractional.height), (11, 4));
    }

    #[test]
    fn scale_multiplies_the_size_and_rejects_nonsense() {
        let source = svg(24, 12, "");
        let doubled = rasterize(source.as_bytes(), 2.0, Limits::DEFAULT).unwrap();
        assert_eq!((doubled.width, doubled.height), (48, 24));
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                matches!(
                    rasterize(source.as_bytes(), scale, Limits::DEFAULT),
                    Err(SvgError::Scale(_))
                ),
                "scale {scale}"
            );
        }
    }

    #[test]
    fn oversized_documents_scale_down_to_the_side_limit() {
        // 100,000 px wide: the probe's process abort when drawn as asked.
        let raster = draw(&svg(
            100_000,
            50_000,
            "<rect width='100000' height='50000'/>",
        ))
        .unwrap();
        assert_eq!(
            (raster.width, raster.height),
            (MAX_SVG_SIDE, MAX_SVG_SIDE / 2)
        );
        assert!(raster.downscaled);
        let small = Limits {
            side: 64,
            ..Limits::DEFAULT
        };
        let raster = rasterize(svg(1000, 10, "").as_bytes(), 1.0, small).unwrap();
        assert_eq!((raster.width, raster.height), (64, 1));
    }

    #[test]
    fn pixels_are_straight_alpha_srgb() {
        let raster = draw(&svg(
            2,
            1,
            "<rect width='1' height='1' fill='#ff8000'/>\
             <rect x='1' width='1' height='1' fill='#ff8000' fill-opacity='0.5'/>",
        ))
        .unwrap();
        assert_eq!(
            raster.image.texture_descriptor.format,
            wgpu_types::TextureFormat::Rgba8UnormSrgb
        );
        assert_eq!(pixel(&raster, 0, 0), [255, 128, 0, 255]);
        // Premultiplied would be [128, 64, 0, 128].
        let [r, g, b, a] = pixel(&raster, 1, 0);
        assert!((127..=129).contains(&a), "alpha {a}");
        assert_eq!((r, b), (255, 0));
        assert!((126..=129).contains(&g), "green {g}");
    }

    #[test]
    fn image_hrefs_are_never_resolved() {
        // A real PNG on disk next to the test: absolute and relative paths
        // and a data URL would all be embedded by usvg's default resolver.
        let dir = std::env::temp_dir().join(format!("bevy_markup_svg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("secret.png");
        let mut bytes = Vec::new();
        {
            let pixmap = tiny_skia::Pixmap::new(1, 1).unwrap();
            bytes.extend(pixmap.encode_png().unwrap());
        }
        std::fs::write(&png, &bytes).unwrap();
        let data_url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";
        let svg_data_url = "data:image/svg+xml;utf8,&lt;svg xmlns='http://www.w3.org/2000/svg' width='1' height='1'/&gt;";
        for href in [png.to_str().unwrap(), "secret.png", data_url, svg_data_url] {
            let source = svg(
                4,
                4,
                &format!(r#"<image width="4" height="4" href="{href}"/>"#),
            );
            let tree = parse(source.as_bytes()).unwrap();
            assert!(
                tree.root().children().is_empty(),
                "{href}: resolved into {:?}",
                tree.root().children()
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn structural_bombs_fail_to_parse() {
        let mut laughs = String::from(r#"<!DOCTYPE svg [<!ENTITY l0 "lol">"#);
        for i in 1..10 {
            laughs += &format!(r#"<!ENTITY l{i} "{}">"#, format!("&l{};", i - 1).repeat(10));
        }
        laughs += "]>";
        laughs += &svg(10, 10, r#"<rect width="5" height="5" id="&l9;"/>"#);
        assert!(matches!(draw(&laughs), Err(SvgError::Parse(_))));

        let mut uses = String::from("<defs><g id='g0'><rect width='1' height='1'/></g>");
        for i in 1..=7 {
            uses += &format!(
                "<g id='g{i}'>{}</g>",
                format!("<use href='#g{}'/>", i - 1).repeat(10)
            );
        }
        uses += "</defs><use href='#g7'/>";
        assert!(matches!(
            draw(&svg(100, 100, &uses)),
            Err(SvgError::Parse(_))
        ));

        // Past usvg's own 1024 levels; the depth check refuses it first.
        let deep = "<g>".repeat(2000) + &"</g>".repeat(2000);
        assert!(matches!(
            draw(&svg(10, 10, &deep)),
            Err(SvgError::TooDeep { .. })
        ));
    }

    /// One construct of the probe's stack overflows (§6), `n` deep: `wrap`
    /// turns the content so far into the next level, adding definitions to
    /// `defs` where the construct is a reference.
    #[derive(Clone, Copy, Debug)]
    enum Level {
        Group,
        Svg,
        Use,
        Pattern,
        Mask,
        Marker,
        FeImage,
        /// A mask attached by a `<style>` rule, not an attribute.
        CssMask,
    }

    const LEVELS: [Level; 8] = [
        Level::Group,
        Level::Svg,
        Level::Use,
        Level::Pattern,
        Level::Mask,
        Level::Marker,
        Level::FeImage,
        Level::CssMask,
    ];

    fn wrap(level: Level, k: usize, inner: String, defs: &mut String, css: &mut String) -> String {
        match level {
            Level::Group => format!("<g>{inner}</g>"),
            Level::Svg => format!("<svg width='8' height='8'>{inner}</svg>"),
            Level::Use => {
                *defs += &format!("<g id='u{k}'>{inner}</g>");
                format!("<use href='#u{k}'/>")
            }
            Level::Pattern => {
                *defs += &format!(
                    "<pattern id='p{k}' width='8' height='8' patternUnits='userSpaceOnUse'>{inner}</pattern>"
                );
                format!("<rect width='8' height='8' fill='url(#p{k})'/>")
            }
            Level::Mask => {
                *defs += &format!("<mask id='m{k}'>{inner}</mask>");
                format!("<rect width='8' height='8' fill='white' mask='url(#m{k})'/>")
            }
            Level::Marker => {
                *defs +=
                    &format!("<marker id='k{k}' markerWidth='8' markerHeight='8'>{inner}</marker>");
                format!("<path d='M0 0h8' stroke='black' marker-end='url(#k{k})'/>")
            }
            Level::FeImage => {
                *defs += &format!(
                    "<g id='r{k}'>{inner}</g><filter id='f{k}'><feImage href='#r{k}'/></filter>"
                );
                format!("<rect width='8' height='8' filter='url(#f{k})'/>")
            }
            Level::CssMask => {
                *defs += &format!("<mask id='c{k}'>{inner}</mask>");
                *css += &format!(".c{k} {{ mask: url(#c{k}) }}");
                format!("<rect class='c{k}' width='8' height='8' fill='white'/>")
            }
        }
    }

    /// `levels` applied innermost first, as a document.
    fn nest(levels: &[Level]) -> String {
        let (mut defs, mut css) = (String::new(), String::new());
        let mut inner = "<rect width='8' height='8' fill='red'/>".to_string();
        for (k, &level) in levels.iter().enumerate() {
            inner = wrap(level, k, inner, &mut defs, &mut css);
        }
        svg(
            64,
            64,
            &format!("<style>{css}</style><defs>{defs}</defs>{inner}"),
        )
    }

    fn depth_of(source: &str) -> Result<u32, SvgError> {
        let document = roxmltree::Document::parse_with_options(
            source,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..Default::default()
            },
        )
        .unwrap();
        recursion_depth(&document, MAX_REFERENCES)
    }

    /// The deepest stack of `pick(i)` levels the check accepts, and the
    /// first it refuses.
    fn deepest_accepted(pick: impl Fn(usize) -> Level) -> (Vec<Level>, String) {
        let mut levels = Vec::new();
        loop {
            levels.push(pick(levels.len()));
            let source = nest(&levels);
            if matches!(depth_of(&source), Ok(depth) if depth > MAX_DEPTH) {
                assert!(
                    matches!(parse(source.as_bytes()), Err(SvgError::TooDeep { .. })),
                    "{levels:?}: past the limit but not refused"
                );
                levels.pop();
                return (levels.clone(), nest(&levels));
            }
        }
    }

    /// The check's soundness: for each construct that overflowed a 2 MB
    /// stack in the probe, and for mixes of them, the deepest document the
    /// check lets through parses, budgets and draws on a 2 MB thread (Bevy's
    /// task-pool default), in this (debug) build. A miss aborts the test
    /// process instead of failing it.
    #[test]
    fn accepted_depths_fit_a_two_megabyte_stack() {
        let mut cases: Vec<(String, Vec<Level>, String)> = Vec::new();
        for level in LEVELS {
            let (levels, source) = deepest_accepted(|_| level);
            cases.push((format!("{level:?}"), levels, source));
        }
        // Mixes: a fixed pseudo-random sequence per seed.
        for seed in 1..=24u64 {
            let pick = |i: usize| {
                let mut x = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add((i as u64).wrapping_mul(1442695040888963407));
                x ^= x >> 29;
                LEVELS[(x % LEVELS.len() as u64) as usize]
            };
            let (levels, source) = deepest_accepted(pick);
            cases.push((format!("mix {seed}"), levels, source));
        }
        for (name, levels, source) in cases {
            let depth = depth_of(&source).unwrap();
            let outcome = std::thread::Builder::new()
                .stack_size(2 << 20)
                .spawn(move || {
                    draw(&source)
                        .map(|raster| raster.cost)
                        .map_err(|err| err.to_string())
                })
                .unwrap()
                .join()
                .unwrap();
            // Drawn or refused by the budget/usvg: anything but a crash
            // (which aborts the process before this line). Each case is a
            // real chain at the limit, not an empty document.
            assert!(
                levels.len() > 1 && depth <= MAX_DEPTH && depth > MAX_DEPTH / 2,
                "{name}: {} levels, depth {depth}: {outcome:?}",
                levels.len()
            );
        }
    }

    /// resvg #1185's overflow (100 octaves of noise) panics where overflow
    /// checks are on; the draw fails with an error instead of unwinding
    /// into its caller (for a redraw task: the app's main thread).
    #[test]
    fn a_renderer_panic_is_an_error() {
        let noisy = svg(
            16,
            16,
            "<filter id='f'><feTurbulence baseFrequency='0.05' numOctaves='100'/></filter><rect width='16' height='16' filter='url(#f)'/>",
        );
        match draw(&noisy) {
            Err(SvgError::Panicked(message)) => assert!(message.contains("overflow"), "{message}"),
            // Without overflow checks the arithmetic wraps and it draws.
            Ok(_) if !cfg!(debug_assertions) => {}
            other => panic!("{:?}", other.map(|raster| raster.cost)),
        }
    }

    #[test]
    fn reference_cycles_are_refused() {
        let cyclic_use = svg(
            8,
            8,
            "<g id='a'><use href='#b'/></g><g id='b'><use href='#a'/></g>",
        );
        assert!(matches!(draw(&cyclic_use), Err(SvgError::ReferenceCycle)));
        let self_mask = svg(
            8,
            8,
            "<mask id='m'><rect width='8' height='8' mask='url(#m)'/></mask><rect width='8' height='8' mask='url(#m)'/>",
        );
        assert!(matches!(draw(&self_mask), Err(SvgError::ReferenceCycle)));
        // Through a `<style>` rule.
        let css = svg(
            8,
            8,
            "<style>.x { fill: url(#p) }</style><pattern id='p' width='8' height='8'><rect class='x' width='8' height='8'/></pattern><rect class='x' width='8' height='8'/>",
        );
        assert!(matches!(draw(&css), Err(SvgError::ReferenceCycle)));
    }

    #[test]
    fn references_are_found_in_every_spelling() {
        let depth = |body: &str| {
            depth_of(&svg(
                8,
                8,
                &format!("<defs><mask id='m'><g><g><g/></g></g></mask></defs>{body}"),
            ))
            .unwrap()
        };
        // svg(1+... ) → rect: 2 + 1; without a reference the mask's content
        // is only defs → mask → g → g → g = 2 + 1 + 1 + 3.
        let unreferenced = depth("<rect/>");
        for body in [
            "<rect mask='url(#m)'/>",
            "<rect mask=\"url( '#m' )\"/>",
            "<rect mask='URL(#m)'/>",
            "<rect style='mask: url(#m)'/>",
            "<rect mask='url(&#35;m)'/>",
            "<use href='#m'/>",
            "<use xlink:href='#m' xmlns:xlink='http://www.w3.org/1999/xlink'/>",
            "<style>rect { mask: url(#m) }</style><rect/>",
            "<style>svg > rect { mask: url(#m) }</style><rect/>",
        ] {
            assert!(depth(body) > unreferenced, "{body}: no reference found");
        }
        // A rule that doesn't match the element adds nothing (`defs` comes
        // first, so the rect isn't `:first-child`).
        assert_eq!(
            depth("<style>circle { mask: url(#m) }</style><rect/>"),
            unreferenced
        );
        assert_eq!(
            depth("<style>rect:first-child { mask: url(#m) }</style><rect/>"),
            unreferenced
        );
    }

    #[test]
    fn markup_from_entities_counts_toward_the_depth() {
        // Each entity adds 60 levels; ten of them nest 540 deep from 4 KB, as
        // roxmltree (and so usvg) expands them.
        let mut dtd = String::from("<!DOCTYPE svg [<!ENTITY e0 \"<g/>\">");
        for i in 1..10 {
            dtd += &format!(
                "<!ENTITY e{i} \"{}&e{};{}\">",
                "<g>".repeat(60),
                i - 1,
                "</g>".repeat(60)
            );
        }
        dtd += "]>";
        let source = dtd + &svg(8, 8, "&e9;");
        assert!(source.len() < 5000);
        assert!(matches!(draw(&source), Err(SvgError::TooDeep { .. })));
    }

    #[test]
    fn reference_fan_out_is_bounded() {
        // Every rect matches every rule: rules × elements edges.
        let css: String = (0..2000)
            .map(|i| format!("rect {{ fill: url(#g{i}) }}"))
            .collect();
        let defs: String = (0..2000)
            .map(|i| format!("<linearGradient id='g{i}'/>"))
            .collect();
        let rects = "<rect/>".repeat(1000);
        let source = svg(
            8,
            8,
            &format!("<style>{css}</style><defs>{defs}</defs>{rects}"),
        );
        assert!(matches!(
            draw(&source),
            Err(SvgError::TooManyReferences { .. })
        ));
    }

    /// The recursion-depth check matches `<style>` rules with simplecss
    /// as usvg does, so both must use the same simplecss. usvg keeps it
    /// private (no type to check at compile time, unlike roxmltree, which
    /// comes from usvg's re-export, and cssparser, ADR 0039), so the
    /// lockfile is checked instead: one simplecss, the one usvg resolves.
    /// Both ask for `0.2`, so Cargo resolves one copy for any app; a split
    /// needs usvg to move to another simplecss, which arrives with a resvg
    /// bump here, and fails this test: bump `simplecss` in `Cargo.toml`.
    #[test]
    fn usvg_and_the_depth_check_share_one_simplecss_and_roxmltree() {
        let lock = include_str!("../Cargo.lock");
        for name in ["simplecss", "roxmltree"] {
            let versions: Vec<&str> = lock
                .split("[[package]]")
                .filter(|package| package.contains(&format!("\nname = \"{name}\"\n")))
                .filter_map(|package| {
                    package
                        .lines()
                        .find_map(|line| line.strip_prefix("version = "))
                })
                .collect();
            assert_eq!(versions.len(), 1, "{name}: {versions:?} in Cargo.lock");
        }
        // The usvg entry names it without a version: unambiguous.
        let usvg = lock
            .split("[[package]]")
            .find(|package| package.contains("\nname = \"usvg\"\n"))
            .expect("usvg in Cargo.lock");
        assert!(
            usvg.contains("\"simplecss\""),
            "usvg's dependencies: {usvg}"
        );
    }

    /// The probe's costly documents (§4), each a few KB and within the depth
    /// limit, fail the budget instead of drawing for seconds.
    #[test]
    fn costly_documents_exceed_the_budget() {
        let rect = |side: u32| format!("<rect width='{side}' height='{side}' filter='url(#f)'/>");
        let nested = |open: &str, close: &str, n: usize, side: u32| {
            open.repeat(n) + &format!("<rect width='{side}' height='{side}'/>") + &close.repeat(n)
        };
        let mut masks =
            "<mask id='m0'><rect width='2048' height='2048' fill='white'/></mask>".to_string();
        for i in 1..=28 {
            masks += &format!(
                "<mask id='m{i}'><rect width='2048' height='2048' fill='white' mask='url(#m{})'/></mask>",
                i - 1
            );
        }
        masks += "<rect width='2048' height='2048' mask='url(#m28)'/>";
        let mut clips =
            "<clipPath id='c0'><rect width='2048' height='2048'/></clipPath>".to_string();
        for i in 1..=30 {
            clips += &format!(
                "<clipPath id='c{i}' clip-path='url(#c{})'><rect width='2048' height='2048'/></clipPath>",
                i - 1
            );
        }
        clips += "<rect width='2048' height='2048' clip-path='url(#c30)'/>";
        let cases = [
            (
                "blur chain",
                svg(
                    1024,
                    1024,
                    &format!(
                        "<filter id='f'>{}</filter>{}",
                        "<feGaussianBlur stdDeviation='5'/>".repeat(1000),
                        rect(1024)
                    ),
                ),
            ),
            (
                "octaves",
                svg(
                    2048,
                    2048,
                    &format!(
                        "<filter id='f'><feTurbulence baseFrequency='0.01' numOctaves='1000000'/></filter>{}",
                        rect(2048)
                    ),
                ),
            ),
            (
                "morphology",
                svg(
                    1024,
                    1024,
                    &format!(
                        "<filter id='f'><feMorphology radius='100000'/></filter>{}",
                        rect(1024)
                    ),
                ),
            ),
            (
                "convolve",
                svg(
                    1024,
                    1024,
                    &format!(
                        "<filter id='f'><feConvolveMatrix order='300' kernelMatrix='{}'/></filter>{}",
                        "1 ".repeat(90_000),
                        rect(1024)
                    ),
                ),
            ),
            (
                "blur region",
                svg(
                    2048,
                    2048,
                    "<filter id='f' x='-10' y='-10' width='20' height='20'><feGaussianBlur stdDeviation='100000'/></filter><rect width='2048' height='2048' filter='url(#f)'/>",
                ),
            ),
            (
                "opacity nest",
                svg(1024, 1024, &nested("<g opacity='0.99'>", "</g>", 200, 1024)),
            ),
            (
                "filter nest",
                svg(
                    1024,
                    1024,
                    &(r#"<filter id="f"><feOffset dx="1"/></filter>"#.to_string()
                        + &nested("<g filter='url(#f)'>", "</g>", 100, 1024)),
                ),
            ),
            ("mask nest", svg(2048, 2048, &masks)),
            ("clip nest", svg(2048, 2048, &clips)),
        ];
        for (name, source) in cases {
            match draw(&source) {
                Err(SvgError::TooExpensive { estimate, max }) => assert!(estimate > max, "{name}"),
                Err(err) => panic!("{name}: failed otherwise: {err}"),
                Ok(raster) => panic!("{name}: drew within budget (estimate {})", raster.cost),
            }
        }
    }

    /// Ordinary content stays far inside the budget.
    #[test]
    fn ordinary_documents_fit_the_budget() {
        let shadow = "<filter id='s'><feDropShadow dx='2' dy='2' stdDeviation='3'/></filter>";
        let cases = [
            (
                "icon",
                svg(
                    24,
                    24,
                    "<path d='M2 2 L22 2 L12 22 Z' fill='gold' stroke='black'/>",
                ),
            ),
            (
                "frame",
                svg(
                    256,
                    256,
                    "<rect x='4' y='4' width='248' height='248' rx='16' fill='none' stroke='#843' stroke-width='8'/>",
                ),
            ),
            (
                "shadowed panel",
                svg(
                    1024,
                    1024,
                    &format!(
                        "{shadow}<g filter='url(#s)' opacity='0.9'><rect width='1000' height='1000' rx='24' fill='#222'/></g>"
                    ),
                ),
            ),
            (
                "full canvas",
                svg(
                    MAX_SVG_SIDE,
                    MAX_SVG_SIDE,
                    "<linearGradient id='g'><stop offset='0' stop-color='red'/><stop offset='1' stop-color='blue'/></linearGradient><rect width='100%' height='100%' fill='url(#g)'/>",
                ),
            ),
        ];
        for (name, source) in cases {
            let raster = draw(&source).unwrap_or_else(|err| panic!("{name}: {err}"));
            assert!(
                raster.cost < MAX_SVG_COST / 2,
                "{name}: estimate {} of {MAX_SVG_COST}",
                raster.cost
            );
        }
    }

    /// A reference used twice by the next one doubles the work per level:
    /// the walk stops at the budget instead of visiting 2^n nodes.
    #[test]
    fn multiplying_references_stop_the_walk() {
        let mut masks =
            "<mask id='m0'><rect width='1' height='1' fill='white'/></mask>".to_string();
        for i in 1..=60 {
            masks += &format!(
                "<mask id='m{i}'><rect width='1' height='1' mask='url(#m{p})'/><rect width='1' height='1' mask='url(#m{p})'/></mask>",
                p = i - 1
            );
        }
        masks += "<rect width='1' height='1' mask='url(#m60)'/>";
        // Without the bound this test doesn't finish.
        assert!(draw(&svg(1, 1, &masks)).is_err(), "drew a 2^60 mask tree");
    }
}
