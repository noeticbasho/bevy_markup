//! Overlays anchored to an element: tooltips, popovers, dropdowns.
//!
//! Put [`HtmlAnchor`] on an absolutely positioned UI root (an `HtmlUi`, say,
//! styled `position: absolute` by its `<html class>`) and bevy_markup keeps
//! it beside the element every frame, like CSS anchor positioning:
//!
//! ```no_run
//! # use bevy::prelude::*;
//! # use bevy_markup::prelude::*;
//! # fn f(mut commands: Commands, assets: Res<AssetServer>, button: Entity) {
//! commands.spawn((
//!     HtmlUi::new(assets.load("tooltip.html")),
//!     HtmlAnchor::new(button, AnchorPlacement::Above).with_gap(8.0),
//! ));
//! # }
//! ```
//!
//! - It's placed in the same frame: right after Bevy's layout, from this
//!   frame's rects, the overlay and its descendants are moved beside the
//!   element (as Bevy's `Popover` does), so it never trails a moving
//!   element. Its insets are written too, so the next layout agrees:
//!   `Right`/`Below` set `left`/`top`, `Above` sets `left`/`bottom`, `Left`
//!   sets `right`/`top`; the other insets are `auto`. `Above`/`Left` anchor
//!   by `bottom`/`right`, which needs the viewport height/width; without a
//!   camera they anchor by `top`/`left`. Its `position_type` is the
//!   stylesheet's or the app's; make it a UI root (insets are viewport
//!   coordinates).
//! - When it doesn't fit on its side of the element but fits better on the
//!   opposite one, it flips there ([`HtmlAnchor::with_flip`] turns this
//!   off); [`AnchoredSide`] says which side it's on. Whatever doesn't fit
//!   either way is clamped inside the element's viewport, overlapping the
//!   element rather than leaving the screen.
//! - It renders on the element's UI camera (`UiTargetCamera`), so it works
//!   for UIs on render-to-texture cameras too.
//! - It's despawned with its element.
//!
//! [`HtmlWorldAnchor`] is the same for a point in the 3D world (an entity
//! plus an offset), projected through a camera.

use bevy::camera::Camera;
use bevy::prelude::*;
use bevy::ui::{ComputedUiTargetCamera, UiGlobalTransform};

/// Keeps this UI node beside `element` (see the [module docs](self)).
#[derive(Component, Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(Component)]
pub struct HtmlAnchor {
    /// The element (any UI node) to stay beside.
    pub element: Entity,
    /// Which side of the element.
    pub placement: AnchorPlacement,
    /// Space between the element and the overlay, in logical px.
    pub gap: f32,
    /// Move to the opposite side when the overlay doesn't fit on
    /// `placement`'s side but does on the other (default `true`).
    pub flip: bool,
}

impl HtmlAnchor {
    pub fn new(element: Entity, placement: AnchorPlacement) -> Self {
        Self {
            element,
            placement,
            gap: 0.0,
            flip: true,
        }
    }

    pub fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// `false`: always `placement`'s side, clamped into the viewport.
    pub fn with_flip(mut self, flip: bool) -> Self {
        self.flip = flip;
        self
    }
}

/// The side of the element an [`HtmlAnchor`]ed overlay sits on, aligned to
/// the element's start edge (top for `Right`/`Left`, left for
/// `Above`/`Below`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum AnchorPlacement {
    #[default]
    Right,
    Left,
    Above,
    Below,
}

impl AnchorPlacement {
    /// The opposite side.
    pub fn mirror(self) -> Self {
        match self {
            AnchorPlacement::Right => AnchorPlacement::Left,
            AnchorPlacement::Left => AnchorPlacement::Right,
            AnchorPlacement::Above => AnchorPlacement::Below,
            AnchorPlacement::Below => AnchorPlacement::Above,
        }
    }
}

/// The side an [`HtmlAnchor`]ed overlay is on: its `placement`, or the
/// mirror side when it flipped. Kept by bevy_markup; read it to style the
/// overlay (a tooltip's arrow, say).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub struct AnchoredSide(pub AnchorPlacement);

/// An anchored overlay: its anchor, insets, layout, camera and side.
type Overlay = (
    Entity,
    &'static HtmlAnchor,
    &'static mut Node,
    Option<&'static ComputedNode>,
    Option<&'static UiTargetCamera>,
    Option<&'static AnchoredSide>,
);

/// The element an overlay is anchored to: its layout and camera.
type AnchorElement = (
    &'static ComputedNode,
    &'static UiGlobalTransform,
    Option<&'static ComputedUiTargetCamera>,
);

/// The overlay's rect (logical px) on `side` of `element`, before any
/// viewport clamp.
fn rect_on(side: AnchorPlacement, element: Rect, size: Vec2, gap: f32) -> Rect {
    let min = match side {
        AnchorPlacement::Right => Vec2::new(element.max.x + gap, element.min.y),
        AnchorPlacement::Left => Vec2::new(element.min.x - gap - size.x, element.min.y),
        AnchorPlacement::Below => Vec2::new(element.min.x, element.max.y + gap),
        AnchorPlacement::Above => Vec2::new(element.min.x, element.min.y - gap - size.y),
    };
    Rect::from_corners(min, min + size)
}

/// How far `rect` sticks out of `0..viewport` along `side`'s axis (the
/// axis flipping can fix).
fn overflow(side: AnchorPlacement, rect: Rect, viewport: Vec2) -> f32 {
    let (start, end, limit) = match side {
        AnchorPlacement::Right | AnchorPlacement::Left => (rect.min.x, rect.max.x, viewport.x),
        AnchorPlacement::Above | AnchorPlacement::Below => (rect.min.y, rect.max.y, viewport.y),
    };
    (-start).max(0.0) + (end - limit).max(0.0)
}

/// Places every [`HtmlAnchor`]ed node beside its element, or despawns it
/// with its element. Runs right after Bevy's layout, from this frame's
/// rects: the overlay's laid-out position (and its descendants') is
/// corrected in place, so it renders, clips and picks beside the element
/// this frame, and its insets are written so the next layout agrees.
#[hotpath::measure]
pub(crate) fn place_anchored(
    mut overlays: Query<Overlay>,
    mut layout: ParamSet<(Query<AnchorElement>, Query<&mut UiGlobalTransform>)>,
    cameras: Query<&Camera>,
    children: Query<&Children>,
    mut commands: Commands,
) {
    for (overlay, anchor, mut node, own, target, side) in &mut overlays {
        let elements = layout.p0();
        let Ok((element, transform, element_camera)) = elements.get(anchor.element) else {
            commands.entity(overlay).try_despawn();
            continue;
        };
        let (element, transform, element_camera) = (*element, *transform, element_camera.cloned());
        let element_camera = element_camera.as_ref();
        let camera = element_camera.and_then(ComputedUiTargetCamera::get);
        if let Some(camera) = camera
            && target.map(|target| target.entity()) != Some(camera)
        {
            commands.entity(overlay).try_insert(UiTargetCamera(camera));
        }

        // Physical px → logical UI px, the space `Node` insets use.
        let scale = element.inverse_scale_factor;
        let size = element.size() * scale;
        let min = transform.translation * scale - size / 2.0;
        let element_rect = Rect::from_corners(min, min + size);
        let viewport = camera
            .and_then(|camera| cameras.get(camera).ok())
            .and_then(Camera::physical_viewport_size)
            .map(|size| size.as_vec2() * scale);
        let own_size = own
            .map(|own| own.size() * own.inverse_scale_factor)
            .filter(|own| own.cmpgt(Vec2::ZERO).all());

        // The side: `placement`'s, unless it doesn't fit and the mirror
        // side fits better (needs the overlay's size and the viewport).
        let chosen = match (own_size, viewport) {
            (Some(own_size), Some(viewport)) if anchor.flip => {
                let preferred = anchor.placement;
                let mirror = preferred.mirror();
                let cost = |side| {
                    overflow(
                        side,
                        rect_on(side, element_rect, own_size, anchor.gap),
                        viewport,
                    )
                };
                if cost(preferred) > 0.0 && cost(mirror) < cost(preferred) {
                    mirror
                } else {
                    preferred
                }
            }
            _ => anchor.placement,
        };
        if side.map(|side| side.0) != Some(chosen) {
            commands.entity(overlay).try_insert(AnchoredSide(chosen));
        }

        // The rect, clamped into the viewport once both sizes are known
        // (the overlay overlaps its element rather than leave the screen).
        let unclamped = rect_on(
            chosen,
            element_rect,
            own_size.unwrap_or(Vec2::ZERO),
            anchor.gap,
        );
        let placed_min = match (own_size, viewport) {
            (Some(own_size), Some(viewport)) => {
                unclamped.min.min(viewport - own_size).max(Vec2::ZERO)
            }
            _ => unclamped.min,
        };

        // Insets. `Above`/`Left` anchor by `bottom`/`right` (the far edge)
        // when there's a viewport, so the overlay's own size doesn't
        // depend on where it starts (an auto-width overlay near an edge).
        let extent = own_size.unwrap_or(Vec2::ZERO);
        let [left, top, right, bottom] = match (chosen, viewport) {
            (AnchorPlacement::Above, Some(viewport)) => [
                Some(placed_min.x),
                None,
                None,
                Some(viewport.y - placed_min.y - extent.y),
            ],
            (AnchorPlacement::Left, Some(viewport)) => [
                None,
                Some(placed_min.y),
                Some(viewport.x - placed_min.x - extent.x),
                None,
            ],
            _ => [Some(placed_min.x), Some(placed_min.y), None, None],
        };
        let inset = |value: Option<f32>| value.map_or(Val::Auto, Val::Px);
        let (left, top, right, bottom) = (inset(left), inset(top), inset(right), inset(bottom));
        if (node.left, node.top, node.right, node.bottom) != (left, top, right, bottom) {
            node.left = left;
            node.top = top;
            node.right = right;
            node.bottom = bottom;
        }

        // This frame: move the laid-out overlay (and everything under it)
        // to the placed rect. Without its size it has no layout yet.
        let (Some(own), Some(own_size)) = (own, own_size) else {
            continue;
        };
        let mut transforms = layout.p1();
        let Ok(current) = transforms.get(overlay) else {
            continue;
        };
        let current_min = current.translation * own.inverse_scale_factor - own_size / 2.0;
        let delta = (placed_min - current_min) / own.inverse_scale_factor;
        if delta.abs().cmpgt(Vec2::splat(0.01)).any() {
            translate_subtree(overlay, delta, &children, &mut transforms);
        }
    }
}

/// Moves `entity`'s and its descendants' laid-out transforms by `delta`
/// physical px (what Bevy's `Popover` does after layout).
fn translate_subtree(
    entity: Entity,
    delta: Vec2,
    children: &Query<&Children>,
    transforms: &mut Query<&mut UiGlobalTransform>,
) {
    let mut stack = vec![entity];
    while let Some(entity) = stack.pop() {
        if let Ok(mut transform) = transforms.get_mut(entity) {
            let mut affine = transform.affine();
            affine.translation += delta;
            *transform = affine.into();
        }
        if let Ok(children) = children.get(entity) {
            stack.extend(children.iter());
        }
    }
}

/// Keeps a UI root over a point in the 3D world: `target`'s position plus
/// `offset`, projected through a camera every frame — nameplates, markers,
/// damage numbers.
///
/// - The root's `left`/`top` put its `pivot` (a fraction of its own size:
///   `(0.5, 1.0)` = bottom center, the default) on the projected point; make
///   it `position: absolute` (its `<html class>` rule).
/// - The camera is `camera`, else the root's `UiTargetCamera`, else the
///   default UI camera (the `IsDefaultUiCamera` one, else the window
///   camera Bevy UI renders with).
/// - It owns the root's `Visibility`: hidden while the point is behind the
///   camera or outside the viewport, or `target` is invisible
///   (`InheritedVisibility`); hide it for app reasons with CSS instead
///   (a root class with `display: none`).
/// - [`HtmlWorldAnchorView`] reports the distance to the camera and whether
///   the point is on screen, e.g. for a distance fade (`opacity`).
/// - It's despawned with `target`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Reflect)]
#[reflect(Component)]
#[require(HtmlWorldAnchorView)]
pub struct HtmlWorldAnchor {
    pub target: Entity,
    /// Added to the target's translation, in world units.
    pub offset: Vec3,
    /// The point of the root placed on the projection, as a fraction of its
    /// size.
    pub pivot: Vec2,
    /// The camera to project with (see the type docs for the default).
    pub camera: Option<Entity>,
}

impl HtmlWorldAnchor {
    pub fn new(target: Entity) -> Self {
        Self {
            target,
            offset: Vec3::ZERO,
            pivot: Vec2::new(0.5, 1.0),
            camera: None,
        }
    }

    pub fn with_offset(mut self, offset: Vec3) -> Self {
        self.offset = offset;
        self
    }

    pub fn with_pivot(mut self, pivot: Vec2) -> Self {
        self.pivot = pivot;
        self
    }

    pub fn with_camera(mut self, camera: Entity) -> Self {
        self.camera = Some(camera);
        self
    }
}

/// What [`HtmlWorldAnchor`] measured this frame.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Reflect)]
#[reflect(Component)]
pub struct HtmlWorldAnchorView {
    /// From the camera to the anchored point, in world units.
    pub distance: f32,
    /// The point projects into the camera's viewport (not behind it).
    pub on_screen: bool,
}

/// A world-anchored root: its anchor, measurements, insets, visibility,
/// measured size and UI camera.
type WorldOverlay = (
    Entity,
    &'static HtmlWorldAnchor,
    &'static mut HtmlWorldAnchorView,
    &'static mut Node,
    &'static mut Visibility,
    Option<&'static ComputedNode>,
    Option<&'static UiTargetCamera>,
);

/// Places every [`HtmlWorldAnchor`]ed root over its target, or despawns it
/// with its target.
#[hotpath::measure]
pub(crate) fn place_world_anchored(
    mut overlays: Query<WorldOverlay>,
    targets: Query<(&GlobalTransform, Option<&InheritedVisibility>)>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    default_camera: DefaultUiCamera,
    // Not registered by minimal headless apps.
    scale: Option<Res<UiScale>>,
    mut commands: Commands,
) {
    for (overlay, anchor, mut view, mut node, mut visibility, own, ui_camera) in &mut overlays {
        let Ok((target, target_visibility)) = targets.get(anchor.target) else {
            commands.entity(overlay).try_despawn();
            continue;
        };
        let camera = anchor
            .camera
            .or(ui_camera.map(UiTargetCamera::entity))
            .or_else(|| default_camera.get());
        let Some((camera, camera_transform)) = camera.and_then(|camera| cameras.get(camera).ok())
        else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let point = target.translation() + anchor.offset;
        // The UI camera's viewport: the root's `Val::Px` coordinates are
        // relative to its top-left (a split-screen UI camera differs from
        // the projecting one).
        let ui_rect = ui_camera
            .map(UiTargetCamera::entity)
            .or_else(|| default_camera.get())
            .and_then(|camera| cameras.get(camera).ok())
            .and_then(|(camera, _)| camera.logical_viewport_rect());
        let projected = camera
            .world_to_viewport(camera_transform, point)
            .ok()
            .and_then(|position| {
                let rect = camera.logical_viewport_rect()?;
                // On-screen: the window-logical position relative to the
                // projecting camera's viewport, inside the viewport —
                // both sides in the same units (dividing by UiScale
                // before the test mixed them: a visible point hid with
                // UiScale < 1, an off-screen one showed with UiScale > 1).
                let local = position - rect.min;
                let on_screen = local.cmpge(Vec2::ZERO).all() && local.cmple(rect.size()).all();
                // UI px: relative to the UI camera's viewport top-left,
                // divided by UiScale (Bevy multiplies `Val::Px` back by
                // the scale factor × UiScale).
                let ui_scale = scale.as_ref().map_or(1.0, |scale| scale.0);
                let min = ui_rect.unwrap_or(rect).min;
                let ui = (position - min) / ui_scale;
                on_screen.then_some(ui)
            });
        let measured = HtmlWorldAnchorView {
            distance: camera_transform.translation().distance(point),
            on_screen: projected.is_some(),
        };
        if *view != measured {
            *view = measured;
        }
        let shown = target_visibility.is_none_or(|visible| visible.get());
        let Some(position) = projected.filter(|_| shown) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let size = own.map_or(Vec2::ZERO, |own| own.size() * own.inverse_scale_factor);
        let top_left = position - size * anchor.pivot;
        let (left, top) = (Val::Px(top_left.x), Val::Px(top_left.y));
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
        visibility.set_if_neq(Visibility::Inherited);
    }
}
