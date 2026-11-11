//! `<canvas>` (ADR 0041): intrinsic sizing on real headless layout, and the
//! binding of named sources to Bevy's `ViewportNode` / `ImageNode` on the
//! canvas's picture child (bug_0215), with
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

/// The canvas's picture: the child showing its source (bug_0215).
fn picture(ui: &mut TestUi, canvas: Entity) -> Option<Entity> {
    let world = ui.world_mut();
    let children = world.get::<Children>(canvas)?.to_vec();
    children
        .into_iter()
        .find(|&child| world.get::<CanvasPicture>(child).is_some())
}

/// The camera the canvas's picture views.
fn viewport(ui: &mut TestUi, canvas: Entity) -> Option<Entity> {
    let picture = picture(ui, canvas)?;
    ui.world_mut().get::<ViewportNode>(picture)?.camera
}

/// The image the canvas's picture shows.
fn shown_image(ui: &mut TestUi, canvas: Entity) -> Option<Handle<Image>> {
    let picture = picture(ui, canvas)?;
    Some(ui.world_mut().get::<ImageNode>(picture)?.image.clone())
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
    assert_eq!(viewport(&mut ui, live), Some(map));
    // On the picture, never on the element (which keeps its CSS images).
    assert!(ui.world_mut().get::<ViewportNode>(live).is_none());
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
    assert_eq!(viewport(&mut ui, copy), None);
    assert_eq!(shown_image(&mut ui, copy), Some(map_image));
    assert!(ui.world_mut().get::<ImageNode>(copy).is_none());

    assert_eq!(showing(&mut ui, still), CanvasShowing::Image(photo.clone()));
    let size = image_size(&mut ui, &photo);
    assert_eq!(
        (size.width, size.height),
        (16, 16),
        "an image source isn't resized"
    );

    assert_eq!(showing(&mut ui, none), CanvasShowing::Nothing);
    assert_eq!(picture(&mut ui, none), None);

    // Unregistering takes the camera off the viewport; the copy's image
    // goes too.
    ui.world_mut().resource_mut::<HtmlCanvases>().remove("map");
    ui.update(2);
    assert_eq!(showing(&mut ui, live), CanvasShowing::Nothing);
    assert_eq!(viewport(&mut ui, live), None);
    assert_eq!(shown_image(&mut ui, copy), None);
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

/// An oversized canvas isn't a candidate for the camera: a later canvas
/// that fits gets the viewport (bug_0217: the first in document order
/// owned it and showed nothing, the other only the stretched 8 × 8 image).
#[test]
fn an_oversized_canvas_leaves_the_camera_to_one_that_fits() {
    let mut ui = page(
        "canvas_oversized_first",
        "<canvas data-canvas='map' width='5000' height='100'></canvas>\
         <canvas data-canvas='map' width='60' height='30'></canvas>",
        "",
    );
    let (map, image) = camera(&mut ui, 8, 8);
    register(&mut ui, "map", CanvasSource::Camera(map));
    ui.update(4);
    let [big, small] = canvases(&mut ui)[..] else {
        panic!("two canvases");
    };
    assert_eq!(showing(&mut ui, big), CanvasShowing::Nothing);
    assert_eq!(showing(&mut ui, small), CanvasShowing::Viewport(map));
    let size = image_size(&mut ui, &image);
    assert_eq!((size.width, size.height), (60, 30));
}

/// A camera whose target image isn't in `Assets<Image>` (still loading,
/// or removed) isn't bound: Bevy's resize unwraps the image and panicked
/// (bug_0218, UPSTREAM U38). Once the image exists, the canvas binds.
#[test]
fn a_camera_without_its_image_waits_for_it() {
    let mut ui = page(
        "canvas_missing_target",
        "<canvas data-canvas='map' width='40' height='20'></canvas>",
        "",
    );
    let reserved = ui.world_mut().resource::<Assets<Image>>().reserve_handle();
    let map = ui
        .world_mut()
        .spawn(RenderTarget::Image(reserved.clone().into()))
        .id();
    register(&mut ui, "map", CanvasSource::Camera(map));
    ui.update(4);
    let canvas = canvases(&mut ui)[0];
    assert_eq!(showing(&mut ui, canvas), CanvasShowing::Nothing);

    let _ = ui.world_mut().resource_mut::<Assets<Image>>().insert(
        reserved.id(),
        Image::new_target_texture(8, 8, TextureFormat::Bgra8UnormSrgb, None),
    );
    ui.update(4);
    assert_eq!(showing(&mut ui, canvas), CanvasShowing::Viewport(map));
    assert_eq!(image_size(&mut ui, &reserved).width, 40);
}

/// A canvas with a CSS `border-image` keeps both: the frame on the element,
/// the source on its picture, through binding and in-place updates
/// (bug_0215: both were the element's `ImageNode`; binding replaced the
/// frame, and the next update the picture).
#[test]
fn a_framed_canvas_keeps_its_frame_and_its_picture() {
    let mut ui = page_with(
        "canvas_framed",
        "<canvas class='framed {{ class }}' data-canvas='photo' width='60' height='30'></canvas>",
        ".framed { border-image: url(frame.png) 4 stretch; border-width: 4px }",
        TemplateContext::default().with("class", "a"),
    );
    let photo = ui
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            16,
            16,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
    let frame: Handle<Image> = ui.load("frame.png");
    let canvas = canvases(&mut ui)[0];
    let element_image = |ui: &mut TestUi| {
        ui.world_mut()
            .get::<ImageNode>(canvas)
            .map(|node| node.image.clone())
    };
    assert_eq!(element_image(&mut ui), Some(frame.clone()));

    register(&mut ui, "photo", CanvasSource::Image(photo.clone()));
    ui.update(3);
    assert_eq!(
        showing(&mut ui, canvas),
        CanvasShowing::Image(photo.clone())
    );
    assert_eq!(
        element_image(&mut ui),
        Some(frame.clone()),
        "the frame stays"
    );
    assert_eq!(shown_image(&mut ui, canvas), Some(photo.clone()));

    // An in-place update re-inserts the element's frame, and keeps the
    // picture entity (a respawn would rebind a camera through a zero-size
    // frame).
    let before = picture(&mut ui, canvas);
    set(&mut ui, "class", "b");
    ui.settle();
    ui.update(2);
    assert_eq!(canvases(&mut ui)[0], canvas, "updated in place");
    assert_eq!(picture(&mut ui, canvas), before, "the same picture");
    assert_eq!(element_image(&mut ui), Some(frame));
    assert_eq!(
        shown_image(&mut ui, canvas),
        Some(photo),
        "the picture stays"
    );
    // The picture fills the padding box, inside the 4px border.
    let world = ui.world_mut();
    let picture = world.get::<Children>(canvas).unwrap()[0];
    let rect = node_rect(world, picture).unwrap();
    assert_eq!(rect.size(), Vec2::new(60.0, 30.0));
}

/// bug_0233: a `pointer-events: none` canvas lets clicks through to what
/// lies under it while it shows a source — its picture child ignores the
/// pointer like the element (an unpickable node blocks nothing; a node
/// without `Pickable` blocks).
#[test]
fn a_pointer_events_none_canvas_lets_clicks_through() {
    let mut ui = TestUi::with_pointer(
        "canvas_pointer_events",
        &[
            (
                "page.html",
                "<button id='under' data-on-click='hit'>under</button>\
                 <canvas class='over' data-canvas='photo' width='200' height='100'></canvas>",
            ),
            (
                "style.css",
                "button { width: 200px; height: 100px }\
                 .over { position: absolute; left: 0; top: 0; pointer-events: var(--events) }",
            ),
        ],
        VIEWPORT,
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::default().with("--events", "none"),
        column(),
    );
    ui.settle();
    let photo = ui
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            16,
            16,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
    register(&mut ui, "photo", CanvasSource::Image(photo.clone()));
    ui.update(3);
    let canvas = canvases(&mut ui)[0];
    assert_eq!(showing(&mut ui, canvas), CanvasShowing::Image(photo));
    let rect = node_rect(ui.app_mut().world(), canvas).unwrap();
    ui.take_signals();
    ui.click_at(rect.center());
    let names: Vec<String> = ui
        .update_collecting_signals(3)
        .into_iter()
        .map(|signal| signal.name.into_owned())
        .collect();
    assert_eq!(names, ["hit"], "the click reaches the button");
}

/// bug_0234: the picture takes the canvas's padding-box corners (its
/// `border-radius` less the border), so Bevy rounds the image or viewport
/// like the element; a restyle carries a new radius over.
#[test]
fn the_picture_follows_the_canvas_corners() {
    let mut ui = page_with(
        "canvas_radius",
        "<canvas class='{{ class }}' data-canvas='photo' width='120' height='120'></canvas>",
        ".round { border-radius: 50px; border-width: 4px }\
         .soft { border-radius: 10px 20px; border-width: 4px 2px }",
        TemplateContext::default().with("class", "round"),
    );
    let photo = ui
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            16,
            16,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
    register(&mut ui, "photo", CanvasSource::Image(photo));
    ui.update(3);
    let canvas = canvases(&mut ui)[0];
    let corners = |ui: &mut TestUi| {
        let picture = picture(ui, canvas).expect("a picture");
        let radius = ui.world_mut().get::<Node>(picture).unwrap().border_radius;
        [
            radius.top_left,
            radius.top_right,
            radius.bottom_right,
            radius.bottom_left,
        ]
        .map(|corner| (corner.x, corner.y))
    };
    let px = Val::Px;
    assert_eq!(corners(&mut ui), [(px(46.0), px(46.0)); 4]);
    set(&mut ui, "class", "soft");
    ui.settle();
    ui.update(3);
    // `10px 20px`: top-left and bottom-right 10, the others 20; borders
    // 4 px top and bottom, 2 px left and right.
    assert_eq!(
        corners(&mut ui),
        [
            (px(8.0), px(6.0)),
            (px(18.0), px(16.0)),
            (px(8.0), px(6.0)),
            (px(18.0), px(16.0)),
        ]
    );
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
    let (map, image) = camera(&mut ui, 8, 8);
    register(&mut ui, "map", CanvasSource::Camera(map));
    ui.update(3);
    let canvases = ui.world_mut().resource::<HtmlCanvases>();
    assert!(canvases.shown("map"));
    assert!(!canvases.shown("other"));
    let size = image_size(&mut ui, &image);
    assert_eq!((size.width, size.height), (300, 150));

    set(&mut ui, "class", "hidden");
    ui.settle();
    ui.update(2);
    assert!(!ui.world_mut().resource::<HtmlCanvases>().shown("map"));
    // A hidden canvas owns no camera: its image keeps its last size
    // (bug_0216: laid out at 0 × 0, it was resized to 1 × 1).
    let size = image_size(&mut ui, &image);
    assert_eq!((size.width, size.height), (300, 150));
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

/// The attributes parse by HTML's rules (entities decoded, digits up to
/// the first non-digit), and a canvas in a Markdown template is the same
/// box: raw HTML passes through.
#[test]
fn attributes_parse_like_html_in_html_and_markdown() {
    let html = "<canvas id='px' width='256px' height=' +128'></canvas>\
                <canvas id='entity' width='&#50;0' height='1&#48;'></canvas>";
    let mut ui = page("canvas_attributes", html, "");
    let lines: Vec<String> = ui
        .layout_dump()
        .lines()
        .skip(1)
        .map(str::to_owned)
        .collect();
    assert_eq!(
        lines,
        ["  canvas#px 0,0 256x128", "  canvas#entity 0,128 20x10"],
    );

    // Markdown: with the opening tag alone on its line the canvas is a
    // CommonMark HTML block, so the same box; written on one line it is
    // inline HTML in a paragraph — text, where a canvas has no box (and
    // shows nothing).
    let mut ui = TestUi::with_layout(
        "canvas_markdown",
        &[(
            "page.md",
            "# Map\n\n<canvas id='map' width='64' height='32'>\n</canvas>\n\n\
             Inline: <canvas id='inline' width='64' height='32'>fallback</canvas>\n",
        )],
        VIEWPORT,
    )
    .spawn("page.md", TemplateContext::default(), column());
    ui.settle();
    let dump = ui.layout_dump();
    assert!(
        dump.lines()
            .any(|line| line.trim_start().starts_with("canvas#map ") && line.ends_with(" 64x32")),
        "the block canvas is a 64×32 box:\n{dump}"
    );
    assert!(!dump.contains("canvas#inline"), "{dump}");
    assert!(!ui.dump().contains("fallback"), "{}", ui.dump());
}

/// Inside text a canvas has no box (only one directly in a container is a
/// node), and its fallback content never shows; a translation of a canvas
/// changes nothing either: there's no content to replace.
#[test]
fn canvas_content_never_shows() {
    let mut ui = TestUi::new(
        "canvas_content",
        &[
            (
                "page.html",
                "<p>Map: <canvas data-canvas='map'>fallback</canvas> end</p>\
                 <p>Spelled <CANVAS>upper fallback</CANVAS> in capitals (bug_0219)</p>\
                 <canvas id='own' data-l10n-id='canvas-label'>own fallback</canvas>",
            ),
            (
                "locales/en-US/main.ftl.ron",
                r#"(locale: "en-US", resources: ["ui.ftl"])"#,
            ),
            ("locales/en-US/ui.ftl", "canvas-label = translated fallback"),
        ],
    )
    .locale("locales/en-US/main.ftl.ron")
    .spawn("page.html", TemplateContext::default(), column());
    ui.settle();
    let dump = ui.dump();
    assert!(!dump.contains("fallback"), "no fallback text:\n{dump}");
    assert!(
        dump.contains("\"Map: \"") || dump.contains("Map: "),
        "{dump}"
    );
    assert!(
        dump.contains("canvas#own"),
        "the translated canvas is still a canvas:\n{dump}"
    );
    assert_eq!(
        ui.world_mut()
            .query::<&HtmlCanvas>()
            .iter(ui.world_mut())
            .count(),
        1,
        "only the canvas in a container is an element"
    );
}

/// The viewport follows visibility: hiding the canvas that has it hands it
/// to the next visible one (in document order), which then resizes the
/// camera's image; showing the first again doesn't take it back (the
/// visible owner keeps it).
#[test]
fn the_viewport_moves_to_a_visible_canvas() {
    let mut ui = page_with(
        "canvas_owner",
        "<canvas data-canvas='map' class='{{ first }}' width='200' height='100'></canvas>\
         <canvas data-canvas='map' width='60' height='30'></canvas>",
        ".hidden { display: none }",
        TemplateContext::default().with("first", "shown"),
    );
    let (map, image) = camera(&mut ui, 8, 8);
    register(&mut ui, "map", CanvasSource::Camera(map));
    ui.update(3);
    let [first, second] = canvases(&mut ui)[..] else {
        panic!("two canvases");
    };
    assert_eq!(showing(&mut ui, first), CanvasShowing::Viewport(map));
    assert_eq!(image_size(&mut ui, &image).width, 200);

    set(&mut ui, "first", "hidden");
    ui.settle();
    ui.update(2);
    assert_eq!(showing(&mut ui, second), CanvasShowing::Viewport(map));
    assert_eq!(showing(&mut ui, first), CanvasShowing::Image(image.clone()));
    let size = image_size(&mut ui, &image);
    assert_eq!(
        (size.width, size.height),
        (60, 30),
        "resized to the new owner"
    );

    set(&mut ui, "first", "shown");
    ui.settle();
    ui.update(2);
    assert_eq!(
        showing(&mut ui, second),
        CanvasShowing::Viewport(map),
        "a visible owner keeps the viewport"
    );
    assert_eq!(showing(&mut ui, first), CanvasShowing::Image(image));
}

/// Re-registering a name switches the canvas between a camera and an
/// image, both ways, and two names with two cameras each get a viewport.
#[test]
fn sources_switch_and_cameras_are_independent() {
    let mut ui = page(
        "canvas_switch",
        "<canvas data-canvas='a' width='40' height='20'></canvas>\
         <canvas data-canvas='b' width='30' height='10'></canvas>",
        "",
    );
    let (camera_a, image_a) = camera(&mut ui, 8, 8);
    let (camera_b, image_b) = camera(&mut ui, 8, 8);
    register(&mut ui, "a", CanvasSource::Camera(camera_a));
    register(&mut ui, "b", CanvasSource::Camera(camera_b));
    ui.update(3);
    let [a, b] = canvases(&mut ui)[..] else {
        panic!("two canvases");
    };
    assert_eq!(showing(&mut ui, a), CanvasShowing::Viewport(camera_a));
    assert_eq!(showing(&mut ui, b), CanvasShowing::Viewport(camera_b));
    assert_eq!(image_size(&mut ui, &image_a).width, 40);
    assert_eq!(image_size(&mut ui, &image_b).width, 30);

    // Camera → image: the viewport loses its camera, the image shows.
    register(&mut ui, "a", CanvasSource::Image(image_b.clone()));
    ui.update(2);
    assert_eq!(showing(&mut ui, a), CanvasShowing::Image(image_b.clone()));
    assert_eq!(viewport(&mut ui, a), None);
    assert_eq!(shown_image(&mut ui, a), Some(image_b));

    // And back: the image goes, the camera returns.
    register(&mut ui, "a", CanvasSource::Camera(camera_a));
    ui.update(2);
    assert_eq!(showing(&mut ui, a), CanvasShowing::Viewport(camera_a));
    assert_eq!(shown_image(&mut ui, a), None);
    assert_eq!(viewport(&mut ui, a), Some(camera_a));
}

/// A canvas is an element like any container: `HtmlElements` finds it by
/// id, its `data-on-click` makes it focusable, and its dataset carries the
/// name.
#[test]
fn a_canvas_is_an_ordinary_element() {
    let mut ui = page(
        "canvas_element",
        "<canvas id='map' class='framed' data-canvas='map' data-on-click='open-map'></canvas>",
        "",
    );
    let canvas = canvases(&mut ui)[0];
    let world = ui.world_mut();
    let element = world.get::<HtmlElement>(canvas).unwrap();
    assert_eq!(element.id.as_deref(), Some("map"));
    assert!(element.classes.iter().any(|class| class == "framed"));
    assert_eq!(element.data("canvas"), Some("map"));
    assert!(
        world.get::<Focusable>(canvas).is_some(),
        "data-on-click makes it focusable"
    );
    assert!(world.get::<ElementSignals>(canvas).is_some());
}

/// A canvas attribute as generated: a number (in one of HTML's spellings)
/// or junk that falls back to the default.
#[derive(Clone, Debug)]
enum Attribute {
    Number(u32, &'static str),
    Junk(&'static str),
}

impl Attribute {
    fn text(&self) -> String {
        match self {
            Attribute::Number(number, spelling) => spelling.replace('N', &number.to_string()),
            Attribute::Junk(junk) => junk.to_string(),
        }
    }

    fn value(&self, default: f32) -> f32 {
        match self {
            Attribute::Number(number, _) => *number as f32,
            Attribute::Junk(_) => default,
        }
    }
}

fn attribute() -> impl proptest::strategy::Strategy<Value = Attribute> {
    use proptest::prelude::*;
    prop_oneof![
        4 => (0u32..=300, prop::sample::select(vec!["N", " N", "Npx", "+N", "N.75"]))
            .prop_map(|(number, spelling)| Attribute::Number(number, spelling)),
        1 => prop::sample::select(vec!["", "x", "-4", "auto"]).prop_map(Attribute::Junk),
    ]
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(32))]

    /// The canvas's laid-out box is CSS's for a replaced element with the
    /// attributes' intrinsic size: declared sides win; one declared side
    /// keeps the attributes' ratio along the main axis; in a column, an
    /// undeclared width stretches to the container (`align-items:
    /// stretch`); a zero side has no ratio, so its auto side is 0.
    #[test]
    fn canvas_boxes_follow_css_sizing(
        width in attribute(),
        height in attribute(),
        css_width in proptest::option::of(1u32..=400),
        css_height in proptest::option::of(1u32..=400),
        row in proptest::bool::ANY,
    ) {
        static CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let case = CASE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut css = String::from(".row { flex-direction: row }");
        if let Some(px) = css_width {
            css += &format!(" canvas {{ width: {px}px }}");
        }
        if let Some(px) = css_height {
            css += &format!(" canvas {{ height: {px}px }}");
        }
        let canvas = format!(
            "<canvas width='{}' height='{}'></canvas>",
            width.text(),
            height.text()
        );
        let html = if row { format!("<div class='row'>{canvas}</div>") } else { canvas };
        let mut ui = page(&format!("canvas_sizing_{case}"), &html, &css);

        let (aw, ah) = (width.value(300.0), height.value(150.0));
        let ratio = (aw > 0.0 && ah > 0.0).then_some(aw / ah);
        let container = VIEWPORT.x as f32;
        let expected = match (css_width, css_height) {
            (None, None) => (aw, ah),
            (Some(w), Some(h)) => (w as f32, h as f32),
            (Some(w), None) => (w as f32, ratio.map_or(0.0, |r| w as f32 / r)),
            (None, Some(h)) if row => (ratio.map_or(0.0, |r| h as f32 * r), h as f32),
            (None, Some(h)) => (container, h as f32),
        };
        let entity = canvases(&mut ui)[0];
        let rect = node_rect(ui.app_mut().world(), entity).unwrap();
        proptest::prop_assert!(
            (rect.width() - expected.0).abs() <= 1.0 && (rect.height() - expected.1).abs() <= 1.0,
            "{html} with `{css}`: got {}x{}, expected {}x{}",
            rect.width(),
            rect.height(),
            expected.0,
            expected.1
        );
    }
}
