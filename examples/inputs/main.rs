//! Text input fields (ADR 0016): `<input type="text">` renders Bevy's
//! editable-text widget behind the `text-input` cargo feature. The value is
//! app state — this app reads the focused fields' live text into the
//! template's context, so a preview paragraph follows every keystroke, and
//! "reset" buttons write the fields back to their initial value.
//!
//! `cargo run --example inputs --features text-input` — click a field and
//! type, or move between the fields with ↑ ↓ / D-pad (while a field has the
//! focus, ← → move the caret); Enter / A presses the reset buttons.
//! Space / Y switches between English and German.

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
            title: "bevy_markup: text inputs",
            ..Default::default()
        },
        |app| {
            app.add_systems(Startup, (setup, warn_without_feature))
                .add_systems(
                    Update,
                    (read_fields_into_preview, handle_signals, switch_language),
                );
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

    commands.insert_resource(DefaultStylesheet::new(
        asset_server.load("inputs/style.css"),
    ));

    let languages: Vec<Handle<LocaleBundle>> = ["en-US", "de"]
        .iter()
        .map(|id| asset_server.load(format!("inputs/locales/{id}/main.ftl.ron")))
        .collect();
    commands.insert_resource(ActiveLocale::new(languages[0].clone()));
    commands.insert_resource(Languages(languages));

    commands.spawn((
        HtmlUi::new(asset_server.load("inputs/profile.html")),
        TemplateContext::new()
            .with("name", "Ada")
            .with("title", "Cartographer")
            // What the preview paragraph shows; kept in step with the
            // fields' live text by `read_fields_into_preview`.
            .with("live_name", "Ada")
            .with("live_title", "Cartographer"),
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

/// The fields' text is app state: read it (by id, through [`HtmlElements`])
/// and feed it to the preview paragraph's context. Only changes re-render
/// (an identical render is skipped, but don't dirty the context per frame).
fn read_fields_into_preview(
    root: Query<Entity, With<HtmlUi>>,
    elements: HtmlElements,
    fields: Query<&EditableText>,
    mut contexts: Query<&mut TemplateContext>,
) {
    let Ok(root) = root.single() else {
        return;
    };
    let mut live: Vec<(&'static str, String)> = Vec::new();
    for (id, key) in [("name", "live_name"), ("title", "live_title")] {
        let text = elements
            .by_id(root, id)
            .and_then(|entity| fields.get(entity).ok())
            .map(|editable| editable.editor.text().to_string());
        let Some(text) = text else { return };
        live.push((key, text));
    }
    for mut context in &mut contexts {
        let changed = live
            .iter()
            .any(|(key, value)| context.get(key).and_then(|v| v.as_str()) != Some(value.as_str()));
        if changed {
            for (key, value) in live.iter() {
                context.insert(*key, value);
            }
        }
    }
}

/// The reset buttons write a field back to its initial value. The field's
/// text is app state: the app sets the `EditableText`, and the widget
/// renders it (a re-render sets the text only when the template's `value`
/// attribute changes).
fn handle_signals(
    root: Query<Entity, With<HtmlUi>>,
    elements: HtmlElements,
    mut signals: MessageReader<ElementSignal>,
    mut commands: Commands,
) {
    let Ok(root) = root.single() else {
        return;
    };
    for signal in signals.read() {
        if signal.trigger != SignalTrigger::Click {
            continue;
        }
        let initial = match signal.name.as_ref() {
            "reset-name" => ("name", "Ada"),
            "reset-title" => ("title", "Cartographer"),
            _ => continue,
        };
        if let Some(field) = elements.by_id(root, initial.0) {
            commands
                .entity(field)
                .insert(EditableText::new(initial.1.to_string()));
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
