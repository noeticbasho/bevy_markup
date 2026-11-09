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
            .and_then(|value| value.as_utf8_str().trim().parse::<u32>().ok())
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

/// The marker on an [`ImageNode`] this module inserted (an app's own stays
/// app state).
#[derive(Component)]
pub(crate) struct CanvasImage;

/// Binds every canvas to its source; runs after layout, before Bevy resizes
/// viewport targets (`update_viewport_render_target_size`), so an oversized
/// box never reaches the resize.
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
        Option<&ViewportNode>,
        Has<CanvasImage>,
    )>,
    targets: Query<&RenderTarget>,
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
        /// The camera its viewport shows now.
        viewing: Option<Entity>,
    }
    let found: Vec<Found> = canvases
        .iter()
        .map(|(entity, element, _, computed, visibility, viewport, _)| {
            let name = element.data("canvas").map(str::to_owned);
            let size = computed.size();
            Found {
                source: name.as_deref().and_then(|name| registry.get(name)).cloned(),
                name,
                size,
                visible: visibility.is_none_or(|visibility| visibility.get())
                    && size.x > 0.0
                    && size.y > 0.0,
                viewing: viewport.and_then(|viewport| viewport.camera),
                entity,
            }
        })
        .collect();

    // One viewport per camera: the canvas that has it while it's visible,
    // else the first visible one in document order, else the first.
    let mut candidates: HashMap<Entity, Vec<&Found>> = HashMap::new();
    for canvas in &found {
        if let Some(CanvasSource::Camera(camera)) = canvas.source {
            candidates.entry(camera).or_default().push(canvas);
        }
    }
    let mut owners: HashMap<Entity, Entity> = HashMap::new();
    for (camera, mut list) in candidates {
        let owner = match list
            .iter()
            .find(|canvas| canvas.visible && canvas.viewing == Some(camera))
        {
            Some(current) => current.entity,
            None => {
                list.sort_by_cached_key(|canvas| document_path(canvas.entity, &parents, &children));
                list.iter()
                    .find(|canvas| canvas.visible)
                    .unwrap_or(&list[0])
                    .entity
            }
        };
        owners.insert(camera, owner);
    }

    let mut shown = HashSet::new();
    for canvas in found {
        if canvas.visible
            && let Some(name) = &canvas.name
        {
            shown.insert(name.clone());
        }
        if canvas.source.is_none()
            && let Some(name) = &canvas.name
            && unregistered.insert(name.clone())
        {
            debug!("html canvas: no source registered as {name:?}; the canvas stays empty");
        }
        let showing = match canvas.source {
            None => CanvasShowing::Nothing,
            Some(CanvasSource::Image(image)) => CanvasShowing::Image(image),
            Some(CanvasSource::Camera(camera)) if owners.get(&camera) != Some(&canvas.entity) => {
                match targets.get(camera).ok().and_then(RenderTarget::as_image) {
                    Some(image) => CanvasShowing::Image(image.clone()),
                    None => CanvasShowing::Nothing,
                }
            }
            Some(CanvasSource::Camera(camera)) => {
                let size = canvas.size;
                if size.x > MAX_CANVAS_SIDE || size.y > MAX_CANVAS_SIDE {
                    if warned.insert(canvas.entity) {
                        warn!(
                            "html canvas {:?}: {}×{} px is larger than {MAX_CANVAS_SIDE} px on a \
                             side; the camera isn't shown",
                            canvas.name.as_deref().unwrap_or(""),
                            size.x,
                            size.y,
                        );
                    }
                    CanvasShowing::Nothing
                } else {
                    warned.remove(&canvas.entity);
                    CanvasShowing::Viewport(camera)
                }
            }
        };

        let Ok((entity, _, mut current, _, _, viewport, has_image)) =
            canvases.get_mut(canvas.entity)
        else {
            continue;
        };
        let mut entity_commands = commands.entity(entity);
        let wanted = match showing {
            CanvasShowing::Viewport(camera) => Some(camera),
            _ => None,
        };
        match viewport {
            Some(viewport) if viewport.camera != wanted => {
                entity_commands.try_insert(ViewportNode { camera: wanted });
            }
            None if wanted.is_some() => {
                entity_commands.try_insert(ViewportNode { camera: wanted });
            }
            _ => {}
        }
        match &showing {
            CanvasShowing::Image(image) if current.showing != showing || !has_image => {
                entity_commands.try_insert((
                    ImageNode::new(image.clone()).with_mode(NodeImageMode::Stretch),
                    CanvasImage,
                ));
            }
            CanvasShowing::Image(_) => {}
            _ if has_image => {
                entity_commands.try_remove::<(ImageNode, CanvasImage)>();
            }
            _ => {}
        }
        current.set_if_neq(HtmlCanvas {
            name: canvas.name,
            showing,
        });
    }
    if registry.shown != shown {
        registry.bypass_change_detection().shown = shown;
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
