//! Tooltips declared in the markup, like a browser's `title` attribute.
//!
//! ```html
//! <div class="button" data-on-click="buy" data-tooltip="shop-buy-tooltip"
//!      data-tooltip-args='{"price": {{ price }}}' data-tooltip-placement="above">…</div>
//! ```
//!
//! Insert [`HtmlTooltips`] with the template tooltips are rendered from.
//! While a pointer is over an element with `data-tooltip` — or over one of
//! its descendants; the nearest such element wins, as with `title` — an
//! `HtmlUi` of that template is shown beside it ([`HtmlAnchor`]), with
//! these template variables:
//!
//! - `key`: the `data-tooltip` value (typically a Fluent message id),
//! - `args`: `data-tooltip-args` parsed as JSON (an object; `{}` when absent
//!   or invalid),
//! - `placement`: `data-tooltip-placement` — `right` (default), `left`,
//!   `above` or `below`; it also picks the side.
//!
//! ```html
//! <html class="tooltip"><p data-l10n-id="{{ key }}" data-l10n-args='{{ args | json }}'>{{ key }}</p></html>
//! ```
//!
//! The tooltip is despawned when the pointer leaves (or its element goes);
//! an in-place update that changes the attributes updates the open tooltip.
//! Style the root with its `<html class>` (`position: absolute`, a high
//! `z-index`, `pointer-events: none`). Pointer-driven only: focus doesn't
//! show tooltips.

use bevy::picking::hover::HoverMap;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use serde_json::Value;

use crate::anchor::{AnchorPlacement, HtmlAnchor};
use crate::html::{HtmlElement, HtmlUi, TemplateContext};
use crate::template::HtmlTemplate;

/// Enables `data-tooltip`: tooltips are `HtmlUi`s of `template`, `gap`
/// logical px from their element (see the [module docs](self)).
#[derive(Resource, Clone, Debug)]
pub struct HtmlTooltips {
    pub template: Handle<HtmlTemplate>,
    pub gap: f32,
}

impl HtmlTooltips {
    pub fn new(template: Handle<HtmlTemplate>) -> Self {
        Self { template, gap: 8.0 }
    }

    pub fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }
}

/// A tooltip root and the element it's for.
#[derive(Component, Clone, Copy, Debug)]
pub struct HtmlTooltip {
    pub element: Entity,
}

/// The element's tooltip variables and side, if it has `data-tooltip`.
fn tooltip_of(element: &HtmlElement) -> Option<(TemplateContext, AnchorPlacement)> {
    let key = element.data("tooltip")?;
    let args = element
        .data("tooltip-args")
        .and_then(|json| crate::template::parse_attribute_json(json).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Default::default()));
    let (placement, name) = match element.data("tooltip-placement") {
        Some("left") => (AnchorPlacement::Left, "left"),
        Some("above") => (AnchorPlacement::Above, "above"),
        Some("below") => (AnchorPlacement::Below, "below"),
        _ => (AnchorPlacement::Right, "right"),
    };
    let context = TemplateContext::new()
        .with("key", key)
        .with("args", &args)
        .with("placement", name);
    Some((context, placement))
}

/// Shows a tooltip for every element with `data-tooltip` under a pointer
/// (the nearest one per hovered node), keeps open ones up to date, and
/// despawns the rest.
#[allow(clippy::too_many_arguments)]
#[hotpath::measure]
pub(crate) fn show_tooltips(
    config: Option<Res<HtmlTooltips>>,
    hover_map: Option<Res<HoverMap>>,
    elements: Query<&HtmlElement>,
    nodes: Query<&ComputedNode>,
    parents: Query<&ChildOf>,
    mut tooltips: Query<(Entity, &HtmlTooltip, &mut TemplateContext, &mut HtmlAnchor)>,
    mut commands: Commands,
) {
    let (Some(config), Some(hover_map)) = (config, hover_map) else {
        return;
    };
    let mut wanted: HashMap<Entity, (TemplateContext, AnchorPlacement, Entity)> = HashMap::new();
    for hovered in hover_map.values() {
        for &hit in hovered.keys() {
            let nearest = std::iter::once(hit)
                .chain(parents.iter_ancestors(hit))
                .find_map(|entity| {
                    let tooltip = tooltip_of(elements.get(entity).ok()?)?;
                    // The tooltip anchors to the nearest ancestor that has
                    // a node: a link span (the hover hit) carries the
                    // tooltip data but no `ComputedNode`, and an anchor to
                    // it despawns every frame.
                    let anchor = std::iter::once(entity)
                        .chain(parents.iter_ancestors(entity))
                        .find(|&candidate| nodes.get(candidate).is_ok())
                        .unwrap_or(entity);
                    Some((entity, tooltip, anchor))
                });
            if let Some((element, (tooltip, placement), anchor)) = nearest {
                wanted.insert(element, (tooltip, placement, anchor));
            }
        }
    }
    for (tooltip, &HtmlTooltip { element }, mut context, mut anchor) in &mut tooltips {
        match wanted.remove(&element) {
            // Still wanted: refresh (an identical render changes nothing).
            Some((new_context, placement, anchor_target)) => {
                if **context != *new_context {
                    *context = new_context;
                }
                if anchor.placement != placement || anchor.gap != config.gap {
                    anchor.placement = placement;
                    anchor.gap = config.gap;
                }
                if anchor.element != anchor_target {
                    anchor.element = anchor_target;
                }
            }
            None => commands.entity(tooltip).despawn(),
        }
    }
    for (element, (context, placement, anchor_target)) in wanted {
        commands.spawn((
            HtmlTooltip { element },
            HtmlUi::new(config.template.clone()),
            context,
            HtmlAnchor::new(anchor_target, placement).with_gap(config.gap),
        ));
    }
}
