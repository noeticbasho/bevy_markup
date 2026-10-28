//! Interaction signals end to end: the harness writes `WindowEvent`s (the
//! input winit sends), Bevy's real picking stack runs (`PointerInputPlugin`
//! → UI picking backend → pointer events), and the tests assert the
//! [`ElementSignal`]s and `PseudoState`s bevy_markup derives from it.
//!
//! Needs [`TestUi::with_pointer`] (layout for hit testing, a primary window
//! and a camera targeting it); every pointer step runs one frame, like real
//! input, so clicks span three frames (move, press, release).

mod common;

#[cfg(feature = "text-input")]
use bevy::ecs::message::Messages;
use bevy::input::ButtonState;
#[cfg(feature = "text-input")]
use bevy::input::keyboard::Key;
#[cfg(feature = "text-input")]
use bevy::input::keyboard::KeyboardInput;
use bevy::input::touch::TouchPhase;
use bevy::input_focus::{FocusCause, InputFocus, InputFocusVisible};
use bevy::math::Vec2;
use bevy::picking::pointer::{PointerButton, PointerId};
use bevy::prelude::*;
use bevy::window::WindowEvent;
use bevy_markup::prelude::*;
use common::{TestUi, node_rect};

const VIEWPORT: UVec2 = UVec2::new(640, 480);

/// A page under a full-width column root, the way apps set up their `HtmlUi`.
fn page_ui(name: &str, page: &str, css: &str, context: TemplateContext) -> TestUi {
    TestUi::with_pointer(
        name,
        &[
            ("page.html", page),
            ("slot.html", "<p>slot</p>"),
            ("style.css", css),
        ],
        VIEWPORT,
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        context,
        Node {
            flex_direction: FlexDirection::Column,
            ..default()
        },
    )
}

fn element_by_id(world: &mut World, root: Entity, id: &str) -> Entity {
    let mut elements = world.query::<(Entity, &HtmlElement)>();
    let candidates: Vec<Entity> = elements
        .iter(world)
        .filter(|(_, element)| element.id.as_deref() == Some(id))
        .map(|(entity, _)| entity)
        .collect();
    candidates
        .into_iter()
        .find(|&entity| {
            std::iter::successors(Some(entity), |&current| {
                world.get::<ChildOf>(current).map(ChildOf::parent)
            })
            .any(|ancestor| ancestor == root)
        })
        .unwrap_or_else(|| panic!("no #{id} below {root}"))
}

/// The element's laid-out center, for pointer moves that must land on it.
fn center(world: &mut World, root: Entity, id: &str) -> (Entity, Vec2) {
    let entity = element_by_id(world, root, id);
    let rect = node_rect(world, entity).expect("laid out");
    (entity, rect.center())
}

fn pseudo(world: &World, entity: Entity) -> PseudoState {
    world
        .get::<PseudoState>(entity)
        .copied()
        .unwrap_or_default()
}

#[test]
fn click_fires_press_release_and_click_with_payload_position_and_element() {
    let page = r#"<div id="btn" data-on-click="buy" data-on-press="down"
                         data-on-release="up" data-with='{ "n": {{ n }} }'><p>Buy</p></div>"#;
    let mut ui = page_ui(
        "signals-click",
        page,
        "",
        TemplateContext::new().with("n", &3),
    );
    ui.settle();
    ui.update(1);
    let (btn, at) = {
        let root = ui.root();
        let world = ui.world_mut();
        center(world, root, "btn")
    };

    // Moving onto the element emits nothing: no enter/leave declared.
    ui.move_pointer(at);
    assert!(
        ui.take_signals().is_empty(),
        "hover emits no pointer signals"
    );

    // Pressing fires the press hook only.
    ui.press_pointer();
    let signals = ui.take_signals();
    assert_eq!(signals.len(), 1, "press");
    let signal = &signals[0];
    assert_eq!(
        (signal.name.as_ref(), signal.trigger),
        ("down", SignalTrigger::Press)
    );
    assert_eq!(signal.target, btn);
    assert_eq!(signal.element.tag, "div");
    assert_eq!(signal.element.id.as_deref(), Some("btn"));
    assert_eq!(signal.payload["n"].as_i64(), Some(3), "data-with rendered");
    assert_eq!(
        signal.source,
        SignalSource::Pointer {
            pointer: PointerId::Mouse,
            button: PointerButton::Primary,
            position: at,
            count: 1,
        },
        "mouse, primary button, viewport px"
    );

    // Releasing fires click (first) and release, at the same position.
    ui.release_pointer();
    let signals = ui.take_signals();
    eprintln!("DEBUG release frame: {signals:?}");
    assert_eq!(signals.len(), 2, "click then release");
    assert_eq!(
        (signals[0].name.as_ref(), signals[0].trigger),
        ("buy", SignalTrigger::Click)
    );
    assert_eq!(
        (signals[1].name.as_ref(), signals[1].trigger),
        ("up", SignalTrigger::Release)
    );
    assert!(matches!(
        signals[1].source,
        SignalSource::Pointer { position, button: PointerButton::Primary, .. } if position == at
    ));
}

/// `on_html_click` runs its system for the name's clicks (pointer and
/// activation) only; `on_html_signal` for every trigger; each signal once.
#[test]
fn signals_route_to_registered_systems() {
    #[derive(Resource, Default)]
    struct Log(Vec<(&'static str, SignalTrigger, bool)>);

    let page = r#"<div id="btn" data-on-click="buy" data-on-press="buy"><p>Buy</p></div>"#;
    let mut ui = page_ui("signals-route", page, "", TemplateContext::new());
    ui.app_mut()
        .init_resource::<Log>()
        .on_html_click("buy", |signal: In<ElementSignal>, mut log: ResMut<Log>| {
            let activated = matches!(signal.source, SignalSource::Activation(_));
            log.0.push(("click", signal.trigger, activated));
        })
        .on_html_signal("buy", |signal: In<ElementSignal>, mut log: ResMut<Log>| {
            log.0.push(("any", signal.trigger, false));
        });
    ui.settle();
    ui.update(1);
    let root = ui.root();
    let (btn, at) = center(ui.world_mut(), root, "btn");
    let take = |ui: &mut TestUi| std::mem::take(&mut ui.world_mut().resource_mut::<Log>().0);

    ui.click_at(at).update(2);
    let mut log = take(&mut ui);
    log.sort_by_key(|entry| format!("{entry:?}"));
    assert_eq!(
        log,
        [
            ("any", SignalTrigger::Click, false),
            ("any", SignalTrigger::Press, false),
            ("click", SignalTrigger::Click, false),
        ]
    );

    ui.world_mut().trigger(ActivateElement {
        entity: btn,
        input: ActivationInput::Synthetic,
    });
    ui.update(2);
    let log = take(&mut ui);
    assert!(
        log.contains(&("click", SignalTrigger::Click, true)),
        "{log:?}"
    );
    assert_eq!(log.len(), 2, "click + any, once each: {log:?}");

    ui.update(3);
    assert!(take(&mut ui).is_empty(), "nothing re-delivered");
}

/// A tooltip that doesn't fit on its `data-tooltip-placement` side flips
/// to the opposite one, and its template learns where it ended up: `side`
/// is `below` for an `above` tooltip on an element at the top edge, while
/// `placement` keeps what the markup asked for.
#[test]
fn a_tooltip_that_does_not_fit_flips_and_reports_its_side() {
    let page =
        r#"<div id="top" data-tooltip="tip" data-tooltip-placement="above"><p>Top</p></div>"#;
    let mut ui = TestUi::with_pointer(
        "tooltip-flip",
        &[
            ("page.html", page),
            (
                "tooltip.html",
                r#"<html style="position: absolute; width: 80px; height: 20px"><p>{{ side }}</p></html>"#,
            ),
        ],
        VIEWPORT,
    )
    .spawn(
        "page.html",
        TemplateContext::new(),
        Node {
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    let template = ui.load::<HtmlTemplate>("tooltip.html");
    ui.world_mut().insert_resource(HtmlTooltips::new(template));
    ui.settle();
    ui.update(1);
    let root = ui.root();
    let world = ui.world_mut();
    let element = element_by_id(world, root, "top");
    let element_rect = node_rect(world, element).unwrap();
    assert_eq!(element_rect.min.y, 0.0, "flush with the top edge");
    let (_, at) = center(world, root, "top");
    ui.move_pointer(at).update(4);
    let world = ui.world_mut();
    let mut query = world.query::<(Entity, &HtmlTooltip, &TemplateContext, &AnchoredSide)>();
    let (tooltip, _, context, side) = query.single(world).expect("one tooltip");
    let word = |key: &str| {
        context
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(side.0, AnchorPlacement::Below);
    assert_eq!(
        (word("placement"), word("side")),
        ("above".to_owned(), "below".to_owned())
    );
    let placed = node_rect(world, tooltip).unwrap();
    assert!(
        placed.min.y >= element_rect.max.y,
        "below the element: {placed:?} vs {element_rect:?}"
    );
}

/// `data-tooltip` shows an `HtmlTooltips` template beside the hovered
/// element: the nearest element with the attribute wins, `key`/`args`/
/// `placement` reach the template, an update refreshes the open tooltip in
/// place, and leaving despawns it.
#[test]
fn data_tooltip_shows_the_nearest_tooltip() {
    let page = r#"<div id="outer" data-tooltip="outer-tip" data-tooltip-placement="above"><p>Outer</p>
  <div id="inner" data-tooltip="inner-tip" data-tooltip-args='{"n": {{ n }}}'><p>Inner</p></div>
</div>
<div id="plain"><p>Plain</p></div>"#;
    let mut ui = TestUi::with_pointer(
        "tooltips",
        &[
            ("page.html", page),
            (
                "tooltip.html",
                "<p>{{ key }} {{ args.n | default(value=0) }}</p>",
            ),
        ],
        VIEWPORT,
    )
    .spawn(
        "page.html",
        TemplateContext::new().with("n", &1),
        Node {
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    let template = ui.load::<HtmlTemplate>("tooltip.html");
    ui.world_mut().insert_resource(HtmlTooltips::new(template));
    ui.settle();
    ui.update(1);
    let root = ui.root();
    let tooltips = |ui: &mut TestUi| {
        let world = ui.world_mut();
        let mut query = world.query::<(Entity, &HtmlTooltip, &TemplateContext, &HtmlAnchor)>();
        query
            .iter(world)
            .map(|(entity, tooltip, context, anchor)| {
                (
                    entity,
                    tooltip.element,
                    context
                        .get("key")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                    context.get("args").cloned(),
                    anchor.placement,
                )
            })
            .collect::<Vec<_>>()
    };
    let world = ui.world_mut();
    let outer = element_by_id(world, root, "outer");
    let inner = element_by_id(world, root, "inner");
    let outer_text = node_rect(world, world.get::<Children>(outer).unwrap()[0])
        .unwrap()
        .center();
    let (_, inner_at) = center(world, root, "inner");
    let (_, plain_at) = center(world, root, "plain");

    ui.move_pointer(outer_text).update(2);
    let shown = tooltips(&mut ui);
    assert_eq!(shown.len(), 1);
    assert_eq!(
        (shown[0].1, shown[0].2.as_str(), shown[0].4),
        (outer, "outer-tip", AnchorPlacement::Above)
    );

    ui.move_pointer(inner_at).update(2);
    let shown = tooltips(&mut ui);
    assert_eq!(shown.len(), 1, "the nearest data-tooltip wins");
    assert_eq!(
        (shown[0].1, shown[0].2.as_str(), shown[0].4),
        (inner, "inner-tip", AnchorPlacement::Right)
    );
    assert_eq!(
        shown[0]
            .3
            .as_ref()
            .map(|args| serde_json::to_value(args).unwrap()["n"].clone()),
        Some(1.into())
    );
    let tooltip = shown[0].0;

    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &2);
    ui.settle();
    ui.update(2);
    let shown = tooltips(&mut ui);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].0, tooltip, "updated in place");
    assert_eq!(
        shown[0]
            .3
            .as_ref()
            .map(|args| serde_json::to_value(args).unwrap()["n"].clone()),
        Some(2.into())
    );

    ui.move_pointer(plain_at).update(2);
    assert!(
        tooltips(&mut ui).is_empty(),
        "no data-tooltip: nothing shown"
    );
    assert!(ui.world_mut().get_entity(tooltip).is_err());
}

/// A signal exposes its element's `data-*` attributes (`signal.data(…)`),
/// so each feature can keep its own attribute instead of sharing one
/// `data-with` blob; they follow in-place updates.
#[test]
fn signals_carry_the_element_dataset() {
    let page =
        r#"<div id="btn" data-on-click="pick" data-kind="{{ kind }}" data-flag><p>Pick</p></div>"#;
    let mut ui = page_ui(
        "signals-dataset",
        page,
        "",
        TemplateContext::new().with("kind", "sword"),
    );
    ui.settle();
    ui.update(1);
    let root = ui.root();
    let (btn, at) = center(ui.world_mut(), root, "btn");
    ui.click_at(at);
    let clicks: Vec<ElementSignal> = ui
        .take_signals()
        .into_iter()
        .filter(|s| s.trigger == SignalTrigger::Click)
        .collect();
    assert_eq!(clicks.len(), 1);
    assert_eq!(clicks[0].data("kind"), Some("sword"));
    assert_eq!(clicks[0].data("flag"), Some(""), "value-less attribute");
    assert_eq!(
        clicks[0].data("on-click"),
        Some("pick"),
        "hooks are data-* too"
    );
    assert_eq!(clicks[0].data("missing"), None);

    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("kind", "shield");
    ui.settle();
    ui.update(1);
    assert_eq!(
        center(ui.world_mut(), root, "btn").0,
        btn,
        "updated in place"
    );
    ui.release_pointer();
    ui.take_signals();
    ui.click_at(at);
    let kinds: Vec<Option<String>> = ui
        .take_signals()
        .into_iter()
        .filter(|s| s.trigger == SignalTrigger::Click)
        .map(|s| s.data("kind").map(str::to_owned))
        .collect();
    assert_eq!(kinds, [Some("shield".to_owned())]);
}

/// As in browsers: `click` is the primary button only; middle and right
/// clicks fire `auxclick`, with the button in the source. `press`/`release`
/// fire for every button.
#[test]
fn other_buttons_auxclick_instead_of_click() {
    let page = r#"<div id="btn" data-on-click="click" data-on-auxclick="aux"
                         data-on-press="down"><p>Button</p></div>"#;
    let mut ui = page_ui("signals-aux", page, "", TemplateContext::new());
    ui.settle();
    ui.update(1);
    let root = ui.root();
    let (_, at) = center(ui.world_mut(), root, "btn");
    ui.move_pointer(at);
    ui.take_signals();
    for (mouse, button) in [
        (MouseButton::Right, PointerButton::Secondary),
        (MouseButton::Middle, PointerButton::Middle),
    ] {
        ui.mouse_button(mouse, ButtonState::Pressed);
        ui.mouse_button(mouse, ButtonState::Released);
        let signals: Vec<(String, SignalTrigger, Option<PointerButton>)> = ui
            .take_signals()
            .into_iter()
            .map(|signal| {
                let button = match signal.source {
                    SignalSource::Pointer { button, .. } => Some(button),
                    _ => None,
                };
                (signal.name.to_string(), signal.trigger, button)
            })
            .collect();
        assert_eq!(
            signals,
            [
                ("down".to_owned(), SignalTrigger::Press, Some(button)),
                ("aux".to_owned(), SignalTrigger::AuxClick, Some(button)),
            ],
            "{mouse:?}: press and auxclick, no click"
        );
    }
}

/// Nested hooks: the deepest bound element under the pointer wins — but only
/// for its own trigger, and only where it actually covers the pointer.
#[test]
fn the_deepest_bound_element_wins_the_click() {
    let page = r#"<div id="outer" data-on-click="outer">
        <div id="inner" data-on-click="inner"><p>In</p></div>
    </div>"#;
    let css = "#outer { padding: 12px }";
    let mut ui = page_ui("signals-nested", page, css, TemplateContext::new());
    ui.settle();
    ui.update(1);
    let root = ui.root();

    // Over the inner element (its text): only the inner hook fires; the
    // outer's observer sees the same event bubble past but is suppressed.
    let (inner, at) = {
        let world = ui.world_mut();
        center(world, root, "inner")
    };
    ui.click_at(at);
    let signals = ui.take_signals();
    assert_eq!(signals.len(), 1, "only the deepest hook fires");
    assert_eq!(
        (signals[0].name.as_ref(), signals[0].target),
        ("inner", inner)
    );

    // Over the outer's own padding: the outer hook fires.
    let (_, at) = {
        let world = ui.world_mut();
        let outer = element_by_id(world, root, "outer");
        let rect = node_rect(world, outer).unwrap();
        (outer, Vec2::new(rect.center().x, rect.max.y - 6.0))
    };
    ui.click_at(at);
    let signals = ui.take_signals();
    assert_eq!(signals.len(), 1, "uncovered area: the outer hook fires");
    assert_eq!(signals[0].name, "outer");
}

/// An app-nested `HtmlUi` root beside pipeline children: a kept subtree's
/// `:hover` keeps working across a build that shifts the document indices
/// (an insertion). The stale refresh used to give up when the entity count
/// under the parent disagreed with the spec's (the nested root), leaving
/// the subtree's `DomNode` handles in the old document — `:hover` died
/// there while reuse kept succeeding.
#[test]
fn hover_survives_a_rebuild_around_a_nested_ui_root() {
    let page = r#"<div id="wrap">{% for i in items %}<p>{{ i }}</p>{% endfor %}
        <div id="target" data-on-enter="enter-target" data-on-leave="leave-target"><p>In</p></div>
    </div>"#;
    let spawn = |name: &str| page_ui(name, page, "", TemplateContext::new().with("items", &[0]));
    let mut reused = spawn("nested-hover-reused");
    let mut fresh = spawn("nested-hover-fresh");
    reused.settle();
    fresh.settle();
    // Nest a slot UI into the wrap (the demo's slot pattern): the entity
    // count under the wrap then disagrees with the spec's children.
    reused.world_mut().add_observer(
        |built: On<HtmlUiBuilt>,
         mut commands: Commands,
         elements: HtmlElements,
         templates: Res<NestedSlot>| {
            let Some(wrap) = elements.by_id(built.entity, "wrap") else {
                return;
            };
            commands.entity(wrap).with_children(|children| {
                children.spawn((
                    HtmlUi::new(templates.0.clone()),
                    TemplateContext::new(),
                    Node::default(),
                ));
            });
        },
    );
    #[derive(Resource)]
    struct NestedSlot(Handle<HtmlTemplate>);
    let template = reused.load::<HtmlTemplate>("slot.html");
    reused.world_mut().insert_resource(NestedSlot(template));
    // The insertion shifts every following index; the pairing walk runs
    // with the nested root present.
    for ui in [&mut reused, &mut fresh] {
        let root = ui.root();
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert("items", &[0, 1]);
    }
    reused.settle();
    fresh.settle();
    // Hover the target in the reused UI: its handles were refreshed.
    let (target, at) = {
        let root = reused.root();
        let world = reused.world_mut();
        center(world, root, "target")
    };
    reused.move_pointer(at);
    assert!(pseudo(reused.world_mut(), target).hovered, "hovered");
    // And the same hover works in the fresh UI (sanity).
    let (ftarget, fat) = {
        let root = fresh.root();
        let world = fresh.world_mut();
        center(world, root, "target")
    };
    fresh.move_pointer(fat);
    assert!(pseudo(fresh.world_mut(), ftarget).hovered, "fresh hovered");
}

/// ADR 0029: a scoped restyle (`:hover` flip) restyles the flipped
/// element's subtree and nothing else. The hovered UI's dump must equal a
/// fresh build of the same state, and an unhover must return the UI to
/// exactly its starting dump.
#[test]
fn hover_restyles_scoped_like_a_fresh_build() {
    let page = r#"<div id="wrap">
        <div class="row" id="row-1"><p class="label" id="label-1">one</p></div>
        <div class="row hover-target" id="row-2" data-on-enter="e2" data-on-leave="l2"><p class="label" id="label-2">two</p></div>
        <div class="row" id="row-3"><p class="label" id="label-3">three</p></div>
        <div class="bump-holder" data-bump="{{ bump }}"></div>
    </div>"#;
    // The ancestor rule (`.row:hover .label`) is the subtree case: the
    // label restyles because an *ancestor* flipped.
    let css = r#"
        .hover-target:hover { color: #ff0000; background-color: #200000; }
        .row:hover .label { color: #00ff00; }
    "#;
    let spawn = |name: &str| page_ui(name, page, css, TemplateContext::new().with("bump", &0));
    let mut hovered = spawn("scoped-hover-reused");
    let mut fresh = spawn("scoped-hover-fresh");
    hovered.settle();
    fresh.settle();
    let before = hovered.dump();

    // Hover row-2 through the real pointer path: a scoped restyle.
    let (target, at) = {
        let root = hovered.root();
        let world = hovered.world_mut();
        center(world, root, "row-2")
    };
    hovered.move_pointer(at);
    assert!(pseudo(hovered.world_mut(), target).hovered, "hovered");
    hovered.settle();
    let after_hover = hovered.dump();
    assert_ne!(after_hover, before, "the hover restyled the row");

    // The reference: the same hover applied to a *full* rebuild — hover
    // it too, then a content change (the `bump` attribute) makes the next
    // build a content build with a pending style change, which reuses
    // nothing (the pre-ADR-0029 path). The dumps differ nowhere (the
    // attribute is invisible to the dump).
    let froot = fresh.root();
    let (_, fat) = {
        let world = fresh.world_mut();
        center(world, froot, "row-2")
    };
    fresh.move_pointer(fat);
    fresh
        .world_mut()
        .get_mut::<TemplateContext>(froot)
        .unwrap()
        .insert("bump", &1);
    fresh.settle();
    assert_eq!(
        after_hover,
        fresh.dump(),
        "the scoped restyle matches a fresh build of the hovered state"
    );

    // Unhover: the scoped restyle returns the UI to its starting dump.
    // The point is off the viewport (inside it, any point hovers a row —
    // the page fills the window).
    hovered.move_pointer(Vec2::new(320.0, 600.0));
    assert!(!pseudo(hovered.world_mut(), target).hovered, "unhovered");
    hovered.settle();
    assert_eq!(hovered.dump(), before, "the unhover restored the start");
}

/// ADR 0029's soundness: a rule keyed on an *ancestor's* pseudo state
/// (`.row:hover .label`) restyles elements that did not flip themselves.
/// The hover here lands on the row's padding — neither label is hovered —
/// so both labels must re-cascade because the row flipped, not because
/// their own state moved.
#[test]
fn ancestor_pseudo_rule_restyles_uncached_descendants() {
    let page = r#"<div id="wrap">
        <div class="row pad" id="row">
            <p class="label" id="label-1">one</p>
            <p class="label" id="label-2">two</p>
        </div>
    </div>"#;
    let css = r#"
        .pad { padding: 24px }
        .pad:hover .label { color: #00ff00 }
    "#;
    let mut ui = page_ui("scoped-hover-ancestor", page, css, TemplateContext::new());
    ui.settle();
    let before = ui.dump();

    // Hover the row's top padding strip: above both labels.
    let root = ui.root();
    let row = element_by_id(ui.world_mut(), root, "row");
    let rect = node_rect(ui.world_mut(), row).expect("laid out");
    let at = Vec2::new((rect.min.x + rect.max.x) / 2.0, rect.min.y + 4.0);
    ui.move_pointer(at);
    let row_entity = element_by_id(ui.world_mut(), root, "row");
    assert!(pseudo(ui.world_mut(), row_entity).hovered, "row hovered");
    let label_1 = element_by_id(ui.world_mut(), root, "label-1");
    let label_2 = element_by_id(ui.world_mut(), root, "label-2");
    assert!(
        !pseudo(ui.world_mut(), label_1).hovered && !pseudo(ui.world_mut(), label_2).hovered,
        "no label hovered"
    );
    ui.settle();

    let after = ui.dump();
    assert_ne!(after, before, "the hover restyled the labels");
    // Both labels carry the rule's green; nothing else moved (the row's
    // own padding/background is untouched by the rule).
    assert!(after.contains("\"one\" default 16px #00ff00"), "{after}");
    assert!(after.contains("\"two\" default 16px #00ff00"), "{after}");

    // And the unhover returns the start.
    ui.move_pointer(Vec2::new(320.0, 600.0));
    ui.settle();
    assert_eq!(ui.dump(), before, "the unhover restored the start");
}

/// `enter`/`leave` count for a whole element subtree (like CSS `:hover`),
/// and each bound element on the way enters and leaves.
#[test]
fn enter_and_leave_track_the_hovered_subtree() {
    let page = r#"<div id="parent" data-on-enter="enter-parent" data-on-leave="leave-parent">
        <div id="child" data-on-enter="enter-child" data-on-leave="leave-child"
             data-with='{ "k": "v" }'><p>In</p></div>
    </div>"#;
    let mut ui = page_ui("signals-hover", page, "", TemplateContext::new());
    ui.settle();
    ui.update(1);
    let root = ui.root();
    let (child, at) = {
        let world = ui.world_mut();
        center(world, root, "child")
    };

    // Moving onto the child enters it and its bound ancestor.
    ui.move_pointer(at);
    let signals = ui.take_signals();
    let mut names: Vec<_> = signals.iter().map(|s| s.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(names, ["enter-child", "enter-parent"]);
    assert!(
        signals.iter().all(|s| s.source
            == SignalSource::Hover {
                pointer: PointerId::Mouse
            }),
        "enter names the pointer"
    );
    let entered_child = signals
        .iter()
        .find(|s| s.name == "enter-child")
        .expect("child enter");
    assert_eq!(entered_child.target, child);
    assert_eq!(entered_child.payload["k"], "v");

    // Moving within the same subtree changes nothing.
    ui.move_pointer(at + Vec2::new(2.0, 0.0));
    assert!(ui.take_signals().is_empty(), "still inside");

    // Moving away leaves every entered element, child first or not
    // (HashMap order): compare as a set.
    let (_, outside) = {
        let world = ui.world_mut();
        let rect = node_rect(world, root).expect("laid out root");
        (root, Vec2::new(rect.center().x, rect.max.y + 40.0))
    };
    ui.move_pointer(outside);
    let signals = ui.take_signals();
    let mut names: Vec<_> = signals.iter().map(|s| s.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(names, ["leave-child", "leave-parent"]);
    assert!(signals.iter().all(|s| s.source
        == SignalSource::Hover {
            pointer: PointerId::Mouse
        }));
}

/// Under a stationary cursor, a content update that keeps the hovered
/// element (new text) emits nothing. One that replaces it (another tag)
/// despawns it: its `leave` still fires, from the snapshot taken at enter —
/// and the new element under the same pointer enters afresh.
#[test]
fn a_replacement_under_the_cursor_emits_the_snapshot_leave() {
    let page = r#"<{{ tag }} id="btn" data-on-enter="enter" data-on-leave="leave"><p>Hi {{ n }}</p></{{ tag }}>"#;
    let mut ui = page_ui(
        "signals-rebuild",
        page,
        "",
        TemplateContext::new().with("n", &1).with("tag", "div"),
    );
    ui.settle();
    let root = ui.root();
    let (btn, at) = {
        let world = ui.world_mut();
        center(world, root, "btn")
    };
    ui.move_pointer(at);
    let signals = ui.take_signals();
    assert_eq!(signals.len(), 1);
    assert_eq!(
        (signals[0].trigger, signals[0].target),
        (SignalTrigger::Enter, btn)
    );

    // New text: updated in place, still hovered, no signals.
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &2);
    let signals = ui.update_collecting_signals(3);
    assert!(signals.is_empty(), "kept in place: {signals:?}");
    assert_eq!(center(ui.world_mut(), root, "btn").0, btn);

    // Another tag: the element is replaced.
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("tag", "section");
    let signals = ui.update_collecting_signals(3);

    let leaves: Vec<&ElementSignal> = signals
        .iter()
        .filter(|s| s.trigger == SignalTrigger::Leave)
        .collect();
    assert_eq!(leaves.len(), 1, "the despawned element leaves: {signals:?}");
    assert_eq!((leaves[0].name.as_ref(), leaves[0].target), ("leave", btn));
    assert_eq!(
        leaves[0].element.id.as_deref(),
        Some("btn"),
        "snapshot identity"
    );

    let enters: Vec<&ElementSignal> = signals
        .iter()
        .filter(|s| s.trigger == SignalTrigger::Enter)
        .collect();
    assert_eq!(enters.len(), 1, "the rebuilt element enters: {signals:?}");
    assert_ne!(enters[0].target, btn, "a fresh entity");
}

/// `:hover` counts for the whole subtree; `:active` while the press is down,
/// on the pressed element and its ancestors (like CSS).
#[test]
fn hover_and_press_pseudo_states_track_the_pointer() {
    let page = r#"<div id="btn" data-on-click="x"><p id="label">Hi</p></div>"#;
    let mut ui = page_ui("signals-pseudo", page, "", TemplateContext::new());
    ui.settle();
    ui.update(1);
    let root = ui.root();
    let (btn, at) = {
        let world = ui.world_mut();
        center(world, root, "btn")
    };
    let label = {
        let world = ui.world_mut();
        element_by_id(world, root, "label")
    };

    // The button holds the initial focus (first focusable in scope, see
    // `focus.rs`); the pointer hasn't touched it yet.
    let initial = pseudo(ui.world_mut(), btn);
    assert!(
        !initial.hovered && !initial.active,
        "untouched by the pointer"
    );
    ui.move_pointer(at);
    let world = ui.world_mut();
    assert!(pseudo(world, btn).hovered, "the button hovers");
    assert!(pseudo(world, label).hovered, "its block hovers with it");
    assert!(!pseudo(world, btn).active);

    // Pressing activates the pressed chain; a click hook alone stays quiet
    // until the click.
    ui.press_pointer();
    assert!(ui.take_signals().is_empty(), "click fires on release");
    let world = ui.world_mut();
    assert!(pseudo(world, btn).active);
    assert!(pseudo(world, label).active);

    // Releasing deactivates, hovering stays.
    ui.release_pointer();
    let world = ui.world_mut();
    assert!(!pseudo(world, btn).active);
    assert!(pseudo(world, btn).hovered);

    // Moving off the UI (below the root) unhovers.
    let (_, below) = {
        let world = ui.world_mut();
        let rect = node_rect(world, root).expect("laid out root");
        (root, Vec2::new(rect.center().x, rect.max.y + 40.0))
    };
    ui.move_pointer(below);
    let world = ui.world_mut();
    assert!(!pseudo(world, btn).hovered);
    assert!(!pseudo(world, label).hovered);
}

/// `pointer-events: none` (inherited by the subtree) makes the element and
/// its hooks untouchable; hover passes through to whatever is behind.
#[test]
fn pointer_events_none_silences_an_element() {
    let page = r#"<div id="wrap"><div id="btn" data-on-click="x"><p>Nope</p></div></div>"#;
    let css = "#btn { pointer-events: none }";
    let mut ui = page_ui("signals-events-none", page, css, TemplateContext::new());
    ui.settle();
    ui.update(1);
    let root = ui.root();
    let (btn, at) = {
        let world = ui.world_mut();
        center(world, root, "btn")
    };

    ui.click_at(at);
    assert!(
        ui.take_signals().is_empty(),
        "the ignored element stays silent"
    );
    assert!(!pseudo(ui.world_mut(), btn).hovered, "and is never hovered");
}

/// Hooks on elements that produce no entity (inline, walked-through) never
/// fire — the block they live in isn't bound either.
#[test]
fn hooks_on_elements_without_entities_stay_dead() {
    let page = r#"<p><b data-on-click="x">Hi</b> there</p>"#;
    let mut ui = page_ui("signals-inline", page, "", TemplateContext::new());
    ui.settle();
    ui.update(1);
    let (_, at) = {
        let world = ui.world_mut();
        // The bold "Hi" starts the paragraph's line; click its glyphs.
        let mut elements = world.query::<(Entity, &HtmlElement)>();
        let (p, rect) = elements
            .iter(world)
            .find(|(_, element)| element.tag == "p")
            .map(|(entity, _)| (entity, node_rect(world, entity).unwrap()))
            .expect("paragraph");
        (p, Vec2::new(rect.min.x + 6.0, rect.center().y))
    };
    ui.click_at(at);
    assert!(ui.take_signals().is_empty(), "no entity, no signal");
}

/// Registered handlers route by signal name: `on_html_signal` runs for
/// every trigger of the name, `on_html_click` only for clicks, and
/// unregistered names run nothing. The message still reaches readers.
#[test]
fn handlers_route_by_name_and_trigger() {
    #[derive(Resource, Default)]
    struct Routed(Vec<(&'static str, SignalTrigger)>);

    let page = r#"<div id="btn" data-on-click="buy" data-on-press="buy"><p>Buy</p></div>"#;
    let mut ui = page_ui("signals-router", page, "", TemplateContext::new());
    ui.settle();
    ui.update(1);
    let (btn, at) = {
        let root = ui.root();
        let world = ui.world_mut();
        center(world, root, "btn")
    };
    ui.world_mut().insert_resource(Routed::default());
    ui.app_mut()
        .on_html_signal(
            "buy",
            |In(signal): In<ElementSignal>, mut log: ResMut<Routed>| {
                log.0.push(("any", signal.trigger));
            },
        )
        .on_html_click(
            "buy",
            |In(_): In<ElementSignal>, mut log: ResMut<Routed>| {
                log.0.push(("click-only", SignalTrigger::Click));
            },
        )
        .on_html_signal(
            "other",
            |In(_): In<ElementSignal>, mut log: ResMut<Routed>| {
                log.0.push(("other", SignalTrigger::Click));
            },
        );

    // Handlers of one name run in registration order.
    let routed = |ui: &mut TestUi| std::mem::take(&mut ui.world_mut().resource_mut::<Routed>().0);

    // Pressing runs the any-trigger handler only.
    ui.move_pointer(at);
    ui.press_pointer();
    assert_eq!(routed(&mut ui), [("any", SignalTrigger::Press)]);

    // Releasing runs both: it's a click.
    ui.release_pointer();
    assert_eq!(
        routed(&mut ui),
        [
            ("any", SignalTrigger::Click),
            ("click-only", SignalTrigger::Click)
        ]
    );
    assert_eq!(btn, btn, "target unchanged");
}

/// `<a href>` links: the link's span becomes a picked entity whose click
/// emits the `open` signal carrying the href; the deepest-bound-element
/// rule suppresses the parent block's own click hooks; a link without an
/// href is plain text.
#[test]
fn links_emit_open_signals_with_the_href() {
    let page = r#"<div id="outer" data-on-click="outer-click">
  <p>See <a id="link" href="{{ href }}">the <b>docs</b></a> and
     <a id="plain">not a link</a>.</p>
</div>"#;
    let mut ui = page_ui(
        "signals-links",
        page,
        "a { color: #00ffff }",
        TemplateContext::new().with("href", "https://example.com/page"),
    );
    ui.settle();
    ui.update(1);

    // The link's span is an entity with the bindings; the plain anchor is
    // not (no href, as in browsers).
    let root = ui.root();
    let world = ui.world_mut();
    let mut elements = world.query::<(Entity, &HtmlElement)>();
    let link = elements
        .iter(world)
        .find(|(_, element)| element.id.as_deref() == Some("link"))
        .map(|(entity, _)| entity)
        .expect("link span entity");
    assert!(
        world.get::<ElementSignals>(link).is_some(),
        "the link span carries its bindings"
    );
    assert!(
        !elements
            .iter(world)
            .any(|(_, element)| element.id.as_deref() == Some("plain")),
        "no href: the plain anchor stays entity-free styled text"
    );

    // Click the link text (the `b` inside it resolves through the link's
    // spans): the `open` signal carries the href, and the outer block's
    // own click hook is suppressed (deepest bound element wins). The
    // link span has no `Node` of its own — its rect comes from the
    // text block's layout, via the block's rect around the link's line.
    let block = elements
        .iter(world)
        .find(|(_, element)| element.tag == "p")
        .map(|(entity, _)| entity)
        .expect("paragraph");
    let rect = node_rect(world, block).expect("laid out");
    let at = vec2(rect.min.x + 40.0, rect.center().y);
    ui.click_at(at);
    let clicks: Vec<ElementSignal> = ui
        .take_signals()
        .into_iter()
        .filter(|signal| signal.trigger == SignalTrigger::Click)
        .collect();
    assert_eq!(clicks.len(), 1, "one click: the link's, not the block's");
    assert_eq!(clicks[0].name.as_ref(), "open");
    assert_eq!(clicks[0].target, link);
    assert_eq!(clicks[0].element.tag, "a");
    assert_eq!(
        clicks[0].payload["href"].as_str(),
        Some("https://example.com/page")
    );

    // The link's href updates in place: the bindings follow.
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("href", "https://example.com/other");
    ui.settle();
    ui.update(1);
    ui.click_at(at);
    let clicks: Vec<ElementSignal> = ui
        .take_signals()
        .into_iter()
        .filter(|signal| signal.trigger == SignalTrigger::Click)
        .collect();
    assert_eq!(clicks.len(), 1, "still one click after the update");
    assert_eq!(
        clicks[0].payload["href"].as_str(),
        Some("https://example.com/other"),
        "the re-rendered href"
    );
}

/// `<form data-on-submit>`: clicking a submit button collects the named
/// inputs' values into the Submit signal on the form; unnamed inputs are
/// excluded; only the clicked form submits. Typing goes through the
/// widget's keyboard handling (the field focused via press-to-focus).
#[test]
fn form_submit_collects_named_inputs() {
    let page = r#"<div class="app">
  <form id="login" data-on-submit="login">
    <input type="text" name="user" value="ada">
    <input type="text" name="pass" id="pass">
    <input type="text" value="unnamed">
    <button type="submit" id="go">Log in</button>
  </form>
  <form id="other" data-on-submit="other">
    <input type="text" name="q" value="query">
    <button type="submit" id="other-go">Go</button>
  </form>
</div>"#;
    let mut ui = page_ui(
        "signals-form",
        page,
        "input { width: 200px; height: 24px } button { height: 24px; width: 100px }",
        TemplateContext::new(),
    );
    ui.settle();
    ui.update(1);

    // The forms and the submit buttons are entities.
    let (login, other, go) = {
        let world = ui.world_mut();
        let mut elements = world.query::<(Entity, &HtmlElement)>();
        let mut by_id = |id: &str| {
            elements
                .iter(world)
                .find(|(_, element)| element.id.as_deref() == Some(id))
                .map(|(entity, _)| entity)
                .unwrap_or_else(|| panic!("{id}"))
        };
        (by_id("login"), by_id("other"), by_id("go"))
    };

    // Click the login form's submit button: the Submit signal carries the
    // named inputs' values (the unnamed one excluded), targets the form,
    // and the other form doesn't submit.
    let at = {
        let world = ui.world_mut();
        node_rect(world, go).expect("laid out").center()
    };
    ui.click_at(at);
    let submits: Vec<ElementSignal> = ui
        .take_signals()
        .into_iter()
        .filter(|signal| signal.trigger == SignalTrigger::Submit)
        .collect();
    assert_eq!(submits.len(), 1, "one submit");
    assert_eq!(submits[0].name.as_ref(), "login");
    assert_eq!(submits[0].target, login);
    assert!(
        !submits.iter().any(|signal| signal.target == other),
        "only the clicked form submits"
    );
    // The values (and the inputs they come from) need the `text-input`
    // feature: without it, `<input>`s don't spawn and the form submits
    // with an empty payload.
    #[cfg(feature = "text-input")]
    {
        assert_eq!(submits[0].payload["user"].as_str(), Some("ada"));
        assert_eq!(submits[0].payload["pass"].as_str(), Some(""));
        assert!(
            submits[0].payload.get("unnamed").is_none(),
            "unnamed inputs excluded"
        );
    }

    // Typing into the pass field and re-submitting needs the
    // `text-input` feature (the field and the widget's keyboard handling
    // only exist with it).
    #[cfg(feature = "text-input")]
    {
        let pass = {
            let world = ui.world_mut();
            let mut elements = world.query::<(Entity, &HtmlElement)>();
            elements
                .iter(world)
                .find(|(_, element)| element.id.as_deref() == Some("pass"))
                .map(|(entity, _)| entity)
                .expect("pass")
        };
        let pass_rect = {
            let world = ui.world_mut();
            node_rect(world, pass).expect("laid out")
        };
        ui.click_at(pass_rect.center());
        ui.take_signals();
        let window = window_of(&mut ui);
        for (code, character) in [(KeyCode::KeyH, "h"), (KeyCode::KeyI, "i")] {
            ui.world_mut()
                .resource_mut::<Messages<KeyboardInput>>()
                .write(KeyboardInput {
                    key_code: code,
                    logical_key: Key::Character(character.into()),
                    state: ButtonState::Pressed,
                    text: Some(character.into()),
                    repeat: false,
                    window,
                });
            ui.update(2);
        }
        ui.click_at(at);
        let submits: Vec<ElementSignal> = ui
            .take_signals()
            .into_iter()
            .filter(|signal| signal.trigger == SignalTrigger::Submit)
            .collect();
        assert_eq!(submits.len(), 1, "one submit after typing");
        assert_eq!(submits[0].payload["pass"].as_str(), Some("hi"), "typed");
        assert_eq!(submits[0].payload["user"].as_str(), Some("ada"));
    }
}

/// The harness's primary window entity (the keyboard/touch event's
/// target).
fn window_of(ui: &mut TestUi) -> Entity {
    use bevy::window::PrimaryWindow;
    ui.world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(ui.world_mut())
        .expect("primary window")
}

/// A translated link (`<a>` inside a `data-l10n-id` message) hovers and
/// recolors like a template link: both are picked spans whose `PseudoState`
/// hover bit drives the `a:hover` restyle. Regression: translated links
/// never showed their hover style.
#[test]
fn translated_links_hover_like_template_links() {
    let page = r#"<div>
  <p data-l10n-id="intro">Plain fallback.</p>
  <p><a href="one://x">One</a></p>
</div>"#;
    let ftl = "intro = Notes live in the <a href=\"two://y\">journal</a>.\n";
    let mut ui = page_ui(
        "signals-translated-links",
        page,
        "a { color: #7fdbff } a:hover { color: #f0b429; background-color: #333333 }",
        TemplateContext::new(),
    );
    ui.write(
        "main.ftl.ron",
        "(locale: \"en-US\", resources: [\"messages.ftl\"],)",
    );
    ui.write("messages.ftl", ftl);
    ui = ui.locale("main.ftl.ron");
    ui.settle();
    ui.update(1);

    // Both link spans: template and translated (document order puts the
    // translated intro first).
    let (translated_link, template_link) = {
        let world = ui.world_mut();
        let mut elements = world.query::<(Entity, &HtmlElement, Option<&ElementSignals>)>();
        let links: Vec<Entity> = elements
            .iter(world)
            .filter(|(_, element, signals)| {
                element.tag == "a"
                    && signals.is_some_and(|signals| signals.0.iter().any(|b| b.name == "open"))
            })
            .map(|(entity, _, _)| entity)
            .collect();
        assert_eq!(links.len(), 2, "two link spans");
        (links[0], links[1])
    };
    // Hover each link's run: probe across its block's first line (the
    // translated link ends the intro's line, the template link is the
    // whole one-line paragraph). The hovered span must light up and
    // recolor.
    for link in [translated_link, template_link] {
        let block = {
            let world = ui.world_mut();
            let mut current = link;
            loop {
                current = world
                    .get::<ChildOf>(current)
                    .map(|parent| parent.0)
                    .expect("no block above the link");
                if node_rect(world, current).is_some() {
                    break current;
                }
            }
        };
        let rect = {
            let world = ui.world_mut();
            node_rect(world, block).expect("laid out")
        };
        let mut hovered = false;
        for step in 1..=10 {
            let at = Vec2::new(
                rect.min.x + rect.width() * (step as f32 / 11.0),
                rect.min.y + 10.0,
            );
            ui.move_pointer(at);
            ui.update(2);
            let state = {
                let world = ui.world_mut();
                world.get::<PseudoState>(link).copied()
            };
            if state.is_some_and(|state| state.hovered) {
                hovered = true;
                break;
            }
        }
        assert!(hovered, "the link span hovers (block {block:?})");
        // The hover restyle recolors the span (and hovering a plain-text
        // run never does).
        let color: bevy::color::Srgba = {
            let world = ui.world_mut();
            world
                .get::<TextColor>(link)
                .map(|c| c.0)
                .expect("styled")
                .into()
        };
        assert_eq!(color, HOVER_COLOR, "the hovered link recolors");
        // The run-level properties follow the state too (`a:hover` may
        // style a link's background).
        let background: bevy::color::Srgba = {
            let world = ui.world_mut();
            world
                .get::<TextBackgroundColor>(link)
                .map(|c| c.0.into())
                .expect("backgrounded")
        };
        assert_eq!(background, HOVER_BACKGROUND, "the hover background applies");
        ui.move_pointer(Vec2::splat(-1000.0));
        ui.update(2);
    }
}

/// The hover rule's background (the test stylesheet's `a:hover`).
const HOVER_BACKGROUND: bevy::color::Srgba = Srgba::new(0.2, 0.2, 0.2, 1.0);

/// The hover rule's color (the test stylesheet's `a:hover`).
const HOVER_COLOR: bevy::color::Srgba = Srgba::new(
    0xF0 as f32 / 255.0,
    0xB4 as f32 / 255.0,
    0x29 as f32 / 255.0,
    1.0,
);

/// Focus the app set on a link span survives repair: the span has bindings
/// (an activation target) but no `Focusable` and no `Node`, which
/// `repair_focus` used to treat as stale and clear every frame — keyboard
/// cycling in the `links` example flashed for one frame and died.
#[test]
fn repair_keeps_focus_on_link_spans() {
    let page = r#"<div>
  <p><a href="one://x">One</a></p>
</div>"#;
    let mut ui = page_ui(
        "signals-link-repair",
        page,
        "a { color: #7fdbff } a:focus-visible { color: #f0b429 }",
        TemplateContext::new(),
    );
    ui.settle();
    ui.update(1);

    let link = {
        let world = ui.world_mut();
        let mut elements = world.query::<(Entity, &HtmlElement)>();
        elements
            .iter(world)
            .find(|(_, element)| element.tag == "a")
            .map(|(entity, _)| entity)
            .expect("a link span")
    };
    // The app's own cycle: focus the span and show the ring.
    {
        let world = ui.world_mut();
        world
            .resource_mut::<InputFocus>()
            .set(link, FocusCause::Navigated);
        world.resource_mut::<InputFocusVisible>().0 = true;
    }
    ui.update(2);
    let focused = |ui: &mut TestUi| {
        let world = ui.world_mut();
        (
            world.resource::<InputFocus>().get(),
            world.get::<PseudoState>(link).copied(),
        )
    };
    let (focus, state) = focused(&mut ui);
    assert_eq!(focus, Some(link), "focus survives repair");
    assert!(state.is_some_and(|state| state.focused && state.focus_visible));
    // And the next frame: repair runs again, the focus stays.
    ui.update(2);
    let (focus, _) = focused(&mut ui);
    assert_eq!(focus, Some(link), "focus survives another repair");
}

/// Clicking a non-submit button inside a form must NOT submit it, and a
/// bound wrapper around the submit button submits it exactly once (the
/// deepest-bound-element rule counts the submit button as bound for
/// `click`).
#[test]
fn type_button_clicks_do_not_submit() {
    let page = r#"<form id="signup" data-on-submit="signup">
  <button type="button" id="clear" data-on-click="clear-form">Clear</button>
  <button type="submit" id="go">Sign up</button>
</form>"#;
    let mut ui = page_ui(
        "no-submit",
        page,
        "button { height: 24px; width: 100px }",
        TemplateContext::new(),
    );
    ui.settle();
    ui.update(1);
    let clear = {
        let world = ui.world_mut();
        let mut elements = world.query::<(Entity, &HtmlElement)>();
        elements
            .iter(world)
            .find(|(_, element)| element.id.as_deref() == Some("clear"))
            .map(|(entity, _)| entity)
            .expect("clear")
    };
    let at = node_rect(ui.world_mut(), clear).expect("laid out").center();
    ui.click_at(at);
    let signals: Vec<ElementSignal> = ui.take_signals();
    assert!(
        signals
            .iter()
            .all(|signal| signal.trigger != SignalTrigger::Submit),
        "the type=button click submitted: {signals:?}"
    );
    assert!(
        signals
            .iter()
            .any(|signal| signal.name.as_ref() == "clear-form"),
        "the button's own click fired: {signals:?}"
    );
}

/// A `<button>` with no `type` is a submit button *in HTML*, but outside
/// a form with a submit hook it has no submit semantics — and it must
/// not count as bound: a click on it reaches the enclosing wrapper's
/// binding (the submit marker made `covered_by_deeper` swallow it).
#[test]
fn plain_button_outside_a_form_does_not_swallow_the_wrapper_click() {
    let page = r#"<div id="wrap" data-on-click="wrap"><button id="plain">Plain</button></div>"#;
    let mut ui = page_ui(
        "plain-button",
        page,
        "button { height: 24px; width: 100px }",
        TemplateContext::new(),
    );
    ui.settle();
    ui.update(1);
    let plain = {
        let world = ui.world_mut();
        let mut elements = world.query::<(Entity, &HtmlElement)>();
        elements
            .iter(world)
            .find(|(_, element)| element.id.as_deref() == Some("plain"))
            .map(|(entity, _)| entity)
            .expect("plain")
    };
    let at = node_rect(ui.world_mut(), plain).expect("laid out").center();
    ui.click_at(at);
    let signals: Vec<ElementSignal> = ui.take_signals();
    assert!(
        signals.iter().any(|signal| signal.name.as_ref() == "wrap"),
        "the wrapper's click never fired: {signals:?}"
    );
    assert!(
        signals
            .iter()
            .all(|signal| signal.trigger != SignalTrigger::Submit),
        "a button outside a form submitted: {signals:?}"
    );
}

/// Clicks the element with `id` and returns the drained signals.
fn click_id(ui: &mut TestUi, id: &str) -> Vec<ElementSignal> {
    let entity = {
        let world = ui.world_mut();
        let mut elements = world.query::<(Entity, &HtmlElement)>();
        elements
            .iter(world)
            .find(|(_, element)| element.id.as_deref() == Some(id))
            .map(|(entity, _)| entity)
            .unwrap_or_else(|| panic!("{id}"))
    };
    let at = node_rect(ui.world_mut(), entity)
        .expect("laid out")
        .center();
    ui.click_at(at);
    ui.take_signals()
}

/// An empty (or blank) `data-on-submit` binds nothing, so its buttons are
/// not submit buttons: a click reaches the enclosing wrapper (review_0003
/// G12: the button was marked, counted as bound, and swallowed the click
/// while emitting nothing).
#[test]
fn an_empty_submit_hook_does_not_swallow_clicks() {
    for hook in ["", "  "] {
        let page = format!(
            r#"<div id="wrap" data-on-click="wrap"><form data-on-submit="{hook}"><button id="go">Go</button></form></div>"#
        );
        let mut ui = page_ui(
            "empty-submit",
            &page,
            "button { height: 24px; width: 100px }",
            TemplateContext::new(),
        );
        ui.settle();
        ui.update(1);
        let signals = click_id(&mut ui, "go");
        assert!(
            signals.iter().any(|signal| signal.name.as_ref() == "wrap"),
            "{hook:?}: the wrapper's click was swallowed: {signals:?}"
        );
    }
}

/// HTML's `<button type>` defaults to `submit` for a missing *and* an
/// invalid value: only `button` and `reset` don't submit (review_0003
/// G16: a misspelt `type` did nothing).
#[test]
fn invalid_button_types_submit() {
    for (kind, submits) in [
        ("sbumit", true),
        ("", true),
        ("SUBMIT", true),
        ("reset", false),
        ("Button", false),
    ] {
        let page = format!(
            r#"<form data-on-submit="send"><button type="{kind}" id="go">Go</button></form>"#
        );
        let mut ui = page_ui(
            "button-types",
            &page,
            "button { height: 24px; width: 100px }",
            TemplateContext::new(),
        );
        ui.settle();
        ui.update(1);
        let signals = click_id(&mut ui, "go");
        let submitted = signals
            .iter()
            .any(|signal| signal.trigger == SignalTrigger::Submit);
        assert_eq!(submitted, submits, "type={kind:?}: {signals:?}");
    }
}

/// Activation (Enter / gamepad A) on a submit button submits the form,
/// with the activation as the signal source.
#[test]
fn activation_on_submit_button_submits() {
    let page = r#"<form id="signup" data-on-submit="signup">
  <button type="submit" id="go">Sign up</button>
</form>"#;
    let mut ui = page_ui(
        "activate-submit",
        page,
        "button { height: 24px; width: 100px }",
        TemplateContext::new(),
    );
    ui.settle();
    ui.update(1);
    let go = {
        let world = ui.world_mut();
        let mut elements = world.query::<(Entity, &HtmlElement)>();
        elements
            .iter(world)
            .find(|(_, element)| element.id.as_deref() == Some("go"))
            .map(|(entity, _)| entity)
            .expect("go")
    };
    ui.world_mut().trigger(ActivateElement {
        entity: go,
        input: ActivationInput::Synthetic,
    });
    // Per frame: a direct trigger writes during `trigger()`, and the
    // message buffers rotate on each update.
    let submits = ui
        .update_collecting_signals(2)
        .into_iter()
        .filter(|signal| signal.trigger == SignalTrigger::Submit)
        .collect::<Vec<_>>();
    assert_eq!(submits.len(), 1, "one submit: {submits:?}");
    assert_eq!(submits[0].name.as_ref(), "signup");
    assert!(matches!(
        submits[0].source,
        SignalSource::Activation(ActivationInput::Synthetic)
    ));

    // A pointer click on the submit button also submits, reporting the
    // pointer as the source.
    let (_, at) = {
        let root = ui.root();
        let world = ui.world_mut();
        center(world, root, "go")
    };
    ui.click_at(at);
    let submits = ui
        .update_collecting_signals(2)
        .into_iter()
        .filter(|signal| signal.trigger == SignalTrigger::Submit)
        .collect::<Vec<_>>();
    assert_eq!(submits.len(), 1, "one submit: {submits:?}");
    assert!(
        matches!(
            submits[0].source,
            SignalSource::Pointer {
                button: PointerButton::Primary,
                ..
            }
        ),
        "the pointer source is real: {:?}",
        submits[0].source
    );
}

/// A touch pointer's entered elements emit their `data-on-leave` when the
/// touch ends (Bevy despawns the pointer and drops it from the hover map).
#[test]
fn touch_end_emits_leave() {
    let page = r#"<div id="any" data-on-enter="in" data-on-leave="out">zone</div>"#;
    let mut ui = page_ui(
        "touch-leave",
        page,
        "div { width: 100px; height: 50px }",
        TemplateContext::new(),
    );
    ui.settle();
    ui.update(1);
    let at = {
        let world = ui.world_mut();
        let zone = {
            let mut elements = world.query::<(Entity, &HtmlElement)>();
            elements
                .iter(world)
                .find(|(_, element)| element.id.as_deref() == Some("any"))
                .map(|(entity, _)| entity)
                .expect("zone")
        };
        node_rect(world, zone).expect("laid out").center()
    };
    // Touch press: enter fires with the touch pointer.
    let window = window_of(&mut ui);
    ui.world_mut()
        .resource_mut::<Messages<WindowEvent>>()
        .write(WindowEvent::TouchInput(TouchInput {
            phase: TouchPhase::Started,
            position: at,
            window,
            id: 3,
            force: None,
        }));
    ui.update(2);
    let entered: Vec<ElementSignal> = ui
        .take_signals()
        .into_iter()
        .filter(|signal| signal.trigger == SignalTrigger::Enter)
        .collect();
    assert_eq!(entered.len(), 1, "one enter: {entered:?}");
    assert!(matches!(
        entered[0].source,
        SignalSource::Hover {
            pointer: PointerId::Touch(3)
        }
    ));

    // Touch end: the pointer despawns; leave fires from the snapshot.
    ui.world_mut()
        .resource_mut::<Messages<WindowEvent>>()
        .write(WindowEvent::TouchInput(TouchInput {
            phase: TouchPhase::Ended,
            position: at,
            window,
            id: 3,
            force: None,
        }));
    // The pointer-input plugin reads the WindowEvent; bevy_picking's
    // deactivation system reads `TouchInput` messages directly (winit
    // writes those; the harness mirrors it).
    ui.world_mut()
        .resource_mut::<Messages<TouchInput>>()
        .write(TouchInput {
            phase: TouchPhase::Ended,
            position: at,
            window,
            id: 3,
            force: None,
        });

    // The despawn (commands) and the hover-map reset each take a frame.
    let leaves = ui
        .update_collecting_signals(4)
        .into_iter()
        .filter(|signal| signal.trigger == SignalTrigger::Leave)
        .collect::<Vec<_>>();
    assert_eq!(leaves.len(), 1, "one leave: {leaves:?}");
    assert_eq!(leaves[0].target, entered[0].target);

    // A vanished pointer's outer entry is dropped too: a second touch
    // doesn't stack on it (the outer map used to grow by one empty entry
    // per touch).
    ui.world_mut()
        .resource_mut::<Messages<WindowEvent>>()
        .write(WindowEvent::TouchInput(TouchInput {
            phase: TouchPhase::Started,
            position: at,
            window,
            id: 4,
            force: None,
        }));
    ui.update(2);
    ui.take_signals();
    ui.world_mut()
        .resource_mut::<Messages<WindowEvent>>()
        .write(WindowEvent::TouchInput(TouchInput {
            phase: TouchPhase::Ended,
            position: at,
            window,
            id: 4,
            force: None,
        }));
    ui.world_mut()
        .resource_mut::<Messages<TouchInput>>()
        .write(TouchInput {
            phase: TouchPhase::Ended,
            position: at,
            window,
            id: 4,
            force: None,
        });
    ui.update_collecting_signals(4);
}

/// bug_0072: a signal handler that despawns the whole UI in a frame where
/// focus also changed must not panic. `update_focus_states` walks the
/// frame's elements and queued a plain `insert<PseudoState>` on the
/// focused element; the click handler's despawn (the exclusive dispatcher
/// applies its handlers' commands mid-frame) landed before that insert
/// applied, and the insert hit a despawned entity.
#[test]
fn same_frame_despawn_with_a_focus_change_does_not_panic() {
    let page = r#"<div id="close" tabindex="0" data-on-click="close"><p>Close</p></div>
<button id="cycle-a" tabindex="0">A</button>
<button id="cycle-b" tabindex="0">B</button>"#;
    let mut ui = page_ui(
        "focus-despawn",
        page,
        "div, button { width: 100px; height: 40px }",
        TemplateContext::new(),
    );
    ui.settle();
    ui.update(1);
    let root = ui.root();
    let (_, at) = center(ui.world_mut(), root, "close");
    // The app drives focus from its own input handling (a keyboard cycle,
    // say): every frame while the UI lives, focus moves, so the click's
    // release frame is one of the frames where `update_focus_states` has
    // state changes queued for elements that are about to be despawned.
    let (a_id, b_id) = {
        let world = ui.world_mut();
        (
            element_by_id(world, root, "cycle-a"),
            element_by_id(world, root, "cycle-b"),
        )
    };
    ui.app_mut().add_systems(
        Update,
        move |mut focus: ResMut<InputFocus>, mut flip: Local<bool>| {
            if *flip {
                focus.set(a_id, FocusCause::Navigated);
            } else {
                focus.set(b_id, FocusCause::Navigated);
            }
            *flip = !*flip;
        },
    );
    // The click closes the UI (the whole root, focused element included).
    ui.app_mut().on_html_click(
        "close",
        move |_: In<ElementSignal>, mut commands: Commands| {
            commands.entity(root).despawn();
        },
    );
    ui.click_at(at);
    assert!(
        ui.world_mut().get_entity(root).is_err(),
        "the handler despawned the UI"
    );
    // And the next frame settles without a panic either (repair finds
    // nothing left to focus).
    ui.update(2);
}

/// bug_0074: a tooltip on a link (`<a href data-tooltip>`) must survive
/// hovering. The hover hit is the link's span, which carries the tooltip
/// data but no `ComputedNode`; anchoring to it treated the tooltip as gone
/// every frame — spawned and despawned 30 frames in a row, never visible.
/// The tooltip anchors to the nearest ancestor that has a node instead.
#[test]
fn a_tooltip_on_a_link_survives_hover() {
    let page = r#"<div id="page"><p>Read <a href="one://x" data-tooltip="link-tip">the report</a>.</p></div>"#;
    let mut ui = TestUi::with_pointer(
        "tooltip-link",
        &[("page.html", page), ("tooltip.html", "<p>{{ key }}</p>")],
        VIEWPORT,
    )
    .spawn(
        "page.html",
        TemplateContext::new(),
        Node {
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    let template = ui.load::<HtmlTemplate>("tooltip.html");
    ui.world_mut().insert_resource(HtmlTooltips::new(template));
    ui.settle();
    ui.update(1);
    let root = ui.root();
    let tooltips = |ui: &mut TestUi| {
        let world = ui.world_mut();
        let mut query = world.query::<(Entity, &HtmlTooltip, &HtmlAnchor)>();
        query
            .iter(world)
            .map(|(entity, tooltip, anchor)| (entity, tooltip.element, anchor.element))
            .collect::<Vec<_>>()
    };
    let (link, block, at) = {
        let world = ui.world_mut();
        let link = {
            let mut elements = world.query::<(Entity, &HtmlElement)>();
            elements
                .iter(world)
                .filter(|(entity, element)| {
                    element.tag == "a"
                        && std::iter::successors(Some(*entity), |&current| {
                            world.get::<ChildOf>(current).map(ChildOf::parent)
                        })
                        .any(|ancestor| ancestor == root)
                })
                .map(|(entity, _)| entity)
                .next()
                .expect("one link span")
        };
        // Hovering needs a position over the link's run: probe across its
        // block (the nearest ancestor with a node) like a real user.
        let block = {
            let mut current = link;
            loop {
                current = world
                    .get::<ChildOf>(current)
                    .map(|parent| parent.0)
                    .expect("no block above the link");
                if node_rect(world, current).is_some() {
                    break current;
                }
            }
        };
        (link, block, node_rect(world, block).expect("laid out"))
    };

    // Hover the link and hold: the tooltip shows and stays shown.
    let tooltip = 'found: {
        for step in 1..=10 {
            ui.move_pointer(Vec2::new(
                at.min.x + at.width() * (step as f32 / 11.0),
                at.min.y + 10.0,
            ))
            .update(2);
            if let Some(&(_, element, _)) = tooltips(&mut ui).first() {
                assert_eq!(element, link, "the tooltip is the link's");
                break 'found tooltips(&mut ui).remove(0).0;
            }
        }
        panic!("the link's tooltip never showed (hovered {link:?} via {block:?})");
    };
    // Several more hovered frames: one stable tooltip, anchored beside the
    // link's block (the span itself has no node to anchor to).
    ui.update(6);
    let shown = tooltips(&mut ui);
    assert_eq!(
        shown,
        vec![(tooltip, link, block)],
        "the tooltip survives hover, anchored to the link's block"
    );
    assert!(
        ui.world_mut().get_entity(tooltip).is_ok(),
        "the tooltip is alive"
    );

    // And leaving despawns it (the pre-fix churn never got this far).
    ui.move_pointer(Vec2::splat(-1000.0)).update(2);
    assert!(
        tooltips(&mut ui).is_empty(),
        "leaving the link despawns its tooltip"
    );
}

/// bug_0075: the first-focusable fallback picks by document order, not
/// `Entity` order. Entity ids recycle: a page built over a previous UI's
/// ids can hold its last element under the lowest `Entity`, and the old
/// `.min()` focused that one.
#[test]
fn first_focusable_fallback_picks_document_order() {
    // The page grows over #z (focused by the first build, so `FocusMemory`
    // remembers it): the growth replaces #z with #a #b #c, so the
    // focus-lost fallback re-picks with the memory stale. The build spawns
    // in document order, so document order (#a first) and `Entity` order
    // disagree (#c holds the largest index).
    let real = r#"{% if grown %}<div id="a" tabindex="0">A</div><div id="b" tabindex="0">B</div><div id="c" tabindex="0">C</div>{% else %}<div id="z" tabindex="0">Z</div>{% endif %}"#;
    let mut ui = page_ui(
        "focus-doc-order",
        real,
        "div { width: 40px; height: 20px }",
        TemplateContext::new().with("grown", &false),
    );
    ui.settle();
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("grown", &true);
    ui.settle();
    ui.update(1);
    let (a, b, c) = {
        let world = ui.world_mut();
        (
            element_by_id(world, root, "a"),
            element_by_id(world, root, "b"),
            element_by_id(world, root, "c"),
        )
    };
    // The document-order setup this regression is about: document order
    // (#a first) and `Entity` order disagree (#c holds the largest index,
    // so an `Entity`-ordered fallback never picks #a).
    assert!(
        a.index_u32() < b.index_u32() && b.index_u32() < c.index_u32(),
        "precondition: the page's entities order {a:?} {b:?} {c:?}"
    );

    // The focus-lost fallback: point focus somewhere that isn't a markup
    // focusable (the window, as Bevy's input dispatch does at startup) and
    // let `repair_focus` re-pick.
    let window = ui.window();
    ui.world_mut()
        .resource_mut::<InputFocus>()
        .set(window, FocusCause::Navigated);
    ui.update(1);
    let focused = {
        let world = ui.world_mut();
        world.resource::<InputFocus>().get()
    };
    assert_eq!(
        focused,
        Some(a),
        "the fallback focuses the document-first element, not the lowest Entity"
    );
}

/// bug_0080: a signal handler that moves `InputFocus` shows the new
/// element's focus state the same frame — `update_focus_states` runs after
/// the dispatcher, so the frame that carried the click restyles the
/// element; it used to lag a frame behind.
#[test]
fn a_handler_moved_focus_shows_its_state_the_same_frame() {
    let page = r#"<div id="move" data-on-click="focus-target"><p>Move</p></div>
<button id="target" tabindex="0">Target</button>"#;
    let mut ui = page_ui(
        "handler-focus-state",
        page,
        "div, button { width: 100px; height: 40px }",
        TemplateContext::new(),
    );
    ui.settle();
    ui.update(1);
    let root = ui.root();
    let (target, _) = center(ui.world_mut(), root, "target");
    let (_, at) = center(ui.world_mut(), root, "move");
    ui.app_mut().on_html_click(
        "focus-target",
        move |_: In<ElementSignal>, mut focus: ResMut<InputFocus>| {
            focus.set(target, FocusCause::Navigated);
        },
    );
    // The click's release frame runs the handler; the target's focused
    // state must be set by the end of that very frame.
    ui.click_at(at);
    let state = {
        let world = ui.world_mut();
        pseudo(world, target)
    };
    assert!(
        state.focused,
        "the handler's focus shows its state the same frame (state {state:?})"
    );
}
