//! Test vectors: HTML + CSS + Fluent + Tera inputs → the Bevy world they
//! produce, checked headlessly (no window, no renderer).
//!
//! Each test writes its input files into a fresh asset root, runs a minimal
//! app (`MinimalPlugins` + assets + images + `BevyMarkupPlugin`) until every asset
//! is loaded and the UI is built, then compares a text dump of the `HtmlUi`
//! entity's subtree against the expected mapping:
//!
//! ```text
//! <entity>  := <label> [border=t,r,b,l] [padding=…] [margin=…] [gap=N] [bg=#hex] [slice=file t,r,b,l mode]
//!              (then, for `Text` nodes, one indented line per run: "text" face size color)
//! <label>   := tag#id.class… | - (no HtmlElement) | html-ui (the root)
//! face      := serif | serif-bold | serif-italic | serif-bold-italic | mono | default
//! ```

mod common;
use common::*;

use bevy::asset::AssetEvent;
use bevy::prelude::*;
use bevy_markup::Skeleton;
use bevy_markup::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Vectors
// ---------------------------------------------------------------------------

/// Tera: variables (autoescaped), loops, conditionals → text; tags → blocks,
/// containers and anonymous loose text; `li` bullets; `pre` keeps whitespace.
#[test]
fn tera_template_structure() {
    let mut ui = TestUi::new(
        "tera",
        &[
            (
                "page.html",
                r#"<h1>Hello {{ name }}</h1>
<ul>{% for item in items %}<li>{{ item }}</li>{% endfor %}</ul>
{% if items | length > 1 %}<p>many</p>{% else %}<p>few</p>{% endif %}
loose   text
<pre>
a
  b</pre>"#,
            ),
            (
                "style.css",
                "html { color: #ffffff; font-family: Spectral; font-size: 20px }",
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new()
            .with("name", "<Ada>")
            .with("items", &["torch", "rope"]),
        Node::default(),
    );
    ui.settle().assert_dump(
        r#"
html-ui
  h1
    "Hello <Ada>" serif 20px #ffffff
  ul
    li margin=0,0,0,12
      "• " serif 20px #ffffff
      "torch" serif 20px #ffffff
    li margin=0,0,0,12
      "• " serif 20px #ffffff
      "rope" serif 20px #ffffff
  p
    "many" serif 20px #ffffff
  -
    "loose text" serif 20px #ffffff
  pre padding=8,8,8,8 overflow=Clip,Visible
    "a\n  b" serif 20px #ffffff
"#,
    );
}

/// System font families (any `FontSource` but a handle): one registered
/// source covers every face, and bold/italic are *requested* (TextFont
/// weight/style) for the system to pick. A file family (`Mono`, regular
/// face only) never gets a weight or style: its faces are fixed files.
#[test]
fn system_font_families_request_weight_and_style() {
    let mut ui = TestUi::new(
        "system-fonts",
        &[
            (
                "page.html",
                "<p>plain</p><p><b>strong</b></p><p><i>slanted</i></p><p><b><i>both</i></b></p>\
                 <p class=\"m\"><b><i>mono</i></b></p>",
            ),
            (
                "style.css",
                "html { font-family: Body; color: #ffffff } b { font-weight: bold } \
                 i { font-style: italic } .m { font-family: monospace }",
            ),
        ],
    );
    ui.world_mut()
        .resource_mut::<FontFamilies>()
        .insert("Body", FontFaces::new(FontSource::serif()));
    let mut ui =
        ui.stylesheet("style.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "plain" Serif 16px #ffffff
  p
    "strong" Serif 16px #ffffff bold
  p
    "slanted" Serif 16px #ffffff italic
  p
    "both" Serif 16px #ffffff bold italic
  p.m
    "mono" mono 16px #ffffff
"#,
    );
}

/// Inline elements: `background` → a `TextBackgroundColor` on the span,
/// `text-decoration` → `Strikethrough` (+ declared color), and `<img>` → an
/// `InlineBox`/`InlineImage` flowing in the block's text (the fixture image
/// is 32x24; a CSS `width` keeps the aspect ratio). A restyle that drops
/// the background takes the component back in place.
#[test]
fn inline_backgrounds_decorations_and_images() {
    let ui = TestUi::new(
        "inline-runs",
        &[
            (
                "page.html",
                "<p>Plain <span class=\"hl\">highlight</span> and \
                 <img class=\"small\" src=\"frame.png\"> then <del>struck</del>.</p>",
            ),
            (
                "style.css",
                "html { color: #ffffff } .hl { background-color: #ffee00 } \
                 img.small { width: 16px } del { text-decoration: line-through #ff4040 }",
            ),
        ],
    );
    let mut ui =
        ui.stylesheet("style.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "Plain " default 16px #ffffff
    "highlight" default 16px #ffffff bg=#ffee00
    " and " default 16px #ffffff
    "frame.png" img 16x12
    " then " default 16px #ffffff
    "struck" default 16px #ffffff line-through=#ff4040
    "." default 16px #ffffff
"#,
    );
    // A restyle without the declarations (the sheet failed): the span
    // components go, the entities stay — and the image, whose CSS width
    // was in the sheet, falls back to its intrinsic size.
    let plain = ui.load::<Stylesheet>("plain.css");
    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(plain);
    // The swap rebuilds once the load *fails* (the missing file's error
    // is the test's vehicle); under full-suite load the io task can
    // starve past any fixed frame count, so wait for the rebuild like
    // settle does everywhere else (bug_0129's flake).
    ui.settle();
    ui.assert_dump(
        r#"
html-ui
  p
    "Plain highlight and " default 16px #ffffff
    "frame.png" img 32x24
    " then struck." default 16px #ffffff
"#,
    );
}

/// CSS: specificity (id > class > type, compound counts), inheritance into
/// inline elements, font faces from weight/style, missing-face fallback,
/// generic family, em/rem/% sizes, unknown family keeps the inherited one.
#[test]
fn css_cascade_and_fonts() {
    let mut ui = TestUi::from_vector("cascade");
    ui.settle().assert_dump(
        r#"
html-ui
  p#lead.note
    "Lead " serif 30px #00ff00
    "bold " serif-bold 30px #00ff00
    "both" serif-bold-italic 30px #00ff00
  p.note
    "Note " serif 30px #ff0000
    "mono " mono 30px #888888
    "bold?" mono 30px #888888
  p.a.b
    "Two classes" default 20px #222222
  h2
    "Heading " serif-bold 40px #ffffff
    "small" serif-bold 20px #ffffff
"#,
    );
}

/// Turning localization off (`ActiveLocale(None)`) brings back the
/// elements' own content: no stale translation stays on screen.
#[test]
fn turning_localization_off_restores_own_content() {
    let mut ui = TestUi::new(
        "locale-off",
        &[
            ("page.html", r#"<p data-l10n-id="title">Own title</p>"#),
            ("style.css", "html { color: #ffffff; font-size: 20px }"),
            (
                "locales/en-US/main.ftl.ron",
                r#"(locale: "en-US", resources: ["ui.ftl"])"#,
            ),
            ("locales/en-US/ui.ftl", "title = Translated title"),
        ],
    )
    .stylesheet("style.css")
    .locale("locales/en-US/main.ftl.ron")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle()
        .assert_dump("\nhtml-ui\n  p\n    \"Translated title\" default 20px #ffffff\n");

    ui.world_mut().resource_mut::<ActiveLocale>().0 = None;
    ui.settle()
        .assert_dump("\nhtml-ui\n  p\n    \"Own title\" default 20px #ffffff\n");
}

/// bug_0087: the loaders read includes and Fluent resources themselves
/// (`read_asset_bytes` skips Bevy's unapproved-path check), so they must
/// honor `AssetPlugin::unapproved_path_mode`: only `Allow` reads outside the
/// asset root; `Deny` refuses like `Forbid` (an include or a resource can't
/// ask for `override_unapproved`). They enforced `Forbid` whatever the app
/// chose.
#[test]
fn includes_and_fluent_resources_honor_the_unapproved_path_mode() {
    use bevy::asset::{LoadState, UnapprovedPathMode};
    let id = std::process::id();
    let (outside_html, outside_ftl) = (
        format!("bm-unapproved-{id}.html"),
        format!("bm-unapproved-{id}.ftl"),
    );
    let page = format!(r#"<p data-l10n-id="title">own</p>{{% include "../{outside_html}" %}}"#);
    let manifest = format!(r#"(locale: "en-US", resources: ["../../../{outside_ftl}"])"#);
    for (mode, name, allowed) in [
        (UnapprovedPathMode::Allow, "Allow", true),
        (UnapprovedPathMode::Deny, "Deny", false),
        (UnapprovedPathMode::Forbid, "Forbid", false),
    ] {
        let mut ui = TestUi::with_unapproved_path_mode(
            "unapproved",
            &[
                ("page.html", page.as_str()),
                (&format!("../{outside_html}"), "<p>outside</p>"),
                ("locales/en-US/main.ftl.ron", manifest.as_str()),
                (&format!("../{outside_ftl}"), "title = translated"),
            ],
            mode,
        );
        let template: Handle<HtmlTemplate> = ui.load("page.html");
        let bundle: Handle<LocaleBundle> = ui.load("locales/en-US/main.ftl.ron");
        ui.update(60);
        let server = ui.world_mut().resource::<AssetServer>();
        let (template_state, bundle_state) =
            (server.load_state(&template), server.load_state(&bundle));
        assert_eq!(
            matches!(template_state, LoadState::Loaded),
            allowed,
            "{name}: the include outside the root: {template_state:?}"
        );
        assert_eq!(
            matches!(bundle_state, LoadState::Loaded),
            allowed,
            "{name}: the resource outside the root: {bundle_state:?}"
        );
        assert_eq!(
            matches!(template_state, LoadState::Failed(_)),
            !allowed,
            "{name}: refused, not pending"
        );
    }
}

/// Fluent: `data-l10n-id` replaces content; args (numbers for plurals,
/// strings HTML-escaped); markup in translations is parsed and styled; a
/// missing message falls back to the element's own content.
#[test]
fn fluent_localization() {
    let mut ui = TestUi::new(
        "fluent",
        &[
            (
                "page.html",
                r#"<h1 data-l10n-id="title">fallback</h1>
<p data-l10n-id="greet" data-l10n-args='{"name": "{{ name }}"}'></p>
<p data-l10n-id="coins" data-l10n-args='{"n": {{ n }}}'></p>
<p data-l10n-id="missing">Own <b>content</b></p>"#,
            ),
            (
                "style.css",
                r#"
html { color: #ffffff; font-family: Spectral; font-size: 20px }
b { font-weight: bold; color: #ffcc00 }
em { font-style: italic }
"#,
            ),
            (
                "locales/en-US/main.ftl.ron",
                r#"(locale: "en-US", resources: ["ui.ftl"])"#,
            ),
            (
                "locales/en-US/ui.ftl",
                r#"
title = Inventory
greet = Welcome, <b>{ $name }</b>!
coins =
    { $n ->
        [one] <em>One</em> coin
       *[other] { $n } coins
    }
"#,
            ),
        ],
    )
    .stylesheet("style.css")
    .locale("locales/en-US/main.ftl.ron")
    .spawn(
        "page.html",
        TemplateContext::new()
            .with("name", "Ada <The Brave>")
            .with("n", &1),
        Node::default(),
    );
    ui.settle().assert_dump(
        r#"
html-ui
  h1
    "Inventory" serif 20px #ffffff
  p
    "Welcome, " serif 20px #ffffff
    "Ada <The Brave>" serif-bold 20px #ffcc00
    "!" serif 20px #ffffff
  p
    "One" serif-italic 20px #ffffff
    " coin" serif 20px #ffffff
  p
    "Own " serif 20px #ffffff
    "content" serif-bold 20px #ffcc00
"#,
    );
}

/// Deliberate differences from `@fluent/dom` (which the Fluent oracle
/// vectors leave out): translation markup isn't sanitized. Nested elements
/// keep their styling (fluent-dom flattens them to text), `class`/`id` from
/// a translation apply (fluent-dom drops non-localizable attributes), and
/// any element is styled, not only text-level ones (fluent-dom turns `div`
/// into text). String args stay text: see `fluent_localization`.
#[test]
fn fluent_permissive_markup() {
    let mut ui = TestUi::new(
        "fluent-permissive",
        &[
            (
                "page.html",
                r#"<p data-l10n-id="nested"></p>
<p data-l10n-id="class-attr"></p>
<p data-l10n-id="block-element"></p>"#,
            ),
            (
                "style.css",
                r#"
html { color: #ffffff; font-family: Spectral; font-size: 20px }
b { font-weight: bold }
i { font-style: italic }
.hot { color: #ff0000 }
div { color: #00ff00 }
"#,
            ),
            (
                "locales/en-US/main.ftl.ron",
                r#"(locale: "en-US", resources: ["ui.ftl"])"#,
            ),
            (
                "locales/en-US/ui.ftl",
                r#"
nested = Text <b>bold <i>both</i></b>
class-attr = Plain <span class="hot">hot</span>
block-element = Before <div>block</div> after
"#,
            ),
        ],
    )
    .stylesheet("style.css")
    .locale("locales/en-US/main.ftl.ron")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "Text " serif 20px #ffffff
    "bold " serif-bold 20px #ffffff
    "both" serif-bold-italic 20px #ffffff
  p
    "Plain " serif 20px #ffffff
    "hot" serif 20px #ff0000
  p
    "Before " serif 20px #ffffff
    "block" serif 20px #00ff00
    " after" serif 20px #ffffff
"#,
    );
}

/// Box model: `border-image` on the root (`html` rule), on a block (wrapper
/// node + inner `Text`) and on a container; px and % slices (% of the 32×24
/// image: top/bottom of the height, left/right of the width); tiling; borders,
/// padding, background, container `gap`.
#[test]
fn css_box_model() {
    let mut ui = TestUi::from_vector("box_model");
    ui.settle().assert_dump(
        r#"
html-ui border=16,16,16,16 padding=10,20,10,20 slice=frame.png 4,4,4,4 stretch
  p.framed border=6,8,6,8 bg=#102030 slice=frame.png 6,8,6,8 tile
    -
      "Framed" serif 20px #ffffff
  div.panel border=12,12,12,12 padding=4,4,4,4 gap=7 col-gap=7 bg=#333333 slice=frame.png 2,3,4,5 stretch
    p
      "One" serif 20px #ffffff
    p
      "Two" serif 20px #ffffff
"#,
    );
}

/// A template that compiles but fails to *render* (an undefined variable)
/// shows the error as a `failed to render:` paragraph naming the problem,
/// styled like the root, and recovers once the context provides the value.
#[test]
fn template_render_failure_shows_the_error_and_recovers() {
    let mut ui = TestUi::new(
        "render-failure",
        &[
            ("page.html", "<p>Hello {{ player }}</p>"),
            ("style.css", "html { color: #ff0000; font-size: 20px }"),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.update(30);
    {
        let world = ui.world_mut();
        let mut elements = world.query::<(Entity, &bevy_markup::html::HtmlElement)>();
        for (entity, element) in elements.iter(world) {
            eprintln!(
                "probe: tag={} node={}",
                element.tag,
                world.entity(entity).get::<Node>().is_some()
            );
        }
    }
    ui.settle();
    let root = ui.root();
    let failed = match ui.world_mut().get::<RenderedHtml>(root) {
        Some(RenderedHtml::Failed(message)) => message.clone(),
        _ => panic!("expected a render failure"),
    };
    assert!(
        failed.contains("player"),
        "error names the variable: {failed}"
    );
    let dump = ui.dump();
    let mut lines = dump.lines();
    assert_eq!(lines.next(), Some("html-ui"));
    assert_eq!(lines.next(), Some("  -"), "{dump}");
    let run = lines.next().unwrap_or_default();
    assert!(
        run.starts_with("    \"failed to render: ")
            && run.contains("player")
            && run.ends_with("default 20px #ff0000"),
        "{dump}"
    );

    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("player", "Ada");
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "Hello Ada" default 20px #ff0000
"#,
    );
}

/// A stylesheet that fails to load renders the UI unstyled (like a browser)
/// instead of leaving it blank forever.
#[test]
fn failed_stylesheet_still_renders() {
    let mut ui = TestUi::new(
        "failed_css",
        &[("page.html", "<p>Text</p>"), ("style.css", "{ not css")],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "Text" default 16px #ffffff
"#,
    );
}

/// A stylesheet that nests style rules thousands deep fails its load
/// like unparseable CSS instead of overflowing lightningcss's recursive
/// parser on the loader thread (review_0004 H02: ~1000 nested `a {`,
/// 3 KB, aborted the whole process — the nesting pre-scan counted only
/// parentheses).
#[test]
fn deeply_nested_style_rules_fail_the_load_instead_of_aborting() {
    let sheet = format!("p {{ color: #ff0000; {} }}", "a {".repeat(20_000));
    let mut ui = TestUi::new(
        "nested_rules_css",
        &[("page.html", "<p>Text</p>"), ("style.css", sheet.as_str())],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "Text" default 16px #ffffff
"#,
    );
}

/// A stylesheet with nested functions that lightningcss parses in
/// exponential time fails its load at once instead of stalling the
/// loader thread (bug_0179: 30 nested `light-dark(` took minutes, 30
/// nested `calc(` around a `var()` hours, both inside the block bound).
#[test]
fn exponentially_slow_function_nesting_fails_the_load_at_once() {
    let calc_var = (0..30).fold("var(--x)".to_owned(), |v, _| format!("calc({v} - 1px)"));
    let sheet = format!(
        "p {{ color: #ff0000; width: {calc_var} }}\np {{ color: {} }}",
        "light-dark(".repeat(30)
    );
    let start = std::time::Instant::now();
    let mut ui = TestUi::new(
        "exponential_css",
        &[("page.html", "<p>Text</p>"), ("style.css", sheet.as_str())],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "Text" default 16px #ffffff
"#,
    );
    assert!(
        start.elapsed().as_secs() < 30,
        "the load fails before lightningcss parses it"
    );
}

/// ADR 0034's trial: no element sits at position 0, so `:nth-child(0)`
/// and `:nth-child(-n+0)` match nothing. The crate's own matcher matched
/// the root `html`, whose style is computed without a position, and the
/// page inherited the color.
#[test]
fn nth_child_zero_matches_no_element() {
    let css = "html { color: #ffffff } :nth-child(0) { color: #ff0000 } :nth-child(-n+0) { color: #ff0000 }";
    let mut ui = TestUi::new(
        "nth_child_zero",
        &[("page.html", "<p>Text</p>"), ("style.css", css)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "Text" default 16px #ffffff
"#,
    );
}

/// Content can't reach Bevy's network asset sources (review_0003 G19): a
/// remote `border-image` is skipped (the rest of the sheet applies — it
/// used to be loaded, and its failed dependency kept the sheet from ever
/// resolving), and a Fluent manifest naming a remote resource fails.
#[test]
fn remote_asset_sources_are_refused() {
    let mut ui = TestUi::new(
        "remote_sources",
        &[
            ("page.html", "<p>Text</p>"),
            (
                "style.css",
                r#"html { color: #ff0000; border-image: url("https://evil.example/f.png") 4 stretch }"#,
            ),
            (
                "locales/en-US/remote.ftl.ron",
                r#"(locale: "en-US", resources: ["https://evil.example/ui.ftl"])"#,
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    let remote_bundle =
        ui.load::<bevy_markup::prelude::LocaleBundle>("locales/en-US/remote.ftl.ron");
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "Text" default 16px #ff0000
"#,
    );
    let server = ui.world_mut().resource::<AssetServer>().clone();
    for _ in 0..200 {
        if server.load_state(remote_bundle.id()).is_failed() {
            break;
        }
        ui.update(1);
    }
    assert!(server.load_state(remote_bundle.id()).is_failed());
}

/// Values Tera prints with Rust escapes still localize: inside a map,
/// Tera writes non-printable and combining characters as `\u{…}` and NUL
/// as `\0`, which JSON rejects — a player name with a family emoji (zero-
/// width joiners) or a decomposed accent fell back to the element's own
/// content (bug_0128, found by the testbench). Both the cached and the
/// per-value Markdown paths.
#[test]
fn tera_escaped_values_still_localize() {
    for (index, name) in [
        "Ada \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}",
        "Jose\u{301}",
        "bell\u{7}",
        "nul\u{0}",
        "private \u{f73c}b",
        "slash \\u{41} stays text",
    ]
    .into_iter()
    .enumerate()
    {
        let mut ui = TestUi::new(
            &format!("tera-escapes-{index}"),
            &[
                (
                    "page.html",
                    r#"<p data-l10n-id="sel" data-l10n-args='{{ {"n": 2, "who": who} | json }}'>FALLBACK</p><p data-l10n-id="hi" data-l10n-args='{{ {"who": who} | json }}'>FALLBACK</p>"#,
                ),
                (
                    "locales/en-US/main.ftl.ron",
                    r#"(locale: "en-US", resources: ["ui.ftl"], markdown: true)"#,
                ),
                (
                    "locales/en-US/ui.ftl",
                    "sel =\n    { $n ->\n        [one] { $who } found one\n       *[other] { $who } found { $n }\n    }\nhi = Hello { $who }!\n",
                ),
            ],
        )
        .locale("locales/en-US/main.ftl.ron")
        .spawn("page.html", TemplateContext::new().with("who", name), Node::default());
        let dump = ui.settle().dump();
        assert!(!dump.contains("FALLBACK"), "{name:?}: {dump}");
        // The visible part of the name survives.
        let visible: String = name
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(3)
            .collect();
        assert!(dump.contains(&visible), "{name:?}: {dump}");
    }
}

/// `data-l10n-args` built by Tera as a map literal is JSON with its
/// strings escaped: a value can't add arguments (review_0003 G20 — the
/// documented `'{"name": "{{ player }}"}'` splice let a name holding `"`
/// inject `"extra": …` once the entities were decoded).
#[test]
fn map_literal_args_cannot_be_injected() {
    let mut ui = TestUi::new(
        "args-injection",
        &[
            (
                "page.html",
                r#"<p data-l10n-id="greet" data-l10n-args='{{ {"name": player} | json }}'>x</p>"#,
            ),
            (
                "locales/en-US/main.ftl.ron",
                r#"(locale: "en-US", resources: ["ui.ftl"])"#,
            ),
            ("locales/en-US/ui.ftl", "greet = Hi { $name }!"),
        ],
    )
    .locale("locales/en-US/main.ftl.ron")
    .spawn(
        "page.html",
        TemplateContext::new().with("player", r#"Ada", "extra": "INJECTED"#),
        Node::default(),
    );
    let dump = ui.settle().dump();
    // The whole value is the name (an injection would cut it at `Ada`).
    assert!(
        dump.contains("INJECTED!"),
        "the value was split into arguments: {dump}"
    );
}

/// A Markdown template may indent its Tera blocks for readability: the
/// indented page builds exactly what its flat twin builds (review_0004
/// H11: the indented bodies rendered as code blocks).
#[test]
fn indented_markdown_templates_build_like_flat_ones() {
    let flat = "# Party\n\n{% for unit in units %}\n{% if unit.hp > 0 %}\n- **{{ unit.name }}** is up\n{% else %}\n- *{{ unit.name }}* is down\n{% endif %}\n{% endfor %}\n\nEnd of list.\n";
    let indented = "# Party\n\n{% for unit in units %}\n    {% if unit.hp > 0 %}\n        - **{{ unit.name }}** is up\n    {% else %}\n        - *{{ unit.name }}* is down\n    {% endif %}\n{% endfor %}\n\nEnd of list.\n";
    let context = || {
        TemplateContext::new().with(
            "units",
            &serde_json::json!([{"name": "Ada", "hp": 3}, {"name": "Brin", "hp": 0}]),
        )
    };
    let dump = |name: &str, page: &str| {
        TestUi::new(name, &[("page.md", page)])
            .spawn("page.md", context(), Node::default())
            .settle()
            .dump()
    };
    let flat_dump = dump("md-flat", flat);
    assert!(
        flat_dump.contains("Ada is up") && flat_dump.contains("Brin is down"),
        "{flat_dump}"
    );
    assert!(!flat_dump.contains("pre"), "{flat_dump}");
    assert_eq!(dump("md-indented", indented), flat_dump);
}

/// Raw HTML containers in a Markdown template may indent their bodies too:
/// the indented page builds what its flat twin builds (an indented `<div>`
/// body after a blank line rendered as a code block).
#[test]
fn indented_html_containers_in_markdown_build_like_flat_ones() {
    let flat =
        "<div class=\"card\">\n\n## Quest\n\nFind the **cat**.\n\n- north\n- east\n\n</div>\n";
    let indented = "<div class=\"card\">\n\n    ## Quest\n\n    Find the **cat**.\n\n    - north\n    - east\n\n</div>\n";
    let dump = |name: &str, page: &str| {
        TestUi::new(name, &[("page.md", page)])
            .spawn("page.md", TemplateContext::new(), Node::default())
            .settle()
            .dump()
    };
    let flat_dump = dump("md-div-flat", flat);
    assert!(
        flat_dump.contains("h2") && flat_dump.contains("east"),
        "{flat_dump}"
    );
    assert_eq!(dump("md-div-indented", indented), flat_dump);
}

/// Runtime: mutating `TemplateContext` re-renders, switching `ActiveLocale`
/// re-localizes, swapping `DefaultStylesheet` restyles — and dropping the
/// `html` rule's box properties restores the app's own `Node` values.
#[test]
fn runtime_changes_rebuild() {
    let mut ui = TestUi::new(
        "runtime",
        &[
            (
                "page.html",
                r#"<p data-l10n-id="count" data-l10n-args='{"n": {{ n }}}'>{{ n }}</p>"#,
            ),
            (
                "framed.css",
                r#"html { color: #ffffff; font-size: 20px; border-image: url("frame.png") 4 fill stretch; border-width: 16px; padding: 10px }"#,
            ),
            ("plain.css", "html { color: #00ff00; font-size: 10px }"),
            ("locales/en-US/main.ftl.ron", r#"(locale: "en-US", resources: ["ui.ftl"])"#),
            ("locales/en-US/ui.ftl", "count = { $n } items"),
            ("locales/de/main.ftl.ron", r#"(locale: "de", resources: ["ui.ftl"])"#),
            ("locales/de/ui.ftl", "count = { $n } Dinge"),
        ],
    )
    .stylesheet("framed.css")
    .locale("locales/en-US/main.ftl.ron")
    .spawn(
        "page.html",
        TemplateContext::new().with("n", &1),
        // The app's own padding, to be restored when CSS stops overriding it.
        Node {
            padding: UiRect::all(Val::Px(3.0)),
            ..default()
        },
    );
    ui.settle().assert_dump(
        r#"
html-ui border=16,16,16,16 padding=10,10,10,10 slice=frame.png 4,4,4,4 stretch
  p
    "1 items" default 20px #ffffff
"#,
    );

    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &5);
    ui.settle().assert_dump(
        r#"
html-ui border=16,16,16,16 padding=10,10,10,10 slice=frame.png 4,4,4,4 stretch
  p
    "5 items" default 20px #ffffff
"#,
    );

    let de = ui.load::<LocaleBundle>("locales/de/main.ftl.ron");
    ui.world_mut().resource_mut::<ActiveLocale>().set(de);
    ui.settle().assert_dump(
        r#"
html-ui border=16,16,16,16 padding=10,10,10,10 slice=frame.png 4,4,4,4 stretch
  p
    "5 Dinge" default 20px #ffffff
"#,
    );

    let plain = ui.load::<Stylesheet>("plain.css");
    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(plain);
    ui.settle().assert_dump(
        r#"
html-ui padding=3,3,3,3
  p
    "5 Dinge" default 10px #00ff00
"#,
    );
}

/// A render change that lands while the build is waiting for a
/// stylesheet is not lost (R05's Wait variant): a context change queued
/// during the `Wait` is still the content after the sheet resolves, not
/// the stale one the wait started with.
#[test]
fn render_change_during_a_wait_carries_the_new_content() {
    let page = r#"<p>{{ n }}</p>"#;
    let spawn = |name: &str| {
        TestUi::new(
            name,
            &[("page.html", page), ("late.css", "html { color: #00ff00 }")],
        )
        .spawn(
            "page.html",
            TemplateContext::new().with("n", &"a"),
            Node::default(),
        )
    };
    let mut ui = spawn("wait-content");
    let mut fresh = spawn("wait-content-fresh");
    ui.settle();
    fresh.settle();
    // Swap to a stylesheet that has not loaded yet: the next build waits.
    let late = ui.load::<Stylesheet>("late.css");
    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(late);
    // The context changes land during the wait (one per frame).
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &"b");
    ui.update(1);
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &"c");
    // The fresh reference changes the same way, with no wait in between.
    let froot = fresh.root();
    fresh
        .world_mut()
        .get_mut::<TemplateContext>(froot)
        .unwrap()
        .insert("n", &"b");
    fresh.update(1);
    fresh
        .world_mut()
        .get_mut::<TemplateContext>(froot)
        .unwrap()
        .insert("n", &"c");
    let late = fresh.load::<Stylesheet>("late.css");
    fresh.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(late);
    ui.settle();
    fresh.settle();
    assert_eq!(ui.dump(), fresh.dump(), "the waited-out build carries `c`");
    assert!(ui.dump().contains("\"c\""));
}

/// An all-dirty workload (every subtree translated, so nothing reuses)
/// with a stale skeleton still produces a fresh build's content (R04):
/// the retry gate must not serve the skeleton's content when the pairing
/// is skipped. On the revert, the last change showed its old value.
#[test]
fn gate_skipped_builds_with_a_stale_skeleton_stay_fresh() {
    let page = r#"{% for i in rows %}<p data-l10n-id="count" data-l10n-args='{"n": {{ n }}}'>{{ i }}</p>{% endfor %}"#;
    let files = [
        ("page.html", page),
        ("style.css", "html { color: #ffffff }"),
        (
            "locales/en-US/main.ftl.ron",
            r#"(locale: "en-US", resources: ["ui.ftl"])"#,
        ),
        ("locales/en-US/ui.ftl", "count = { $n } items"),
    ];
    let spawn = |name: &str| {
        TestUi::new(name, &files)
            .locale("locales/en-US/main.ftl.ron")
            .spawn(
                "page.html",
                TemplateContext::new()
                    .with("n", &1)
                    .with("rows", &[0, 1, 2, 3, 4, 5, 6, 7, 8]),
                Node::default(),
            )
    };
    let mut reused = spawn("stale-skeleton-reused");
    let mut fresh = spawn("stale-skeleton-fresh");
    reused.settle();
    fresh.settle();
    // Several context changes: each rebuilds every translated row (all
    // dirty), driving the skeleton's strikes up so the retry gate skips
    // the pairing walk.
    for value in [2, 3, 4, 5, 6] {
        for ui in [&mut reused, &mut fresh] {
            let root = ui.root();
            ui.world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("n", &value);
        }
        reused.settle();
        fresh.settle();
    }
    assert_eq!(reused.dump(), fresh.dump());
    assert!(reused.dump().contains("6 items"), "the last change wins");
}

/// Reuse keeps an unchanged subtree whose root tag produces no node of
/// its own: `<span>` runs directly in a container, while a sibling
/// changes. On the R04 revert the reused span's content vanished when
/// its parent changed.
#[test]
fn reuse_keeps_a_bare_span_when_a_sibling_changes() {
    let page = r#"<div><span>{{ a }}</span><p>{{ b }}</p></div>"#;
    let spawn = |name: &str| {
        TestUi::new(
            name,
            &[
                ("page.html", page),
                ("style.css", "html { color: #ffffff }"),
            ],
        )
        .spawn(
            "page.html",
            TemplateContext::new().with("a", &"1").with("b", &"x"),
            Node::default(),
        )
    };
    let mut reused = spawn("bare-span-reused");
    let mut fresh = spawn("bare-span-fresh");
    reused.settle();
    fresh.settle();
    for (a, b) in [("1", "y"), ("2", "z"), ("2", "w")] {
        for ui in [&mut reused, &mut fresh] {
            let root = ui.root();
            let mut context = ui.world_mut().get_mut::<TemplateContext>(root).unwrap();
            context.insert("a", &a);
            context.insert("b", &b);
        }
        reused.settle();
        fresh.settle();
        assert_eq!(reused.dump(), fresh.dump(), "a={a} b={b}");
    }
    assert!(reused.dump().contains("\"2\""), "the span's content stayed");
}

/// A content change with reuse and an inline `var()` elsewhere costs no
/// spurious restyles: a reuse build's fresh scan misses the reused
/// subtree's inline names, and assigning that smaller set forced two
/// full restyles per change (review_0003 G27). The union keeps the
/// names, so one content change is one build.
#[test]
fn reuse_with_inline_vars_forces_no_extra_restyles() {
    #[derive(Resource, Default, Debug)]
    struct Events {
        built: usize,
        restyled: usize,
    }
    let files = [
        (
            "page.html",
            r#"<div style="color: var(--c)"><p>one</p><p>two</p><p>three</p></div><p>{{ n }}</p>"#,
        ),
        ("style.css", "html { color: #ffffff }"),
    ];
    let spawn = |name: &str| {
        TestUi::new(name, &files).stylesheet("style.css").spawn(
            "page.html",
            TemplateContext::new().with("--c", &"#ff0000").with("n", &1),
            Node::default(),
        )
    };
    let mut reused = spawn("inline-var-reuse-reused");
    let mut fresh = spawn("inline-var-reuse-fresh");
    reused.settle();
    fresh.settle();
    reused.world_mut().init_resource::<Events>();
    reused.world_mut().add_observer(
        |_: On<bevy_markup::prelude::HtmlUiBuilt>, mut events: ResMut<Events>| events.built += 1,
    );
    reused.world_mut().add_observer(
        |_: On<bevy_markup::prelude::HtmlUiRestyled>, mut events: ResMut<Events>| {
            events.restyled += 1
        },
    );
    // One content change: one build and no restyle (the reverted union
    // produced built +1 restyled +2 per change; a skeleton-stripped twin
    // restyles 0 times too — review_0004 H10 tightened `<= 1`).
    *reused.world_mut().resource_mut::<Events>() = Events::default();
    for ui in [&mut reused, &mut fresh] {
        let root = ui.root();
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert("n", &2);
    }
    reused.settle();
    fresh.settle();
    let events = reused.world_mut().resource::<Events>();
    assert!(
        events.restyled == 0,
        "spurious restyles: built={} restyled={}",
        events.built,
        events.restyled
    );
    assert_eq!(reused.dump(), fresh.dump());
    // The inline var still tracks its context value across changes.
    for ui in [&mut reused, &mut fresh] {
        let root = ui.root();
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert("--c", &"#00ff00");
    }
    reused.settle();
    fresh.settle();
    assert!(reused.dump().contains("#00ff00"), "inline var restyles");
    assert_eq!(reused.dump(), fresh.dump());
}

/// A render change that lands while the build waits for a stylesheet is
/// not lost across a *reuse* (R05's Wait variant, bug_0034): two
/// elements change twice during the wait; the build that finally runs
/// must carry the latest values, exactly like a fresh twin — the revert
/// kept the first change's document and dropped the second.
#[test]
fn wait_swallows_two_changes_then_reuses_currently() {
    let files = [
        ("page.html", r#"<p>{{ p }}</p><p>{{ q }}</p>"#),
        ("late.css", "html { color: #ffffff }"),
        ("style.css", "html { color: #ffffff }"),
    ];
    let spawn = |name: &str| {
        TestUi::new(name, &files).stylesheet("style.css").spawn(
            "page.html",
            TemplateContext::new().with("p", &"a").with("q", &"b"),
            Node::default(),
        )
    };
    let mut reused = spawn("wait-reuse-reused");
    let mut fresh = spawn("wait-reuse-fresh");
    reused.settle();
    fresh.settle();
    // Swap to a stylesheet that has not loaded yet (a Wait), and change
    // both values while it loads.
    let late = reused.load::<Stylesheet>("late.css");
    reused.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(late);
    let late2 = fresh.load::<Stylesheet>("late.css");
    fresh.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(late2);
    for ui in [&mut reused, &mut fresh] {
        let root = ui.root();
        let mut context = ui.world_mut().get_mut::<TemplateContext>(root).unwrap();
        context.insert("p", &"b");
        context.insert("q", &"a");
    }
    reused.update(1);
    fresh.update(1);
    // A second change while still waiting.
    for ui in [&mut reused, &mut fresh] {
        let root = ui.root();
        let mut context = ui.world_mut().get_mut::<TemplateContext>(root).unwrap();
        context.insert("p", &"a");
        context.insert("q", &"c");
    }
    reused.settle();
    fresh.settle();
    assert!(reused.dump().contains("\"a\""), "{}", reused.dump());
    assert!(reused.dump().contains("\"c\""), "{}", reused.dump());
    assert_eq!(reused.dump(), fresh.dump());
}

/// A style change that lands while the debug outline is shown is not
/// lost: outline builds leave the skeleton frozen, and consuming the
/// pending-style bit there let the outline-removal build reuse the
/// stale specs — the reused UI kept the old theme (review_0003 G26).
#[test]
fn style_change_while_outlined_applies_on_removal() {
    let files = [
        ("page.html", r#"<p class="s">Static</p><p>{{ x }}</p>"#),
        ("a.css", "html { color: #ffffff } .s { color: #ff0000 }"),
        ("b.css", "html { color: #ffffff } .s { color: #0000ff }"),
    ];
    let spawn = |name: &str| {
        TestUi::new(name, &files).stylesheet("a.css").spawn(
            "page.html",
            TemplateContext::new().with("x", &1),
            Node::default(),
        )
    };
    let mut reused = spawn("outlined-swap-reused");
    let mut fresh = spawn("outlined-swap-fresh");
    reused.settle();
    fresh.settle();
    // Show the outline, then swap the theme while it is shown.
    let root = reused.root();
    reused
        .world_mut()
        .entity_mut(root)
        .insert(bevy_markup::html::HtmlDebugOutline);
    reused.update(2);
    // The outline and the swap don't need build-count convergence; a
    // plain settle here can outrun the (single) build it waits for.
    reused.settle_quiet();
    // Both UIs swap; only the outlined one exercises the stale-skeleton
    // reuse on removal.
    let b = reused.load::<Stylesheet>("b.css");
    let b2 = fresh.load::<Stylesheet>("b.css");
    reused.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(b);
    fresh.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(b2);
    reused.settle_quiet();
    fresh.settle_quiet();
    // Remove the outline and change the content in the same frame.
    reused
        .world_mut()
        .entity_mut(root)
        .remove::<bevy_markup::html::HtmlDebugOutline>();
    reused
        .world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("x", &2);
    let froot = fresh.root();
    fresh
        .world_mut()
        .get_mut::<TemplateContext>(froot)
        .unwrap()
        .insert("x", &2);
    reused.settle();
    fresh.settle();
    let dump = reused.dump();
    assert!(dump.contains("#0000ff"), "the new theme applies: {dump}");
    assert_eq!(reused.dump(), fresh.dump());
}

/// CSS variables end to end: `var(--name)` resolves the UI's
/// `TemplateContext` first (a change re-renders and restyles), then the
/// sheet's own `--` declarations.
#[test]
fn template_context_drives_css_variables() {
    let mut ui = TestUi::new(
        "css_variables",
        &[
            ("page.html", r#"<p>{{ n }}</p>"#),
            (
                "style.css",
                r#"
html { color: var(--text, #ffffff) }
p { color: var(--accent); font-size: var(--px) }
"#,
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new()
            .with("n", &1)
            .with("--accent", "#ff8800")
            .with("--px", &24),
        Node::default(),
    );
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "1" default 24px #ff8800
"#,
    );

    // A context change with a new value re-resolves the variable.
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("--accent", "#00cc88");
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "1" default 24px #00cc88
"#,
    );
}

/// Transitions end to end: a `:active` restyle of a `transition:` element
/// starts an animation — the transitioned color keeps showing while the
/// same restyle's un-transitioned properties land at once (which proves
/// the restyle itself ran).
#[test]
fn transitions_animate_a_restyle() {
    let mut ui = TestUi::new(
        "transitions",
        &[
            ("page.html", "<p>hi</p>"),
            (
                "style.css",
                r#"
html { color: #ffffff }
p { color: #000000; transition: color 100s linear }
p:active { color: #ffffff; font-size: 30px }
"#,
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "hi" default 16px #000000
"#,
    );

    // Restyle the p by state (`:active`): the un-transitioned font-size
    // lands immediately; the transitioned color is held at its start.
    let element = ui
        .world_mut()
        .query::<(Entity, &bevy_markup::html::HtmlElement)>()
        .iter(ui.world_mut())
        .find(|(_, element)| element.tag == "p")
        .expect("the p element")
        .0;
    ui.world_mut()
        .entity_mut(element)
        .insert(bevy_markup::signals::PseudoState {
            active: true,
            ..default()
        });
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "hi" default 30px #000000
"#,
    );
}

/// A block with only a background (no border) still gets its box: the
/// background is drawn. A boxed `li` keeps its list indent on the box.
#[test]
fn background_only_block_keeps_its_background() {
    let mut ui = TestUi::new(
        "background-only",
        &[
            ("page.html", r#"<p class="shaded">Shaded</p><ul><li class="shaded">Item</li></ul>"#),
            (
                "style.css",
                "html { color: #ffffff; font-family: Spectral; font-size: 20px } .shaded { background-color: #102030 }",
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p.shaded bg=#102030
    -
      "Shaded" serif 20px #ffffff
  ul
    li.shaded margin=0,0,0,12 bg=#102030
      -
        "• " serif 20px #ffffff
        "Item" serif 20px #ffffff
"#,
    );
}

/// A stylesheet swap restyles the existing children in place: the same
/// entities — with what the app attached — take the new styles, and
/// `HtmlUiRestyled` fires instead of `HtmlUiBuilt`. When the new styles
/// need another structure (blocks gaining a box need wrapper nodes), the
/// children are rebuilt and `HtmlUiBuilt` fires.
#[test]
fn style_changes_restyle_in_place_unless_the_shape_changes() {
    #[derive(Component)]
    struct Wired;
    #[derive(Resource, Default, Debug)]
    struct Events {
        built: usize,
        restyled: usize,
    }
    fn descendants(world: &World, entity: Entity, out: &mut Vec<Entity>) {
        for &child in world.entity(entity).get::<Children>().into_iter().flatten() {
            out.push(child);
            descendants(world, child, out);
        }
    }

    let mut ui = TestUi::new(
        "restyle",
        &[
            (
                "page.html",
                r#"<p id="a">One</p><div class="box"><p>Two</p></div>"#,
            ),
            ("red.css", "html { color: #ff0000; font-size: 20px }"),
            (
                "blue.css",
                "html { color: #0000ff; font-size: 20px } .box { padding: 4px }",
            ),
            (
                "boxed.css",
                "html { color: #0000ff; font-size: 20px } p { background-color: #102030 }",
            ),
        ],
    )
    .stylesheet("red.css");
    let blue = ui.load::<Stylesheet>("blue.css");
    let boxed = ui.load::<Stylesheet>("boxed.css");
    let mut ui = ui.spawn("page.html", TemplateContext::new(), Node::default());
    let world = ui.world_mut();
    world.init_resource::<Events>();
    world.add_observer(|_: On<HtmlUiBuilt>, mut events: ResMut<Events>| events.built += 1);
    world.add_observer(|_: On<HtmlUiRestyled>, mut events: ResMut<Events>| events.restyled += 1);
    ui.settle().assert_dump(
        r#"
html-ui
  p#a
    "One" default 20px #ff0000
  div.box
    p
      "Two" default 20px #ff0000
"#,
    );
    let root = ui.root();
    let world = ui.world_mut();
    let mut before = Vec::new();
    descendants(world, root, &mut before);
    let mut elements = world.query::<(Entity, &HtmlElement)>();
    let wired = elements
        .iter(world)
        .find(|(_, element)| element.id.as_deref() == Some("a"))
        .map(|(entity, _)| entity)
        .unwrap();
    world.entity_mut(wired).insert(Wired);

    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(blue);
    ui.settle().assert_dump(
        r#"
html-ui
  p#a
    "One" default 20px #0000ff
  div.box padding=4,4,4,4
    p
      "Two" default 20px #0000ff
"#,
    );
    let world = ui.world_mut();
    let mut after = Vec::new();
    descendants(world, root, &mut after);
    assert_eq!(after, before, "restyle kept every child entity");
    assert!(
        world.entity(wired).contains::<Wired>(),
        "app component survived"
    );
    let events = world.resource::<Events>();
    assert_eq!((events.built, events.restyled), (1, 1));

    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(boxed);
    ui.settle().assert_dump(
        r#"
html-ui
  p#a bg=#102030
    -
      "One" default 20px #0000ff
  div.box
    p bg=#102030
      -
        "Two" default 20px #0000ff
"#,
    );
    let events = ui.world_mut().resource::<Events>();
    assert_eq!(
        (events.built, events.restyled),
        (2, 1),
        "new wrappers: rebuilt"
    );
}

/// Swapping stylesheet `a` → `b` must end exactly where a fresh build with
/// `b` does — also when only a nested node's shape changes, a frame goes
/// away, or text runs merge (the restyle must notice and rebuild).
#[test]
fn restyles_that_change_shape_match_a_fresh_build() {
    let page = r#"<p id="a">One</p><div class="box"><p class="inner">Plain <b>bold</b></p></div>"#;
    let cases = [
        // Only the nested block gains a box (needs a wrapper).
        (
            "html { color: #ffffff }",
            "html { color: #ffffff } .inner { background-color: #102030 }",
        ),
        // The container loses its frame.
        (
            r#"html { color: #ffffff } .box { border-image: url("frame.png") 4 fill stretch; border-width: 4px }"#,
            "html { color: #ffffff }",
        ),
        // `b` stops differing from its block: two runs become one.
        (
            "html { color: #ffffff } b { color: #ff0000 }",
            "html { color: #ffffff }",
        ),
    ];
    for (a, b) in cases {
        let mut swapped = TestUi::new(
            "restyle-shape",
            &[("page.html", page), ("a.css", a), ("b.css", b)],
        )
        .stylesheet("a.css");
        let b_sheet = swapped.load::<Stylesheet>("b.css");
        let mut swapped = swapped.spawn("page.html", TemplateContext::new(), Node::default());
        swapped.settle();
        swapped.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(b_sheet);
        let mut fresh = TestUi::new("restyle-fresh", &[("page.html", page), ("b.css", b)])
            .stylesheet("b.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        assert_eq!(swapped.settle().dump(), fresh.settle().dump(), "{a} → {b}");
    }
}

/// The `html` rule's box properties leave an `ImageNode` the app put on the
/// `HtmlUi` node alone when the stylesheet has no frame of its own.
#[test]
fn root_box_keeps_the_apps_own_image() {
    let mut ui = TestUi::new(
        "root-image",
        &[
            ("page.html", "<p>Text</p>"),
            ("style.css", "html { padding: 5px }"),
        ],
    )
    .stylesheet("style.css");
    let template = ui.load::<HtmlTemplate>("page.html");
    let backdrop = ui.load::<Image>("frame.png");
    let root = ui
        .world_mut()
        .spawn((HtmlUi::new(template), ImageNode::new(backdrop.clone())))
        .id();
    // `TestUi::spawn` isn't used: the root needs the app's own `ImageNode`.
    ui.settle_quiet();
    ui.update(10);
    let world = ui.world_mut();
    assert_eq!(
        world.get::<Node>(root).unwrap().padding,
        UiRect::all(Val::Px(5.0))
    );
    let image = world
        .get::<ImageNode>(root)
        .expect("the app's ImageNode survived");
    assert_eq!(image.image, backdrop);
}

/// An `ImageNode` the app put on a built element (an icon) is app state, not
/// a CSS frame: a restyle keeps the element and its image instead of
/// rebuilding. Before the frame got its own marker, the image failed the
/// restyle's shape check, so every restyle — including the `PseudoState`
/// insert after each build — rebuilt the UI and the app re-attached the
/// image: a rebuild every frame.
#[test]
fn restyle_keeps_an_apps_image_on_an_element() {
    #[derive(Resource, Default)]
    struct Builds(usize);

    let mut ui = TestUi::new(
        "element-image",
        &[
            ("page.html", r#"<div id="icon"></div><p>Label</p>"#),
            ("red.css", "html { color: #ff0000 }"),
            ("blue.css", "html { color: #0000ff } #icon { width: 16px }"),
        ],
    )
    .stylesheet("red.css");
    let blue = ui.load::<Stylesheet>("blue.css");
    let icon_image = ui.load::<Image>("frame.png");
    let mut ui = ui.spawn("page.html", TemplateContext::new(), Node::default());
    ui.world_mut().init_resource::<Builds>();
    ui.world_mut()
        .add_observer(|_: On<HtmlUiBuilt>, mut builds: ResMut<Builds>| builds.0 += 1);
    ui.settle();
    let world = ui.world_mut();
    let mut elements = world.query::<(Entity, &HtmlElement)>();
    let icon = elements
        .iter(world)
        .find(|(_, element)| element.id.as_deref() == Some("icon"))
        .map(|(entity, _)| entity)
        .unwrap();
    world
        .entity_mut(icon)
        .insert(ImageNode::new(icon_image.clone()));
    let builds = world.resource::<Builds>().0;

    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(blue);
    ui.settle_quiet();
    ui.update(10);
    let world = ui.world_mut();
    assert_eq!(
        world.resource::<Builds>().0,
        builds,
        "restyled, not rebuilt"
    );
    let entity = world.entity(icon);
    assert_eq!(
        entity.get::<Node>().unwrap().width,
        Val::Px(16.0),
        "restyle applied"
    );
    assert_eq!(
        entity
            .get::<ImageNode>()
            .expect("the app's image survived")
            .image,
        icon_image
    );
}

/// `position`/insets, `border-radius`, `border-color`, `z-index` and
/// `pointer-events` reach the built nodes; `pointer-events` inherits (the
/// block inside `.overlay` is unpickable too); `static` ignores insets.
/// A stylesheet swap without them takes back only what CSS inserted: an
/// app's own `ZIndex` on another element survives.
#[test]
fn positioning_radius_border_color_z_index_and_pointer_events() {
    let page = r#"<div class="overlay"><p id="tip">Tip</p></div><div id="static"></div><div id="app"></div><div class="pinned"></div><div class="oval"></div>"#;
    let css =
        ".overlay { position: absolute; top: 4px; left: 8px; z-index: 5; pointer-events: none; \
               border-radius: 3px; border-width: 1px; border-color: #ff0000 }
               #static { position: static; top: 9px }
               .pinned { position: fixed; top: 5px; left: 6px }
               .oval { border-radius: 20px 8px / 10px 25% }";
    let mut ui = TestUi::new(
        "positioning",
        &[
            ("page.html", page),
            ("a.css", css),
            ("b.css", "p { color: #ffffff }"),
        ],
    )
    .stylesheet("a.css");
    let plain = ui.load::<Stylesheet>("b.css");
    let mut ui = ui.spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  div.overlay border=1,1,1,1 pos=abs inset=4,auto,auto,8 radius=3,3,3,3 bcolor=#ff0000,#ff0000,#ff0000,#ff0000 z=5 pick=none
    p#tip pick=none
      "Tip" default 16px #ffffff
  div#static
  div#app
  div.pinned pos=fixed inset=5,auto,auto,6
  div.oval radius=20/10,8/Percent(25.0),20/10,8/Percent(25.0)
"#,
    );
    let world = ui.world_mut();
    let mut elements = world.query::<(Entity, &HtmlElement)>();
    let app = elements
        .iter(world)
        .find(|(_, element)| element.id.as_deref() == Some("app"))
        .map(|(entity, _)| entity)
        .unwrap();
    world.entity_mut(app).insert(ZIndex(2));

    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(plain);
    ui.settle_quiet();
    ui.update(10);
    ui.assert_dump(
        r#"
html-ui
  div.overlay
    p#tip
      "Tip" default 16px #ffffff
  div#static
  div#app z=2
  div.pinned
  div.oval
"#,
    );
}

/// The element with this `id` below `root`.
fn element_by_id(world: &mut World, root: Entity, id: &str) -> Entity {
    let mut elements = world.query::<(Entity, &HtmlElement)>();
    let candidates: Vec<Entity> = elements
        .iter(world)
        .filter(|(_, element)| element.id.as_deref() == Some(id))
        .map(|(entity, _)| entity)
        .collect();
    candidates
        .into_iter()
        .find(|&entity| {
            std::iter::successors(Some(entity), |&current| {
                world.get::<ChildOf>(current).map(ChildOf::parent)
            })
            .any(|ancestor| ancestor == root)
        })
        .unwrap_or_else(|| panic!("no #{id} below {root}"))
}

fn focused(world: &World) -> Option<Entity> {
    world.resource::<bevy::input_focus::InputFocus>().get()
}

fn navigable(world: &World, entity: Entity) -> bool {
    world
        .entity(entity)
        .contains::<bevy::ui::auto_directional_navigation::AutoDirectionalNavigation>()
}

/// Browser-style focus: `data-on-click` and `tabindex >= 0` elements are
/// focusable (`tabindex="-1"` opts out), `autofocus` takes the initial
/// focus, `:focus-visible` (with `outline`) applies only while focus is
/// shown, activation emits the click signal, a rebuild keeps focus on the
/// same `id`, a modal root confines focus and an `HtmlNoFocus` root never
/// takes it. `autofocus` comes first on `#b`: a value-less attribute must
/// not eat the next attribute's first character (bug_0019).
#[test]
fn focus_navigation_scope_and_styles() {
    let page = r#"{% if n > 1 %}<section{% else %}<div{% endif %} id="a" data-on-click="pick-a"><p>A {{ n }}</p>{% if n > 1 %}</section>{% else %}</div>{% endif %}
        <div id="b" autofocus data-on-click="pick-b" data-with='{"n": {{ n }}}'><p>B</p></div>
        <div id="c" tabindex="0"><p>C</p></div>
        <div id="d" data-on-click="pick-d" tabindex="-1"><p>D</p></div>"#;
    let css = "div:focus-visible { outline: 2px solid #ff0000; outline-offset: 3px } \
               #c:focus { background-color: #00ff00 }";
    let modal = r#"<div id="ok" data-on-click="ok"><p>OK</p></div>"#;
    let mut ui = TestUi::new(
        "focus",
        &[
            ("page.html", page),
            ("style.css", css),
            ("modal.html", modal),
        ],
    )
    .stylesheet("style.css");
    let modal_template = ui.load::<HtmlTemplate>("modal.html");
    let mut ui = ui.spawn(
        "page.html",
        TemplateContext::new().with("n", &1),
        Node::default(),
    );
    ui.settle();
    ui.update(3);
    let root = ui.root();
    let world = ui.world_mut();
    let [a, b, c, d] = ["a", "b", "c", "d"].map(|id| element_by_id(world, root, id));
    assert_eq!(focused(world), Some(b), "autofocus");
    assert_eq!(
        [a, b, c, d].map(|entity| navigable(world, entity)),
        [true, true, true, false]
    );
    assert!(world.get::<Outline>(b).is_none(), "focus not shown yet");

    // Shown focus: `:focus-visible` restyles in place.
    world
        .resource_mut::<bevy::input_focus::InputFocusVisible>()
        .0 = true;
    ui.update(3);
    let world = ui.world_mut();
    let outline = world.get::<Outline>(b).expect(":focus-visible outline");
    assert_eq!(
        (outline.width, outline.offset),
        (Val::Px(2.0), Val::Px(3.0))
    );

    // Moving focus moves the styles (`:focus` alone styles `#c`).
    world
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(c, bevy::input_focus::FocusCause::Navigated);
    ui.update(3);
    let world = ui.world_mut();
    assert!(world.get::<Outline>(b).is_none(), "outline left b");
    assert_eq!(
        world.get::<BackgroundColor>(c).map(|bg| hex(bg.0)),
        Some("#00ff00".to_owned())
    );

    // Activation emits the element's click signal with its payload and the
    // input the app reported.
    world.trigger(ActivateElement {
        entity: b,
        input: ActivationInput::Key(KeyCode::Enter),
    });
    let signals: Vec<ElementSignal> = world
        .resource::<Messages<ElementSignal>>()
        .iter_current_update_messages()
        .cloned()
        .collect();
    assert_eq!(signals.len(), 1);
    assert_eq!(
        (
            signals[0].name.as_ref(),
            signals[0].payload["n"].as_i64(),
            signals[0].source
        ),
        (
            "pick-b",
            Some(1),
            SignalSource::Activation(ActivationInput::Key(KeyCode::Enter))
        )
    );

    // `a` becomes another element (a `section`): the update replaces it, and
    // focus follows the `id`.
    world
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(a, bevy::input_focus::FocusCause::Navigated);
    ui.update(2);
    let world = ui.world_mut();
    world
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &2);
    ui.settle();
    ui.update(3);
    let world = ui.world_mut();
    let new_a = element_by_id(world, root, "a");
    assert_ne!(new_a, a, "replaced");
    assert_eq!(focused(world), Some(new_a), "focus restored by id");

    // A content update that keeps the structure updates in place: same
    // entities, focus untouched, and `b`'s new `data-with` is what fires.
    let b = element_by_id(world, root, "b");
    world
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &3);
    ui.settle();
    ui.update(3);
    let world = ui.world_mut();
    assert_eq!(element_by_id(world, root, "a"), new_a, "kept in place");
    assert_eq!(focused(world), Some(new_a));
    world.trigger(ActivateElement {
        entity: b,
        input: ActivationInput::Synthetic,
    });
    let payloads: Vec<Option<i64>> = world
        .resource::<Messages<ElementSignal>>()
        .iter_current_update_messages()
        .filter(|signal| signal.name == "pick-b")
        .map(|signal| signal.payload["n"].as_i64())
        .collect();
    assert_eq!(payloads, [Some(3)], "the updated binding, once");

    // A modal root takes focus and confines navigation.
    let dialog = world
        .spawn((HtmlUi::new(modal_template.clone()), HtmlModal))
        .id();
    ui.settle_quiet();
    ui.update(5);
    let world = ui.world_mut();
    let ok = element_by_id(world, dialog, "ok");
    assert_eq!(focused(world), Some(ok), "modal takes focus");
    assert!(navigable(world, ok));
    let page_b = element_by_id(world, root, "b");
    assert!(!navigable(world, page_b), "page outside the modal");

    // Closing it hands focus back to the page (its autofocus element).
    world.entity_mut(dialog).despawn();
    ui.update(3);
    let world = ui.world_mut();
    let new_b = element_by_id(world, root, "b");
    assert_eq!(focused(world), Some(new_b));
    assert!(navigable(world, new_b));

    // An `HtmlNoFocus` root's elements never take part.
    let panel = world.spawn((HtmlUi::new(modal_template), HtmlNoFocus)).id();
    ui.settle_quiet();
    ui.update(5);
    let world = ui.world_mut();
    let panel_ok = element_by_id(world, panel, "ok");
    assert!(!navigable(world, panel_ok));
    assert_eq!(focused(world), Some(new_b));
}

/// `HtmlFocus::navigate` moves focus by layout position and shows it; at an
/// edge it triggers `FocusEdge` instead. `HtmlFocus::activate` emits the
/// focused element's click signal.
#[test]
fn html_focus_navigates_by_layout_and_reports_edges() {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::math::CompassOctant;

    #[derive(Resource, Default)]
    struct Edges(Vec<CompassOctant>);

    let page = r#"<div id="top" data-on-click="top" autofocus><p>Top</p></div>
        <div id="bottom" data-on-click="bottom"><p>Bottom</p></div>"#;
    let mut ui = TestUi::with_layout("navigate", &[("page.html", page)], UVec2::new(320, 240))
        .spawn(
            "page.html",
            TemplateContext::new(),
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        );
    ui.settle();
    ui.update(3);
    let root = ui.root();
    let world = ui.world_mut();
    world.init_resource::<Edges>();
    world
        .add_observer(|edge: On<FocusEdge>, mut edges: ResMut<Edges>| edges.0.push(edge.direction));
    let [top, bottom] = ["top", "bottom"].map(|id| element_by_id(world, root, id));
    assert_eq!(focused(world), Some(top));

    let moved = world
        .run_system_once(|mut focus: HtmlFocus| focus.navigate(CompassOctant::South))
        .unwrap();
    assert_eq!(moved, Some(bottom));
    assert_eq!(focused(world), Some(bottom));
    assert!(
        world.resource::<bevy::input_focus::InputFocusVisible>().0,
        "navigation shows focus"
    );

    let moved = world
        .run_system_once(|mut focus: HtmlFocus| focus.navigate(CompassOctant::South))
        .unwrap();
    world.flush();
    assert_eq!(moved, None);
    assert_eq!(world.resource::<Edges>().0, [CompassOctant::South]);
    assert_eq!(focused(world), Some(bottom));

    world
        .run_system_once(|mut focus: HtmlFocus| focus.activate(ActivationInput::Synthetic))
        .unwrap();
    world.flush();
    let names: Vec<String> = world
        .resource::<Messages<ElementSignal>>()
        .iter_current_update_messages()
        .map(|signal| signal.name.to_string())
        .collect();
    assert_eq!(names, ["bottom"]);
}

/// Writing a `TemplateContext` whose render is unchanged (same values, or a
/// value the template doesn't print) builds nothing: entities and attached
/// components stay. A change that alters the output rebuilds once.
#[test]
fn identical_render_skips_the_rebuild() {
    #[derive(Component)]
    struct Wired;

    let mut ui = TestUi::new("same-render", &[("page.html", r#"<p id="n">{{ n }}</p>"#)]).spawn(
        "page.html",
        TemplateContext::new().with("n", &1).with("unused", &0),
        Node::default(),
    );
    ui.settle();
    let root = ui.root();
    let world = ui.world_mut();
    let paragraph = element_by_id(world, root, "n");
    world.entity_mut(paragraph).insert(Wired);
    let builds = ui.builds();

    for (key, value) in [("n", 1), ("unused", 7)] {
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert(key, &value);
        ui.update(5);
        assert_eq!(ui.builds(), builds, "{key} = {value}: nothing to rebuild");
    }
    let world = ui.world_mut();
    assert!(
        world.entity(paragraph).contains::<Wired>(),
        "entity and its components kept"
    );

    world
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &2);
    ui.settle().assert_dump(
        r#"
html-ui
  p#n
    "2" default 16px #ffffff
"#,
    );
    assert_eq!(ui.builds(), builds + 1, "a changed render rebuilds once");
}

/// `is="…"` runs the app's definition on every spawned element, in document
/// order, with the element's `data-*` dataset and its UI root, before
/// `HtmlUiBuilt` observers run. A rebuild runs it again on the new entities;
/// a restyle keeps the entities and what the definition attached, without
/// running it. An undefined name, and `is` on an element that doesn't become
/// a node (an inline `span`), are ignored.
#[test]
fn custom_elements_run_on_every_spawn() {
    #[derive(Component)]
    struct Icon(String);
    #[derive(Resource, Default)]
    struct Connected(Vec<(Entity, Option<String>, String, Option<String>)>);
    #[derive(Resource, Default)]
    struct IconsAtBuilt(Vec<usize>);

    let mut ui = TestUi::new(
        "custom-elements",
        &[
            (
                "page.html",
                r#"<div id="a" is="icon" data-icon="{{ icon }}" data-size="2">
                     <p id="b" is=" icon " data-icon="inner">x <span is="icon" data-icon="inline">y</span></p>
                   </div>
                   <p id="c" is="undefined">z</p>"#,
            ),
            ("white.css", "p { color: #ffffff }"),
            ("red.css", "p { color: #ff0000 }"),
        ],
    )
    .stylesheet("white.css");
    let app = ui.app_mut();
    app.init_resource::<Connected>()
        .init_resource::<IconsAtBuilt>()
        .define_html_element(
            "icon",
            |connected: In<ElementConnected>,
             elements: Query<&HtmlElement>,
             mut log: ResMut<Connected>,
             mut commands: Commands| {
                let icon = connected.data("icon").unwrap_or_default().to_owned();
                log.0.push((
                    connected.root,
                    elements
                        .get(connected.entity)
                        .ok()
                        .and_then(|e| e.id.clone()),
                    icon.clone(),
                    connected.data("size").map(str::to_owned),
                ));
                commands.entity(connected.entity).insert(Icon(icon));
            },
        )
        .add_observer(
            |_: On<HtmlUiBuilt>, icons: Query<&Icon>, mut seen: ResMut<IconsAtBuilt>| {
                seen.0.push(icons.iter().count());
            },
        );
    let mut ui = ui.spawn(
        "page.html",
        TemplateContext::new().with("icon", &"key_e"),
        Node::default(),
    );
    ui.settle();
    let root = ui.root();
    let log = |ui: &mut TestUi| ui.world_mut().resource::<Connected>().0.clone();
    let id = |id: &str| Some(id.to_owned());
    assert_eq!(
        log(&mut ui),
        [
            (root, id("a"), "key_e".to_owned(), Some("2".to_owned())),
            (root, id("b"), "inner".to_owned(), None),
        ],
        "document order, dataset, root; span and undefined name skipped"
    );
    assert_eq!(
        ui.world_mut().resource::<IconsAtBuilt>().0,
        [2],
        "attached before HtmlUiBuilt"
    );
    let icons = |ui: &mut TestUi| {
        let world = ui.world_mut();
        let mut query = world.query::<(Entity, &Icon)>();
        let mut icons: Vec<_> = query
            .iter(world)
            .map(|(e, icon)| (e, icon.0.clone()))
            .collect();
        icons.sort();
        icons
    };
    let built = icons(&mut ui);

    // Restyle: same entities, components kept, definition not re-run.
    let red = ui.load::<Stylesheet>("red.css");
    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(red);
    ui.settle();
    assert!(ui.dump().contains("#ff0000"), "restyled:\n{}", ui.dump());
    assert_eq!(
        icons(&mut ui),
        built,
        "restyle keeps the attached components"
    );
    assert_eq!(log(&mut ui).len(), 2, "restyle doesn't re-run definitions");

    // Rebuild: new entities, definitions re-run with the new data.
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("icon", &"key_r");
    ui.settle();
    let rebuilt = icons(&mut ui);
    assert_eq!(rebuilt.len(), 2);
    assert!(
        rebuilt
            .iter()
            .all(|(entity, _)| built.iter().all(|(old, _)| old != entity))
    );
    assert_eq!(
        log(&mut ui)[2..]
            .iter()
            .map(|row| row.2.as_str())
            .collect::<Vec<_>>(),
        ["key_r", "inner"]
    );
    assert_eq!(ui.world_mut().resource::<IconsAtBuilt>().0, [2, 2]);
}

/// The root rule — `html`, and the document's own `<html id class>` —
/// styles the `HtmlUi` entity itself: layout, position and insets, padding,
/// `border-radius` on its `Node`; `background-color`, `z-index`,
/// `pointer-events` as components. Only what it declares: the app's other
/// fields (moved every frame, say) survive rebuilds, and a stylesheet that
/// stops declaring something gives the app's value back.
#[test]
fn root_rule_styles_the_html_ui_entity() {
    let mut ui = TestUi::new(
        "root-rule",
        &[
            (
                "page.html",
                r#"<html class="hud {{ extra }}" style="height: 30px; opacity: 0.5"><p>x</p></html>"#,
            ),
            (
                "styled.css",
                "html { padding: 3px }
                 .hud { position: absolute; top: 10px; width: 40%; flex-direction: column;
                        background-color: #ff0000; z-index: 5; pointer-events: none;
                        border-radius: 4px }
                 .wide { width: 90% }",
            ),
            ("plain.css", "p { color: #ffffff }"),
        ],
    )
    .stylesheet("styled.css")
    .spawn(
        "page.html",
        TemplateContext::new().with("extra", ""),
        Node {
            left: Val::Px(7.0),
            width: Val::Px(50.0),
            padding: UiRect::all(Val::Px(1.0)),
            ..default()
        },
    );
    let root = ui.root();
    let blue = Color::srgb(0.0, 0.0, 1.0);
    ui.world_mut()
        .entity_mut(root)
        .insert(BackgroundColor(blue));
    ui.settle();

    let world = ui.world_mut();
    let node = world.get::<Node>(root).unwrap().clone();
    assert_eq!(node.position_type, PositionType::Absolute);
    assert_eq!(
        (node.top, node.left),
        (Val::Px(10.0), Val::Px(7.0)),
        "top CSS, left the app's"
    );
    assert_eq!(node.width, Val::Percent(40.0));
    assert_eq!(node.flex_direction, FlexDirection::Column);
    assert_eq!(
        node.padding,
        UiRect::all(Val::Px(3.0)),
        "the `html` rule applies too"
    );
    assert_eq!(
        node.border_radius.top_left,
        (Val::Px(4.0), Val::Px(4.0)).into()
    );
    assert_eq!(
        world.get::<BackgroundColor>(root).unwrap().0,
        Color::srgba(1.0, 0.0, 0.0, 0.5),
        "faded by the root's own opacity"
    );
    assert_eq!(world.get::<ZIndex>(root), Some(&ZIndex(5)));
    assert_eq!(world.get::<Pickable>(root), Some(&Pickable::IGNORE));
    assert_eq!(node.height, Val::Px(30.0), "the root's own `style`");
    let text = |world: &mut World| {
        let mut query = world.query_filtered::<&TextColor, With<Text>>();
        query.single(world).unwrap().0
    };
    assert_eq!(
        text(world).alpha(),
        0.5,
        "the root's opacity fades its subtree"
    );

    // The app moves its UI; a rebuild (new class) keeps that and restyles.
    world.get_mut::<Node>(root).unwrap().left = Val::Px(20.0);
    world
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("extra", "wide");
    ui.settle();
    let node = ui.world_mut().get::<Node>(root).unwrap().clone();
    assert_eq!((node.width, node.left), (Val::Percent(90.0), Val::Px(20.0)));

    // A stylesheet without root rules: everything back to the app's.
    let plain = ui.load::<Stylesheet>("plain.css");
    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(plain);
    ui.settle();
    let world = ui.world_mut();
    let node = world.get::<Node>(root).unwrap().clone();
    assert_eq!(node.position_type, PositionType::Relative);
    assert_eq!((node.top, node.left), (Val::Auto, Val::Px(20.0)));
    assert_eq!(node.width, Val::Px(50.0));
    assert_eq!(node.flex_direction, FlexDirection::Row);
    assert_eq!(node.padding, UiRect::all(Val::Px(1.0)));
    assert_eq!(node.border_radius.top_left, CornerRadius::ZERO);
    assert_eq!(world.get::<BackgroundColor>(root).unwrap().0, blue);
    assert_eq!(world.get::<ZIndex>(root), Some(&ZIndex(0)));
    assert_eq!(world.get::<Pickable>(root), None);
    assert_eq!(node.height, Val::Px(30.0), "the template's own style stays");
    assert_eq!(text(world).alpha(), 0.5);
}

/// `HtmlAnchor` keeps an overlay beside its element on the requested side,
/// follows the element when it moves, stays inside the viewport once it has
/// a size, and is despawned with the element.
#[test]
fn anchored_overlay_follows_clamps_and_despawns() {
    let mut ui = TestUi::with_layout(
        "anchor",
        &[
            ("page.html", r#"<div id="a"></div>"#),
            (
                "style.css",
                "html { flex-direction: column; padding: 20px }
                 #a { width: 100px; height: 30px; margin-top: 100px }",
            ),
            (
                "far.css",
                "html { flex-direction: column; padding: 20px }
                 #a { width: 100px; height: 30px; margin-top: 100px; margin-left: 250px }",
            ),
        ],
        UVec2::new(320, 240),
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();
    let root = ui.root();
    let world = ui.world_mut();
    let a = element_by_id(world, root, "a");
    let overlay = world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(60.0),
                height: Val::Px(20.0),
                ..default()
            },
            HtmlAnchor::new(a, AnchorPlacement::Right).with_gap(4.0),
        ))
        .id();
    let rects = |ui: &mut TestUi| {
        ui.update(3);
        let world = ui.world_mut();
        (
            node_rect(world, a).unwrap(),
            node_rect(world, overlay).unwrap(),
        )
    };
    let (element, placed) = rects(&mut ui);
    assert_eq!(element, Rect::new(20.0, 120.0, 120.0, 150.0));
    assert_eq!(
        placed.min,
        Vec2::new(124.0, 120.0),
        "right of it, tops aligned"
    );

    for (placement, min) in [
        (AnchorPlacement::Below, Vec2::new(20.0, 154.0)),
        (AnchorPlacement::Above, Vec2::new(20.0, 96.0)),
        // Clamped inside the left edge (`inset <= view_w - width`): the
        // overlay overlaps the element instead of going off screen (the
        // same trade the far-edge Right/Below clamps make; the docs
        // promise clamping, not flipping).
        (AnchorPlacement::Left, Vec2::new(0.0, 120.0)),
    ] {
        ui.world_mut()
            .get_mut::<HtmlAnchor>(overlay)
            .unwrap()
            .placement = placement;
        assert_eq!(rects(&mut ui).1.min, min, "{placement:?}");
    }

    // The element moves to the right edge (a restyle keeps its entity): the
    // overlay follows, clamped to the 320px viewport.
    ui.world_mut()
        .get_mut::<HtmlAnchor>(overlay)
        .unwrap()
        .placement = AnchorPlacement::Right;
    let far = ui.load::<Stylesheet>("far.css");
    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(far);
    ui.settle();
    let (element, placed) = rects(&mut ui);
    assert_eq!(element.min.x, 270.0);
    assert_eq!(placed.min, Vec2::new(260.0, 120.0), "clamped to 320 - 60");

    ui.world_mut().entity_mut(root).despawn();
    ui.update(2);
    assert!(
        ui.world_mut().get_entity(overlay).is_err(),
        "despawned with its element"
    );
}

/// An anchored overlay whose element has no camera (no viewport to clamp
/// against) falls back to the overlay's measured size: `Above` sets
/// `bottom` from the element's top minus gap minus the overlay's height,
/// `Left` sets `right` from the element's left minus gap minus its width.
#[test]
fn anchored_overlay_without_a_viewport_uses_the_measured_size() {
    let mut ui = TestUi::with_layout(
        "anchor-no-viewport",
        &[("page.html", "<p>x</p>")],
        UVec2::new(320, 240),
    )
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();
    // A bare "element": laid-out components but no `ComputedUiTargetCamera`.
    let element = ui
        .world_mut()
        .spawn((
            ComputedNode {
                size: Vec2::new(40.0, 30.0),
                ..ComputedNode::DEFAULT
            },
            bevy::ui::UiGlobalTransform::from_translation(Vec2::new(120.0, 115.0)),
        ))
        .id();
    let overlay = ui
        .world_mut()
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(60.0),
                height: Val::Px(20.0),
                ..default()
            },
            HtmlAnchor::new(element, AnchorPlacement::Above).with_gap(4.0),
        ))
        .id();
    let insets = |ui: &mut TestUi| {
        ui.update(2);
        let world = ui.world_mut();
        let node = world.get::<Node>(overlay).unwrap();
        (node.left, node.top, node.right, node.bottom)
    };

    // Above the element, top-anchored by the measured height.
    assert_eq!(
        insets(&mut ui),
        (
            Val::Px(100.0),
            Val::Px(76.0), // min.y - gap - height
            Val::Auto,
            Val::Auto
        )
    );

    ui.world_mut()
        .get_mut::<HtmlAnchor>(overlay)
        .unwrap()
        .placement = AnchorPlacement::Left;
    // Left of the element, left-anchored by the measured width.
    assert_eq!(
        insets(&mut ui),
        (
            Val::Px(36.0), // min.x - gap - width
            Val::Px(100.0),
            Val::Auto,
            Val::Auto
        )
    );
}

/// Template dependencies: `{% include %}`s load with their template
/// (duplicate references load once, and a duplicate include still renders
/// twice), a dependency's reload re-renders the whole set, and a missing or
/// circular dependency fails the template's load.
#[test]
fn template_dependencies_load_reload_and_fail() {
    let mut ui = TestUi::new(
        "template-deps",
        &[
            (
                "pages/page.html",
                r#"{% extends "base.html" %}
{% block body %}{% include "part.html" %}{% include "part.html" %}{% endblock %}"#,
            ),
            (
                "pages/base.html",
                r#"<div id="frame">{% include "part.html" %}{% block body %}{% endblock %}</div>"#,
            ),
            ("pages/part.html", r#"<p id="part">{{ n }}</p>"#),
            ("cyclic.html", r#"{% include "cyclic.html" %}cycle"#),
            ("pages/broken.html", r#"{% include "missing.html" %}"#),
        ],
    )
    .spawn(
        "pages/page.html",
        TemplateContext::new().with("n", &1),
        Node::default(),
    );
    let cyclic = ui.load::<HtmlTemplate>("cyclic.html");
    let broken = ui.load::<HtmlTemplate>("pages/broken.html");
    ui.settle().assert_dump(
        r#"
html-ui
  div#frame
    p#part
      "1" default 16px #ffffff
    p#part
      "1" default 16px #ffffff
    p#part
      "1" default 16px #ffffff
"#,
    );

    // The cyclic include loaded (terminating on the seen set); the missing
    // dependency failed its template's load.
    let server = ui.world_mut().resource::<AssetServer>().clone();
    // A circular include fails its load (`CircularInclude`), like the
    // missing one.
    assert!(server.load_state(cyclic.id()).is_failed());
    assert!(server.load_state(broken.id()).is_failed());

    // Rewriting a dependency and reloading it re-renders the whole set.
    ui.write("pages/part.html", r#"<p id="part">{{ n }}!</p>"#);
    server.reload("pages/page.html");
    ui.settle().assert_dump(
        r#"
html-ui
  div#frame
    p#part
      "1!" default 16px #ffffff
    p#part
      "1!" default 16px #ffffff
    p#part
      "1!" default 16px #ffffff
"#,
    );
}

/// A content update reconciles instead of rebuilding: elements matched by
/// `id` (anywhere among their siblings) or by position (id-less ones) keep
/// their entities and what the app attached, with new text; an inserted
/// element is spawned in document order; a removed one is despawned; a block
/// whose run count changes keeps its entity with new spans.
#[test]
fn content_updates_reconcile_in_place() {
    #[derive(Component)]
    struct Wired;

    let page = r#"<p id="title">Hello {{ name }}</p>
{% if extra %}<p id="extra">Extra</p>{% endif %}
<div id="list">{% for item in items %}<p>{{ item }}</p>{% endfor %}</div>
<p id="rich">{% if bold %}<b>bold</b> {% endif %}tail</p>"#;
    let mut ui = TestUi::new(
        "reconcile",
        &[
            ("page.html", page),
            (
                "style.css",
                "html { color: #ffffff } b { font-weight: bold }",
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new()
            .with("name", "A")
            .with("extra", &false)
            .with("items", &[1, 2])
            .with("bold", &false),
        Node::default(),
    );
    ui.settle();
    let root = ui.root();
    let world = ui.world_mut();
    let [title, list, rich] = ["title", "list", "rich"].map(|id| element_by_id(world, root, id));
    let first_item = world.get::<Children>(list).unwrap()[0];
    for entity in [title, list, rich, first_item] {
        world.entity_mut(entity).insert(Wired);
    }
    let builds = ui.builds();

    let context = &mut *ui.world_mut().get_mut::<TemplateContext>(root).unwrap();
    context.insert("name", "B");
    context.insert("extra", &true);
    context.insert("items", &[1, 2, 3]);
    context.insert("bold", &true);
    ui.settle().assert_dump(
        r#"
html-ui
  p#title
    "Hello B" default 16px #ffffff
  p#extra
    "Extra" default 16px #ffffff
  div#list
    p
      "1" default 16px #ffffff
    p
      "2" default 16px #ffffff
    p
      "3" default 16px #ffffff
  p#rich
    "bold" default 16px #ffffff
    " tail" default 16px #ffffff
"#,
    );
    assert_eq!(ui.builds(), builds + 1, "one update");
    let world = ui.world_mut();
    for (entity, what) in [
        (title, "title"),
        (list, "list"),
        (rich, "rich (new runs)"),
        (first_item, "first list item"),
    ] {
        assert!(world.entity(entity).contains::<Wired>(), "{what} kept");
    }
    assert_eq!(element_by_id(world, root, "rich"), rich);

    // Removing them again despawns the extra element and the third item.
    let extra = element_by_id(world, root, "extra");
    let third = world.get::<Children>(list).unwrap()[2];
    let context = &mut *world.get_mut::<TemplateContext>(root).unwrap();
    context.insert("extra", &false);
    context.insert("items", &[1, 2]);
    ui.settle();
    let world = ui.world_mut();
    assert!(world.get_entity(extra).is_err() && world.get_entity(third).is_err());
    assert_eq!(world.get::<Children>(list).unwrap().len(), 2);
    assert!(world.entity(title).contains::<Wired>() && world.entity(list).contains::<Wired>());
}

/// The `style` attribute cascades as in CSS (over normal rules, under
/// `!important` ones unless itself `!important`), and `opacity` fades the
/// element's subtree — backgrounds and text, nested opacities multiplying.
/// A templated inline value updates the same entity in place.
#[test]
fn style_attribute_and_opacity() {
    let page = r#"<div id="box" style="width: {{ w }}%; background-color: #ff0000; overflow-y: scroll"><p>x</p></div>
<div id="half" class="half"><p id="green" style="color: #00ff00">y</p><div id="quarter" style="opacity: 0.5"><p id="inner">z</p></div></div>
<p id="rule" class="imp" style="color: #0000ff">a</p>
<p id="inline" class="imp" style="color: #0000ff !important">b</p>"#;
    let css = "p { color: #ffffff } #box { width: 10% } \
               .half { opacity: 0.5; background-color: #ffffff } \
               .imp { color: #ff00ff !important }";
    let mut ui = TestUi::new("inline", &[("page.html", page), ("style.css", css)])
        .stylesheet("style.css")
        .spawn(
            "page.html",
            TemplateContext::new().with("w", &40),
            Node::default(),
        );
    ui.settle();
    let root = ui.root();
    let world = ui.world_mut();
    let get = |world: &mut World, id: &str| element_by_id(world, root, id);
    let text_color = |world: &mut World, id: &str| {
        let block = get(world, id);
        world.get::<TextColor>(block).unwrap().0
    };
    let boxed = get(world, "box");
    assert_eq!(
        world.get::<Node>(boxed).unwrap().width,
        Val::Percent(40.0),
        "inline over #id"
    );
    assert_eq!(
        world.get::<BackgroundColor>(boxed).unwrap().0,
        Color::srgb(1.0, 0.0, 0.0)
    );
    let half = get(world, "half");
    assert_eq!(
        world.get::<BackgroundColor>(half).unwrap().0,
        Color::srgba(1.0, 1.0, 1.0, 0.5)
    );
    assert_eq!(text_color(world, "green"), Color::srgba(0.0, 1.0, 0.0, 0.5));
    assert_eq!(
        text_color(world, "inner"),
        Color::srgba(1.0, 1.0, 1.0, 0.25),
        "0.5 × 0.5"
    );
    assert_eq!(
        text_color(world, "rule"),
        Color::srgb(1.0, 0.0, 1.0),
        "!important rule wins"
    );
    assert_eq!(
        text_color(world, "inline"),
        Color::srgb(0.0, 0.0, 1.0),
        "inline !important wins"
    );

    world
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("w", &75);
    ui.settle();
    let world = ui.world_mut();
    assert_eq!(get(world, "box"), boxed, "updated in place");
    let node = world.get::<Node>(boxed).unwrap();
    assert_eq!(node.width, Val::Percent(75.0));
    assert_eq!(
        node.overflow.y,
        OverflowAxis::Scroll,
        "CSS overflow survives updates"
    );
}

/// Focus on something that isn't UI — Bevy's input dispatch focuses the
/// primary window at startup — counts as nothing focused: the `autofocus`
/// element takes it (bug_0022). Focus on another UI node the app owns is
/// left alone.
#[test]
fn focus_on_the_window_yields_to_autofocus() {
    let page = r#"<div id="a" data-on-click="a"><p>A</p></div>
<div id="b" autofocus data-on-click="b"><p>B</p></div>"#;
    let mut ui = TestUi::new("focus-window", &[("page.html", page)]).spawn(
        "page.html",
        TemplateContext::new(),
        Node::default(),
    );
    ui.settle();
    let root = ui.root();
    let world = ui.world_mut();
    let b = element_by_id(world, root, "b");
    let window = world.spawn(Window::default()).id();
    world
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(window, bevy::input_focus::FocusCause::Navigated);
    ui.update(3);
    assert_eq!(focused(ui.world_mut()), Some(b), "window focus → autofocus");

    let world = ui.world_mut();
    let app_button = world.spawn(Node::default()).id();
    world
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(app_button, bevy::input_focus::FocusCause::Navigated);
    ui.update(3);
    assert_eq!(
        focused(ui.world_mut()),
        Some(app_button),
        "app UI focus kept"
    );
}

/// Templates compose: a page extends a base template, uses a component from a
/// library it includes (Tera 2 components are shared by every template in
/// the set; including a definitions-only file renders nothing), and includes
/// a part — all by paths relative to its own file.
#[test]
fn templates_extend_import_and_include() {
    let mut ui = TestUi::new(
        "template-refs",
        &[
            (
                "pages/page.html",
                r#"{% extends "base.html" %}
{% block body %}{% include "../lib/ui.html" %}{{ <ui.button id="go" label={label} /> }}{% include "part.html" %}{% endblock %}"#,
            ),
            (
                "pages/base.html",
                r#"<div id="frame">{% block body %}{% endblock %}</div>"#,
            ),
            (
                "pages/part.html",
                r#"<p id="part">{{ label }} part</p>"#,
            ),
            (
                "lib/ui.html",
                r#"{% component ui.button(id, label) %}<div id="{{ id }}" class="button"><p>{{ label }}</p></div>{% endcomponent ui.button %}"#,
            ),
        ],
    )
    .spawn(
        "pages/page.html",
        TemplateContext::new().with("label", "Go"),
        Node::default(),
    );
    ui.settle().assert_dump(
        r#"
html-ui
  div#frame
    div#go.button
      p
        "Go" default 16px #ffffff
    p#part
      "Go part" default 16px #ffffff
"#,
    );
}

/// `HtmlWorldAnchor` puts its root's pivot on the target's projection,
/// hides it behind the camera, off screen or with an invisible target,
/// reports distance and visibility, and goes with its target. (The harness
/// camera keeps an identity projection at the origin: world (x, y) maps to
/// viewport ((x + 1) / 2 × 320, (1 − y) / 2 × 240).)
#[test]
fn world_anchor_projects_hides_and_despawns() {
    let mut ui = TestUi::with_layout("world-anchor", &[], UVec2::new(320, 240));
    let world = ui.world_mut();
    let place = |world: &mut World, target: Entity, at: Vec3| {
        world.entity_mut(target).insert((
            Transform::from_translation(at),
            GlobalTransform::from_translation(at),
        ));
    };
    let target = world.spawn(Visibility::Inherited).id();
    place(world, target, Vec3::new(0.25, 0.5, 0.0));
    let overlay = world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(20.0),
                height: Val::Px(10.0),
                ..default()
            },
            HtmlWorldAnchor::new(target).with_offset(Vec3::new(0.25, 0.0, 0.0)),
        ))
        .id();
    let state = |ui: &mut TestUi| {
        ui.update(3);
        let world = ui.world_mut();
        let node = world.get::<Node>(overlay).unwrap();
        (
            (node.left, node.top),
            *world.get::<Visibility>(overlay).unwrap(),
            *world.get::<HtmlWorldAnchorView>(overlay).unwrap(),
        )
    };
    let (position, visibility, view) = state(&mut ui);
    assert_eq!(
        position,
        (Val::Px(230.0), Val::Px(50.0)),
        "(240, 60) minus the bottom-center pivot of 20×10"
    );
    assert_eq!(visibility, Visibility::Inherited);
    assert!(view.on_screen);
    assert!((view.distance - (0.5f32.powi(2) * 2.0).sqrt()).abs() < 1e-5);

    for (at, why) in [
        (Vec3::new(2.0, 0.0, 0.0), "off screen"),
        (Vec3::new(0.0, 0.0, -0.5), "behind the camera"),
    ] {
        place(ui.world_mut(), target, at);
        let (_, visibility, view) = state(&mut ui);
        assert_eq!(visibility, Visibility::Hidden, "{why}");
        assert!(!view.on_screen, "{why}");
    }

    let world = ui.world_mut();
    place(world, target, Vec3::ZERO);
    world.entity_mut(target).insert(Visibility::Hidden);
    let (_, visibility, view) = state(&mut ui);
    assert_eq!(visibility, Visibility::Hidden, "invisible target");
    assert!(view.on_screen);

    ui.world_mut().entity_mut(target).despawn();
    ui.update(2);
    assert!(
        ui.world_mut().get_entity(overlay).is_err(),
        "despawned with its target"
    );
}

/// `HtmlWorldAnchor::with_pivot` offsets the overlay by a fraction of its
/// own size, `with_camera` projects through that camera instead of the
/// default, and with no camera to project through the overlay hides.
#[test]
fn world_anchor_pivot_camera_override_and_no_camera_hides() {
    let mut ui = TestUi::with_layout("world-anchor-pivot", &[], UVec2::new(320, 240));
    let world = ui.world_mut();
    let target = world.spawn(Visibility::Inherited).id();
    world.entity_mut(target).insert((
        Transform::from_translation(Vec3::new(0.25, 0.5, 0.0)),
        GlobalTransform::from_translation(Vec3::new(0.25, 0.5, 0.0)),
    ));
    // A second camera, translated: projecting through it moves the overlay.
    let other = world
        .spawn((
            Camera2d,
            Camera {
                computed: bevy::camera::ComputedCameraValues {
                    target_info: Some(bevy::camera::RenderTargetInfo {
                        physical_size: UVec2::new(320, 240),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                ..default()
            },
            bevy::camera::RenderTarget::None {
                size: UVec2::new(320, 240),
            },
            GlobalTransform::from_translation(Vec3::new(0.5, 0.0, 0.0)),
        ))
        .id();
    let overlay = world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(20.0),
                height: Val::Px(10.0),
                ..default()
            },
            HtmlWorldAnchor::new(target)
                .with_pivot(Vec2::new(0.5, 1.0))
                .with_camera(other),
        ))
        .id();
    let placed = |ui: &mut TestUi| {
        ui.update(3);
        let world = ui.world_mut();
        let node = world.get::<Node>(overlay).unwrap();
        (
            node.left,
            node.top,
            *world.get::<Visibility>(overlay).unwrap(),
        )
    };

    // Through `other` at (0.5, 0, 0), world (0.25, 0.5) lands on viewport
    // (120, 60); the pivot (0.5, 1) shifts by a tenth and a half of the
    // overlay's 20×10.
    assert_eq!(
        placed(&mut ui),
        (Val::Px(110.0), Val::Px(50.0), Visibility::Inherited)
    );

    // Despawning the anchor's camera leaves nothing to project through.
    ui.world_mut().despawn(other);
    ui.update(3);
    assert_eq!(
        placed(&mut ui),
        (Val::Px(110.0), Val::Px(50.0), Visibility::Hidden)
    );
}

/// An in-place update writes only what differs: the element whose value
/// changed gets a new `Node`, the others keep theirs untouched (no change
/// detection), so layout and text systems skip them.
#[test]
fn updates_touch_only_changed_components() {
    #[derive(Resource, Default)]
    struct Changed(Vec<String>);

    let page = r#"<div id="a" style="width: {{ w }}px"><p>A</p></div><div id="b" style="width: 10px"><p>B {{ t }}</p></div>"#;
    let mut ui = TestUi::new("touch-changed", &[("page.html", page)]).spawn(
        "page.html",
        TemplateContext::new().with("w", &10).with("t", "x"),
        Node::default(),
    );
    ui.app_mut().init_resource::<Changed>().add_systems(
        Last,
        |nodes: Query<(&HtmlElement, Ref<Node>)>,
         texts: Query<(&ChildOf, Ref<TextSpan>)>,
         elements: Query<&HtmlElement>,
         mut changed: ResMut<Changed>| {
            for (element, node) in &nodes {
                if node.is_changed() && !node.is_added() {
                    changed.0.push(format!(
                        "node {}",
                        element.id.as_deref().unwrap_or(&element.tag)
                    ));
                }
            }
            for (parent, span) in &texts {
                if span.is_changed() && !span.is_added() {
                    let owner = elements
                        .get(parent.parent())
                        .map(|e| e.tag.clone())
                        .unwrap_or_default();
                    changed.0.push(format!("span in {owner}"));
                }
            }
        },
    );
    ui.settle();
    ui.update(2);
    ui.world_mut().resource_mut::<Changed>().0.clear();
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("w", &20);
    ui.settle();
    ui.update(1);
    let changed = std::mem::take(&mut ui.world_mut().resource_mut::<Changed>().0);
    assert_eq!(changed, ["node a"], "only a's Node is written");
}

/// Without an `IsDefaultUiCamera` marker, `HtmlWorldAnchor` projects with
/// Bevy's default UI camera (the window camera), like UI does (bug_0027).
#[test]
fn world_anchor_uses_the_implicit_default_ui_camera() {
    let mut ui = TestUi::with_layout("world-anchor-implicit", &[], UVec2::new(320, 240));
    let world = ui.world_mut();
    let marked: Vec<Entity> = world
        .query_filtered::<Entity, With<bevy::ui::IsDefaultUiCamera>>()
        .iter(world)
        .collect();
    for camera in marked {
        world
            .entity_mut(camera)
            .remove::<bevy::ui::IsDefaultUiCamera>()
            .insert(bevy::camera::RenderTarget::Window(
                bevy::window::WindowRef::Primary,
            ));
    }
    let window = world
        .spawn((
            Window {
                resolution: bevy::window::WindowResolution::new(320, 240),
                ..default()
            },
            bevy::window::PrimaryWindow,
        ))
        .id();
    let _ = window;
    let target = world
        .spawn((
            Visibility::Inherited,
            Transform::default(),
            GlobalTransform::default(),
        ))
        .id();
    let overlay = world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            HtmlWorldAnchor::new(target),
        ))
        .id();
    ui.update(3);
    let world = ui.world_mut();
    assert!(
        world.get::<HtmlWorldAnchorView>(overlay).unwrap().on_screen,
        "projected with the window's camera"
    );
    assert_eq!(
        *world.get::<Visibility>(overlay).unwrap(),
        Visibility::Inherited
    );
}

/// Untyped loads (folders, `load_untyped`) pick bevy_markup's loaders by file
/// extension: `.css`, `.html`/`.htm`, `.slice.ron`.
#[test]
fn loaders_are_found_by_extension() {
    use std::any::TypeId;

    let mut ui = TestUi::new(
        "extensions",
        &[
            ("a.css", "p { color: red }"),
            ("b.html", "<p>b</p>"),
            ("c.htm", "<p>c</p>"),
            ("e.md", "# md\n"),
            ("f.markdown", "md\n"),
            (
                "d.slice.ron",
                r#"(image: "frame.png", border: (left: 1, right: 1, top: 1, bottom: 1))"#,
            ),
        ],
    );
    let expected = [
        ("a.css", TypeId::of::<Stylesheet>()),
        ("b.html", TypeId::of::<HtmlTemplate>()),
        ("c.htm", TypeId::of::<HtmlTemplate>()),
        ("e.md", TypeId::of::<HtmlTemplate>()),
        ("f.markdown", TypeId::of::<HtmlTemplate>()),
        (
            "d.slice.ron",
            TypeId::of::<bevy_markup::nine_slice::NineSlice>(),
        ),
    ];
    let handles: Vec<_> = expected
        .iter()
        .map(|(path, _)| {
            ui.world_mut()
                .resource::<AssetServer>()
                .load_builder()
                .load_untyped(*path)
        })
        .collect();
    for _ in 0..3000 {
        ui.update(1);
        let loaded = ui
            .world_mut()
            .resource::<Assets<bevy::asset::LoadedUntypedAsset>>();
        if handles.iter().all(|handle| loaded.contains(handle)) {
            break;
        }
    }
    let loaded = ui
        .world_mut()
        .resource::<Assets<bevy::asset::LoadedUntypedAsset>>();
    for ((path, type_id), handle) in expected.iter().zip(&handles) {
        let asset = loaded
            .get(handle)
            .unwrap_or_else(|| panic!("{path} didn't load untyped"));
        assert_eq!(asset.handle.type_id(), *type_id, "{path}");
    }
}

/// Markdown templates (ADR 0014): a `.md` page renders through Tera +
/// pulldown-cmark into the same DOM model — headings, emphasis styled by
/// CSS, lists, an `<img>`, and raw HTML with `data-on-click` passing
/// through unclassed while the constructed elements carry
/// `class="markdown"`. Autoescaping is forced for `.md`: a context
/// value's markup arrives as *text*, not tags.
#[test]
fn markdown_templates_render_and_escape() {
    let ui = TestUi::new(
        "markdown",
        &[
            (
                "page.md",
                "# {{ title }}\n\nHello *world* and {{ value }}.\n\n- first\n- second\n\n![seal](frame.png)\n\n<div data-on-click=\"buy\">Buy</div>\n",
            ),
            (
                "style.css",
                "h1 { color: #ff0000; font-size: 30px }\n\
                 em { color: #00ff00 }\n\
                 li { color: #ffffff }",
            ),
        ],
    );
    let mut ui = ui.stylesheet("style.css").spawn(
        "page.md",
        TemplateContext::new()
            .with("title", "Journal")
            .with("value", "<b>not tags</b>"),
        Node::default(),
    );
    ui.settle().assert_dump(
        r#"
html-ui
  h1.markdown
    "Journal" default 30px #ff0000
  p.markdown
    "Hello " default 16px #ffffff
    "world" default 16px #00ff00
    " and <b>not tags</b>." default 16px #ffffff
  ul.markdown
    li.markdown margin=0,0,0,12
      "• " default 16px #ffffff
      "first" default 16px #ffffff
    li.markdown margin=0,0,0,12
      "• " default 16px #ffffff
      "second" default 16px #ffffff
  p.markdown
    "frame.png" img 32x24
  div focus on=Click:buy
    -
      "Buy" default 16px #ffffff
"#,
    );
}

/// Markdown in Fluent values (ADR 0014): a bundle whose manifest sets
/// `markdown: true` converts its values (`**bold**` → `<strong>`); without
/// the flag, values stay HTML as written.
#[test]
fn markdown_bundles_opt_in_per_manifest() {
    let mut ui = TestUi::new(
        "markdown-l10n",
        &[
            ("page.html", "<p data-l10n-id=\"m\">fallback</p>"),
            (
                "md.ftl.ron",
                "(locale: \"en-US\", resources: [\"m.ftl\"], markdown: true)",
            ),
            (
                "plain.ftl.ron",
                "(locale: \"en-US\", resources: [\"m.ftl\"])",
            ),
            ("m.ftl", "m = struck **hard** with *flair*.\n"),
            (
                "style.css",
                "html { color: #ffffff; font-family: serif }\n\
                 strong { font-weight: bold }\n\
                 em { font-style: italic }",
            ),
        ],
    )
    .stylesheet("style.css")
    .locale("md.ftl.ron")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.world_mut()
        .resource_mut::<FontFamilies>()
        .insert("System Serif", FontFaces::new(FontSource::serif()));
    ui.world_mut()
        .resource_mut::<FontFamilies>()
        .set_generic(GenericFamily::Serif, "System Serif");
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "struck " Serif 16px #ffffff
    "hard" Serif 16px #ffffff bold
    " with " Serif 16px #ffffff
    "flair" Serif 16px #ffffff italic
    "." Serif 16px #ffffff
"#,
    );
    // The same resource without the manifest flag: the value is HTML as
    // written (the `**` are text).
    let plain = ui.load::<LocaleBundle>("plain.ftl.ron");
    ui.world_mut().resource_mut::<ActiveLocale>().0 = Some(plain);
    ui.settle_quiet();
    ui.update(10);
    ui.assert_dump(
        r#"
html-ui
  p
    "struck **hard** with *flair*." Serif 16px #ffffff
"#,
    );
}

/// `<input type="text">` (the `text-input` feature): a block element with
/// Bevy 0.20's editable-text widget — the initial value from the `value`
/// attribute, the text styled by the cascade, the box by CSS. The element
/// is focusable (press-to-focus, navigation). `maxlength` maps to the
/// widget's `max_characters` (edits past it are ignored; `placeholder`
/// is not supported — ADR 0022/0023).
#[cfg(feature = "text-input")]
#[test]
fn text_input_element_maps_to_the_widget() {
    let mut ui = TestUi::new(
        "text-input",
        &[
            (
                "page.html",
                r#"<input type="text" value="hello" maxlength="12" class="field">"#,
            ),
            (
                "style.css",
                "html { color: #ffffff; font-family: serif; font-size: 20px }\n\
                 .field { width: 200px; padding: 4px; background-color: #222222 }",
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    // `settle()` never stabilizes here: the widget's cursor/edit systems
    // mark the text changed every frame. Bounded updates instead.
    ui.update(30).assert_dump(
        r#"
html-ui
  input.field padding=4,4,4,4 w=200 focus bg=#222222
    "hello" default 20px #ffffff
"#,
    );
    // `maxlength` maps to the widget's `max_characters` (ADR 0023).
    let world = ui.world_mut();
    let field = world
        .query::<(Entity, &bevy::ui_widgets::TextInput)>()
        .iter(world)
        .next()
        .expect("the field")
        .0;
    assert_eq!(
        world
            .get::<bevy::text::EditableText>(field)
            .unwrap()
            .max_characters,
        Some(12)
    );
}

/// HTML's `<input type>` defaults to `text` for a missing *and* an
/// invalid value; only the other known types build no field
/// (review_0003 G36: `type=""` and a misspelt type built nothing).
#[cfg(feature = "text-input")]
#[test]
fn invalid_input_types_are_text_fields() {
    for (kind, field) in [
        ("", true),
        ("txet", true),
        ("TEXT", true),
        ("checkbox", false),
        ("Password", false),
    ] {
        let page = format!(r#"<input type="{kind}" value="v">"#);
        let mut ui = TestUi::new("input-types", &[("page.html", page.as_str())]).spawn(
            "page.html",
            TemplateContext::new(),
            Node::default(),
        );
        ui.update(10);
        let world = ui.world_mut();
        let fields = world
            .query::<&bevy::ui_widgets::TextInput>()
            .iter(world)
            .count();
        assert_eq!(fields == 1, field, "type={kind:?}");
    }
}

/// `type` values are ASCII case-insensitive (HTML enumerated attributes):
/// `type="Text"` builds a field.
#[cfg(feature = "text-input")]
#[test]
fn input_type_is_case_insensitive() {
    let mut ui = TestUi::new(
        "input-type-case",
        &[
            ("page.html", r#"<input type="Text" class="field">"#),
            (
                "style.css",
                "html { color: #ffffff } .field { width: 100px }",
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.update(30);
    let world = ui.world_mut();
    let fields = world
        .query_filtered::<Entity, With<bevy::ui_widgets::TextInput>>()
        .iter(world)
        .count();
    assert_eq!(fields, 1, "type=Text didn't build a field");
}

/// (`--a: var(--brand)`) restyles when it changes: the vars hash used to
/// track only directly referenced names.
#[test]
fn transitive_var_change_restyles() {
    let mut ui = TestUi::new(
        "transitive-var",
        &[
            ("page.html", "<p>colored</p>"),
            (
                "style.css",
                "html { color: #ffffff } p { --a: var(--brand); color: var(--a) }",
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new().with("--brand", &"#ff0000"),
        Node::default(),
    );
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "colored" default 16px #ff0000
"#,
    );
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("--brand", &"#00ff00");
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "colored" default 16px #00ff00
"#,
    );
}

/// Other `type`s stay unsupported (ADR 0016): only a text (or missing)
/// `type` builds the field; the others are dropped like any unsupported
/// empty inline leaf — no field, no element. The `type` attribute is the
/// real attribute, not a `data-type`.
#[cfg(feature = "text-input")]
#[test]
fn non_text_input_types_are_not_fields() {
    let mut ui = TestUi::new(
        "input-types",
        &[
            (
                "page.html",
                r#"<input type="checkbox"><input type="password"><input>"#,
            ),
            ("style.css", "html { color: #ffffff }"),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.update(20);
    let world = ui.world_mut();
    let mut query = world.query::<(
        &bevy_markup::html::HtmlElement,
        Option<&bevy::text::EditableText>,
        Option<&bevy::ui_widgets::TextInput>,
        Option<&Focusable>,
    )>();
    let inputs: Vec<_> = query
        .iter(world)
        .map(|(element, text, widget, focus)| {
            (
                element.tag.clone(),
                text.is_some(),
                widget.is_some(),
                focus.is_some(),
            )
        })
        .collect();
    assert_eq!(
        inputs,
        vec![("input".to_owned(), true, true, true)],
        "the checkbox and password inputs must not become fields"
    );
}

/// A kept `<input>` is updated in place: the text follows the `value`
/// attribute only when the attribute changed (the typed text is app
/// state), and the cascade's font/color restyle it.
#[cfg(feature = "text-input")]
#[test]
fn kept_input_follows_value_and_styles() {
    let mut ui = TestUi::new(
        "input-update",
        &[
            (
                "page.html",
                r#"<p>{{ n }}</p><input type="text" class="field" value="{{ v }}">"#,
            ),
            (
                "style.css",
                "html { color: #ffffff; font-size: 20px } .field { color: #00ffff }",
            ),
            (
                "b.css",
                "html { color: #ffffff; font-size: 20px } .field { color: #ff00ff; font-size: 30px }",
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new().with("n", &1).with("v", &"one"),
        Node::default(),
    );
    ui.update(30);
    // The `value` attribute changed: the field's text follows it.
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("v", &"two");
    ui.update(30);
    let entity = {
        let world = ui.world_mut();
        world
            .query::<(Entity, &bevy::ui_widgets::TextInput)>()
            .iter(world)
            .map(|(entity, _)| entity)
            .next()
            .expect("the text field")
    };
    let text = ui
        .world_mut()
        .get::<bevy::text::EditableText>(entity)
        .unwrap()
        .value()
        .to_string();
    assert_eq!(text, "two", "the value attribute change did not land");

    // The attribute is unchanged; typed text survives a re-render.
    ui.world_mut()
        .entity_mut(entity)
        .insert(bevy::text::EditableText::new("typed by the user"));
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &2);
    ui.update(30);
    let text = ui
        .world_mut()
        .get::<bevy::text::EditableText>(entity)
        .unwrap()
        .value()
        .to_string();
    assert_eq!(
        text, "typed by the user",
        "the re-render clobbered the typed text"
    );

    // A restyle updates the field's cascade styling.
    let b_sheet = ui.load::<Stylesheet>("b.css");
    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(b_sheet);
    ui.update(30);
    let color = ui
        .world_mut()
        .get::<bevy::prelude::TextColor>(entity)
        .expect("the field kept its TextColor")
        .0;
    assert_eq!(color, bevy::color::Color::srgb(1.0, 0.0, 1.0));
}

/// The text-input branch must style the field from the same box the
/// normal path computed: the `resolve_names` call resolves its grid
/// placement, and dropping it lost a *named* `grid-row`/`-column`
/// (numeric placement maps without the names).
#[cfg(feature = "text-input")]
#[test]
fn text_input_keeps_its_grid_placement() {
    let mut ui = TestUi::new(
        "input-grid",
        &[
            (
                "page.html",
                r#"<div class="grid"><input type="text" style="grid-row: mid"></div>"#,
            ),
            (
                "style.css",
                "html { color: #ffffff } .grid { display: grid; grid-template-rows: [top] auto [mid] auto auto }",
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.update(30);
    let world = ui.world_mut();
    let rows: Vec<_> = world
        .query::<(&bevy_markup::html::HtmlElement, &Node)>()
        .iter(world)
        .filter(|(element, _)| element.tag == "input")
        .map(|(_, node)| (node.grid_row.get_start(), node.grid_row.get_span()))
        .collect();
    assert_eq!(
        rows,
        vec![(Some(2), None)],
        "the input lost its named grid placement"
    );
}

/// A root `calc()` resolves against the viewport — including on a
/// top-level root, which the resolution pass skipped entirely — and stops
/// resolving once the declaration goes (the `CalcSized` component was
/// never removed, so the old calc kept overwriting the width).
#[test]
fn root_calc_applies_and_stops() {
    let mut ui = TestUi::with_layout(
        "root-calc",
        &[
            ("page.html", "<p>x</p>"),
            ("a.css", "html { width: calc(50% - 20px) }"),
            ("b.css", "html { padding: 5px }"),
        ],
        UVec2::new(640, 480),
    )
    .stylesheet("a.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();
    let root = ui.root();
    let width = ui.world_mut().get::<Node>(root).unwrap().width;
    assert_eq!(width, Val::Px(300.0), "the root's calc() did not resolve");

    // A theme that drops the calc: the width goes back to the app's node.
    let b_sheet = ui.load::<Stylesheet>("b.css");
    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(b_sheet);
    ui.settle();
    let width = ui.world_mut().get::<Node>(root).unwrap().width;
    assert_eq!(
        width,
        Val::Auto,
        "the dropped calc() kept overwriting the width"
    );
}

/// `calc()` on a direct child of a top-level root resolves (review_0004
/// H07: the pass required the parent to have a parent of its own, so
/// every root-level element's `calc()` stayed at its `Auto` placeholder
/// — containers, blocks and stylesheet rules alike).
#[test]
fn calc_resolves_on_direct_children_of_a_top_level_root() {
    let page = r#"<div id="div" style="width: calc(50% - 10px); height: 5px"></div>
        <p id="p" style="width: calc(50vw - 10px)">x</p><p id="rule" class="half">x</p>"#;
    let mut ui = TestUi::with_layout(
        "calc-root-children",
        &[
            ("page.html", page),
            ("style.css", ".half { width: calc(25% + 10px) }"),
        ],
        UVec2::new(400, 300),
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new(),
        Node {
            width: Val::Px(400.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    ui.settle();
    ui.update(3);
    let root = ui.root();
    let world = ui.world_mut();
    for (id, width) in [("div", 190.0), ("p", 190.0), ("rule", 110.0)] {
        let element = element_by_id(world, root, id);
        assert_eq!(
            world.get::<Node>(element).unwrap().width,
            Val::Px(width),
            "#{id}"
        );
    }
}

/// Viewport units in a nested element's mixed `calc()` measure the
/// viewport, not the top-level root's box (review_0004 H08: under a
/// 100px root in a 400px viewport, `calc(50vw - 10px)` was 40px while a
/// plain `50vw` — Bevy's `Vw` — was 200px).
#[test]
fn viewport_units_in_nested_calc_measure_the_viewport() {
    let page = r#"<div><p id="calc" style="width: calc(50vw - 10px)">x</p>
        <p id="plain" style="width: 50vw">x</p></div>"#;
    let mut ui = TestUi::with_layout(
        "calc-vw-nested",
        &[("page.html", page)],
        UVec2::new(400, 300),
    )
    .spawn(
        "page.html",
        TemplateContext::new(),
        Node {
            width: Val::Px(100.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    ui.settle();
    ui.update(3);
    let root = ui.root();
    let world = ui.world_mut();
    let calc = element_by_id(world, root, "calc");
    let plain = element_by_id(world, root, "plain");
    assert_eq!(world.get::<Node>(calc).unwrap().width, Val::Px(190.0));
    let plain_width = common::node_rect(world, plain).unwrap().width();
    assert_eq!(plain_width, 200.0, "the control: Bevy's own Vw");
}

/// Without any `IsDefaultUiCamera` marker, the root's calc viewport is
/// the window camera Bevy UI renders with (`DefaultUiCamera`), not the
/// last camera iterated (review_0003 G10: a minimap camera spawned after
/// the UI camera made `calc(50% - 20px)` resolve to 30px, not 300px).
#[test]
fn root_calc_without_a_marked_camera_uses_the_window_camera() {
    let mut ui = TestUi::with_pointer(
        "root-calc-unmarked",
        &[
            ("page.html", "<p>x</p>"),
            ("a.css", "html { width: calc(50% - 20px) }"),
        ],
        UVec2::new(640, 480),
    )
    .stylesheet("a.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    let world = ui.world_mut();
    let marked = world
        .query_filtered::<Entity, With<bevy::ui::IsDefaultUiCamera>>()
        .single(world)
        .unwrap();
    world
        .entity_mut(marked)
        .remove::<bevy::ui::IsDefaultUiCamera>();
    world.spawn((
        bevy::camera::Camera2d,
        bevy::camera::Camera {
            computed: bevy::camera::ComputedCameraValues {
                target_info: Some(bevy::camera::RenderTargetInfo {
                    physical_size: UVec2::new(100, 100),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        },
        bevy::camera::RenderTarget::None {
            size: UVec2::new(100, 100),
        },
    ));
    ui.settle();
    ui.update(3);
    let root = ui.root();
    let width = ui.world_mut().get::<Node>(root).unwrap().width;
    assert_eq!(
        width,
        Val::Px(300.0),
        "the minimap camera became the viewport"
    );
}

/// A second camera without the `IsDefaultUiCamera` marker must not
/// become the root's calc viewport: the default UI camera is preferred,
/// and a root's own `UiTargetCamera` wins over both.
#[test]
fn root_calc_resolves_against_the_ui_camera() {
    let mut ui = TestUi::with_layout(
        "root-calc-camera",
        &[
            ("page.html", "<p>x</p>"),
            ("a.css", "html { width: calc(50% - 20px) }"),
        ],
        UVec2::new(640, 480),
    )
    .stylesheet("a.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    // A small non-UI camera sorts before the default one under the old
    // `min_by_key(is_some)` (it has no marker), stealing the viewport:
    // 50% - 20px of 100px was 30px instead of 300px.
    let world = ui.world_mut();
    world.spawn((
        bevy::camera::Camera2d,
        bevy::camera::Camera {
            computed: bevy::camera::ComputedCameraValues {
                target_info: Some(bevy::camera::RenderTargetInfo {
                    physical_size: UVec2::new(100, 100),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        },
        bevy::camera::RenderTarget::None {
            size: UVec2::new(100, 100),
        },
    ));
    ui.settle();
    let root = ui.root();
    let width = ui.world_mut().get::<Node>(root).unwrap().width;
    assert_eq!(
        width,
        Val::Px(300.0),
        "the wrong camera became the viewport"
    );

    // An explicit `UiTargetCamera` on the root wins over the default.
    let second = {
        let world = ui.world_mut();
        world
            .spawn((
                bevy::camera::Camera2d,
                bevy::camera::Camera {
                    computed: bevy::camera::ComputedCameraValues {
                        target_info: Some(bevy::camera::RenderTargetInfo {
                            physical_size: UVec2::new(200, 200),
                            scale_factor: 1.0,
                        }),
                        ..default()
                    },
                    ..default()
                },
                bevy::camera::RenderTarget::None {
                    size: UVec2::new(200, 200),
                },
                bevy::ui::IsDefaultUiCamera,
            ))
            .id()
    };
    ui.world_mut()
        .entity_mut(root)
        .insert(bevy::ui::UiTargetCamera(second));
    // The calc pass resolves per frame; it is not a build.
    ui.settle_quiet();
    let width = ui.world_mut().get::<Node>(root).unwrap().width;
    assert_eq!(width, Val::Px(80.0), "the targeted camera's viewport won");
}

/// A restyle that respawns spans (runs merging under a new sheet) must
/// fire `HtmlUiBuilt`, not `HtmlUiRestyled`: the promise behind the
/// restyled event is that every element was kept, and link spans are new
/// entities the app's `HtmlUiBuilt` wiring has to see.
#[test]
fn restyle_that_respawns_spans_reports_a_build() {
    #[derive(Resource, Default)]
    struct Counts {
        built: usize,
        restyled: usize,
    }

    let mut ui = TestUi::new(
        "span-respawn",
        &[
            ("page.html", "<p>Plain <b>bold</b> text</p>"),
            ("a.css", "html { color: #ffffff } b { color: #ff0000 }"),
            ("b.css", "html { color: #ffffff }"),
        ],
    )
    .stylesheet("a.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.world_mut().insert_resource(Counts::default());
    ui.app_mut()
        .add_observer(|_: On<HtmlUiBuilt>, mut counts: ResMut<Counts>| {
            counts.built += 1;
        });
    ui.app_mut()
        .add_observer(|_: On<HtmlUiRestyled>, mut counts: ResMut<Counts>| {
            counts.restyled += 1;
        });
    ui.settle();
    let (built, restyled) = {
        let counts = ui.world_mut().resource::<Counts>();
        (counts.built, counts.restyled)
    };

    // The merged runs change the span count: the restyle respawns them.
    let b_sheet = ui.load::<Stylesheet>("b.css");
    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(b_sheet);
    ui.settle();
    let counts = ui.world_mut().resource::<Counts>();
    assert_eq!(
        counts.built,
        built + 1,
        "the respawn was not reported as a build"
    );
    assert_eq!(
        counts.restyled, restyled,
        "a span respawn must not fire HtmlUiRestyled"
    );
}

/// Syncing Bevy's `RemSize` to the root rule's font-size (16px vs Bevy's
/// 20px default) must not queue a restyle: Bevy's layout re-resolves
/// `Val::Rem`s itself, and a spurious `HtmlUiRestyled` broke the "kept"
/// promise right after the first build.
#[test]
fn rem_size_sync_does_not_restyle() {
    #[derive(Resource, Default)]
    struct Counts {
        built: usize,
        restyled: usize,
    }

    let mut ui = TestUi::new(
        "rem-sync",
        &[
            ("page.html", "<p>x</p>"),
            ("style.css", "html { color: #ffffff }"),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.world_mut().insert_resource(Counts::default());
    ui.app_mut()
        .add_observer(|_: On<HtmlUiBuilt>, mut counts: ResMut<Counts>| {
            counts.built += 1;
        });
    ui.app_mut()
        .add_observer(|_: On<HtmlUiRestyled>, mut counts: ResMut<Counts>| {
            counts.restyled += 1;
        });
    ui.settle();
    let counts = ui.world_mut().resource::<Counts>();
    assert!(counts.built >= 1, "the UI never built");
    assert_eq!(counts.restyled, 0, "the RemSize sync queued a restyle");
}

/// Pathologically deep user content is capped: ~150 nesting levels
/// overflowed a debug-build stack in the recursive walks. Deeper content
/// is dropped (warned once); the capped prefix builds normally.
#[test]
fn deep_nesting_is_capped() {
    let deep = format!("{}x{}", "<div>".repeat(200), "</div>".repeat(200));
    let mut ui = TestUi::new(
        "deep-nesting",
        &[("page.html", "<p>before</p>{{ deep | safe }}<p>after</p>")],
    )
    .spawn(
        "page.html",
        TemplateContext::new().with("deep", &deep),
        Node::default(),
    );
    ui.settle();
    let dump = ui.dump();
    let divs = dump
        .lines()
        .filter(|line| line.trim_start().starts_with("div"))
        .count();
    assert_eq!(divs, 100, "the cap should keep exactly 100 levels:\n{dump}");
    assert!(
        dump.contains("\"before\""),
        "content before the deep tree vanished:\n{dump}"
    );
    assert!(
        dump.contains("\"after\""),
        "content after the deep tree vanished:\n{dump}"
    );
}

/// Deep *inline* content (runs, not nodes) is bounded too: 1000 nested
/// `<b>` used to overflow the stack in `push_runs`. The text survives;
/// the cap drops the deepest levels.
#[test]
fn deep_inline_nesting_is_capped() {
    let deep = format!("<p>x{}y{}</p>", "<b>".repeat(1000), "</b>".repeat(1000));
    let mut ui = TestUi::new("deep-inline", &[("page.html", "{{ deep | safe }}")]).spawn(
        "page.html",
        TemplateContext::new().with("deep", &deep),
        Node::default(),
    );
    ui.settle();
    let dump = ui.dump();
    assert!(
        dump.contains("\"x"),
        "the paragraph before the deep runs vanished:\n{dump}"
    );
    assert!(dump.contains('y'), "the run content vanished:\n{dump}");
}

/// Deep inline nesting is *capped*: past MAX_DOM_DEPTH the chain stops
/// growing (the text survives), so the per-element ancestor work stays
/// linear — 20 000 nested `<b>` used to cost ~4×10⁸ ancestor copies
/// (review_0003 G06; the run must finish in bounded time).
#[test]
fn deep_inline_nesting_chain_is_bounded() {
    let deep = format!("<p>x{}y{}</p>", "<b>".repeat(20_000), "</b>".repeat(20_000));
    let mut ui = TestUi::new("deep-inline-chain", &[("page.html", "{{ deep | safe }}")]).spawn(
        "page.html",
        TemplateContext::new().with("deep", &deep),
        Node::default(),
    );
    let start = std::time::Instant::now();
    ui.settle();
    let elapsed = start.elapsed();
    let dump = ui.dump();
    assert!(dump.contains("\"x"), "the run content vanished:\n{dump}");
    assert!(dump.contains('y'), "the run content vanished:\n{dump}");
    // The old unbounded walk: ~4×10⁸ ancestor copies per build.
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "deep inline nesting is quadratic again: {elapsed:?}"
    );
}

/// An attribute-heavy page's memory stays bounded: the ancestor chain is
/// shared (Rc) and its ancestors carry only the selector fields — a
/// 10 000-attribute element with 10 000 children used to hold ~10⁸ map
/// entries (review_0003 G05). The chain is shared, so the page builds
/// in bounded time.
#[test]
fn attribute_heavy_pages_stay_bounded() {
    let mut page = String::from("<div");
    for i in 0..10_000 {
        page.push_str(&format!(" data-a{i}=\"{i}\""));
    }
    page.push('>');
    page.push_str("<s></s>&nbsp;".repeat(10_000).as_str());
    page.push_str("</div>");
    let mut ui = TestUi::new("attr-heavy", &[("page.html", "{{ page | safe }}")]).spawn(
        "page.html",
        TemplateContext::new().with("page", &page),
        Node::default(),
    );
    let start = std::time::Instant::now();
    ui.settle();
    let elapsed = start.elapsed();
    let dump = ui.dump();
    assert!(
        dump.contains("div"),
        "the div vanished:\n{}",
        &dump[..80.min(dump.len())]
    );
    assert!(
        elapsed < std::time::Duration::from_secs(20),
        "attribute-heavy pages are quadratic again: {elapsed:?}"
    );
}

/// The outline renderer walks the whole document; 3000 levels used to
/// overflow it (it is depth-bounded like the build walks now).
#[test]
fn deep_outline_does_not_overflow() {
    let deep = format!("{}x{}", "<div>".repeat(3000), "</div>".repeat(3000));
    let mut ui = TestUi::new("deep-outline", &[("page.html", "{{ deep | safe }}")]).spawn(
        "page.html",
        TemplateContext::new().with("deep", &deep),
        Node::default(),
    );
    let root = ui.root();
    ui.world_mut()
        .entity_mut(root)
        .insert(bevy_markup::html::HtmlDebugOutline);
    ui.settle_quiet();
    // The outline is logged at debug; reaching a settled dump without an
    // overflow is the assertion.
    let _ = ui.dump();
}

/// An inline `style` attribute's `var()` references the context: a change
/// to the variable restyles the element (the vars hash hashes the inline
/// names the last build collected — the sheet can't know them).
#[test]
fn inline_style_var_change_restyles() {
    let mut ui = TestUi::new(
        "inline-var",
        &[(
            "page.html",
            r#"<p style="color: var(--brand); --x: var(--brand)">colored</p>"#,
        )],
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new().with("--brand", &"#ff0000"),
        Node::default(),
    );
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "colored" default 16px #ff0000
"#,
    );
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("--brand", &"#00ff00");
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "colored" default 16px #00ff00
"#,
    );
}

/// F14's tracking gaps (review_0003 G30): a run-level inline style (an
/// `<span>` inside a block), a translation's markup, and a non-ASCII
/// variable name all restyle on a context change — the text scan missed
/// all three.
#[test]
fn inline_var_tracking_gaps_restyle() {
    let get = |page: &str| {
        TestUi::new(
            "inline-gaps",
            &[
                ("page.html", page),
                ("style.css", "html { color: #ffffff }"),
            ],
        )
        .stylesheet("style.css")
        .spawn(
            "page.html",
            TemplateContext::new().with("--brand", &"#ff0000"),
            Node::default(),
        )
    };
    // A run-level inline style inside a block.
    let mut ui = get(r#"<p>Hello <span style="color: var(--brand)">x</span></p>"#);
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "Hello " default 16px #ffffff
    "x" default 16px #ff0000
"#,
    );
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("--brand", &"#00ff00");
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "Hello " default 16px #ffffff
    "x" default 16px #00ff00
"#,
    );
    // A non-ASCII variable name.
    let mut ui = get(r#"<p style="color: var(--café)">x</p>"#);
    ui.settle();
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("--café", &"#00ff00");
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "x" default 16px #00ff00
"#,
    );
}

/// A translation's inline styles are tracked too (G30: the fragment's
/// names were discarded, so a context change never restyled the
/// translated element's inline spans).
#[test]
fn translation_inline_var_change_restyles() {
    let files = [
        ("page.html", r#"<p data-l10n-id="hi">fallback</p>"#),
        ("style.css", "html { color: #ffffff }"),
        (
            "locales/en-US/main.ftl.ron",
            r#"(locale: "en-US", resources: ["ui.ftl"])"#,
        ),
        (
            "locales/en-US/ui.ftl",
            "hi = Hi <b style=\"color: var(--brand)\">you</b>",
        ),
    ];
    let mut ui = TestUi::new("translation-inline-var", &files)
        .stylesheet("style.css")
        .locale("locales/en-US/main.ftl.ron")
        .spawn(
            "page.html",
            TemplateContext::new().with("--brand", &"#ff0000"),
            Node::default(),
        );
    ui.settle();
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("--brand", &"#00ff00");
    ui.settle();
    let dump = ui.dump();
    assert!(
        dump.contains("#00ff00"),
        "the translated inline restyled: {dump}"
    );
}

/// `<a href>` links: the link's runs become a picked span entity that
/// emits the `open` signal with the href when clicked; the text styles by
/// the cascade (`a` rules); a link without an href is plain styled text.
#[test]
fn links_carry_their_href_on_span_entities() {
    let mut ui = TestUi::new(
        "links",
        &[
            (
                "page.html",
                r#"<p>See <a href="https://example.com" class="ref">the docs</a>
                   and <a id="plain">not a link</a>.</p>"#,
            ),
            (
                "style.css",
                "html { color: #ffffff; font-family: serif }\n\
                 a { color: #00ffff; text-decoration: underline }",
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "See " default 16px #ffffff
    "the docs" default 16px #00ffff open=https://example.com underline
    " and " default 16px #ffffff
    "not a link" default 16px #00ffff underline
    "." default 16px #ffffff
"#,
    );
}

/// A loaded template is named after its asset path (Tera picks HTML
/// autoescaping from the `.html` name).
#[test]
fn template_is_named_after_its_asset_path() {
    let mut ui = TestUi::new("template-name", &[("ui/page.html", "<p>x</p>")]);
    let handle = ui.load::<HtmlTemplate>("ui/page.html");
    for _ in 0..3000 {
        ui.update(1);
        if ui
            .world_mut()
            .resource::<Assets<HtmlTemplate>>()
            .contains(&handle)
        {
            break;
        }
    }
    let templates = ui.world_mut().resource::<Assets<HtmlTemplate>>();
    assert_eq!(
        templates.get(&handle).expect("loaded").name(),
        "ui/page.html"
    );
}

/// Counts updates (`HtmlUiBuilt` or `HtmlUiRestyled`) per entity.
#[derive(Resource, Default)]
struct UpdatesPer(bevy::platform::collections::HashMap<Entity, usize>);

/// Editing a stylesheet asset in place (hot reload) rebuilds the UIs using
/// it — and only those: a UI whose own stylesheet failed to load isn't
/// rebuilt again because some other sheet reloaded.
#[test]
fn stylesheet_reload_rebuilds_only_its_users() {
    let mut ui = TestUi::new(
        "sheet-reload",
        &[
            ("page.html", "<p>Text</p>"),
            ("good.css", "html { color: #ff0000 }"),
            ("broken.css", "{ not css"),
        ],
    );
    let good = ui.load::<Stylesheet>("good.css");
    let broken = ui.load::<Stylesheet>("broken.css");
    let template = ui.load::<HtmlTemplate>("page.html");
    let mut ui = ui.spawn("page.html", TemplateContext::new(), Node::default());
    let user = ui.root();
    let world = ui.world_mut();
    world.init_resource::<UpdatesPer>();
    world.add_observer(|built: On<HtmlUiBuilt>, mut per: ResMut<UpdatesPer>| {
        *per.0.entry(built.entity).or_default() += 1;
    });
    world.add_observer(
        |restyled: On<HtmlUiRestyled>, mut per: ResMut<UpdatesPer>| {
            *per.0.entry(restyled.entity).or_default() += 1;
        },
    );
    world.entity_mut(user).insert(HtmlStylesheet(good.clone()));
    let failed = world
        .spawn((HtmlUi::new(template), HtmlStylesheet(broken)))
        .id();
    ui.settle();
    let counts = |ui: &mut TestUi| {
        let per = &ui.world_mut().resource::<UpdatesPer>().0;
        (
            per.get(&user).copied().unwrap_or(0),
            per.get(&failed).copied().unwrap_or(0),
        )
    };
    let (user_before, failed_before) = counts(&mut ui);
    assert!(
        user_before >= 1 && failed_before >= 1,
        "both built: {user_before}, {failed_before}"
    );

    // In-place edit: `AssetMut` emits `Modified` once dereferenced mutably.
    {
        let mut sheets = ui.world_mut().resource_mut::<Assets<Stylesheet>>();
        let mut sheet = sheets.get_mut(&good).expect("good.css loaded");
        let _: &mut Stylesheet = &mut sheet;
    }
    ui.settle();
    assert_eq!(counts(&mut ui), (user_before + 1, failed_before));
}

/// A frame image that changes (hot reload, or one arriving after the
/// stylesheet) re-slices `%` frames against its new size.
#[test]
fn image_change_reslices_percent_frames() {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    let mut ui = TestUi::new(
        "image-reload",
        &[
            ("page.html", r#"<p class="framed">Framed</p>"#),
            (
                "style.css",
                r#"html { color: #ffffff; font-family: Spectral; font-size: 20px }
.framed { border-image: url("frame.png") 25% fill stretch; border-width: 6px }"#,
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    let frame = ui.load::<Image>("frame.png");
    // 25% of the 32×24 fixture: 6 (top/bottom), 8 (left/right).
    let expected = |slices: &str| {
        format!(
            "\nhtml-ui\n  p.framed border=6,6,6,6 slice=frame.png {slices} stretch\n    -\n      \"Framed\" serif 20px #ffffff\n"
        )
    };
    ui.settle().assert_dump(&expected("6,8,6,8"));

    let bigger = Image::new_fill(
        Extent3d {
            width: 64,
            height: 48,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255; 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    ui.world_mut()
        .resource_mut::<Assets<Image>>()
        .insert(&frame, bigger)
        .unwrap();
    ui.settle().assert_dump(&expected("12,16,12,16"));
}

/// The UI isn't built while its stylesheet is still loading (no unstyled
/// flash), and is built, styled, once it arrives.
#[test]
fn loading_stylesheet_defers_the_build() {
    let mut ui = TestUi::new(
        "sheet-loading",
        &[
            ("page.html", "<p>Text</p>"),
            ("style.css", "html { color: #ff0000 }"),
        ],
    );
    // A handle nothing will ever load: the stylesheet stays "loading".
    let pending = ui
        .world_mut()
        .resource::<Assets<Stylesheet>>()
        .reserve_handle();
    ui.world_mut()
        .insert_resource(DefaultStylesheet::new(pending.clone()));
    let real = ui.load::<Stylesheet>("style.css");
    let mut ui = ui.spawn("page.html", TemplateContext::new(), Node::default());
    let root = ui.root();
    for _ in 0..3000 {
        ui.update(1);
        let world = ui.world_mut();
        let rendered = matches!(
            world.get::<RenderedHtml>(root),
            Some(RenderedHtml::Ready(_))
        );
        if rendered && world.resource::<Assets<Stylesheet>>().contains(&real) {
            break;
        }
    }
    ui.update(10);
    assert_eq!(ui.builds(), 0, "built while the stylesheet was loading");

    let mut sheets = ui.world_mut().resource_mut::<Assets<Stylesheet>>();
    let sheet = sheets.remove(&real).expect("style.css loaded");
    sheets.insert(&pending, sheet).unwrap();
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "Text" default 16px #ff0000
"#,
    );
}

// ---------------------------------------------------------------------------
// Browser oracle
// ---------------------------------------------------------------------------

/// Every `tests/vectors/*/` with a `browser.json` (written by
/// `scripts/browser_oracle.py` from headless Chromium) is built here and
/// compared against the browser's computed styles: per non-whitespace
/// character (face, size, color) and per block/container element (padding,
/// border widths, border-image, background, gap). Deliberate differences are
/// listed in the comment above `FIXTURE_SIZE`.
#[test]
fn browser_oracle() {
    let vectors = vectors_with("browser.json");
    assert!(!vectors.is_empty(), "no oracle vectors found");

    let mut failures = Vec::new();
    for name in &vectors {
        let json = std::fs::read_to_string(vectors_dir().join(name).join("browser.json")).unwrap();
        let oracle: serde_json::Value = serde_json::from_str(&json).unwrap();
        let mut ui = TestUi::from_vector(name);
        ui.settle();
        for problem in compare_with_browser(&mut ui, &oracle) {
            failures.push(format!("{name}: {problem}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} difference(s) from the browser:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Bevy UI's headless layout of bevy_markup's column model, derived by hand: the
/// default font (FiraMono) advances 0.6em per character and lines are 1.2em;
/// the root's padding insets everything, a container's border and padding
/// inset its children and `gap` separates them, text wraps at the content
/// width (50 characters at 12px), `pre` adds bevy_markup's 8px padding.
#[test]
fn layout_column_stacking() {
    let mut ui = TestUi::from_layout_vector("layout_blocks", UVec2::new(640, 480));
    let actual = ui.settle().layout_dump();
    let expected = "\
html-ui 0,0 640x263
  h1 20,10 600x48
  p 20,58 600x48
  div.panel 20,106 600x83
    p 28,114 584x24
    p.tall 28,145 584x36
  pre 20,189 600x64";
    assert_eq!(actual.trim_end(), expected, "\n--- actual ---\n{actual}");
}

/// Bevy rounds layout to whole pixels; Chromium keeps 1/64px fractions.
const LAYOUT_TOLERANCE: f32 = 1.0;

/// Every `tests/vectors/layout_*/` with a `browser.json` is laid out by Bevy
/// UI headlessly (in the oracle's viewport, under a full-width column root)
/// and each root/block/container border box compared with Chromium's
/// `getBoundingClientRect` under the oracle's bevy_markup layout stylesheet
/// (`BEVY_MARKUP_CSS` in `scripts/browser_oracle.py`), within [`LAYOUT_TOLERANCE`]
/// per value.
#[test]
fn layout_oracle() {
    let vectors: Vec<String> = vectors_with("browser.json")
        .into_iter()
        .filter(|name| name.starts_with("layout_"))
        .collect();
    assert!(!vectors.is_empty(), "no layout oracle vectors found");

    let mut failures = Vec::new();
    for name in &vectors {
        let json = std::fs::read_to_string(vectors_dir().join(name).join("browser.json")).unwrap();
        let oracle: serde_json::Value = serde_json::from_str(&json).unwrap();
        let viewport = &oracle["generator"]["viewport"];
        let viewport = UVec2::new(
            viewport[0].as_u64().unwrap() as u32,
            viewport[1].as_u64().unwrap() as u32,
        );
        let mut ui = TestUi::from_layout_vector(name, viewport);
        ui.settle();
        let root = ui.root();
        let world = ui.world_mut();
        let pairs = match pair_elements(world, root, &oracle) {
            Ok(pairs) => pairs,
            Err(problem) => {
                failures.push(format!("{name}: {problem}"));
                continue;
            }
        };
        for (entity, record, label) in pairs {
            let rect = node_rect(world, entity).expect("laid out");
            let ours = [rect.min.x, rect.min.y, rect.width(), rect.height()];
            let theirs: Vec<f32> = record["rect"]
                .as_array()
                .expect("layout vectors record rects")
                .iter()
                .map(|value| value.as_f64().unwrap() as f32)
                .collect();
            if ours
                .iter()
                .zip(&theirs)
                .any(|(a, b)| (a - b).abs() > LAYOUT_TOLERANCE)
            {
                failures.push(format!(
                    "{name}: {label} [x, y, w, h]: bevy_markup {ours:?} vs browser {theirs:?}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} layout difference(s) from the browser:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Fluent oracle
// ---------------------------------------------------------------------------

/// Every `tests/vectors/*/` with a `fluent.html` (written by
/// `scripts/fluent_oracle.sh`: Fluent's reference DOM bindings, `@fluent/dom`,
/// translating `page.html` with `messages.ftl`) must build the same world
/// localized by bevy_markup as unlocalized from that reference translation — same
/// nodes, runs, faces, sizes and colors.
#[test]
fn fluent_oracle() {
    let vectors = vectors_with("fluent.html");
    assert!(!vectors.is_empty(), "no Fluent oracle vectors found");

    let mut failures = Vec::new();
    for name in &vectors {
        let mut localized = TestUi::from_vector(name);
        let ours = localized.settle().dump();
        let mut reference = TestUi::vector_page(name, "fluent.html", false);
        let theirs = reference.settle().dump();
        if ours != theirs {
            failures.push(format!(
                "{name} (- @fluent/dom, + bevy_markup):\n{}",
                line_diff(&theirs, &ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} vector(s) differ from @fluent/dom:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Lines only in `old` as `- `, only in `new` as `+ `, shared as `  `
/// (longest common subsequence; dumps are small).
fn line_diff(old: &str, new: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, String::new());
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            out += &format!("  {}\n", a[i]);
            (i, j) = (i + 1, j + 1);
        } else if j < b.len() && (i == a.len() || lcs[i][j + 1] >= lcs[i + 1][j]) {
            out += &format!("+ {}\n", b[j]);
            j += 1;
        } else {
            out += &format!("- {}\n", a[i]);
            i += 1;
        }
    }
    out
}

// Deliberate, documented differences from browsers (skipped by
// `compare_with_browser`):
// - root `background-color`: bevy_markup leaves the `HtmlUi` node's background to the
//   app.
// - `border-style`: bevy_markup ignores it (a `border-width` always applies); vectors
//   set `border-style: solid` so browsers compute the widths.
// - bullets: bevy_markup's `li` prefix `• ` is text on the `Text` root (not compared);
//   browsers draw a marker.

const FIXTURE_SIZE: (f32, f32) = (32.0, 24.0);
const BLOCKS: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6", "p", "li", "pre"];
const CONTAINERS: &[&str] = &[
    "div",
    "section",
    "article",
    "header",
    "footer",
    "main",
    "nav",
    "aside",
    "ul",
    "ol",
    "blockquote",
    "figure",
    "form",
    // ADR 0028: the flex table model — the table/sections/caption are
    // columns, a tr is a row, the cells are containers.
    "table",
    "tbody",
    "thead",
    "tfoot",
    "caption",
    "tr",
    "td",
    "th",
];

/// The face CSS font matching picks for this computed style, with the test
/// harness's registered families (`Spectral` with all four faces, `Mono` with
/// regular only, `monospace` → `Mono`).
fn browser_face(record: &serde_json::Value) -> String {
    let family = record["fontFamily"].as_str().unwrap_or_default();
    let bold = record["fontWeight"].as_f64().unwrap_or(400.0) > 500.0;
    let italic = record["fontStyle"].as_str().unwrap_or("normal") != "normal";
    for name in family.split(',') {
        match name
            .trim()
            .trim_matches(['"', '\''])
            .to_ascii_lowercase()
            .as_str()
        {
            "spectral" => {
                return match (bold, italic) {
                    (false, false) => "serif",
                    (true, false) => "serif-bold",
                    (false, true) => "serif-italic",
                    (true, true) => "serif-bold-italic",
                }
                .to_owned();
            }
            "mono" | "monospace" => return "mono".to_owned(),
            _ => {}
        }
    }
    "default".to_owned()
}

fn browser_color(css: &str) -> Option<String> {
    let inner = css
        .strip_prefix("rgba(")
        .or_else(|| css.strip_prefix("rgb("))?;
    let parts: Vec<f32> = inner
        .trim_end_matches(')')
        .split(',')
        .map(|part| part.trim().parse().unwrap())
        .collect();
    let alpha = parts.get(3).copied().unwrap_or(1.0);
    (alpha > 0.0).then(|| {
        hex(Color::srgba_u8(
            parts[0] as u8,
            parts[1] as u8,
            parts[2] as u8,
            (alpha * 255.0).round() as u8,
        ))
    })
}

fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn compare_with_browser(ui: &mut TestUi, oracle: &serde_json::Value) -> Vec<String> {
    let mut problems = Vec::new();
    let root = ui.root();
    let world = ui.world_mut();

    // Text: one (char, face, size, color) per non-whitespace character.
    let mut ours = Vec::new();
    collect_chars(world, root, &mut ours);
    let theirs: Vec<(char, String, f64, String)> = oracle["runs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|run| {
            let style = (
                browser_face(run),
                round(run["fontSize"].as_f64().unwrap()),
                browser_color(run["color"].as_str().unwrap()).unwrap_or_default(),
            );
            run["text"]
                .as_str()
                .unwrap()
                .chars()
                .filter(|c| !c.is_whitespace())
                .map(move |c| (c, style.0.clone(), style.1, style.2.clone()))
                .collect::<Vec<_>>()
        })
        .collect();
    let text =
        |chars: &[(char, String, f64, String)]| chars.iter().map(|c| c.0).collect::<String>();
    if text(&ours) != text(&theirs) {
        problems.push(format!(
            "text differs: bevy_markup {:?} vs browser {:?}",
            text(&ours),
            text(&theirs)
        ));
    } else if let Some(index) = (0..ours.len()).find(|&i| ours[i] != theirs[i]) {
        let context: String = ours[index.saturating_sub(8)..=index]
            .iter()
            .map(|c| c.0)
            .collect();
        let (_, face, size, color) = &ours[index];
        let (_, b_face, b_size, b_color) = &theirs[index];
        problems.push(format!(
            "text style at …{context:?}: bevy_markup {face} {size}px {color} vs browser {b_face} {b_size}px {b_color}"
        ));
    }

    // Boxes: the root (`html`) and every block/container, in document order.
    let pairs = match pair_elements(world, root, oracle) {
        Ok(pairs) => pairs,
        Err(problem) => {
            problems.push(problem);
            return problems;
        }
    };
    for (entity, theirs, label) in pairs {
        let tag = theirs["tag"].as_str().unwrap();
        let entity_ref = world.entity(entity);
        let node = entity_ref.get::<Node>().cloned().unwrap_or_default();
        let sides = |rect: UiRect| {
            [rect.top, rect.right, rect.bottom, rect.left].map(|v| match v {
                Val::Px(px) => px as f64,
                _ => 0.0,
            })
        };
        let floats = |key: &str| -> [f64; 4] {
            let values: Vec<f64> = theirs[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            [values[0], values[1], values[2], values[3]]
        };
        let mut check = |what: &str, ours: String, browser: String| {
            if ours != browser {
                problems.push(format!(
                    "{label}: {what} bevy_markup {ours} vs browser {browser}"
                ));
            }
        };

        check(
            "padding",
            format!("{:?}", sides(node.padding)),
            format!("{:?}", floats("padding")),
        );
        check(
            "border",
            format!("{:?}", sides(node.border)),
            format!("{:?}", floats("border")),
        );

        let our_slice = entity_ref
            .get::<ImageNode>()
            .and_then(|image| match &image.image_mode {
                NodeImageMode::Sliced(slicer) => {
                    let file = image
                        .image
                        .path()
                        .map(|p| p.path().display().to_string())
                        .unwrap_or_default();
                    let (min, max) = (slicer.border.min_inset, slicer.border.max_inset);
                    let mode = match slicer.sides_scale_mode {
                        SliceScaleMode::Stretch => "stretch",
                        SliceScaleMode::Tile { .. } => "tile",
                    };
                    Some(format!(
                        "{file} {},{},{},{} {mode}",
                        min.y, max.x, max.y, min.x
                    ))
                }
                _ => None,
            });
        check(
            "border-image",
            format!("{our_slice:?}"),
            format!("{:?}", browser_slice(theirs)),
        );

        // Root background: allowed difference (belongs to the app).
        if tag != "html" {
            let our_bg = entity_ref
                .get::<BackgroundColor>()
                .filter(|bg| bg.0 != Color::NONE)
                .map(|bg| hex(bg.0));
            let their_bg = browser_color(theirs["backgroundColor"].as_str().unwrap());
            check("background", format!("{our_bg:?}"), format!("{their_bg:?}"));
        }
        if CONTAINERS.contains(&tag) {
            let our_gap = match node.row_gap {
                Val::Px(px) => px as f64,
                _ => 0.0,
            };
            // `normal` = no CSS gap → bevy_markup uses the root's gap (0 here).
            let their_gap = theirs["rowGap"]
                .as_str()
                .unwrap()
                .trim_end_matches("px")
                .parse()
                .unwrap_or(0.0);
            check("gap", format!("{our_gap}"), format!("{their_gap}"));
        }
    }
    problems
}

/// `file t,r,b,l stretch|tile` from the browser's computed border-image, with
/// `%` slices resolved against the 32×24 fixture.
fn browser_slice(record: &serde_json::Value) -> Option<String> {
    let source = record["borderImageSource"].as_str().unwrap();
    let file = source
        .strip_prefix("url(\"")?
        .trim_end_matches("\")")
        .rsplit('/')
        .next()?
        .to_owned();
    let values: Vec<&str> = record["borderImageSlice"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .filter(|v| *v != "fill")
        .collect();
    let at = |i: usize| match values.len() {
        1 => values[0],
        2 => values[i % 2],
        3 => values[if i == 3 { 1 } else { i }],
        _ => values[i],
    };
    let (width, height) = FIXTURE_SIZE;
    let side = |i: usize| -> f32 {
        let value = at(i);
        match value.strip_suffix('%') {
            Some(percent) => {
                percent.parse::<f32>().unwrap() / 100.0 * if i % 2 == 0 { height } else { width }
            }
            None => value.parse().unwrap(),
        }
    };
    let tile = record["borderImageRepeat"]
        .as_str()
        .unwrap()
        .split_whitespace()
        .any(|r| r != "stretch");
    Some(format!(
        "{file} {},{},{},{} {}",
        side(0),
        side(1),
        side(2),
        side(3),
        if tile { "tile" } else { "stretch" }
    ))
}

/// `(char, face, size, color)` for each non-whitespace character of every
/// `TextSpan` below `entity`, in document order (`Text` roots hold only the
/// `li` bullet, an allowed difference).
fn collect_chars(world: &World, entity: Entity, out: &mut Vec<(char, String, f64, String)>) {
    let entity_ref = world.entity(entity);
    if let (Some(span), Some(font), Some(color)) = (
        entity_ref.get::<TextSpan>(),
        entity_ref.get::<TextFont>(),
        entity_ref.get::<TextColor>(),
    ) {
        let size = match font.font_size {
            FontSize::Px(px) => round(px as f64),
            _ => f64::NAN,
        };
        let face = face_label(&font.font);
        for c in span.0.chars().filter(|c| !c.is_whitespace()) {
            out.push((c, face.clone(), size, hex(color.0)));
        }
    }
    if let Some(children) = entity_ref.get::<Children>() {
        for &child in children {
            collect_chars(world, child, out);
        }
    }
}

/// bevy_markup's root and block/container entities paired, in document order, with
/// the browser's records of the same elements (inline elements have no
/// entity), each labelled `tag.class…`.
fn pair_elements<'o>(
    world: &World,
    root: Entity,
    oracle: &'o serde_json::Value,
) -> Result<Vec<(Entity, &'o serde_json::Value, String)>, String> {
    let mut ours = vec![root];
    collect_elements(world, root, &mut ours);
    // A browser's HTML parser synthesizes `tbody` around bare `tr`s
    // (bevy_markup's tl parser doesn't); skip it on both sides. An
    // explicit tbody loses its box comparison.
    ours.retain(|&entity| {
        world
            .entity(entity)
            .get::<HtmlElement>()
            .is_none_or(|element| element.tag != "tbody")
    });
    let theirs: Vec<&serde_json::Value> = oracle["elements"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            let tag = e["tag"].as_str().unwrap();
            (tag == "html" || BLOCKS.contains(&tag) || CONTAINERS.contains(&tag)) && tag != "tbody"
        })
        .collect();
    if ours.len() != theirs.len() {
        return Err(format!(
            "element count: bevy_markup {} vs browser {}",
            ours.len(),
            theirs.len()
        ));
    }
    let mut pairs = Vec::new();
    for (entity, record) in ours.into_iter().zip(theirs) {
        let tag = record["tag"].as_str().unwrap();
        let label = match record["classes"].as_array().unwrap().as_slice() {
            [] => tag.to_owned(),
            classes => format!(
                "{tag}.{}",
                classes
                    .iter()
                    .map(|c| c.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join(".")
            ),
        };
        if let Some(element) = world.entity(entity).get::<HtmlElement>()
            && element.tag != tag
        {
            return Err(format!(
                "{label}: element order differs (bevy_markup has {})",
                element.tag
            ));
        }
        pairs.push((entity, record, label));
    }
    Ok(pairs)
}

/// Entities with an `HtmlElement` below `entity`, in document order.
fn collect_elements(world: &World, entity: Entity, out: &mut Vec<Entity>) {
    if let Some(children) = world.entity(entity).get::<Children>() {
        for &child in children {
            if world.entity(child).contains::<HtmlElement>() {
                out.push(child);
            }
            collect_elements(world, child, out);
        }
    }
}

/// An `HtmlUi` nested inside another one's subtree, both updating in the
/// same frame. While the element holding it is kept, the nested UI survives
/// the ancestor's update and gets its own; when the ancestor's update
/// replaces that element, the nested UI is despawned with it — after its own
/// commands (deepest UIs go first), so nothing hits a despawned entity
/// (bug_0016).
#[test]
fn nested_ui_survives_or_goes_with_its_slot_without_panics() {
    let mut ui = TestUi::new(
        "nested-rebuild",
        &[
            (
                "outer.html",
                r#"<div id="{{ slot }}"><p data-l10n-id="outer">Outer</p></div>"#,
            ),
            ("inner.html", r#"<p data-l10n-id="inner">Inner</p>"#),
            (
                "style.css",
                "html { color: #ffffff; font-family: Spectral; font-size: 20px }",
            ),
            (
                "locales/en-US/main.ftl.ron",
                r#"(locale: "en-US", resources: ["ui.ftl"])"#,
            ),
            ("locales/en-US/ui.ftl", "outer = Outer EN\ninner = Inner EN"),
            (
                "locales/de/main.ftl.ron",
                r#"(locale: "de", resources: ["ui.ftl"])"#,
            ),
            ("locales/de/ui.ftl", "outer = Outer DE\ninner = Inner DE"),
        ],
    )
    .stylesheet("style.css")
    .locale("locales/en-US/main.ftl.ron")
    .spawn(
        "outer.html",
        TemplateContext::new().with("slot", "slot"),
        Node::default(),
    );
    ui.settle();

    // Nest a second UI into the outer's slot.
    let slot = {
        let world = ui.world_mut();
        let mut query = world.query::<(Entity, &HtmlElement)>();
        query
            .iter(world)
            .find(|(_, element)| element.id.as_deref() == Some("slot"))
            .map(|(entity, _)| entity)
            .expect("slot")
    };
    let template = ui.world_mut().resource::<AssetServer>().load("inner.html");
    let nested = ui
        .world_mut()
        .spawn((
            HtmlUi::new(template),
            TemplateContext::new(),
            Node::default(),
            ChildOf(slot),
        ))
        .id();
    ui.settle().assert_dump(
        r#"
html-ui
  div#slot
    p
      "Outer EN" serif 20px #ffffff
    -
      p
        "Inner EN" serif 20px #ffffff
"#,
    );

    // Both UIs re-localize in the same frame; the slot is kept, and the
    // nested UI with it.
    let german = ui
        .world_mut()
        .resource::<AssetServer>()
        .load("locales/de/main.ftl.ron");
    ui.world_mut().insert_resource(ActiveLocale::new(german));
    ui.settle().assert_dump(
        r#"
html-ui
  div#slot
    p
      "Outer DE" serif 20px #ffffff
    -
      p
        "Inner DE" serif 20px #ffffff
"#,
    );

    // Both update again in one frame, and the outer's update replaces the
    // slot (a new `id`): the nested UI goes with it.
    let english = ui
        .world_mut()
        .resource::<AssetServer>()
        .load("locales/en-US/main.ftl.ron");
    ui.world_mut().insert_resource(ActiveLocale::new(english));
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("slot", "other");
    ui.settle().assert_dump(
        r#"
html-ui
  div#other
    p
      "Outer EN" serif 20px #ffffff
"#,
    );
    assert!(
        ui.world_mut().get_entity(nested).is_err(),
        "the nested UI is despawned with its replaced slot"
    );
}

/// `data-on-*` hooks parse into `ElementSignals` on the element entity (the
/// click/press/release observers ride along; hover is tracked by
/// `hover_signals`).
#[test]
fn data_on_hooks_attach_to_elements() {
    let mut ui = TestUi::new(
        "signal-hooks",
        &[
            (
                "page.html",
                r#"<div id="box" data-on-click="buy" data-with='{"n": 3}'><p>Hello</p></div>"#,
            ),
            ("style.css", "html { color: #ffffff; font-size: 20px }"),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();

    let world = ui.world_mut();
    let mut query = world.query::<(Entity, &HtmlElement, &ElementSignals)>();
    let found = query
        .iter(world)
        .find(|(_, element, _)| element.id.as_deref() == Some("box"))
        .map(|(_, _, signals)| signals)
        .expect("box element with signals");
    assert_eq!(found.0.len(), 1);
    assert_eq!(found.0[0].name, "buy");
    assert_eq!(found.0[0].payload["n"], 3);
}

/// `:hover`/`:active` styles apply from the element's [`PseudoState`] and
/// update in place: the state change restyles, it doesn't rebuild (the
/// entity keeps whatever the app attached).
#[test]
fn hover_state_restyles_in_place() {
    let mut ui = TestUi::new(
        "pseudo-hover",
        &[
            (
                "page.html",
                r#"<div id="box" class="card"><p>Hello</p></div>"#,
            ),
            (
                "style.css",
                r#"
html { color: #ffffff; font-size: 20px }
.card { background-color: #111111 }
.card:hover { background-color: #222222; color: #00ff00 }
"#,
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();

    let box_entity = {
        let world = ui.world_mut();
        let mut query = world.query::<(Entity, &HtmlElement)>();
        query
            .iter(world)
            .find(|(_, element)| element.id.as_deref() == Some("box"))
            .map(|(entity, _)| entity)
            .expect("box")
    };

    // Hover: the state change restyles the card in place.
    ui.world_mut().entity_mut(box_entity).insert(PseudoState {
        hovered: true,
        ..default()
    });
    ui.settle().assert_dump(
        r#"
html-ui
  div#box.card bg=#222222
    p
      "Hello" default 20px #00ff00
"#,
    );

    // Unhover: back to the base styles.
    ui.world_mut()
        .entity_mut(box_entity)
        .insert(PseudoState::default());
    ui.settle().assert_dump(
        r#"
html-ui
  div#box.card bg=#111111
    p
      "Hello" default 20px #ffffff
"#,
    );
}

/// A restyle of an `HtmlUi` whose slots hold app-nested `HtmlUi`s must not
/// fall back to a rebuild: `same_shape` ignores nested-UI children, and the
/// fallback's `despawn_related` killed the nested UIs — panicking when they
/// updated the same frame (bug_0017).
#[test]
fn restyle_with_nested_uis_keeps_them() {
    let mut ui = TestUi::new(
        "nested-restyle",
        &[
            (
                "outer.html",
                r#"<div id="slot"><p class="card">Outer</p></div>"#,
            ),
            ("inner.html", r#"<p class="card">Inner</p>"#),
            (
                "style.css",
                r#"
html { color: #ffffff; font-size: 20px }
.card { background-color: #111111 }
.card:hover { background-color: #222222 }
"#,
            ),
        ],
    )
    .stylesheet("style.css")
    .spawn("outer.html", TemplateContext::new(), Node::default());
    ui.settle();

    let (slot, inner_template) = {
        let world = ui.world_mut();
        let mut elements = world.query::<(Entity, &HtmlElement)>();
        let slot = elements
            .iter(world)
            .find(|(_, element)| element.id.as_deref() == Some("slot"))
            .map(|(entity, _)| entity)
            .expect("slot");
        let template = world.resource::<AssetServer>().load("inner.html");
        (slot, template)
    };
    let inner = ui
        .world_mut()
        .spawn((
            HtmlUi::new(inner_template),
            TemplateContext::new(),
            Node::default(),
            ChildOf(slot),
        ))
        .id();
    ui.settle();

    // Hover both UIs in the same frame: the outer restyles (its slots keep
    // the nested UI), the inner restyles too — nobody despawns anybody.
    let mut cards = ui
        .world_mut()
        .query::<(Entity, &HtmlElement, Option<&PseudoState>)>();
    let cards: Vec<Entity> = cards
        .iter(ui.world_mut())
        .filter(|(_, element, _)| element.has_class("card"))
        .map(|(entity, _, _)| entity)
        .collect();
    assert_eq!(cards.len(), 2);
    for entity in cards {
        ui.world_mut().entity_mut(entity).insert(PseudoState {
            hovered: true,
            ..default()
        });
    }
    ui.settle().assert_dump(
        r#"
html-ui
  div#slot
    p.card bg=#222222
      -
        "Outer" default 20px #ffffff
    -
      p.card bg=#222222
        -
          "Inner" default 20px #ffffff
"#,
    );
    assert!(
        ui.world_mut().get_entity(inner).is_ok(),
        "the nested UI survived the outer restyle"
    );
}

/// An `<img>`'s CSS `width`/`height` sizes the inline box against the
/// containing block's content box at layout time: one dimension scales
/// with the aspect ratio preserved (the fixture is 32×24), both stretch,
/// and padding insets the resolution. Without a declaration the image
/// keeps its intrinsic size.
#[test]
fn layout_image_css_sizes() {
    let page = r#"<div id="root">
  <p id="half"><img class="half" src="frame.png"></p>
  <p id="padded"><img class="full" src="frame.png"></p>
  <p id="stretched"><img class="stretch" src="frame.png"></p>
  <p id="tall"><img class="tall" src="frame.png"></p>
  <p id="intrinsic"><img src="frame.png"></p>
</div>"#;
    let css = "img.half { width: 50% } img.full { width: 100% } \
padded { padding: 10px } p#padded { padding: 10px } img.stretch { width: 100%; height: 50% } img.tall { height: 50% }";
    let mut ui = TestUi::with_layout(
        "image-css-sizes",
        &[("page.html", page), ("style.css", css)],
        UVec2::new(400, 600),
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new(),
        Node {
            width: Val::Px(400.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    ui.settle();
    ui.update(4);

    // The image box: the span's `InlineBox`, sized by the layout pass.
    let world = ui.world_mut();
    let mut boxes = world.query::<(&bevy::text::InlineBox, &bevy::ui::widget::InlineImage)>();
    let sized: Vec<Vec2> = boxes.iter(world).map(|(box_, _)| box_.size).collect();
    // The paragraphs are root-width blocks (400 content); the padded one
    // has 10px padding per side, so its content box is 380.
    let expected = vec![
        Vec2::new(200.0, 150.0), // half: 50% of 400, aspect 32:24
        Vec2::new(380.0, 285.0), // full: 100% of 380
        Vec2::new(400.0, 300.0), // stretch: both dimensions
        Vec2::new(400.0, 300.0), // tall: 50% of 600, aspect preserved
        Vec2::new(32.0, 24.0),   // intrinsic (no declaration)
    ];
    let mut actual = sized.clone();
    actual.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap().then(a.y.total_cmp(&b.y)));
    let mut expected_sorted = expected.clone();
    expected_sorted.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap().then(a.y.total_cmp(&b.y)));
    for (actual, expected) in actual.iter().zip(&expected_sorted) {
        assert!(
            (actual - expected).max_element() <= LAYOUT_TOLERANCE,
            "image box {actual:?} != {expected:?} (all {sized:?})"
        );
    }
}

/// ADR 0021 regression tests: a build that reuses its clean skeleton must
/// equal a build without one. Each test drives one long-lived `reused` UI
/// and a `fresh` comparator that drops its skeleton before every change,
/// so every build there collects from scratch.
mod skeleton_reuse {
    use super::*;
    use bevy_markup::Skeleton;

    /// A retry after many gate-skipped builds serves the CURRENT content
    /// (R04, bug_0033): a gate-skipped build updates its entities from
    /// fresh specs and replaces the skeleton alongside — the comparator's
    /// skeleton is stripped every step so its build is always fresh. On the
    /// R04 revert the skeleton kept the first step's specs while the doc
    /// advanced, and the retry's clean match served the stale spec's text.
    #[test]
    fn gate_retry_after_all_dirty_builds_stays_current() {
        let rows = (0..9)
            .map(|i| format!("<p>Row {i}: {{{{ v }}}}</p>"))
            .collect::<String>();
        let page = format!(r#"<p class="t">T: {{{{ b }}}}</p>{rows}"#);
        let (mut reused, mut fresh) = spawn_two(
            "gate-retry",
            &[
                ("page.html", page.as_str()),
                ("a.css", "html { color: #ffffff }"),
            ],
            &TemplateContext::new().with("v", &0u32).with("b", &0u32),
        );
        reused.settle();
        fresh.settle();

        let toggle = |ui: &mut TestUi, keep: bool, v: u32| {
            let root = ui.root();
            if !keep {
                ui.world_mut().entity_mut(root).remove::<Skeleton>();
            }
            ui.world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("v", &v);
        };
        // One-key changes: all nine rows are dirty every step, the pairing
        // scores useless and the gate skips (backing off 16, 32 … builds).
        // 32 steps: the pairing retries at step ~17 (v=1) and then skips —
        // the final render (v=0) differs from the last retry's, so the
        // structural assert below can't pass by parity.
        for step in 1..=32u32 {
            let v = step % 2;
            toggle(&mut reused, true, v);
            toggle(&mut fresh, false, v);
            reused.settle();
            fresh.settle();
            assert_eq!(reused.dump(), fresh.dump(), "toggle {step}");
        }

        // The structural half of the guard: after the gate-skipped builds
        // the skeleton must have been replaced wholesale — its document is
        // the CURRENT render (R04's revert ticked the counter only and left
        // the skeleton at the first step's state, and the retry's clean
        // match then served the stale spec's text).
        let root = reused.root();
        let skeleton_source = reused
            .world_mut()
            .get::<Skeleton>(root)
            .and_then(|skeleton| skeleton.document_source().map(str::to_owned));
        let current_source = reused
            .world_mut()
            .get::<bevy_markup::prelude::RenderedHtml>(root)
            .map(|rendered| match &*rendered {
                bevy_markup::prelude::RenderedHtml::Ready(document) => document.source().to_owned(),
                _ => String::new(),
            });
        assert_eq!(
            skeleton_source.as_deref(),
            current_source.as_deref(),
            "the skeleton's document is the current render"
        );

        // The visible half: the b change (which can land on a retry) shows
        // the current text.
        toggle(&mut reused, true, 0);
        toggle(&mut fresh, false, 0);
        for ui in [&mut reused, &mut fresh] {
            let root = ui.root();
            ui.world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("b", &1);
        }
        reused.settle();
        fresh.settle();
        assert_eq!(reused.dump(), fresh.dump());
        assert!(reused.dump().contains("T: 1"), "{}", reused.dump());
    }

    fn spawn_two(
        name: &'static str,
        files: &[(&str, &str)],
        context: &TemplateContext,
    ) -> (TestUi, TestUi) {
        let reused = TestUi::new(&format!("{name}-reused"), files)
            .stylesheet("a.css")
            .spawn("page.html", context.clone(), Node::default());
        let fresh = TestUi::new(&format!("{name}-fresh"), files)
            .stylesheet("a.css")
            .spawn("page.html", context.clone(), Node::default());
        (reused, fresh)
    }

    /// Applies `op` to both UIs (the comparator drops its skeleton first,
    /// so its build is fresh), settles both, and compares the dumps.
    /// Both UIs are settled when it returns.
    fn assert_same_after(reused: &mut TestUi, fresh: &mut TestUi, op: impl Fn(&mut TestUi, bool)) {
        reused.settle();
        fresh.settle();
        op(reused, true);
        op(fresh, false);
        assert_eq!(
            reused.settle().dump(),
            fresh.settle().dump(),
            "the reused UI diverged from a fresh build"
        );
    }

    fn set_context(ui: &mut TestUi, key: &'static str, value: &str) {
        let root = ui.root();
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert(key, &value);
    }

    /// Changing the dynamic content must not drop the unchanged
    /// walk-through-tag siblings around it (`span`/`b` in a container
    /// produce no spec of their own; returning early after a failed reuse
    /// lookup lost them).
    #[test]
    fn reused_walkthrough_siblings_keep_their_content() {
        let files = [
            (
                "page.html",
                r#"<div class="panel"><p class="static">Static</p><span>Inline tag</span><b>Bold tag</b><p>Value: {{ n }}</p></div>"#,
            ),
            ("a.css", "html { color: #ffffff; font-size: 20px }"),
        ];
        let (mut reused, mut fresh) =
            spawn_two("walkthrough", &files, &TemplateContext::new().with("n", &1));
        assert_same_after(&mut reused, &mut fresh, |ui, keep| {
            let root = ui.root();
            if !keep {
                ui.world_mut().entity_mut(root).remove::<Skeleton>();
            }
            set_context(ui, "n", "2");
        });
        let dump = reused.dump();
        assert!(
            dump.contains("\"Inline tag\" default 20px #ffffff")
                && dump.contains("\"Bold tag\" default 20px #ffffff"),
            "walk-through-tag content vanished:\n{dump}"
        );
    }

    /// A stylesheet swap in the same frame as a content change (the
    /// theme-button case) must restyle the unchanged subtrees too: the
    /// cached specs were built from the previous sheet.
    #[test]
    fn stylesheet_swap_with_a_content_change_restyles_reused_subtrees() {
        let files = [
            (
                "page.html",
                "<p class=\"static\">Static</p><p>Value: {{ n }}</p>",
            ),
            (
                "a.css",
                "html { color: #ffffff } .static { color: #ff0000 }",
            ),
            (
                "b.css",
                "html { color: #ffffff } .static { color: #00ff00 }",
            ),
        ];
        let (mut reused, mut fresh) =
            spawn_two("swap", &files, &TemplateContext::new().with("n", &1));
        let b_sheet = reused.load::<Stylesheet>("b.css");
        let b_sheet_fresh = fresh.load::<Stylesheet>("b.css");
        assert_same_after(&mut reused, &mut fresh, move |ui, keep| {
            let root = ui.root();
            if !keep {
                ui.world_mut().entity_mut(root).remove::<Skeleton>();
            }
            ui.world_mut()
                .entity_mut(root)
                .insert(HtmlStylesheet(if keep {
                    b_sheet.clone()
                } else {
                    b_sheet_fresh.clone()
                }));
            set_context(ui, "n", "2");
        });
        let dump = reused.dump();
        assert!(
            dump.contains("\"Static\" default 16px #00ff00"),
            "the static paragraph kept the previous sheet's color:\n{dump}"
        );
    }

    /// A change that arrives while the replacement sheet is loading is
    /// merged into the build that resolves it (a failed sheet emits no
    /// event): that build looks content-only, but the styles changed —
    /// no reuse. The ready override (blue) is replaced by a failing
    /// sheet; the default sheet (red) must take over.
    #[test]
    fn change_while_a_sheet_loads_restyles_when_it_fails() {
        let files = [
            (
                "page.html",
                "<p class=\"static\">Static</p><p>Value: {{ n }}</p>",
            ),
            (
                "a.css",
                "html { color: #ffffff } .static { color: #ff0000 }",
            ),
            (
                "b.css",
                "html { color: #ffffff } .static { color: #0000ff }",
            ),
        ];
        let mut reused = TestUi::new("failed-sheet-reused", &files)
            .stylesheet("a.css")
            .spawn(
                "page.html",
                TemplateContext::new().with("n", &1),
                Node::default(),
            );
        let b_sheet = reused.load::<Stylesheet>("b.css");
        let reused_root = reused.root();
        reused
            .world_mut()
            .entity_mut(reused_root)
            .insert(HtmlStylesheet(b_sheet));
        reused.settle();
        assert!(
            reused.dump().contains("#0000ff"),
            "the override should style the paragraph blue:\n{}",
            reused.dump()
        );
        let mut fresh = TestUi::new("failed-sheet-fresh", &files)
            .stylesheet("a.css")
            .spawn(
                "page.html",
                TemplateContext::new().with("n", &1),
                Node::default(),
            );
        let b_sheet = fresh.load::<Stylesheet>("b.css");
        let fresh_root = fresh.root();
        fresh
            .world_mut()
            .entity_mut(fresh_root)
            .insert(HtmlStylesheet(b_sheet));
        fresh.settle();

        // The swap plus a change arrive while the sheet loads (a `Wait` —
        // the render lands that frame); the sheet then fails without an
        // event, and a further change (the sheet untouched — re-requesting
        // it would flip it to `Loading` for a frame) is the build that
        // resolves the merge.
        for (ui, keep) in [(&mut reused, true), (&mut fresh, false)] {
            let root = ui.root();
            if !keep {
                ui.world_mut().entity_mut(root).remove::<Skeleton>();
            }
            let missing: Handle<Stylesheet> = ui.load("missing.css");
            ui.world_mut()
                .entity_mut(root)
                .insert(HtmlStylesheet(missing));
            set_context(ui, "n", "2");
        }
        reused.settle();
        fresh.settle();
        set_context(&mut reused, "n", "3");
        set_context(&mut fresh, "n", "3");
        assert_eq!(
            reused.settle().dump(),
            fresh.settle().dump(),
            "the failed sheet's update reused the stale styles"
        );
        assert!(
            reused.dump().contains("\"Static\" default 16px #ff0000"),
            "the static paragraph should fall back to the default sheet:\n{}",
            reused.dump()
        );
    }

    /// A render that changed while the debug outline froze the skeleton
    /// (or while a `Wait` swallowed the build) must not leave the
    /// skeleton's document older than its specs: the next reuse would
    /// pair against the wrong document.
    #[test]
    fn render_change_during_outline_frames_stays_in_step() {
        let files = [(
            "page.html",
            "<p>{{ x }}</p><p class=\"static\">Static</p><p>{{ y }}</p>",
        )];
        let sheet = "html { color: #ffffff }";
        let files = [("page.html", files[0].1), ("a.css", sheet)];
        let (mut reused, mut fresh) = spawn_two(
            "outline-freeze",
            &files,
            &TemplateContext::new().with("x", &"a").with("y", &"b"),
        );
        reused.settle();
        fresh.settle();

        // The render changes while the outline is on (the skeleton is
        // frozen at `x = a`), the outline goes with NO content change (the
        // specs are replaced from the `x = b` render; the stored document
        // must follow), and a change back to `x = a` reuses: paired
        // against the stale document, `x` looks clean and the cached
        // spec's `b` would stick.
        let toggle_outline = |ui: &mut TestUi, outlined: bool, strip: bool| {
            let root = ui.root();
            if strip {
                ui.world_mut().entity_mut(root).remove::<Skeleton>();
            }
            let mut entity = ui.world_mut().entity_mut(root);
            if outlined {
                entity.insert(HtmlDebugOutline);
            } else {
                entity.remove::<HtmlDebugOutline>();
            }
        };
        for (ui, keep) in [(&mut reused, true), (&mut fresh, false)] {
            toggle_outline(ui, true, !keep);
            set_context(ui, "x", "b");
        }
        reused.settle();
        fresh.settle();
        assert_eq!(reused.dump(), fresh.dump());
        for (ui, keep) in [(&mut reused, true), (&mut fresh, false)] {
            toggle_outline(ui, false, !keep);
        }
        reused.settle();
        fresh.settle();
        assert_eq!(
            reused.dump(),
            fresh.dump(),
            "the skeleton fell out of step with its document"
        );
        set_context(&mut reused, "x", "a");
        set_context(&mut fresh, "x", "a");
        set_context(&mut reused, "y", "d");
        set_context(&mut fresh, "y", "d");
        reused.settle();
        fresh.settle();
        assert_eq!(
            reused.dump(),
            fresh.dump(),
            "the retried pairing paired against the wrong document"
        );
        assert!(
            reused.dump().contains("\"a\" default 16px #ffffff")
                && reused.dump().contains("\"d\" default 16px #ffffff"),
            "wrong text after the outline came off:\n{}",
            reused.dump()
        );
    }

    /// After builds the adaptive gate skipped (reuse scored useless), the
    /// skeleton must still describe the entities: a gate-skipped build
    /// updates them from fresh specs, so its specs must replace the
    /// cached ones. The sequence: enough one-key changes to switch the
    /// gate off, a change the skipped builds apply (`t = 5`), a restyle
    /// (re-styles the entities from the current document), then a change
    /// back (`t = 0`) whose subtree pairs clean against a stale document
    /// and would keep the stale content.
    #[test]
    fn gate_skipped_builds_stay_sound() {
        let rows = (0..9)
            .map(|i| format!("<p>Row {i}: {{{{ v }}}}</p>"))
            .collect::<String>();
        let page = format!("<p class=\"t\">T: {{{{ t }}}}</p>{rows}");
        let files = [
            ("page.html", page.as_str()),
            ("a.css", "html { color: #ffffff }"),
            ("a2.css", "html { color: #ffffff }"),
        ];
        let (mut reused, mut fresh) = spawn_two(
            "gate",
            &files,
            &TemplateContext::new().with("v", &0).with("t", &0),
        );
        reused.settle();
        fresh.settle();

        let toggle = |ui: &mut TestUi, keep: bool, v: u32| {
            let root = ui.root();
            if !keep {
                ui.world_mut().entity_mut(root).remove::<Skeleton>();
            }
            ui.world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("v", &v);
        };
        // One-key changes: the first retry scores useless (1 of 10
        // subtrees), the gate switches off for the next 2^strike builds.
        for step in 1..=33u32 {
            let v = step % 2;
            toggle(&mut reused, true, v);
            toggle(&mut fresh, false, v);
            reused.settle();
            fresh.settle();
            assert_eq!(reused.dump(), fresh.dump(), "toggle {step}");
        }

        // Applied by a gate-skipped build, then a restyle, then back.
        for (ui, keep) in [(&mut reused, true), (&mut fresh, false)] {
            let root = ui.root();
            if !keep {
                ui.world_mut().entity_mut(root).remove::<Skeleton>();
            }
            ui.world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("t", &5);
        }
        reused.settle();
        fresh.settle();
        assert_eq!(reused.dump(), fresh.dump());
        let a2 = reused.load::<Stylesheet>("a2.css");
        reused.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(a2);
        let a2 = fresh.load::<Stylesheet>("a2.css");
        fresh.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(a2);
        reused.settle();
        fresh.settle();
        assert_eq!(reused.dump(), fresh.dump());

        toggle(&mut reused, true, 1);
        toggle(&mut fresh, false, 1);
        {
            let root = reused.root();
            reused
                .world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("t", &0);
        }
        {
            let root = fresh.root();
            fresh
                .world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("t", &0);
        }
        reused.settle();
        fresh.settle();
        assert_eq!(
            reused.dump(),
            fresh.dump(),
            "the retried pairing served a skeleton the gate-skipped builds left stale"
        );
        assert!(
            reused.dump().contains("\"T: 0\" default 16px #ffffff"),
            "the t row kept its stale content:\n{}",
            reused.dump()
        );
    }

    /// `:hover` keeps working inside a reused subtree: the skip refreshes
    /// the subtree's `DomNode` handles, which the state walk keys on.
    #[test]
    fn hover_applies_inside_reused_subtrees() {
        let files = [
            (
                "page.html",
                // The loop shifts every later node's index between builds:
                // a stale handle then no longer keys the state walk.
                "<p>{% for i in range(end=n) %}<b>r</b>{% endfor %}</p><div class=\"panel\"><p class=\"x\">Hover me</p></div>",
            ),
            (
                "a.css",
                "html { color: #ffffff; font-size: 20px } .x:hover { color: #ff0000 }",
            ),
        ];
        let (mut reused, _fresh) =
            spawn_two("hover", &files, &TemplateContext::new().with("n", &1));
        reused.settle();

        // Content changes around the clean panel: its subtree reuses.
        // `n` is a number: it bounds a Tera `range`.
        for n in 2..=4u32 {
            let root = reused.root();
            reused
                .world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("n", &n);
            reused.settle();
        }

        let hovered = reused
            .world_mut()
            .query::<(Entity, &bevy_markup::html::HtmlElement)>()
            .iter(reused.world_mut())
            .find(|(_, element)| element.classes.iter().any(|class| class == "x"))
            .expect("the hoverable element")
            .0;
        reused
            .world_mut()
            .entity_mut(hovered)
            .insert(bevy_markup::signals::PseudoState {
                hovered: true,
                ..default()
            });
        let dump = reused.settle().dump();
        assert!(
            dump.contains("\"Hover me\" default 20px #ff0000"),
            ":hover was lost in the reused subtree:\n{dump}"
        );
    }
}

// ---------------------------------------------------------------------------
// Loader regressions
// ---------------------------------------------------------------------------

/// Markdown conversion is chosen per file (bug_0060): an `.html` partial
/// included from a `.md` page stays HTML — its indented HTML is not a
/// Markdown indented code block — and a `.md` partial included from an
/// `.html` page converts. (The blank line ends the raw-HTML block, so
/// converting `part.html` would wrap the indented `<p>` in `pre.markdown`.)
#[test]
fn markdown_conversion_is_per_file() {
    TestUi::new(
        "markdown-partials",
        &[
            (
                "page.md",
                "# Title\n\n{% include \"part.html\" %}\n\nTrailing.\n",
            ),
            (
                "part.html",
                "<div class=\"card\">\n\n    <p id=\"part\">indented</p>\n</div>\n",
            ),
        ],
    )
    .spawn("page.md", TemplateContext::new(), Node::default())
    .settle()
    .assert_dump(
        r#"
html-ui
  h1.markdown
    "Title" default 16px #ffffff
  div.card
    p#part
      "indented" default 16px #ffffff
  p.markdown
    "Trailing." default 16px #ffffff
"#,
    );

    TestUi::new(
        "markdown-partial-from-html",
        &[
            ("page.html", "<h2>Doc</h2>\n{% include \"note.md\" %}\n"),
            ("note.md", "# Note\n\nHello *there*.\n"),
        ],
    )
    .spawn("page.html", TemplateContext::new(), Node::default())
    .settle()
    .assert_dump(
        r#"
html-ui
  h2
    "Doc" default 16px #ffffff
  h1.markdown
    "Note" default 16px #ffffff
  p.markdown
    "Hello there." default 16px #ffffff
"#,
    );
}

/// A render that changes while the active bundle is absent (loading, here:
/// failed) drops the previous document's translations — the elements show
/// their fallback content, not translations keyed on the old DOM's handles
/// (which would land on whatever element now sits at those handles;
/// bug_0064). When a real bundle arrives, the fresh document localizes.
#[test]
fn fresh_render_without_a_bundle_shows_fallback_not_stale_translations() {
    let mut ui = TestUi::new(
        "stale-localize",
        &[
            (
                "page.html",
                "{% if extra %}<div>extra</div>\n{% endif %}<p data-l10n-id=\"m\">fallback</p>",
            ),
            ("a.ftl.ron", "(locale: \"en-US\", resources: [\"a.ftl\"])"),
            ("a.ftl", "m = translated\n"),
            (
                "bad.ftl.ron",
                "(locale: \"en-US\", resources: [\"missing.ftl\"])",
            ),
            ("c.ftl.ron", "(locale: \"en-US\", resources: [\"c.ftl\"])"),
            ("c.ftl", "m = localized again\n"),
        ],
    )
    .locale("a.ftl.ron")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "translated" default 16px #ffffff
"#,
    );

    // The bundle fails while the context change re-renders in the same
    // frame: the fresh document adds a `<div>` before the `<p>`, shifting
    // the DOM handles the old map keyed on.
    let bad = ui.load::<LocaleBundle>("bad.ftl.ron");
    let root = ui.root();
    ui.world_mut().resource_mut::<ActiveLocale>().0 = Some(bad);
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("extra", &true);
    ui.settle().assert_dump(
        r#"
html-ui
  div
    -
      "extra" default 16px #ffffff
  p
    "fallback" default 16px #ffffff
"#,
    );

    // A real bundle arrives: the fresh document localizes normally.
    let c = ui.load::<LocaleBundle>("c.ftl.ron");
    ui.world_mut().resource_mut::<ActiveLocale>().set(c);
    ui.settle().assert_dump(
        r#"
html-ui
  div
    -
      "extra" default 16px #ffffff
  p
    "localized again" default 16px #ffffff
"#,
    );
}

/// A hot reload delivers `Modified` and `LoadedWithDependencies` in one
/// frame: the event scan must fold over all of them (one re-resolve, one
/// `HtmlUiBuilt`), not stop at the first match and re-serve the second next
/// frame (bug_0068).
#[test]
fn one_frame_bundle_events_build_once() {
    let mut ui = TestUi::new(
        "bundle-events",
        &[
            ("page.html", "<p data-l10n-id=\"m\">fallback</p>"),
            ("b.ftl.ron", "(locale: \"en-US\", resources: [\"b.ftl\"])"),
            ("b.ftl", "m = translated\n"),
        ],
    )
    .locale("b.ftl.ron")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "translated" default 16px #ffffff
"#,
    );
    let id = ui
        .world_mut()
        .resource::<ActiveLocale>()
        .0
        .as_ref()
        .expect("active bundle")
        .id();
    let builds = ui.builds();
    // Both events of a hot reload, delivered in one frame.
    let mut messages = ui
        .world_mut()
        .resource_mut::<Messages<AssetEvent<LocaleBundle>>>();
    messages.write(AssetEvent::Modified { id });
    messages.write(AssetEvent::LoadedWithDependencies { id });
    ui.update(10);
    assert_eq!(
        ui.builds(),
        builds + 1,
        "both events in one frame re-resolve once"
    );
    ui.assert_dump(
        r#"
html-ui
  p
    "translated" default 16px #ffffff
"#,
    );
}

/// Commented-out and raw-block includes are not references (bug_0067): an
/// include of a deleted file inside them must not fail the load; Tera
/// prints the raw block's tag literally. (The fenced-code/code-span skip is
/// covered by the loader unit test `comments_raw_and_code_are_not_references`:
/// end to end, a fenced `{% include %}` still fails the load at Tera's own
/// add-time include validation, so no vector can assert a literal code
/// sample while the load succeeds.)
#[test]
fn comments_raw_and_code_include_nothing() {
    let mut ui = TestUi::new(
        "skipped-includes",
        &[(
            "page.html",
            "{# {%- include \"gone.html\" -%} #}\n<p>after comment</p>\n\
             {% raw %}{% include \"gone.html\" %}{% endraw %}\n",
        )],
    );
    let handle = ui.load::<HtmlTemplate>("page.html");
    let mut ui = ui.spawn("page.html", TemplateContext::new(), Node::default());
    // A failing template never builds (the UI stays `Pending`), so settle
    // quietly and check the load state before the dump.
    ui.settle_quiet();
    assert!(
        !ui.world_mut()
            .resource::<AssetServer>()
            .load_state(handle.id())
            .is_failed(),
        "the commented-out and raw-block includes must not fail the load"
    );
    ui.assert_dump(
        r#"
html-ui
  p
    "after comment" default 16px #ffffff
  -
    "{% include \"gone.html\" %}" default 16px #ffffff
"#,
    );
}

/// A `url()` inside a `--` declaration can be anything (a data URI, a
/// scheme): an unresolvable one is skipped at load instead of failing the
/// whole stylesheet (a `data:image/svg+xml` value in a custom property is
/// the normal shape for an icon registry).
#[test]
fn a_custom_property_url_does_not_fail_the_sheet() {
    let sheet = ":root { --icon: url(\"data:image/svg+xml;utf8,<svg/>\") } \
         p { --x: url(\"https://example.com/icon.svg\") } \
         html { color: #ffffff } p { color: #ff0000 }";
    let mut ui = TestUi::new(
        "custom-url",
        &[("page.html", r#"<p>colored</p>"#), ("style.css", sheet)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle().assert_dump(
        r#"
html-ui
  p
    "colored" default 16px #ff0000
"#,
    );
}

/// Template includes and FTL resource paths may not escape the asset root
/// with `../` paths (bug_0069): the include fails its resolution without
/// reading the outside file, and a manifest naming an outside resource
/// fails its bundle load.
#[test]
fn unapproved_paths_fail_their_loads() {
    static RUN: AtomicUsize = AtomicUsize::new(0);
    let unique = |suffix: &str| {
        format!(
            "bevy_markup-escape-{}-{}.{suffix}",
            std::process::id(),
            RUN.fetch_add(1, Ordering::Relaxed)
        )
    };

    // A file one level above the (temporary) asset root, which the
    // include names.
    let outside = unique("html");
    std::fs::write(std::env::temp_dir().join(&outside), "<p>outside</p>").unwrap();
    let page = format!("{{% include \"../{outside}\" %}}");
    let mut ui = TestUi::new("unapproved-include", &[("page.html", page.as_str())]);
    let handle = ui.load::<HtmlTemplate>("page.html");
    ui.settle_quiet();
    assert!(
        ui.world_mut()
            .resource::<AssetServer>()
            .load_state(handle.id())
            .is_failed(),
        "an include escaping the asset root must fail the load"
    );
    drop(ui);
    let _ = std::fs::remove_file(std::env::temp_dir().join(&outside));

    let resource = unique("ftl");
    std::fs::write(std::env::temp_dir().join(&resource), "m = outside\n").unwrap();
    let manifest = format!("(locale: \"en-US\", resources: [\"../{resource}\"])");
    let mut ui = TestUi::new("unapproved-resource", &[("bad.ftl.ron", manifest.as_str())]);
    let bundle = ui.load::<LocaleBundle>("bad.ftl.ron");
    ui.settle_quiet();
    assert!(
        ui.world_mut()
            .resource::<AssetServer>()
            .load_state(bundle.id())
            .is_failed(),
        "a manifest resource escaping the asset root must fail the load"
    );
    drop(ui);
    let _ = std::fs::remove_file(std::env::temp_dir().join(&resource));
}

/// `Above`/`Left` overlays clamp to the viewport once their own size is
/// known (bug_0077): the `bottom`/`right` inset is measured from the
/// viewport's far edge, so staying inside means `inset <= limit - extent`
/// — the overlay's extent, not the inset alone. An element flush with
/// the top (left) edge gets `bottom` `= view_h - height` (`right =
/// view_w - width`) — the overlay sits flush inside the edge — not
/// `view_h + gap`, which put the whole overlay past the viewport's edge.
#[test]
fn anchored_overlay_above_left_clamp_to_the_viewport() {
    let mut ui = TestUi::with_layout(
        "anchor-edge",
        &[
            ("page.html", r#"<div id="a"></div>"#),
            (
                "style.css",
                "html { padding: 0 }\n#a { width: 100px; height: 30px }",
            ),
        ],
        UVec2::new(320, 240),
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();
    let root = ui.root();
    let world = ui.world_mut();
    let a = element_by_id(world, root, "a");
    assert_eq!(
        node_rect(world, a),
        Some(Rect::new(0.0, 0.0, 100.0, 30.0)),
        "the element sits flush with the viewport's top-left corner"
    );
    let overlay = world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(60.0),
                height: Val::Px(20.0),
                ..default()
            },
            HtmlAnchor::new(a, AnchorPlacement::Above).with_gap(4.0),
        ))
        .id();
    let placed = |ui: &mut TestUi| {
        ui.update(3);
        let world = ui.world_mut();
        let node = world.get::<Node>(overlay).unwrap();
        (
            node_rect(world, overlay).unwrap(),
            (node.left, node.top, node.right, node.bottom),
        )
    };

    // Above: `bottom` counts up from the viewport's bottom edge; the
    // unclamped value is `240 - 0 + 4` = 244, and keeping the 20px-tall
    // overlay inside the viewport clamps it to `240 - 20` — the overlay
    // sits at the top edge, flush (the gap is given up, the tooltip is
    // not placed off screen).
    let (rect, insets) = placed(&mut ui);
    assert_eq!(
        insets,
        (Val::Px(0.0), Val::Auto, Val::Auto, Val::Px(220.0)),
        "the bottom inset clamps to `viewport - overlay height` (unclamped: 240 + gap)"
    );
    assert_eq!(
        rect,
        Rect::new(0.0, 0.0, 60.0, 20.0),
        "the overlay is inside the viewport, not above it"
    );

    ui.world_mut()
        .get_mut::<HtmlAnchor>(overlay)
        .unwrap()
        .placement = AnchorPlacement::Left;
    let (rect, insets) = placed(&mut ui);
    assert_eq!(
        insets,
        (Val::Auto, Val::Px(0.0), Val::Px(260.0), Val::Auto),
        "the right inset clamps to `viewport - overlay width` (unclamped: 320 + gap)"
    );
    assert_eq!(
        rect,
        Rect::new(0.0, 0.0, 60.0, 20.0),
        "the overlay is inside the viewport, not left of it"
    );
}

/// `HtmlWorldAnchor` divides the projection by `UiScale` and measures
/// against the viewport, not the render target (bug_0078): with
/// `UiScale(2.0)` the root sits at half the projected window px (not
/// double), with an offset (split-screen) viewport a point inside the
/// viewport reads on-screen instead of hidden, and the on-screen bounds
/// are tested in the same units as the position (with `UiScale` ≠ 1 the
/// old mixed-unit test hid visible points and showed off-screen ones).
#[test]
fn world_anchor_respects_ui_scale_and_viewport_origin() {
    // With `UiScale(2.0)`: the point (0.5, 0.5) projects to window px
    // (240, 60), and the anchor writes UI px — half of that.
    let mut ui = TestUi::with_layout("world-anchor-scale", &[], UVec2::new(320, 240));
    ui.world_mut().insert_resource(UiScale(2.0));
    let world = ui.world_mut();
    let target = world.spawn(Visibility::Inherited).id();
    world.entity_mut(target).insert((
        Transform::from_translation(Vec3::new(0.25, 0.5, 0.0)),
        GlobalTransform::from_translation(Vec3::new(0.25, 0.5, 0.0)),
    ));
    let overlay = world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(20.0),
                height: Val::Px(10.0),
                ..default()
            },
            HtmlWorldAnchor::new(target).with_offset(Vec3::new(0.25, 0.0, 0.0)),
        ))
        .id();
    let state = |ui: &mut TestUi, overlay: Entity| {
        ui.update(3);
        let world = ui.world_mut();
        let node = world.get::<Node>(overlay).unwrap();
        (
            (node.left, node.top),
            *world.get::<Visibility>(overlay).unwrap(),
            *world.get::<HtmlWorldAnchorView>(overlay).unwrap(),
        )
    };
    assert_eq!(
        state(&mut ui, overlay),
        (
            (Val::Px(110.0), Val::Px(20.0)),
            Visibility::Inherited,
            HtmlWorldAnchorView {
                distance: (0.5f32.powi(2) * 2.0).sqrt(),
                on_screen: true,
            }
        ),
        "window px (240, 60) divided by the UiScale of 2, minus the pivot"
    );

    // A split-screen camera with an offset viewport: projections come back
    // relative to the render target, so measuring against the viewport
    // (not the target's full size) keeps the point on-screen.
    let mut ui = TestUi::with_layout("world-anchor-split", &[], UVec2::new(320, 240));
    let world = ui.world_mut();
    let target = world.spawn(Visibility::Inherited).id();
    world.entity_mut(target).insert((
        Transform::from_translation(Vec3::new(0.25, 0.5, 0.0)),
        GlobalTransform::from_translation(Vec3::new(0.25, 0.5, 0.0)),
    ));
    let split = world
        .spawn((
            Camera2d,
            Camera {
                viewport: Some(bevy::camera::Viewport {
                    physical_position: UVec2::new(160, 80),
                    physical_size: UVec2::new(160, 120),
                    ..default()
                }),
                computed: bevy::camera::ComputedCameraValues {
                    target_info: Some(bevy::camera::RenderTargetInfo {
                        physical_size: UVec2::new(320, 240),
                        scale_factor: 1.0,
                    }),
                    ..default()
                },
                ..default()
            },
            bevy::camera::RenderTarget::None {
                size: UVec2::new(320, 240),
            },
            GlobalTransform::from_translation(Vec3::ZERO),
        ))
        .id();
    let overlay = world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(20.0),
                height: Val::Px(10.0),
                ..default()
            },
            HtmlWorldAnchor::new(target)
                .with_offset(Vec3::new(0.25, 0.0, 0.0))
                .with_camera(split),
        ))
        .id();
    // The point (0.5, 0.5) projects to window px (280, 110) — (160, 80)
    // into the viewport, well inside its 160×120 (on-screen). The UI root
    // spans the full window (its own `UiTargetCamera` would shift the
    // origin), so the plate lands at the point's window position.
    let ((left, top), visibility, view) = state(&mut ui, overlay);
    assert_eq!(
        (left, top),
        (Val::Px(270.0), Val::Px(100.0)),
        "(280, 110) in the full-window UI's px, minus the pivot"
    );
    assert_eq!(visibility, Visibility::Inherited, "the point is on screen");
    assert!(view.on_screen);

    // The bounds use the same units as the position: with `UiScale(2.0)`
    // a point past the window's edge (world 1.0 + offset 0.25 projects
    // past window px 320) is off screen — the old mixed-unit test
    // compared the divided position with the undivided size and showed
    // it.
    let mut ui = TestUi::with_layout("world-anchor-scale-far", &[], UVec2::new(320, 240));
    ui.world_mut().insert_resource(UiScale(2.0));
    let world = ui.world_mut();
    let target = world.spawn(Visibility::Inherited).id();
    world.entity_mut(target).insert((
        Transform::from_translation(Vec3::new(1.0, 0.5, 0.0)),
        GlobalTransform::from_translation(Vec3::new(1.0, 0.5, 0.0)),
    ));
    let overlay = world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            HtmlWorldAnchor::new(target).with_offset(Vec3::new(0.25, 0.0, 0.0)),
        ))
        .id();
    let (_, visibility, view) = state(&mut ui, overlay);
    assert_eq!(visibility, Visibility::Hidden, "off the window's edge");
    assert!(!view.on_screen);

    // And with `UiScale(0.5)` a visible point (window px 240 of 320) is
    // on screen — the old test hid it (divided: 480, compared with 320).
    let mut ui = TestUi::with_layout("world-anchor-scale-half", &[], UVec2::new(320, 240));
    ui.world_mut().insert_resource(UiScale(0.5));
    let world = ui.world_mut();
    let target = world.spawn(Visibility::Inherited).id();
    world.entity_mut(target).insert((
        Transform::from_translation(Vec3::new(0.25, 0.5, 0.0)),
        GlobalTransform::from_translation(Vec3::new(0.25, 0.5, 0.0)),
    ));
    let overlay = world
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            HtmlWorldAnchor::new(target).with_offset(Vec3::new(0.25, 0.0, 0.0)),
        ))
        .id();
    let (_, visibility, view) = state(&mut ui, overlay);
    assert_eq!(visibility, Visibility::Inherited, "the point is visible");
    assert!(view.on_screen);
}

/// A headless app driving `update()` directly — never calling `finish()`
/// — must not panic on the focus systems: the navigation resources
/// `HtmlFocus` reads are registered in `build` too (F18; the R59 fix
/// moved the plugin itself into `finish`, and `app.update()` without
/// `finish()` failed the systems with "Resource does not exist").
#[test]
fn update_without_finish_drives_the_focus_systems() {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::input_focus::directional_navigation::{
        AutoNavigationConfig, DirectionalNavigationMap,
    };
    #[allow(unused_imports)]
    use bevy::prelude::*;
    let _ = (
        DirectionalNavigationMap::default(),
        AutoNavigationConfig::default(),
    );
    // A minimal app without the harness (which calls `finish()`): the
    // same plugin stack as `TestUi::new` minus `app.finish()`.
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        ImagePlugin::default(),
        BevyMarkupPlugin,
    ));
    // bevy_render normally registers the image loader.
    app.register_asset_loader(bevy::image::ImageLoader::new(
        bevy::image::CompressedImageFormats::empty(),
    ));
    // Nothing else: the plugin itself registers what the text-input
    // widget's systems read (review_0003 G11: `PointerState` was missing,
    // and the first update failed validation).
    app.update();
    // `HtmlFocus` is app-driven (`run_system_once`): its navigator's
    // params must validate without `finish()`.
    app.world_mut()
        .run_system_once(|mut focus: HtmlFocus| {
            let _ = focus.focused();
        })
        .expect("HtmlFocus's navigator validated without finish()");
    app.update();
}

// ADR 0029: scoped interaction restyles. Every scoped restyle must equal
// a fresh build of the same state; the twins below force the reference
// with a content bump (a content build carrying a style change reuses
// nothing — the pre-scoping path).

/// The shared setup: a page whose `{{ n }}`/`data-bump` lets the twin
/// force a full rebuild without changing the dump.
fn scoped_flip_pair(
    name: &str,
    page: &str,
    css: &str,
    context: TemplateContext,
) -> (TestUi, TestUi) {
    let mut target = TestUi::new(name, &[("page.html", page), ("style.css", css)])
        .stylesheet("style.css")
        .spawn("page.html", context.clone(), Node::default());
    target.settle();
    let mut fresh = TestUi::new(
        &format!("{name}-fresh"),
        &[("page.html", page), ("style.css", css)],
    )
    .stylesheet("style.css")
    .spawn("page.html", context, Node::default());
    fresh.settle();
    (target, fresh)
}

fn bump_target(ui: &mut TestUi, step: u32) {
    let root = ui.root();
    let world = ui.world_mut();
    world
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &step);
    ui.settle();
}

/// A flip on an element no pseudo-referencing selector could match
/// restyles nothing: the flip re-collects the element (it is on the flip's
/// path), its fresh style equals the cached one, and the dump is
/// unchanged.
#[test]
fn flip_on_an_unreactive_element_changes_nothing() {
    let page = r#"<div data-bump="{{ n }}"><p id="plain">Text</p></div>"#;
    let mut ui = TestUi::new(
        "scoped-unreactive",
        &[("page.html", page), ("style.css", "p { color: #ffffff }")],
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new().with("n", &0),
        Node::default(),
    );
    ui.settle();
    let before = ui.dump();
    let root = ui.root();
    let plain = element_by_id(ui.world_mut(), root, "plain");
    ui.world_mut().entity_mut(plain).insert(PseudoState {
        hovered: true,
        ..default()
    });
    ui.settle();
    assert_eq!(ui.dump(), before, "an unreactive flip restyles nothing");
}

/// A `var()` change is a full-page restyle under the scoping: every row
/// recolors, matching a fresh build (the vars' consumers are unknown, so
/// nothing reuses — but the result must be right).
#[test]
fn var_change_restyles_everything_under_scoping() {
    let page = r#"<div class="hot" id="hot"><p>hot</p></div>
        {% for i in rows %}<div class="row" data-bump="{{ n }}"><p style="color: var(--c)">row {{ i }}</p></div>{% endfor %}"#;
    let css = "html { --c: #ff0000 } .hot:hover { color: #00ff00 }";
    let context = || {
        TemplateContext::new()
            .with("--c", &"#ff0000")
            .with("rows", &[0, 1, 2])
            .with("n", &0)
    };
    let (mut target, mut fresh) = scoped_flip_pair("scoped-var", page, css, context());
    let before = target.dump();

    // The var change on the target, the same change plus a bump on the
    // fresh twin (which forces its full rebuild).
    let root = target.root();
    target
        .world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("--c", &"#00ff00");
    target.settle();
    let root = fresh.root();
    fresh
        .world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("--c", &"#00ff00");
    bump_target(&mut fresh, 1);
    assert!(before.contains("#ff0000"), "the rows started red");
    assert!(
        target.dump().contains("#00ff00"),
        "the var recolored the rows"
    );
    assert_eq!(target.dump(), fresh.dump(), "the var restyle matches fresh");
}

/// A flip and a content change in one frame: the content build carries
/// the style change and reuses nothing (style_pending), and the dump
/// matches a fresh twin — the flipped element's state survives the
/// re-render (the flip's handle numbering moved with the new document).
#[test]
fn flip_and_content_change_in_one_frame() {
    let page = r#"<div class="hot" id="hot"><p>hot {{ n }}</p></div>
        <p>plain {{ n }}</p>"#;
    let css = ".hot:hover { color: #00ff00 }";
    let (mut target, mut fresh) = scoped_flip_pair(
        "scoped-flip-content",
        page,
        css,
        TemplateContext::new().with("n", &0),
    );
    let hot = {
        let root = target.root();
        element_by_id(target.world_mut(), root, "hot")
    };
    // The flip and the content change land in the same frame.
    let root = target.root();
    target.world_mut().entity_mut(hot).insert(PseudoState {
        hovered: true,
        ..default()
    });
    target
        .world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &1);
    target.settle();
    // The twin: the same flip, then the same content change (its own
    // flip lands first, scoped; the bump then forces the full rebuild).
    let fhot = {
        let froot = fresh.root();
        element_by_id(fresh.world_mut(), froot, "hot")
    };
    fresh.world_mut().entity_mut(fhot).insert(PseudoState {
        hovered: true,
        ..default()
    });
    fresh.settle();
    bump_target(&mut fresh, 1);
    assert!(
        target.dump().contains("\"hot 1\" default 16px #00ff00"),
        "{})",
        target.dump()
    );
    assert_eq!(target.dump(), fresh.dump(), "the held flip matches fresh");
}

/// Focus restyles are scoped too: `:focus-visible` marks the focused
/// element in place, moving focus moves the mark, and clearing focus
/// restores the start — each step matching a fresh twin.
#[test]
fn focus_restyles_scoped() {
    let page = r#"<div id="a" tabindex="0"><p>A</p></div>
        <div id="b" tabindex="0"><p>B</p></div>"#;
    let css = "div:focus-visible { background-color: #111111 }";
    let (mut target, mut fresh) =
        scoped_flip_pair("scoped-focus", page, css, TemplateContext::new());
    let start = target.dump();
    let mark = |ui: &mut TestUi, id: &str| {
        let root = ui.root();
        let entity = element_by_id(ui.world_mut(), root, id);
        ui.world_mut()
            .resource_mut::<bevy::input_focus::InputFocus>()
            .set(entity, bevy::input_focus::FocusCause::Navigated);
        ui.world_mut()
            .resource_mut::<bevy::input_focus::InputFocusVisible>()
            .0 = true;
        ui.settle();
    };
    mark(&mut target, "a");
    mark(&mut fresh, "a");
    let marked = target.dump();
    assert!(
        marked.contains("div#a focus bg=#111111"),
        "a marked: {marked}"
    );
    assert_eq!(marked, fresh.dump(), "the focus restyle matches fresh");
    mark(&mut target, "b");
    mark(&mut fresh, "b");
    let after = target.dump();
    assert_eq!(after, fresh.dump(), "the moved focus matches fresh");
    assert!(
        after.contains("div#a focus") && !after.contains("div#a focus bg"),
        "a unmarked: {after}"
    );
    assert!(
        after.contains("div#b focus bg=#111111"),
        "b marked: {after}"
    );
    // Hiding the focus mark restores the start (clearing focus itself
    // would only trigger the repair system: headless, nothing took it
    // back, so focus is restored to a focusable immediately).
    for ui in [&mut target, &mut fresh] {
        ui.world_mut()
            .resource_mut::<bevy::input_focus::InputFocusVisible>()
            .0 = false;
        ui.settle();
    }
    assert_eq!(target.dump(), start, "the hidden mark restored the start");
    assert_eq!(target.dump(), fresh.dump());
}

/// A flip on a *link span* (the run inside a block): the block's
/// reactivity covers its runs' inline elements, so the span's color
/// restyles without the block's own state moving.
#[test]
fn link_span_flip_restyles_its_block() {
    use bevy::ecs::system::RunSystemOnce;
    let page = r#"<p data-bump="{{ n }}">See <a href="https://example.test" id="link">the link</a> now</p>"#;
    let css = "a:hover { color: #ff0000 }";
    let (mut target, mut fresh) = scoped_flip_pair(
        "scoped-link",
        page,
        css,
        TemplateContext::new().with("n", &0),
    );
    let flip_span = |ui: &mut TestUi, hovered: bool| {
        // The link's span, via `HtmlElements::by_tag` (the links example's
        // own discovery path).
        use bevy::ecs::system::RunSystemOnce;
        let root = ui.root();
        let span = ui
            .world_mut()
            .run_system_once(move |elements: HtmlElements| {
                elements.by_tag(root, "a").next().expect("the link span")
            })
            .expect("span lookup");
        ui.world_mut().entity_mut(span).insert(PseudoState {
            hovered,
            ..default()
        });
        ui.settle();
    };
    flip_span(&mut target, true);
    // The twin: the same flip (a scoped restyle), then a content change
    // to force the full rebuild — its fresh collection reads the state.
    let root = fresh.root();
    let fspan = fresh
        .world_mut()
        .run_system_once(move |elements: HtmlElements| {
            elements.by_tag(root, "a").next().expect("the link span")
        })
        .expect("span lookup");
    fresh.world_mut().entity_mut(fspan).insert(PseudoState {
        hovered: true,
        ..default()
    });
    bump_target(&mut fresh, 1);
    let target_dump = target.dump();
    assert!(
        target_dump.contains("\"the link\" default 16px #ff0000"),
        "the link restyled: {target_dump}"
    );
    assert_eq!(target_dump, fresh.dump(), "the span flip matches fresh");
}

/// An ancestor-keyed rule two levels up: the flip on the padded root
/// restyles the deep label through the chain (`.pad:hover .mid .label`).
#[test]
fn deep_ancestor_rule_restyles_the_chain() {
    let page = r#"<div class="pad" id="pad">
        <div class="mid"><p class="label" id="deep">deep</p></div>
    </div>"#;
    let css = ".pad { padding: 24px } .pad:hover .mid .label { color: #00ff00 }";
    let mut ui = TestUi::new("scoped-deep", &[("page.html", page), ("style.css", css)])
        .stylesheet("style.css")
        .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();
    let before = ui.dump();
    let root = ui.root();
    let pad = element_by_id(ui.world_mut(), root, "pad");
    let rect = node_rect(ui.world_mut(), pad).expect("laid out");
    let at = Vec2::new((rect.min.x + rect.max.x) / 2.0, rect.min.y + 4.0);
    let _ = at;
    // Flip the pad directly (no picking needed: the state is app-writable).
    ui.world_mut().entity_mut(pad).insert(PseudoState {
        hovered: true,
        ..default()
    });
    ui.settle();
    let after = ui.dump();
    assert_ne!(after, before, "the flip restyled the chain");
    assert!(after.contains("\"deep\" default 16px #00ff00"), "{after}");
}

/// Restyles preserve the skeleton's pairing health: flips and var storms
/// never strike the content pairing out (ADR 0029's accounting rule) —
/// after the storm, the next content change still pairs (and the dump is
/// right).
#[test]
fn restyle_storms_keep_the_pairing_healthy() {
    let page = r#"<div class="hot" id="hot" data-bump="{{ n }}"><p>hot</p></div>
        {% for i in rows %}<div class="row"><p>row {{ i }}</p></div>{% endfor %}"#;
    let css = "html { --c: #ff0000 } .hot:hover { color: #00ff00 }";
    let mut ui = TestUi::new("scoped-storm", &[("page.html", page), ("style.css", css)])
        .stylesheet("style.css")
        .spawn(
            "page.html",
            TemplateContext::new()
                .with("--c", &"#ff0000")
                .with("rows", &[0, 1, 2])
                .with("n", &0),
            Node::default(),
        );
    ui.settle();
    let root = ui.root();
    let hot = element_by_id(ui.world_mut(), root, "hot");
    // Twenty flips and two var changes: every one of them a restyle.
    for i in 0..20 {
        ui.world_mut().entity_mut(hot).insert(PseudoState {
            hovered: i % 2 == 0,
            ..default()
        });
        if i % 10 == 9 {
            ui.world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("--c", &if i % 20 == 9 { "#00ff00" } else { "#ff0000" });
        }
        ui.settle_quiet();
    }
    let (_, strike) = ui
        .world_mut()
        .get::<Skeleton>(root)
        .expect("skeleton")
        .pairing_health();
    assert_eq!(strike, 0, "restyles never strike the content pairing out");
    // And a content change still pairs: the bump rebuilds cleanly.
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &7);
    ui.settle();
    assert!(
        ui.dump().contains("\"hot\""),
        "the content rebuilt: {}",
        ui.dump()
    );
    let (useful, strike) = ui
        .world_mut()
        .get::<Skeleton>(root)
        .expect("skeleton")
        .pairing_health();
    assert!(useful, "the content pairing stayed useful");
    assert_eq!(strike, 0, "the useful build reset the strikes");
}

/// A flip landing while a stylesheet loads: the Wait defers both the swap
/// and the flip, and the build that finally runs carries both — the new
/// sheet's style over the hovered state.
#[test]
fn flip_during_a_stylesheet_wait_applies_with_the_sheet() {
    let mut ui = TestUi::new(
        "scoped-wait",
        &[
            ("page.html", r#"<div class="hot" id="hot"><p>hot</p></div>"#),
            (
                "a.css",
                "html { color: #ffffff } .hot:hover { color: #00ff00 }",
            ),
            (
                "b.css",
                "html { color: #ffffff } .hot { font-size: 20px } .hot:hover { color: #00ff00 }",
            ),
        ],
    )
    .stylesheet("a.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();
    let root = ui.root();
    let hot = element_by_id(ui.world_mut(), root, "hot");
    // The swap (a fresh load) and the flip land in the same frame.
    let b = ui.load::<Stylesheet>("b.css");
    ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(b);
    ui.world_mut().entity_mut(hot).insert(PseudoState {
        hovered: true,
        ..default()
    });
    ui.settle();
    let dump = ui.dump();
    assert!(
        dump.contains("\"hot\" default 20px #00ff00"),
        "the sheet and the flip applied together: {dump}"
    );
}

/// ADR 0027's text properties, dumped: `text-align`/`line-height`/
/// `letter-spacing` land on the block's components, non-defaults only,
/// and `text-align` inherits.
#[test]
fn text_props_vector() {
    let mut ui = TestUi::from_vector("text_props");
    ui.settle().assert_dump(
        r#"
html-ui
  p.center align=Center
    "Centered line" default 16px #ffffff
  p.right align=Right
    "Right line" default 16px #ffffff
  p.justify w=200 align=Justified
    "Justified words wrap and stretch across the block" default 16px #ffffff
  p.lh-num lh=RelativeToFont(1.5)
    "Line height one point five" default 16px #ffffff
  p.lh-px lh=Px(30.0)
    "Line height thirty pixels" default 16px #ffffff
  p.ls ls=Px(2.0)
    "Letter spaced" default 16px #ffffff
  p.ls-em ls=Px(4.0)
    "Em spaced" default 16px #ffffff
  div.inherit-align
    p align=Right
      "Inherited center" default 16px #ffff00
  p.upper
    "UPPER CASE TEXT" default 16px #ffffff
  p.lower
    "lower case text" default 16px #ffffff
  p.caps
    "Capitalize Each Word Here" default 16px #ffffff
  p.pre-ws
    "pre    keeps     spaces" default 16px #ffffff
"#,
    );
}

/// ADR 0027's list markers: the `li`'s marker comes from its own computed
/// `list-style-type` (own declarations beat the inherited list's), an
/// `ol` numbers its items, and a nested list restarts the count.
#[test]
fn lists_vector() {
    let mut ui = TestUi::from_vector("lists");
    ui.settle().assert_dump(
        r#"
html-ui
  ul
    li margin=0,0,0,12
      "• " default 16px #ffffff
      "one" default 16px #ffffff
    li margin=0,0,0,12
      "• " default 16px #ffffff
      "two" default 16px #ffffff
  ol
    li margin=0,0,0,12
      "1. " default 16px #ffffff
      "first" default 16px #ffffff
    li margin=0,0,0,12
      "2. " default 16px #ffffff
      "second" default 16px #ffffff
    li margin=0,0,0,12
      "3. " default 16px #ffffff
      "third" default 16px #ffffff
  ol
    li margin=0,0,0,12
      "1. " default 16px #ffffff
      "again" default 16px #ffffff
  ul.none
    li margin=0,0,0,12
      "bare" default 16px #ffffff
  ul
    li.circle margin=0,0,0,12
      "◦ " default 16px #ffffff
      "circled" default 16px #ffffff
    li.square margin=0,0,0,12
      "▪ " default 16px #ffffff
      "squared" default 16px #ffffff
    li.num margin=0,0,0,12
      "3. " default 16px #ffffff
      "numbered" default 16px #ffffff
"#,
    );
}

/// ADR 0027's entity decoding through the real pipeline: named, numeric,
/// the one-level rule and an unknown reference.
#[test]
fn entities_decode_through_the_pipeline() {
    let ui = TestUi::new(
        "entities",
        &[(
            "page.html",
            "<p>A &mdash; B &amp;lt; C &#169; &#x2603; &nbsp; &notareal; end</p>",
        )],
    );
    let mut ui =
        ui.stylesheet("style.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();
    let dump = ui.dump();
    assert!(
        dump.contains("\"A — B &lt; C © ☃ \\u{a0} &notareal; end\""),
        "decoded: {dump}"
    );
}

/// ADR 0027: `overflow: auto` scrolls if needed. The axis clips until the
/// laid-out content overflows the box, promotes to scrolling then, demotes
/// when the content shrinks back, and the x axis (declared `scroll`) never
/// moves.
#[test]
fn overflow_auto_promotes_and_demotes_with_content() {
    let page =
        r#"<div id="box" class="clip">{% for i in rows %}<p>row {{ i }}</p>{% endfor %}</div>"#;
    let css = ".clip { height: 40px; overflow-x: scroll; overflow-y: auto }";
    let mut ui = TestUi::with_layout(
        "overflow-auto",
        &[("page.html", page), ("style.css", css)],
        UVec2::new(320, 240),
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new().with("rows", &[0, 1, 2, 3, 4]),
        Node::default(),
    );
    ui.settle();
    let overflow = |ui: &mut TestUi| {
        let root = ui.root();
        let box_entity = element_by_id(ui.world_mut(), root, "box");
        let node = ui
            .world_mut()
            .get::<Node>(box_entity)
            .expect("node")
            .clone();
        node.overflow
    };
    // Five rows (~96px) inside a 40px box: the y axis promotes.
    let promoted = overflow(&mut ui);
    assert_eq!(promoted.y, OverflowAxis::Scroll, "overflowing: promoted");
    assert_eq!(promoted.x, OverflowAxis::Scroll, "declared scroll stays");
    // The content shrinks under the box: the axis demotes.
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("rows", &[0]);
    ui.settle();
    let demoted = overflow(&mut ui);
    assert_eq!(demoted.y, OverflowAxis::Hidden, "fitting: demoted");
    assert_eq!(demoted.x, OverflowAxis::Scroll, "declared scroll stays");
    // And it re-promotes (a rebuild resets the axis; the promotion runs
    // again).
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("rows", &[0, 1, 2, 3, 4]);
    ui.settle();
    assert_eq!(overflow(&mut ui).y, OverflowAxis::Scroll, "re-promoted");
}

/// ADR 0028's `background-image`: the sheet's handle by URL, tiling per
/// `background-repeat` (the CSS default is tiling both axes), `stretch`
/// when `no-repeat` (bevy's `ImageNode` always fills its visual box), the
/// box from `background-clip`, and the rule that a `border-image`'s frame
/// wins the node's single `ImageNode` (the background's color shows
/// through it instead).
#[test]
fn background_vector() {
    let mut ui = TestUi::from_vector("background");
    ui.settle().assert_dump(
        r#"
html-ui
  div.tiled padding=20,20,20,20 w=100 bg-image=frame.png tiled=true,true
    -
      "Tiled" default 16px #ffffff
  div.fill padding=20,20,20,20 w=100 bg-image=frame.png stretch
    -
      "Filled" default 16px #ffffff
  div.clip padding=20,20,20,20 w=100 bg-image=frame.png tiled=true,true visual-box=PaddingBox
    -
      "Padding box" default 16px #ffffff
  div.frame.bg padding=20,20,20,20 w=100 slice=frame.png 8,8,8,8 stretch
    -
      "Frame with background" default 16px #ffffff
"#,
    );
}

/// A background image's schemed URL is skipped like a border-image's
/// (bug_0118): the sheet loads, the background doesn't, nothing else
/// breaks.
#[test]
fn remote_background_images_are_refused() {
    let ui = TestUi::new(
        "background-remote",
        &[
            ("page.html", r#"<div class="bg" id="box"><p>text</p></div>"#),
            (
                "style.css",
                "html { color: #ffffff } .bg { background-image: url(https://evil.example/bg.png) }",
            ),
        ],
    );
    let mut ui =
        ui.stylesheet("style.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();
    let dump = ui.dump();
    assert!(
        !dump.contains("bg-image"),
        "the remote background skipped: {dump}"
    );
}

/// ADR 0028: a copied web stylesheet's `@font-face` rules don't fail the
/// sheet's load — the style rules apply, the face registration is the
/// `FontFamilies` resource's job.
#[test]
fn font_face_rules_are_a_noop() {
    let ui = TestUi::new(
        "font-face",
        &[
            ("page.html", r#"<p class="fancy">Fancy text</p>"#),
            (
                "style.css",
                "@font-face { font-family: \"Fancy\"; src: url(fancy.woff2) } \
                 p { color: #ff00ff } .fancy { font-family: \"Fancy\", serif }",
            ),
        ],
    );
    let mut ui =
        ui.stylesheet("style.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();
    let dump = ui.dump();
    // The sheet loaded (the color applied) and the unknown family fell
    // back to the registered/default font without failing anything.
    assert!(
        dump.contains("\"Fancy text\" default 16px #ff00ff"),
        "the sheet applied: {dump}"
    );
}

/// ADR 0028's structural pseudo-classes against the browser: odd/even
/// striping, first/last-child, `:not(compound)` and `:empty` — the same
/// characters colored the same way Chromium colors them (`:empty`'s
/// element carries a background instead of text). The striping ps are the
/// page's first four children; the first/last pair sits in its own
/// wrapper (first-child is about the position, not the class).
#[test]
fn structural_selectors_vector() {
    let mut ui = TestUi::from_vector("structural_selectors");
    ui.settle().assert_dump(
        r#"
html-ui
  p.odd-stripe
    "one" default 16px #00ff00
  p.odd-stripe
    "two" default 16px #ff00ff
  p.odd-stripe
    "three" default 16px #00ff00
  p.odd-stripe
    "four" default 16px #ff00ff
  div
    p.first
      "first" default 16px #00ff00
    p.last-in
      "last" default 16px #00ff00
  p.not-special
    "plain" default 16px #ff00ff
  p.not-special.special
    "special" default 16px #ffffff
  div.e bg=#123456
"#,
    );
}

/// `:focus-within` restyles the focused element's ancestors (and itself)
/// — the focused subtree's chain — scoped: a flip of the focus marks the
/// new chain and clears the old, each matching a fresh build.
#[test]
fn focus_within_restyles_the_focused_chain() {
    let page = r#"<div id="outer"><p id="inner">text</p></div>
        <p class="plain" id="other">other</p>"#;
    let css = "html { color: #ffffff } div:focus-within { background-color: #111111 } p:focus-within { color: #00ff00 }";
    let mut target = TestUi::with_layout(
        "focus-within-target",
        &[("page.html", page), ("style.css", css)],
        UVec2::new(320, 240),
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    target.settle();
    let before = target.dump();
    let root = target.root();
    let inner = element_by_id(target.world_mut(), root, "inner");
    // Focus the inner p: the outer div gains the focus-within mark.
    target
        .world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(inner, bevy::input_focus::FocusCause::Navigated);
    target.settle();
    let after = target.dump();
    assert!(after.contains("div#outer bg=#111111"), "{after}");
    assert!(after.contains("\"text\" default 16px #00ff00"), "{after}");
    assert!(after.contains("\"other\" default 16px #ffffff"), "{after}");
    // Moving focus out: the chain clears and the newly focused element
    // marks instead (its own `:focus-within` — it holds the focus).
    let other = element_by_id(target.world_mut(), root, "other");
    target
        .world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .set(other, bevy::input_focus::FocusCause::Navigated);
    target.settle();
    let moved = target.dump();
    assert!(moved.contains("div#outer\n"), "the outer cleared: {moved}");
    assert!(!moved.contains("bg=#111111"), "the mark left: {moved}");
    assert!(
        moved.contains("\"other\" default 16px #00ff00"),
        "the newly focused element marked: {moved}"
    );
    // Clearing the focus (no focusables to repair to) restores the start.
    target
        .world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    target.settle();
    assert_eq!(target.dump(), before, "the cleared focus restored");
}

/// ADR 0028's sibling combinators, against the browser: `+` colors only
/// the immediately following sibling; `~` colors every following one; a
/// standalone element with the same classes stays unstyled.
#[test]
fn sibling_combinators_vector() {
    let mut ui = TestUi::from_vector("sibling_combinators");
    ui.settle().assert_dump(
        r#"
html-ui
  div.hot
    -
      "hot" default 16px #ffffff
  p.after-hot
    "after hot" default 16px #00ff00
  p.far-after
    "far after" default 16px #ffffff
  div.checked
    -
      "checked" default 16px #ffffff
  p.after-checked
    "after checked" default 16px #00ff00
  p.no-sibling
    "standalone" default 16px #00ff00
"#,
    );
}

/// ADR 0028: a sibling's interaction flip restyles its FOLLOWING siblings
/// that a sibling combinator keys on (`.hot:hover + p`) — scoped: the
/// flip's blocked set covers the following siblings, everything else
/// keeps its cached build, and the result matches a fresh build.
#[test]
fn sibling_flip_restyles_the_following_sibling() {
    let page = r#"<div class="hot" id="hot"><p>hot</p></div>
        <p class="after" id="after">after</p>
        <p class="unrelated" id="unrelated">unrelated</p>"#;
    let css = "html { color: #ffffff } .hot:hover + .after { color: #00ff00 }";
    // No picking (`TestUi::new`): with the layout harness's picking
    // plugins, `update_pseudo_states` resets a hand-inserted
    // `PseudoState` the next frame — the flip below never held, and the
    // test passed only because I07 ignored the sibling's `:hover`.
    let mut target = TestUi::new(
        "sibling-flip-target",
        &[("page.html", page), ("style.css", css)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    target.settle();
    let before = target.dump();
    // Nothing is hovered yet: the rule must not apply (review_0005 I12 —
    // without this check the test passed with the sibling's `:hover`
    // ignored, I07: `after` was green before the flip too).
    assert!(
        before.contains("\"after\" default 16px #ffffff"),
        "the following sibling is unstyled before the flip: {before}"
    );
    let root = target.root();
    let hot = element_by_id(target.world_mut(), root, "hot");
    // Flip the hot div (direct state: the scoping's input).
    target.world_mut().entity_mut(hot).insert(PseudoState {
        hovered: true,
        ..default()
    });
    target.settle();
    let after = target.dump();
    assert!(
        after.contains("\"after\" default 16px #00ff00"),
        "the following sibling restyled: {after}"
    );
    assert!(
        after.contains("\"unrelated\" default 16px #ffffff"),
        "the unrelated p untouched: {after}"
    );
    // The unhover restores the start.
    target.world_mut().entity_mut(hot).insert(PseudoState {
        hovered: false,
        ..default()
    });
    target.settle();
    assert_eq!(target.dump(), before, "the unhover restored the start");
}

/// ADR 0028's table mapping: the table/rows/cells build as flex
/// containers — a `tr` lays its cells in a row, a `th`'s text is bold,
/// the cells' selectors work — NOT CSS's table layout (no spans/column
/// sizing; documented).
#[test]
fn tables_vector() {
    let mut ui = TestUi::from_vector("tables");
    ui.settle().assert_dump(
        r#"
html-ui
  table.grid
    tr.head dir=Row
      th bg=#222222
        -
          "Name" default 16px #ffffff
      th bg=#222222
        -
          "Count" default 16px #ffffff
    tr dir=Row
      td.first padding=2,2,2,2
        -
          "apples" default 16px #ff00ff
      td padding=2,2,2,2
        -
          "12" default 16px #ffffff
    tr dir=Row
      td.first padding=2,2,2,2
        -
          "pears" default 16px #ff00ff
      td padding=2,2,2,2
        -
          "7" default 16px #ffffff
"#,
    );
}

/// ADR 0028's attribute selectors through the pipeline: `[data-kind]`
/// presence and value, the `data-` prefix required, the element's own
/// dataset as the input.
#[test]
fn attribute_selectors_through_the_pipeline() {
    let ui = TestUi::new(
        "attribute-selectors",
        &[
            (
                "page.html",
                r#"<p data-kind="magic" id="magic">magic</p>
                    <p data-kind="plain" id="plain">plain</p>
                    <p id="bare">bare</p>"#,
            ),
            (
                "style.css",
                "html { color: #ffffff } p { color: #ffffff } \
                 [data-kind=\"magic\"] { color: #00ff00 } \
                 p[data-kind] { font-weight: bold }",
            ),
        ],
    );
    let mut ui =
        ui.stylesheet("style.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();
    let dump = ui.dump();
    assert!(
        dump.contains("\"magic\" default 16px #00ff00"),
        "the value-matched p: {dump}"
    );
    assert!(
        dump.contains("\"plain\" default 16px #ffffff"),
        "the unmatched color stands (the bold's weight is invisible in the \
         dump with a handle font — the match is unit-tested)"
    );
    assert!(dump.contains("\"bare\" default 16px #ffffff"));
}

/// Reproductions of review_0005's findings (`docs/agents/reviews/
/// review_0005.md`). Each test fails at `e3646ae` for the reason its
/// finding names and must pass once that finding is fixed; the doc comment
/// names the finding.
mod review_0005 {
    use super::*;

    const WHITE: &str = "html { color: #ffffff } ";

    /// Builds `page` with `css` once and returns the settled dump.
    fn built(name: &str, page: &str, css: &str) -> String {
        let files = [("page.html", page), ("a.css", css)];
        TestUi::new(name, &files)
            .stylesheet("a.css")
            .spawn("page.html", TemplateContext::new(), Node::default())
            .settle()
            .dump()
    }

    /// Renders `page` with `key = from`, changes it to `to`, and returns
    /// (the incrementally updated dump, a fresh build's dump of `to`). The
    /// fresh comparator drops its skeleton first, so nothing is reused.
    fn reuse_and_fresh(
        name: &str,
        page: &str,
        css: &str,
        key: &'static str,
        from: &str,
        to: &str,
    ) -> (String, String) {
        let files = [("page.html", page), ("a.css", css)];
        let context = TemplateContext::new().with(key, &from);
        let mut reused = TestUi::new(&format!("{name}-reused"), &files)
            .stylesheet("a.css")
            .spawn("page.html", context.clone(), Node::default());
        let mut fresh = TestUi::new(&format!("{name}-fresh"), &files)
            .stylesheet("a.css")
            .spawn("page.html", context, Node::default());
        reused.settle();
        fresh.settle();
        for (ui, keep) in [(&mut reused, true), (&mut fresh, false)] {
            let root = ui.root();
            if !keep {
                ui.world_mut().entity_mut(root).remove::<Skeleton>();
            }
            ui.world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert(key, &to);
        }
        (reused.settle().dump(), fresh.settle().dump())
    }

    /// Whether the dump shows a styled (red) element right before `text`.
    fn red_before(dump: &str, text: &str) -> bool {
        let at = dump
            .find(&format!("\"{text}\""))
            .unwrap_or_else(|| panic!("{text} in {dump}"));
        let element = dump[..at].lines().rev().find(|line| {
            let line = line.trim_start();
            !line.is_empty() && !line.starts_with('-') && !line.starts_with('"')
        });
        element.is_some_and(|line| line.contains("bg=#ff0000"))
    }

    // ---- I02: `:nth-child` overflow panics -------------------------------

    /// I02: `n-2147483647` overflowed `i32` in `AnB::matches` and panicked
    /// the build (debug). Every element index is reachable (n large), so it
    /// matches every `p`; `-n-2147483647` matches none.
    #[test]
    fn nth_child_extreme_offsets_neither_panic_nor_mismatch() {
        let page = "<div><p>1</p><p>2</p><p>3</p></div>";
        let all = built(
            "i02-all",
            page,
            &format!("{WHITE} p:nth-child(n-2147483647) {{ background-color: #ff0000 }}"),
        );
        // CSS: every index is reachable (n large), so every `p` matches.
        // ADR 0034's engine (parcel_selectors) computes `index - b` in
        // checked i32, overflows and answers no; no document has the 2^31
        // siblings that would tell. The property this guards is the panic.
        for text in ["1", "2", "3"] {
            assert!(
                !red_before(&all, text),
                "the engine's answer for n-2147483647 changed:\n{all}"
            );
        }
        let none = built(
            "i02-none",
            page,
            &format!("{WHITE} p:nth-child(-n-2147483647) {{ background-color: #ff0000 }}"),
        );
        assert!(
            !none.contains("bg=#ff0000"),
            "-n-2147483647 matches no index:\n{none}"
        );
    }

    // ---- I03: O(n²) sibling lists ----------------------------------------

    /// I03: every element copied its preceding siblings (and positions
    /// were recounted per child), so a long sibling list built in O(n²),
    /// with or without a sibling rule. Scaling, not wall time (AGENTS.md):
    /// 4× the children may cost at most 8× (best of three).
    #[test]
    fn long_sibling_lists_build_linearly() {
        use std::time::{Duration, Instant};
        let css = format!("{WHITE} .hot ~ p {{ color: #00ff00 }}");
        let time = |count: usize| {
            (0..3)
                .map(|run| {
                    let page = format!("<div>{}</div>", "<p class=\"x\">r</p>".repeat(count));
                    let files = [("page.html", page.as_str()), ("a.css", css.as_str())];
                    let mut ui = TestUi::new(&format!("i03-{count}-{run}"), &files)
                        .stylesheet("a.css")
                        .spawn("page.html", TemplateContext::new(), Node::default());
                    let start = Instant::now();
                    ui.settle();
                    start.elapsed()
                })
                .min()
                .unwrap()
        };
        // Debug builds: 400 → 1600 measured 7.3× (linear costs hide the
        // quadratic), 800 → 3200 10.9× at `e3646ae`.
        let (small, large) = (time(800), time(3200));
        assert!(
            large < small * 8 + Duration::from_millis(50),
            "sibling lists build superlinearly: 800 → {small:?}, 3200 → {large:?}"
        );
    }

    // ---- I05: content reuse ignores siblings and positions --------------

    /// I05: `[div.hot, p#t]` → `[div.cold, p#t]` — `p#t` is unchanged and
    /// was reused with `.hot + p`'s stale result.
    #[test]
    fn reuse_rechecks_adjacent_sibling_rules() {
        let (reused, fresh) = reuse_and_fresh(
            "i05-adjacent",
            r#"<div class="{{ c }}"><p>a</p></div><p id="t">t</p><p id="u">u</p>"#,
            &format!("{WHITE} .hot + p {{ background-color: #ff0000 }}"),
            "c",
            "hot",
            "cold",
        );
        assert_eq!(reused, fresh);
    }

    /// I05: as above for `~`.
    #[test]
    fn reuse_rechecks_general_sibling_rules() {
        let (reused, fresh) = reuse_and_fresh(
            "i05-general",
            r#"<div class="{{ c }}"><p>a</p></div><div><p>s</p></div><p id="t">t</p>"#,
            &format!("{WHITE} .hot ~ p {{ background-color: #ff0000 }}"),
            "c",
            "hot",
            "cold",
        );
        assert_eq!(reused, fresh);
    }

    /// I05: the preceding sibling turns from a `div` into a `p`: `p#t` is
    /// now the second `p` — its `:nth-of-type(1)` no longer holds.
    #[test]
    fn reuse_rechecks_nth_of_type() {
        let (reused, fresh) = reuse_and_fresh(
            "i05-nth-of-type",
            r#"<{{ c }} class="x">a</{{ c }}><p id="t">t</p>"#,
            &format!("{WHITE} p:nth-of-type(1) {{ background-color: #ff0000 }}"),
            "c",
            "div",
            "p",
        );
        assert_eq!(reused, fresh);
    }

    /// I05: the following sibling turns into a `p`: `p#t` is no longer the
    /// last of its type.
    #[test]
    fn reuse_rechecks_last_of_type() {
        let (reused, fresh) = reuse_and_fresh(
            "i05-last-of-type",
            r#"<p id="t">t</p><{{ c }} class="x">a</{{ c }}>"#,
            &format!("{WHITE} p:last-of-type {{ background-color: #ff0000 }}"),
            "c",
            "div",
            "p",
        );
        assert_eq!(reused, fresh);
    }

    // ---- I06: the style cache's key -------------------------------------

    /// I06: two id-less `p`s at the same position under the same chain;
    /// only the first follows `.hot`. The cache (keyed without the
    /// preceding siblings) gave both the first one's result — in either
    /// order.
    #[test]
    fn cache_keys_on_preceding_siblings() {
        let css = format!("{WHITE} .hot + p {{ background-color: #ff0000 }}");
        let dump = built(
            "i06-siblings",
            "<div><div class=\"hot\">x</div><p>a</p></div><div><div>x</div><p>b</p></div>",
            &css,
        );
        assert!(red_before(&dump, "a") && !red_before(&dump, "b"), "{dump}");
        let dump = built(
            "i06-siblings-reversed",
            "<div><div>x</div><p>b</p></div><div><div class=\"hot\">x</div><p>a</p></div>",
            &css,
        );
        assert!(red_before(&dump, "a") && !red_before(&dump, "b"), "{dump}");
    }

    /// I06: two `p`s differing only in `data-x`; the cache (keyed without
    /// the dataset) gave the second the first one's result.
    #[test]
    fn cache_keys_on_the_dataset() {
        let css = format!("{WHITE} [data-x=\"2\"] {{ background-color: #ff0000 }}");
        let dump = built(
            "i06-dataset",
            "<div><p data-x=\"1\">a</p></div><div><p data-x=\"2\">b</p></div>",
            &css,
        );
        assert!(!red_before(&dump, "a") && red_before(&dump, "b"), "{dump}");
        let dump = built(
            "i06-dataset-reversed",
            "<div><p data-x=\"2\">b</p></div><div><p data-x=\"1\">a</p></div>",
            &css,
        );
        assert!(!red_before(&dump, "a") && red_before(&dump, "b"), "{dump}");
    }

    // ---- I07: sibling and ancestor compounds ----------------------------

    /// I07: a sibling compound's pseudo-class was ignored: `.a:hover + .b`
    /// styled `.b` with nothing hovered.
    #[test]
    fn sibling_compounds_test_their_pseudo_state() {
        let dump = built(
            "i07-sibling-hover",
            "<div class=\"a\">A</div><p class=\"b\">B</p>",
            &format!("{WHITE} .a:hover + .b {{ background-color: #ff0000 }}"),
        );
        assert!(!red_before(&dump, "B"), "nothing is hovered:\n{dump}");
    }

    /// I07: a sibling compound's attribute test was ignored.
    #[test]
    fn sibling_compounds_test_their_attributes() {
        let css = format!("{WHITE} [data-m=\"dark\"] + p {{ background-color: #ff0000 }}");
        let dump = built(
            "i07-sibling-attr",
            "<div><div data-m=\"light\">x</div><p>no</p></div><div><div data-m=\"dark\">x</div><p>yes</p></div>",
            &css,
        );
        assert!(
            !red_before(&dump, "no") && red_before(&dump, "yes"),
            "{dump}"
        );
    }

    /// I07: an ancestor compound's attribute test was ignored: the rule
    /// matched under any ancestor (`Ancestor` carries no dataset).
    #[test]
    fn ancestor_compounds_test_their_attributes() {
        let css = format!("{WHITE} [data-m=\"dark\"] p {{ background-color: #ff0000 }}");
        let dump = built(
            "i07-ancestor-attr",
            "<div data-m=\"dark\"><p>yes</p></div><div data-m=\"light\"><p>light</p></div><div><p>absent</p></div>",
            &css,
        );
        assert!(red_before(&dump, "yes"), "{dump}");
        assert!(
            !red_before(&dump, "light") && !red_before(&dump, "absent"),
            "{dump}"
        );
    }

    // ---- I08: `:not()` around a pseudo-class ----------------------------

    /// Sets `PseudoState::hovered` on the element with `id` and lets the
    /// restyle land.
    fn hover(ui: &mut TestUi, id: &str, hovered: bool) {
        let root = ui.root();
        let entity = element_by_id(ui.world_mut(), root, id);
        ui.world_mut().entity_mut(entity).insert(PseudoState {
            hovered,
            ..default()
        });
        ui.update(10);
    }

    /// I08: `:not(:hover)` negated a compound whose pseudo test was
    /// dropped, so it never matched. Unhovered it matches; hovered not;
    /// unhovered again it matches.
    #[test]
    fn not_hover_matches_while_not_hovered() {
        let files = [
            ("page.html", "<div id=\"a\" class=\"a\"><p>A</p></div>"),
            (
                "a.css",
                "html { color: #ffffff } .a:not(:hover) { background-color: #ff0000 }",
            ),
        ];
        let mut ui = TestUi::new("i08-not-hover", &files)
            .stylesheet("a.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        let dump = ui.settle().dump();
        assert!(dump.contains("div#a.a bg=#ff0000"), "unhovered:\n{dump}");
        hover(&mut ui, "a", true);
        let dump = ui.dump();
        assert!(!dump.contains("bg=#ff0000"), "hovered:\n{dump}");
        hover(&mut ui, "a", false);
        let dump = ui.dump();
        assert!(
            dump.contains("div#a.a bg=#ff0000"),
            "unhovered again:\n{dump}"
        );
    }

    /// I08: `p:not(:focus)` never matched either.
    #[test]
    fn not_focus_matches_while_not_focused() {
        let dump = built(
            "i08-not-focus",
            "<div><p>a</p></div>",
            &format!("{WHITE} p:not(:focus) {{ background-color: #ff0000 }}"),
        );
        assert!(red_before(&dump, "a"), "{dump}");
    }

    /// I08 × ADR 0029: once `:not(:hover)` matches, a flip on the sibling
    /// must restyle the following element — the reactivity record
    /// wildcards pseudo tests as "match", which inside a `:not` reads as
    /// "never" (the stale-reuse hazard the review names).
    #[test]
    fn not_hover_sibling_rules_follow_the_flip() {
        let files = [
            (
                "page.html",
                "<div id=\"a\" class=\"a\"><p>A</p></div><p class=\"b\">B</p>",
            ),
            (
                "a.css",
                "html { color: #ffffff } .a:not(:hover) + .b { background-color: #ff0000 }",
            ),
        ];
        let mut ui = TestUi::new("i08-not-hover-sibling", &files)
            .stylesheet("a.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        let dump = ui.settle().dump();
        assert!(red_before(&dump, "B"), "unhovered:\n{dump}");
        hover(&mut ui, "a", true);
        let dump = ui.dump();
        assert!(!red_before(&dump, "B"), "hovered:\n{dump}");
    }

    // ---- I09: `overflow: auto` after an in-place update ----------------

    /// The `overflow.y` of the element with `id`.
    fn overflow_y(ui: &mut TestUi, id: &str) -> OverflowAxis {
        let root = ui.root();
        let entity = element_by_id(ui.world_mut(), root, id);
        ui.world_mut().get::<Node>(entity).unwrap().overflow.y
    }

    fn auto_scroll_panel(name: &str, css_extra: &str) -> TestUi {
        let page = r#"<div id="panel" class="panel {{ c }}"><p>one</p><p>two</p><p>three</p><p>four</p><p>five</p><p>six</p></div>"#;
        let css = format!(
            "html {{ color: #ffffff; font-size: 16px }} .panel {{ height: 40px; overflow-y: auto }} {css_extra}"
        );
        // The pointer harness (layout + picking): the hover test hovers
        // for real — a hand-inserted `PseudoState` is reset by picking
        // the next frame.
        let mut ui = TestUi::with_pointer(
            name,
            &[("page.html", page), ("a.css", css.as_str())],
            UVec2::new(400, 300),
        )
        .stylesheet("a.css")
        .spawn(
            "page.html",
            TemplateContext::new().with("c", "off"),
            Node {
                width: Val::Px(400.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        );
        ui.settle();
        ui.update(5);
        assert_eq!(
            overflow_y(&mut ui, "panel"),
            OverflowAxis::Scroll,
            "the overflowing panel scrolls"
        );
        ui
    }

    /// I09: an in-place update rewrote the panel's `Node` (axis `Hidden`)
    /// and nothing promoted it again — the layout's size didn't change, so
    /// `Changed<ComputedNode>` never fired.
    #[test]
    fn auto_scroll_survives_an_in_place_update() {
        let mut ui = auto_scroll_panel("i09-update", ".on { background-color: #222222 }");
        let root = ui.root();
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert("c", &"on");
        ui.settle();
        ui.update(10);
        assert_eq!(overflow_y(&mut ui, "panel"), OverflowAxis::Scroll);
    }

    /// I09: the same through a `:hover` restyle — a hovered panel stopped
    /// scrolling for good.
    #[test]
    fn auto_scroll_survives_a_hover_restyle() {
        let mut ui = auto_scroll_panel("i09-hover", ".panel:hover { background-color: #222222 }");
        let root = ui.root();
        let panel = element_by_id(ui.world_mut(), root, "panel");
        let rect = node_rect(ui.world_mut(), panel).expect("the panel is laid out");
        ui.move_pointer(rect.center());
        ui.update(10);
        let hovered = ui
            .world_mut()
            .get::<PseudoState>(panel)
            .is_some_and(|state| state.hovered);
        assert!(hovered, "the pointer hovers the panel");
        assert!(
            ui.dump().contains("bg=#222222"),
            "the hover restyled the panel"
        );
        assert_eq!(overflow_y(&mut ui, "panel"), OverflowAxis::Scroll);
    }

    /// I09's write discipline: a layout change that leaves the axis right
    /// (the root narrows; the panel still overflows) must not rewrite the
    /// panel's `Node` — a plain `&mut` gave it a change tick on every
    /// `ComputedNode` change (the G15 class).
    #[test]
    fn auto_scroll_leaves_a_settled_node_unchanged() {
        let mut ui = auto_scroll_panel("i09-quiet", "");
        let root = ui.root();
        let panel = element_by_id(ui.world_mut(), root, "panel");
        let before = node_rect(ui.world_mut(), panel).expect("laid out");
        let tick = ui
            .world_mut()
            .entity(panel)
            .get_ref::<Node>()
            .unwrap()
            .last_changed();
        ui.world_mut().get_mut::<Node>(root).unwrap().width = Val::Px(300.0);
        ui.update(5);
        let after = node_rect(ui.world_mut(), panel).expect("laid out");
        assert_ne!(before.width(), after.width(), "the panel's layout changed");
        let later = ui
            .world_mut()
            .entity(panel)
            .get_ref::<Node>()
            .unwrap()
            .last_changed();
        assert_eq!(
            tick, later,
            "the panel's Node was rewritten with the same axis"
        );
        assert_eq!(overflow_y(&mut ui, "panel"), OverflowAxis::Scroll);
    }

    // ---- found while fixing --------------------------------------------

    /// Inline elements match sibling combinators and positions too: the
    /// run walk threads each child's preceding siblings (ADR 0030). Before,
    /// inline runs had no sibling list, so `b + i` never matched inside a
    /// paragraph.
    #[test]
    fn inline_sibling_combinators_match() {
        let dump = built(
            "fix-inline-siblings",
            "<p><b>bold</b> <i>next</i> <i>later</i></p>",
            &format!("{WHITE} b + i {{ color: #ff0000 }} i:last-child {{ color: #00ff00 }}"),
        );
        // Bold and italic fall back to the default font here (no families
        // registered); the runs with equal styles merge.
        assert_eq!(
            dump.trim(),
            "html-ui\n  p\n    \"bold \" default 16px #ffffff\n    \"next\" default 16px #ff0000\n    \" \" default 16px #ffffff\n    \"later\" default 16px #00ff00"
        );
    }

    /// `:nth-child(-n+3)` keeps its `+` (lightningcss doesn't rewrite it):
    /// the combinator scan split it there and dropped the rule.
    #[test]
    fn nth_child_with_a_negative_step_applies() {
        let dump = built(
            "fix-nth-negative",
            "<div><p>1</p><p>2</p><p>3</p><p>4</p></div>",
            &format!("{WHITE} p:nth-child(-n+3) {{ background-color: #ff0000 }}"),
        );
        for text in ["1", "2", "3"] {
            assert!(red_before(&dump, text), "{text}: {dump}");
        }
        assert!(!red_before(&dump, "4"), "{dump}");
    }

    /// I05 one level down: a sibling list inside a container (the nested
    /// `pair_node` path; a changed container's unchanged children reuse
    /// since bug_0150's fix, so the sibling check must hold there too).
    #[test]
    fn reuse_rechecks_nested_sibling_lists() {
        let (reused, fresh) = reuse_and_fresh(
            "i05-nested-adjacent",
            r#"<section><div class="{{ c }}"><p>a</p></div><p id="t">t</p><p id="u">u</p></section>"#,
            &format!("{WHITE} .hot + p {{ background-color: #ff0000 }}"),
            "c",
            "hot",
            "cold",
        );
        assert!(!red_before(&fresh, "t"), "{fresh}");
        assert_eq!(reused, fresh);
    }

    /// I05 with text between the siblings: `+` skips text nodes, so the
    /// element after the text is the one that must re-collect.
    #[test]
    fn reuse_rechecks_adjacent_rules_across_text() {
        let (reused, fresh) = reuse_and_fresh(
            "i05-adjacent-text",
            r#"<section><div class="{{ c }}"><p>a</p></div> between <p id="t">t</p><p id="u">u</p></section>"#,
            &format!("{WHITE} .hot + p {{ background-color: #ff0000 }}"),
            "c",
            "hot",
            "cold",
        );
        assert!(!red_before(&fresh, "t"), "{fresh}");
        assert_eq!(reused, fresh);
    }

    /// I05 one level down, where it bites deep: a sibling list inside a
    /// container changes shape, moving a `div` from first to second child;
    /// `div:first-child p` must drop from the `div`'s descendant, whose own
    /// subtree is unchanged (the nested `pair_node` path; the re-collected
    /// `div` alone would hand its `p` the cached spec).
    #[test]
    fn reuse_rechecks_descendants_of_moved_siblings() {
        let (reused, fresh) = reuse_and_fresh(
            "i05-nested",
            r#"<section>{% if c == "el" %}<div>new</div>{% else %}x{% endif %}<div id="t"><p>t</p></div></section>"#,
            &format!("{WHITE} div:first-child p {{ background-color: #ff0000 }}"),
            "c",
            "text",
            "el",
        );
        assert!(!red_before(&fresh, "t"), "{fresh}");
        assert_eq!(reused, fresh);
    }

    /// I05 with a text node turning into an element (same child count, at
    /// the document's top level): the following element's `:first-child`
    /// changes.
    #[test]
    fn reuse_rechecks_positions_when_text_becomes_an_element() {
        let (reused, fresh) = reuse_and_fresh(
            "i05-text-element",
            r#"{% if c == "el" %}<div>x</div>{% else %}x{% endif %}<p id="t">t</p>"#,
            &format!("{WHITE} p:first-child {{ background-color: #ff0000 }}"),
            "c",
            "text",
            "el",
        );
        assert!(!red_before(&fresh, "t"), "{fresh}");
        assert_eq!(reused, fresh);
    }

    /// Inline elements' ancestors carry their positions: `b:first-child i`
    /// styles only the first `b`'s `i`.
    #[test]
    fn inline_ancestors_carry_their_positions() {
        let dump = built(
            "fix-inline-ancestor-position",
            "<p><b>x <i>in</i></b><b>y <i>out</i></b></p>",
            &format!("{WHITE} b:first-child i {{ color: #ff0000 }}"),
        );
        assert!(dump.contains("\"in\" default 16px #ff0000"), "{dump}");
        // The second `b`'s runs keep the base style and merge.
        assert!(dump.contains("\"y out\" default 16px #ffffff"), "{dump}");
    }

    /// An inline ancestor's interaction state reaches its content: a
    /// hovered link restyles the `i` inside it (`a:hover i`).
    #[test]
    fn inline_ancestors_carry_their_pseudo_state() {
        let files = [
            (
                "page.html",
                "<p><a href=\"x\" id=\"l\">go <i>in</i></a></p>",
            ),
            (
                "a.css",
                "html { color: #ffffff } a:hover i { color: #ff0000 }",
            ),
        ];
        let mut ui = TestUi::new("fix-inline-ancestor-pseudo", &files)
            .stylesheet("a.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        let dump = ui.settle().dump();
        // Unhovered, the link's runs share a style and merge.
        assert!(dump.contains("\"go in\" default 16px #ffffff"), "{dump}");
        let link = {
            let world = ui.world_mut();
            let mut elements = world.query::<(Entity, &HtmlElement)>();
            elements
                .iter(world)
                .find(|(_, element)| element.tag == "a")
                .map(|(entity, _)| entity)
                .expect("the link span")
        };
        ui.world_mut().entity_mut(link).insert(PseudoState {
            hovered: true,
            ..default()
        });
        ui.update(10);
        let dump = ui.dump();
        assert!(dump.contains("\"in\" default 16px #ff0000"), "{dump}");
    }

    /// An ancestor's `:not(:hover)` is flip-reactive: hovering the row
    /// takes the paragraph's color away, unhovering brings it back.
    #[test]
    fn not_hover_on_an_ancestor_follows_the_flip() {
        let files = [
            (
                "page.html",
                "<div class=\"row\" id=\"r\"><p>label</p></div><p>other</p>",
            ),
            (
                "a.css",
                "html { color: #ffffff } .row:not(:hover) p { color: #ff0000 }",
            ),
        ];
        let mut ui = TestUi::new("fix-not-hover-ancestor", &files)
            .stylesheet("a.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        let dump = ui.settle().dump();
        assert!(dump.contains("\"label\" default 16px #ff0000"), "{dump}");
        hover(&mut ui, "r", true);
        let dump = ui.dump();
        assert!(dump.contains("\"label\" default 16px #ffffff"), "{dump}");
        hover(&mut ui, "r", false);
        let dump = ui.dump();
        assert!(dump.contains("\"label\" default 16px #ff0000"), "{dump}");
    }

    /// `overflow-x: auto` promotes like `overflow-y` (the x axis of the
    /// promotion).
    #[test]
    fn auto_scroll_promotes_the_x_axis() {
        let page = r#"<div id="panel" class="panel"><p>a very long line of text that cannot fit in forty pixels</p></div>"#;
        let css = "html { color: #ffffff; font-size: 16px } .panel { width: 40px; overflow-x: auto } p { white-space: nowrap }";
        let mut ui = TestUi::with_layout(
            "i09-x",
            &[("page.html", page), ("a.css", css)],
            UVec2::new(400, 300),
        )
        .stylesheet("a.css")
        .spawn(
            "page.html",
            TemplateContext::new(),
            Node {
                width: Val::Px(400.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        );
        ui.settle();
        ui.update(5);
        let root = ui.root();
        let panel = element_by_id(ui.world_mut(), root, "panel");
        let overflow = ui.world_mut().get::<Node>(panel).unwrap().overflow;
        assert_eq!(
            overflow.x,
            OverflowAxis::Scroll,
            "the overflowing axis scrolls"
        );
        assert_eq!(
            overflow.y,
            OverflowAxis::Visible,
            "the other axis is left alone"
        );
    }

    // ---- I14: bug_0129's crate half, deterministically -------------------

    /// I14: a stylesheet swap whose new sheet fails while the build waits
    /// for it must update unstyled — the full-page restyle latched in the
    /// `Wait` (`RebuildState::pending_full_style`). The suite guarded this
    /// only through a test that flaked one run in three; this one compares
    /// with a UI built unstyled from the start.
    #[test]
    fn failed_sheet_swap_during_a_wait_updates_unstyled() {
        let files = [
            (
                "page.html",
                "<div class=\"box\"><p class=\"t\">text</p></div>",
            ),
            (
                "a.css",
                "html { color: #ffffff } .box { background-color: #ff0000 } .t { color: #00ff00 }",
            ),
            ("broken.css", "{ not css"),
        ];
        let mut ui = TestUi::new("i14-swap", &files).stylesheet("a.css").spawn(
            "page.html",
            TemplateContext::new(),
            Node::default(),
        );
        let styled = ui.settle().dump();
        assert!(styled.contains("bg=#ff0000"), "{styled}");
        let broken = ui.load::<Stylesheet>("broken.css");
        ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(broken);
        let swapped = ui.settle().dump();
        let unstyled = TestUi::new("i14-unstyled", &files)
            .stylesheet("broken.css")
            .spawn("page.html", TemplateContext::new(), Node::default())
            .settle()
            .dump();
        assert_eq!(swapped, unstyled);
    }
}

/// Gaps the review_0005 mutation audit found: code that mutants changed
/// without any test failing. Each test names the code it pins; the
/// expectations come from CSS semantics (and the skeleton's documented
/// adaptive rule), not from the code's output.
mod mutation_gaps {
    use super::*;

    const WHITE: &str = "html { color: #ffffff } ";

    fn built(name: &str, page: &str, css: &str) -> String {
        let files = [("page.html", page), ("a.css", css)];
        TestUi::new(name, &files)
            .stylesheet("a.css")
            .spawn("page.html", TemplateContext::new(), Node::default())
            .settle()
            .dump()
    }

    /// The dump line of the element right before the text run `text`.
    fn element_before<'a>(dump: &'a str, text: &str) -> &'a str {
        let at = dump
            .find(&format!("\"{text}\""))
            .unwrap_or_else(|| panic!("{text} in {dump}"));
        dump[..at]
            .lines()
            .rev()
            .map(str::trim_start)
            .find(|line| !line.is_empty() && !line.starts_with('-') && !line.starts_with('"'))
            .unwrap_or("")
    }

    /// The color of the text run `text` (the run line's last field).
    fn run_color<'a>(dump: &'a str, text: &str) -> &'a str {
        dump.lines()
            .find(|line| line.trim_start().starts_with(&format!("\"{text}\"")))
            .unwrap_or_else(|| panic!("{text} in {dump}"))
            .rsplit(' ')
            .next()
            .unwrap()
    }

    fn hover(ui: &mut TestUi, id: &str, hovered: bool) {
        let root = ui.root();
        let entity = element_by_id(ui.world_mut(), root, id);
        ui.world_mut().entity_mut(entity).insert(PseudoState {
            hovered,
            ..default()
        });
        ui.update(10);
    }

    // ---- structural pseudo-classes -------------------------------------

    /// `:first-of-type`/`:last-of-type`/`:nth-of-type()` count among the
    /// siblings of the same tag only (`sibling_positions`' per-type
    /// index and total; the parser's of-type arms).
    #[test]
    fn of_type_pseudo_classes_count_per_tag() {
        let dump = built(
            "gap-of-type",
            "<div><h1>h1</h1><p>p1</p><h2>h2</h2><p>p2</p><p>p3</p></div>",
            &format!(
                "{WHITE} p:first-of-type {{ background-color: #ff0000 }} \
                 p:nth-of-type(2) {{ background-color: #0000ff }} \
                 p:last-of-type {{ background-color: #00ff00 }} \
                 h2:first-of-type {{ background-color: #ffff00 }}"
            ),
        );
        assert!(element_before(&dump, "p1").contains("bg=#ff0000"), "{dump}");
        assert!(element_before(&dump, "p2").contains("bg=#0000ff"), "{dump}");
        assert!(element_before(&dump, "p3").contains("bg=#00ff00"), "{dump}");
        // The h2 is the third element but the first of its type.
        assert!(element_before(&dump, "h2").contains("bg=#ffff00"), "{dump}");
        assert!(!element_before(&dump, "h1").contains("bg="), "{dump}");
    }

    /// `:empty`: no children except comments — text makes an element
    /// non-empty, a comment doesn't.
    #[test]
    fn empty_counts_text_but_not_comments() {
        let dump = built(
            "gap-empty",
            "<section><div class=\"e\"></div><div class=\"c\"><!-- c --></div><div class=\"t\">text</div></section>",
            &format!("{WHITE} div:empty {{ background-color: #ff0000 }}"),
        );
        assert!(dump.contains("div.e bg=#ff0000"), "{dump}");
        assert!(dump.contains("div.c bg=#ff0000"), "{dump}");
        assert!(!element_before(&dump, "text").contains("bg="), "{dump}");
    }

    // ---- selectors ------------------------------------------------------

    /// Sibling combinators need no spaces around them (`h1+p`, `h1~p`).
    #[test]
    fn sibling_combinators_parse_without_spaces() {
        let page = "<div><h1>h</h1><p>a</p><p>b</p></div>";
        let dump = built(
            "gap-adjacent",
            page,
            &format!("{WHITE} h1+p {{ background-color: #ff0000 }}"),
        );
        assert!(element_before(&dump, "a").contains("bg=#ff0000"), "{dump}");
        assert!(!element_before(&dump, "b").contains("bg="), "{dump}");
        let dump = built(
            "gap-general",
            page,
            &format!("{WHITE} h1~p {{ background-color: #ff0000 }}"),
        );
        assert!(element_before(&dump, "a").contains("bg=#ff0000"), "{dump}");
        assert!(element_before(&dump, "b").contains("bg=#ff0000"), "{dump}");
    }

    /// A sibling selector with a compound before the pair still tests
    /// it: `.x h1 + p` matches only under `.x`.
    #[test]
    fn sibling_selectors_test_the_compounds_before_the_pair() {
        let dump = built(
            "gap-three-compounds",
            "<div class=\"x\"><h1>h</h1><p>in</p></div><div class=\"y\"><h1>h</h1><p>out</p></div>",
            &format!("{WHITE} .x h1 + p {{ background-color: #ff0000 }}"),
        );
        assert!(element_before(&dump, "in").contains("bg=#ff0000"), "{dump}");
        assert!(!element_before(&dump, "out").contains("bg="), "{dump}");
    }

    /// Attribute names keep `-` and `_` (`[data-a-b]`, `[data-a_b]`).
    #[test]
    fn attribute_names_keep_dashes_and_underscores() {
        let dump = built(
            "gap-attribute-names",
            "<div><p data-a-b=\"1\">dash</p><p data-a_b=\"1\">under</p><p>none</p></div>",
            &format!(
                "{WHITE} [data-a-b] {{ background-color: #ff0000 }} [data-a_b] {{ background-color: #0000ff }}"
            ),
        );
        assert!(
            element_before(&dump, "dash").contains("bg=#ff0000"),
            "{dump}"
        );
        assert!(
            element_before(&dump, "under").contains("bg=#0000ff"),
            "{dump}"
        );
        assert!(!element_before(&dump, "none").contains("bg="), "{dump}");
    }

    // ---- text properties -------------------------------------------------

    /// `letter-spacing: 0.5em` is half the element's own font size,
    /// `0.25rem` a quarter of the root's; `line-height: 2rem` twice the
    /// root's.
    #[test]
    fn em_letter_spacing_and_rem_line_height_resolve() {
        let dump = built(
            "gap-text-units",
            "<div><p class=\"ls\">spaced</p><p class=\"rem\">root</p><p class=\"lh\">tall</p></div>",
            "html { color: #ffffff; font-size: 20px } \
             .ls { font-size: 10px; letter-spacing: 0.5em } \
             .rem { font-size: 10px; letter-spacing: 0.25rem } \
             .lh { font-size: 10px; line-height: 2rem }",
        );
        assert!(
            element_before(&dump, "spaced").contains("ls=Px(5.0)"),
            "{dump}"
        );
        assert!(
            element_before(&dump, "root").contains("ls=Px(5.0)"),
            "{dump}"
        );
        assert!(
            element_before(&dump, "tall").contains("lh=Px(40.0)"),
            "{dump}"
        );
    }

    /// `unset` on the new inherited text properties drops an earlier
    /// rule's value (they inherit the root's defaults here).
    #[test]
    fn unset_resets_the_text_properties() {
        let dump = built(
            "gap-text-unset",
            "<div><p>set</p><p class=\"u\">unset</p></div>",
            &format!(
                "{WHITE} p {{ text-align: center; line-height: 2; letter-spacing: 3px; text-transform: uppercase }} \
                 p.u {{ text-align: unset; line-height: unset; letter-spacing: unset; text-transform: unset }}"
            ),
        );
        let set = element_before(&dump, "SET");
        assert!(
            set.contains("align=") && set.contains("lh=") && set.contains("ls="),
            "{dump}"
        );
        let unset = element_before(&dump, "unset");
        assert!(
            !unset.contains("align=") && !unset.contains("lh=") && !unset.contains("ls="),
            "{dump}"
        );
    }

    /// `text-transform` with an unsupported extra keyword drops the whole
    /// declaration: the earlier rule's `uppercase` stands.
    #[test]
    fn text_transform_with_full_width_is_dropped() {
        let dump = built(
            "gap-text-transform",
            "<div><p class=\"l\">Lower</p><p class=\"c\">cap it</p></div>",
            &format!(
                "{WHITE} p {{ text-transform: uppercase }} \
                 .l {{ text-transform: lowercase full-width }} \
                 .c {{ text-transform: capitalize full-width }}"
            ),
        );
        assert!(dump.contains("\"LOWER\""), "{dump}");
        assert!(dump.contains("\"CAP IT\""), "{dump}");
    }

    /// Sibling combinators work inside a translation's markup: the
    /// fragment's elements carry their own sibling list.
    #[test]
    fn sibling_combinators_match_inside_translations() {
        let files = [
            ("page.html", "<div data-l10n-id=\"m\">fallback</div>"),
            (
                "a.css",
                "html { color: #ffffff } h1 + p { background-color: #ff0000 }",
            ),
            (
                "locales/en-US/main.ftl.ron",
                r#"(locale: "en-US", resources: ["ui.ftl"])"#,
            ),
            (
                "locales/en-US/ui.ftl",
                "m = <h1>head</h1><p>next</p><p>later</p>",
            ),
        ];
        let dump = TestUi::new("gap-fragment-siblings", &files)
            .stylesheet("a.css")
            .locale("locales/en-US/main.ftl.ron")
            .spawn("page.html", TemplateContext::new(), Node::default())
            .settle()
            .dump();
        assert!(
            element_before(&dump, "next").contains("bg=#ff0000"),
            "{dump}"
        );
        assert!(!element_before(&dump, "later").contains("bg="), "{dump}");
    }

    // ---- backgrounds -----------------------------------------------------

    /// `background-repeat: repeat-x` tiles one axis; `background-clip`
    /// picks the visual box (border-box: the dump's default, unflagged).
    #[test]
    fn background_repeat_axes_and_clip_boxes() {
        let dump = built(
            "gap-background",
            "<section><div class=\"x\">x</div><div class=\"y\">y</div>\
             <div class=\"content\">content</div><div class=\"border\">border</div></section>",
            &format!(
                "{WHITE} div {{ background-image: url(frame.png); padding: 4px }} \
                 .x {{ background-repeat: repeat-x }} .y {{ background-repeat: repeat-y }} \
                 .content {{ background-clip: content-box }} .border {{ background-clip: border-box }}"
            ),
        );
        assert!(
            dump.contains("div.x padding=4,4,4,4 bg-image=frame.png tiled=true,false"),
            "{dump}"
        );
        assert!(
            dump.contains("div.y padding=4,4,4,4 bg-image=frame.png tiled=false,true"),
            "{dump}"
        );
        assert!(dump.contains("div.content padding=4,4,4,4 bg-image=frame.png tiled=true,true visual-box=ContentBox"), "{dump}");
        assert!(
            dump.contains("div.border padding=4,4,4,4 bg-image=frame.png tiled=true,true\n"),
            "{dump}"
        );
    }

    /// The `background` shorthand sets the color and the image, and
    /// resets `background-repeat` to `repeat`.
    #[test]
    fn background_shorthand_sets_color_image_and_resets_repeat() {
        let dump = built(
            "gap-background-shorthand",
            "<section><div class=\"c\">c</div><div class=\"i\">i</div></section>",
            &format!(
                "{WHITE} .c {{ background: #ff0000 }} \
                 .i {{ background-repeat: no-repeat; background: url(frame.png) }}"
            ),
        );
        assert!(dump.contains("div.c bg=#ff0000"), "{dump}");
        // The shorthand's omitted color resets to transparent.
        let image = element_before(&dump, "i");
        assert!(
            image.starts_with("div.i ") && image.ends_with(" bg-image=frame.png tiled=true,true"),
            "{dump}"
        );
    }

    // ---- scoped interaction restyles (ADR 0029) ----------------------------

    /// A hover that sets a container's color back to the initial white:
    /// its children must re-cascade (the cached container spec's style is
    /// what the walk compares the fresh inherited style against).
    #[test]
    fn hover_back_to_the_default_color_recascades_children() {
        let files = [
            (
                "page.html",
                "<div class=\"row\" id=\"r\"><p>child</p></div><p>other</p>",
            ),
            (
                "a.css",
                "html { color: #ffffff } .row { color: #ff0000 } .row:hover { color: #ffffff }",
            ),
        ];
        let mut ui = TestUi::new("gap-hover-default", &files)
            .stylesheet("a.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        let dump = ui.settle().dump();
        assert_eq!(run_color(&dump, "child"), "#ff0000", "{dump}");
        hover(&mut ui, "r", true);
        let dump = ui.dump();
        assert_eq!(run_color(&dump, "child"), "#ffffff", "{dump}");
        hover(&mut ui, "r", false);
        let dump = ui.dump();
        assert_eq!(run_color(&dump, "child"), "#ff0000", "{dump}");
    }

    /// A boxed block (one with a background) under a hovered ancestor
    /// restyles: its spec records that it reacts to the flip.
    #[test]
    fn boxed_block_under_a_hovered_ancestor_restyles() {
        let files = [
            (
                "page.html",
                "<div class=\"row\" id=\"r\"><p class=\"boxed\">boxed</p></div><p>other</p>",
            ),
            (
                "a.css",
                "html { color: #ffffff } .boxed { background-color: #000080 } .row:hover .boxed { color: #00ff00 }",
            ),
        ];
        let mut ui = TestUi::new("gap-hover-boxed", &files)
            .stylesheet("a.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        let dump = ui.settle().dump();
        assert_eq!(run_color(&dump, "boxed"), "#ffffff", "{dump}");
        hover(&mut ui, "r", true);
        let dump = ui.dump();
        assert_eq!(run_color(&dump, "boxed"), "#00ff00", "{dump}");
    }

    /// An inline run whose rule depends on an ancestor's hover restyles
    /// with it (`.row:hover b`).
    #[test]
    fn inline_run_under_a_hovered_ancestor_restyles() {
        let files = [
            (
                "page.html",
                "<div class=\"row\" id=\"r\"><p>plain <b>bold</b></p></div><p>other</p>",
            ),
            (
                "a.css",
                "html { color: #ffffff } .row:hover b { color: #00ff00 }",
            ),
        ];
        let mut ui = TestUi::new("gap-hover-inline", &files)
            .stylesheet("a.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        let dump = ui.settle().dump();
        // One style, one run.
        assert_eq!(run_color(&dump, "plain bold"), "#ffffff", "{dump}");
        hover(&mut ui, "r", true);
        let dump = ui.dump();
        assert_eq!(run_color(&dump, "plain "), "#ffffff", "{dump}");
        assert_eq!(run_color(&dump, "bold"), "#00ff00", "{dump}");
    }

    // ---- full-page restyle triggers (ADR 0029 `full_style`) --------------

    /// Swapping the UI's own stylesheet restyles every element (nothing
    /// may reuse a cached spec styled by the old sheet).
    #[test]
    fn own_stylesheet_swap_restyles_unchanged_content() {
        let files = [
            ("page.html", "<div><p>a</p><p>b</p></div>"),
            ("a.css", "html { color: #ffffff } p { color: #ff0000 }"),
            ("b.css", "html { color: #ffffff } p { color: #0000ff }"),
        ];
        let mut ui = TestUi::new("gap-own-swap", &files).spawn(
            "page.html",
            TemplateContext::new(),
            Node::default(),
        );
        let a = ui.load::<Stylesheet>("a.css");
        let root = ui.root();
        ui.world_mut().entity_mut(root).insert(HtmlStylesheet(a));
        let dump = ui.settle().dump();
        assert_eq!(run_color(&dump, "b"), "#ff0000", "{dump}");
        let b = ui.load::<Stylesheet>("b.css");
        ui.world_mut().entity_mut(root).insert(HtmlStylesheet(b));
        let dump = ui.settle().dump();
        assert_eq!(run_color(&dump, "a"), "#0000ff", "{dump}");
        assert_eq!(run_color(&dump, "b"), "#0000ff", "{dump}");
    }

    /// Swapping `DefaultStylesheet` restyles a UI without its own sheet.
    #[test]
    fn default_stylesheet_swap_restyles_unchanged_content() {
        let files = [
            ("page.html", "<div><p>a</p><p>b</p></div>"),
            ("a.css", "html { color: #ffffff } p { color: #ff0000 }"),
            ("b.css", "html { color: #ffffff } p { color: #0000ff }"),
        ];
        let mut ui = TestUi::new("gap-default-swap", &files)
            .stylesheet("a.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        let dump = ui.settle().dump();
        assert_eq!(run_color(&dump, "b"), "#ff0000", "{dump}");
        let b = ui.load::<Stylesheet>("b.css");
        ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(b);
        let dump = ui.settle().dump();
        assert_eq!(run_color(&dump, "a"), "#0000ff", "{dump}");
        assert_eq!(run_color(&dump, "b"), "#0000ff", "{dump}");
    }

    /// The same swap to a sheet that has already loaded: no `Wait`
    /// latches the full-page restyle, so the swap frame's own signal must
    /// (`own != Ready && default_changed` for a UI without its own sheet).
    #[test]
    fn default_stylesheet_swap_to_a_loaded_sheet_restyles() {
        let files = [
            ("page.html", "<div><p>a</p><p>b</p></div>"),
            ("a.css", "html { color: #ffffff } p { color: #ff0000 }"),
            ("b.css", "html { color: #ffffff } p { color: #0000ff }"),
        ];
        let mut ui = TestUi::new("gap-default-swap-loaded", &files)
            .stylesheet("a.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        let b = ui.load::<Stylesheet>("b.css");
        let dump = ui.settle().dump();
        assert_eq!(run_color(&dump, "b"), "#ff0000", "{dump}");
        assert!(
            ui.app_mut()
                .world()
                .resource::<AssetServer>()
                .is_loaded_with_dependencies(&b)
        );
        ui.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(b);
        let dump = ui.settle().dump();
        assert_eq!(run_color(&dump, "a"), "#0000ff", "{dump}");
        assert_eq!(run_color(&dump, "b"), "#0000ff", "{dump}");
    }

    /// A context `var()` change restyles every element using the sheet's
    /// variable (the content renders identically).
    #[test]
    fn sheet_var_change_restyles_unchanged_content() {
        let files = [
            ("page.html", "<div><p>a</p><p>b</p></div>"),
            ("a.css", "html { color: #ffffff } p { color: var(--c) }"),
        ];
        let mut ui = TestUi::new("gap-var", &files).stylesheet("a.css").spawn(
            "page.html",
            TemplateContext::new().with("--c", &"#ff0000"),
            Node::default(),
        );
        let dump = ui.settle().dump();
        assert_eq!(run_color(&dump, "b"), "#ff0000", "{dump}");
        let root = ui.root();
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert("--c", &"#0000ff");
        let dump = ui.settle().dump();
        assert_eq!(run_color(&dump, "a"), "#0000ff", "{dump}");
        assert_eq!(run_color(&dump, "b"), "#0000ff", "{dump}");
    }

    // ---- skeleton reuse (ADR 0021) ---------------------------------------

    /// Renders `<p>`s from `values`, then changes them to `changed`, and
    /// returns the skeleton's `(useful, strike)` after the content build.
    fn health_after(name: &str, values: &[&str], changed: &[&str]) -> (bool, u8) {
        health_after_with(name, values, changed, WHITE)
    }

    fn health_after_with(name: &str, values: &[&str], changed: &[&str], css: &str) -> (bool, u8) {
        health_after_in(name, values, changed, css, false)
    }

    /// `nested`: the paragraphs sit in a `section` (the nested pairing
    /// path) instead of at the document's top level.
    fn health_after_in(
        name: &str,
        values: &[&str],
        changed: &[&str],
        css: &str,
        nested: bool,
    ) -> (bool, u8) {
        let page: String = (0..values.len())
            .map(|i| format!("<p>{{{{ v{i} }}}}</p>"))
            .collect();
        let page = if nested {
            format!("<section>{page}</section>")
        } else {
            page
        };
        let keys: Vec<&'static str> = (0..values.len())
            .map(|i| &*Box::leak(format!("v{i}").into_boxed_str()))
            .collect();
        let mut context = TemplateContext::new();
        for (key, value) in keys.iter().zip(values) {
            context.insert(*key, value);
        }
        let files = [("page.html", page.as_str()), ("a.css", css)];
        let mut ui = TestUi::new(name, &files).stylesheet("a.css").spawn(
            "page.html",
            context,
            Node::default(),
        );
        ui.settle();
        let root = ui.root();
        let mut context = ui.world_mut().get_mut::<TemplateContext>(root).unwrap();
        for (key, value) in keys.iter().zip(changed) {
            context.insert(*key, value);
        }
        ui.settle();
        ui.world_mut()
            .get::<Skeleton>(root)
            .unwrap()
            .pairing_health()
    }

    /// One changed paragraph of four: three subtrees reuse, so the
    /// pairing stays useful (no strike) — reuse actually happens on a
    /// content build.
    #[test]
    fn a_mostly_unchanged_page_reuses_and_stays_useful() {
        assert_eq!(
            health_after(
                "gap-reuse-useful",
                &["a", "b", "c", "d"],
                &["a", "b", "c", "D"]
            ),
            (true, 0)
        );
    }

    /// Two changed paragraphs of three: one subtree reuses, under half —
    /// the pairing is useless and takes a strike (the backoff).
    #[test]
    fn a_mostly_changed_page_strikes_the_pairing() {
        assert_eq!(
            health_after("gap-reuse-useless", &["a", "b", "c"], &["a", "B", "C"]),
            (false, 1)
        );
    }

    /// Content reuse inside a container: one changed paragraph of four in
    /// a `section` reuses the other three (bug_0150: a changed container
    /// had no pairing entry, so ADR 0029's inherited-style gate made every
    /// child re-collect — reuse worked only at the document's top level).
    #[test]
    fn a_changed_container_reuses_its_unchanged_children() {
        assert_eq!(
            health_after_in(
                "gap-reuse-nested",
                &["a", "b", "c", "d"],
                &["a", "b", "c", "D"],
                WHITE,
                true
            ),
            (true, 0)
        );
    }

    /// Renders `page` with `key = from`, changes it to `to`, and returns the
    /// skeleton's `(useful, strike)` after that content build.
    fn health_after_page(
        name: &str,
        page: &str,
        css: &str,
        key: &'static str,
        from: &str,
        to: &str,
    ) -> (bool, u8) {
        let files = [("page.html", page), ("a.css", css)];
        let mut ui = TestUi::new(name, &files).stylesheet("a.css").spawn(
            "page.html",
            TemplateContext::new().with(key, &from),
            Node::default(),
        );
        ui.settle();
        let root = ui.root();
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert(key, &to);
        ui.settle();
        ui.world_mut()
            .get::<Skeleton>(root)
            .unwrap()
            .pairing_health()
    }

    /// An attribute change leaves positions alone: a position-only sheet
    /// keeps reusing the other siblings (ADR 0030 §5's refinement; the
    /// first cut un-cleaned the whole list on any attribute change).
    #[test]
    fn an_attribute_change_keeps_reuse_under_a_position_only_sheet() {
        let page = r#"<section><p>a</p><p>b</p><p class="{{ c }}">c</p><p>d</p><p>e</p><p>f</p></section>"#;
        let css = format!(
            "{WHITE} p:nth-child(2n) {{ color: #ff0000 }} p:last-of-type {{ color: #00ff00 }}"
        );
        assert_eq!(
            health_after_page("gap-attr-positions", page, &css, "c", "x", "y"),
            (true, 0)
        );
    }

    /// Under a `+`-only sheet an attribute change un-cleans just the next
    /// element: the rest reuses.
    #[test]
    fn an_attribute_change_unclean_only_the_next_sibling_under_adjacent_rules() {
        let page = r#"<section><p>a</p><p class="{{ c }}">b</p><p>c</p><p>d</p><p>e</p><p>f</p></section>"#;
        let css = format!("{WHITE} .hot + p {{ color: #ff0000 }}");
        assert_eq!(
            health_after_page("gap-attr-adjacent", page, &css, "c", "hot", "cold"),
            (true, 0)
        );
    }

    /// A sibling-dependent sheet still reuses across a text-only change:
    /// the sibling list keeps its shape, so nothing is un-cleaned (ADR
    /// 0030 §5 must not turn reuse off for such sheets).
    #[test]
    fn a_sibling_dependent_sheet_reuses_across_text_changes() {
        let css = "html { color: #ffffff } p:nth-child(2) { color: #ff0000 } p + p { background-color: #000080 }";
        assert_eq!(
            health_after_with(
                "gap-reuse-sibling-sheet",
                &["a", "b", "c", "d"],
                &["a", "b", "c", "D"],
                css
            ),
            (true, 0)
        );
        assert_eq!(
            health_after_in(
                "gap-reuse-sibling-sheet-nested",
                &["a", "b", "c", "d"],
                &["a", "b", "c", "D"],
                css,
                true
            ),
            (true, 0),
            "the same inside a container"
        );
    }
}

/// ADR 0031: a windowed UI materializes the scroll window's band and
/// replaces the culled subtrees with height-preserving spacers — above
/// and below the fold — so the document's total height (the scroll
/// range) is unchanged. Scrolling moves the band.
#[test]
fn windowed_build_materializes_the_band_and_spacers_the_rest() {
    let page: String = (1..=60)
        .map(|i| format!("<p id=\"p{i}\">Paragraph {i}</p>"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut ui = TestUi::with_layout(
        "windowed",
        &[
            ("page.html", &page),
            ("style.css", "html { flex-direction: column }"),
        ],
        UVec2::new(300, 200),
    )
    .stylesheet("style.css")
    // A scrolling viewport: the root is a fixed-height clipped column
    // (a content-sized root grows to its content — taffy's root minimum —
    // and the band would cover the whole document).
    .spawn(
        "page.html",
        TemplateContext::new(),
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(200.0),
            flex_direction: FlexDirection::Column,
            overflow: Overflow {
                x: OverflowAxis::Visible,
                y: OverflowAxis::Scroll,
            },
            ..default()
        },
    );
    let root = ui.root();
    ui.world_mut()
        .entity_mut(root)
        .insert(HtmlWindowed::default());
    // Build 1: the model is empty, so the whole document materializes and
    // is measured; the measurement's news rebuild it once (review_0006
    // J08), culled to the band at scroll 0 — [−200, 400): p1 … p20.
    ui.settle();
    let first = ui.dump();
    assert_eq!(first.matches("\n  p#p").count(), 20, "{first}");
    assert!(
        first.contains("p#p20\n") && !first.contains("p#p21\n"),
        "{first}"
    );

    // Scroll: the band is [offset, offset + viewport] grown by one
    // viewport on each side. Each paragraph is one 20px line (16px text
    // at 1.2, ceiled), so paragraph i's margin box is [(i-1)*20, i*20].
    let offset = 300.0;
    ui.world_mut()
        .entity_mut(root)
        .insert(ScrollPosition(Vec2::new(0.0, offset)));
    ui.settle();
    let dump = ui.dump();

    // Materialized: the paragraphs intersecting the half-open band
    // [100, 700) — p6 (y=100) through p35 (y=680..700). The band's edge
    // is exclusive: a block ending exactly at band.0 or starting at
    // band.1 is out.
    for i in 6..=35 {
        assert!(
            dump.contains(&format!("p#p{i}\n")),
            "paragraph {i} must be materialized: {dump}"
        );
    }
    // Culled: everything else, replaced by spacers preserving the
    // document's height (5 paragraphs above, 25 below).
    for i in [1, 2, 3, 4, 5, 36, 45, 60] {
        assert!(
            !dump.contains(&format!("p#p{i}\n")),
            "paragraph {i} must be culled: {dump}"
        );
    }
    // The culled runs coalesce: one spacer above (5 paragraphs = 100px),
    // one below (25 = 500px) — a flat culled document costs taffy one
    // node per run, not one per block.
    assert_eq!(dump.matches("spacer h=100").count(), 1, "{dump}");
    assert_eq!(dump.matches("spacer h=500").count(), 1, "{dump}");
    // The height invariant, measured: the laid-out content (spacers plus
    // the materialized paragraphs) is the full document's 60 × 20 px —
    // and the band sits where the full document puts it (p6's top at
    // 100 px in the document, 200 px above the viewport at offset 300).
    // (The previous assertion compared constants: review_0006 J06.)
    ui.update(3);
    let world = ui.world_mut();
    let content = world.get::<ComputedNode>(root).expect("laid out");
    assert!(
        (content.content_size.y * content.inverse_scale_factor - 1200.0).abs() < 1.0,
        "{content:?}"
    );
    let p6 = element_by_id(world, root, "p6");
    let rect = node_rect(world, p6).expect("p6 laid out");
    assert!(
        (rect.min.y - (100.0 - offset)).abs() < 1.0,
        "p6 at {rect:?}"
    );

    // Scrolling further moves the band; the top part culled, the bottom
    // part materialized.
    ui.world_mut()
        .entity_mut(root)
        .insert(ScrollPosition(Vec2::new(0.0, 900.0)));
    ui.settle();
    let dump = ui.dump();
    // The band is [offset − margin, offset + viewport + margin] =
    // [700, 1300): p36 (y=700) through p60.
    assert!(
        dump.contains("p#p36\n") && dump.contains("p#p60\n"),
        "{dump}"
    );
    assert!(
        !dump.contains("p#p1\n") && !dump.contains("p#p35\n"),
        "{dump}"
    );
}

/// Mouse-wheel scrolling through the real picking path: the wheel event
/// (what winit sends) bubbles `PointerScroll` from the hovered row to the
/// root's `ScrollArea`, bevy's widget scrolls it, and — ADR 0031 — a
/// windowed UI rebuilds at the new scroll offset.
#[test]
fn mouse_wheel_scrolls_a_windowed_ui() {
    use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
    use bevy::ui_widgets::ScrollArea;
    use bevy::window::WindowEvent;

    let page: String = (1..=40)
        .map(|i| format!("<p id=\"p{i}\">Paragraph {i}</p>"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut ui = TestUi::with_pointer(
        "wheel-scroll",
        &[
            ("page.html", &page),
            ("style.css", "html { flex-direction: column }"),
        ],
        UVec2::new(300, 200),
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new(),
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(200.0),
            flex_direction: FlexDirection::Column,
            overflow: Overflow {
                x: OverflowAxis::Visible,
                y: OverflowAxis::Scroll,
            },
            ..default()
        },
    );
    let root = ui.root();
    ui.world_mut().entity_mut(root).insert((
        HtmlWindowed::default(),
        ScrollPosition::default(),
        ScrollArea,
    ));
    ui.settle();
    // Scroll down: the wheel's y is negative (the content moves up).
    ui.move_pointer(Vec2::new(150.0, 100.0));
    let window = ui.window();
    ui.world_mut()
        .resource_mut::<Messages<WindowEvent>>()
        .write(WindowEvent::MouseWheel(MouseWheel {
            window,
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: -5.0,
            phase: bevy::input::touch::TouchPhase::Moved,
        }));
    ui.update(3);
    let scroll = ui
        .world_mut()
        .get::<ScrollPosition>(root)
        .expect("the root scrolls")
        .0
        .y;
    assert!(scroll > 0.0, "the wheel scrolled the root: {scroll}");
    // The windowed build followed the offset: the paragraph at the
    // viewport's top is built and sits where the scroll puts it (20 px
    // per paragraph). Checking only that "a spacer exists" passed with
    // the band hundreds of pixels off (review_0006 J06).
    ui.update(10);
    let scroll = ui.world_mut().get::<ScrollPosition>(root).unwrap().0.y;
    let index = (scroll / 20.0).floor() as usize + 1;
    let world = ui.world_mut();
    let top = element_by_id(world, root, &format!("p{index}"));
    let rect = node_rect(world, top).expect("the top paragraph is built");
    let expected = (index as f32 - 1.0) * 20.0 - scroll;
    assert!(
        (rect.min.y - expected).abs() < 1.0,
        "p{index} at {:.1}, expected {expected:.1}",
        rect.min.y
    );
}

/// review_0006: the windowed build (ADR 0031) and bug_0151.
/// bug_0172: a document's `<body>` keeps its style and its place in
/// selectors. ADR 0031's root unwrap collected `body`'s children with the
/// root's style and no ancestors: `body { … }`, `body > p` and
/// `.b section p` were lost for every `<html><body>` document.
#[test]
fn body_styles_and_selectors_hold_in_a_whole_document() {
    let page = "<html><head><title>t</title></head><body class=\"b\"><p id=\"x\">x</p><section><p id=\"y\">y</p></section></body></html>";
    let css = "html { flex-direction: column; color: #ffffff } .b { color: #ff0000 } body > p { font-size: 30px } .b section p { color: #00ff00 }";
    let mut ui = TestUi::new("bug0172", &[("page.html", page), ("style.css", css)])
        .stylesheet("style.css")
        .spawn("page.html", TemplateContext::new(), Node::default());
    ui.settle();
    ui.assert_dump(
        "html-ui dir=Column
  p#x
    \"x\" default 30px #ff0000
  section
    p#y
      \"y\" default 16px #00ff00
",
    );
}

mod review_0006 {
    use super::*;

    /// A scrolling root of `rows` one-line paragraphs, optionally
    /// windowed, settled at `offset`.
    fn scrolled(name: &str, css: &str, rows: usize, windowed: bool, offset: f32) -> TestUi {
        let page: String = (1..=rows)
            .map(|i| format!("<p id=\"p{i}\">Paragraph {i}</p>"))
            .collect::<Vec<_>>()
            .join("\n");
        scrolled_page(name, css, &page, windowed, offset)
    }

    /// The testbench's big-document shape: row containers (their own
    /// `gap`, laid out as rows) under a root `gap`.
    fn bench_rows(rows: usize) -> String {
        (1..=rows)
            .map(|i| format!("<div class=\"row\" id=\"r{i}\"><p class=\"k\">unit-{i}</p><p class=\"v\">{}</p></div>", i * 7 % 1000))
            .collect::<Vec<_>>()
            .join("\n")
    }

    const BENCH_CSS: &str = "html { flex-direction: column; gap: 2px } .row { flex-direction: row; gap: 8px; padding: 1px 4px; border-width: 1px }";

    fn scrolled_page(name: &str, css: &str, page: &str, windowed: bool, offset: f32) -> TestUi {
        scrolled_with(
            name,
            css,
            page,
            windowed.then(HtmlWindowed::default),
            offset,
        )
    }

    fn scrolled_with(
        name: &str,
        css: &str,
        page: &str,
        windowed: Option<HtmlWindowed>,
        offset: f32,
    ) -> TestUi {
        scrolled_at_scale(name, css, page, windowed, offset, 1.0)
    }

    /// As [`scrolled_with`] on a display of scale factor `scale` (the
    /// camera's target: physical size × scale; rects come out physical).
    fn scrolled_at_scale(
        name: &str,
        css: &str,
        page: &str,
        windowed: Option<HtmlWindowed>,
        offset: f32,
        scale: f32,
    ) -> TestUi {
        let mut ui = TestUi::with_layout(
            name,
            &[("page.html", page), ("style.css", css)],
            UVec2::new(300, 200),
        )
        .stylesheet("style.css")
        .spawn(
            "page.html",
            TemplateContext::new(),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(200.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow {
                    x: OverflowAxis::Visible,
                    y: OverflowAxis::Scroll,
                },
                ..default()
            },
        );
        if scale != 1.0 {
            let world = ui.world_mut();
            let mut cameras = world.query::<&mut Camera>();
            for mut camera in cameras.iter_mut(world) {
                if let Some(info) = camera.computed.target_info.as_mut() {
                    info.physical_size = (UVec2::new(300, 200).as_vec2() * scale).as_uvec2();
                    info.scale_factor = scale;
                }
            }
        }
        let root = ui.root();
        let windowed = match windowed {
            Some(component) => {
                ui.world_mut().entity_mut(root).insert(component);
                true
            }
            None => false,
        };
        ui.settle();
        ui.world_mut()
            .entity_mut(root)
            .insert(ScrollPosition(Vec2::new(0.0, offset)));
        // A scroll rebuilds only a windowed UI, and only when its band
        // changes (J05) — `settle` demands a build, so run frames instead.
        let _ = windowed;
        ui.update(12);
        ui
    }

    /// Each element's (top, bottom) by id, in the window's coordinates.
    fn rects(ui: &mut TestUi) -> std::collections::BTreeMap<String, (f32, f32)> {
        let world = ui.world_mut();
        let mut elements = world.query::<(Entity, &HtmlElement)>();
        let found: Vec<(Entity, String)> = elements
            .iter(world)
            .filter_map(|(entity, element)| element.id.clone().map(|id| (entity, id)))
            .collect();
        found
            .into_iter()
            .filter_map(|(entity, id)| {
                node_rect(world, entity).map(|rect| (id, (rect.min.y, rect.max.y)))
            })
            .collect()
    }

    /// bug_0151: every materialized row of a windowed UI sits where a
    /// full (unwindowed) build of the same document puts it, at the same
    /// scroll offset — and the rows the viewport shows are materialized.
    /// With a root `gap`, the coalesced above-spacer dropped the gaps
    /// between its culled rows, so the band rose by (culled − 1) × gap:
    /// 4 px × ~100 rows here, the whole band off the viewport.
    #[test]
    fn windowed_rows_sit_where_the_full_build_puts_them() {
        let css = "html { flex-direction: column; gap: 4px }";
        let offset = 2400.0;
        let mut full = scrolled("bug0151-full", css, 300, false, offset);
        let mut windowed = scrolled("bug0151-windowed", css, 300, true, offset);
        let (full, windowed) = (rects(&mut full), rects(&mut windowed));
        assert!(
            windowed.len() < 300,
            "the windowed UI culled: {} materialized",
            windowed.len()
        );
        let mut wrong = Vec::new();
        for (id, rect) in &windowed {
            let expected = full[id];
            if (rect.0 - expected.0).abs() > 1.0 {
                wrong.push(format!(
                    "{id}: at {:.0}, full build {:.0}",
                    rect.0, expected.0
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of {} misplaced, e.g. {:?}",
            wrong.len(),
            windowed.len(),
            &wrong[..wrong.len().min(3)]
        );
        // The viewport (window y 0..200) shows materialized rows.
        let visible: Vec<&String> = full
            .iter()
            .filter(|(_, (top, bottom))| *bottom > 0.0 && *top < 200.0)
            .map(|(id, _)| id)
            .collect();
        assert!(!visible.is_empty());
        for id in visible {
            assert!(
                windowed.contains_key(id),
                "{id} is in the viewport but not materialized"
            );
        }
    }

    /// Compares a windowed UI's materialized rows with a full build at
    /// the same scroll offset; returns the misplaced and missing rows.
    fn band_errors(full: &mut TestUi, windowed: &mut TestUi) -> Vec<String> {
        band_errors_in(full, windowed, (0.0, 200.0))
    }

    /// As [`band_errors`] with the visible region `(top, bottom)` in
    /// window px (a bordered root clips at its padding box).
    fn band_errors_in(
        full: &mut TestUi,
        windowed: &mut TestUi,
        visible: (f32, f32),
    ) -> Vec<String> {
        let (full, windowed) = (rects(full), rects(windowed));
        let mut errors: Vec<String> = windowed
            .iter()
            .filter(|(id, rect)| (rect.0 - full[*id].0).abs() > 1.0)
            .map(|(id, rect)| format!("{id}: at {:.0}, full build {:.0}", rect.0, full[id].0))
            .collect();
        errors.extend(
            full.iter()
                .filter(|(id, (top, bottom))| {
                    *bottom > visible.0 && *top < visible.1 && !windowed.contains_key(*id)
                })
                .map(|(id, _)| format!("{id}: visible but not materialized")),
        );
        errors
    }

    /// bug_0164: the band follows the offset the root shows, not the
    /// app's `ScrollPosition`. Bevy clamps the position only for its
    /// layout, so an overshoot (here, far past the end) kept the band
    /// below the content: everything culled, a blank viewport.
    #[test]
    fn an_overshooting_scroll_shows_the_end() {
        let mut full = scrolled(
            "bug0160-over-full",
            "html { flex-direction: column }",
            300,
            false,
            1.0e6,
        );
        let mut windowed = scrolled(
            "bug0160-over-windowed",
            "html { flex-direction: column }",
            300,
            true,
            1.0e6,
        );
        let errors = band_errors(&mut full, &mut windowed);
        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            rects(&mut windowed).contains_key("p300"),
            "the last row is built"
        );
    }

    /// bug_0164 the way users meet it (found by `tests/windowing_model.rs`):
    /// a view scrolled to the bottom of a list that then shrinks. The
    /// app's `ScrollPosition` stays past the new end.
    #[test]
    fn content_shrinking_under_a_bottom_scrolled_view_shows_the_end() {
        let page: String = (1..=300)
            .map(|i| {
                let row = format!("<p id=\"p{i}\">Paragraph {i}</p>");
                if i > 100 {
                    format!("{{% if not short is defined %}}{row}{{% endif %}}")
                } else {
                    row
                }
            })
            .collect();
        let css = "html { flex-direction: column }";
        let bottom = 300.0 * 19.2 - 200.0;
        let mut full = scrolled_page("bug0160-shrink-full", css, &page, false, bottom);
        let mut windowed = scrolled_page("bug0160-shrink-windowed", css, &page, true, bottom);
        for ui in [&mut full, &mut windowed] {
            let root = ui.root();
            ui.world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("short", &true);
            ui.update(12);
        }
        let errors = band_errors(&mut full, &mut windowed);
        assert!(errors.is_empty(), "{errors:#?}");
        let built = rects(&mut windowed);
        assert!(
            built.contains_key("p100") && !built.contains_key("p101"),
            "the new end is built"
        );
    }

    /// bug_0151 in the testbench's shape (the project owner's run): row
    /// containers with their own `gap` under a root `gap`, at several
    /// depths. The root cursor advanced by the last pushed frame's gap —
    /// a materialized row's 8 px, not the root's 2 px.
    #[test]
    fn windowed_bench_rows_track_the_scroll_at_every_depth() {
        let page = bench_rows(600);
        for offset in [500.0, 4000.0, 9000.0] {
            let mut full = scrolled_page("bug0151-rows-full", BENCH_CSS, &page, false, offset);
            let mut windowed =
                scrolled_page("bug0151-rows-windowed", BENCH_CSS, &page, true, offset);
            let errors = band_errors(&mut full, &mut windowed);
            assert!(
                errors.is_empty(),
                "offset {offset}: {} errors, e.g. {:?}",
                errors.len(),
                &errors[..errors.len().min(3)]
            );
        }
    }

    /// The band with no margin is exactly the viewport: a cursor that runs
    /// ahead of the layout (the root advanced by a materialized row's own
    /// 8 px gap instead of the root's 2 px) leaves the viewport's lower
    /// rows unmaterialized — a one-viewport margin hid it.
    #[test]
    fn a_marginless_band_still_covers_the_viewport() {
        let page = bench_rows(600);
        for offset in [500.0, 4000.0, 9000.0] {
            let mut full = scrolled_page("bug0151-tight-full", BENCH_CSS, &page, false, offset);
            let mut windowed = scrolled_with(
                "bug0151-tight-windowed",
                BENCH_CSS,
                &page,
                Some(HtmlWindowed {
                    margin_viewports: 0.0,
                }),
                offset,
            );
            let errors = band_errors(&mut full, &mut windowed);
            assert!(
                errors.is_empty(),
                "offset {offset}: {} errors, e.g. {:?}",
                errors.len(),
                &errors[..errors.len().min(3)]
            );
        }
    }

    /// The number of materialized elements and of entities carrying a
    /// `WindowModel` (only the windowed root should).
    fn census(ui: &mut TestUi) -> (usize, usize) {
        let world = ui.world_mut();
        let elements = world.query::<&HtmlElement>().iter(world).count();
        let models = world
            .query::<&bevy_markup::WindowModel>()
            .iter(world)
            .count();
        (elements, models)
    }

    /// J02: content inside one container (most real pages: `<main>`,
    /// a `<section>`) is windowed too. `measure_window_heights` recorded
    /// only the root's direct children, so a container's children never
    /// had heights and nothing inside it was ever culled: 600 rows in a
    /// `<main>` materialized all 1 801 elements.
    #[test]
    fn content_inside_a_container_is_windowed() {
        let page = format!("<main>{}</main>", bench_rows(600));
        let mut full = scrolled_page("j02-nested-full", BENCH_CSS, &page, false, 4000.0);
        let mut windowed = scrolled_page("j02-nested-windowed", BENCH_CSS, &page, true, 4000.0);
        let (elements, _) = census(&mut windowed);
        assert!(elements < 300, "{elements} of 1 801 elements materialized");
        let errors = band_errors(&mut full, &mut windowed);
        assert!(
            errors.is_empty(),
            "{} errors, e.g. {:?}",
            errors.len(),
            &errors[..errors.len().min(3)]
        );
    }

    /// J02, deeper: two container levels with margins, borders and
    /// padding. The rows inside sit where the full build puts them (the
    /// container cursor counted its margin twice, invisible while nested
    /// frames culled nothing), and nothing far away is built.
    #[test]
    fn nested_containers_with_boxes_cull_in_place() {
        let rows = bench_rows(600);
        let page = format!("<main><section class=\"list\">{rows}</section></main>");
        let css = format!(
            "{BENCH_CSS} main {{ margin-top: 30px; padding: 6px; border-width: 3px }} \
             .list {{ margin-top: 40px; padding-top: 5px; gap: 3px }}"
        );
        for offset in [700.0, 5000.0] {
            let mut full = scrolled_page("j02-boxes-full", &css, &page, false, offset);
            // No band margin: a cursor error shows as unbuilt rows at the
            // viewport's edge instead of hiding in the margin.
            let mut windowed = scrolled_with(
                "j02-boxes-windowed",
                &css,
                &page,
                Some(HtmlWindowed {
                    margin_viewports: 0.0,
                }),
                offset,
            );
            let (elements, _) = census(&mut windowed);
            assert!(
                elements < 300,
                "offset {offset}: {elements} elements materialized"
            );
            let errors = band_errors(&mut full, &mut windowed);
            assert!(
                errors.is_empty(),
                "offset {offset}: {} errors, e.g. {:?}",
                errors.len(),
                &errors[..errors.len().min(3)]
            );
        }
    }

    /// A UI without `HtmlWindowed` gets no `WindowModel` (every UI's
    /// entities carry keys; the measurement used to insert a model on any
    /// keyed entity's parent).
    #[test]
    fn an_unwindowed_ui_has_no_window_model() {
        let page = format!("<main>{}</main>", bench_rows(50));
        let mut ui = scrolled_page("j02-unwindowed", BENCH_CSS, &page, false, 0.0);
        let (_, models) = census(&mut ui);
        assert_eq!(models, 0);
    }

    /// Only windowed UIs' entities carry a `WindowKey` (the measurement
    /// visits keyed entities every frame their layout changes; keys on
    /// every UI made non-windowed UIs pay it too). A windowed UI's built
    /// elements are all keyed — including the ones its first, window-less
    /// build (no layout yet) spawned.
    #[test]
    fn window_keys_only_on_windowed_uis() {
        let keyed = |ui: &mut TestUi| -> (usize, usize) {
            let world = ui.world_mut();
            let elements = world.query::<&HtmlElement>().iter(world).count();
            let keys = world
                .query::<(&HtmlElement, &bevy_markup::WindowKey)>()
                .iter(world)
                .count();
            (elements, keys)
        };
        let page = format!("<main>{}</main>", bench_rows(50));
        let mut plain = scrolled_page("keys-unwindowed", BENCH_CSS, &page, false, 0.0);
        assert_eq!(keyed(&mut plain).1, 0, "an unwindowed UI carries keys");
        let mut windowed = scrolled_page("keys-windowed", BENCH_CSS, &page, true, 300.0);
        let (elements, keys) = keyed(&mut windowed);
        assert_eq!(
            keys, elements,
            "every built element of a windowed UI is keyed"
        );
    }

    /// J02: only the windowed root carries a `WindowModel`. The
    /// measurement inserted one on the parent of every nested keyed
    /// entity (each row container: 25 for one band, 602 inside a `<main>`).
    #[test]
    fn only_the_windowed_root_carries_a_window_model() {
        let page = bench_rows(600);
        let mut windowed = scrolled_page("j02-models", BENCH_CSS, &page, true, 4000.0);
        let (_, models) = census(&mut windowed);
        assert_eq!(models, 1);
    }

    /// J03: a windowed root with padding (or border) settles and culls.
    /// The width check compared the root's border-box width with the
    /// model's content width, so they never agreed: the model cleared on
    /// every build (nothing culled) and the UI rebuilt every frame.
    #[test]
    fn a_padded_windowed_root_settles_and_culls() {
        let page = bench_rows(600);
        let css = format!("{BENCH_CSS} html {{ padding: 8px }}");
        let mut ui = TestUi::with_layout(
            "j03-padding",
            &[("page.html", page.as_str()), ("style.css", css.as_str())],
            UVec2::new(300, 200),
        )
        .stylesheet("style.css")
        .spawn(
            "page.html",
            TemplateContext::new(),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(200.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow {
                    x: OverflowAxis::Visible,
                    y: OverflowAxis::Scroll,
                },
                ..default()
            },
        );
        let root = ui.root();
        ui.world_mut().entity_mut(root).insert((
            HtmlWindowed::default(),
            ScrollPosition(Vec2::new(0.0, 4000.0)),
        ));
        ui.update(40);
        let before = ui.builds();
        ui.update(30);
        let builds = ui.builds() - before;
        let (elements, _) = census(&mut ui);
        assert_eq!(
            builds, 0,
            "an idle windowed UI rebuilt {builds} times in 30 frames"
        );
        assert!(elements < 300, "{elements} of 1 800 elements materialized");
    }

    /// J03 at a HiDPI scale: the model, the band and the width anchor in
    /// one unit system. The build read the root's physical size for the
    /// viewport and the width anchor, and the measurement scaled the
    /// (logical) margins by the inverse factor: at scale 2 the model
    /// cleared every build and nothing culled.
    #[test]
    fn a_hidpi_windowed_ui_culls_and_places_its_rows() {
        let page = bench_rows(600);
        let css = format!("{BENCH_CSS} .row {{ margin-top: 3px; margin-bottom: 2px }}");
        for offset in [600.0, 5000.0] {
            let mut full = scrolled_at_scale("j03-hidpi-full", &css, &page, None, offset, 2.0);
            let mut windowed = scrolled_at_scale(
                "j03-hidpi-windowed",
                &css,
                &page,
                Some(HtmlWindowed {
                    margin_viewports: 0.0,
                }),
                offset,
                2.0,
            );
            let before = windowed.builds();
            windowed.update(20);
            assert_eq!(
                windowed.builds() - before,
                0,
                "offset {offset}: an idle HiDPI UI rebuilt"
            );
            let (elements, _) = census(&mut windowed);
            assert!(
                elements < 300,
                "offset {offset}: {elements} elements materialized"
            );
            let errors = band_errors_in(&mut full, &mut windowed, (0.0, 400.0));
            assert!(
                errors.is_empty(),
                "offset {offset}: {} errors, e.g. {:?}",
                errors.len(),
                &errors[..errors.len().min(3)]
            );
            // A marginless band builds the viewport, no more: a physical
            // viewport (twice the logical one at scale 2) built twice the
            // rows.
            let visible = rects(&mut full)
                .values()
                .filter(|(top, bottom)| *bottom > 0.0 && *top < 400.0)
                .count();
            let built = rects(&mut windowed).len();
            assert!(
                built <= visible + 6,
                "offset {offset}: {built} elements built for {visible} visible"
            );
        }
    }

    /// J03 with a padded, bordered root: the band starts at the padding
    /// box (the visible region), not the content box.
    #[test]
    fn a_padded_bordered_root_places_its_band() {
        let page = bench_rows(600);
        let css = format!("{BENCH_CSS} html {{ padding: 40px 6px 30px; border-width: 5px }}");
        for offset in [300.0, 5000.0] {
            let mut full = scrolled_page("j03-boxed-full", &css, &page, false, offset);
            let mut windowed = scrolled_with(
                "j03-boxed-windowed",
                &css,
                &page,
                Some(HtmlWindowed {
                    margin_viewports: 0.0,
                }),
                offset,
            );
            // Visible: the padding box, inside the 5 px border.
            let errors = band_errors_in(&mut full, &mut windowed, (5.0, 195.0));
            assert!(
                errors.is_empty(),
                "offset {offset}: {} errors, e.g. {:?}",
                errors.len(),
                &errors[..errors.len().min(3)]
            );
        }
    }

    /// With `margin_viewports: 0`, a built element must touch the visible
    /// region `(top, bottom)` (window px), within `slack` px.
    fn tightness_errors(
        full: &mut TestUi,
        windowed: &mut TestUi,
        visible: (f32, f32),
        slack: f32,
    ) -> Vec<String> {
        let (full, windowed) = (rects(full), rects(windowed));
        windowed
            .keys()
            .filter(|id| {
                let (top, bottom) = full[*id];
                bottom < visible.0 - slack || top > visible.1 + slack
            })
            .map(|id| format!("{id}: built at {:?}, outside {visible:?}", full[id]))
            .collect()
    }

    /// The root's box and scale in every windowing term (review_0006's
    /// mutation run): an asymmetric border (the visible region's height),
    /// top padding (where the content starts), scrollbars on both axes (the
    /// scroll range, as taffy computes it), scale 2, a marginless band — checked
    /// for coverage *and* tightness, mid-document and past the end (the
    /// offset clamp, bug_0164).
    #[test]
    fn the_root_box_and_scale_bound_the_band_exactly() {
        let page = bench_rows(400);
        let css =
            format!("{BENCH_CSS} html {{ padding: 30px 6px 10px; border-width: 20px 3px 24px }}");
        // Vertical scrolling only (taffy's range counts no scrollbar: the
        // vertical one's width must not count) and both axes (the
        // horizontal one's height must).
        for (offset, both_axes) in [
            (2500.0, false),
            (1.0e6, false),
            (2500.0, true),
            (1.0e6, true),
        ] {
            let mut uis = [
                scrolled_at_scale("root-box-full", &css, &page, None, 0.0, 2.0),
                scrolled_at_scale(
                    "root-box-windowed",
                    &css,
                    &page,
                    Some(HtmlWindowed {
                        margin_viewports: 0.0,
                    }),
                    0.0,
                    2.0,
                ),
            ];
            for ui in &mut uis {
                let root = ui.root();
                let mut node = ui.world_mut().get_mut::<Node>(root).unwrap();
                node.scrollbar_width = 10.0;
                if both_axes {
                    node.overflow.x = OverflowAxis::Scroll;
                }
                ui.update(6);
                ui.world_mut()
                    .entity_mut(root)
                    .insert(ScrollPosition(Vec2::new(0.0, offset)));
                ui.update(12);
            }
            let [full, windowed] = &mut uis;
            // Physical px: the border is 20 px (40) on top and 24 px (48)
            // at the bottom of the 200 px (400) root.
            let errors = band_errors_in(full, windowed, (40.0, 352.0));
            assert!(
                errors.is_empty(),
                "offset {offset}, both axes {both_axes}: {} errors, e.g. {:?}",
                errors.len(),
                &errors[..errors.len().min(3)]
            );
            let loose = tightness_errors(full, windowed, (40.0, 352.0), 12.0);
            assert!(
                loose.is_empty(),
                "offset {offset}: {} built outside, e.g. {:?}",
                loose.len(),
                &loose[..loose.len().min(3)]
            );
        }
    }

    /// J08: a windowed UI culls after its first measurement, without
    /// waiting for a scroll. The first build has no heights and builds
    /// everything; nothing triggered a second build, so a UI that was
    /// never scrolled stayed fully built.
    #[test]
    fn a_windowed_ui_culls_without_being_scrolled() {
        let page = bench_rows(600);
        let mut ui = TestUi::with_layout(
            "j08-unscrolled",
            &[("page.html", page.as_str()), ("style.css", BENCH_CSS)],
            UVec2::new(300, 200),
        )
        .stylesheet("style.css")
        .spawn(
            "page.html",
            TemplateContext::new(),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(200.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow {
                    x: OverflowAxis::Visible,
                    y: OverflowAxis::Scroll,
                },
                ..default()
            },
        );
        let root = ui.root();
        ui.world_mut()
            .entity_mut(root)
            .insert((HtmlWindowed::default(), ScrollPosition::default()));
        ui.update(40);
        let (elements, _) = census(&mut ui);
        assert!(elements < 300, "{elements} of 1 800 elements materialized");
        let before = ui.builds();
        ui.update(20);
        assert_eq!(ui.builds() - before, 0, "the culled UI keeps rebuilding");
    }

    /// A windowed UI that isn't laid out (a zero-height root: no layout
    /// yet, or a render-less app) builds everything: an unknown viewport
    /// windows nothing. With J08's rebuild, a zero band culled the whole
    /// document into one spacer.
    #[test]
    fn an_unlaid_windowed_ui_builds_everything() {
        let page = bench_rows(40);
        let files = [("page.html", page.as_str()), ("style.css", BENCH_CSS)];
        let mut ui = TestUi::new("j08-unlaid", &files)
            .stylesheet("style.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        let root = ui.root();
        ui.world_mut()
            .entity_mut(root)
            .insert((HtmlWindowed::default(), ScrollPosition::default()));
        ui.settle();
        ui.update(10);
        let (elements, _) = census(&mut ui);
        assert_eq!(elements, 120, "{}", ui.dump());
    }

    /// J04: a scroll that keeps most of the band reuses it. A scroll-only
    /// build never pairs with the skeleton (it requires a render change or
    /// a restyle), so every scroll frame re-cascades and re-collects the
    /// whole band: 0 of 74 specs reused for a one-row scroll.
    #[test]
    fn scrolling_by_one_row_reuses_the_band() {
        let page = bench_rows(600);
        let mut ui = scrolled_page("j04-scroll-reuse", BENCH_CSS, &page, true, 4000.0);
        let root = ui.root();
        ui.world_mut()
            .entity_mut(root)
            .insert(ScrollPosition(Vec2::new(0.0, 4024.0)));
        ui.update(5);
        let (reused, total) = ui
            .world_mut()
            .get::<Skeleton>(root)
            .and_then(|skeleton| skeleton.last_reuse())
            .expect("a content build ran");
        assert!(reused * 2 >= total, "{reused} of {total} reused");
    }

    /// J04's reuse stays correct: small scrolls in both directions (rows
    /// reuse their specs, the band's edges re-collect), then a content
    /// change while scrolled; after every step the materialized rows sit
    /// where an unwindowed build of the same state puts them.
    #[test]
    fn scrolling_in_steps_matches_the_full_build() {
        let page = format!(
            "<main>{}</main>",
            (1..=400)
                .map(|i| format!("<div class=\"row\" id=\"r{i}\"><p class=\"k\">unit-{i}</p><p class=\"v\">{{% if i == {i} %}}{{{{ v }}}}{{% else %}}{i}{{% endif %}}</p></div>"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let files = [("page.html", page.as_str()), ("style.css", BENCH_CSS)];
        let spawn = |name: &str, windowed: bool| {
            let mut ui = TestUi::with_layout(name, &files, UVec2::new(300, 200))
                .stylesheet("style.css")
                .spawn(
                    "page.html",
                    TemplateContext::new().with("i", &150).with("v", &"short"),
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(200.0),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow {
                            x: OverflowAxis::Visible,
                            y: OverflowAxis::Scroll,
                        },
                        ..default()
                    },
                );
            let root = ui.root();
            if windowed {
                ui.world_mut().entity_mut(root).insert((
                    HtmlWindowed {
                        margin_viewports: 0.25,
                    },
                    ScrollPosition::default(),
                ));
            }
            ui.settle();
            ui
        };
        let mut windowed = spawn("j04-steps-windowed", true);
        let mut full = spawn("j04-steps-full", false);
        let step = |ui: &mut TestUi, offset: f32| {
            let root = ui.root();
            ui.world_mut()
                .entity_mut(root)
                .insert(ScrollPosition(Vec2::new(0.0, offset)));
            ui.update(6);
        };
        for offset in [
            3000.0, 3030.0, 3100.0, 3101.0, 3170.0, 3050.0, 2990.0, 3400.0,
        ] {
            step(&mut windowed, offset);
            step(&mut full, offset);
            let errors = band_errors(&mut full, &mut windowed);
            assert!(
                errors.is_empty(),
                "offset {offset}: {} errors, e.g. {:?}",
                errors.len(),
                &errors[..errors.len().min(3)]
            );
        }
        // A content change while scrolled (a row in the band gets a value):
        // a longer text, then the scroll moves on.
        for ui in [&mut windowed, &mut full] {
            let root = ui.root();
            ui.world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("v", &"a much longer value");
            // Row 155 sits in the band at 3 400 px (22 px per row).
            ui.world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert("i", &155);
            ui.update(8);
        }
        let errors = band_errors(&mut full, &mut windowed);
        assert!(
            errors.is_empty(),
            "after a content change: {} errors, e.g. {:?}",
            errors.len(),
            &errors[..errors.len().min(3)]
        );
        let root = windowed.root();
        let (reused, total) = windowed
            .world_mut()
            .get::<Skeleton>(root)
            .and_then(|skeleton| skeleton.last_reuse())
            .unwrap();
        assert!(reused > 0, "{reused} of {total} reused");
    }

    /// Sections of ten rows swept through the band in small steps: a
    /// section that straddled the band's edge (its cached spec holds a
    /// spacer) and then lies wholly inside must re-collect, not reuse the
    /// half-culled spec.
    #[test]
    fn sections_entering_the_band_rebuild_their_culled_half() {
        let page: String = (0..40)
            .map(|s| {
                let rows: String = (0..10)
                    .map(|r| format!("<p id=\"p{s}-{r}\">row {s}.{r}</p>"))
                    .collect();
                format!("<section class=\"s\">{rows}</section>")
            })
            .collect();
        let css = "html { flex-direction: column; gap: 6px } .s { gap: 2px }";
        let mut windowed = scrolled_page("sections-windowed", css, &page, true, 900.0);
        let mut full = scrolled_page("sections-full", css, &page, false, 900.0);
        for step in 1..=24 {
            let offset = 900.0 + step as f32 * 47.0;
            for ui in [&mut windowed, &mut full] {
                let root = ui.root();
                ui.world_mut()
                    .entity_mut(root)
                    .insert(ScrollPosition(Vec2::new(0.0, offset)));
                ui.update(6);
            }
            let errors = band_errors(&mut full, &mut windowed);
            assert!(
                errors.is_empty(),
                "offset {offset}: {} errors, e.g. {:?}",
                errors.len(),
                &errors[..errors.len().min(3)]
            );
        }
    }

    /// Loose text between elements (anonymous blocks, keyed by their raw
    /// text node) keeps culling across scroll builds: the model keeps its
    /// keys for an unchanged document (remapping through the pairing —
    /// element pairs only — dropped the anonymous blocks' heights, and the
    /// cursor stopped culling at the first one).
    #[test]
    fn loose_text_keeps_culling_across_scrolls() {
        let page: String = (1..=300)
            .map(|i| format!("loose line {i}<p id=\"p{i}\">paragraph {i}</p>"))
            .collect();
        let css = "html { flex-direction: column; gap: 2px }";
        for (step, offset) in [(0, 3000.0), (1, 3050.0), (2, 3200.0)] {
            let mut full = scrolled_page("loose-full", css, &page, false, offset);
            let mut windowed = scrolled_page("loose-windowed", css, &page, true, 3000.0);
            if step > 0 {
                let root = windowed.root();
                windowed
                    .world_mut()
                    .entity_mut(root)
                    .insert(ScrollPosition(Vec2::new(0.0, 3050.0)));
                windowed.update(6);
                windowed
                    .world_mut()
                    .entity_mut(root)
                    .insert(ScrollPosition(Vec2::new(0.0, offset)));
                windowed.update(6);
            }
            let built = rects(&mut windowed).len();
            assert!(built < 100, "offset {offset}: {built} paragraphs built");
            let errors = band_errors(&mut full, &mut windowed);
            assert!(
                errors.is_empty(),
                "offset {offset}: {} errors, e.g. {:?}",
                errors.len(),
                &errors[..errors.len().min(3)]
            );
        }
    }

    /// A taller viewport rebuilds the band (the root's height is part of
    /// the band; before J05 only the width and the offset triggered a
    /// rebuild, so the extra viewport stayed empty).
    #[test]
    fn a_taller_viewport_rebuilds_the_band() {
        let page = bench_rows(600);
        let mut windowed = scrolled_with(
            "j05-taller-windowed",
            BENCH_CSS,
            &page,
            Some(HtmlWindowed {
                margin_viewports: 0.0,
            }),
            4000.0,
        );
        let mut full = scrolled_page("j05-taller-full", BENCH_CSS, &page, false, 4000.0);
        for ui in [&mut windowed, &mut full] {
            let root = ui.root();
            ui.world_mut().get_mut::<Node>(root).unwrap().height = Val::Px(400.0);
        }
        // The camera's target stays 200 px tall: grow it so the window
        // shows the taller root.
        for ui in [&mut windowed, &mut full] {
            let world = ui.world_mut();
            let mut cameras = world.query::<&mut Camera>();
            for mut camera in cameras.iter_mut(world) {
                if let Some(info) = camera.computed.target_info.as_mut() {
                    info.physical_size = UVec2::new(300, 400);
                }
            }
            ui.update(8);
        }
        let errors = band_errors_in(&mut full, &mut windowed, (0.0, 400.0));
        assert!(
            errors.is_empty(),
            "{} errors, e.g. {:?}",
            errors.len(),
            &errors[..errors.len().min(3)]
        );
    }

    /// J05's slack at a tall block: with a 2 000 px block filling one band
    /// edge, the other edge's culled rows set the slack — a scroll that
    /// brings them into view must rebuild even though the tall block
    /// alone would allow a longer move.
    #[test]
    fn the_slack_follows_the_culled_rows_beside_a_tall_block() {
        let rows = |range: std::ops::RangeInclusive<usize>| -> String {
            range
                .map(|i| format!("<p id=\"p{i}\">row {i}</p>"))
                .collect()
        };
        let css = "html { flex-direction: column; gap: 2px } .tall { height: 2000px }";
        let tight = Some(HtmlWindowed {
            margin_viewports: 0.0,
        });
        // Culled below: the band starts inside the tall block; scrolling
        // down 1 800 px brings the rows after it into view.
        let page = format!("<p class=\"tall\" id=\"tall\">tall</p>{}", rows(1..=200));
        let mut windowed = scrolled_with("slack-below-windowed", css, &page, tight, 100.0);
        let mut full = scrolled_page("slack-below-full", css, &page, false, 100.0);
        for ui in [&mut windowed, &mut full] {
            let root = ui.root();
            ui.world_mut()
                .entity_mut(root)
                .insert(ScrollPosition(Vec2::new(0.0, 1900.0)));
            ui.update(8);
        }
        let errors = band_errors(&mut full, &mut windowed);
        assert!(
            errors.is_empty(),
            "below: {} errors, e.g. {:?}",
            errors.len(),
            &errors[..errors.len().min(3)]
        );
        // Culled above: the band sits inside a tall block after 100 rows;
        // scrolling up 200 px brings the last rows into view.
        let page = format!("{}<p class=\"tall\" id=\"tall\">tall</p>", rows(1..=100));
        let start = 100.0 * 22.0 + 100.0;
        let mut windowed = scrolled_with("slack-above-windowed", css, &page, tight, start);
        let mut full = scrolled_page("slack-above-full", css, &page, false, start);
        for ui in [&mut windowed, &mut full] {
            let root = ui.root();
            ui.world_mut()
                .entity_mut(root)
                .insert(ScrollPosition(Vec2::new(0.0, start - 200.0)));
            ui.update(8);
        }
        let errors = band_errors(&mut full, &mut windowed);
        assert!(
            errors.is_empty(),
            "above: {} errors, e.g. {:?}",
            errors.len(),
            &errors[..errors.len().min(3)]
        );
        // Leaving: rows at the band's top with the tall block below them;
        // scrolling down past the rows must cull them (the materialized
        // rows' own slack — the tall block alone allows a long move).
        let start = 100.0 * 22.0 - 100.0;
        let mut windowed = scrolled_with("slack-leave-windowed", css, &page, tight, start);
        let root = windowed.root();
        windowed
            .world_mut()
            .entity_mut(root)
            .insert(ScrollPosition(Vec2::new(0.0, start + 400.0)));
        windowed.update(8);
        let built = rects(&mut windowed)
            .keys()
            .filter(|id| id.starts_with('p'))
            .count();
        assert!(built <= 1, "{built} rows still built above the band");
    }

    /// J05 with a subtree reused whole (`note_reused`): its rows weren't
    /// visited, so the slack keeps the band's top clear of the subtree's
    /// top. The layout makes that the binding constraint: the section is
    /// the first child of a wrapper whose 200 px top padding holds the
    /// band's top edge — no neighbour above it in its column (any element
    /// between the edge and the section would bind first, by one gap),
    /// the wrapper itself straddles the edge (so it re-collects and the
    /// section reuses whole), nothing is culled below (a tall tail), and
    /// the reuse comes from a content change, not a scroll.
    /// Scrolling 150 px down then puts the section's first rows above the
    /// band; only the subtree's slack triggers the rebuild that culls
    /// them (without it they stay built until some later rebuild).
    #[test]
    fn a_reused_section_culls_its_rows_once_they_leave_the_band() {
        let section: String = (1..=10)
            .map(|i| format!("<p id=\"s{i}\">section {i}</p>"))
            .collect();
        let page = format!(
            "<p class=\"tall\">tall</p><div class=\"wrap\"><section class=\"s\">{section}</section><p id=\"end\">{{{{ v }}}}</p><p class=\"tail\">tail</p></div>"
        );
        let css = "html { flex-direction: column; gap: 2px } .wrap { padding-top: 200px; gap: 2px } .s { gap: 2px } .tall { height: 3000px } .tail { height: 1000px }";
        let files = [("page.html", page.as_str()), ("style.css", css)];
        let mut ui = TestUi::with_layout("note-reused", &files, UVec2::new(300, 200))
            .stylesheet("style.css")
            .spawn(
                "page.html",
                TemplateContext::new().with("v", &"first"),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(200.0),
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow {
                        x: OverflowAxis::Visible,
                        y: OverflowAxis::Scroll,
                    },
                    ..default()
                },
            );
        let root = ui.root();
        // The band at 3 300 is [3 100, 3 700): the tall block (0 … 3 000)
        // is culled, the section (3 202 … 3 420) lies wholly inside.
        ui.world_mut().entity_mut(root).insert((
            HtmlWindowed::default(),
            ScrollPosition(Vec2::new(0.0, 3300.0)),
        ));
        ui.settle();
        ui.update(6);
        // A content change after the section: a content build in which
        // the section reuses whole.
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert("v", &"second");
        ui.update(6);
        let (reused, _) = ui
            .world_mut()
            .get::<Skeleton>(root)
            .and_then(|skeleton| skeleton.last_reuse())
            .unwrap();
        assert!(reused > 0, "the section reused whole");
        // 150 px down: the band's top (3 250) passes s1 (3 202 … 3 222)
        // and s2 (3 224 … 3 244).
        ui.world_mut()
            .entity_mut(root)
            .insert(ScrollPosition(Vec2::new(0.0, 3450.0)));
        ui.update(6);
        let built = rects(&mut ui);
        for id in ["s1", "s2"] {
            assert!(
                !built.contains_key(id),
                "{id} is above the band but still built: {:?}",
                built.keys().collect::<Vec<_>>()
            );
        }
        assert!(
            built.contains_key("s3"),
            "s3 straddles the band's top and stays built"
        );
    }

    /// The bottom-edge mirror of the test above: the section is the last
    /// child of a wrapper whose 200 px bottom padding holds the band's
    /// bottom edge; the content change is before it, so it reuses whole.
    /// 150 px up, the band's bottom passes s7–s10, and only the reused
    /// subtree's slack rebuilds to cull them (review_0006's mutation run:
    /// `note_reused`'s bottom term survived).
    #[test]
    fn a_reused_section_culls_its_rows_once_they_leave_the_band_bottom() {
        let section: String = (1..=10)
            .map(|i| format!("<p id=\"s{i}\">section {i}</p>"))
            .collect();
        let page = format!(
            "<p class=\"tall\">tall</p><div class=\"wrap\"><p class=\"head\">head</p><p id=\"end\">{{{{ v }}}}</p><section class=\"s\">{section}</section></div><p class=\"tail\">tail</p>"
        );
        let css = "html { flex-direction: column; gap: 2px } .wrap { padding-bottom: 200px; gap: 2px } .s { gap: 2px } .tall { height: 1000px } .head { height: 1000px } .tail { height: 3000px }";
        let files = [("page.html", page.as_str()), ("style.css", css)];
        let mut ui = TestUi::with_layout("note-reused-bottom", &files, UVec2::new(300, 200))
            .stylesheet("style.css")
            .spawn(
                "page.html",
                TemplateContext::new().with("v", &"first"),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(200.0),
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow {
                        x: OverflowAxis::Visible,
                        y: OverflowAxis::Scroll,
                    },
                    ..default()
                },
            );
        let root = ui.root();
        // The band at 1 900 is [1 700, 2 300): the section (2 026 … 2 244)
        // lies wholly inside, the band's bottom in the wrapper's padding.
        ui.world_mut().entity_mut(root).insert((
            HtmlWindowed::default(),
            ScrollPosition(Vec2::new(0.0, 1900.0)),
        ));
        ui.settle();
        ui.update(6);
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert("v", &"second");
        ui.update(6);
        let (reused, _) = ui
            .world_mut()
            .get::<Skeleton>(root)
            .and_then(|skeleton| skeleton.last_reuse())
            .unwrap();
        assert!(reused > 0, "the section reused whole");
        // 150 px up: the band's bottom (2 150) passes s7 (2 158 …) to s10.
        ui.world_mut()
            .entity_mut(root)
            .insert(ScrollPosition(Vec2::new(0.0, 1750.0)));
        ui.update(6);
        let built = rects(&mut ui);
        for id in ["s7", "s8", "s9", "s10"] {
            assert!(
                !built.contains_key(id),
                "{id} is below the band but still built: {:?}",
                built.keys().collect::<Vec<_>>()
            );
        }
        assert!(
            built.contains_key("s6"),
            "s6 straddles the band's bottom and stays built"
        );
    }

    /// The band's promise: blocks that stay in it keep their entities
    /// across scroll builds and content builds (review_0006's mutation
    /// run: replacing every kept element on every build passed).
    #[test]
    fn rows_that_stay_in_the_band_keep_their_entities() {
        let page: String = (1..=300)
            .map(|i| match i {
                145 => "<p id=\"p145\">{{ v | default(value=\"first\") }}</p>".to_owned(),
                // Boxed rows (a wrapper plus its text node) too.
                _ if i % 10 == 0 => format!("<p id=\"p{i}\" class=\"bx\">row {i}</p>"),
                _ => format!("<p id=\"p{i}\">row {i}</p>"),
            })
            .collect();
        let css =
            "html { flex-direction: column; gap: 2px } .bx { padding: 2px; border-width: 1px }";
        let mut ui = scrolled_page("kept-entities", css, &page, true, 2900.0);
        let entity_of = |ui: &mut TestUi, id: &str| -> Entity {
            let world = ui.world_mut();
            let mut query = world.query::<(Entity, &HtmlElement)>();
            query
                .iter(world)
                .find(|(_, element)| element.id.as_deref() == Some(id))
                .map(|(entity, _)| entity)
                .unwrap()
        };
        let (before, boxed) = (entity_of(&mut ui, "p141"), entity_of(&mut ui, "p140"));
        let root = ui.root();
        // A scroll build (a small step past the slack) and a content build.
        ui.world_mut()
            .entity_mut(root)
            .insert(ScrollPosition(Vec2::new(0.0, 2960.0)));
        ui.update(12);
        assert_eq!(
            (entity_of(&mut ui, "p141"), entity_of(&mut ui, "p140")),
            (before, boxed),
            "kept across a scroll build"
        );
        // The changed row is re-collected (a fresh spec, not the reused
        // skip) and updated in place: it keeps its entity too.
        let changed = entity_of(&mut ui, "p145");
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert("v", &"changed");
        ui.update(12);
        assert_eq!(
            (entity_of(&mut ui, "p141"), entity_of(&mut ui, "p140")),
            (before, boxed),
            "kept across a content build"
        );
        assert_eq!(
            entity_of(&mut ui, "p145"),
            changed,
            "the changed row is updated in place"
        );
    }

    /// J05: a scroll that leaves the band's membership unchanged doesn't
    /// rebuild. The offset is compared exactly, so every sub-pixel step of
    /// a smooth scroll rebuilt the UI.
    #[test]
    fn a_scroll_inside_the_band_does_not_rebuild() {
        let page = bench_rows(600);
        let mut ui = scrolled_page("j05-subpixel", BENCH_CSS, &page, true, 4000.0);
        let root = ui.root();
        let before = ui.builds();
        ui.world_mut()
            .entity_mut(root)
            .insert(ScrollPosition(Vec2::new(0.0, 4000.5)));
        ui.update(5);
        assert_eq!(
            ui.builds() - before,
            0,
            "a half-pixel scroll rebuilt the UI"
        );
    }

    /// bug_0151's second symptom: hovering a materialized row (a scoped
    /// restyle) must not move or thin the band.
    #[test]
    fn a_hover_restyle_keeps_the_band() {
        let page = bench_rows(600);
        let offset = 6000.0;
        let mut full = scrolled_page("bug0151-hover-full", BENCH_CSS, &page, false, offset);
        let mut windowed = scrolled_page("bug0151-hover-windowed", BENCH_CSS, &page, true, offset);
        let root = windowed.root();
        let target = {
            let world = windowed.world_mut();
            let mut elements = world.query::<(Entity, &HtmlElement)>();
            let visible: Vec<Entity> = elements
                .iter(world)
                .filter(|(_, element)| element.classes.iter().any(|class| class == "row"))
                .map(|(entity, _)| entity)
                .collect();
            visible[visible.len() / 2]
        };
        windowed.world_mut().entity_mut(target).insert(PseudoState {
            hovered: true,
            ..default()
        });
        windowed.update(10);
        let _ = root;
        let errors = band_errors(&mut full, &mut windowed);
        assert!(
            errors.is_empty(),
            "{} errors, e.g. {:?}",
            errors.len(),
            &errors[..errors.len().min(3)]
        );
    }
}
