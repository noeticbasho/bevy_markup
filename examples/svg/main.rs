//! SVG (ADR 0038, the `svg` feature): inline `<svg>` icons whose
//! `currentColor` follows the CSS color (the theme button and `:hover`
//! recolor them), one `star.svg` as an `<img>` at four CSS sizes (each
//! drawn again at its displayed pixels), an SVG `border-image` frame and a
//! tiled SVG `background-image`, an SVG with `<text>` (drawn with the
//! `svg-text` feature), and a hostile SVG (a `<use>` cycle) that's refused
//! while the page stays intact. The scale button steps `UiScale` (1×,
//! 1.5×, 2×): every SVG is drawn again at the new pixel density.
//!
//! `cargo run --example svg --features svg` (add `svg-text` for the text
//! badge). Click the buttons, or arrows / D-pad + Enter / A; T / X theme,
//! S / B scale, L / Y language.

use bevy::prelude::*;
use bevy_markup::prelude::*;

#[path = "../shared/input.rs"]
mod input;

#[path = "../shared/harness.rs"]
mod harness;

use input::Hotkeys;

#[hotpath::main]
fn main() {
    harness::run(
        harness::ExampleWindow {
            title: "bevy_markup: SVG",
            ..Default::default()
        },
        |app| {
            app.init_resource::<Settings>()
                .add_systems(Startup, setup)
                .add_systems(Update, (handle_signals, hotkeys, apply_settings).chain());
        },
    );
}

/// The accent and ink colors per theme (context `var()`s).
const THEMES: [(&str, &str); 3] = [
    ("#f0b429", "#e6edf3"),
    ("#7fdb8f", "#e6f3ea"),
    ("#ff7aa2", "#f3e6ec"),
];

/// The `UiScale` steps.
const SCALES: [f32; 3] = [1.0, 1.5, 2.0];

/// What the buttons and hotkeys change.
#[derive(Resource, Default)]
struct Settings {
    theme: usize,
    scale: usize,
    language: usize,
}

/// Both locales, preloaded so switching is immediate.
#[derive(Resource)]
struct Languages(Vec<Handle<LocaleBundle>>);

fn setup(mut commands: Commands, asset_server: Res<AssetServer>, mut fonts: ResMut<FontFamilies>) {
    commands.spawn(Camera2d);

    // CSS generic keywords → the system's fonts. (SVG `<text>` can't use
    // system families: it falls back to Bevy's default font.)
    fonts
        .insert("System Sans", FontFaces::new(FontSource::sans_serif()))
        .insert("System Mono", FontFaces::new(FontSource::monospace()))
        .set_generic(GenericFamily::SansSerif, "System Sans")
        .set_generic(GenericFamily::Monospace, "System Mono");

    commands.insert_resource(DefaultStylesheet::new(asset_server.load("svg/style.css")));
    let languages: Vec<Handle<LocaleBundle>> = ["en-US", "de"]
        .iter()
        .map(|id| asset_server.load(format!("svg/locales/{id}/main.ftl.ron")))
        .collect();
    commands.insert_resource(ActiveLocale::new(languages[0].clone()));
    commands.insert_resource(Languages(languages));

    commands.spawn((
        HtmlUi::new(asset_server.load("svg/page.html")),
        context(&Settings::default()),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    ));
}

/// The template's context: the theme's colors and the labels' arguments.
fn context(settings: &Settings) -> TemplateContext {
    let (accent, ink) = THEMES[settings.theme];
    TemplateContext::new()
        .with("theme", &settings.theme)
        .with("scale", &SCALES[settings.scale])
        .with("--accent", accent)
        .with("--ink", ink)
}

/// Clicks (and activations) on the toolbar.
fn handle_signals(mut signals: MessageReader<ElementSignal>, mut settings: ResMut<Settings>) {
    for signal in signals.read() {
        if signal.trigger != SignalTrigger::Click {
            continue;
        }
        step(&mut settings, signal.name.as_ref());
    }
}

/// The same actions from the keyboard and a gamepad.
fn hotkeys(hotkeys: Hotkeys, mut settings: ResMut<Settings>) {
    if hotkeys.just_pressed(KeyCode::KeyT, GamepadButton::West) {
        step(&mut settings, "theme");
    }
    if hotkeys.just_pressed(KeyCode::KeyS, GamepadButton::East) {
        step(&mut settings, "scale");
    }
    if hotkeys.just_pressed(KeyCode::KeyL, GamepadButton::North) {
        step(&mut settings, "language");
    }
}

fn step(settings: &mut Settings, action: &str) {
    match action {
        "theme" => settings.theme = (settings.theme + 1) % THEMES.len(),
        "scale" => settings.scale = (settings.scale + 1) % SCALES.len(),
        "language" => settings.language = (settings.language + 1) % 2,
        _ => {}
    }
}

/// The settings into the context, `UiScale` and the active locale.
fn apply_settings(
    settings: Res<Settings>,
    languages: Res<Languages>,
    mut active: ResMut<ActiveLocale>,
    mut ui_scale: ResMut<UiScale>,
    mut contexts: Query<&mut TemplateContext>,
) {
    if !settings.is_changed() {
        return;
    }
    for mut current in &mut contexts {
        *current = context(&settings);
    }
    if ui_scale.0 != SCALES[settings.scale] {
        ui_scale.0 = SCALES[settings.scale];
    }
    let language = languages.0[settings.language % languages.0.len()].clone();
    if active.0.as_ref() != Some(&language) {
        active.set(language);
    }
}
