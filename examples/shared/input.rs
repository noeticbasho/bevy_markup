//! Keyboard and gamepad input shared by the examples (`#[path]`-included;
//! not an example itself). bevy_markup owns focus and activation but reads
//! no input (the app picks its bindings), so this is the app side:
//!
//! - arrows / D-pad / left stick move focus ([`HtmlFocus::navigate`]), with
//!   hold-to-repeat; on a focused element with `data-setting="<name>"`,
//!   left/right instead send a [`SettingStep`] for the app to apply
//!   (sliders, steppers). With [`ArrowMode::Linear`] (an app resource),
//!   left/right instead step through the focusable elements in Tab order
//!   and up/down are left to the app (the demo scrolls with them);
//! - Tab / Shift+Tab move to the next / previous element in Tab order
//!   ([`HtmlFocus::tab`]: positive `tabindex` first, then document order);
//! - Enter / gamepad A activate the focused element
//!   ([`HtmlFocus::activate`]): the same `ElementSignal` a click sends, with
//!   the key or button as its source;
//! - [`Hotkeys`] maps an example's shortcut key to a gamepad button too.
//!
//! The mouse needs nothing: picking drives hover, clicks and `:focus-visible`
//! (a press hides the ring, navigating shows it).

// Each example uses the parts it needs.
#![allow(dead_code)]

use bevy::ecs::system::SystemParam;
use bevy::math::CompassOctant;
use bevy::prelude::*;
use bevy_markup::prelude::*;

pub struct ExampleInputPlugin;

impl Plugin for ExampleInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SettingStep>()
            .init_resource::<ArrowMode>()
            .add_systems(
                Update,
                (
                    navigate.run_if(resource_equals(ArrowMode::Directional)),
                    step_linear.run_if(resource_equals(ArrowMode::Linear)),
                    tab,
                    activate,
                ),
            );
    }
}

/// What the arrows / D-pad / left stick do.
#[derive(Resource, Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum ArrowMode {
    /// All four directions move focus to the nearest element that way.
    #[default]
    Directional,
    /// Left/right step to the previous/next focusable element in Tab order
    /// (document order unless a positive `tabindex` reorders it); up/down
    /// do nothing here (the app uses them, e.g. to scroll).
    Linear,
}

/// Left/right on a focused `data-setting` element: step that setting.
#[derive(Message, Clone, Debug)]
pub struct SettingStep {
    /// The element's `data-setting` value.
    pub setting: String,
    /// -1 or 1.
    pub steps: i32,
}

/// An example's shortcut on the keyboard *and* a gamepad button.
#[derive(SystemParam)]
pub struct Hotkeys<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    gamepads: Query<'w, 's, &'static Gamepad>,
}

impl Hotkeys<'_, '_> {
    pub fn just_pressed(&self, key: KeyCode, button: GamepadButton) -> bool {
        self.keys.just_pressed(key) || self.gamepads.iter().any(|pad| pad.just_pressed(button))
    }
}

/// Hold-to-repeat for one axis: fires on press, then after [`Self::DELAY`]
/// every [`Self::INTERVAL`] while held.
#[derive(Default)]
pub struct Repeat {
    held: i32,
    next: f32,
}

impl Repeat {
    pub const DELAY: f32 = 0.35;
    pub const INTERVAL: f32 = 0.08;

    /// `direction` is -1, 0 or 1 this frame; returns the direction to act on.
    pub fn update(&mut self, direction: i32, now: f32) -> i32 {
        if direction == 0 {
            self.held = 0;
            return 0;
        }
        if direction != self.held {
            self.held = direction;
            self.next = now + Self::DELAY;
            return direction;
        }
        if now >= self.next {
            self.next = now + Self::INTERVAL;
            return direction;
        }
        0
    }
}

/// Held directional input: -1/0/1 per axis from the arrow keys, the D-pad
/// and the left stick (any gamepad). `x` is right-positive, `y` up-positive.
#[derive(SystemParam)]
pub struct Arrows<'w, 's> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    gamepads: Query<'w, 's, &'static Gamepad>,
}

impl Arrows<'_, '_> {
    pub fn axes(&self) -> IVec2 {
        let axis = |negative: KeyCode,
                    positive: KeyCode,
                    pad_negative,
                    pad_positive,
                    stick: fn(&Gamepad) -> f32| {
            let pad = |button| self.gamepads.iter().any(|pad| pad.pressed(button));
            let stick = self
                .gamepads
                .iter()
                .map(stick)
                .find(|value| value.abs() > 0.5);
            let positive =
                self.keys.pressed(positive) || pad(pad_positive) || stick.is_some_and(|v| v > 0.0);
            let negative =
                self.keys.pressed(negative) || pad(pad_negative) || stick.is_some_and(|v| v < 0.0);
            positive as i32 - negative as i32
        };
        IVec2::new(
            axis(
                KeyCode::ArrowLeft,
                KeyCode::ArrowRight,
                GamepadButton::DPadLeft,
                GamepadButton::DPadRight,
                |pad| pad.left_stick().x,
            ),
            axis(
                KeyCode::ArrowDown,
                KeyCode::ArrowUp,
                GamepadButton::DPadDown,
                GamepadButton::DPadUp,
                |pad| pad.left_stick().y,
            ),
        )
    }
}

fn navigate(
    time: Res<Time>,
    arrows: Arrows,
    elements: Query<&HtmlElement>,
    #[cfg(feature = "text-input")] text_fields: Query<(), With<bevy::ui_widgets::TextInput>>,
    mut repeat: Local<(Repeat, Repeat)>,
    mut steps: MessageWriter<SettingStep>,
    mut focus: HtmlFocus,
) {
    let IVec2 {
        x: horizontal,
        y: vertical,
    } = arrows.axes();
    // While a text field holds the focus, left/right move its caret (the
    // widget reads the arrow keys through `FocusedInput`), so they don't
    // navigate; up/down still leave the field.
    #[cfg(feature = "text-input")]
    let horizontal = if focus
        .focused()
        .is_some_and(|entity| text_fields.contains(entity))
    {
        0
    } else {
        horizontal
    };
    let now = time.elapsed_secs();
    let (horizontal_repeat, vertical_repeat) = &mut *repeat;
    match vertical_repeat.update(vertical, now) {
        1 => drop(focus.navigate(CompassOctant::North)),
        -1 => drop(focus.navigate(CompassOctant::South)),
        _ => {}
    }
    let setting = focus
        .focused()
        .and_then(|entity| elements.get(entity).ok())
        .and_then(|element| element.data("setting").map(str::to_owned));
    match (horizontal_repeat.update(horizontal, now), setting) {
        (0, _) => {}
        (steps_by, Some(setting)) => {
            steps.write(SettingStep {
                setting,
                steps: steps_by,
            });
        }
        (1, None) => drop(focus.navigate(CompassOctant::East)),
        (_, None) => drop(focus.navigate(CompassOctant::West)),
    }
}

/// [`ArrowMode::Linear`]: left/right step through the focusable elements in
/// Tab order ([`HtmlFocus::tab`]), stopping at the ends, and show the focus
/// ring like navigating does.
fn step_linear(time: Res<Time>, arrows: Arrows, mut repeat: Local<Repeat>, mut focus: HtmlFocus) {
    let step = repeat.update(arrows.axes().x, time.elapsed_secs());
    if step == 0 {
        return;
    }
    let order = focus.tab_order();
    let at_end = focus
        .focused()
        .and_then(|current| order.iter().position(|&entity| entity == current))
        .is_some_and(|index| {
            if step > 0 {
                index + 1 == order.len()
            } else {
                index == 0
            }
        });
    if !at_end {
        focus.tab(if step > 0 {
            NavAction::Next
        } else {
            NavAction::Previous
        });
    }
}

/// Tab / Shift+Tab: the next / previous element in Tab order, wrapping.
fn tab(keys: Res<ButtonInput<KeyCode>>, mut focus: HtmlFocus) {
    if keys.just_pressed(KeyCode::Tab) {
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        focus.tab(if shift {
            NavAction::Previous
        } else {
            NavAction::Next
        });
    }
}

/// Enter / gamepad A: activate the focused element.
fn activate(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut focus: HtmlFocus,
) {
    let activation = if keys.just_pressed(KeyCode::Enter) {
        Some(ActivationInput::Key(KeyCode::Enter))
    } else {
        gamepads
            .iter()
            .find(|(_, pad)| pad.just_pressed(GamepadButton::South))
            .map(|(gamepad, _)| ActivationInput::GamepadButton {
                gamepad,
                button: GamepadButton::South,
            })
    };
    if let Some(input) = activation {
        focus.activate(input);
    }
}
