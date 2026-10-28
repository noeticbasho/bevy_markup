//! Declarative interaction signals: elements declare hooks in the HTML, and
//! the library turns interactions into buffered [`ElementSignal`]s.
//!
//! ```html
//! <div class="opt" data-on-click="select-language"
//!      data-with='{ "index": {{ loop.index0 }} }'>English</div>
//! ```
//!
//! - **`data-on-click` / `-auxclick` / `-press` / `-release` / `-enter` /
//!   `-leave`** name the signal: the value is an app-side constant, sent as
//!   [`ElementSignal::name`]. As in browsers, `click` is the primary button
//!   (or activation by keyboard/gamepad, see [`crate::focus`]), `auxclick`
//!   any other button (middle, right); `press`/`release` fire for every
//!   button. [`ElementSignal::source`] says what produced the signal: which
//!   pointer and button (mouse, touch, a custom pointer such as a VR laser),
//!   or which input activated the focused element. Unknown `data-on-*` attributes are skipped
//!   (logged at `debug`), as are hooks on elements that produce no entity
//!   (`html`, `body`, unknown tags outside blocks; `button` is not a
//!   container — use `div`).
//! - **`data-with`** is a JSON object rendered by Tera with the template's
//!   context, like `data-l10n-args`: a snapshot of the data at render time.
//!   Numbers stay numbers; invalid JSON becomes `null` (logged at `debug`).
//!   It applies to all of the element's hooks; absent means [`Value::Null`].
//! - **`data-*`**: every signal also carries its element, with the element's
//!   `data-*` attributes as [`HtmlElement::dataset`] — read them with
//!   [`ElementSignal::data`]. Prefer one attribute per feature
//!   (`data-tooltip="…" data-selector="…"`) to packing several features'
//!   keys into one `data-with` object.
//!
//! Signals arrive as one buffered message type; drain it with
//! `MessageReader<ElementSignal>` wherever the app likes (usually an
//! `Update` system). Nested bound elements: the deepest one under the
//! pointer wins — a parent's hook is suppressed when the interaction hit a
//! bound child — so boxes can nest buttons. Hover (`enter`/`leave`) counts
//! for a whole element subtree, like CSS `:hover`.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use bevy::ecs::system::{SystemId, SystemParam};
use bevy::picking::events::{PointerClick, PointerEvent, PointerPress, PointerRelease};
use bevy::picking::hover::{HoverMap, PickingInteraction};
use bevy::picking::pointer::{PointerButton, PointerId};
use bevy::prelude::*;
#[cfg(feature = "text-input")]
use bevy::text::EditableText;
use serde_json::Value;

use crate::html::HtmlElement;
use crate::template::decode_attribute;

/// Which interaction emits a signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalTrigger {
    /// Primary-button click, or activation of the focused element.
    Click,
    /// Click with any other button (middle, right, …).
    AuxClick,
    Press,
    Release,
    Enter,
    Leave,
    /// A form was submitted: the payload carries the named inputs' values.
    /// Fired when a submit button in the form is clicked.
    Submit,
}

/// One declared hook: the signal name and its rendered payload.
#[derive(Clone, Debug, PartialEq)]
pub struct SignalBinding {
    pub trigger: SignalTrigger,
    pub name: String,
    pub payload: Value,
}

/// The `data-on-*` hooks of one element.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct ElementSignals(pub Vec<SignalBinding>);

/// A UI interaction on an element that declares a `data-on-*` hook.
#[derive(Message, Clone, Debug)]
pub struct ElementSignal {
    /// The `data-on-*` value, an app-side constant.
    pub name: Cow<'static, str>,
    /// The trigger that fired.
    pub trigger: SignalTrigger,
    /// The bound element.
    pub target: Entity,
    /// The element's `tag`/`id`/`classes`.
    pub element: HtmlElement,
    /// The rendered `data-with`, or `null`.
    pub payload: Value,
    /// What produced the signal.
    pub source: SignalSource,
}

impl ElementSignal {
    /// The bound element's `data-<key>` attribute (its
    /// [`dataset`](HtmlElement::dataset) as rendered when it was last
    /// updated): per-feature data for a hook, e.g. `data-tooltip="…"`.
    pub fn data(&self, key: &str) -> Option<&str> {
        self.element.data(key)
    }
}

/// What produced an [`ElementSignal`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SignalSource {
    /// A pointer click, press or release.
    Pointer {
        /// The mouse, a touch, or a custom pointer (a VR laser, a mocked
        /// cursor, …: `PointerId::Custom` ids are the app's to tell apart).
        pointer: PointerId,
        button: PointerButton,
        /// Pointer position in viewport px.
        position: Vec2,
        /// Consecutive clicks (2 for a double click); 1 for press/release.
        count: u8,
    },
    /// The pointer that entered or left the element.
    Hover { pointer: PointerId },
    /// The focused element was activated
    /// ([`HtmlFocus::activate`](crate::focus::HtmlFocus::activate)) by this
    /// input, as the app reported it.
    Activation(ActivationInput),
}

/// The input that activated the focused element, reported by the app when
/// it calls [`HtmlFocus::activate`](crate::focus::HtmlFocus::activate) —
/// bevy_markup reads no input itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationInput {
    /// A keyboard key (Enter, Space, …).
    Key(KeyCode),
    /// A gamepad button (South, …) of this gamepad entity.
    GamepadButton {
        gamepad: Entity,
        button: GamepadButton,
    },
    /// Generated by the app rather than by an input device: a test or tool
    /// harness, a scripted tutorial, a replay.
    Synthetic,
    /// Any other input the app maps to activation.
    Other,
}

const TRIGGERS: [(&str, SignalTrigger); 7] = [
    ("data-on-click", SignalTrigger::Click),
    ("data-on-auxclick", SignalTrigger::AuxClick),
    ("data-on-press", SignalTrigger::Press),
    ("data-on-release", SignalTrigger::Release),
    ("data-on-enter", SignalTrigger::Enter),
    ("data-on-leave", SignalTrigger::Leave),
    ("data-on-submit", SignalTrigger::Submit),
];

/// The `data-on-*`/`data-with` attributes of `tag`, parsed.
pub(crate) fn signal_bindings(tag: &tl::HTMLTag) -> Vec<SignalBinding> {
    let attributes = tag.attributes();
    let mut bindings = Vec::new();
    let mut payload = Value::Null;
    let mut payload_parsed = false;
    for (attribute, trigger) in TRIGGERS {
        let Some(Some(value)) = attributes.get(attribute) else {
            continue;
        };
        let name = decode_attribute(value.as_utf8_str()).into_owned();
        if name.trim().is_empty() {
            debug!("html signals: empty {attribute} value; skipped");
            continue;
        }
        if !payload_parsed {
            payload = match attributes.get("data-with").flatten() {
                Some(value) => {
                    let json = decode_attribute(value.as_utf8_str());
                    match crate::template::parse_attribute_json(&json) {
                        Ok(payload) => payload,
                        Err(error) => {
                            debug!("html signals: invalid data-with JSON ({error}): {json:?}");
                            Value::Null
                        }
                    }
                }
                None => Value::Null,
            };
            payload_parsed = true;
        }
        bindings.push(SignalBinding {
            trigger,
            name,
            payload: payload.clone(),
        });
    }
    bindings
}

/// Marks an element whose pointer observers ([`observe_pointer_signals`])
/// are attached.
#[derive(Component)]
pub(crate) struct PointerSignalObservers;

/// An `<input>`'s form identity: submissions collect the field's value
/// under this name (unnamed inputs are excluded from submissions).
/// Constructed behind the `text-input` feature.
#[derive(Component, Debug, Deref)]
#[cfg_attr(not(feature = "text-input"), expect(dead_code))]
pub(crate) struct InputName(pub String);

/// Marks a `<button type="submit">` inside a form: clicking it collects
/// the enclosing form's named inputs and emits the form's `Submit` signal.
/// For the deepest-bound-element rule it counts as bound for `click`: a
/// bound ancestor's `click` doesn't fire for a click on it.
#[derive(Component)]
pub(crate) struct FormSubmitButton;

/// Attaches the click/press/release observers to `entity` once. They read
/// the element's [`ElementSignals`] when the event arrives, so an in-place
/// update that changes the bindings (or their `data-with`) needs no new
/// observers. `enter`/`leave` are handled by [`hover_signals`] instead:
/// pointer `Over`/`Out` events don't reach ancestors — the hover map holds
/// only the deepest picked node, since nodes block picking by default.
pub(crate) fn observe_pointer_signals(entity: &mut EntityCommands) {
    entity.queue(|mut entity: EntityWorldMut| {
        if entity.contains::<PointerSignalObservers>() {
            return;
        }
        entity.insert(PointerSignalObservers);
        entity.observe(on_click);
        entity.observe(on_press);
        entity.observe(on_release);
    });
}

/// The pointer events with `data-on-*` triggers.
trait PointerSignal: PointerEvent + std::fmt::Debug + Clone + Reflect {
    /// The trigger this event fires and the button behind it. Named
    /// `signal_trigger` to stay clear of `Event`'s associated `Trigger`.
    fn signal_trigger(&self) -> (SignalTrigger, PointerButton);
    /// Consecutive clicks.
    fn count(&self) -> u8 {
        1
    }
}

impl PointerSignal for PointerClick {
    fn signal_trigger(&self) -> (SignalTrigger, PointerButton) {
        let trigger = match self.button {
            PointerButton::Primary => SignalTrigger::Click,
            _ => SignalTrigger::AuxClick,
        };
        (trigger, self.button)
    }
    fn count(&self) -> u8 {
        self.count
    }
}

impl PointerSignal for PointerPress {
    fn signal_trigger(&self) -> (SignalTrigger, PointerButton) {
        (SignalTrigger::Press, self.button)
    }
}

impl PointerSignal for PointerRelease {
    fn signal_trigger(&self) -> (SignalTrigger, PointerButton) {
        (SignalTrigger::Release, self.button)
    }
}

/// System params shared by the three pointer observers.
#[derive(SystemParam)]
struct PointerParams<'w, 's> {
    writer: MessageWriter<'w, ElementSignal>,
    signals: Query<'w, 's, &'static ElementSignals>,
    parents: Query<'w, 's, &'static ChildOf>,
    elements: Query<'w, 's, &'static HtmlElement>,
    submits: Query<'w, 's, (), With<FormSubmitButton>>,
    #[cfg(feature = "text-input")]
    children: Query<'w, 's, &'static Children>,
    #[cfg(feature = "text-input")]
    editable: Query<'w, 's, &'static EditableText>,
    #[cfg(feature = "text-input")]
    names: Query<'w, 's, &'static InputName>,
}

fn on_click(event: On<PointerClick>, params: PointerParams) {
    let hit = event.original_event_target();
    on_pointer(&*event, hit, params)
}

fn on_press(event: On<PointerPress>, params: PointerParams) {
    let hit = event.original_event_target();
    on_pointer(&*event, hit, params)
}

fn on_release(event: On<PointerRelease>, params: PointerParams) {
    let hit = event.original_event_target();
    on_pointer(&*event, hit, params)
}

/// System params for activation (Enter / gamepad A): the same queries the
/// pointer observers use, so activation on a submit button submits its
/// form and activation bindings write like click bindings.
#[derive(SystemParam)]
pub(crate) struct ActivationParams<'w, 's> {
    pub(crate) writer: MessageWriter<'w, ElementSignal>,
    pub(crate) signals: Query<'w, 's, &'static ElementSignals>,
    pub(crate) parents: Query<'w, 's, &'static ChildOf>,
    pub(crate) elements: Query<'w, 's, &'static HtmlElement>,
    /// The elements paired with their `Option<&HtmlElement>` (the
    /// `activate_element` observer's shape).
    pub(crate) elements_and_state:
        Query<'w, 's, (&'static ElementSignals, Option<&'static HtmlElement>)>,
    pub(crate) submits: Query<'w, 's, (), With<FormSubmitButton>>,
    #[cfg(feature = "text-input")]
    pub(crate) children: Query<'w, 's, &'static Children>,
    #[cfg(feature = "text-input")]
    pub(crate) editable: Query<'w, 's, &'static EditableText>,
    #[cfg(feature = "text-input")]
    pub(crate) names: Query<'w, 's, &'static InputName>,
}

/// Emits the observed element's bindings for the event's trigger, unless a
/// deeper bound element under the pointer owns it. `event_target()` is the
/// entity this invocation runs for; `hit` the deepest picked entity
/// (`On::original_event_target`), extracted by the concrete observer.
fn on_pointer<E: PointerSignal>(event: &E, hit: Entity, mut params: PointerParams) {
    let target = event.event_target();
    let (trigger, button) = event.signal_trigger();
    let Ok(bound) = params.signals.get(target) else {
        return;
    };
    if covered_by_deeper(
        hit,
        target,
        trigger,
        &params.signals,
        &params.submits,
        &params.parents,
    ) {
        return;
    }
    let pointer = event.pointer();
    let source = SignalSource::Pointer {
        pointer: pointer.id,
        button,
        position: pointer.position,
        count: event.count(),
    };
    for binding in bound.0.iter().filter(|binding| binding.trigger == trigger) {
        params.writer.write(ElementSignal {
            name: Cow::Owned(binding.name.clone()),
            trigger,
            target,
            element: params.elements.get(target).cloned().unwrap_or_default(),
            payload: binding.payload.clone(),
            source,
        });
    }

    // A submit button clicked: collect the enclosing form's named inputs
    // and emit the form's `Submit` signal with their values (ADR: form
    // submissions are signals — Bevy has no HTTP client, the app decides
    // what submitting means). Only submit buttons: any other bound element
    // in a form (`type="button"`, a link, a bound wrapper) just clicks.
    if trigger == SignalTrigger::Click && params.submits.contains(target) {
        emit_form_submit(target, source.clone(), &mut params);
    }
}

/// Finds the enclosing form of `button` (a `<form>` whose bindings include
/// a `Submit` hook), collects the named inputs' values from its subtree,
/// and emits the form's `Submit` signal.
fn emit_form_submit(button: Entity, source: SignalSource, params: &mut PointerParams) {
    emit_form(
        button,
        source,
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

/// Submits `button`'s enclosing form: the form's `Submit` signal with the
/// named inputs' values. Called by the pointer observers (the click's
/// source) and by activation on a submit button (Enter / gamepad A).
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_form(
    button: Entity,
    source: SignalSource,
    writer: &mut MessageWriter<ElementSignal>,
    parents: &Query<'_, '_, &'static ChildOf>,
    elements: &Query<'_, '_, &'static HtmlElement>,
    signals: &Query<'_, '_, &'static ElementSignals>,
    #[cfg(feature = "text-input")] children: &Query<'_, '_, &'static Children>,
    #[cfg(feature = "text-input")] editable: &Query<'_, '_, &'static EditableText>,
    #[cfg(feature = "text-input")] names: &Query<'_, '_, &'static InputName>,
) {
    // The nearest form ancestor with a `data-on-submit` hook.
    let Some((form, name)) = std::iter::successors(
        parents.get(button).ok().map(|child_of| child_of.parent()),
        |current| parents.get(*current).ok().map(|child_of| child_of.parent()),
    )
    .find_map(|form| {
        let element = elements.get(form).ok()?;
        if element.tag != "form" {
            return None;
        }
        let signals = signals.get(form).ok()?;
        let binding = signals
            .0
            .iter()
            .find(|binding| binding.trigger == SignalTrigger::Submit)?;
        Some((form, binding.name.clone()))
    }) else {
        return;
    };

    // The named inputs' values in the form's subtree, in tree order.
    // Without the `text-input` feature, `<input>`s don't spawn: the form
    // submits with an empty payload.
    #[allow(unused_mut)]
    let mut payload = serde_json::Map::new();
    #[cfg(feature = "text-input")]
    collect_input_values(form, children, editable, names, &mut payload);

    writer.write(ElementSignal {
        name: Cow::Owned(name),
        trigger: SignalTrigger::Submit,
        target: form,
        element: elements.get(form).cloned().unwrap_or_default(),
        payload: serde_json::Value::Object(payload),
        source,
    });
}

/// Walks `form`'s subtree and inserts each named input's live text under
/// its name (tree order; unnamed inputs excluded).
#[cfg(feature = "text-input")]
fn collect_input_values(
    form: Entity,
    children: &Query<'_, '_, &'static Children>,
    editable: &Query<'_, '_, &'static EditableText>,
    names: &Query<'_, '_, &'static InputName>,
    payload: &mut serde_json::Map<String, serde_json::Value>,
) {
    let mut stack: Vec<Entity> = vec![form];
    while let Some(entity) = stack.pop() {
        if let Ok(children) = children.get(entity) {
            stack.extend(children.iter().rev());
        }
        if let Ok(name) = names.get(entity)
            && let Ok(editable) = editable.get(entity)
        {
            payload.insert(
                name.0.clone(),
                serde_json::json!(editable.editor.text().to_string()),
            );
        }
    }
}

/// Whether the interaction hit a bound element between `hit` and `target`
/// (excluding `target`, which is checked by its own invocation): that
/// element is deeper and owns the interaction. A form's submit button
/// counts as bound for `click` (its click is the form's submit). `false`
/// when `target` isn't an ancestor of `hit` at all.
fn covered_by_deeper(
    hit: Entity,
    target: Entity,
    trigger: SignalTrigger,
    signals: &Query<&ElementSignals>,
    submits: &Query<(), With<FormSubmitButton>>,
    parents: &Query<&ChildOf>,
) -> bool {
    let mut current = hit;
    let mut deeper = false;
    loop {
        if current == target {
            return deeper;
        }
        deeper |= signals
            .get(current)
            .is_ok_and(|signals| signals.0.iter().any(|binding| binding.trigger == trigger))
            || (trigger == SignalTrigger::Click && submits.contains(current));
        match parents.get(current) {
            Ok(parent) => current = parent.0,
            // `target` isn't an ancestor of `hit`; don't suppress.
            Err(_) => return false,
        }
    }
}

/// Signal handlers registered with [`HtmlSignalsExt`], by signal name and
/// (for [`on_html_click`](HtmlSignalsExt::on_html_click)) trigger.
#[derive(Resource, Default)]
pub(crate) struct SignalHandlers(HashMap<String, Vec<Handler>>);

/// A handler: its trigger filter (`None` = every trigger) and system.
type Handler = (Option<SignalTrigger>, SystemId<In<ElementSignal>>);

/// Routes [`ElementSignal`]s to systems by name, instead of one
/// `MessageReader` matching names: each handler is an ordinary system taking
/// the signal as `In<ElementSignal>`.
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use bevy_markup::prelude::*;
/// # #[derive(Event)] struct Play;
/// fn play(signal: In<ElementSignal>, mut commands: Commands) {
///     info!("play, via {:?}", signal.source);
///     commands.trigger(Play);
/// }
/// # let mut app = App::new();
/// app.on_html_click("lobby.play", play);
/// ```
///
/// Handlers run in `PostUpdate` before [`HtmlUiSystems::Render`](crate::HtmlUiSystems),
/// so what they change shows the same frame. The message is still sent;
/// readers and handlers can be mixed.
pub trait HtmlSignalsExt {
    /// Runs `system` for every signal named `name`, whatever its trigger.
    fn on_html_signal<M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<ElementSignal>, (), M> + 'static,
    ) -> &mut Self;

    /// Runs `system` for `name`'s [`SignalTrigger::Click`]s: primary-button
    /// clicks and activations.
    fn on_html_click<M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<ElementSignal>, (), M> + 'static,
    ) -> &mut Self;
}

fn add_handler<M>(
    app: &mut App,
    name: String,
    trigger: Option<SignalTrigger>,
    system: impl IntoSystem<In<ElementSignal>, (), M> + 'static,
) {
    let world = app.world_mut();
    let id = world.register_system(system);
    world
        .get_resource_or_init::<SignalHandlers>()
        .0
        .entry(name)
        .or_default()
        .push((trigger, id));
}

impl HtmlSignalsExt for App {
    fn on_html_signal<M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<ElementSignal>, (), M> + 'static,
    ) -> &mut Self {
        add_handler(self, name.into(), None, system);
        self
    }

    fn on_html_click<M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<ElementSignal>, (), M> + 'static,
    ) -> &mut Self {
        add_handler(self, name.into(), Some(SignalTrigger::Click), system);
        self
    }
}

/// Runs the registered handlers for the signals sent since the last run.
#[hotpath::measure]
pub(crate) fn dispatch_signals(
    world: &mut World,
    mut cursor: Local<bevy::ecs::message::MessageCursor<ElementSignal>>,
) {
    let Some(handlers) = world.get_resource::<SignalHandlers>() else {
        return;
    };
    let messages = world.resource::<Messages<ElementSignal>>();
    let runs: Vec<(SystemId<In<ElementSignal>>, ElementSignal)> = cursor
        .read(messages)
        .flat_map(|signal| {
            handlers
                .0
                .get(signal.name.as_ref())
                .into_iter()
                .flatten()
                .filter(|(trigger, _)| trigger.is_none_or(|trigger| trigger == signal.trigger))
                .map(|&(_, id)| (id, signal.clone()))
        })
        .collect();
    for (id, signal) in runs {
        let name = signal.name.clone();
        if let Err(error) = world.run_system_with(id, signal) {
            warn!("html signals: handler for {name:?}: {error}");
        }
    }
}

/// Tracks pointers over `data-on-enter`/`-leave` elements and emits their/// Tracks pointers over `data-on-enter`/`-leave` elements and emits their
/// signals. An element counts as hovered while the pointer is over it *or
/// any descendant* (like CSS `:hover`): the hover map only holds the
/// deepest picked node, so ancestors are matched by hand.
///
/// A `leave` must fire even when the element was despawned in the meantime
/// (a rebuild under a stationary cursor), so entering snapshots the
/// element's `leave` bindings and identity for the emit.
#[hotpath::measure]
pub(crate) fn hover_signals(
    hover_map: Option<Res<HoverMap>>,
    mut entered: Local<HashMap<PointerId, HashMap<Entity, Hovering>>>,
    bound: Query<(Entity, &ElementSignals)>,
    parents: Query<&ChildOf>,
    elements: Query<&HtmlElement>,
    mut writer: MessageWriter<ElementSignal>,
) {
    let Some(hover_map) = hover_map else {
        // No picking plugins (headless tests).
        return;
    };

    // Where the pointers are, in terms of bound elements: each hovered node
    // plus its ancestors.
    let mut now: HashMap<PointerId, HashSet<Entity>> = HashMap::new();
    for (pointer, hovered) in hover_map.iter() {
        let set = now.entry(*pointer).or_default();
        for entity in hovered.keys() {
            let mut current = Some(*entity);
            while let Some(node) = current {
                if let Ok((_, signals)) = bound.get(node)
                    && signals.0.iter().any(|binding| {
                        matches!(binding.trigger, SignalTrigger::Enter | SignalTrigger::Leave)
                    })
                {
                    set.insert(node);
                }
                current = parents.get(node).ok().map(|parent| parent.0);
            }
        }
    }

    for (pointer, set) in &now {
        for &entity in set {
            if entered
                .get(pointer)
                .is_some_and(|previous| previous.contains_key(&entity))
            {
                continue;
            }
            // Entering: remember the `leave` side for the (possible) emit.
            let Ok((_, signals)) = bound.get(entity) else {
                continue;
            };
            let leaves = signals
                .0
                .iter()
                .filter(|binding| binding.trigger == SignalTrigger::Leave)
                .cloned()
                .collect::<Vec<_>>();
            let element = elements.get(entity).cloned().unwrap_or_default();
            for binding in &signals.0 {
                if binding.trigger == SignalTrigger::Enter {
                    writer.write(ElementSignal {
                        name: Cow::Owned(binding.name.clone()),
                        trigger: SignalTrigger::Enter,
                        target: entity,
                        element: element.clone(),
                        payload: binding.payload.clone(),
                        source: SignalSource::Hover { pointer: *pointer },
                    });
                }
            }
            entered
                .entry(*pointer)
                .or_default()
                .insert(entity, Hovering { element, leaves });
        }
    }
    // Leaving (or despawned by a rebuild), per entered element; a pointer
    // that vanished (Bevy despawns touch pointers on touch end and drops
    // them from the hover map) emits every entered element's leaves and
    // is dropped itself, so the outer map doesn't grow by one empty
    // entry per touch.
    entered.retain(|pointer, previous| {
        let Some(set) = now.get(pointer) else {
            for (entity, hovering) in previous.drain() {
                for binding in &hovering.leaves {
                    writer.write(ElementSignal {
                        name: Cow::Owned(binding.name.clone()),
                        trigger: SignalTrigger::Leave,
                        target: entity,
                        element: hovering.element.clone(),
                        payload: binding.payload.clone(),
                        source: SignalSource::Hover { pointer: *pointer },
                    });
                }
            }
            return false;
        };
        previous.retain(|entity, hovering| {
            if set.contains(entity) {
                return true;
            }
            for binding in &hovering.leaves {
                writer.write(ElementSignal {
                    name: Cow::Owned(binding.name.clone()),
                    trigger: SignalTrigger::Leave,
                    target: *entity,
                    element: hovering.element.clone(),
                    payload: binding.payload.clone(),
                    source: SignalSource::Hover { pointer: *pointer },
                });
            }
            false
        });
        true
    });
}

/// What a hovering pointer remembers about one entered element.
pub(crate) struct Hovering {
    element: HtmlElement,
    leaves: Vec<SignalBinding>,
}

/// The interaction pseudo-state of an element: `:hover` / `:active` /
/// `:focus` / `:focus-visible` for the cascade. Hover and active are
/// maintained from picking, focus from `InputFocus` (see [`crate::focus`]);
/// a change restyles the element's UI (in place). Apps may also set it to
/// force the styles.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PseudoState {
    pub hovered: bool,
    pub active: bool,
    pub focused: bool,
    pub focus_visible: bool,
}

/// Maintains [`PseudoState`] on every element from the picking hover map
/// and the pressed entities: an element is hovered while the pointer is
/// over it or any descendant, and active while a press started on it or a
/// descendant (like CSS).
#[hotpath::measure]
pub(crate) fn update_pseudo_states(
    hover_map: Option<Res<HoverMap>>,
    interactions: Query<(Entity, &PickingInteraction)>,
    elements: Query<(Entity, &HtmlElement, Option<&PseudoState>)>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let Some(hover_map) = hover_map else {
        // No picking plugins (headless tests).
        return;
    };
    let chain = |root: Entity, set: &mut HashSet<Entity>| {
        let mut current = Some(root);
        while let Some(node) = current {
            set.insert(node);
            current = parents.get(node).ok().map(|parent| parent.0);
        }
    };
    let mut hovered = HashSet::new();
    for hovered_entities in hover_map.values() {
        for entity in hovered_entities.keys() {
            chain(*entity, &mut hovered);
        }
    }
    let mut active = HashSet::new();
    for (entity, interaction) in &interactions {
        if *interaction == PickingInteraction::Pressed {
            chain(entity, &mut active);
        }
    }
    for (entity, _, state) in &elements {
        let current = state.copied().unwrap_or_default();
        let desired = PseudoState {
            hovered: hovered.contains(&entity),
            active: active.contains(&entity),
            ..current
        };
        // An all-default state on an element without the component is the
        // same as no component (CSS matching treats both as unhovered):
        // inserting it would only fire a spurious change on first build.
        if desired == PseudoState::default() && state.is_none() {
            continue;
        }
        if state.copied() != Some(desired) {
            // `try_`: an app system may despawn the UI this frame, its
            // commands applied before these (bug_0021).
            commands.entity(entity).try_insert(desired);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::NormalizedRenderTarget;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::picking::backend::HitData;
    use bevy::picking::events::Pointer;
    use bevy::picking::pointer::Location;
    use bevy::window::WindowRef;
    use std::time::Duration;

    /// bug_0021: an app system despawning a UI in the same frame (its
    /// command buffer applied first) must not make the queued `PseudoState`
    /// insert panic.
    #[test]
    fn pseudo_states_tolerate_a_same_frame_despawn() {
        let mut app = App::new();
        app.init_resource::<HoverMap>();
        let element = app.world_mut().spawn(HtmlElement::default()).id();
        app.add_systems(
            Update,
            (
                move |mut commands: Commands| commands.entity(element).despawn(),
                update_pseudo_states,
            )
                .chain_ignore_deferred(),
        );
        app.update();
        assert!(app.world().get_entity(element).is_err());
    }
    use tl::parse;

    fn bindings(html: &str) -> Vec<SignalBinding> {
        let dom = parse(html, tl::ParserOptions::default()).expect("parse");
        let tag = dom.children().iter().find_map(|handle| {
            handle.get(dom.parser()).and_then(|node| match node {
                tl::Node::Tag(tag) => Some(tag),
                _ => None,
            })
        });
        signal_bindings(tag.expect("tag"))
    }

    #[test]
    fn hooks_parse_with_payloads() {
        let bindings = bindings(
            r#"<div data-on-click="buy" data-on-enter="peek"
                     data-with='{ "n": 3, "id": "torch" }'></div>"#,
        );
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].trigger, SignalTrigger::Click);
        assert_eq!(bindings[0].name, "buy");
        assert_eq!(bindings[0].payload["n"], 3);
        assert_eq!(bindings[0].payload["id"], "torch");
        assert_eq!(bindings[1].trigger, SignalTrigger::Enter);
        assert_eq!(bindings[1].payload["id"], "torch");
    }

    #[test]
    fn absent_or_invalid_payload_is_null() {
        let parsed = bindings(r#"<div data-on-leave="unpeek"></div>"#);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].trigger, SignalTrigger::Leave);
        assert_eq!(parsed[0].payload, Value::Null);

        let parsed = bindings(r#"<div data-on-click="x" data-with="{oops}"></div>"#);
        assert_eq!(parsed[0].payload, Value::Null);
    }

    #[test]
    fn unknown_triggers_and_empty_names_are_skipped() {
        let parsed =
            bindings(r#"<div data-on-drag="x" data-on-click="" data-on-press="  "></div>"#);
        assert!(parsed.is_empty());
    }

    #[test]
    fn entities_are_decoded_in_names_and_payloads() {
        let parsed =
            bindings(r#"<div data-on-click="a &amp; b" data-with='{"s": "x &lt; y"}'></div>"#);
        assert_eq!(parsed[0].name, "a & b");
        assert_eq!(parsed[0].payload["s"], "x < y");
    }

    /// A deeper bound element suppresses its ancestors, and only for its own
    /// trigger; an unrelated hit doesn't suppress.
    #[test]
    fn deeper_bound_element_wins() {
        let mut world = World::new();
        let binding = |trigger: SignalTrigger| SignalBinding {
            trigger,
            name: "x".into(),
            payload: Value::Null,
        };
        // parent ← middle ← hit; only `hit` (the deepest node) is bound for
        // click, `parent` for click and enter.
        let parent = world
            .spawn(ElementSignals(vec![
                binding(SignalTrigger::Click),
                binding(SignalTrigger::Enter),
            ]))
            .id();
        let middle = world.spawn_empty().id();
        world.entity_mut(middle).insert(ChildOf(parent));
        let hit = world
            .spawn(ElementSignals(vec![binding(SignalTrigger::Click)]))
            .insert(ChildOf(middle))
            .id();
        let unrelated = world.spawn_empty().id();

        let (click, enter, outside) = world
            .run_system_once(
                move |signals: Query<&ElementSignals>,
                      parents: Query<&ChildOf>,
                      submits: Query<(), With<FormSubmitButton>>| {
                    (
                        covered_by_deeper(
                            hit,
                            parent,
                            SignalTrigger::Click,
                            &signals,
                            &submits,
                            &parents,
                        ),
                        covered_by_deeper(
                            hit,
                            parent,
                            SignalTrigger::Enter,
                            &signals,
                            &submits,
                            &parents,
                        ),
                        covered_by_deeper(
                            hit,
                            unrelated,
                            SignalTrigger::Click,
                            &signals,
                            &submits,
                            &parents,
                        ),
                    )
                },
            )
            .unwrap();
        // The child is bound for click: the parent's click is suppressed,
        // its enter isn't. An unrelated hit suppresses nothing.
        assert!(click);
        assert!(!enter);
        assert!(!outside);
    }

    /// Primary clicks fire `Click` (with the click count); every other
    /// button fires `AuxClick`; presses and releases keep their triggers and
    /// button, with a count of 1.
    #[test]
    fn click_triggers_follow_the_button() {
        let pointer = Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::Window(
                    WindowRef::Primary
                        .normalize(Some(Entity::PLACEHOLDER))
                        .unwrap(),
                ),
                position: Vec2::ZERO,
            },
        );
        let hit = || HitData::new(Entity::PLACEHOLDER, 0.0, None, None);
        for button in [
            PointerButton::Primary,
            PointerButton::Secondary,
            PointerButton::Middle,
        ] {
            let click = PointerClick {
                entity: Entity::PLACEHOLDER,
                pointer: pointer.clone(),
                button,
                hit: hit(),
                duration: Duration::ZERO,
                count: 2,
            };
            let (trigger, event_button) = click.signal_trigger();
            assert_eq!(event_button, button);
            assert_eq!(
                trigger,
                if button == PointerButton::Primary {
                    SignalTrigger::Click
                } else {
                    SignalTrigger::AuxClick
                }
            );
            assert_eq!(click.count(), 2);

            let press = PointerPress {
                entity: Entity::PLACEHOLDER,
                pointer: pointer.clone(),
                button,
                hit: hit(),
                count: 1,
            };
            assert_eq!(press.signal_trigger(), (SignalTrigger::Press, button));
            assert_eq!(press.count(), 1);

            let release = PointerRelease {
                entity: Entity::PLACEHOLDER,
                pointer: pointer.clone(),
                button,
                hit: hit(),
            };
            assert_eq!(release.signal_trigger(), (SignalTrigger::Release, button));
            assert_eq!(release.count(), 1);
        }
    }
}
