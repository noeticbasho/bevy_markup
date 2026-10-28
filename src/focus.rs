//! Focus and directional (gamepad / keyboard) navigation, browser-style.
//!
//! - **Focusable elements**: elements with a `data-on-click` hook (the
//!   counterpart of a `<button>`), `<button>`s, text `<input>`s and
//!   elements with `tabindex="0"` (or any non-negative value); a negative
//!   `tabindex` opts an element out. They get a [`Focusable`] component.
//! - **Focus** is Bevy's own [`InputFocus`] resource, so markup UIs share it
//!   with any other UI in the app. [`InputFocusVisible`] says whether focus
//!   should be *shown*: directional navigation sets it, a pointer press
//!   clears it (the browser's `:focus-visible` heuristic).
//! - **CSS**: `:focus` matches the focused element, `:focus-visible` the
//!   focused element while focus is shown; `outline` draws a Bevy
//!   [`Outline`] — the usual focus ring:
//!
//!   ```css
//!   .button:focus-visible { outline: 2px solid #f9c74f; outline-offset: 2px }
//!   ```
//! - **Autofocus**: when focus isn't on a focusable element in scope (nothing
//!   focused yet, the focused element was despawned, a modal opened), focus
//!   moves to the element with the `autofocus` attribute, else the first
//!   focusable element in scope.
//! - **Updates keep focus**: content updates keep matching elements (and
//!   focus on them) in place; when the focused element is replaced, focus
//!   returns to the element with the same `id` in the same `HtmlUi` (give
//!   focusable elements an `id`).
//! - **Scope**: every visible `HtmlUi` takes part, except roots marked
//!   [`HtmlNoFocus`] (e.g. UI rendered onto a 3D quad and driven by a laser).
//!   A visible [`HtmlModal`] root confines focus and navigation to itself
//!   (like a modal `<dialog>`); with several, the topmost wins (highest
//!   `GlobalZIndex`, then `ZIndex` — e.g. the root rule's `z-index`).
//!   An element's scope is its nearest `HtmlUi` ancestor.
//! - **Tab order** is HTML's: positive `tabindex` values first, ascending,
//!   then the other focusable elements in document order; elements under
//!   `display: none` are skipped. [`HtmlFocus::tab`] steps through it
//!   (wrapping; an open modal keeps it inside), [`HtmlFocus::tab_order`]
//!   lists it. Elements in scope carry Bevy's [`TabIndex`] and roots a
//!   [`TabGroup`] (modal on the open [`HtmlModal`]), so Bevy's
//!   `TabNavigationPlugin` drives them too in an app that renders (it
//!   reads `InheritedVisibility`); `HtmlFocus::tab` doesn't need that.
//! - **Input is the app's**: bevy_markup reads no keys or buttons. Bind your
//!   own input and call [`HtmlFocus::navigate`] / [`HtmlFocus::tab`] /
//!   [`HtmlFocus::activate`].
//!   Activation emits the element's `data-on-click` signals as
//!   [`ElementSignal`] messages, like a pointer click, with the input the
//!   app reports as the source ([`SignalSource::Activation`]: a key, a
//!   gamepad button, or [`ActivationInput::Synthetic`] for harnesses). Navigating off the edge in a direction with no
//!   neighbour triggers [`FocusEdge`] on the focused element (e.g. to page a
//!   list).

use std::borrow::Cow;

use bevy::ecs::system::SystemParam;
use bevy::input_focus::directional_navigation::DirectionalNavigationPlugin;
/// [`HtmlFocus::tab`]'s step (Bevy's): `Next` for Tab, `Previous` for
/// Shift+Tab, `First` / `Last`.
pub use bevy::input_focus::tab_navigation::NavAction;
use bevy::input_focus::tab_navigation::{TabGroup, TabIndex};
use bevy::input_focus::{FocusCause, InputFocus, InputFocusVisible};
use bevy::math::CompassOctant;
use bevy::picking::events::PointerPress;
use bevy::prelude::*;
use bevy::ui::UiSystems;
use bevy::ui::auto_directional_navigation::{AutoDirectionalNavigation, AutoDirectionalNavigator};

use crate::HtmlUiSystems;
use crate::html::{HtmlElement, HtmlUi};
use crate::signals::{
    ActivationInput, ActivationParams, ElementSignal, ElementSignals, PseudoState, SignalBinding,
    SignalSource, SignalTrigger,
};

/// A focusable element (see the [module docs](self)). `autofocus` is the
/// HTML attribute; `tab_index` is its `tabindex` (`0` when absent), which
/// orders Tab navigation.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
#[reflect(Component, Default)]
pub struct Focusable {
    pub autofocus: bool,
    pub tab_index: i32,
}

impl Focusable {
    /// The Bevy [`TabIndex`] giving HTML's order: positive `tabindex`
    /// values first, ascending, then the `0`s in document order. Bevy
    /// sorts by the number (ties in tree order), so `0` maps to the
    /// largest value and positive values stay below it.
    pub fn bevy_tab_index(&self) -> TabIndex {
        TabIndex(match self.tab_index {
            0 => i32::MAX,
            n => n.clamp(1, i32::MAX - 1),
        })
    }
}

/// On an `HtmlUi` root: while it's visible, focus and navigation stay inside
/// it. The topmost (`GlobalZIndex`, then `ZIndex`) wins among several.
#[derive(Component, Clone, Copy, Debug, Default, Reflect)]
#[reflect(Component, Default)]
pub struct HtmlModal;

/// On an `HtmlUi` root: its elements never take focus or navigation (they
/// stay clickable).
#[derive(Component, Clone, Copy, Debug, Default, Reflect)]
#[reflect(Component, Default)]
pub struct HtmlNoFocus;

/// Triggered on the focused element when [`HtmlFocus::navigate`] finds no
/// focusable neighbour in `direction`.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct FocusEdge {
    pub entity: Entity,
    pub direction: CompassOctant,
}

/// Trigger on an element to activate it: emits its `data-on-click` signals
/// as [`ElementSignal`]s, like a pointer click, with
/// [`SignalSource::Activation`]`(input)`. [`HtmlFocus::activate`] triggers
/// it on the focused element.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct ActivateElement {
    pub entity: Entity,
    /// What activated it.
    pub input: ActivationInput,
}

/// Drive focus from the app's own input bindings.
#[derive(SystemParam)]
pub struct HtmlFocus<'w, 's> {
    navigator: AutoDirectionalNavigator<'w, 's>,
    tab_roots: Query<'w, 's, (Entity, Option<&'static TabGroup>), FocusRoots>,
    tab_indices: Query<'w, 's, &'static TabIndex, With<Focusable>>,
    children: Query<'w, 's, &'static Children>,
    roots: Query<'w, 's, (), With<HtmlUi>>,
    visible: ResMut<'w, InputFocusVisible>,
    commands: Commands<'w, 's>,
}

impl HtmlFocus<'_, '_> {
    /// Moves focus to the nearest focusable element in `direction` and shows
    /// it (`:focus-visible`). Returns the newly focused element; at an edge
    /// it triggers [`FocusEdge`] instead and returns `None`.
    pub fn navigate(&mut self, direction: CompassOctant) -> Option<Entity> {
        self.visible.0 = true;
        match self.navigator.navigate(direction) {
            Ok(entity) => Some(entity),
            Err(_) => {
                if let Some(entity) = self.navigator.input_focus() {
                    self.commands.trigger(FocusEdge { entity, direction });
                }
                None
            }
        }
    }

    /// Moves focus in Tab order and shows it (`:focus-visible`):
    /// [`NavAction::Next`] for Tab, [`NavAction::Previous`] for Shift+Tab
    /// (`First` / `Last` jump). The order is HTML's — positive `tabindex`
    /// ascending, then the rest in document order — across the UIs in
    /// scope, wrapping at the ends; an open [`HtmlModal`] keeps it inside.
    /// Returns the newly focused element, `None` when nothing is
    /// focusable.
    pub fn tab(&mut self, action: NavAction) -> Option<Entity> {
        self.visible.0 = true;
        let order = self.tab_order();
        let focus = &mut self.navigator.manual_directional_navigation.focus;
        let current = focus
            .get()
            .and_then(|entity| order.iter().position(|&e| e == entity));
        let count = order.len();
        let next = match (current, action) {
            _ if count == 0 => return None,
            (Some(index), NavAction::Next) => (index + 1) % count,
            (Some(index), NavAction::Previous) => (index + count - 1) % count,
            (None, NavAction::Next) | (_, NavAction::First) => 0,
            (None, NavAction::Previous) | (_, NavAction::Last) => count - 1,
        };
        focus.set(order[next], FocusCause::Navigated);
        Some(order[next])
    }

    /// The focusable elements in scope in Tab order (what [`tab`](Self::tab)
    /// steps through), e.g. to stop at the ends instead of wrapping.
    pub fn tab_order(&self) -> Vec<Entity> {
        // The elements `sync_navigation` gave a `TabIndex` (in scope,
        // shown), each root's subtree in document order, roots by their
        // `TabGroup` order, then stably by index. Walked here rather than
        // with Bevy's `TabNavigation`, which skips entities whose
        // `InheritedVisibility` hasn't propagated (headless apps, the frame
        // after a spawn).
        let mut roots: Vec<(Entity, i32)> = self
            .tab_roots
            .iter()
            .map(|(root, group)| (root, group.map_or(0, |group| group.order)))
            .collect();
        roots.sort_by_key(|&(_, order)| order);
        let mut order: Vec<(Entity, TabIndex)> = Vec::new();
        for (root, _) in roots {
            let mut stack = vec![root];
            while let Some(entity) = stack.pop() {
                if let Ok(index) = self.tab_indices.get(entity) {
                    order.push((entity, *index));
                }
                if let Ok(children) = self.children.get(entity) {
                    // A nested `HtmlUi` is walked as its own root.
                    stack.extend(
                        children
                            .iter()
                            .rev()
                            .filter(|&child| !self.roots.contains(child)),
                    );
                }
            }
        }
        order.sort_by_key(|&(_, index)| index);
        order.into_iter().map(|(entity, _)| entity).collect()
    }

    /// Activates the focused element (see [`ActivateElement`]); `input` is
    /// what the app saw (the signals' [`SignalSource::Activation`]).
    pub fn activate(&mut self, input: ActivationInput) {
        if let Some(entity) = self.navigator.input_focus() {
            self.commands.trigger(ActivateElement { entity, input });
        }
    }

    /// The focused entity, if any.
    pub fn focused(&mut self) -> Option<Entity> {
        self.navigator.input_focus()
    }
}

/// `tag`'s focusability: `tabindex` decides when present (`>= 0` focusable,
/// negative not); otherwise `native` elements (`<button>`, a text
/// `<input>`) and elements with a `data-on-click` hook are.
pub(crate) fn focusable(
    tag: &tl::HTMLTag,
    signals: &[SignalBinding],
    native: bool,
) -> Option<Focusable> {
    let attributes = tag.attributes();
    let tabindex = attributes
        .get("tabindex")
        .flatten()
        .and_then(|value| value.as_utf8_str().trim().parse::<i32>().ok());
    let focusable = match tabindex {
        Some(index) => index >= 0,
        None => {
            native
                || signals
                    .iter()
                    .any(|binding| binding.trigger == SignalTrigger::Click)
        }
    };
    focusable.then(|| Focusable {
        // `contains`: a value-less `autofocus` has no value to `get`.
        autofocus: attributes.contains("autofocus"),
        tab_index: tabindex.unwrap_or(0),
    })
}

/// Directional navigation: focus memory, press-to-focus, activation and
/// the app's `HtmlFocus` API. The navigation plugin itself is added in
/// [`add_navigation_plugin`] (from the crate plugin's `finish`), so an app
/// adding Bevy's [`DirectionalNavigationPlugin`] itself is detected
/// instead of hitting Bevy's duplicate-plugin error.
pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<InputFocus>()
        .init_resource::<InputFocusVisible>()
        .init_resource::<FocusMemory>()
        .add_observer(activate_element)
        .add_observer(focus_on_press)
        .add_observer(drop_navigation)
        .add_systems(
            PostUpdate,
            (
                (remember_focus, update_focus_states)
                    // After the dispatcher: a signal handler that moves
                    // focus shows the new state the same frame.
                    .after(crate::signals::dispatch_signals)
                    .before(HtmlUiSystems::Render),
                (sync_navigation, repair_focus)
                    .chain()
                    .after(HtmlUiSystems::Build)
                    .before(UiSystems::Prepare),
            ),
        );
}

/// Adds Bevy's [`DirectionalNavigationPlugin`] when the app hasn't (its
/// neighbor lookup backs the focus navigation). Called from the crate
/// plugin's `finish`, after every plugin's `build`: an app adding the
/// plugin itself anywhere in its `build` is detected here.
pub(crate) fn add_navigation_plugin(app: &mut App) {
    if !app.is_plugin_added::<DirectionalNavigationPlugin>() {
        app.add_plugins(DirectionalNavigationPlugin);
    }
}

/// The first navigable element in scope, in document order: a depth-first
/// walk of each in-scope root's subtree (the same order the build
/// spawns/updates in). `Entity`-ordered `.min()` picked recycled ids by
/// generation, which could be the page's *last* element.
fn first_focusable_in_scope(
    roots_in_scope: &Query<ScopeRoot, (With<HtmlUi>, Without<HtmlNoFocus>)>,
    navigable: &Query<(Entity, &HtmlElement, &Focusable), With<AutoDirectionalNavigation>>,
    children: &Query<&'static Children>,
    visibility: &Query<&Visibility>,
    parents: &Query<&ChildOf>,
    roots: &Query<(), With<HtmlUi>>,
) -> Option<Entity> {
    let in_scope = |entity: Entity| {
        visible_in_hierarchy(entity, visibility, parents)
            && html_root(entity, parents, roots).is_some_and(|root| {
                active_modal(roots_in_scope, visibility, parents).is_none_or(|modal| root == modal)
            })
    };
    // Nothing is navigable: the walk below (a stack of every in-scope
    // entity) costs nothing to skip, and repair_focus runs it every frame
    // while nothing is focused (HUD-only UIs).
    if navigable.is_empty() {
        return None;
    }
    let mut stack: Vec<Entity> = roots_in_scope
        .iter()
        .map(|(root, ..)| root)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    while let Some(entity) = stack.pop() {
        if navigable.contains(entity) && in_scope(entity) {
            return Some(entity);
        }
        if let Ok(children) = children.get(entity) {
            // Depth-first, document order: children in order, revisited
            // after the current level (hence the reverse push).
            stack.extend(children.iter().rev());
        }
    }
    None
}

/// The `HtmlUi` `entity` belongs to (itself included).
fn html_root(
    entity: Entity,
    parents: &Query<&ChildOf>,
    roots: &Query<(), With<HtmlUi>>,
) -> Option<Entity> {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .find(|&ancestor| roots.contains(ancestor))
}

/// Whether `entity` is visible in the hierarchy, resolved from `Visibility`
/// directly (the nearest non-`Inherited` value up the tree decides; all
/// `Inherited` = visible) rather than `InheritedVisibility`, which lags a
/// frame behind spawns and needs the render-side propagation systems.
fn visible_in_hierarchy(
    entity: Entity,
    visibility: &Query<&Visibility>,
    parents: &Query<&ChildOf>,
) -> bool {
    for ancestor in std::iter::once(entity).chain(parents.iter_ancestors(entity)) {
        match visibility.get(ancestor) {
            Ok(Visibility::Hidden) => return false,
            Ok(Visibility::Visible) => return true,
            _ => {}
        }
    }
    true
}

/// The last focused element by root and `id`, to restore after a rebuild.
#[derive(Resource, Default)]
struct FocusMemory {
    root: Option<Entity>,
    id: Option<String>,
}

/// Runs before this frame's rebuilds can despawn the focused element.
fn remember_focus(
    focus: Res<InputFocus>,
    elements: Query<&HtmlElement, With<Focusable>>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<HtmlUi>>,
    mut memory: ResMut<FocusMemory>,
) {
    let Some(entity) = focus.get() else {
        return;
    };
    let Ok(element) = elements.get(entity) else {
        return;
    };
    memory.root = html_root(entity, &parents, &roots);
    memory.id = element.id.clone();
}

/// The visible modal root in scope, if any (Bevy's order of UI roots:
/// `GlobalZIndex`, then `ZIndex` — CSS `z-index` on the root rule).
fn active_modal(
    roots_in_scope: &Query<ScopeRoot, (With<HtmlUi>, Without<HtmlNoFocus>)>,
    visibility: &Query<&Visibility>,
    parents: &Query<&ChildOf>,
) -> Option<Entity> {
    let visible = |entity| visible_in_hierarchy(entity, visibility, parents);
    roots_in_scope
        .iter()
        .filter(|&(entity, modal, ..)| modal && visible(entity))
        .max_by_key(|(_, _, global, local)| (global.map_or(0, |z| z.0), local.map_or(0, |z| z.0)))
        .map(|(entity, ..)| entity)
}

/// Keeps `PseudoState`'s focus bits in step with `InputFocus` /
/// `InputFocusVisible` (a change restyles in place).
fn update_focus_states(
    focus: Res<InputFocus>,
    visible: Res<InputFocusVisible>,
    elements: Query<(Entity, Option<&PseudoState>), With<HtmlElement>>,
    added: Query<(), Added<HtmlElement>>,
    mut commands: Commands,
) {
    if !focus.is_changed() && !visible.is_changed() && added.is_empty() {
        return;
    }
    for (entity, state) in &elements {
        let current = state.copied().unwrap_or_default();
        let focused = focus.get() == Some(entity);
        let desired = PseudoState {
            focused,
            focus_visible: focused && visible.0,
            ..current
        };
        if state.copied() != Some(desired) && (state.is_some() || focused) {
            // `try_insert`, not `insert`: a signal handler that despawned
            // the UI this frame (same frame as a focus change) leaves dead
            // entities here — the bug_0021 class, which missed this
            // system.
            commands.entity(entity).try_insert(desired);
        }
    }
}

/// The `HtmlUi` roots that take part in focus.
type FocusRoots = (With<HtmlUi>, Without<HtmlNoFocus>);

/// An `HtmlUi` root's tab group as `sync_navigation` keeps it.
type RootTabGroup = (Entity, Has<HtmlNoFocus>, Option<&'static TabGroup>);

/// A focusable element and the navigation components it has.
type FocusableNavigation = (
    Entity,
    &'static Focusable,
    Has<AutoDirectionalNavigation>,
    Option<&'static TabIndex>,
);

/// An `HtmlUi` root that takes part in focus: entity, modal, stacking.
type ScopeRoot = (
    Entity,
    Has<HtmlModal>,
    Option<&'static GlobalZIndex>,
    Option<&'static ZIndex>,
);

/// Gives exactly the focusable elements in scope `AutoDirectionalNavigation`
/// and Bevy's [`TabIndex`] (not under a `display: none` element, which Tab
/// skips like a browser), and every root in scope a [`TabGroup`] — modal
/// on the active [`HtmlModal`] only, so Bevy's tab navigation (ours, or
/// its own `TabNavigationPlugin`) stays inside it.
#[allow(clippy::too_many_arguments)] // a Bevy system
#[hotpath::measure]
fn sync_navigation(
    roots_in_scope: Query<ScopeRoot, (With<HtmlUi>, Without<HtmlNoFocus>)>,
    all_roots: Query<RootTabGroup, With<HtmlUi>>,
    elements: Query<FocusableNavigation>,
    visibility: Query<&Visibility>,
    nodes: Query<&Node>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<HtmlUi>>,
    mut commands: Commands,
) {
    let visible = |entity| visible_in_hierarchy(entity, &visibility, &parents);
    let modal = active_modal(&roots_in_scope, &visibility, &parents);
    for (entity, focusable, navigable, tab_index) in &elements {
        let wanted = html_root(entity, &parents, &roots).is_some_and(|root| match modal {
            Some(modal) => root == modal,
            None => roots_in_scope.contains(root) && visible(root),
        });
        // `try_`: the app may despawn the UI in the same frame (bug_0021).
        if wanted && !navigable {
            commands
                .entity(entity)
                .try_insert(AutoDirectionalNavigation::default());
        } else if !wanted && navigable {
            commands
                .entity(entity)
                .try_remove::<AutoDirectionalNavigation>();
        }
        let tab_wanted = (wanted && !displayed_none(entity, &nodes, &parents, &roots))
            .then(|| focusable.bevy_tab_index());
        if tab_wanted != tab_index.copied() {
            match tab_wanted {
                Some(index) => commands.entity(entity).try_insert(index),
                None => commands.entity(entity).try_remove::<TabIndex>(),
            };
        }
    }
    for (root, no_focus, group) in &all_roots {
        let wanted = (!no_focus).then(|| TabGroup {
            // An order the app set on the root is kept.
            order: group.map_or(0, |group| group.order),
            modal: modal == Some(root),
        });
        let current = group.map(|group| (group.order, group.modal));
        if wanted.map(|group| (group.order, group.modal)) != current {
            match wanted {
                Some(group) => commands.entity(root).try_insert(group),
                None => commands.entity(root).try_remove::<TabGroup>(),
            };
        }
    }
}

/// Whether `entity` or an ancestor up to its `HtmlUi` root is
/// `display: none`.
fn displayed_none(
    entity: Entity,
    nodes: &Query<&Node>,
    parents: &Query<&ChildOf>,
    roots: &Query<(), With<HtmlUi>>,
) -> bool {
    for ancestor in std::iter::once(entity).chain(parents.iter_ancestors(entity)) {
        if nodes
            .get(ancestor)
            .is_ok_and(|node| node.display == Display::None)
        {
            return true;
        }
        if roots.contains(ancestor) {
            break;
        }
    }
    false
}

/// Puts focus back on a focusable element in scope when it isn't on one:
/// the remembered element (same root and `id`), else the `autofocus`
/// element, else the first one. Focus on another UI node (the app's own UI)
/// is left alone; focus on something that isn't UI — Bevy's input dispatch
/// focuses the primary window at startup — counts as none (bug_0022). With
/// nothing focusable in scope, focus on a markup element is cleared.
#[hotpath::measure]
fn repair_focus(
    mut focus: ResMut<InputFocus>,
    memory: Res<FocusMemory>,
    navigable: Query<(Entity, &HtmlElement, &Focusable), With<AutoDirectionalNavigation>>,
    markup: Query<(), With<Focusable>>,
    targets: Query<(), With<ElementSignals>>,
    ui_nodes: Query<(), With<Node>>,
    roots_in_scope: Query<ScopeRoot, (With<HtmlUi>, Without<HtmlNoFocus>)>,
    children: Query<&'static Children>,
    visibility: Query<&Visibility>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<HtmlUi>>,
) {
    let current = focus.get();
    // An activation target the app focused itself (a link span, which has
    // bindings but no box to navigate by) keeps the focus — but only in
    // scope: an open modal takes it, like it takes a navigable element's.
    let in_scope = |entity| {
        visible_in_hierarchy(entity, &visibility, &parents)
            && html_root(entity, &parents, &roots).is_some_and(|root| {
                active_modal(&roots_in_scope, &visibility, &parents)
                    .is_none_or(|modal| root == modal)
            })
    };
    if current.is_some_and(|entity| {
        navigable.contains(entity)
            || (ui_nodes.contains(entity) && !markup.contains(entity))
            || (targets.contains(entity) && in_scope(entity))
    }) {
        return;
    }
    let remembered = memory.id.as_deref().and_then(|id| {
        navigable.iter().find_map(|(entity, element, _)| {
            (element.id.as_deref() == Some(id)
                && html_root(entity, &parents, &roots) == memory.root)
                .then_some(entity)
        })
    });
    let target = remembered
        .or_else(|| {
            navigable
                .iter()
                .find_map(|(entity, _, focusable)| focusable.autofocus.then_some(entity))
        })
        // The "first focusable element in scope" fallback: document order
        // (a depth-first walk of the in-scope roots), not `Entity` order —
        // recycled ids sort by generation, and the old `.min()` could
        // focus the last element of the page.
        .or_else(|| {
            first_focusable_in_scope(
                &roots_in_scope,
                &navigable,
                &children,
                &visibility,
                &parents,
                &roots,
            )
        });
    match target {
        Some(entity) => focus.set(entity, FocusCause::Navigated),
        None if current.is_some() => focus.clear(),
        None => {}
    }
}

/// An element a rebuild made unfocusable (its `Focusable` removed) leaves
/// directional and Tab navigation; `sync_navigation` only visits
/// focusable elements.
fn drop_navigation(remove: On<Remove<Focusable>>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<(AutoDirectionalNavigation, TabIndex)>();
}

/// A pointer press focuses the pressed focusable element (the deepest one
/// under the pointer) and hides the focus ring, like a browser's mousedown.
fn focus_on_press(
    press: On<PointerPress>,
    focusables: Query<(), (With<Focusable>, With<AutoDirectionalNavigation>)>,
    any_focusable: Query<(), With<Focusable>>,
    parents: Query<&ChildOf>,
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
) {
    if !focusables.contains(press.entity) {
        return;
    }
    let deepest = std::iter::once(press.original_event_target())
        .chain(parents.iter_ancestors(press.original_event_target()))
        .find(|&entity| any_focusable.contains(entity));
    if deepest == Some(press.entity) {
        focus.set(press.entity, FocusCause::Pressed);
        visible.0 = false;
    }
}

fn activate_element(activate: On<ActivateElement>, mut params: ActivationParams) {
    let target = activate.entity;
    // A submit button's activation is the form's submit (Enter / gamepad A
    // on `<button type="submit">`), with the activation as its source —
    // the same path a click on it takes.
    if params.submits.get(target).is_ok() {
        crate::signals::emit_form(
            target,
            SignalSource::Activation(activate.input),
            &mut params.writer,
            &params.parents,
            &params.elements,
            &params.signals,
            #[cfg(feature = "text-input")]
            &params.children,
            #[cfg(feature = "text-input")]
            &params.editable,
            #[cfg(feature = "text-input")]
            &params.names,
        );
    }
    let Ok((signals, element)) = params.elements_and_state.get(target) else {
        return;
    };
    for binding in signals
        .0
        .iter()
        .filter(|binding| binding.trigger == SignalTrigger::Click)
    {
        params.writer.write(ElementSignal {
            name: Cow::Owned(binding.name.clone()),
            trigger: SignalTrigger::Click,
            target: activate.entity,
            element: element.cloned().unwrap_or_default(),
            payload: binding.payload.clone(),
            source: SignalSource::Activation(activate.input),
        });
    }
}
