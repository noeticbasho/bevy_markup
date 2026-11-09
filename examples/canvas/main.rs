//! `<canvas>` (ADR 0041): a character sheet whose portrait is a live 3D
//! render. The hero exists only on render layer 1: the portrait camera sees
//! it, the world camera (layer 0) doesn't, so it never appears in the world
//! behind the sheet. The template names the source
//! (`<canvas data-canvas="portrait">`); the app registers its camera under
//! that name in `HtmlCanvases`.
//!
//! - Bevy resizes the camera's image to the canvas (HiDPI included) and
//!   forwards the pointer into its scene: dragging over the portrait turns
//!   the hero, clicking it changes the armor (Bevy's mesh picking, through
//!   the canvas).
//! - The thumbnail names the same camera: one canvas per camera gets the
//!   live viewport, the other shows its image.
//! - "Hide the portrait" hides both; `HtmlCanvases::shown` then turns the
//!   camera off (the status line says whether it renders).
//!
//! `cargo run --example canvas` — drag or click the portrait; Q / E or LB /
//! RB turn the hero; ↑ ↓ / D-pad + Enter / A press the buttons; Space / Y
//! switches English/German.

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::picking::events::{PointerClick, PointerDrag};
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
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
            title: "bevy_markup: canvas",
            ..Default::default()
        },
        |app| {
            app.add_plugins(MeshPickingPlugin)
                .add_systems(Startup, (setup_world, setup_portrait, setup_sheet))
                .add_systems(
                    Update,
                    (
                        handle_signals,
                        turn_with_keys,
                        switch_language,
                        portrait_camera_follows_the_sheet,
                    ),
                );
        },
    );
}

/// The hero's render layer: only the portrait camera draws it.
const PORTRAIT_LAYER: usize = 1;

/// The armors the hero cycles through: name (a Fluent selector) and color.
const ARMORS: [(&str, Color); 3] = [
    ("leather", Color::srgb(0.55, 0.35, 0.2)),
    ("steel", Color::srgb(0.7, 0.72, 0.78)),
    ("gold", Color::srgb(0.9, 0.7, 0.2)),
];

/// The hero's root (turned by drags and keys).
#[derive(Component)]
struct Hero;

/// The mesh whose material is the armor.
#[derive(Component)]
struct Armor;

#[derive(Resource)]
struct PortraitCamera(Entity);

/// Both locales, preloaded so switching is immediate.
#[derive(Resource)]
struct Languages(Vec<Handle<LocaleBundle>>);

/// The world behind the sheet, on the default layer 0: a ground and a few
/// pillars around the origin, where the hero stands in the portrait's
/// layer — and isn't seen here.
fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        IsDefaultUiCamera,
        Transform::from_xyz(0.0, 4.0, 9.0).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 3000.0,
            ..default()
        },
        Transform::from_xyz(3.0, 6.0, 2.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(20.0, 20.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.18, 0.22, 0.2))),
    ));
    let pillar = meshes.add(Cuboid::new(0.6, 2.0, 0.6));
    let stone = materials.add(Color::srgb(0.45, 0.45, 0.5));
    for index in 0..6 {
        let angle = index as f32 * std::f32::consts::TAU / 6.0;
        commands.spawn((
            Mesh3d(pillar.clone()),
            MeshMaterial3d(stone.clone()),
            Transform::from_xyz(angle.cos() * 4.0, 1.0, angle.sin() * 4.0),
        ));
    }
}

/// The hero and the camera that draws it into an image, all on
/// [`PORTRAIT_LAYER`], with their own light. The camera is registered as the
/// `portrait` canvas source.
fn setup_portrait(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut canvases: ResMut<HtmlCanvases>,
) {
    let layer = RenderLayers::layer(PORTRAIT_LAYER);
    // Any size: Bevy resizes it to the canvas.
    let image = images.add(Image::new_target_texture(
        240,
        320,
        TextureFormat::Bgra8UnormSrgb,
        None,
    ));
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                // Before the window camera; transparent, so the canvas's CSS
                // background shows behind the hero.
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(image.into()),
            // Its own ambient light (the global one lights the world).
            AmbientLight {
                brightness: 400.0,
                ..default()
            },
            layer.clone(),
            Transform::from_xyz(0.0, 1.1, 3.2).looking_at(Vec3::new(0.0, 0.9, 0.0), Vec3::Y),
        ))
        .id();
    commands.insert_resource(PortraitCamera(camera));
    canvases.insert("portrait", CanvasSource::Camera(camera));

    commands.spawn((
        DirectionalLight {
            illuminance: 4000.0,
            ..default()
        },
        layer.clone(),
        Transform::from_xyz(2.0, 3.0, 3.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));

    // Every mesh entity carries the layer: a parent's `RenderLayers` doesn't
    // reach its children.
    let skin = materials.add(Color::srgb(0.85, 0.68, 0.55));
    let armor = materials.add(ARMORS[0].1);
    commands
        .spawn((Hero, Transform::default(), Visibility::default()))
        .with_children(|hero| {
            hero.spawn((
                Armor,
                Mesh3d(meshes.add(Capsule3d::new(0.35, 0.8))),
                MeshMaterial3d(armor),
                Transform::from_xyz(0.0, 0.75, 0.0),
                layer.clone(),
            ));
            hero.spawn((
                Mesh3d(meshes.add(Sphere::new(0.25))),
                MeshMaterial3d(skin.clone()),
                Transform::from_xyz(0.0, 1.55, 0.0),
                layer.clone(),
            ));
            // A nose, so turning shows.
            hero.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.08, 0.08, 0.12))),
                MeshMaterial3d(skin),
                Transform::from_xyz(0.0, 1.55, 0.27),
                layer.clone(),
            ));
        })
        // Picking events bubble from the meshes to the root: the pointer
        // reaches them through the canvas (Bevy's `viewport_picking`).
        .observe(turn_on_drag)
        .observe(change_armor_on_click);
}

fn setup_sheet(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut fonts: ResMut<FontFamilies>,
) {
    fonts
        .insert("System Serif", FontFaces::new(FontSource::serif()))
        .insert("System Mono", FontFaces::new(FontSource::monospace()))
        .set_generic(GenericFamily::Serif, "System Serif")
        .set_generic(GenericFamily::Monospace, "System Mono");

    commands.insert_resource(DefaultStylesheet::new(
        asset_server.load("canvas/style.css"),
    ));

    let languages: Vec<Handle<LocaleBundle>> = ["en-US", "de"]
        .iter()
        .map(|id| asset_server.load(format!("canvas/locales/{id}/main.ftl.ron")))
        .collect();
    commands.insert_resource(ActiveLocale::new(languages[0].clone()));
    commands.insert_resource(Languages(languages));

    commands.spawn((
        HtmlUi::new(asset_server.load("canvas/sheet.html")),
        TemplateContext::new()
            .with("name", "Ada")
            .with("armor", ARMORS[0].0)
            .with("portrait", &true)
            .with("rendering", &true),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(24.0),
            top: Val::Px(24.0),
            width: Val::Px(620.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    ));
}

fn turn_on_drag(drag: On<PointerDrag>, mut heroes: Query<&mut Transform, With<Hero>>) {
    for mut transform in &mut heroes {
        transform.rotate_y(drag.delta.x * 0.02);
    }
}

fn change_armor_on_click(
    _click: On<PointerClick>,
    mut contexts: Query<&mut TemplateContext>,
    armor: Query<&MeshMaterial3d<StandardMaterial>, With<Armor>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    next_armor(&mut contexts, &armor, &mut materials);
}

fn next_armor(
    contexts: &mut Query<&mut TemplateContext>,
    armor: &Query<&MeshMaterial3d<StandardMaterial>, With<Armor>>,
    materials: &mut Assets<StandardMaterial>,
) {
    for mut context in contexts.iter_mut() {
        let current = context
            .get("armor")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let index = ARMORS
            .iter()
            .position(|(name, _)| *name == current)
            .unwrap_or(0);
        let (name, color) = ARMORS[(index + 1) % ARMORS.len()];
        context.insert("armor", name);
        for material in armor {
            if let Some(mut material) = materials.get_mut(material) {
                material.base_color = color;
            }
        }
    }
}

fn handle_signals(
    mut signals: MessageReader<ElementSignal>,
    mut contexts: Query<&mut TemplateContext>,
    armor: Query<&MeshMaterial3d<StandardMaterial>, With<Armor>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    languages: Res<Languages>,
    mut active: ResMut<ActiveLocale>,
) {
    for signal in signals.read() {
        if signal.trigger != SignalTrigger::Click {
            continue;
        }
        match signal.name.as_ref() {
            "armor" => next_armor(&mut contexts, &armor, &mut materials),
            "toggle-portrait" => {
                for mut context in &mut contexts {
                    let shown = context
                        .get("portrait")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    context.insert("portrait", &!shown);
                }
            }
            "language" => next_language(&languages, &mut active),
            _ => {}
        }
    }
}

/// Q / E, LB / RB: turn the hero without a pointer.
fn turn_with_keys(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    mut heroes: Query<&mut Transform, With<Hero>>,
) {
    let held = |key, button| keys.pressed(key) || gamepads.iter().any(|pad| pad.pressed(button));
    let direction = held(KeyCode::KeyE, GamepadButton::RightTrigger) as i32
        - held(KeyCode::KeyQ, GamepadButton::LeftTrigger) as i32;
    for mut transform in &mut heroes {
        transform.rotate_y(direction as f32 * 2.0 * time.delta_secs());
    }
}

/// The camera renders only while a canvas shows it (`HtmlCanvases::shown`);
/// the status line reports it.
fn portrait_camera_follows_the_sheet(
    canvases: Res<HtmlCanvases>,
    portrait: Res<PortraitCamera>,
    mut cameras: Query<&mut Camera>,
    mut contexts: Query<&mut TemplateContext>,
) {
    let shown = canvases.shown("portrait");
    let Ok(mut camera) = cameras.get_mut(portrait.0) else {
        return;
    };
    if camera.is_active != shown {
        camera.is_active = shown;
        for mut context in &mut contexts {
            context.insert("rendering", &shown);
        }
    }
}

fn switch_language(hotkeys: Hotkeys, languages: Res<Languages>, mut active: ResMut<ActiveLocale>) {
    if hotkeys.just_pressed(KeyCode::Space, GamepadButton::North) {
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
