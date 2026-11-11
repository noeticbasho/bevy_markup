//! `<canvas>`: a box showing a render target the app registers by name
//! (ADR 0041).
//!
//! ```html
//! <canvas data-canvas="portrait" width="256" height="384"></canvas>
//! ```
//!
//! ```no_run
//! # use bevy::prelude::*;
//! # use bevy::camera::RenderTarget;
//! # use bevy::render::render_resource::TextureFormat;
//! # use bevy_markup::prelude::*;
//! fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut canvases: ResMut<HtmlCanvases>) {
//!     let image = images.add(Image::new_target_texture(256, 384, TextureFormat::Bgra8UnormSrgb, None));
//!     let camera = commands
//!         .spawn((Camera3d::default(), RenderTarget::Image(image.into())))
//!         .id();
//!     canvases.insert("portrait", CanvasSource::Camera(camera));
//! }
//! ```
//!
//! The element is a leaf node like a container with no children (its
//! content is fallback for browsers without canvas support, never shown).
//! Its `width`/`height` attributes (CSS px, default 300 × 150) are its size
//! unless CSS declares one; with one side declared, the other keeps the
//! attributes' ratio.
//!
//! `sync_canvases` binds each canvas every frame, after layout:
//!
//! - [`CanvasSource::Camera`]: Bevy's [`ViewportNode`]. Bevy resizes the
//!   camera's image to the box's physical size and forwards pointer input
//!   over the box into the camera's scene (`viewport_picking`). One camera
//!   resized by two boxes would flip between their sizes, so only one canvas
//!   per camera gets the viewport (the one that already has it, else the
//!   first found); the others show the camera's image stretched.
//! - [`CanvasSource::Image`]: an [`ImageNode`] stretched to the box. Never
//!   resized: the app chose its size.
//!
//! Bevy's resize has no limit, and a target image always has CPU data, so a
//! template asking for a 100 000 px canvas would allocate tens of GB and
//! abort. A box larger than [`MAX_CANVAS_SIDE`] physical px on a side gets
//! no camera (Bevy's resize skips it) and logs a warning once.

use std::collections::{HashMap, HashSet};

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::ui::widget::{NodeImageMode, ViewportNode};

use crate::html::HtmlElement;

/// The largest canvas side a camera renders at, in physical px (a 4096²
/// RGBA target is 64 MB; ADR 0038's `MAX_SVG_SIDE` is the same).
pub const MAX_CANVAS_SIDE: f32 = 4096.0;

/// HTML's default canvas size.
const DEFAULT_SIZE: (f32, f32) = (300.0, 150.0);

/// What a named canvas shows.
#[derive(Clone, Debug, PartialEq)]
pub enum CanvasSource {
    /// A camera rendering into an image (`RenderTarget::Image`): shown live,
    /// resized to the box, with pointer input forwarded into its scene.
    Camera(Entity),
    /// An image, stretched to the box (a render target the app sizes
    /// itself, a CPU-drawn image, a video frame).
    Image(Handle<Image>),
}

/// The app's canvas sources by name: `<canvas data-canvas="name">` shows
/// the source registered under `name`. Registering, replacing or removing
/// one rebinds the canvases without rebuilding any UI.
#[derive(Resource, Default, Debug)]
pub struct HtmlCanvases {
    sources: HashMap<String, CanvasSource>,
    /// Names shown at the last `sync_canvases` (written without change
    /// detection: it changes every frame a canvas comes or goes).
    shown: HashSet<String>,
}

impl HtmlCanvases {
    /// Registers `source` under `name`, returning the one it replaces.
    pub fn insert(
        &mut self,
        name: impl Into<String>,
        source: CanvasSource,
    ) -> Option<CanvasSource> {
        self.sources.insert(name.into(), source)
    }

    /// Unregisters `name`: its canvases show nothing.
    pub fn remove(&mut self, name: &str) -> Option<CanvasSource> {
        self.sources.remove(name)
    }

    /// The source registered under `name`.
    pub fn get(&self, name: &str) -> Option<&CanvasSource> {
        self.sources.get(name)
    }

    /// Whether any canvas named `name` was visible (inherited visibility,
    /// laid out at a non-zero size) at the last update. A camera keeps
    /// rendering while nothing shows it; turn it off with this
    /// (`Camera::is_active`) where the last frame needn't be kept.
    pub fn shown(&self, name: &str) -> bool {
        self.shown.contains(name)
    }
}

/// On every `<canvas>` element: what `sync_canvases` attached.
#[derive(Component, Debug, Default, PartialEq)]
pub struct HtmlCanvas {
    /// The `data-canvas` name.
    pub name: Option<String>,
    /// What the canvas shows now.
    pub showing: CanvasShowing,
}

/// What a canvas shows, for [`HtmlCanvas::showing`].
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CanvasShowing {
    /// Nothing: no name, no source under it, or a camera canvas larger than
    /// [`MAX_CANVAS_SIDE`].
    #[default]
    Nothing,
    /// The camera, live ([`ViewportNode`]).
    Viewport(Entity),
    /// An image, stretched: an image source, or the image of a camera whose
    /// viewport another canvas has.
    Image(Handle<Image>),
}

/// Sizes a canvas's layout from its `width`/`height` attributes where CSS
/// didn't declare one (HTML's intrinsic size; invalid or missing values
/// are the defaults).
pub(crate) fn intrinsic_size(tag: &tl::HTMLTag, layout: &mut crate::cascade::LayoutDecl) {
    let attribute = |name: &str, default: f32| {
        tag.attributes()
            .get(name)
            .flatten()
            .and_then(|value| dimension(&crate::template::decode_attribute(value.as_utf8_str())))
            .map_or(default, |value| value as f32)
    };
    let width = attribute("width", DEFAULT_SIZE.0);
    let height = attribute("height", DEFAULT_SIZE.1);
    match (layout.width.is_some(), layout.height.is_some()) {
        (false, false) => {
            layout.width = Some(Val::Px(width));
            layout.height = Some(Val::Px(height));
        }
        // One side declared: the other follows the ratio (a zero side has
        // none to keep).
        (true, false) | (false, true) if width > 0.0 && height > 0.0 => {
            layout.aspect_ratio = Some(width / height);
        }
        _ => {}
    }
}

/// A canvas `width`/`height` value by HTML's rules for parsing non-negative
/// integers: leading ASCII whitespace skipped, an optional sign, then the
/// digits up to the first non-digit (`"256px"` is 256, `"12.5"` is 12, `"-0"`
/// is 0). The attributes reflect as `unsigned long`, limited to
/// `0..=i32::MAX`; anything else (no digits, negative, too large) is `None`:
/// the default size.
pub(crate) fn dimension(value: &str) -> Option<u32> {
    let rest = value.trim_start_matches(['\t', '\n', '\x0C', '\r', ' ']);
    let (negative, rest) = match rest.as_bytes().first() {
        Some(b'-') => (true, &rest[1..]),
        Some(b'+') => (false, &rest[1..]),
        _ => (false, rest),
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    // Leading zeros don't count toward the size; past 10 significant
    // digits it's out of range anyway.
    let significant = rest[..digits].trim_start_matches('0');
    if significant.len() > 10 {
        return None;
    }
    let number: u64 = significant.parse().unwrap_or(0);
    if negative && number != 0 {
        return None;
    }
    u32::try_from(number)
        .ok()
        .filter(|&number| number <= i32::MAX as u32)
}

/// Marks a spawned `<canvas>` element with [`HtmlCanvas`].
pub(crate) fn mark_canvas(
    add: On<Add<HtmlElement>>,
    elements: Query<&HtmlElement>,
    mut commands: Commands,
) {
    if elements
        .get(add.entity)
        .is_ok_and(|element| element.tag == "canvas")
    {
        commands
            .entity(add.entity)
            .try_insert(HtmlCanvas::default());
    }
}

/// A canvas's picture: the child node showing its source (bug_0215).
#[derive(Component)]
pub struct CanvasPicture;

/// Binds every canvas to its source; runs after layout, before Bevy resizes
/// viewport targets (`update_viewport_render_target_size`), so an oversized
/// box never reaches the resize.
///
/// The source shows on a [`CanvasPicture`] child filling the canvas's
/// padding box, never on the element itself: the element keeps its CSS
/// `border-image`/`background-image` (bug_0215: both were an `ImageNode` on
/// the element, and each update or bind replaced the other). Updates keep
/// the child as they keep app-nested UIs. A camera's viewport goes on the
/// picture once it's laid out (a zero size would resize the camera's image
/// to 1 × 1).
#[allow(clippy::type_complexity)]
pub(crate) fn sync_canvases(
    mut commands: Commands,
    mut registry: ResMut<HtmlCanvases>,
    mut canvases: Query<(
        Entity,
        &HtmlElement,
        &mut HtmlCanvas,
        &ComputedNode,
        Option<&InheritedVisibility>,
        Option<&Children>,
    )>,
    pictures: Query<
        (&ComputedNode, Option<&ViewportNode>, Option<&ImageNode>),
        With<CanvasPicture>,
    >,
    targets: Query<&RenderTarget>,
    images: Res<Assets<Image>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    mut warned: Local<HashSet<Entity>>,
    mut unregistered: Local<HashSet<String>>,
) {
    /// One canvas, as found this frame.
    struct Found {
        entity: Entity,
        name: Option<String>,
        source: Option<CanvasSource>,
        size: Vec2,
        visible: bool,
        /// Within [`MAX_CANVAS_SIDE`] on both sides.
        fits: bool,
        picture: Option<Entity>,
        /// The camera its picture's viewport shows now.
        viewing: Option<Entity>,
    }
    let found: Vec<Found> = canvases
        .iter()
        .map(|(entity, element, _, computed, visibility, kids)| {
            let name = element.data("canvas").map(str::to_owned);
            let size = computed.size();
            let picture = kids
                .into_iter()
                .flat_map(|kids| kids.iter())
                .find(|kid| pictures.contains(*kid));
            Found {
                source: name.as_deref().and_then(|name| registry.get(name)).cloned(),
                name,
                size,
                visible: visibility.is_none_or(|visibility| visibility.get())
                    && size.x > 0.0
                    && size.y > 0.0,
                fits: size.x <= MAX_CANVAS_SIDE && size.y <= MAX_CANVAS_SIDE,
                viewing: picture
                    .and_then(|picture| pictures.get(picture).ok())
                    .and_then(|(_, viewport, _)| viewport?.camera),
                picture,
                entity,
            }
        })
        .collect();

    // One viewport per camera: the canvas that has it while it's visible
    // and fits, else the first such one in document order, else none — an
    // invisible canvas owns nothing (bug_0216: a hidden canvas laid out at
    // 0 × 0 resized the camera's image to 1 × 1) and neither does an
    // oversized one (bug_0217: it kept the camera from one that fits).
    let mut candidates: HashMap<Entity, Vec<&Found>> = HashMap::new();
    for canvas in &found {
        if let Some(CanvasSource::Camera(camera)) = canvas.source
            && canvas.visible
            && canvas.fits
        {
            candidates.entry(camera).or_default().push(canvas);
        }
    }
    let mut owners: HashMap<Entity, Entity> = HashMap::new();
    for (camera, mut list) in candidates {
        let owner = match list.iter().find(|canvas| canvas.viewing == Some(camera)) {
            Some(current) => current.entity,
            None => {
                list.sort_by_cached_key(|canvas| document_path(canvas.entity, &parents, &children));
                list[0].entity
            }
        };
        owners.insert(camera, owner);
    }

    let mut shown = HashSet::new();
    let mut missing = HashSet::new();
    for canvas in found {
        if canvas.visible
            && let Some(name) = &canvas.name
        {
            shown.insert(name.clone());
        }
        if canvas.source.is_none()
            && let Some(name) = &canvas.name
        {
            missing.insert(name.clone());
            if unregistered.insert(name.clone()) {
                debug!("html canvas: no source registered as {name:?}; the canvas stays empty");
            }
        }
        let target = |camera: Entity| targets.get(camera).ok().and_then(RenderTarget::as_image);
        let showing = match canvas.source {
            None => CanvasShowing::Nothing,
            Some(CanvasSource::Image(image)) => CanvasShowing::Image(image),
            Some(CanvasSource::Camera(_)) if !canvas.fits => {
                if canvas.visible && warned.insert(canvas.entity) {
                    warn!(
                        "html canvas {:?}: {}×{} px is larger than {MAX_CANVAS_SIDE} px on a \
                         side; the camera isn't shown",
                        canvas.name.as_deref().unwrap_or(""),
                        canvas.size.x,
                        canvas.size.y,
                    );
                }
                CanvasShowing::Nothing
            }
            Some(CanvasSource::Camera(camera)) if owners.get(&camera) != Some(&canvas.entity) => {
                match target(camera) {
                    Some(image) => CanvasShowing::Image(image.clone()),
                    None => CanvasShowing::Nothing,
                }
            }
            // Bevy's resize unwraps the target image (UPSTREAM U38,
            // bug_0218): a camera whose image isn't in the assets yet (or
            // anymore) isn't bound.
            Some(CanvasSource::Camera(camera)) => match target(camera) {
                Some(image) if images.contains(image) => CanvasShowing::Viewport(camera),
                _ => CanvasShowing::Nothing,
            },
        };
        if canvas.fits {
            warned.remove(&canvas.entity);
        }

        bind(
            &mut commands,
            &canvas.entity,
            canvas.picture,
            &showing,
            &pictures,
        );
        if let Ok((.., mut current, _, _, _)) = canvases.get_mut(canvas.entity) {
            current.set_if_neq(HtmlCanvas {
                name: canvas.name,
                showing,
            });
        }
    }
    // Forget despawned canvases and names that came back or went.
    warned.retain(|entity| canvases.contains(*entity));
    unregistered.retain(|name| missing.contains(name));
    if registry.shown != shown {
        registry.bypass_change_detection().shown = shown;
    }
}

/// Puts what `showing` asks for on `canvas`'s picture, spawning it or
/// despawning it as needed.
fn bind(
    commands: &mut Commands,
    canvas: &Entity,
    picture: Option<Entity>,
    showing: &CanvasShowing,
    pictures: &Query<
        (&ComputedNode, Option<&ViewportNode>, Option<&ImageNode>),
        With<CanvasPicture>,
    >,
) {
    let picture = match (showing, picture) {
        (CanvasShowing::Nothing, Some(picture)) => {
            commands.entity(picture).try_despawn();
            return;
        }
        (CanvasShowing::Nothing, None) => return,
        (_, Some(picture)) => picture,
        (_, None) => {
            // Laid out from the next frame: a viewport waits until then.
            let picture = commands
                .spawn((
                    CanvasPicture,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    ChildOf(*canvas),
                ))
                .id();
            if let CanvasShowing::Image(image) = showing {
                commands
                    .entity(picture)
                    .insert(ImageNode::new(image.clone()).with_mode(NodeImageMode::Stretch));
            }
            return;
        }
    };
    let Ok((computed, viewport, image_node)) = pictures.get(picture) else {
        return;
    };
    let mut entity = commands.entity(picture);
    match showing {
        CanvasShowing::Viewport(camera) => {
            let size = computed.size();
            let wanted = (size.x > 0.0 && size.y > 0.0).then_some(*camera);
            if viewport.map(|viewport| viewport.camera) != Some(wanted) {
                entity.try_insert(ViewportNode { camera: wanted });
            }
            if image_node.is_some() {
                entity.try_remove::<ImageNode>();
            }
        }
        CanvasShowing::Image(image) => {
            if viewport.is_some() {
                entity.try_remove::<ViewportNode>();
            }
            if image_node.is_none_or(|node| node.image != *image) {
                entity.try_insert(ImageNode::new(image.clone()).with_mode(NodeImageMode::Stretch));
            }
        }
        CanvasShowing::Nothing => {}
    }
}

/// `entity`'s position in its UI tree: each ancestor's index among its
/// parent's children, root first (sorts in document order).
fn document_path(
    entity: Entity,
    parents: &Query<&ChildOf>,
    children: &Query<&Children>,
) -> Vec<usize> {
    let mut path = Vec::new();
    let mut child = entity;
    while let Ok(parent) = parents.get(child) {
        let index = children
            .get(parent.parent())
            .ok()
            .and_then(|siblings| siblings.iter().position(|sibling| sibling == child))
            .unwrap_or(0);
        path.push(index);
        child = parent.parent();
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::dimension;
    use proptest::prelude::*;

    /// HTML's rules for parsing non-negative integers, case by case.
    #[test]
    fn dimensions_parse_like_html() {
        for (value, expected) in [
            ("256", Some(256)),
            ("  256", Some(256)),
            ("\t\n\x0C\r 7", Some(7)),
            ("256px", Some(256)),
            ("12.9", Some(12)),
            ("+5", Some(5)),
            ("-0", Some(0)),
            ("0", Some(0)),
            ("007", Some(7)),
            ("2147483647", Some(2_147_483_647)),
            ("000000000000002147483647", Some(2_147_483_647)),
            // Errors: the default size.
            ("", None),
            ("   ", None),
            ("px", None),
            ("-5", None),
            ("+", None),
            ("+-5", None),
            ("2147483648", None),
            ("99999999999999999999", None),
            // Not ASCII whitespace (U+00A0, U+3000).
            ("\u{a0}5", None),
            ("\u{3000}5", None),
        ] {
            assert_eq!(dimension(value), expected, "{value:?}");
        }
    }

    proptest! {
        /// Any in-range number survives ASCII whitespace before it, a `+`,
        /// leading zeros and any suffix that doesn't start with a digit.
        #[test]
        fn a_number_reads_back_whatever_surrounds_it(
            number in 0u32..=i32::MAX as u32,
            space in "[\t\n\x0C\r ]{0,3}",
            plus in proptest::bool::ANY,
            zeros in 0usize..4,
            suffix in "([^0-9].*)?",
        ) {
            let value = format!(
                "{space}{}{}{number}{suffix}",
                if plus { "+" } else { "" },
                "0".repeat(zeros),
            );
            prop_assert_eq!(dimension(&value), Some(number));
        }

        /// Never a panic, and never more than `i32::MAX`.
        #[test]
        fn any_text_parses_in_range_or_not_at_all(value in ".{0,24}") {
            if let Some(number) = dimension(&value) {
                prop_assert!(number <= i32::MAX as u32);
            }
        }
    }
}
