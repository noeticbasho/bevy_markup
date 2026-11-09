//! The agent/QA tool surface behind the crate's `dev-tools` feature:
//! [`bevy_mcp_harness`] serves the example over localhost BRP (JSON-RPC on
//! 127.0.0.1:15702 — `game/screenshot`, `game/ui`, `game/keyboard`,
//! `game/gamepad`, `game/mouse`) and MCP (127.0.0.1:15710/mcp), so an agent
//! can playtest, click and capture without touching the example's code.
//!
//! [`run`] is also the examples' launcher and knows three compositions:
//!
//! - default: the windowed app (what a user runs);
//! - `--no-render`: a **render-less agent host** — no window, no winit, no
//!   wgpu at all (see `bevy_mcp_harness`'s headless example). UI layout,
//!   picking, input mocks and `game/ui` all work; `game/screenshot`
//!   returns a clean error (nothing renders). The frame loop is driven by
//!   `ScheduleRunnerPlugin`, so the app never waits for window focus —
//!   a windowed run freezes its loop when unfocused, which breaks agent
//!   QA.
//! - `--offscreen`: the windowed plugins minus winit, rendering into the
//!   harness's offscreen texture (`game/screenshot` captures real frames,
//!   still without a window). Needs a GPU or lavapipe.
//!
//! Off by default (`--features dev-tools`); the plugin compiles away
//! otherwise, so examples ship unchanged. Screenshots land in
//! `mcp_harness/screenshots/` (gitignored), full-resolution PNGs with a
//! state sidecar. Port flags: `--brp-port N`, `--mcp-port N`.
//!
//! Dev/QA only: BRP is unauthenticated by design and binds to 127.0.0.1.

use bevy::app::{ScheduleRunnerPlugin, TaskPoolPlugin};
use bevy::diagnostic::FrameCountPlugin;
use bevy::picking::input::PointerInputPlugin;
use bevy::prelude::*;
use bevy::time::TimePlugin;
use bevy::window::ExitCondition;

/// The launcher's settings: the windowed composition's window, and whether
/// the example uses the shared input bindings.
pub struct ExampleWindow {
    /// The window title (headless runs ignore it).
    pub title: &'static str,
    /// The resolution; default 1280×720.
    pub resolution: (u32, u32),
    /// Add [`super::input::ExampleInputPlugin`] (arrows / D-pad / stick
    /// navigation, Enter / A activation); default on. Off for an example
    /// that binds those inputs itself — both bindings would act on one press.
    pub shared_input: bool,
    /// Runs on the empty `App`, before any plugin: what must exist before
    /// `AssetPlugin` (an extra asset source, as the testbench's in-memory
    /// `testbench://` one). Default: nothing.
    pub before_plugins: fn(&mut App),
}

impl Default for ExampleWindow {
    fn default() -> Self {
        Self {
            title: "",
            resolution: (1280, 720),
            shared_input: true,
            before_plugins: |_| {},
        }
    }
}

/// The frame loop's rate for the headless compositions.
const HEADLESS_FRAME_RATE: f64 = 1.0 / 60.0;

/// Runs an example: builds the app with the right composition for the
/// command line (windowed, `--no-render`, `--offscreen`), then hands it to
/// `setup` (systems, resources, the example's own wiring) and runs it.
pub fn run(window: ExampleWindow, setup: impl FnOnce(&mut App)) {
    let flag = |name: &str, env: &str| {
        std::env::args().any(|arg| arg == name)
            || std::env::var(env).is_ok_and(|value| !value.is_empty() && value != "0")
    };
    let no_render = flag("--no-render", "BEVY_MARKUP_NO_RENDER");
    let headless = no_render || flag("--offscreen", "BEVY_MARKUP_OFFSCREEN");
    let mut app = App::new();
    (window.before_plugins)(&mut app);
    if headless && !no_render {
        // Offscreen-rendered headless: full `DefaultPlugins` minus winit —
        // `ScheduleRunnerPlugin` drives the frame loop, and every camera
        // renders into the harness's offscreen texture (`game/screenshot`
        // captures real frames; needs a GPU or lavapipe). The render app
        // runs one frame behind the main app; with no winit driving frames
        // that pipeline deadlocks (the harness's headless example's note).
        app.add_plugins((
            DefaultPlugins
                .build()
                .disable::<bevy::winit::WinitPlugin>()
                .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
                .set(bevy::window::WindowPlugin {
                    // A primary `Window` entity with no native window behind
                    // it (no winit: it never gets a `RawHandleWrapper`, so
                    // the renderer never extracts it). Bevy 0.20's ray
                    // picking needs one even for image targets
                    // (`Location::is_in_viewport`, UPSTREAM U33): without it
                    // mesh picking hits nothing offscreen.
                    primary_window: Some(bevy::window::Window {
                        title: window.title.into(),
                        resolution: window.resolution.into(),
                        ..default()
                    }),
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(bevy::asset::AssetPlugin {
                    file_path: "examples/assets".into(),
                    ..default()
                }),
            ScheduleRunnerPlugin::run_loop(std::time::Duration::from_secs_f64(HEADLESS_FRAME_RATE)),
            bevy_markup::BevyMarkupPlugin,
            HarnessPlugin { headless: true },
        ));
    } else if headless {
        // Headless: no window, no winit (the frame loop is
        // `ScheduleRunnerPlugin`, so nothing waits for window focus). The
        // shared offscreen target comes from the harness plugin.
        app.add_plugins((
            TaskPoolPlugin::default(),
            FrameCountPlugin,
            TimePlugin,
            bevy::log::LogPlugin::default(),
            bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            },
            bevy::asset::AssetPlugin {
                // The example assets live beside the examples, not in ./assets.
                file_path: "examples/assets".into(),
                ..default()
            },
            bevy::text::TextPlugin,
            bevy::ui::UiPlugin,
            // Focus systems (`focus.rs`'s `:focus` states) read `InputFocus`.
            bevy::input_focus::InputFocusPlugin,
            bevy::input::InputPlugin,
        ));
        // Render-less hosts have no render app to register the image asset
        // and loader (the 9-slice frames' `url()`s still load, as bytes).
        // `init_asset` must run AFTER `AssetPlugin` (it needs the asset
        // server) and BEFORE the harness plugin: it replaces an existing
        // store, divorcing handles already issued (the harness's offscreen
        // target lands in the real store).
        app.init_asset::<bevy::image::Image>()
            .init_asset::<bevy::image::TextureAtlasLayout>()
            .register_asset_loader(bevy::image::ImageLoader::new(
                bevy::image::CompressedImageFormats::empty(),
            ));
        app.add_plugins((
            // Picking core: the pointer entity, the mocked `PointerInput`
            // events, the hover map.
            bevy::picking::PickingPlugin,
            PointerInputPlugin,
            bevy::picking::InteractionPlugin,
            ScheduleRunnerPlugin::run_loop(std::time::Duration::from_secs_f64(HEADLESS_FRAME_RATE)),
            // The widgets' button observers (a plugin group, so added on
            // its own); part of `DefaultPlugins` in the windowed
            // composition. Before `BevyMarkupPlugin`: with the `text-input`
            // feature the crate's own `TextInputPlugin` add is guarded by
            // `is_plugin_added` (lib.rs) — after the group it would panic
            // as a duplicate.
            bevy::ui_widgets::UiWidgetsPlugins,
            bevy_markup::BevyMarkupPlugin,
            HarnessPlugin { headless },
        ));
    } else {
        app.add_plugins((
            DefaultPlugins
                .set(bevy::asset::AssetPlugin {
                    file_path: "examples/assets".into(),
                    ..default()
                })
                .set(bevy::window::WindowPlugin {
                    primary_window: Some(Window {
                        title: window.title.into(),
                        resolution: bevy::window::WindowResolution::new(
                            window.resolution.0 as u32,
                            window.resolution.1 as u32,
                        ),
                        ..default()
                    }),
                    ..default()
                }),
            bevy_markup::BevyMarkupPlugin,
            HarnessPlugin { headless: false },
        ));
    }
    if window.shared_input {
        app.add_plugins(super::input::ExampleInputPlugin);
    }
    setup(&mut app);
    run_bounded(&mut app);
}

/// `--exit-after <frames>`: the example runs for a bounded number of
/// frames and exits — and a generic keyboard pattern is injected, so the
/// examples' input systems run their navigation and activation branches.
/// A bounded QA run needs no window, no editor and no shutdown logic; the
/// flag works with every composition.
fn run_bounded(app: &mut App) {
    // From the command line (`--exit-after N`) or the environment
    // (`BEVY_MARKUP_EXIT_AFTER=N`); the env form works under runners that
    // eat the command line (coverage's `cargo test --example` runs the
    // example through the test harness, which parses argv itself).
    let frames = std::env::args()
        .position(|arg| arg == "--exit-after")
        .and_then(|index| std::env::args().nth(index + 1))
        .and_then(|frames| frames.parse::<u32>().ok())
        .or_else(|| {
            std::env::var("BEVY_MARKUP_EXIT_AFTER")
                .ok()
                .and_then(|frames| frames.parse::<u32>().ok())
        });
    if let Some(frames) = frames {
        app.insert_resource(ExitAfter(frames));
        // After `keyboard_input_system` (which clears `just_pressed` each
        // frame): every `Update` system sees the injected press.
        app.add_systems(PreUpdate, exit_after.after(bevy::input::InputSystems));
    }
    app.run();
}

#[derive(Resource)]
pub struct ExitAfter(pub u32);

fn exit_after(
    frames: Res<bevy::diagnostic::FrameCount>,
    limit: Res<ExitAfter>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut exit: MessageWriter<AppExit>,
) {
    if frames.0 >= limit.0 {
        exit.write(AppExit::Success);
        return;
    }
    // One key pressed for a frame every 40 frames, cycling navigation,
    // activation and the language/menu keys — enough for the examples'
    // systems to run their branches without scripting each example.
    const KEYS: [KeyCode; 6] = [
        KeyCode::ArrowDown,
        KeyCode::ArrowRight,
        KeyCode::ArrowUp,
        KeyCode::Enter,
        KeyCode::ArrowLeft,
        KeyCode::Space,
    ];
    let step = frames.0 % 40;
    let key = KEYS[(frames.0 / 40) as usize % KEYS.len()];
    if step == 20 {
        keys.press(key);
    } else {
        keys.release(key);
    }
}

/// Adds [`bevy_mcp_harness::BevyMcpHarnessPlugin`] when the `dev-tools`
/// feature is on.
pub struct HarnessPlugin {
    /// The headless compositions (`--no-render` / `--offscreen`): target
    /// the harness's offscreen texture so `game/screenshot` and the mocked
    /// pointer work without a window. Read only with `dev-tools` (the
    /// plugin compiles away otherwise).
    #[cfg_attr(not(feature = "dev-tools"), allow(dead_code))]
    pub headless: bool,
}

#[cfg(feature = "dev-tools")]
impl Plugin for HarnessPlugin {
    fn build(&self, app: &mut App) {
        let config = bevy_mcp_harness::McpHarnessConfig {
            // The examples' UIs declare clicks via `data-on-click` element
            // signals, not `bevy_ui::Interaction` — without this hook
            // `game/ui` would never report the buttons `clickable`.
            clickable: Some(std::sync::Arc::new(markup_clickable)),
            // Both headless compositions render into (or at least target)
            // the harness's offscreen texture: the mocked pointer and the
            // `game/ui` dump need a camera target with a size.
            offscreen: if self.headless {
                bevy_mcp_harness::OffscreenMode::Owned(bevy_mcp_harness::DEFAULT_OFFSCREEN_SIZE)
            } else {
                bevy_mcp_harness::OffscreenMode::Windowed
            },
            ..bevy_mcp_harness::McpHarnessConfig::from_env()
        };
        app.add_plugins(bevy_mcp_harness::BevyMcpHarnessPlugin { config });
    }
}

#[cfg(not(feature = "dev-tools"))]
impl Plugin for HarnessPlugin {
    fn build(&self, _app: &mut App) {}
}

/// An element carries `data-on-*` hooks: clickable under bevy_markup's
/// convention.
#[cfg(feature = "dev-tools")]
fn markup_clickable(world: &World, entity: Entity) -> bool {
    world
        .get::<bevy_markup::prelude::ElementSignals>(entity)
        .is_some_and(|signals| !signals.0.is_empty())
}
