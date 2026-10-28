//! The benchmark harness shared by the criterion (`pipeline`) and divan
//! (`pipeline-divan`) targets: the same synthetic documents, stylesheets
//! and headless app composition, so the two harnesses measure the same
//! thing and their numbers corroborate each other (ADR 0018/0019).

// One shared harness for two bench targets: some helpers serve only one
// of them (the interaction benches are divan-only), so the module allows
// dead code instead of per-target cfgs.
#![allow(dead_code)]

use bevy::asset::AssetMetaCheck;
use bevy::asset::uuid::Uuid;
use bevy::camera::{ComputedCameraValues, RenderTarget, RenderTargetInfo};
use bevy::image::{CompressedImageFormats, ImageLoader};
use bevy::prelude::*;
use bevy::text::DEFAULT_FONT_DATA;
use bevy::window::{PrimaryWindow, WindowEvent, WindowRef};
use bevy_markup::html::{HtmlUi, TemplateContext};
use bevy_markup::prelude::*;
use std::marker::PhantomData;
use std::sync::LazyLock;

/// The document sizes 0009's method used.
pub(crate) const SIZES: [usize; 2] = [120, 600];

/// Viewport for the headless layout (the test harness's size).
pub(crate) const VIEWPORT: UVec2 = UVec2::new(1280, 800);

/// The frame bench's stylesheet: a kitchen sink of the CSS subset's
/// mapping arms (fonts, grid, flex, `var()`/`calc()`, a transition,
/// positioning, a `border-image` frame over the `frame.png` fixture) — the
/// cascade exercises them on every re-collect.
pub(crate) const CSS: &str = r#"
html {
  color: #ffffff;
  font-family: Spectral;
  font-size: 16px;
  border-image: url("frame.png") 4 fill stretch;
  border-width: 4px;
  padding: 10px;
}
header { display: flex; flex-direction: row; gap: 8px; padding: var(--pad) }
header > .chip { background-color: #1a1a1a; flex-grow: 1 }
.grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 4px;
}
.grid > .cell { background-color: #101010; padding: 2px }
.counter {
  color: #000000;
  font-size: calc(var(--base) + 4px);
  transition: color 0.2s linear;
  position: relative;
  top: calc(var(--pad) * 2);
}
.counter.active { color: #ffffff }
p.row { margin: 2px 0 }
"#;

/// The markup prelude for the `frame` workloads: a kitchen-sink shell —
/// a flex header, a grid, and a counter whose class and color are
/// templated (per-frame change with a transition attached: the advance
/// system's active path).
pub(crate) const SHELL_PRELUDE: &str = r#"<header><span class="chip">a</span><span class="chip">b</span></header>
<div class="grid">{% for i in range(end=8) %}<span class="cell">cell {{ i }}</span>{% endfor %}</div>
<p class="counter {% if n % 2 == 1 %}active{% endif %}" style="color: rgb({{ n % 255 }}, 0, 0)">{{ n }}</p>
"#;

/// The markup for the `frame` workloads: the shell around N paragraphs.
/// `counter_only`: the paragraphs are static (the `changing` workload —
/// one templated value in the shell); otherwise every paragraph templates
/// the context (the `rebuilding` workload).
pub(crate) fn shell_template(elements: usize, counter_only: bool) -> String {
    let line = if counter_only {
        "item {{ i }} of the list"
    } else {
        "item {{ i }} of {{ n }}"
    };
    format!(
        r#"{prelude}{{% for i in range(end={elements}) %}}<p class="row">{line}</p>
{{% endfor %}}"#,
        prelude = SHELL_PRELUDE
    )
}

/// The `localized` workload's markup: every paragraph localized, args
/// changing per frame.
pub(crate) fn localized_template(elements: usize) -> String {
    format!(
        "{{% for i in range(end={elements}) %}}\
         <p class=\"row\" data-l10n-id=\"row\" data-l10n-args='{{\"i\": {{{{ i }}}}, \"n\": {{{{ n }}}}}}'>fallback</p>\n\
         {{% endfor %}}"
    )
}

/// The `markdown` workload's template: a `.md` document (the Markdown
/// loader by extension) whose Fluent values are Markdown too.
pub(crate) fn markdown_template(elements: usize) -> String {
    format!(
        "{{% for i in range(end={elements}) %}}\
         ## Section {{{{ i }}}}\n\
         A *markdown* paragraph with `code`, some **emphasis** and a data-l10n-id line:\n\
         <p class=\"row\" data-l10n-id=\"row\" data-l10n-args='{{\"i\": {{{{ i }}}}, \"n\": {{{{ n }}}}}}'>fallback</p>\n\n\
         {{% endfor %}}"
    )
}

/// The locale manifest + messages for the localized and markdown
/// workloads; `markdown` opts the values into Markdown conversion.
pub(crate) fn locale_files(markdown: bool) -> Vec<(String, String)> {
    let markdown_line = if markdown {
        "\n    markdown: true,"
    } else {
        ""
    };
    let rows = (0..600)
        .map(|_| {
            format!(
                "row-n = Item {{ $i }} of **{{ $n }}** things.\nrow = Item {{ $i }} of the list.\n"
            )
        })
        .collect::<Vec<_>>()
        .concat();
    vec![
        (
            "locales/en-US/main.ftl.ron".into(),
            format!("(\n    locale: \"en-US\",\n    resources: [\"ui.ftl\"],{markdown_line}\n)\n"),
        ),
        ("locales/en-US/ui.ftl".into(), rows),
    ]
}

/// The context: the one value the workloads change.
pub(crate) fn context(n: i64) -> tera::Context {
    tera::Context::from_serialize(&serde_json::json!({ "n": n })).unwrap()
}

/// The rendered HTML for the parse benchmarks (Tera one-off).
pub(crate) fn rendered(source: &str) -> String {
    tera::Tera::one_off(source, &context(0), true).unwrap()
}

/// Fake font ids, served from Bevy's embedded font (the test harness's
/// trick): the cascade resolves families and weights against them, and
/// text measurement has real glyphs.
pub(crate) fn font(id: u128) -> Handle<Font> {
    Handle::Uuid(Uuid::from_u128(id), PhantomData)
}

pub(crate) static SPECTRAL: LazyLock<[Handle<Font>; 4]> =
    LazyLock::new(|| [font(1), font(2), font(3), font(4)]);

/// The frame bench's fixture frame image (the tests' 32×24 PNG), for the
/// `border-image`.
pub(crate) static FRAME_PNG: &[u8] = include_bytes!("../../tests/fixtures/frame.png");

/// A headless app over a temp asset root with `files`, laid out against a
/// fixed-size camera — the `tests/common` layout composition, without
/// schedule shuffling (a bench wants a pinned order) or the build
/// counters. Returns the app and the `HtmlUi` root.
pub(crate) fn bench_app(files: &[(&str, &str)]) -> (App, Entity) {
    bench_app_loading(files, "page.html")
}

/// [`bench_app`](Self), loading `template` as the UI's template (the
/// Markdown workload's is a `.md` file).
pub(crate) fn bench_app_loading(files: &[(&str, &str)], template: &str) -> (App, Entity) {
    let template = template.to_owned();
    let dir = std::env::temp_dir().join(format!(
        "bevy_markup-bench-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("frame.png"), FRAME_PNG).unwrap();
    for (path, contents) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: dir.to_string_lossy().into_owned(),
            meta_check: AssetMetaCheck::Never,
            ..default()
        },
        ImagePlugin::default(),
        BevyMarkupPlugin,
        // The layout harness's composition: `UiPlugin` lays roots out
        // against their camera's hand-set target size; its picking and
        // focus systems need the input, window, picking and texture-atlas
        // resources.
        bevy::input::InputPlugin,
        bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        },
        bevy::text::TextPlugin::default(),
        bevy::ui::UiPlugin::default(),
        bevy::picking::DefaultPickingPlugins,
        bevy::image::TextureAtlasPlugin,
    ));
    // bevy_render normally registers the image loader.
    app.register_asset_loader(ImageLoader::new(CompressedImageFormats::empty()));
    // The CSS's `font-family: Spectral` resolves to the fake faces, served
    // from the embedded font (real glyphs for measurement).
    let mut fonts = app.world_mut().resource_mut::<Assets<Font>>();
    for handle in SPECTRAL.iter() {
        fonts
            .insert(handle.id(), Font::from_bytes(DEFAULT_FONT_DATA.to_vec()))
            .expect("fresh id");
    }
    drop(fonts);
    app.world_mut().resource_mut::<FontFamilies>().insert(
        "Spectral",
        FontFaces::new(SPECTRAL[0].clone())
            .with_bold(SPECTRAL[1].clone())
            .with_italic(SPECTRAL[2].clone())
            .with_bold_italic(SPECTRAL[3].clone()),
    );

    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: VIEWPORT,
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        },
        RenderTarget::None { size: VIEWPORT },
        bevy::ui::IsDefaultUiCamera,
    ));

    // Spawn the UI; run to a steady state (the assets load and the first
    // build lands; a few frames past it for the layout to settle).
    let server = app.world().resource::<AssetServer>();
    let template = server.load(&template);
    let sheet = server.load("style.css");
    let locale = server.load("locales/en-US/main.ftl.ron");
    let root = app
        .world_mut()
        .spawn((
            HtmlUi::new(template),
            HtmlStylesheet(sheet),
            TemplateContext::new()
                .with("n", &0)
                .with("--pad", "8px")
                .with("--base", "12px"),
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .id();
    app.world_mut().resource_mut::<ActiveLocale>().set(locale);
    for _ in 0..60 {
        app.update();
    }
    (app, root)
}

/// The context mutation the frame workloads do per iteration.
pub(crate) fn set_n(app: &mut App, root: Entity, n: i64) {
    app.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &n);
}

/// Owned (String, String) pairs as borrowed (path, contents).
pub(crate) fn borrowed(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|(path, contents)| (path.as_str(), contents.as_str()))
        .collect()
}

/// The `selectors` workload's stylesheet (ADR 0034's trial): every kind
/// of selector the cascade supports — descendant and child combinators,
/// `+`/`~` next to the subject, the structural pseudo-classes, `data-*`
/// attribute tests, `:not()`, `:hover`, `:focus-within` — plus 40
/// near-miss rules per card class, so most rules fail part-way the way a
/// large real sheet's do. Only the grammar both matchers support.
pub(crate) fn selector_css() -> String {
    let mut css = String::from(
        r#"
html { color: #ffffff; font-size: 16px; padding: 4px }
.list > li:nth-child(odd) { background-color: #111111 }
.list > li:nth-child(3n+1) .label { color: #ff0000 }
.list li:first-child, .list li:last-child { padding: 2px }
.list li:nth-of-type(2n) > span.value { color: #00ff00 }
li[data-kind="a"] .label { color: #0000ff }
li[data-kind] + li .value { color: #eeeeee }
li.done ~ li.todo { margin: 1px 0 }
section.card p:not(.muted) { color: #dddddd }
section.card > header + p { font-size: 18px }
.card .list li:not(:hover) .label { color: #cccccc }
.card li:hover > .value { color: #ffff00 }
main section:nth-child(even) .list > li:last-of-type { padding: 4px }
span:empty { padding: 1px }
li:focus-within { background-color: #222222 }
"#,
    );
    for k in 0..40 {
        css.push_str(&format!(
            ".card.c{k} .list > li:nth-child({}) .label {{ color: #{:02x}0000 }}
             .card.c{k} li.todo + li[data-kind=\"b\"] > .value {{ padding: {}px }}\n",
            k % 5 + 1,
            k * 6,
            k % 4
        ));
    }
    css
}

/// The `selectors` workload's markup: cards of ten-item lists (~43
/// elements per card, `size / 40` cards), every value templated, so each
/// frame re-collects the whole document and runs the cascade per element.
pub(crate) fn selectors_template(size: usize) -> String {
    let cards = (size / 40).max(1);
    format!(
        r#"<main>{{% for s in range(end={cards}) %}}<section class="card c{{{{ s % 40 }}}}"><header>Card {{{{ s }}}}</header><p class="{{% if s % 3 == 0 %}}muted{{% endif %}}">{{{{ n }}}}</p><ul class="list">{{% for i in range(end=10) %}}<li class="{{% if i < 3 %}}done{{% else %}}todo{{% endif %}}" data-kind="{{% if i % 2 == 0 %}}a{{% else %}}b{{% endif %}}"><span class="label">L{{{{ i }}}}</span><span class="value">{{{{ n + i }}}}</span><span></span></li>{{% endfor %}}</ul></section>{{% endfor %}}</main>"#
    )
}

/// The frame workloads' file lists: `workload` ∈ idle/changing/
/// changing-nested/rebuilding —
/// the same documents at the given size (the `changing` one differs only in
/// the counter paragraph; `rebuilding` templates every paragraph).
pub(crate) fn frame_files(size: usize, workload: &str) -> Vec<(String, String)> {
    let page = match workload {
        "changing" => shell_template(size, true),
        // The same page inside one container: the changing counter makes
        // the container unclean, and its unchanged children must still
        // reuse (bug_0150: they re-collected — reuse worked only for
        // top-level siblings, which is all `changing` exercised).
        "changing-nested" => format!("<main>{}</main>", shell_template(size, true)),
        "selectors" => selectors_template(size),
        _ => shell_template(size, false),
    };
    let css = if workload == "selectors" {
        selector_css()
    } else {
        CSS.to_owned()
    };
    vec![
        ("page.html".to_owned(), page),
        ("style.css".to_owned(), css),
        (
            "locales/en-US/main.ftl.ron".to_owned(),
            r#"(locale: "en-US", resources: ["ui.ftl"])"#.to_owned(),
        ),
        (
            "locales/en-US/ui.ftl".to_owned(),
            "row = Item {{ $i }} of the list.\n".to_owned(),
        ),
    ]
}

/// The specialized workloads' file lists: the localized paragraphs or the
/// Markdown document at the given size, with their bundle.
pub(crate) fn specialized_files(size: usize, workload: &str) -> Vec<(String, String)> {
    let (path, page, markdown) = match workload {
        "markdown" => ("page.md", markdown_template(size), true),
        _ => ("page.html", localized_template(size), false),
    };
    let mut files = vec![
        (path.to_owned(), page),
        ("style.css".to_owned(), CSS.to_owned()),
    ];
    files.extend(locale_files(markdown));
    files
}

/// The workload's template path (`page.md` for the Markdown one).
pub(crate) fn template_path(workload: &str) -> &'static str {
    if workload == "markdown" {
        "page.md"
    } else {
        "page.html"
    }
}

/// The template contents for a workload at a size.
pub(crate) fn workload_page(size: usize, workload: &str) -> String {
    match workload {
        "changing" => shell_template(size, true),
        _ => shell_template(size, false),
    }
}

/// The interactive workloads' markup: N bound rows (`data-on-click` makes
/// them focusable and hover-bound); two carry tooltips, and the counter
/// paragraph too. For the hover and focus benches.
pub(crate) fn hover_template(elements: usize) -> String {
    format!(
        r#"{prelude}
{{% for i in range(end={elements}) %}}<p class="row" data-on-click="ping" data-tooltip="tip" data-l10n-args='{{"i": {{{{ i }}}}}}'>item {{{{ i }}}} of {{{{ n }}}}</p>
{{% endfor %}}"#,
        prelude = SHELL_PRELUDE
    )
}

/// The tooltips' template (the menu's shape: the Fluent message the
/// `data-tooltip` key names).
pub(crate) const TOOLTIP_TEMPLATE: &str = r#"<html class="tooltip"><p class="tooltip-text" data-l10n-id="{{ key }}">{{ key }}</p></html>"#;

/// [`bench_app_loading`], plus a primary window and the camera targeting
/// it — the `tests/common` `with_pointer` composition: real picking input
/// runs (`WindowEvent`s drive the full backend → `HoverMap` chain).
pub(crate) fn bench_app_pointer(files: &[(&str, &str)], template: &str) -> (App, Entity) {
    let (mut app, root) = bench_app_loading(files, template);
    app.world_mut().spawn((
        Window {
            resolution: bevy::window::WindowResolution::new(VIEWPORT.x, VIEWPORT.y),
            ..default()
        },
        PrimaryWindow,
    ));
    let mut target = app
        .world_mut()
        .query_filtered::<&mut RenderTarget, With<bevy::ui::IsDefaultUiCamera>>()
        .single_mut(app.world_mut())
        .expect("the bench camera");
    *target = RenderTarget::Window(WindowRef::Primary);
    (app, root)
}

/// Writes a `CursorMoved` window event (what winit would send) and runs
/// one frame — the pointer path the hover benches drive per iteration.
pub(crate) fn move_pointer(app: &mut App, position: Vec2) {
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world_mut())
        .expect("primary window");
    app.world_mut()
        .resource_mut::<Messages<WindowEvent>>()
        .write(WindowEvent::CursorMoved(bevy::window::CursorMoved {
            window,
            position,
            delta: None,
        }));
    app.update();
}
