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
//! it. A CSS `border-image`/`background-image` SVG is drawn at the node's
//! scale factor (HiDPI, `UiScale`) with the same geometry: a frame's corners
//! and a background's tiles keep their logical size, a stretched background
//! is drawn at the node's pixels.
//!
//! # Inline `<svg>`
//!
//! An `<svg>` written in a template is one inline image, like an `<img>`:
//!
//! ```html
//! <p>Gold <svg class="icon" viewBox="0 0 24 24"><circle cx="12" cy="12"
//!   r="10" fill="currentColor"/></svg> 120</p>
//! ```
//!
//! ```css
//! .icon { width: 16px; color: gold; }
//! .icon:hover { color: white; }
//! ```
//!
//! - Its source text is the document, as the template rendered it. It
//!   must be well-formed XML (quoted attributes, closed or self-closed
//!   tags, no HTML-only entities like `&nbsp;`); one that isn't shows
//!   nothing. `xmlns` (and `xmlns:xlink`) may be left out, as in HTML.
//! - `currentColor` is the element's computed CSS `color`, so CSS
//!   recolors it (`:hover`, themes) like text. Other page CSS doesn't reach
//!   inside it; the SVG's own attributes and `<style>` do.
//! - It's sized like an `<img>`: CSS `width`/`height`, else the document's
//!   own `width`/`height`, else its `viewBox` (not the browser's 300 × 150
//!   default or its stretch to the container).
//! - Each distinct text is drawn once; elements showing the same text
//!   share the image, and it's redrawn at its displayed size like an
//!   `<img>`. A small document is drawn during the build (a recolor shows
//!   no blank frame); a large or costly one on a background task.
//! - In Markdown, it's raw HTML: a blank line inside it ends the raw HTML
//!   block (CommonMark), so write it without blank lines.
//!
//! Static SVG only: no animation, no scripts.
//!
//! # Text
//!
//! `<text>` inside an SVG needs the `svg-text` feature (resvg's font stack);
//! without it, text draws nothing. With it, an SVG's `font-family`
//! resolves like the page's CSS: through the families registered in
//! [`FontFamilies`](crate::fonts::FontFamilies) (any case), the generic
//! keywords mapped with `set_generic`, and CSS's face fallback; a family
//! nobody registered uses Bevy's default font. Only faces given as font
//! files (`FontSource::Handle`) are available: a system family
//! (`FontSource::Family`, a generic system font) falls back to the default
//! font. A font that loads after an SVG was drawn draws it again: `.svg`
//! files with text are reloaded, and inline `<svg>` with text is redrawn.
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
//!   bound.
//! - **Conversion work.** Before the document is converted at all, the
//!   loader bounds what conversion would build: each `<use>` copies its
//!   target, a marker is converted once per vertex of every path carrying
//!   it, an inherited pattern or gradient once per shape, and an elliptical
//!   arc splits into more curves the larger its radius. A document whose
//!   estimate exceeds [`MAX_SVG_WORK`] (a few KB can describe millions of
//!   copies) fails to load, with an error naming the estimate.
//! - **Structure.** Nesting deeper than usvg's converter can recurse on a
//!   2 MB stack, and a reference cycle — including a pattern or marker that
//!   inherits a `fill`, `stroke` or `marker-*` naming itself from an
//!   ancestor — fail to load. Entity expansion past usvg's limits fails
//!   in usvg's parser.
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
use bevy::ui::widget::NodeImageMode;
use resvg::{tiny_skia, usvg};
// usvg's own roxmltree: the document handed to `Tree::from_xmltree` is
// parsed by the parser usvg was built with, so the two can't be different
// versions. (The direct `roxmltree` dependency is only a version floor.)
use serde::{Deserialize, Serialize};
use usvg::roxmltree;

use crate::svg_bounds;

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

/// The largest estimated work usvg's conversion of an SVG may take, before
/// it's drawn: element instances after `<use>` copies, markers (per path
/// vertex), patterns and gradients (per shape inheriting them), and each
/// instance's path segments (an elliptical arc splits into more cubics the
/// larger its radius). One element instance is 16 units, one segment 4, a
/// unit ~60 ns of release-build conversion (ADR 0040): ~0.5 s at the limit,
/// like [`MAX_SVG_COST`]'s drawing. A document of 50 000 elements and a
/// million path segments fits.
pub const MAX_SVG_WORK: u64 = 1 << 23;

/// The farthest any drawn path may reach from the image's origin, in the
/// drawn image's pixels (2¹⁹). Shapes may extend past the image, as they
/// often do (a background larger than the view, a map cropped by its
/// `viewBox`); this bounds how far. tiny-skia 0.12's rasterizer panics, in
/// release builds too, on paths far outside its canvas (UPSTREAM U24,
/// U27, U28): with anti-aliasing it shifts the path's unclipped rows left
/// by 2 in an `i32`, which wraps past 2²⁹, and it clips a cubic crossing
/// the canvas by a search whose 0.25 px tolerance f32 can't resolve past
/// ~2²¹. Measured on control points, with strokes padded by their width ×
/// miter limit; the smallest extent seen to panic is ~1.8·10⁶ px.
pub const MAX_SVG_EXTENT: f32 = 524_288.0;

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
pub(crate) struct SvgLoader {
    pub(crate) fonts: SvgFonts,
}

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
        self.fonts.note(&bytes, load_context.path());
        let raster = rasterize_with(&bytes, settings.scale, Limits::DEFAULT, &self.fonts)
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
    /// usvg's conversion would build too much ([`MAX_SVG_WORK`]).
    TooLarge {
        work: u64,
        max: u64,
    },
    ReferenceCycle,
    TooManyReferences {
        max: usize,
    },
    /// A path reaches farther from the image than [`MAX_SVG_EXTENT`].
    TooFarOut {
        extent: f32,
        max: f32,
    },
    /// A value usvg would panic on (UPSTREAM U26), refused before it runs.
    OutOfRange {
        element: &'static str,
        attribute: &'static str,
        value: String,
        /// What the value must be, as the end of a sentence.
        reason: &'static str,
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
            Self::TooLarge { work, max } => write!(
                f,
                "expands too far to convert (estimated work {work}, limit {max}): \
                 reduce its <use> copies, its markers × path vertices, the shapes \
                 inheriting a pattern or gradient, or its arcs' radii"
            ),
            Self::ReferenceCycle => {
                write!(
                    f,
                    "a url(#…)/href reference (or an inherited fill, stroke or marker) \
                     leads back to an element it came from"
                )
            }
            Self::TooManyReferences { max } => {
                write!(f, "more than {max} url(#…)/href references")
            }
            Self::TooFarOut { extent, max } => write!(
                f,
                "draws too far outside the image (a path reaches {extent:.0} px from it at this \
                 size, limit {max:.0}): reduce its coordinates, radii or stroke widths"
            ),
            Self::OutOfRange {
                element,
                attribute,
                value,
                reason,
            } => write!(
                f,
                "<{element}> has an out-of-range {attribute} ({value:?}): {reason}"
            ),
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
    /// The conversion budget when the bytes are parsed too.
    pub work: u64,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        side: MAX_SVG_SIDE,
        cost: MAX_SVG_COST,
        work: MAX_SVG_WORK,
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

/// The fonts SVG `<text>` is drawn with (the `svg-text` feature,
/// [`svg_fonts`](crate::svg_fonts)), shared by the loaders, the inline
/// cache and the system that keeps it current. Without the feature it's
/// empty and `<text>` draws nothing.
#[derive(Resource, Clone, Default)]
pub(crate) struct SvgFonts {
    #[cfg(feature = "svg-text")]
    shared: std::sync::Arc<crate::svg_fonts::Shared>,
}

impl SvgFonts {
    #[cfg(feature = "svg-text")]
    pub(crate) fn shared(&self) -> &crate::svg_fonts::Shared {
        &self.shared
    }

    /// The font set's generation: inline `<svg>` with text keys its drawing
    /// by it (0 without `svg-text`).
    fn generation(&self) -> u64 {
        #[cfg(feature = "svg-text")]
        return self.shared.current().generation;
        #[cfg(not(feature = "svg-text"))]
        0
    }

    /// Notes a loaded file whose text the fonts affect.
    fn note(&self, bytes: &[u8], path: &bevy::asset::AssetPath<'_>) {
        #[cfg(feature = "svg-text")]
        if crate::svg_fonts::has_text(bytes) {
            self.shared.note_text(path.clone_owned());
        }
        #[cfg(not(feature = "svg-text"))]
        let _ = (bytes, path);
    }
}

/// usvg options that resolve no `<image href>`: usvg's default resolver
/// reads any file path the process can (absolute, cwd-relative, `..`) and
/// embeds `data:` URLs (probe report §2). With `svg-text`, `<text>` uses
/// the app's fonts.
fn options(fonts: &SvgFonts) -> usvg::Options<'static> {
    #[cfg_attr(not(feature = "svg-text"), allow(unused_mut))]
    let mut options = usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    #[cfg(feature = "svg-text")]
    fonts.shared.current().apply(&mut options);
    #[cfg(not(feature = "svg-text"))]
    let _ = fonts;
    options
}

/// Parses an SVG the way `usvg::Tree::from_data` does, with one step in
/// between: the XML is parsed once (roxmltree, configured as usvg's
/// `from_str` configures it), [`svg_bounds::bounds`] bounds how deeply usvg
/// would recurse and how much it would build, and only then does usvg
/// convert that same document (`Tree::from_xmltree`). One parse, so the
/// check and usvg can't disagree about the document's structure (a DTD
/// entity expanding into markup, say).
#[cfg(any(test, feature = "fuzzing"))]
pub(crate) fn parse(bytes: &[u8]) -> Result<usvg::Tree, SvgError> {
    parse_with(bytes, &SvgFonts::default())
}

/// [`parse`] with the fonts `<text>` resolves against.
pub(crate) fn parse_with(bytes: &[u8], fonts: &SvgFonts) -> Result<usvg::Tree, SvgError> {
    parse_within(bytes, fonts, MAX_SVG_WORK)
}

/// [`parse_with`] within a conversion budget other than [`MAX_SVG_WORK`].
#[hotpath::measure]
pub(crate) fn parse_within(
    bytes: &[u8],
    fonts: &SvgFonts,
    max_work: u64,
) -> Result<usvg::Tree, SvgError> {
    guarded(|| parse_unguarded(bytes, fonts, max_work))
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

fn parse_unguarded(bytes: &[u8], fonts: &SvgFonts, max_work: u64) -> Result<usvg::Tree, SvgError> {
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
    let bounds = svg_bounds::bounds(&document, svg_bounds::MAX_REFERENCES)?;
    if bounds.depth > svg_bounds::MAX_DEPTH {
        return Err(SvgError::TooDeep {
            depth: bounds.depth,
            max: svg_bounds::MAX_DEPTH,
        });
    }
    if bounds.work > max_work {
        return Err(SvgError::TooLarge {
            work: bounds.work,
            max: max_work,
        });
    }
    usvg::Tree::from_xmltree(&document, &options(fonts)).map_err(SvgError::Parse)
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
#[cfg(any(test, feature = "fuzzing"))]
pub(crate) fn rasterize(bytes: &[u8], scale: f32, limits: Limits) -> Result<Raster, SvgError> {
    rasterize_with(bytes, scale, limits, &SvgFonts::default())
}

/// [`rasterize`] with the fonts `<text>` resolves against.
pub(crate) fn rasterize_with(
    bytes: &[u8],
    scale: f32,
    limits: Limits,
    fonts: &SvgFonts,
) -> Result<Raster, SvgError> {
    let tree = parse_within(bytes, fonts, limits.work)?;
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
#[hotpath::measure]
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
    check_extent(tree, width, height)?;
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
pub(crate) struct SvgDocumentLoader {
    pub(crate) fonts: SvgFonts,
}

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
        self.fonts.note(&bytes, load_context.path());
        let tree = parse_with(&bytes, &self.fonts)
            .map_err(|err| format!("svg {}: {err}", load_context.path()))?;
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

/// Inline `<svg>` documents (ADR 0038 §4), keyed by their prepared source
/// text ([`prepare_inline`]): each distinct text is parsed once and drawn
/// once at its intrinsic size; every element showing it shares the image.
/// The build asks for an image while it collects (`Styler::inline_svg_of`)
/// and gets a handle reserved in `Assets<Image>` plus the intrinsic size,
/// so the inline box is sized in that same build. A cheap document is
/// drawn right away and inserted by [`finish_inline_svgs`] in the same
/// frame (a `:hover` recolor shows no blank frame); a costly or large one
/// draws on the `AsyncComputeTaskPool`, and its insertion is a `Modified`
/// event, which restyles the UI as an `<img>` load does.
#[derive(Resource, Default)]
pub(crate) struct InlineSvgs {
    inner: std::sync::Mutex<InlineInner>,
    fonts: SvgFonts,
}

#[derive(Default)]
struct InlineInner {
    entries: std::collections::HashMap<String, InlineEntry>,
    /// Drawn during the build, inserted by `finish_inline_svgs`.
    ready: Vec<(AssetId<Image>, Image)>,
    drawing: Vec<(
        String,
        bevy::tasks::Task<Result<(Image, std::sync::Arc<usvg::Tree>), String>>,
    )>,
}

struct InlineEntry {
    image: Handle<Image>,
    /// `None` until a large document's task has parsed it.
    intrinsic: Option<Vec2>,
    tree: Option<std::sync::Arc<usvg::Tree>>,
    /// For the display-size redraw (`rasterize_svg_images`).
    document: Option<Handle<SvgDocument>>,
    /// Frames in a row no `InlineImage` showed it.
    unused: u32,
}

/// Inline source text up to this size is parsed during the build; larger
/// text is parsed on a task (the build doesn't stall on it).
const INLINE_SYNC_BYTES: usize = 64 * 1024;
/// A document estimated to cost up to this many units (~1–4 ms, probe
/// report §7) is drawn during the build; a costlier one on a task.
const INLINE_SYNC_COST: u64 = 1 << 20;
/// Frames an unused inline image is kept (a restyle that re-collects the
/// element finds it again); then it's dropped.
const INLINE_KEEP_FRAMES: u32 = 120;

/// An inline `<svg>`'s source as an SVG document: the element's own text
/// (`raw`, as the template rendered it), with what HTML supplies and XML
/// needs added to its start tag: the SVG namespace (and `xlink`'s, when
/// the text uses `xlink:`), and `color` set to the element's computed CSS
/// color, so `currentColor` follows the cascade (`svg { color: … }`,
/// `:hover`, themes). Attributes the author wrote are kept; an author's
/// `color` attribute wins, as in a browser. `None` if `raw` doesn't start
/// with an `<svg` start tag.
pub(crate) fn prepare_inline(raw: &str, color: Option<[u8; 4]>) -> Option<String> {
    let rest = raw.strip_prefix("<svg")?;
    if !rest.starts_with(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/') {
        return None;
    }
    let start_tag = &rest[..rest.find('>')?];
    let has = |name: &str| {
        start_tag
            .split(|c: char| c.is_ascii_whitespace())
            .any(|part| part.split('=').next() == Some(name))
    };
    let mut added = String::new();
    if !has("xmlns") {
        added += r#" xmlns="http://www.w3.org/2000/svg""#;
    }
    if raw.contains("xlink:") && !has("xmlns:xlink") {
        added += r#" xmlns:xlink="http://www.w3.org/1999/xlink""#;
    }
    if let Some([r, g, b, a]) = color
        && !has("color")
    {
        let alpha = f32::from(a) / 255.0;
        added += &format!(" color=\"rgba({r}, {g}, {b}, {alpha})\"");
    }
    Some(format!("<svg{added}{rest}"))
}

impl InlineSvgs {
    /// The image for a prepared inline document and its intrinsic size
    /// (`None` while a large document parses). `None` if it doesn't parse
    /// or is refused (logged once per text at `debug`).
    #[hotpath::measure(impl_type = "InlineSvgs")]
    pub(crate) fn image(
        &self,
        text: String,
        images: &Assets<Image>,
    ) -> Option<(Handle<Image>, Option<Vec2>)> {
        // Text is drawn with the current fonts: a new font set is a new
        // drawing (the `FontFamilies` restyle collects it).
        let key = if cfg!(feature = "svg-text") && text.contains("<text") {
            format!("{}\0{text}", self.fonts.generation())
        } else {
            text.clone()
        };
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(entry) = inner.entries.get_mut(&key) {
            entry.unused = 0;
            return Some((entry.image.clone(), entry.intrinsic));
        }
        let image = images.reserve_handle();
        let mut entry = InlineEntry {
            image: image.clone(),
            intrinsic: None,
            tree: None,
            document: None,
            unused: 0,
        };
        if text.len() <= INLINE_SYNC_BYTES {
            let tree = match parse_with(text.as_bytes(), &self.fonts) {
                Ok(tree) => std::sync::Arc::new(tree),
                Err(err) => {
                    debug!("inline svg skipped: {err}");
                    return None;
                }
            };
            let (width, height, _) = raster_size(&tree, 1.0, MAX_SVG_SIDE).ok()?;
            entry.intrinsic = Some(Vec2::new(width as f32, height as f32));
            // Over the hard budget: refused now, no task, no empty image
            // (the element shows nothing, as a refused `<img>` does).
            let cost = estimate_cost(&tree, width, height, MAX_SVG_COST);
            if cost > MAX_SVG_COST {
                debug!(
                    "inline svg skipped: {}",
                    SvgError::TooExpensive {
                        estimate: cost,
                        max: MAX_SVG_COST
                    }
                );
                return None;
            }
            let cheap = cost <= INLINE_SYNC_COST;
            if cheap {
                match draw(&tree, width, height, Limits::DEFAULT) {
                    Ok((drawn, _)) => inner.ready.push((image.id(), drawn)),
                    Err(err) => {
                        debug!("inline svg skipped: {err}");
                        return None;
                    }
                }
            } else {
                let task_tree = tree.clone();
                let task = bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                    draw(&task_tree, width, height, Limits::DEFAULT)
                        .map(|(image, _)| (image, task_tree))
                        .map_err(|err| err.to_string())
                });
                inner.drawing.push((key.clone(), task));
            }
            entry.tree = Some(tree);
        } else {
            let task_text = text.clone();
            let task_fonts = self.fonts.clone();
            let task = bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                let tree =
                    parse_with(task_text.as_bytes(), &task_fonts).map_err(|err| err.to_string())?;
                let (width, height, _) =
                    raster_size(&tree, 1.0, MAX_SVG_SIDE).map_err(|err| err.to_string())?;
                let tree = std::sync::Arc::new(tree);
                draw(&tree, width, height, Limits::DEFAULT)
                    .map(|(image, _)| (image, tree))
                    .map_err(|err| err.to_string())
            });
            inner.drawing.push((key.clone(), task));
        }
        let intrinsic = entry.intrinsic;
        inner.entries.insert(key, entry);
        Some((image, intrinsic))
    }

    /// A cache whose `<text>` resolves against `fonts`.
    pub(crate) fn new(fonts: SvgFonts) -> Self {
        Self {
            inner: std::sync::Mutex::default(),
            fonts,
        }
    }

    /// The parsed document behind an inline image, for the redraw.
    pub(crate) fn document_of(&self, image: AssetId<Image>) -> Option<Handle<SvgDocument>> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        inner
            .entries
            .values()
            .find(|entry| entry.image.id() == image)
            .and_then(|entry| entry.document.clone())
    }
}

/// After the build: inserts the inline images it drew, finishes the drawing
/// tasks, gives each parsed inline document an [`SvgDocument`] (for the
/// display-size redraw), and drops entries no `InlineImage` has shown for
/// [`INLINE_KEEP_FRAMES`] frames.
#[hotpath::measure]
pub(crate) fn finish_inline_svgs(
    inline: Res<InlineSvgs>,
    mut images: ResMut<Assets<Image>>,
    mut documents: ResMut<Assets<SvgDocument>>,
    spans: Query<(&bevy::ui::widget::InlineImage, Option<&SvgRaster>)>,
) {
    let mut inner = inline
        .inner
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    for (id, image) in std::mem::take(&mut inner.ready) {
        let _ = images.insert(id, image);
    }
    let mut finished = Vec::new();
    inner.drawing.retain_mut(
        |(text, task)| match bevy::tasks::futures::check_ready(task) {
            Some(result) => {
                finished.push((text.clone(), result));
                false
            }
            None => true,
        },
    );
    for (text, result) in finished {
        let Some(entry) = inner.entries.get_mut(&text) else {
            continue;
        };
        match result {
            Ok((image, tree)) => {
                let id = entry.image.id();
                entry.intrinsic = Some(image.size().as_vec2());
                entry.tree = Some(tree);
                let _ = images.insert(id, image);
                // `Modified`, which restyles the UI showing it (its box
                // size wasn't known when the build collected it).
                let _ = images.get_mut(id);
            }
            Err(err) => debug!("inline svg not drawn: {err}"),
        }
    }
    let shown: std::collections::HashSet<AssetId<Image>> = spans
        .iter()
        .flat_map(|(image, raster)| {
            [
                Some(image.image.id()),
                raster.map(|raster| raster.source.id()),
            ]
        })
        .flatten()
        .collect();
    let drawing: std::collections::HashSet<String> =
        inner.drawing.iter().map(|(text, _)| text.clone()).collect();
    for (text, entry) in inner.entries.iter_mut() {
        if entry.document.is_none()
            && let Some(tree) = &entry.tree
        {
            entry.document = Some(documents.add(SvgDocument { tree: tree.clone() }));
        }
        if shown.contains(&entry.image.id()) || drawing.contains(text) {
            entry.unused = 0;
        } else {
            entry.unused += 1;
        }
    }
    inner
        .entries
        .retain(|_, entry| entry.unused <= INLINE_KEEP_FRAMES);
}

/// On an element whose CSS `border-image`/`background-image` is an SVG:
/// the 1× image and mode the build gave it, and the sharper raster shown
/// instead (ADR 0038 §7). Like [`SvgRaster`], put back the same frame the
/// build re-inserts the 1× `ImageNode`.
#[derive(Component)]
pub(crate) struct SvgFrame {
    source: Handle<Image>,
    document: Handle<SvgDocument>,
    mode: NodeImageMode,
    shown: Option<(UVec2, Handle<Image>)>,
    wanted: Option<UVec2>,
}

/// `mode` for an image `scale`× the size it was written for, drawn with the
/// same logical geometry. Bevy reads slice insets in image px both to cut
/// the texture and to size the corners on screen (× `min(node / image,
/// max_corner_scale)`), and tiles at image size × `stretch_value`: insets
/// × scale with the corner cap ÷ scale, and tile stretch ÷ scale, cut the
/// same regions and draw them at the same size.
pub(crate) fn scale_mode(mode: &NodeImageMode, scale: f32) -> NodeImageMode {
    let tile = |mode: SliceScaleMode| match mode {
        SliceScaleMode::Tile { stretch_value } => SliceScaleMode::Tile {
            stretch_value: stretch_value / scale,
        },
        stretch => stretch,
    };
    match mode {
        NodeImageMode::Sliced(slicer) => NodeImageMode::Sliced(TextureSlicer {
            border: BorderRect {
                min_inset: slicer.border.min_inset * scale,
                max_inset: slicer.border.max_inset * scale,
            },
            center_scale_mode: tile(slicer.center_scale_mode),
            sides_scale_mode: tile(slicer.sides_scale_mode),
            max_corner_scale: slicer.max_corner_scale / scale,
        }),
        NodeImageMode::Tiled {
            tile_x,
            tile_y,
            stretch_value,
        } => NodeImageMode::Tiled {
            tile_x: *tile_x,
            tile_y: *tile_y,
            stretch_value: stretch_value / scale,
        },
        other => other.clone(),
    }
}

/// Where a span's [`SvgDocument`] comes from.
enum DocumentSource {
    Path(bevy::asset::AssetPath<'static>),
    Inline(Handle<SvgDocument>),
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
#[hotpath::measure]
pub(crate) fn rasterize_svg_images(
    mut commands: Commands,
    server: Res<AssetServer>,
    inline: Res<InlineSvgs>,
    mut images: ResMut<Assets<Image>>,
    documents: Res<Assets<SvgDocument>>,
    mut document_events: MessageReader<AssetEvent<SvgDocument>>,
    mut rasters: ResMut<SvgRasters>,
    mut spans: Query<(
        Entity,
        &ChildOf,
        &mut bevy::ui::widget::InlineImage,
        &bevy::text::InlineBox,
        Option<&mut SvgRaster>,
    )>,
    blocks: Query<&ComputedNode>,
    mut frames: Query<
        (
            Entity,
            &mut ImageNode,
            &ComputedNode,
            Option<&mut SvgFrame>,
            Option<&crate::build::CssRoot>,
        ),
        Or<(With<crate::build::CssFrame>, With<crate::build::CssRoot>)>,
    >,
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

    // A document reloaded (new fonts for its `<text>`, or the file
    // changed): its rasters are stale.
    let reloaded: std::collections::HashSet<AssetId<SvgDocument>> = document_events
        .read()
        .filter_map(|event| match event {
            AssetEvent::Modified { id } => Some(*id),
            _ => None,
        })
        .collect();
    if !reloaded.is_empty() {
        rasters.ready.retain(|key, _| !reloaded.contains(&key.0));
        rasters.refused.retain(|key| !reloaded.contains(&key.0));
    }

    let mut in_use = std::collections::HashSet::new();
    for (entity, parent, mut image, inline_box, mut raster) in &mut spans {
        if let Some(raster) = raster.as_mut()
            && reloaded.contains(&raster.document.id())
            && raster.shown.is_some()
        {
            // Back to the (also reloaded) 1x image until the redraw.
            raster.shown = None;
            raster.wanted = None;
            image.image = raster.source.clone();
        }
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
        // An `<img src="….svg">` (its document loads by the same path), or
        // an inline `<svg>` (its document is the cache's).
        let document = match source.path() {
            Some(path)
                if path
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("svg")) =>
            {
                Some(DocumentSource::Path(path.clone_owned()))
            }
            Some(_) => None,
            None => inline.document_of(source.id()).map(DocumentSource::Inline),
        };
        let Some(document) = document else {
            if raster.is_some() {
                commands.entity(entity).remove::<SvgRaster>();
            }
            continue;
        };
        let Some(intrinsic) = images.get(&source).map(|image| image.size().as_vec2()) else {
            continue;
        };
        let mut raster = match raster {
            Some(raster) if raster.source == source => raster,
            _ => {
                // A new `<img>`, or its `src` changed: start over.
                let document = match document {
                    DocumentSource::Path(path) => server.load(path),
                    DocumentSource::Inline(document) => document,
                };
                commands.entity(entity).insert(SvgRaster {
                    source,
                    document,
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
    // CSS `border-image`/`background-image` SVGs (ADR 0038 §7): drawn at
    // their intrinsic size × the node's scale factor (a sliced or tiled
    // image keeps its logical geometry: the mode's insets, corner cap and
    // tile size are scaled to match), or at the node's pixels when
    // stretched over it.
    for (entity, mut node_image, node, frame, root) in &mut frames {
        if root.is_some_and(|root| !root.owns_image()) {
            continue;
        }
        let shown_handle = frame
            .as_ref()
            .and_then(|frame| frame.shown.as_ref().map(|(_, handle)| handle.clone()));
        let (source, mode) = match &frame {
            Some(frame) if shown_handle.as_ref() == Some(&node_image.image) => {
                (frame.source.clone(), frame.mode.clone())
            }
            _ => (node_image.image.clone(), node_image.image_mode.clone()),
        };
        let Some(path) = source.path().filter(|path| {
            path.path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
        }) else {
            if frame.is_some() {
                commands.entity(entity).remove::<SvgFrame>();
            }
            continue;
        };
        let Some(intrinsic) = images.get(&source).map(|image| image.size().as_vec2()) else {
            continue;
        };
        let mut frame = match frame {
            Some(frame) if frame.source == source => frame,
            _ => {
                commands.entity(entity).insert(SvgFrame {
                    document: server.load(path.clone_owned()),
                    source,
                    mode,
                    shown: None,
                    wanted: None,
                });
                continue;
            }
        };
        // The build re-inserts the 1x image on an update: keep its mode.
        if node_image.image == frame.source {
            frame.mode = mode.clone();
        }
        let scale = 1.0 / node.inverse_scale_factor().max(f32::EPSILON);
        let target = match mode {
            NodeImageMode::Stretch | NodeImageMode::Auto => node.size(),
            _ => intrinsic * scale,
        };
        let target = UVec2::new(
            (target.x.ceil() as u32).clamp(1, MAX_SVG_SIDE),
            (target.y.ceil() as u32).clamp(1, MAX_SVG_SIDE),
        );
        let document = frame.document.id();
        if target == intrinsic.as_uvec2() || !target.cmpgt(UVec2::ZERO).all() {
            if frame.shown.take().is_some() {
                node_image.image = frame.source.clone();
                node_image.image_mode = frame.mode.clone();
            }
            continue;
        }
        if let Some((size, shown)) = frame.shown.clone() {
            in_use.insert((document, size));
            let scaled = scale_mode(&frame.mode, size.x as f32 / intrinsic.x);
            if node_image.image != shown || node_image.image_mode != scaled {
                node_image.image = shown;
                node_image.image_mode = scaled;
            }
            if size == target {
                continue;
            }
        }
        let key = (document, target);
        in_use.insert(key);
        if frame.wanted != Some(target) {
            frame.wanted = Some(target);
            continue;
        }
        if let Some(handle) = rasters.ready.get(&key).cloned() {
            node_image.image = handle.clone();
            node_image.image_mode = scale_mode(&frame.mode, target.x as f32 / intrinsic.x);
            frame.shown = Some((target, handle));
        } else if !rasters.refused.contains(&key) && !rasters.drawing.contains_key(&key) {
            let Some(document) = documents.get(&frame.document) else {
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
#[hotpath::measure]
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

/// Refuses a tree with a path reaching farther than [`MAX_SVG_EXTENT`]
/// pixels from the drawn image (tiny-skia panics on such paths).
fn check_extent(tree: &usvg::Tree, width: u32, height: u32) -> Result<(), SvgError> {
    let size = tree.size();
    let canvas = tiny_skia::Transform::from_scale(
        width as f32 / size.width(),
        height as f32 / size.height(),
    );
    let mut walk = ExtentWalk { extent: 0.0 };
    match walk.group(tree.root(), canvas, 0) {
        Ok(()) => Ok(()),
        Err(Over) => Err(SvgError::TooFarOut {
            extent: walk.extent,
            max: MAX_SVG_EXTENT,
        }),
    }
}

/// Walks the tree as resvg draws it, composing each group's transform on
/// the way down, and records how far each path reaches in pixels.
struct ExtentWalk {
    extent: f32,
}

impl ExtentWalk {
    fn group(
        &mut self,
        group: &usvg::Group,
        parent: tiny_skia::Transform,
        depth: u32,
    ) -> Result<(), Over> {
        if depth > MAX_COST_DEPTH {
            self.extent = f32::INFINITY;
            return Err(Over);
        }
        let transform = parent.pre_concat(group.transform());
        // `clip::apply`: the clip's own transform on top; nested clips get
        // the group's.
        let mut clip = group.clip_path();
        while let Some(path) = clip {
            self.group(
                path.root(),
                transform.pre_concat(path.transform()),
                depth + 1,
            )?;
            clip = path.clip_path();
        }
        // `mask::apply`: drawn with the group's transform.
        let mut mask = group.mask();
        while let Some(layer) = mask {
            self.group(layer.root(), transform, depth + 1)?;
            mask = layer.mask();
        }
        // `feImage` content: drawn at the transform's scale in the filter
        // region.
        for filter in group.filters() {
            for primitive in filter.primitives() {
                if let usvg::filter::Kind::Image(image) = primitive.kind() {
                    self.group(image.root(), scale_of(transform), depth + 1)?;
                }
            }
        }
        for child in group.children() {
            match child {
                usvg::Node::Group(child) => self.group(child, transform, depth + 1)?,
                usvg::Node::Path(path) => self.path(path, transform, depth)?,
                // Drawn as its flattened outlines.
                usvg::Node::Text(text) => self.group(text.flattened(), transform, depth + 1)?,
                // Never produced with the refusing resolver.
                usvg::Node::Image(_) => {}
            }
        }
        Ok(())
    }

    fn path(
        &mut self,
        path: &usvg::Path,
        transform: tiny_skia::Transform,
        depth: u32,
    ) -> Result<(), Over> {
        let pad = path.stroke().map_or(0.0, |stroke| {
            stroke.width().get() * 0.5 * stroke.miterlimit().get().max(std::f32::consts::SQRT_2)
        });
        let bounds = path.data().bounds();
        let reach = tiny_skia::Rect::from_ltrb(
            bounds.left() - pad,
            bounds.top() - pad,
            bounds.right() + pad,
            bounds.bottom() + pad,
        )
        .and_then(|rect| rect.transform(transform))
        .map_or(f32::INFINITY, |rect| {
            rect.left()
                .abs()
                .max(rect.top().abs())
                .max(rect.right().abs())
                .max(rect.bottom().abs())
        });
        // `!(<=)`: NaN is too far too.
        if !(reach <= MAX_SVG_EXTENT) {
            self.extent = reach;
            return Err(Over);
        }
        self.extent = self.extent.max(reach);
        // `render_pattern_pixmap`: the tile is drawn at the scale of the
        // path's transform and the pattern's.
        let fill = path.fill().map(|fill| fill.paint());
        let stroke = path.stroke().map(|stroke| stroke.paint());
        for paint in fill.into_iter().chain(stroke) {
            if let usvg::Paint::Pattern(pattern) = paint {
                let tile = scale_of(transform.pre_concat(pattern.transform()));
                self.group(pattern.root(), tile, depth + 1)?;
            }
        }
        Ok(())
    }
}

/// The scale part of a transform, as resvg takes it for tiles and filter
/// images.
fn scale_of(transform: tiny_skia::Transform) -> tiny_skia::Transform {
    let (sx, sy) = transform.get_scale();
    tiny_skia::Transform::from_scale(sx, sy)
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

    /// What an inline `<svg>` needs before XML parses it (ADR 0038 §4).
    #[test]
    fn inline_text_gets_namespaces_and_the_cascade_color() {
        let red = Some([255, 0, 0, 255]);
        assert_eq!(
            prepare_inline("<svg width=\"8\"><rect/></svg>", red).unwrap(),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" color=\"rgba(255, 0, 0, 1)\" width=\"8\"><rect/></svg>"
        );
        // The author's namespace and color stay; nothing is added twice.
        assert_eq!(
            prepare_inline(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" color=\"blue\"/>",
                red
            )
            .unwrap(),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" color=\"blue\"/>"
        );
        // `xlink:` in the text needs its namespace declared.
        let with_xlink = prepare_inline("<svg><use xlink:href=\"#a\"/></svg>", None).unwrap();
        assert!(
            with_xlink.contains("xmlns:xlink=\"http://www.w3.org/1999/xlink\""),
            "{with_xlink}"
        );
        assert!(parse(with_xlink.as_bytes()).is_ok());
        // Only an `<svg` start tag.
        assert!(prepare_inline("<svgx>", red).is_none());
        assert!(prepare_inline("<p>", red).is_none());
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
        // 10⁷ rects: refused by the work bound before usvg's own node
        // limit (a million) would see them.
        assert!(matches!(
            draw(&svg(100, 100, &uses)),
            Err(SvgError::TooLarge { .. })
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
        svg_bounds::bounds(&document, svg_bounds::MAX_REFERENCES).map(|bounds| bounds.depth)
    }

    /// The deepest stack of `pick(i)` levels the check accepts, and the
    /// first it refuses.
    fn deepest_accepted(pick: impl Fn(usize) -> Level) -> (Vec<Level>, String) {
        let mut levels = Vec::new();
        loop {
            levels.push(pick(levels.len()));
            let source = nest(&levels);
            if matches!(depth_of(&source), Ok(depth) if depth > svg_bounds::MAX_DEPTH) {
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
                levels.len() > 1
                    && depth <= svg_bounds::MAX_DEPTH
                    && depth > svg_bounds::MAX_DEPTH / 2,
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

    /// The depth and work bounds match `<style>` rules with simplecss and
    /// walk path data with svgtypes as usvg does, so both must use the same
    /// copies. usvg keeps it
    /// private (no type to check at compile time, unlike roxmltree, which
    /// comes from usvg's re-export, and cssparser, ADR 0039), so the
    /// lockfile is checked instead: one simplecss, the one usvg resolves.
    /// Both ask for `0.2`, so Cargo resolves one copy for any app; a split
    /// needs usvg to move to another simplecss, which arrives with a resvg
    /// bump here, and fails this test: bump `simplecss` in `Cargo.toml`.
    #[test]
    fn usvg_and_the_depth_check_share_one_simplecss_roxmltree_and_svgtypes() {
        let lock = include_str!("../Cargo.lock");
        for name in ["simplecss", "roxmltree", "svgtypes"] {
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

    fn too_large(source: &str) -> bool {
        matches!(parse(source.as_bytes()), Err(SvgError::TooLarge { work, max }) if work > max)
    }

    /// bug_0188: `<use>` copies multiply per level. The nightly fuzz run's
    /// input (minimized): 10 `<use>` per level, 5 levels, 10⁵ rects from
    /// under 1 KB — 0.7 s of release-build conversion before the render
    /// budget could refuse it, under usvg's million-node limit.
    #[test]
    fn use_fan_out_is_refused_before_conversion() {
        // `levels` levels of ten `<use>`s each. usvg copies `<use>` content
        // wherever it is, `<defs>` included: every level counts.
        let fan_out = |levels: usize| {
            let mut uses = String::from("<defs><g id='g0'><rect width='1' height='1'/></g>");
            for i in 1..=levels {
                uses += &format!(
                    "<g id='g{i}'>{}</g>",
                    format!("<use href='#g{}'/>", i - 1).repeat(10)
                );
            }
            svg(64, 64, &format!("{uses}</defs><use href='#g{levels}'/>"))
        };
        let source = fan_out(5);
        assert!(source.len() < 2048);
        assert!(too_large(&source));
        // Three levels (10³ rects) convert.
        parse(fan_out(3).as_bytes()).unwrap();
    }

    /// bug_0189: a marker is converted once per vertex of each path that
    /// carries it, including through inheritance. 44 KB: 4·10⁶ rects (over
    /// two minutes of conversion before the fix).
    #[test]
    fn markers_count_per_vertex() {
        let marker = format!(
            "<defs><marker id='m'>{}</marker></defs>",
            "<rect width='1' height='1'/>".repeat(1000)
        );
        let vertices = " 1 1".repeat(4000);
        let direct = svg(
            64,
            64,
            &format!("{marker}<path marker-mid='url(#m)' d='M0 0{vertices}'/>"),
        );
        assert!(direct.len() < 50_000);
        assert!(too_large(&direct));
        // Inherited from a group, and through a `<style>` rule.
        let inherited = svg(
            64,
            64,
            &format!("{marker}<g marker-mid='url(#m)'><path d='M0 0{vertices}'/></g>"),
        );
        assert!(too_large(&inherited));
        let styled = svg(
            64,
            64,
            &format!(
                "<style>g {{ marker-mid: url(#m) }}</style>{marker}<g><path d='M0 0{vertices}'/></g>"
            ),
        );
        assert!(too_large(&styled));
        // Through a `<use>` whose content inherits it.
        let used = svg(
            64,
            64,
            &format!(
                "{marker}<defs><path id='p' d='M0 0{vertices}'/></defs><use href='#p' marker-mid='url(#m)'/>"
            ),
        );
        assert!(too_large(&used));
        // An arrowhead on a few hundred lines converts.
        let arrows = svg(
            64,
            64,
            &format!(
                "<defs><marker id='a'><path d='M0 0 L10 5 L0 10 Z'/></marker></defs><g marker-end='url(#a)'>{}</g>",
                "<line x1='0' y1='0' x2='10' y2='10'/><polyline points='0 0 5 5 10 0'/>"
                    .repeat(300)
            ),
        );
        parse(arrows.as_bytes()).unwrap();
    }

    /// bug_0190: kurbo splits an arc into more cubics the larger its
    /// radius (`(11·r)^⅙` per turn), and svgtypes buffers them with a
    /// quadratic drain: one 40-byte arc took 4 s, a larger one never
    /// finished. Shapes take the same path through their radius.
    #[test]
    fn huge_arcs_are_refused() {
        for body in [
            "<path d='M0 0 A1e30 1e30 0 1 1 1e30 0'/>",
            "<path d='M0 0 a1e30 1e30 0 1 1 1e30 0'/>",
            "<path d='M0 0 A3e38 3e38 0 1 1 3e38 0'/>",
            // The radius scaled up to reach the endpoint (`rx` tiny).
            "<path d='M0 0 A1e-3 1 0 1 1 1e27 0'/>",
            "<circle r='3e38'/>",
            "<ellipse rx='1' ry='3e38'/>",
            "<rect width='1e38' height='1e38' rx='1e38'/>",
            // Lengths resolved through font sizes.
            "<g font-size='1e20'><g font-size='1e15em'><circle r='1e3em'/></g></g>",
            "<svg viewBox='0 0 1e38 1e38'><circle r='50%'/></svg>",
        ] {
            assert!(too_large(&svg(64, 64, body)), "{body}");
        }
        // Ordinary radii convert, `%` and `em` included.
        for body in [
            "<path d='M10 10 A20 20 0 1 1 30 30 a5 8 30 0 0 4 4 Z'/>",
            "<circle r='50%'/><ellipse rx='2em' ry='3em'/><rect width='40' height='40' rx='4mm'/>",
            "<svg viewBox='0 0 1e6 1e6'><circle r='40%'/></svg>",
        ] {
            parse(svg(64, 64, body).as_bytes()).unwrap();
        }
    }

    /// bug_0191: a pattern whose content inherits a `fill` naming the
    /// pattern itself made usvg recurse until the stack overflowed (a
    /// process abort, not a panic). The inherited reference is an edge of
    /// the reference graph, so it's a cycle. A marker inheriting itself is
    /// refused the same way (usvg skips it, but the check doesn't model
    /// that: the CSS spelling has always been refused).
    #[test]
    fn inherited_self_references_are_refused() {
        for body in [
            "<g fill='url(#p)'><pattern id='p' width='10' height='10' patternUnits='userSpaceOnUse'><rect width='5' height='5'/></pattern><rect width='64' height='64'/></g>",
            "<g style='stroke: url(#p)'><defs><pattern id='p' width='10' height='10'><g><circle r='3'/></g></pattern></defs></g>",
            "<g marker-mid='url(#m)'><marker id='m'><path d='M0 0 L1 1 L2 0'/></marker><path d='M0 0 L10 10 L20 0'/></g>",
        ] {
            assert!(
                matches!(
                    parse(svg(64, 64, body).as_bytes()),
                    Err(SvgError::ReferenceCycle)
                ),
                "{body}"
            );
        }
        // A pattern under a group filled with something else converts.
        parse(
            svg(
                64,
                64,
                "<g fill='url(#g)'><linearGradient id='g'><stop offset='0'/></linearGradient><pattern id='p' width='10' height='10'><rect width='5' height='5'/></pattern><rect width='64' height='64' fill='url(#p)'/></g>",
            )
            .as_bytes(),
        )
        .unwrap();
    }

    /// tiny-skia 0.12 panics, in release builds too, on paths far outside
    /// its canvas (UPSTREAM U24, U27, U28): these all panicked before the
    /// extent check and are refused by it now.
    #[test]
    fn paths_far_outside_the_image_are_refused() {
        let far = |body: &str| {
            matches!(
                rasterize(svg(64, 64, body).as_bytes(), 1.0, Limits::DEFAULT),
                Err(SvgError::TooFarOut { .. })
            )
        };
        for body in [
            // U24: the anti-aliased row range wraps past 2²⁹.
            "<path d='M0 0 A1e9 1e9 0 1 1 1e9 0 A1e9 1e9 0 1 1 0 0'/>",
            "<path d='M603553400 -786566100 L-232962910 303603170 L554380750 -829689350 Z'/>",
            // U27: a cubic crossing the canvas at ~2·10⁶ px.
            "<path d='M594013.050 381118.560 C23.441 49.273 26.433 1484859.225 1774116.403 0.411 C1298853.611 -1384415.656 16.117 54.780 -1023737.272 -1796567.757 C61.380 -457240.839 35.599 63694.954 1712724.296 3.251' shape-rendering='crispEdges' stroke='red' stroke-width='3'/>",
            // A stroke's width counts, and so do transforms.
            "<path d='M0 0 L64 64' stroke='red' stroke-width='2e6'/>",
            "<g transform='scale(1e6)'><rect width='1' height='1'/></g>",
            // Pattern tiles, clip paths and text outlines are walked too.
            "<pattern id='p' width='8' height='8' patternUnits='userSpaceOnUse'><path d='M0 0 L1e7 1e7 L0 1e7 Z'/></pattern><rect width='64' height='64' fill='url(#p)'/>",
            "<clipPath id='c'><path d='M0 0 L1e7 1e7 L0 1e7 Z'/></clipPath><rect width='64' height='64' clip-path='url(#c)'/>",
        ] {
            assert!(far(body), "{body}");
        }
        // The limit is in image pixels: a viewBox that scales the drawing
        // down lets user units grow, and shapes may pass the edge.
        draw(
            "<svg xmlns='http://www.w3.org/2000/svg' width='64' height='64' viewBox='0 0 1e7 1e7'><path d='M0 0 L1e7 0 L0 1e7 Z'/></svg>",
        )
        .unwrap();
        draw(&svg(
            64,
            64,
            "<rect x='-100000' y='-100000' width='200000' height='200000' fill='red'/><circle cx='32' cy='32' r='5000' stroke='blue' stroke-width='40'/>",
        ))
        .unwrap();
    }

    /// usvg 0.49 unwraps a `PositiveF32` made from `baseFrequency` as an
    /// `f32`: a number past `f32::MAX` is infinity and panicked, in release
    /// too (UPSTREAM U26). The check refuses it before usvg runs.
    #[test]
    fn infinite_turbulence_frequencies_are_refused() {
        for frequency in ["1e40", "0.1 1e39", "-0 1e40"] {
            let body = format!(
                "<filter id='f'><feTurbulence baseFrequency='{frequency}'/></filter><rect width='8' height='8' filter='url(#f)'/>"
            );
            assert!(
                matches!(
                    rasterize(svg(64, 64, &body).as_bytes(), 1.0, Limits::DEFAULT),
                    Err(SvgError::OutOfRange { .. })
                ),
                "{frequency}"
            );
        }
        // Finite frequencies convert and draw. A number past `f64` fails
        // svgtypes' parse, and usvg drops the attribute.
        let draw = |frequency: &str| {
            let body = format!(
                "<filter id='f'><feTurbulence baseFrequency='{frequency}'/></filter><rect width='8' height='8' filter='url(#f)'/>"
            );
            rasterize(svg(64, 64, &body).as_bytes(), 1.0, Limits::DEFAULT)
        };
        for frequency in ["0.05", "1e6", "1e400"] {
            draw(frequency).unwrap_or_else(|err| panic!("{frequency}: {err}"));
        }
        // Huge finite ones overflow resvg's lattice index: a debug-build
        // panic (UPSTREAM U20), caught by `guarded`; release wraps and masks
        // it into range. A resvg fix turns this arm into `Ok`.
        for frequency in ["1e20", "3e38 0.1"] {
            match draw(frequency) {
                Err(SvgError::Panicked(message)) if cfg!(debug_assertions) => {
                    assert!(message.contains("with overflow"), "{message}")
                }
                result => assert!(result.is_ok(), "{frequency}: {:?}", result.err()),
            }
        }
    }

    /// usvg 0.49 multiplies `feConvolveMatrix`'s `order` sides as `u32`
    /// unchecked: a debug-build panic, a wrapped kernel size in release
    /// (UPSTREAM U29). The check refuses a product past `u32` before usvg
    /// runs, reading the sides as usvg does.
    #[test]
    fn convolve_orders_past_u32_are_refused() {
        let draw = |order: &str| {
            let body = format!(
                "<filter id='f'><feConvolveMatrix order='{order}'/></filter><rect width='8' height='8' filter='url(#f)'/>"
            );
            rasterize(svg(64, 64, &body).as_bytes(), 1.0, Limits::DEFAULT)
        };
        // The fuzz find's order saturates to `i32::MAX` on both sides.
        for order in [
            "65536",
            "70000 70000",
            "1e10 3",
            "5555555555555555555555555555555553",
        ] {
            assert!(
                matches!(draw(order), Err(SvgError::OutOfRange { .. })),
                "{order}"
            );
        }
        // A product that fits, a side usvg replaces with 3, a bad list:
        // usvg drops the primitive (no matching kernel) or draws.
        for order in ["3", "5 5", "65535 65537", "-1 1e10", "0", "x 70000"] {
            let result = draw(order);
            assert!(
                !matches!(
                    result,
                    Err(SvgError::OutOfRange { .. } | SvgError::Panicked(_))
                ),
                "{order}: {:?}",
                result.err()
            );
        }
    }

    /// usvg 0.49 unwraps a `PositiveF32` of each `feMorphology` radius ×
    /// the primitive scale, which panics in every build when it isn't
    /// finite (UPSTREAM U30). The check refuses such radii before usvg runs.
    #[test]
    fn morphology_radii_that_overflow_are_refused() {
        let draw = |units: &str, radius: &str| {
            let body = format!(
                "<filter id='f' {units}><feMorphology radius='{radius}'/></filter><rect width='8' height='8' filter='url(#f)'/>"
            );
            rasterize(svg(64, 64, &body).as_bytes(), 1.0, Limits::DEFAULT)
        };
        let bbox = "primitiveUnits='objectBoundingBox'";
        // The fuzz find's radius; one infinite side; past 1 in box units.
        for (units, radius) in [
            ("", "1e49"),
            ("", "2 1e39"),
            (bbox, "2"),
            (bbox, "0.5 1e39"),
        ] {
            assert!(
                matches!(draw(units, radius), Err(SvgError::OutOfRange { .. })),
                "{units} {radius}"
            );
        }
        // Finite radii, a negative side (usvg skips the scaling), a bad or
        // three-number list (usvg's defaults), box units up to 1.
        for (units, radius) in [
            ("", "3"),
            ("", "1e30 2"),
            ("", "-1 1e49"),
            ("", "x 1e49"),
            ("", "1e49 1e49 1e49"),
            (bbox, "0.1 1"),
        ] {
            let result = draw(units, radius);
            assert!(
                !matches!(
                    result,
                    Err(SvgError::OutOfRange { .. } | SvgError::Panicked(_))
                ),
                "{units} {radius}: {:?}",
                result.err()
            );
        }
    }

    /// Ordinary illustrations stay far inside the work budget: the bound
    /// over-approximates, but not by enough to refuse real content.
    #[test]
    fn ordinary_documents_fit_the_work_budget() {
        let work = |body: &str| {
            let source = svg(512, 512, body);
            let document = roxmltree::Document::parse(&source).unwrap();
            svg_bounds::bounds(&document, svg_bounds::MAX_REFERENCES)
                .unwrap()
                .work
        };
        let gradient = "<linearGradient id='g'><stop offset='0' stop-color='red'/><stop offset='0.5' stop-color='gold'/><stop offset='1' stop-color='blue'/></linearGradient>";
        let arrow =
            "<marker id='a' markerWidth='6' markerHeight='6'><path d='M0 0 L6 3 L0 6 Z'/></marker>";
        let icon = "<symbol id='i' viewBox='0 0 24 24'><path d='M12 2 L22 12 L12 22 L2 12 Z'/><circle cx='12' cy='12' r='4'/></symbol>";
        let path: String = (0..200)
            .map(|i| format!(" L{} {}", i % 37, i % 53))
            .collect();
        let cases = [
            (
                "map",
                format!(
                    "<defs>{gradient}</defs><g fill='url(#g)' stroke='#333'>{}</g>",
                    format!("<path d='M0 0{path} Z'/>").repeat(200)
                ),
            ),
            (
                "diagram",
                format!(
                    "<defs>{arrow}</defs><g marker-end='url(#a)' stroke='black'>{}</g>",
                    "<line x1='0' y1='0' x2='100' y2='40'/><path d='M0 0 C20 40 60 40 80 0'/>"
                        .repeat(500)
                ),
            ),
            (
                "icons",
                format!(
                    "<defs>{icon}</defs>{}",
                    "<use href='#i' width='24' height='24'/>".repeat(2000)
                ),
            ),
            (
                "rounded",
                "<rect width='100%' height='100%' rx='2em'/><circle r='45%'/>".repeat(1000),
            ),
        ];
        for (name, body) in cases {
            let work = work(&body);
            assert!(
                work < MAX_SVG_WORK / 4,
                "{name}: work {work} of {MAX_SVG_WORK}"
            );
        }
    }
}
