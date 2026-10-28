//! Form submission: a `<form data-on-submit="signup">` collects its named
//! `<input>`s when its `<button type="submit">` is clicked — the form emits
//! one `Submit` signal whose payload is a JSON object of `{name: value}`
//! (unnamed inputs excluded). bevy_markup has no HTTP client; the app
//! decides what submitting means. This one puts the payload in the context:
//! the template swaps the form for a receipt panel (Tera `{% if %}`), and
//! "edit again" clears it.
//!
//! The `text-input` feature renders the fields and carries their live
//! values; without it the form still submits, with an empty payload.
//!
//! `cargo run --example forms --features text-input` — click the fields and
//! type, or move with ↑ ↓ / D-pad (while a field has the focus, ← → move
//! the caret); Enter / A presses the focused button. Space / Y switches
//! between English and German.

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy_markup::prelude::*;

#[path = "../shared/input.rs"]
mod input;

#[path = "../shared/harness.rs"]
mod harness;

#[hotpath::main]
fn main() {
    harness::run(
        harness::ExampleWindow {
            title: "bevy_markup: forms",
            ..Default::default()
        },
        |app| {
            app.add_systems(Startup, (setup, warn_without_feature))
                .add_systems(Update, (handle_signals, switch_language));
        },
    );
}

/// Both locales, preloaded so switching is immediate.
#[derive(Resource)]
struct Languages(Vec<Handle<LocaleBundle>>);

fn setup(mut commands: Commands, asset_server: Res<AssetServer>, mut fonts: ResMut<FontFamilies>) {
    commands.spawn(Camera2d);

    // CSS generic keywords → the system's fonts (Bevy's system_font_discovery).
    fonts
        .insert("System Serif", FontFaces::new(FontSource::serif()))
        .insert("System Mono", FontFaces::new(FontSource::monospace()))
        .set_generic(GenericFamily::Serif, "System Serif")
        .set_generic(GenericFamily::Monospace, "System Mono");

    commands.insert_resource(DefaultStylesheet::new(asset_server.load("forms/style.css")));

    let languages: Vec<Handle<LocaleBundle>> = ["en-US", "de"]
        .iter()
        .map(|id| asset_server.load(format!("forms/locales/{id}/main.ftl.ron")))
        .collect();
    commands.insert_resource(ActiveLocale::new(languages[0].clone()));
    commands.insert_resource(Languages(languages));

    commands.spawn((
        HtmlUi::new(asset_server.load("forms/signup.html")),
        TemplateContext::new().with("name", "Ada"),
        Node {
            width: Val::Px(560.0),
            margin: UiRect::all(Val::Auto),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(14.0),
            ..default()
        },
    ));
}

/// The example is pointless without the feature that renders the fields.
fn warn_without_feature() {
    #[cfg(not(feature = "text-input"))]
    warn!("run with --features text-input, or the <input> fields won't render");
}

/// The signals: `Submit` carries the named inputs' values as a JSON object,
/// routed like any other signal. The payload goes into the context as
/// `submitted`, and the template's `{% if submitted %}` swaps the form for
/// the receipt panel; "edit again" clears it, "clear fields" empties the
/// two fields.
fn handle_signals(
    root: Query<Entity, With<HtmlUi>>,
    elements: HtmlElements,
    mut signals: MessageReader<ElementSignal>,
    mut contexts: Query<&mut TemplateContext>,
    mut commands: Commands,
) {
    let Ok(root) = root.single() else {
        return;
    };
    for signal in signals.read() {
        match signal.trigger {
            // `Submit` carries the named inputs' values as a JSON object;
            // the receipt prints it as its `data-l10n-args` (`| json`).
            SignalTrigger::Submit if signal.name == "signup" => {
                for mut context in &mut contexts {
                    context.insert("submitted", &signal.payload);
                }
            }
            SignalTrigger::Click if signal.name == "edit-again" => {
                for mut context in &mut contexts {
                    context.remove("submitted");
                }
            }
            SignalTrigger::Click if signal.name == "clear-form" => {
                // The fields' text is app state: the app writes the
                // `EditableText`s (a re-render sets the text only when the
                // template's `value` attribute changes, so this stays put).
                for id in ["name", "role"] {
                    if let Some(field) = elements.by_id(root, id) {
                        commands.entity(field).insert(EditableText::default());
                    }
                }
            }
            _ => {}
        }
    }
}

fn switch_language(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    focus: Res<bevy::input_focus::InputFocus>,
    focused_text: Query<(), (With<bevy::ui_widgets::TextInput>, With<EditableText>)>,
    languages: Res<Languages>,
    mut active: ResMut<ActiveLocale>,
) {
    let pad = gamepads
        .iter()
        .any(|pad| pad.just_pressed(GamepadButton::North));
    // Space is a typed character in a text field: only switch the
    // language while nothing holds the caret.
    let typing = focus
        .get()
        .is_some_and(|focused| focused_text.contains(focused))
        && keys.pressed(KeyCode::Space);
    if keys.just_pressed(KeyCode::Space) && !typing || pad {
        next_language(&languages, &mut active);
    }
}

fn next_language(languages: &Languages, active: &mut ActiveLocale) {
    let current = languages
        .0
        .iter()
        .position(|bundle| active.0.as_ref() == Some(bundle))
        .unwrap_or(0);
    active.set(languages.0[(current + 1) % languages.0.len()].clone());
}
