//! `<canvas>` (ADR 0041): intrinsic sizing on real headless layout, and the
//! binding of named sources to Bevy's `ViewportNode` / `ImageNode`, with
//! Bevy's own viewport resize running (no renderer: a camera is an entity
//! with a `RenderTarget`, which is all the resize reads).

mod common;

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::ui::widget::ViewportNode;
use bevy_markup::prelude::*;
use common::*;

const VIEWPORT: UVec2 = UVec2::new(640, 480);

fn column() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        width: Val::Percent(100.0),
        ..default()
    }
}

fn page(name: &str, html: &str, css: &str) -> TestUi {
    page_with(name, html, css, TemplateContext::default())
}

fn page_with(name: &str, html: &str, css: &str, context: TemplateContext) -> TestUi {
    let mut ui = TestUi::with_layout(name, &[("page.html", html), ("style.css", css)], VIEWPORT)
        .stylesheet("style.css")
        .spawn("page.html", context, column());
    ui.settle();
    ui
}

/// The canvases below the root, in document order.
fn canvases(ui: &mut TestUi) -> Vec<Entity> {
    let world = ui.world_mut();
    let mut query = world.query_filtered::<Entity, With<HtmlCanvas>>();
    let mut found: Vec<Entity> = query.iter(world).collect();
    found.sort_by_key(|&entity| {
        let rect = node_rect(world, entity).unwrap();
        (rect.min.y as i32, rect.min.x as i32)
    });
    found
}

fn showing(ui: &mut TestUi, canvas: Entity) -> CanvasShowing {
    ui.world_mut()
        .get::<HtmlCanvas>(canvas)
        .unwrap()
        .showing
        .clone()
}

/// A render-target image and an entity standing in for its camera.
fn camera(ui: &mut TestUi, width: u32, height: u32) -> (Entity, Handle<Image>) {
    let world = ui.world_mut();
    let image = world
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            width,
            height,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
    let camera = world.spawn(RenderTarget::Image(image.clone().into())).id();
    (camera, image)
}

fn image_size(ui: &mut TestUi, image: &Handle<Image>) -> Extent3d {
    ui.world_mut()
        .resource::<Assets<Image>>()
        .get(image)
        .unwrap()
        .texture_descriptor
        .size
}

/// Sets a context variable on the root (re-renders the template).
fn set<T: serde::Serialize + ?Sized>(ui: &mut TestUi, key: &str, value: &T) {
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert(key.to_owned(), value);
}

fn register(ui: &mut TestUi, name: &str, source: CanvasSource) {
    ui.world_mut()
        .resource_mut::<HtmlCanvases>()
        .insert(name, source);
}

/// HTML's intrinsic size: the attributes (CSS px, default 300 × 150), under
/// CSS; one declared side keeps the attributes' ratio. The content is
/// fallback, never shown.
#[test]
fn canvases_size_like_html() {
    let mut ui = page(
        "canvas_sizes",
        "<canvas id='plain'>fallback <b>text</b></canvas>\
         <canvas id='attrs' width='256' height='128'></canvas>\
         <canvas id='wide' width='256' height='128'></canvas>\
         <canvas id='both' width='256' height='128'></canvas>\
         <canvas id='bad' width='x' height='-3'></canvas>",
        "#wide { width: 128px } #both { width: 50px; height: 40px }",
    );
    let actual = ui.layout_dump();
    let expected = "\
html-ui 0,0 640x664
  canvas#plain 0,0 300x150
  canvas#attrs 0,150 256x128
  canvas#wide 0,278 128x64
  canvas#both 0,342 50x40
  canvas#bad 0,382 300x150";
    // (The root's height is the stack's: 150+128+64+40+150 = 532, plus
    // nothing else; checked line by line below for the canvases.)
    let canvases: Vec<&str> = actual.lines().skip(1).collect();
    let expected: Vec<&str> = expected.lines().skip(1).collect();
    assert_eq!(canvases, expected, "\n--- actual ---\n{actual}");
    assert!(
        !ui.dump().contains("fallback"),
        "a canvas's content is never built:\n{}",
        ui.dump()
    );
}

/// A camera source gets Bevy's `ViewportNode`, and Bevy resizes the
/// camera's image to the box; a second canvas naming the same camera shows
/// its image stretched; an image source is stretched and never resized; an
/// unregistered name shows nothing.
#[test]
fn sources_bind_to_viewports_and_images() {
    let mut ui = page(
        "canvas_sources",
        "<canvas id='live' data-canvas='map' width='200' height='100'></canvas>\
         <canvas id='copy' data-canvas='map' width='50' height='50'></canvas>\
         <canvas id='still' data-canvas='photo' width='60' height='30'></canvas>\
         <canvas id='none' data-canvas='missing'></canvas>",
        "",
    );
    let (map, map_image) = camera(&mut ui, 8, 8);
    let photo = ui
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            16,
            16,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
    register(&mut ui, "map", CanvasSource::Camera(map));
    register(&mut ui, "photo", CanvasSource::Image(photo.clone()));
    let builds = ui.builds();
    ui.update(3);
    assert_eq!(ui.builds(), builds, "registering sources rebuilds nothing");

    let [live, copy, still, none] = canvases(&mut ui)[..] else {
        panic!("four canvases");
    };
    assert_eq!(showing(&mut ui, live), CanvasShowing::Viewport(map));
    assert_eq!(
        ui.world_mut().get::<ViewportNode>(live).unwrap().camera,
        Some(map)
    );
    let size = image_size(&mut ui, &map_image);
    assert_eq!(
        (size.width, size.height),
        (200, 100),
        "Bevy resized the target"
    );

    assert_eq!(
        showing(&mut ui, copy),
        CanvasShowing::Image(map_image.clone())
    );
    assert!(ui.world_mut().get::<ViewportNode>(copy).is_none());
    assert_eq!(
        ui.world_mut().get::<ImageNode>(copy).unwrap().image,
        map_image
    );

    assert_eq!(showing(&mut ui, still), CanvasShowing::Image(photo.clone()));
    let size = image_size(&mut ui, &photo);
    assert_eq!(
        (size.width, size.height),
        (16, 16),
        "an image source isn't resized"
    );

    assert_eq!(showing(&mut ui, none), CanvasShowing::Nothing);
    assert!(ui.world_mut().get::<ImageNode>(none).is_none());

    // Unregistering takes the camera off the viewport; the copy's image
    // goes too.
    ui.world_mut().resource_mut::<HtmlCanvases>().remove("map");
    ui.update(2);
    assert_eq!(showing(&mut ui, live), CanvasShowing::Nothing);
    assert_eq!(
        ui.world_mut().get::<ViewportNode>(live).unwrap().camera,
        None
    );
    assert!(ui.world_mut().get::<ImageNode>(copy).is_none());
    assert_eq!(ui.builds(), builds, "unregistering rebuilds nothing");
}

/// A restyle or content update keeps the binding (the build doesn't own
/// `ViewportNode` or the canvas's `ImageNode`), and a resize follows CSS.
#[test]
fn bindings_survive_updates() {
    let mut ui = page_with(
        "canvas_updates",
        "<canvas data-canvas='map' style='width: {{ w }}px; height: 50px'></canvas>",
        "",
        TemplateContext::default().with("w", &300),
    );
    let (map, image) = camera(&mut ui, 8, 8);
    register(&mut ui, "map", CanvasSource::Camera(map));
    ui.update(3);
    let canvas = canvases(&mut ui)[0];
    assert_eq!(
        image_size(&mut ui, &image).width,
        300,
        "no `w`: the default"
    );

    set(&mut ui, "w", &120);
    ui.settle();
    ui.update(2);
    assert_eq!(canvases(&mut ui)[0], canvas, "the canvas is kept");
    assert_eq!(showing(&mut ui, canvas), CanvasShowing::Viewport(map));
    let size = image_size(&mut ui, &image);
    assert_eq!((size.width, size.height), (120, 50));
}

/// A canvas larger than `MAX_CANVAS_SIDE` gets no camera: Bevy's resize
/// never sees it, so the target keeps its size (a 100 000 px canvas would
/// otherwise allocate ~40 GB). Back under the limit, it binds again.
#[test]
fn oversized_canvases_get_no_camera() {
    let mut ui = page_with(
        "canvas_oversized",
        "<canvas data-canvas='map' style='width: {{ w }}px; height: 10px'></canvas>",
        "",
        TemplateContext::default().with("w", &100_000),
    );
    let (map, image) = camera(&mut ui, 8, 8);
    register(&mut ui, "map", CanvasSource::Camera(map));
    ui.update(3);
    let canvas = canvases(&mut ui)[0];
    assert_eq!(showing(&mut ui, canvas), CanvasShowing::Nothing);
    let size = image_size(&mut ui, &image);
    assert_eq!((size.width, size.height), (8, 8), "never resized");

    set(&mut ui, "w", &(MAX_CANVAS_SIDE as u32));
    ui.settle();
    ui.update(2);
    assert_eq!(showing(&mut ui, canvas), CanvasShowing::Viewport(map));
    assert_eq!(image_size(&mut ui, &image).width, MAX_CANVAS_SIDE as u32);
}

/// `HtmlCanvases::shown` reports whether a canvas with the name is laid out
/// at a non-zero size: the app's switch for its camera.
#[test]
fn shown_reports_visible_canvases() {
    let mut ui = page_with(
        "canvas_shown",
        "<canvas data-canvas='map' class='{{ class }}'></canvas>",
        ".hidden { display: none }",
        TemplateContext::default().with("class", "shown"),
    );
    let (map, _) = camera(&mut ui, 8, 8);
    register(&mut ui, "map", CanvasSource::Camera(map));
    ui.update(2);
    let canvases = ui.world_mut().resource::<HtmlCanvases>();
    assert!(canvases.shown("map"));
    assert!(!canvases.shown("other"));

    set(&mut ui, "class", "hidden");
    ui.settle();
    ui.update(2);
    assert!(!ui.world_mut().resource::<HtmlCanvases>().shown("map"));
}

/// What the stub backend saw: the forwarded pointer's positions in the
/// camera's image, and the clicks its scene entity received.
#[derive(Resource, Default)]
struct SceneLog {
    positions: Vec<Vec2>,
    clicks: usize,
}

/// The pointer over a camera canvas reaches the camera's scene (Bevy's
/// `viewport_picking` on the `ViewportNode` the canvas got): a click on
/// the canvas's center arrives at the image's center, and a picking backend
/// reporting a hit there turns it into a click on the scene's entity. A
/// stub backend stands in for mesh picking (no renderer here).
#[test]
fn pointer_input_reaches_the_cameras_scene() {
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo};
    use bevy::picking::PickingSystems;
    use bevy::picking::backend::{HitData, PointerHits};
    use bevy::picking::events::PointerClick;
    use bevy::picking::pointer::{PointerId, PointerLocation};

    let mut ui = TestUi::with_pointer(
        "canvas_pointer",
        &[(
            "page.html",
            "<canvas data-canvas='scene' width='200' height='100'></canvas>",
        )],
        VIEWPORT,
    )
    .spawn("page.html", TemplateContext::default(), column());
    ui.settle();

    let (camera, image) = {
        let world = ui.world_mut();
        let image = world
            .resource_mut::<Assets<Image>>()
            .add(Image::new_target_texture(
                200,
                100,
                TextureFormat::Bgra8UnormSrgb,
                None,
            ));
        // A camera rendering into the image: its viewport size is what the
        // forwarded position scales to (set by hand: no renderer).
        let camera = world
            .spawn((
                Camera {
                    order: -1,
                    computed: ComputedCameraValues {
                        target_info: Some(RenderTargetInfo {
                            physical_size: UVec2::new(200, 100),
                            scale_factor: 1.0,
                        }),
                        ..default()
                    },
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
            ))
            .id();
        (camera, image)
    };
    let scene = ui
        .world_mut()
        .spawn_empty()
        .observe(|_: On<PointerClick>, mut log: ResMut<SceneLog>| log.clicks += 1)
        .id();
    ui.app_mut().init_resource::<SceneLog>().add_systems(
        PreUpdate,
        (move |pointers: Query<(&PointerId, &PointerLocation)>,
               mut hits: MessageWriter<PointerHits>,
               mut log: ResMut<SceneLog>| {
            for (&pointer, location) in &pointers {
                let Some(location) = location.location() else {
                    continue;
                };
                if location.target
                    != bevy::camera::NormalizedRenderTarget::Image(image.clone().into())
                {
                    continue;
                }
                log.positions.push(location.position);
                hits.write(PointerHits::new(
                    pointer,
                    vec![(scene, HitData::new(camera, 0.0, None, None))],
                    0.0,
                ));
            }
        })
        .in_set(PickingSystems::Backend),
    );
    register(&mut ui, "scene", CanvasSource::Camera(camera));
    ui.update(3);

    let canvas = canvases(&mut ui)[0];
    assert_eq!(showing(&mut ui, canvas), CanvasShowing::Viewport(camera));
    let rect = node_rect(ui.app_mut().world(), canvas).unwrap();
    ui.click_at(rect.center());
    ui.update(3);

    let log = ui.world_mut().resource::<SceneLog>();
    assert!(
        log.positions.contains(&Vec2::new(100.0, 50.0)),
        "the canvas's center is the image's: {:?}",
        log.positions
    );
    assert_eq!(
        log.clicks, 1,
        "the scene entity was clicked through the canvas"
    );
}
