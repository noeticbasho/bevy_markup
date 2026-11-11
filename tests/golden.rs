//! Golden images (AGENTS.md Testing TODO 7): a few reference scenes rendered
//! by the real Bevy renderer to an offscreen image, read back and compared
//! with `tests/golden/<scene>/expected.png` within a tolerance. The only
//! layer that sees real glyph rasterization, wrapping and 9-slice drawing.
//! A smoke check, not a spec: pixels depend on the driver and Bevy version.
//!
//! `#[ignore]`d (needs a GPU adapter; plain `cargo test` must not depend on
//! one). Run with `scripts/golden.sh`, which pins the software rasterizer
//! the references were recorded with. `BEVY_MARKUP_UPDATE_GOLDEN=1` rewrites the
//! references instead of comparing. On mismatch the actual image and a diff
//! (differing pixels red over a dimmed reference) are written under
//! `target/tmp/golden/` and their paths printed.
//!
//! Scene inputs live in `tests/golden/<scene>/`: `page.html`, `style.css`,
//! optionally `messages.ftl` (made the active en-US bundle) and `*.svg`
//! images. Each scene gets a copy of `tests/fixtures/frame.png` beside its
//! CSS. Text uses Bevy's embedded default font only (reproducible: no
//! system fonts). The `svg` scene needs the `svg` feature (the script
//! passes it).

use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::app::PluginsState;
use bevy::asset::{AssetMetaCheck, LoadState, RenderAssetUsages};
use bevy::camera::RenderTarget;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::render_resource::{
    Extent3d, PipelineCache, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::ExitCondition;
use bevy_markup::prelude::*;

const FRAME_PNG: &[u8] = include_bytes!("fixtures/frame.png");

// Tolerance, from measurements (2026-10, Bevy 0.19.1, Mesa 26.2.3): lavapipe
// renders these scenes bit-identically run to run (max channel delta 0 over
// 60+ runs). Against the lavapipe references, RADV (AMD iGPU) differs by at
// most 1 per channel and NVIDIA's driver by at most 5, with no pixel above
// 8 on either. So: a pixel differs above 8, and a scene may have ≤0.05%
// differing pixels (27–38 here) to absorb an isolated antialiasing flip on
// another driver or Mesa version. Still caught: one changed letter in a
// 15px word (53 pixels, 0.098%), a 1px padding change, a tile→stretch frame.
/// A pixel differs when any RGBA channel is off by more than this (0–255).
const CHANNEL_TOLERANCE: u8 = 8;
/// The scene fails when more than this fraction of its pixels differ.
const MAX_DIFF_FRACTION: f64 = 0.0005;

struct Scene {
    name: &'static str,
    size: UVec2,
    background: Color,
}

const SCENES: &[Scene] = &[
    Scene {
        name: "text",
        size: UVec2::new(320, 240),
        background: Color::srgb_u8(0x1c, 0x24, 0x30),
    },
    Scene {
        name: "frame",
        size: UVec2::new(280, 200),
        background: Color::srgb_u8(0x30, 0x30, 0x30),
    },
    Scene {
        name: "l10n",
        size: UVec2::new(300, 180),
        background: Color::srgb_u8(0x22, 0x1e, 0x1a),
    },
    Scene {
        name: "radius",
        size: UVec2::new(160, 190),
        background: Color::srgb_u8(0x30, 0x30, 0x30),
    },
    Scene {
        name: "background",
        size: UVec2::new(200, 300),
        background: Color::srgb_u8(0x30, 0x30, 0x30),
    },
    #[cfg(feature = "svg")]
    Scene {
        name: "svg",
        size: UVec2::new(260, 200),
        background: Color::srgb_u8(0x1c, 0x24, 0x30),
    },
];

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

#[derive(Resource, Default)]
struct Captures(Vec<(&'static str, Image)>);

#[derive(Resource, Default)]
struct Builds(usize);

#[test]
#[ignore = "needs a GPU adapter; run scripts/golden.sh"]
fn golden_scenes() {
    let started = Instant::now();
    let root = std::env::temp_dir().join(format!("bevy_markup-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut app = render_app(&root);
    let adapter = app.world().resource::<RenderAdapterInfo>();
    eprintln!(
        "golden: adapter {:?} ({:?}, driver {:?} {:?})",
        adapter.name, adapter.backend, adapter.driver, adapter.driver_info
    );

    let mut tracked: Vec<UntypedHandle> = Vec::new();
    for scene in SCENES {
        tracked.extend(spawn_scene(&mut app, &root, scene));
    }
    settle(&mut app, &tracked);
    let captures = capture(&mut app);
    std::fs::remove_dir_all(&root).ok();
    eprintln!("golden: rendered in {:.1?}", started.elapsed());

    let update = std::env::var_os("BEVY_MARKUP_UPDATE_GOLDEN").is_some_and(|v| v != "0");
    // Drop a previous run's actual/diff images so only this run's remain.
    std::fs::remove_dir_all(failure_dir()).ok();
    let mut failures = Vec::new();
    for (name, image) in captures {
        let actual = Rgba::from_image(image);
        let expected_path = golden_dir().join(name).join("expected.png");
        if update {
            actual.save(&expected_path);
            eprintln!("golden: updated {}", expected_path.display());
            continue;
        }
        if let Err(message) = compare(name, &expected_path, &actual) {
            failures.push(message);
        }
    }
    assert!(
        failures.is_empty(),
        "golden images differ:\n{}",
        failures.join("\n")
    );
}

/// Full Bevy (renderer, UI, text) without a window; the asset root is
/// `root`, filled by [`spawn_scene`]. Rendering runs inside `app.update()`
/// (no pipelined render thread), so the render world can be inspected.
fn render_app(root: &Path) -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin {
                file_path: root.to_string_lossy().into_owned(),
                meta_check: AssetMetaCheck::Never,
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            // Warnings and errors only; the test prints the adapter itself
            // (the software-adapter warning is expected here).
            .set(bevy::log::LogPlugin {
                level: bevy::log::Level::WARN,
                filter: format!("{},bevy_render::renderer=error", bevy::log::DEFAULT_FILTER),
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
            .disable::<bevy::audio::AudioPlugin>()
            .disable::<bevy::gilrs::GilrsPlugin>(),
    )
    .add_plugins(BevyMarkupPlugin)
    .init_resource::<Captures>()
    .init_resource::<Builds>()
    .add_observer(|_: On<HtmlUiBuilt>, mut builds: ResMut<Builds>| builds.0 += 1);
    // What `App::run` does before the first update (renderer init is async).
    while app.plugins_state() == PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
    app
}

/// Writes the scene's inputs under `root/<name>/` and spawns its camera
/// (rendering to an offscreen image) and `HtmlUi`; returns the handles
/// that must load before the scene is ready.
fn spawn_scene(app: &mut App, root: &Path, scene: &Scene) -> Vec<UntypedHandle> {
    let source = golden_dir().join(scene.name);
    let dir = root.join(scene.name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("frame.png"), FRAME_PNG).unwrap();
    for file in ["page.html", "style.css", "messages.ftl"] {
        if source.join(file).exists() {
            std::fs::copy(source.join(file), dir.join(file)).unwrap();
        }
    }
    for entry in std::fs::read_dir(&source).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "svg") {
            std::fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();
        }
    }

    let world = app.world_mut();
    let server = world.resource::<AssetServer>().clone();
    let template: Handle<HtmlTemplate> = server.load(format!("{}/page.html", scene.name));
    let sheet: Handle<Stylesheet> = server.load(format!("{}/style.css", scene.name));
    let mut tracked = vec![template.clone().untyped(), sheet.clone().untyped()];
    if dir.join("messages.ftl").exists() {
        std::fs::write(
            dir.join("main.ftl.ron"),
            r#"(locale: "en-US", resources: ["messages.ftl"])"#,
        )
        .unwrap();
        let bundle = server.load(format!("{}/main.ftl.ron", scene.name));
        tracked.push(bundle.clone().untyped());
        // One locale for the whole app: scenes without `data-l10n-id`
        // don't see it.
        world.insert_resource(ActiveLocale::new(bundle));
    }

    let mut target = Image::new_target_texture(
        scene.size.x,
        scene.size.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = world.resource_mut::<Assets<Image>>().add(target);
    let camera = world
        .spawn((
            Camera2d,
            Camera {
                clear_color: ClearColorConfig::Custom(scene.background),
                ..default()
            },
            RenderTarget::Image(target.clone().into()),
            SceneTarget(scene.name, target),
        ))
        .id();
    world.spawn((
        HtmlUi::new(template),
        HtmlStylesheet(sheet),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        UiTargetCamera(camera),
    ));
    tracked
}

#[derive(Component)]
struct SceneTarget(&'static str, Handle<Image>);

/// Updates until every tracked asset is loaded, every scene was built, no
/// render pipeline is still compiling (they compile asynchronously; an
/// unfinished one silently skips its draws, e.g. all 9-slice frames), every
/// inline image shows a raster of its box's size and no image changed (SVG
/// redraws run on tasks after layout, ADR 0038 §3), and all of that held
/// for a few frames.
fn settle(app: &mut App, tracked: &[UntypedHandle]) {
    let deadline = Instant::now() + std::time::Duration::from_secs(120);
    let mut stable = 0;
    let mut last_builds = 0;
    let mut image_events = app
        .world()
        .resource::<Messages<AssetEvent<Image>>>()
        .get_cursor();
    while Instant::now() < deadline {
        app.update();
        let images_changed = image_events
            .read(app.world().resource::<Messages<AssetEvent<Image>>>())
            .any(|event| {
                matches!(
                    event,
                    AssetEvent::Modified { .. } | AssetEvent::Added { .. }
                )
            });
        let rasters_sized = inline_rasters_sized(app.world_mut());
        let server = app.world().resource::<AssetServer>();
        for handle in tracked {
            if let Some(LoadState::Failed(error)) = server.get_load_state(handle.id()) {
                panic!("golden: asset {:?} failed to load: {error}", handle.path());
            }
        }
        let loaded = tracked
            .iter()
            .all(|h| server.is_loaded_with_dependencies(h.id()));
        let builds = app.world().resource::<Builds>().0;
        let compiling = app
            .sub_app(RenderApp)
            .world()
            .resource::<PipelineCache>()
            .waiting_pipelines()
            .next()
            .is_some();
        if loaded
            && !compiling
            && !images_changed
            && rasters_sized
            && builds >= SCENES.len()
            && builds == last_builds
        {
            stable += 1;
            if stable >= 10 {
                return;
            }
        } else {
            stable = 0;
        }
        last_builds = builds;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("golden: scenes never settled (assets loading, pipelines compiling or no build)");
}

/// Whether every inline image (`<img>`, inline `<svg>`) shows a raster of
/// its laid-out box's size: an SVG is redrawn at the size it's shown at
/// (scale factor 1 here), a raster image (`frame.png`) keeps its own size
/// and is skipped.
fn inline_rasters_sized(world: &mut World) -> bool {
    let mut boxes = world.query::<(&bevy::text::InlineBox, &bevy::ui::widget::InlineImage)>();
    let images = world.resource::<Assets<Image>>();
    boxes.iter(world).all(|(inline_box, image)| {
        let path = image.image.path().map(|path| path.to_string());
        let is_png = path.is_some_and(|path| path.ends_with(".png"));
        is_png
            || images.get(&image.image).is_some_and(|raster| {
                (raster.size().as_vec2() - inline_box.size)
                    .abs()
                    .max_element()
                    <= 1.0
            })
    })
}

/// Screenshots every scene's target and updates until all arrive.
fn capture(app: &mut App) -> Vec<(&'static str, Image)> {
    let world = app.world_mut();
    let targets: Vec<(&'static str, Handle<Image>)> = world
        .query::<&SceneTarget>()
        .iter(world)
        .map(|t| (t.0, t.1.clone()))
        .collect();
    for (name, target) in targets {
        world.spawn(Screenshot::image(target)).observe(
            move |captured: On<ScreenshotCaptured>, mut captures: ResMut<Captures>| {
                captures.0.push((name, captured.image.clone()));
            },
        );
    }
    for _ in 0..600 {
        app.update();
        if app.world().resource::<Captures>().0.len() == SCENES.len() {
            let mut captures = std::mem::take(&mut app.world_mut().resource_mut::<Captures>().0);
            captures.sort_by_key(|(name, _)| SCENES.iter().position(|s| s.name == *name));
            return captures;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("golden: screenshots never arrived");
}

/// Straight 8-bit RGBA pixels.
struct Rgba {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Rgba {
    fn from_image(image: Image) -> Self {
        let rgba = image
            .try_into_dynamic()
            .expect("8-bit screenshot")
            .to_rgba8();
        Self {
            width: rgba.width(),
            height: rgba.height(),
            pixels: rgba.into_raw(),
        }
    }

    fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let image = Image::from_buffer(
            &bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::Default,
            RenderAssetUsages::default(),
        )
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Some(Self::from_image(image))
    }

    fn save(&self, path: &Path) {
        let image = Image::new(
            Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.pixels.clone(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        image
            .try_into_dynamic()
            .unwrap()
            .save(path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}

/// Where failing runs leave actual and diff images.
fn failure_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("golden")
}

/// Compares `actual` with the reference at `expected_path`; on mismatch
/// writes the actual image and a diff to [`failure_dir`] and returns a
/// message naming them.
fn compare(name: &str, expected_path: &Path, actual: &Rgba) -> Result<(), String> {
    let out = failure_dir();
    let actual_path = out.join(format!("{name}.actual.png"));
    let Some(expected) = Rgba::load(expected_path) else {
        actual.save(&actual_path);
        return Err(format!(
            "{name}: no reference {} (actual: {}; BEVY_MARKUP_UPDATE_GOLDEN=1 records it)",
            expected_path.display(),
            actual_path.display()
        ));
    };
    if (expected.width, expected.height) != (actual.width, actual.height) {
        actual.save(&actual_path);
        return Err(format!(
            "{name}: size {}x{} vs reference {}x{} (actual: {})",
            actual.width,
            actual.height,
            expected.width,
            expected.height,
            actual_path.display()
        ));
    }

    let mut diff = Vec::with_capacity(expected.pixels.len());
    let (mut differing, mut worst) = (0usize, 0u8);
    for (e, a) in expected
        .pixels
        .chunks_exact(4)
        .zip(actual.pixels.chunks_exact(4))
    {
        let delta = e.iter().zip(a).map(|(e, a)| e.abs_diff(*a)).max().unwrap();
        worst = worst.max(delta);
        if delta > CHANNEL_TOLERANCE {
            differing += 1;
            diff.extend_from_slice(&[255, 0, 0, 255]);
        } else {
            let luma = ((e[0] as u32 * 3 + e[1] as u32 * 6 + e[2] as u32) / 10 / 3) as u8;
            diff.extend_from_slice(&[luma, luma, luma, 255]);
        }
    }
    let total = (actual.width * actual.height) as usize;
    let fraction = differing as f64 / total as f64;
    eprintln!(
        "golden: {name}: {differing}/{total} pixels differ by more than {CHANNEL_TOLERANCE} \
         (max channel delta {worst})"
    );
    if fraction <= MAX_DIFF_FRACTION {
        return Ok(());
    }
    let diff_path = out.join(format!("{name}.diff.png"));
    actual.save(&actual_path);
    Rgba {
        width: actual.width,
        height: actual.height,
        pixels: diff,
    }
    .save(&diff_path);
    Err(format!(
        "{name}: {differing}/{total} pixels ({:.3}%) differ by more than {CHANNEL_TOLERANCE} \
         per channel (allowed {:.3}%)\n  reference: {}\n  actual:    {}\n  diff:      {}",
        fraction * 100.0,
        MAX_DIFF_FRACTION * 100.0,
        expected_path.display(),
        actual_path.display(),
        diff_path.display()
    ))
}
