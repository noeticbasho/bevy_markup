//! Lint tests over the real content in `examples/assets/`: the bugs content authors
//! hit (a locale missing a message, a hard-coded English string, a template
//! that fails to render, a stylesheet pointing at a moved image).
//!
//! The checkers themselves are also unit-tested on inline inputs below.
//!
//! The checks:
//! 1. FTL: each locale of a bundle family defines the same messages (value +
//!    attributes) as its `en-US`; extra messages are allowed only as helpers
//!    referenced from inside that bundle (`item-name`). Parse errors,
//!    duplicate ids, and manifests naming another locale are reported too.
//! 2. Templates compile (as the loader does) and render through the real
//!    pipeline with the examples' context and stylesheets; every
//!    `data-l10n-id` resolves (no `l10n error`) in every locale.
//! 3. Localized templates hold no hard-coded text outside `data-l10n-id`
//!    elements (see [`needs_translation`] for the rule).
//! 4. Every CSS file parses; every `url()` resolves to a file in `examples/assets/`.
//! 5. A pseudo-locale generated from `en-US` (accented, bracketed, ~30%
//!    longer) leaves no untranslated text run in the rendered UI.

mod common;
use common::TestUi;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use bevy::prelude::*;
use bevy_markup::l10n::LocalizedText;
use bevy_markup::prelude::*;
use bevy_markup::tl;
use fluent_syntax::ast;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Content inventory
// ---------------------------------------------------------------------------

/// The examples' assets, which this file lints (committed with the repo).
fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/assets")
}

/// Top-level `examples/assets/` directories the pages never load, so they aren't
/// copied into the test asset roots.
const NOT_CONTENT: &[&str] = &[
    // Artwork sources (.kra) for the UI images.
    "src",
];

/// One template as an example shows it.
struct Page {
    /// Asset path.
    template: &'static str,
    /// The Tera context(s) the example passes, as JSON objects.
    contexts: fn() -> Vec<Value>,
    /// Stylesheets the example shows it with.
    stylesheets: &'static [&'static str],
    /// Bundle family (the directory holding `<locale>/main.ftl.ron`).
    locales: &'static str,
    /// `Some(reason)`: deliberately not localized, exempt from checks 3 and 5.
    unlocalized: Option<&'static str>,
}

const DEMO_THEMES: &[&str] = &[
    "ui/themes/crimson.css",
    "ui/themes/parchment.css",
    "ui/themes/terminal.css",
    "ui/themes/large_print.css",
];

/// `examples/demo/shell.rs` `demo_context()`.
fn demo_contexts() -> Vec<Value> {
    vec![json!({
        "player": "ada <the brave>",
        "hp": 7,
        "max_hp": 10,
        "items": [
            { "name": "torch", "count": 3 },
            { "name": "rope", "count": 1 },
            { "name": "key", "count": 0 },
        ],
    })]
}

/// `examples/demo/shell.rs` `shell_context()`: the selector rows' labels and
/// active options, in both variants so every `active` branch renders. The
/// labels are data (native names, not translations).
fn shell_contexts() -> Vec<Value> {
    let base = json!({
        "langs": ["English", "Русский", "Deutsch", "日本語"],
        "lang_active": 0,
        "themes": ["Crimson", "Parchment", "Terminal", "Large print"],
        "theme_active": 0,
        "scrollbars_on": true,
        "outline_slots": [
            "slot-outline-plain",
            "slot-outline-template",
            "slot-outline-l10n",
        ],
        // `update_scrollbars`: every area starts hidden; the second context
        // shows some (one scrolled).
        "scrollbars": {
            "slot-inventory": { "show": false, "top": 0.0, "height": 0.0, "dragging": false },
            "side-column": { "show": false, "top": 0.0, "height": 0.0, "dragging": false },
            "slot-outline-plain": { "show": false, "top": 0.0, "height": 0.0, "dragging": false },
            "slot-outline-template": { "show": false, "top": 0.0, "height": 0.0, "dragging": false },
            "slot-outline-l10n": { "show": false, "top": 0.0, "height": 0.0, "dragging": false },
        },
    });
    let mut other = base.clone();
    other["lang_active"] = json!(2);
    other["theme_active"] = json!(1);
    other["scrollbars_on"] = json!(false);
    other["scrollbars"]["slot-inventory"] =
        json!({ "show": true, "top": 40.5, "height": 33.3, "dragging": true });
    other["scrollbars"]["slot-outline-l10n"] =
        json!({ "show": true, "top": 0.0, "height": 17.6, "dragging": false });
    vec![base, other]
}

/// `examples/quickstart/main.rs`: starts at 0 coins, each click adds one (so the
/// zero, one and other plural forms all show).
fn quickstart_contexts() -> Vec<Value> {
    (0..3)
        .map(|coins| json!({ "player": "Ada", "coins": coins }))
        .collect()
}

/// `examples/inline/main.rs`: the quests' done-flags (some open, one done).
fn inline_contexts() -> Vec<Value> {
    vec![
        json!({ "player": "Ada", "day": 7, "quests": [true, false, false] }),
        json!({ "player": "Ada", "day": 7, "quests": [true, true, true] }),
    ]
}

/// `examples/inputs/main.rs`: the fields' initial values and the preview
/// following them (empty, half typed, both set).
fn inputs_contexts() -> Vec<Value> {
    // One context carries a quote and a backslash in the preview's
    // `data-l10n-args` (the spliced-JSON bug).
    vec![
        json!({
            "name": "Ada", "title": "Cartographer",
            "live_name": "Ada", "live_title": "Cartographer",
        }),
        json!({
            "name": "Ada", "title": "Cartographer",
            "live_name": "", "live_title": "",
        }),
        json!({
            "name": "Ada", "title": "Cartographer",
            "live_name": "Ada \"The\" \\ Brave", "live_title": "",
        }),
    ]
}

/// `examples/links/main.rs`: the status line before and after opening.
fn links_contexts() -> Vec<Value> {
    // "never" selects the not-yet branch of the status selector; any other
    // value is the last opened href.
    vec![
        json!({ "opened": "never" }),
        json!({ "opened": "atlas://north-shelf" }),
    ]
}

/// `examples/forms/main.rs`: the form (no submission) and the receipt panel
/// (with submitted values).
fn forms_contexts() -> Vec<Value> {
    vec![
        json!({ "name": "Ada" }),
        json!({
            "name": "Ada",
            "submitted": { "name": "Ada \"The\" \\ Brave", "role": "Cartographer" },
        }),
    ]
}

/// `examples/testbench/main.rs` `show_shell`: every scenario's check ids
/// in each state, both stage layouts, churn on and off.
fn testbench_contexts() -> Vec<Value> {
    let groups = json!([
        { "id": "perf", "scenarios": ["big-document", "all-dirty", "restyle-storm", "markdown", "localized", "many-uis", "calc-layout"] },
        { "id": "security", "scenarios": ["nested-rules", "deep-dom", "var-bombs", "grid-bombs", "hostile-args", "args-injection", "remote-sources", "markdown-bombs"] },
        { "id": "correctness", "scenarios": ["reuse-vs-fresh", "indent-vs-flat"] },
    ]);
    let stats = json!({ "avg_ms": "16.7", "max_ms": "33.2", "fps": "60", "builds": 60, "restyles": 0, "uis": 1, "elements": 2040 });
    let checks = |state: &str| {
        json!(
            [
                "budget",
                "built",
                "survived",
                "sheet-refused",
                "no-forged",
                "no-fallback",
                "map-literal",
                "include-refused",
                "border-skipped",
                "twins",
            ]
            .iter()
            .map(|id| json!({ "id": id, "state": state, "detail": "(12 / 0)" }))
            .collect::<Vec<_>>()
        )
    };
    ["big-document", "nested-rules", "indent-vs-flat"]
        .iter()
        .zip(["wait", "pass", "fail"])
        .zip([false, false, true])
        .map(|((active, state), row)| {
            json!({
                "groups": groups, "active": active, "size": 500, "churn": !row, "row": row,
                "stats": stats, "checks": checks(state),
            })
        })
        .collect()
}

/// `examples/calc/main.rs`: the steppers at their defaults, their
/// minimum and their maximum (the calc() formulas change with them).
fn calc_contexts() -> Vec<Value> {
    vec![
        json!({
            "settings": { "sidebar": 160, "cards": 3, "indent": 8 },
        }),
        json!({
            "settings": { "sidebar": 80, "cards": 2, "indent": 0 },
        }),
        json!({
            "settings": { "sidebar": 280, "cards": 5, "indent": 48 },
        }),
    ]
}

/// `examples/grid-names/main.rs`: the board's picks (nothing checked,
/// everything checked) and the supply list.
fn grid_names_contexts() -> Vec<Value> {
    vec![
        json!({
            "player": "Ada", "hp": 7, "max_hp": 10, "coins": 12,
            "picks": [false, false, false, false, false, false, false],
            "items": [
                { "name": "torch", "count": 3, "checked": false },
                { "name": "rope", "count": 1, "checked": false },
                { "name": "key", "count": 0, "checked": false },
            ],
        }),
        json!({
            "player": "Ada", "hp": 7, "max_hp": 10, "coins": 12,
            "picks": [true, true, true, true, true, true, true],
            "items": [
                { "name": "torch", "count": 3, "checked": true },
                { "name": "rope", "count": 1, "checked": true },
                { "name": "key", "count": 0, "checked": true },
            ],
        }),
    ]
}

/// `examples/combinators/main.rs`: each group selected in turn.
fn combinators_contexts() -> Vec<Value> {
    [0, 1, 2]
        .iter()
        .map(|selected| {
            json!({
                "groups": GROUPS.iter().enumerate().map(|(index, label)| json!({
                    "label": label,
                    "selected": index == *selected,
                })).collect::<Vec<_>>(),
                "selected_label": GROUPS[*selected],
            })
        })
        .collect()
}

/// The combinators example's group names (also in `main.rs`).
const GROUPS: [&str; 3] = ["north", "middle", "south"];

/// `examples/markdown/main.rs`: both reader pages.
fn markdown_contexts() -> Vec<Value> {
    vec![
        json!({ "active_tab": "release" }),
        json!({ "active_tab": "tutorial" }),
    ]
}

/// `examples/grid/main.rs`: both page layouts (the status message selects on
/// `layout`); items as in `ITEMS`.
fn grid_contexts() -> Vec<Value> {
    let items = json!([
        { "id": "sword", "count": 1, "featured": false },
        { "id": "map", "count": 1, "featured": true },
        { "id": "shield", "count": 1, "featured": false },
        { "id": "potion", "count": 5, "featured": false },
        { "id": "herb", "count": 12, "featured": false },
        { "id": "lantern", "count": 1, "featured": false },
        { "id": "rope", "count": 2, "featured": false },
        { "id": "gem", "count": 3, "featured": false },
        { "id": "key", "count": 1, "featured": false },
    ]);
    // `examples/grid/main.rs` `GEAR` / `QUESTS`.
    let gear = json!([
        { "slot": "neck", "item": "amulet" },
        { "slot": "head", "item": "helm" },
        { "slot": "back", "item": "" },
        { "slot": "main-hand", "item": "sword" },
        { "slot": "off-hand", "item": "shield" },
        { "slot": "ring", "item": "signet" },
        { "slot": "trinket", "item": "" },
        { "slot": "feet", "item": "boots" },
    ]);
    let quests = json!([
        { "id": "descend", "status": "main", "progress": 35, "reward": 500 },
        { "id": "torches", "status": "active", "progress": 60, "reward": 50 },
        { "id": "map", "status": "active", "progress": 20, "reward": 80 },
        { "id": "key", "status": "active", "progress": 0, "reward": 120 },
        { "id": "rope", "status": "done", "progress": 100, "reward": 30 },
        { "id": "rats", "status": "done", "progress": 100, "reward": 25 },
        { "id": "bridge", "status": "failed", "progress": 40, "reward": 0 },
    ]);
    // Every screen with a selection on it (the gear one empty, the quests
    // one failed: their own detail lines), in both page layouts.
    [
        ("wide", "items", "slot-map"),
        ("narrow", "items", "slot-herb"),
        ("wide", "gear", "gear-back"),
        ("narrow", "gear", "gear-main-hand"),
        ("wide", "quests", "quest-bridge"),
        ("narrow", "quests", "quest-rope"),
    ]
    .map(|(layout, tab, selected)| {
        json!({ "layout": layout, "items": items, "weight": "18.5 kg", "gold": 240,
                    "gear": gear, "quests": quests, "active_tab": tab, "selected": selected })
    })
    .to_vec()
}

/// `examples/menu/main.rs` `show_settings()`: every volume step's ends, every
/// difficulty and every `input_name()` (the Fluent selectors' branches).
fn menu_contexts() -> Vec<Value> {
    let inputs = [
        "none",
        "mouse-primary",
        "mouse-secondary",
        "mouse-middle",
        "touch",
        "key",
        "gamepad",
        "synthetic",
    ];
    inputs
        .iter()
        .zip([(0, "easy"), (50, "normal"), (100, "hard")].iter().cycle())
        .map(|(input, (volume, difficulty))| {
            json!({ "volume": volume, "difficulty": difficulty, "last_input": input })
        })
        .collect()
}

/// `examples/menu/main.rs`: the dialog takes no variables.
fn dialog_contexts() -> Vec<Value> {
    vec![json!({})]
}

/// `HtmlTooltips` in `examples/menu/main.rs`: one tooltip per button's
/// `data-tooltip` key (`args` empty, default placement).
fn menu_tooltip_contexts() -> Vec<Value> {
    ["menu-volume-tip", "menu-difficulty-tip", "menu-reset-tip"]
        .map(|key| json!({ "key": key, "args": {}, "placement": "right" }))
        .to_vec()
}

/// `examples/world/main.rs` `plate_context()`: every faction, epithet, health
/// class and `world-status` hits branch (0, one, other); fading and hidden.
fn plate_contexts() -> Vec<Value> {
    let plate = |name: &str,
                 faction: &str,
                 level: u32,
                 epithet: &str,
                 hp: u32,
                 hits: u32,
                 health: &str,
                 alpha: f32,
                 hidden: bool| {
        json!({ "name": name, "faction": faction, "level": level, "epithet": epithet, "hp": hp,
                "hits": hits, "health": health, "alpha": alpha, "hidden": hidden, "z": 900 })
    };
    vec![
        plate("Ada", "ally", 12, "brave", 100, 0, "good", 1.0, false),
        plate("Bo", "neutral", 3, "quiet", 65, 1, "good", 0.4, false),
        plate("Cy", "hostile", 7, "grim", 30, 2, "low", 1.0, false),
        plate("Dee", "ally", 9, "swift", 60, 1, "hurt", 1.0, false),
        plate("Eli", "hostile", 15, "lost", 100, 0, "good", 0.0, true),
        plate("Fay", "neutral", 5, "wise", 100, 0, "good", 1.0, false),
    ]
}

/// `examples/world/main.rs` `show_hud()`.
fn world_hud_contexts() -> Vec<Value> {
    vec![
        json!({ "on_screen": 0, "total": 0 }),
        json!({ "on_screen": 4, "total": 6 }),
    ]
}

/// `examples/live/main.rs` `show_party()`: a mixed party (a low, fading member)
/// and the empty one.
fn party_contexts() -> Vec<Value> {
    vec![
        json!({ "units": [
            { "name": "Ada", "hp": 87.0, "low": false, "opacity": 1.0 },
            { "name": "Bo", "hp": 12.0, "low": true, "opacity": 0.45 },
        ] }),
        json!({ "units": [] }),
    ]
}

/// `examples/live/main.rs` `show_stats()`.
fn stats_contexts() -> Vec<Value> {
    vec![json!({ "updates": 12, "spawned": 5, "frames": 600 })]
}

/// `examples/hot-reload/main.rs`: the palette the app last wrote (0–2; the
/// contexts render every palette name the Fluent select produces).
fn hot_reload_contexts() -> Vec<Value> {
    (0..3)
        .map(|palette| json!({ "player": "Ada", "coins": 2, "palette": palette }))
        .collect()
}

/// Every template under `assets/` (`every_template_is_listed` keeps this
/// complete).
const PAGES: &[Page] = &[
    Page {
        template: "ui/content/test.html",
        contexts: demo_contexts,
        stylesheets: DEMO_THEMES,
        locales: "locales",
        unlocalized: Some("plain-HTML sample, shown only as a DOM outline (debug panel)"),
    },
    Page {
        template: "ui/content/inventory.html",
        contexts: demo_contexts,
        stylesheets: DEMO_THEMES,
        locales: "locales",
        unlocalized: Some("Tera-only sample, shown only as a DOM outline (debug panel)"),
    },
    Page {
        template: "ui/content/l10n.html",
        contexts: demo_contexts,
        stylesheets: DEMO_THEMES,
        locales: "locales",
        unlocalized: None,
    },
    Page {
        template: "ui/content/shell.html",
        contexts: shell_contexts,
        stylesheets: DEMO_THEMES,
        locales: "locales",
        unlocalized: None,
    },
    Page {
        template: "quickstart/hello.html",
        contexts: quickstart_contexts,
        stylesheets: &["quickstart/style.css"],
        locales: "quickstart/locales",
        unlocalized: None,
    },
    Page {
        template: "hot-reload/page.html",
        contexts: hot_reload_contexts,
        stylesheets: &["hot-reload/style.css"],
        locales: "hot-reload/locales",
        unlocalized: None,
    },
    Page {
        template: "inline/page.html",
        contexts: inline_contexts,
        stylesheets: &["inline/style.css"],
        locales: "inline/locales",
        unlocalized: None,
    },
    Page {
        template: "inputs/profile.html",
        contexts: inputs_contexts,
        stylesheets: &["inputs/style.css"],
        locales: "inputs/locales",
        unlocalized: None,
    },
    Page {
        template: "links/report.html",
        contexts: links_contexts,
        stylesheets: &["links/style.css"],
        locales: "links/locales",
        unlocalized: None,
    },
    Page {
        template: "forms/signup.html",
        contexts: forms_contexts,
        stylesheets: &["forms/style.css"],
        locales: "forms/locales",
        unlocalized: None,
    },
    Page {
        template: "grid-names/board.html",
        contexts: grid_names_contexts,
        stylesheets: &["grid-names/style.css"],
        locales: "grid-names/locales",
        unlocalized: None,
    },
    Page {
        template: "testbench/shell.html",
        contexts: testbench_contexts,
        stylesheets: &["testbench/style.css"],
        locales: "testbench/locales",
        unlocalized: None,
    },
    Page {
        template: "calc/page.html",
        contexts: calc_contexts,
        stylesheets: &["calc/style.css"],
        locales: "calc/locales",
        unlocalized: None,
    },
    Page {
        template: "combinators/panel.html",
        contexts: combinators_contexts,
        stylesheets: &["combinators/style.css"],
        locales: "combinators/locales",
        unlocalized: None,
    },
    Page {
        template: "inline/notes.md",
        contexts: inline_contexts,
        stylesheets: &["inline/style.css"],
        locales: "inline/locales",
        unlocalized: None,
    },
    Page {
        template: "markdown/reader.md",
        contexts: markdown_contexts,
        stylesheets: &["markdown/style.css"],
        locales: "markdown/locales",
        unlocalized: None,
    },
    Page {
        template: "markdown/release.md",
        contexts: markdown_contexts,
        stylesheets: &["markdown/style.css"],
        locales: "markdown/locales",
        unlocalized: None,
    },
    Page {
        template: "markdown/tutorial.md",
        contexts: markdown_contexts,
        stylesheets: &["markdown/style.css"],
        locales: "markdown/locales",
        unlocalized: None,
    },
    Page {
        template: "grid/grid.html",
        contexts: grid_contexts,
        stylesheets: &["grid/style.css"],
        locales: "grid/locales",
        unlocalized: None,
    },
    Page {
        template: "menu/menu.html",
        contexts: menu_contexts,
        stylesheets: &["menu/style.css"],
        locales: "menu/locales",
        unlocalized: None,
    },
    Page {
        template: "menu/dialog.html",
        contexts: dialog_contexts,
        stylesheets: &["menu/style.css"],
        locales: "menu/locales",
        unlocalized: None,
    },
    Page {
        template: "menu/tooltip.html",
        contexts: menu_tooltip_contexts,
        stylesheets: &["menu/style.css"],
        locales: "menu/locales",
        unlocalized: None,
    },
    Page {
        template: "world/plate.html",
        contexts: plate_contexts,
        stylesheets: &["world/style.css"],
        locales: "world/locales",
        unlocalized: None,
    },
    Page {
        template: "world/hud.html",
        contexts: world_hud_contexts,
        stylesheets: &["world/style.css"],
        locales: "world/locales",
        unlocalized: None,
    },
    Page {
        template: "live/party.html",
        contexts: party_contexts,
        stylesheets: &["live/style.css"],
        locales: "live/locales",
        unlocalized: None,
    },
    Page {
        template: "live/stats.html",
        contexts: stats_contexts,
        stylesheets: &["live/style.css"],
        locales: "live/locales",
        unlocalized: None,
    },
];

const REFERENCE_LOCALE: &str = "en-US";

/// Files under `dir` (recursive), sorted.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// `path` relative to `assets`, `/`-separated (an asset path).
fn asset_path(assets: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(assets).unwrap();
    let parts: Vec<&str> = relative.iter().map(|part| part.to_str().unwrap()).collect();
    parts.join("/")
}

/// Content files under `assets/` with extension `ext`, as asset paths.
fn content_files(assets: &Path, ext: &str) -> Vec<String> {
    walk(assets)
        .iter()
        .map(|path| asset_path(assets, path))
        .filter(|path| !NOT_CONTENT.contains(&path.split('/').next().unwrap()))
        .filter(|path| path.ends_with(&format!(".{ext}")))
        .collect()
}

/// A headless app whose asset root is a copy of the content.
fn content_ui(assets: &Path) -> TestUi {
    let mut ui = TestUi::new("content-lint", &[]);
    for path in walk(assets) {
        let path = asset_path(assets, &path);
        if !NOT_CONTENT.contains(&path.split('/').next().unwrap()) {
            ui = ui.with_file(&path, &std::fs::read(assets.join(&path)).unwrap());
        }
    }
    ui
}

fn template_context(value: &Value) -> TemplateContext {
    value
        .as_object()
        .expect("contexts are JSON objects")
        .iter()
        .fold(TemplateContext::new(), |context, (key, value)| {
            context.with(key.clone(), value)
        })
}

/// The string leaves of `values`: data interpolated into the page.
fn data_strings(values: &[Value]) -> Vec<String> {
    fn collect(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(string) => out.push(string.clone()),
            Value::Array(items) => items.iter().for_each(|item| collect(item, out)),
            Value::Object(map) => map.values().for_each(|item| collect(item, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    values.iter().for_each(|value| collect(value, &mut out));
    out
}

/// Renders `page` with `context`, `stylesheet` and the bundle at `bundle`
/// (asset path), plus `extra` files in the asset root. A template that
/// doesn't compile fails its load and would never render, and a stylesheet
/// with a dangling `url()` never finishes loading its dependencies; neither
/// would settle, so both are checked first and reported instead.
fn render(
    assets: &Path,
    page: &Page,
    context: &Value,
    stylesheet: &str,
    bundle: &str,
    extra: &[(String, String)],
) -> Result<TestUi, String> {
    compile(assets, page.template).map_err(|err| format!("doesn't compile: {err}"))?;
    let css = std::fs::read_to_string(assets.join(stylesheet))
        .map_err(|err| format!("{stylesheet}: {err}"))?;
    let css_problems = stylesheet_problems(assets, stylesheet, &css);
    if !css_problems.is_empty() {
        return Err(css_problems.join("; "));
    }
    let mut ui = content_ui(assets);
    for (path, contents) in extra {
        ui = ui.with_file(path, contents.as_bytes());
    }
    let mut ui = ui.stylesheet(stylesheet).locale(bundle).spawn(
        page.template,
        template_context(context),
        Node::default(),
    );
    ui.settle();
    Ok(ui)
}

fn bundle_path(page: &Page, locale: &str) -> String {
    format!("{}/{locale}/main.ftl.ron", page.locales)
}

/// Every text run (`Text` and `TextSpan`) under the `HtmlUi`, in order.
fn text_runs(ui: &mut TestUi) -> Vec<String> {
    let root = ui.root();
    let world = ui.world_mut();
    let mut runs = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        let entity = world.entity(entity);
        if let Some(text) = entity.get::<Text>()
            && !text.0.is_empty()
        {
            runs.push(text.0.clone());
        }
        if let Some(span) = entity.get::<TextSpan>() {
            runs.push(span.0.clone());
        }
        if let Some(children) = entity.get::<Children>() {
            stack.extend(children.iter().rev());
        }
    }
    runs
}

fn assert_no_problems(what: &str, problems: &[String]) {
    assert!(problems.is_empty(), "{what}:\n  {}", problems.join("\n  "));
}

fn error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(err) = source {
        message.push_str(&format!(": {err}"));
        source = err.source();
    }
    message
}

#[test]
fn every_template_is_listed() {
    let assets = assets();
    let on_disk: BTreeSet<String> = ["html", "htm", "md", "markdown"]
        .into_iter()
        .flat_map(|ext| content_files(&assets, ext))
        .collect();
    // Component libraries are rendered as part of the pages including them.
    let libraries: BTreeSet<String> = PAGES
        .iter()
        .flat_map(|page| template_set(&assets, page.template).unwrap_or_default())
        .map(|(name, _)| name)
        .filter(|name| !PAGES.iter().any(|page| page.template == name))
        .collect();
    let on_disk: BTreeSet<String> = on_disk.difference(&libraries).cloned().collect();
    let listed: BTreeSet<String> = PAGES.iter().map(|page| page.template.to_owned()).collect();
    let mut problems: Vec<String> = on_disk
        .difference(&listed)
        .map(|path| format!("{path}: not in PAGES (add it with the example's context)"))
        .collect();
    problems.extend(
        listed
            .difference(&on_disk)
            .map(|path| format!("{path}: listed, not on disk")),
    );
    assert_no_problems("templates vs PAGES", &problems);
}

// ---------------------------------------------------------------------------
// 1. Every locale defines the en-US messages
// ---------------------------------------------------------------------------

/// What a locale must match per message: value presence and attribute names.
#[derive(Debug, PartialEq)]
struct Shape {
    value: bool,
    attributes: BTreeSet<String>,
}

impl std::fmt::Display for Shape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.value { "value" } else { "no value" })?;
        for attribute in &self.attributes {
            write!(f, " .{attribute}")?;
        }
        Ok(())
    }
}

/// The messages of one locale's bundle (all its resources).
#[derive(Default)]
struct Catalog {
    messages: BTreeMap<String, Shape>,
    /// Messages referenced from inside the bundle (`{ other-message }`).
    references: BTreeSet<String>,
    /// Parse errors and duplicate ids.
    problems: Vec<String>,
}

/// Parses `files` (`(name, FTL source)`) into one catalog.
fn catalog(files: &[(String, String)]) -> Catalog {
    let mut catalog = Catalog::default();
    for (name, source) in files {
        let resource = match fluent_syntax::parser::parse(source.as_str()) {
            Ok(resource) => resource,
            Err((resource, errors)) => {
                for error in errors {
                    let line = source
                        .get(..error.pos.start)
                        .map_or(0, |s| s.lines().count());
                    catalog.problems.push(format!("{name}:{line}: {error}"));
                }
                resource
            }
        };
        for entry in &resource.body {
            match entry {
                ast::Entry::Message(message) => {
                    let id = message.id.name.to_owned();
                    let shape = Shape {
                        value: message.value.is_some(),
                        attributes: message
                            .attributes
                            .iter()
                            .map(|attribute| attribute.id.name.to_owned())
                            .collect(),
                    };
                    if catalog.messages.insert(id.clone(), shape).is_some() {
                        catalog
                            .problems
                            .push(format!("{name}: `{id}` defined twice"));
                    }
                    message
                        .value
                        .iter()
                        .for_each(|value| pattern_refs(value, &mut catalog.references));
                    for attribute in &message.attributes {
                        pattern_refs(&attribute.value, &mut catalog.references);
                    }
                }
                ast::Entry::Term(term) => {
                    pattern_refs(&term.value, &mut catalog.references);
                    for attribute in &term.attributes {
                        pattern_refs(&attribute.value, &mut catalog.references);
                    }
                }
                _ => {}
            }
        }
    }
    catalog
}

fn pattern_refs(pattern: &ast::Pattern<&str>, out: &mut BTreeSet<String>) {
    for element in &pattern.elements {
        if let ast::PatternElement::Placeable { expression } = element {
            expression_refs(expression, out);
        }
    }
}

fn expression_refs(expression: &ast::Expression<&str>, out: &mut BTreeSet<String>) {
    match expression {
        ast::Expression::Select { selector, variants } => {
            inline_refs(selector, out);
            for variant in variants {
                pattern_refs(&variant.value, out);
            }
        }
        ast::Expression::Inline(inline) => inline_refs(inline, out),
    }
}

fn inline_refs(inline: &ast::InlineExpression<&str>, out: &mut BTreeSet<String>) {
    match inline {
        ast::InlineExpression::MessageReference { id, .. } => {
            out.insert(id.name.to_owned());
        }
        ast::InlineExpression::FunctionReference { arguments, .. } => {
            for argument in &arguments.positional {
                inline_refs(argument, out);
            }
            for argument in &arguments.named {
                inline_refs(&argument.value, out);
            }
        }
        ast::InlineExpression::Placeable { expression } => expression_refs(expression, out),
        _ => {}
    }
}

/// How `locale` differs from `reference` (the en-US catalog). Terms are
/// locale-private by Fluent's design and not compared. A message missing
/// from en-US is fine only as a helper the locale references itself (like
/// the demo's `item-name`, which maps English item ids); anything else extra
/// is a stale or misspelled id.
fn locale_problems(reference: &Catalog, locale: &Catalog) -> Vec<String> {
    let mut problems = locale.problems.clone();
    for (id, shape) in &reference.messages {
        match locale.messages.get(id) {
            None => problems.push(format!("missing `{id}`")),
            Some(own) if own != shape => {
                problems.push(format!("`{id}`: {own} ({REFERENCE_LOCALE}: {shape})"))
            }
            Some(_) => {}
        }
    }
    for id in locale.messages.keys() {
        if !reference.messages.contains_key(id) && !locale.references.contains(id) {
            problems.push(format!(
                "extra `{id}` (not in {REFERENCE_LOCALE}, not referenced in this bundle)"
            ));
        }
    }
    problems
}

/// `main.ftl.ron` as the bundle loader reads it.
#[derive(serde::Deserialize)]
struct Manifest {
    locale: String,
    resources: Vec<String>,
}

/// A locale's resources (`(asset path, source)`) per its `main.ftl.ron`, and
/// problems reading them.
fn read_bundle(assets: &Path, locale_dir: &Path) -> (Vec<(String, String)>, Vec<String>) {
    let manifest_path = locale_dir.join("main.ftl.ron");
    let name = asset_path(assets, &manifest_path);
    let manifest: Manifest = match ron::from_str(&std::fs::read_to_string(&manifest_path).unwrap())
    {
        Ok(manifest) => manifest,
        Err(err) => return (Vec::new(), vec![format!("{name}: {err}")]),
    };
    let mut problems = Vec::new();
    let dir_locale = locale_dir.file_name().unwrap().to_str().unwrap();
    if manifest.locale != dir_locale {
        problems.push(format!(
            "{name}: locale `{}` in the `{dir_locale}` directory",
            manifest.locale
        ));
    }
    let mut files = Vec::new();
    for resource in &manifest.resources {
        let path = locale_dir.join(resource);
        match std::fs::read_to_string(&path) {
            Ok(source) => files.push((asset_path(assets, &path), source)),
            Err(err) => problems.push(format!("{name}: resource `{resource}`: {err}")),
        }
    }
    (files, problems)
}

/// Bundle families: directories whose subdirectories hold `main.ftl.ron`,
/// mapped to those locale directories.
fn families(assets: &Path) -> BTreeMap<PathBuf, Vec<PathBuf>> {
    let mut families: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for path in walk(assets) {
        if path.file_name().is_some_and(|name| name == "main.ftl.ron") {
            let locale_dir = path.parent().unwrap().to_owned();
            let family = locale_dir.parent().unwrap().to_owned();
            families.entry(family).or_default().push(locale_dir);
        }
    }
    families
}

#[test]
fn every_locale_defines_the_en_us_messages() {
    let assets = assets();
    let families = families(&assets);
    assert!(!families.is_empty(), "no bundle families under assets/");
    let mut problems = Vec::new();
    for (family, locale_dirs) in &families {
        let family_name = asset_path(&assets, family);
        let Some(reference_dir) = locale_dirs
            .iter()
            .find(|dir| dir.ends_with(REFERENCE_LOCALE))
        else {
            problems.push(format!("{family_name}: no {REFERENCE_LOCALE} bundle"));
            continue;
        };
        let (files, read_problems) = read_bundle(&assets, reference_dir);
        let reference = catalog(&files);
        let prefix = format!("{family_name}/{REFERENCE_LOCALE}");
        problems.extend(
            read_problems
                .iter()
                .chain(&reference.problems)
                .map(|p| format!("{prefix}: {p}")),
        );
        for locale_dir in locale_dirs.iter().filter(|dir| *dir != reference_dir) {
            let (files, read_problems) = read_bundle(&assets, locale_dir);
            let prefix = asset_path(&assets, locale_dir);
            let found = read_problems
                .into_iter()
                .chain(locale_problems(&reference, &catalog(&files)));
            problems.extend(found.map(|p| format!("{prefix}: {p}")));
        }
    }
    assert_no_problems("locale bundles differ from en-US", &problems);
}

fn ftl(name: &str, source: &str) -> (String, String) {
    (name.to_owned(), source.to_owned())
}

#[test]
fn locale_check_reports_missing_extra_and_mismatched_messages() {
    let reference = catalog(&[ftl(
        "en.ftl",
        "a = A\n\
         b = B\n    .title = T\n\
         c = { -term }\n\
         d = D\n\
         only-attrs =\n    .label = L\n\
         -term = Term\n",
    )]);
    let locale = catalog(&[
        ftl(
            "one.ftl",
            // `helper` is referenced (from a select variant), `stale` isn't;
            // terms (`-own`) are private to a locale.
            "b = B\n    .label = L\n\
             c = { $n ->\n        [one] { helper }\n       *[other] { -own }\n    }\n\
             helper = H\n\
             stale = S\n\
             only-attrs = V\n    .label = L\n\
             -own = Own\n",
        ),
        ftl("two.ftl", "d = D\nd = again\nbroken = {\n"),
    ]);
    let problems = locale_problems(&reference, &locale);
    // The parse error's wording is fluent-syntax's; its location is ours.
    assert!(problems[0].starts_with("two.ftl:3: "), "{problems:#?}");
    assert_eq!(
        problems[1..],
        [
            "two.ftl: `d` defined twice",
            "missing `a`",
            "`b`: value .label (en-US: value .title)",
            "`only-attrs`: value .label (en-US: no value .label)",
            "extra `stale` (not in en-US, not referenced in this bundle)",
        ]
    );
}

// ---------------------------------------------------------------------------
// 2. Templates compile and render; every data-l10n-id resolves
// ---------------------------------------------------------------------------

/// Compiles `template` the way the `HtmlTemplate` loader does: name = asset
/// path (so `.html` autoescapes), together with every template it includes
/// or extends.
fn compile(assets: &Path, template: &str) -> Result<(), String> {
    let mut tera = bevy_markup::template::new_tera();
    tera.add_raw_templates(template_set(assets, template)?)
        .map_err(|err| error_chain(&err))
}

/// `template` and the templates it references (transitively), each with its
/// `{% include/extends "…" %}` paths rewritten to asset paths — a simple
/// mirror of the loader's resolution (same-directory and `../` paths).
fn template_set(assets: &Path, template: &str) -> Result<Vec<(String, String)>, String> {
    let mut set = Vec::new();
    let mut pending = vec![template.to_owned()];
    while let Some(name) = pending.pop() {
        if set.iter().any(|(done, _)| *done == name) {
            continue;
        }
        let source =
            std::fs::read_to_string(assets.join(&name)).map_err(|err| format!("{name}: {err}"))?;
        let dir = Path::new(&name).parent().unwrap_or(Path::new(""));
        let mut rewritten = source.clone();
        for keyword in ["include", "extends"] {
            for chunk in source.split("{%").skip(1) {
                let body = chunk.trim_start_matches('-').trim_start();
                let Some(after) = body.strip_prefix(keyword) else {
                    continue;
                };
                let after = after.trim_start();
                let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
                    continue;
                };
                let Some(end) = after[1..].find(quote) else {
                    continue;
                };
                let literal = &after[1..1 + end];
                let mut resolved = dir.to_path_buf();
                for part in Path::new(literal).components() {
                    match part {
                        std::path::Component::ParentDir => {
                            resolved.pop();
                        }
                        part => resolved.push(part),
                    }
                }
                let resolved = resolved.to_string_lossy().replace('\\', "/");
                rewritten = rewritten.replace(
                    &format!("{quote}{literal}{quote}"),
                    &format!("{quote}{resolved}{quote}"),
                );
                pending.push(resolved);
            }
        }
        set.push((name, rewritten));
    }
    Ok(set)
}

/// `None` when the root rendered; otherwise why not.
fn render_failure(ui: &mut TestUi) -> Option<String> {
    let root = ui.root();
    match ui.world_mut().entity(root).get::<RenderedHtml>() {
        Some(RenderedHtml::Ready(_)) => None,
        Some(RenderedHtml::Failed(message)) => Some(format!("failed to render: {message}")),
        Some(RenderedHtml::Pending) | None => Some("never rendered".to_owned()),
    }
}

#[test]
fn templates_render_with_the_examples_context_and_stylesheets() {
    let assets = assets();
    let mut problems = Vec::new();
    for page in PAGES {
        if let Err(err) = compile(&assets, page.template) {
            problems.push(format!("{}: doesn't compile: {err}", page.template));
            continue;
        }
        for (index, context) in (page.contexts)().iter().enumerate() {
            for stylesheet in page.stylesheets {
                let label = format!("{} (context {index}, {stylesheet})", page.template);
                let bundle = bundle_path(page, REFERENCE_LOCALE);
                let mut ui = match render(&assets, page, context, stylesheet, &bundle, &[]) {
                    Ok(ui) => ui,
                    Err(err) => {
                        problems.push(format!("{label}: {err}"));
                        continue;
                    }
                };
                if let Some(failure) = render_failure(&mut ui) {
                    problems.push(format!("{label}: {failure}"));
                }
                // `render` checked the sheet statically; this is the real loader.
                let world = ui.world_mut();
                let sheet = world.resource::<DefaultStylesheet>().0.clone().unwrap();
                if world
                    .resource::<AssetServer>()
                    .load_state(sheet.id())
                    .is_failed()
                {
                    problems.push(format!("{label}: stylesheet failed to load"));
                }
            }
        }
    }
    assert_no_problems("templates", &problems);
}

/// The `l10n error` lines of `ui`'s DOM outline: elements whose translation
/// failed (missing message, no value, formatting error such as a variable
/// the template doesn't pass) and fall back to their own content.
fn l10n_errors(ui: &mut TestUi) -> Vec<String> {
    let root = ui.root();
    let entity = ui.world_mut().entity(root);
    let (Some(RenderedHtml::Ready(document)), Some(localized)) =
        (entity.get::<RenderedHtml>(), entity.get::<LocalizedText>())
    else {
        return vec!["not rendered".to_owned()];
    };
    document
        .outline(localized)
        .lines()
        .filter_map(|line| {
            line.trim_start()
                .strip_prefix("l10n error: ")
                .map(str::to_owned)
        })
        .collect()
}

#[test]
fn every_l10n_id_resolves_in_every_locale() {
    let assets = assets();
    let families = families(&assets);
    let mut problems = Vec::new();
    'pages: for page in PAGES {
        let family = families
            .get(&assets.join(page.locales))
            .unwrap_or_else(|| panic!("{}: no bundle family `{}`", page.template, page.locales));
        for locale_dir in family {
            let locale = locale_dir.file_name().unwrap().to_str().unwrap();
            for (index, context) in (page.contexts)().iter().enumerate() {
                let bundle = bundle_path(page, locale);
                let mut ui = match render(&assets, page, context, page.stylesheets[0], &bundle, &[])
                {
                    Ok(ui) => ui,
                    Err(err) => {
                        problems.push(format!("{}: {err}", page.template));
                        continue 'pages;
                    }
                };
                for error in l10n_errors(&mut ui) {
                    problems.push(format!(
                        "{} ({locale}, context {index}): {error}",
                        page.template
                    ));
                }
            }
        }
    }
    assert_no_problems("unresolved data-l10n-id", &problems);
}

// ---------------------------------------------------------------------------
// 3. No hard-coded text outside data-l10n-id
// ---------------------------------------------------------------------------

/// Whether visible `text` needs a translation: it does if letters remain
/// after removing the interpolated `data` (case-insensitively, as Tera
/// filters like `title`/`upper` change case). Digits, punctuation and
/// symbols (`+`, `×`, `★`, `•`) read the same in every locale, and data
/// values (player names, item ids) aren't strings to translate — Fluent
/// receives them as arguments. Everything else needs a `data-l10n-id`,
/// including code samples (their comments are prose; cf. `code-sample`).
fn needs_translation(text: &str, data: &[String]) -> bool {
    let mut residue = text.to_lowercase();
    for value in data.iter().filter(|value| !value.is_empty()) {
        residue = residue.replace(&value.to_lowercase(), " ");
    }
    residue.chars().any(char::is_alphabetic)
}

/// Visible text in `dom` that isn't under a `data-l10n-id` element and
/// [needs translation](needs_translation), as `tag > tag: "text"`. Skips what
/// bevy_markup never shows: `head`, `script`, `style`, comments.
fn hard_coded_text(dom: &tl::VDom, data: &[String]) -> Vec<String> {
    fn visit(
        parser: &tl::Parser,
        handle: tl::NodeHandle,
        path: &mut Vec<String>,
        data: &[String],
        out: &mut Vec<String>,
    ) {
        match handle.get(parser) {
            Some(tl::Node::Tag(tag)) => {
                let name = tag.name().as_utf8_str().to_ascii_lowercase();
                let hidden = matches!(name.as_str(), "head" | "script" | "style");
                if hidden || tag.attributes().get("data-l10n-id").is_some() {
                    return;
                }
                path.push(name);
                for &child in tag.children().top().iter() {
                    visit(parser, child, path, data, out);
                }
                path.pop();
            }
            Some(tl::Node::Raw(text)) => {
                // `tl` keeps character references; decode as the crate does.
                let text = htmlize::unescape(text.as_utf8_str());
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if needs_translation(&text, data) {
                    out.push(format!("{}: {text:?}", path.join(" > ")));
                }
            }
            Some(tl::Node::Comment(_)) | None => {}
        }
    }
    let mut out = Vec::new();
    for &handle in dom.children() {
        visit(dom.parser(), handle, &mut Vec::new(), data, &mut out);
    }
    out
}

#[test]
fn hard_coded_text_check_finds_untranslated_text() {
    let html = r#"<head><title>Title</title><style>p { color: red }</style></head>
        <h1 data-l10n-id="t">Fallback <b>kept</b></h1>
        <div data-l10n-id="c"><p>Translated container</p></div>
        <p>Loose</p>
        <div><span>Nested</span> &amp; tail</div>
        <!-- a comment -->
        <script>let x = 1;</script>
        <p>3 × 4 + ★ — 42%</p>
        <p>&lt;&gt;</p>
        <p>Ada &lt;The Brave&gt;</p>
        <p>Welcome, ADA &lt;the brave&gt;</p>
        <li>TORCH 3</li>"#;
    let dom = tl::parse(html, tl::ParserOptions::default()).unwrap();
    let data = ["ada <the brave>".to_owned(), "torch".to_owned()];
    assert_eq!(
        hard_coded_text(&dom, &data),
        [
            r#"p: "Loose""#,
            r#"div > span: "Nested""#,
            r#"div: "& tail""#,
            r#"p: "Welcome, ADA <the brave>""#,
        ]
    );
}

#[test]
fn localized_templates_have_no_hard_coded_text() {
    let assets = assets();
    let mut problems = Vec::new();
    'pages: for page in PAGES.iter().filter(|page| page.unlocalized.is_none()) {
        let contexts = (page.contexts)();
        let data = data_strings(&contexts);
        for (index, context) in contexts.iter().enumerate() {
            let bundle = bundle_path(page, REFERENCE_LOCALE);
            let mut ui = match render(&assets, page, context, page.stylesheets[0], &bundle, &[]) {
                Ok(ui) => ui,
                Err(err) => {
                    problems.push(format!("{}: {err}", page.template));
                    continue 'pages;
                }
            };
            let root = ui.root();
            let Some(RenderedHtml::Ready(document)) =
                ui.world_mut().entity(root).get::<RenderedHtml>()
            else {
                problems.push(format!("{} (context {index}): not rendered", page.template));
                continue;
            };
            for text in hard_coded_text(document.dom(), &data) {
                problems.push(format!("{} (context {index}): {text}", page.template));
            }
        }
    }
    assert_no_problems("text outside data-l10n-id", &problems);
}

// ---------------------------------------------------------------------------
// 4. CSS parses; url()s resolve
// ---------------------------------------------------------------------------

/// Every `url(...)` argument in `css`, as written (quotes removed). Skips
/// comments and string literals, so only real `url` tokens count.
fn css_urls(css: &str) -> Vec<String> {
    let chars: Vec<char> = css.chars().collect();
    let is_ident = |c: char| c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii();
    // Index after the string literal opened at `start`.
    let skip_string = |start: usize| {
        let quote = chars[start];
        let mut i = start + 1;
        while i < chars.len() && chars[i] != quote {
            i += if chars[i] == '\\' { 2 } else { 1 };
        }
        i + 1
    };
    let mut urls = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
        } else if c == '"' || c == '\'' {
            i = skip_string(i);
        } else if is_ident(c) {
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();
            if !ident.eq_ignore_ascii_case("url") || chars.get(i) != Some(&'(') {
                continue;
            }
            i += 1;
            while chars.get(i).is_some_and(|c| c.is_whitespace()) {
                i += 1;
            }
            let start = i;
            if matches!(chars.get(i), Some('"' | '\'')) {
                i = skip_string(i);
                urls.push(chars[start + 1..i - 1].iter().collect());
            } else {
                while i < chars.len() && chars[i] != ')' && !chars[i].is_whitespace() {
                    i += 1;
                }
                urls.push(chars[start..i].iter().collect());
            }
        } else {
            i += 1;
        }
    }
    urls
}

/// The asset path `url` in the stylesheet at asset path `css` refers to
/// (relative to the `.css`, `/…` from the asset root), or why it can't be one.
fn resolve_url(css: &str, url: &str) -> Result<String, String> {
    if url.contains("://") || url.starts_with("data:") {
        return Err("not an asset path".to_owned());
    }
    let (base, url) = match url.strip_prefix('/') {
        Some(rooted) => ("", rooted),
        None => (css.rsplit_once('/').map_or("", |(dir, _)| dir), url),
    };
    let mut parts: Vec<String> = Vec::new();
    for component in Path::new(base).join(url).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().unwrap().to_owned()),
            Component::ParentDir => {
                parts.pop().ok_or("leaves the asset root")?;
            }
            _ => {}
        }
    }
    Ok(parts.join("/"))
}

/// Problems with the stylesheet at asset path `css` (source `source`) under
/// the asset root `root`: parse errors (lightningcss, as the `Stylesheet`
/// loader parses) and `url()`s that don't resolve to a file.
fn stylesheet_problems(root: &Path, css: &str, source: &str) -> Vec<String> {
    use bevy_markup::lightningcss::stylesheet::{ParserOptions, StyleSheet};
    let mut problems = Vec::new();
    let options = ParserOptions {
        filename: css.to_owned(),
        ..ParserOptions::default()
    };
    if let Err(err) = StyleSheet::parse(source, options) {
        problems.push(format!("{css}: {err}"));
    }
    for url in css_urls(source) {
        match resolve_url(css, &url) {
            Ok(path) if root.join(&path).is_file() => {}
            Ok(path) => problems.push(format!("{css}: url({url:?}): no file {path}")),
            Err(err) => problems.push(format!("{css}: url({url:?}): {err}")),
        }
    }
    problems
}

#[test]
fn css_url_scanner_finds_only_url_tokens() {
    let css = r#"a { b: url(x.png); c: url( "y z.png" ) }
        /* url(commented.png) */
        d { content: "url(in-string.png)"; e: URL('q\'s.png') }
        f { background: myurl(not.png); g: url(../up/r.png) }"#;
    assert_eq!(
        css_urls(css),
        ["x.png", "y z.png", r"q\'s.png", "../up/r.png"]
    );
}

#[test]
fn stylesheet_check_reports_parse_errors_and_dangling_urls() {
    let root = std::env::temp_dir().join(format!(
        "bevy_markup-content-lint-css-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("ui/themes")).unwrap();
    std::fs::write(root.join("ui/frame.png"), common::FRAME_PNG).unwrap();
    std::fs::write(root.join("ui/themes/near.png"), common::FRAME_PNG).unwrap();

    let ok = r#"html { border-image: url("../frame.png") 16 fill; }
        p { border-image-source: url(near.png); }
        h1 { border-image-source: url(/ui/frame.png); }"#;
    assert_eq!(
        stylesheet_problems(&root, "ui/themes/a.css", ok),
        Vec::<String>::new()
    );

    let bad = r#"p { border-image-source: url("frame.png"); }
        h1 { border-image-source: url(../../../outside.png); }
        h2 { border-image-source: url(https://example.com/x.png); }
        { not css"#;
    let problems = stylesheet_problems(&root, "ui/themes/b.css", bad);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(problems.len(), 4, "{problems:#?}");
    assert!(
        problems[0].starts_with("ui/themes/b.css: "),
        "parse error: {}",
        problems[0]
    );
    assert_eq!(
        problems[1..],
        [
            r#"ui/themes/b.css: url("frame.png"): no file ui/themes/frame.png"#,
            r#"ui/themes/b.css: url("../../../outside.png"): leaves the asset root"#,
            r#"ui/themes/b.css: url("https://example.com/x.png"): not an asset path"#,
        ]
    );
}

#[test]
fn every_stylesheet_parses_and_its_urls_resolve() {
    let assets = assets();
    let sheets = content_files(&assets, "css");
    assert!(!sheets.is_empty(), "no stylesheets under assets/");
    let problems: Vec<String> = sheets
        .iter()
        .flat_map(|css| {
            let source = std::fs::read_to_string(assets.join(css)).unwrap();
            stylesheet_problems(&assets, css, &source)
        })
        .collect();
    assert_no_problems("stylesheets", &problems);
}

// ---------------------------------------------------------------------------
// 5. Pseudo-locale
// ---------------------------------------------------------------------------

/// Letter → accented look-alike (pseudo-localization).
#[rustfmt::skip]
const ACCENTS: [(char, char); 52] = [
    ('a', 'à'), ('b', 'ƀ'), ('c', 'ç'), ('d', 'ð'), ('e', 'é'), ('f', 'ƒ'), ('g', 'ĝ'),
    ('h', 'ĥ'), ('i', 'î'), ('j', 'ĵ'), ('k', 'ķ'), ('l', 'ļ'), ('m', 'ɱ'), ('n', 'ñ'),
    ('o', 'ö'), ('p', 'þ'), ('q', 'ǫ'), ('r', 'ŕ'), ('s', 'š'), ('t', 'ţ'), ('u', 'û'),
    ('v', 'ṽ'), ('w', 'ŵ'), ('x', 'ẋ'), ('y', 'ý'), ('z', 'ž'),
    ('A', 'À'), ('B', 'Ɓ'), ('C', 'Ç'), ('D', 'Ð'), ('E', 'É'), ('F', 'Ƒ'), ('G', 'Ĝ'),
    ('H', 'Ĥ'), ('I', 'Î'), ('J', 'Ĵ'), ('K', 'Ķ'), ('L', 'Ļ'), ('M', 'Ṁ'), ('N', 'Ñ'),
    ('O', 'Ö'), ('P', 'Þ'), ('Q', 'Ǫ'), ('R', 'Ŕ'), ('S', 'Š'), ('T', 'Ţ'), ('U', 'Û'),
    ('V', 'Ṽ'), ('W', 'Ŵ'), ('X', 'Ẋ'), ('Y', 'Ý'), ('Z', 'Ž'),
];

/// Pseudo-localized text: has a wrap marker or an accented letter.
fn is_pseudo(text: &str) -> bool {
    text.chars()
        .any(|c| c == '[' || c == ']' || ACCENTS.iter().any(|&(_, accented)| accented == c))
}

/// Accents the letters of FTL text outside markup: tags (`<…>`, attributes
/// included) and character references (`&lt;`, `&#42;`) stay as written.
/// `in_tag` carries the state across a pattern's elements (a placeable can
/// sit inside a tag). Returns the text and its letter count.
fn accent(text: &str, in_tag: &mut bool) -> (String, usize) {
    let mut out = String::with_capacity(text.len() * 2);
    let mut letters = 0;
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let reference = (c == '&' && !*in_tag)
            .then(|| rest[1..].find(';').map(|end| &rest[..end + 2]))
            .flatten()
            .filter(|r| {
                r.len() > 2
                    && r[1..r.len() - 1]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '#')
            });
        if let Some(reference) = reference {
            out.push_str(reference);
            rest = &rest[reference.len()..];
            continue;
        }
        if *in_tag {
            *in_tag = c != '>';
            out.push(c);
        } else if c == '<' {
            *in_tag = true;
            out.push(c);
        } else {
            letters += usize::from(c.is_alphabetic());
            out.push(
                ACCENTS
                    .iter()
                    .find(|&&(plain, _)| plain == c)
                    .map_or(c, |&(_, a)| a),
            );
        }
        rest = &rest[c.len_utf8()..];
    }
    (out, letters)
}

/// Accents every text element of `pattern` (select variants included; keys,
/// selectors and literals untouched). Returns the letter count of the
/// longest variant path.
fn accent_pattern(pattern: &mut ast::Pattern<String>, in_tag: &mut bool) -> usize {
    let mut letters = 0;
    for element in &mut pattern.elements {
        match element {
            ast::PatternElement::TextElement { value } => {
                let (text, count) = accent(value, in_tag);
                *value = text;
                letters += count;
            }
            ast::PatternElement::Placeable { expression } => {
                letters += accent_expression(expression, in_tag);
            }
        }
    }
    letters
}

fn accent_expression(expression: &mut ast::Expression<String>, in_tag: &mut bool) -> usize {
    match expression {
        ast::Expression::Select { variants, .. } => variants
            .iter_mut()
            .map(|variant| accent_pattern(&mut variant.value, &mut in_tag.clone()))
            .max()
            .unwrap_or(0),
        ast::Expression::Inline(ast::InlineExpression::Placeable { expression }) => {
            accent_expression(expression, in_tag)
        }
        ast::Expression::Inline(_) => 0,
    }
}

/// Accents `pattern`, pads it by 30% of its letters with `~`, and wraps it
/// in `[`…`]`. The markers are string-literal placeables: a text `[` at the
/// start of a line would read as a variant key.
fn pseudo_pattern(pattern: &mut ast::Pattern<String>) {
    let letters = accent_pattern(pattern, &mut false);
    let literal = |value: String| ast::PatternElement::Placeable {
        expression: ast::Expression::Inline(ast::InlineExpression::StringLiteral { value }),
    };
    let padding = "~".repeat((letters * 3).div_ceil(10));
    pattern.elements.insert(0, literal("[".to_owned()));
    pattern.elements.push(literal(format!("{padding}]")));
}

/// A pseudo-localized copy of FTL `source`: message values and attributes
/// accented, wrapped and lengthened; placeables, markup and character
/// references intact. Term values are accented but not wrapped (they show
/// inside messages); term attributes stay as written (selectors match them).
fn pseudo_localize(source: &str) -> String {
    let mut resource = fluent_syntax::parser::parse(source.to_owned())
        .unwrap_or_else(|(_, errors)| panic!("FTL doesn't parse: {errors:?}"));
    for entry in &mut resource.body {
        match entry {
            ast::Entry::Message(message) => {
                message.value.iter_mut().for_each(pseudo_pattern);
                for attribute in &mut message.attributes {
                    pseudo_pattern(&mut attribute.value);
                }
            }
            ast::Entry::Term(term) => {
                accent_pattern(&mut term.value, &mut false);
            }
            _ => {}
        }
    }
    let pseudo = fluent_syntax::serializer::serialize(&resource);
    if let Err((_, errors)) = fluent_syntax::parser::parse(pseudo.as_str()) {
        panic!("pseudo FTL doesn't parse: {errors:?}\n{pseudo}");
    }
    pseudo
}

#[test]
fn accent_keeps_markup_and_character_references() {
    let (text, letters) = accent(
        r#"Plain <b class="x">bold</b> &lt;hi&gt; &amp; &#42; & done"#,
        &mut false,
    );
    assert_eq!(
        text,
        r#"Þļàîñ <b class="x">ƀöļð</b> &lt;ĥî&gt; &amp; &#42; & ðöñé"#
    );
    assert_eq!(letters, 15);
    // A tag split by a placeable (`<a title="{ $t }">`) stays a tag.
    let mut in_tag = false;
    assert_eq!(accent(r#"<a title=""#, &mut in_tag).0, r#"<a title=""#);
    assert_eq!(
        accent(r#"" class="link">go</a>"#, &mut in_tag).0,
        r#"" class="link">ĝö</a>"#
    );
}

#[test]
fn pseudo_locale_keeps_placeables_markup_and_selectors() {
    use fluent::{FluentArgs, FluentBundle, FluentResource};

    let source = r#"greeting = Welcome back, { $name }!
    .title = Hi
count =
    { $n ->
        [one] One { $item }
       *[other] { $n } items
    }
code = fn main() {"{"} <b>42</b> {"}"}
notes =
    First line
    second line.
-brand = Acme
    .gender = neuter
uses-brand =
    { -brand.gender ->
        [neuter] { -brand } rocks
       *[other] nope
    }
"#;
    let pseudo = pseudo_localize(source);
    // Same messages and attributes as the source.
    let (original, localized) = (
        catalog(&[ftl("en", source)]),
        catalog(&[ftl("xa", &pseudo)]),
    );
    assert_eq!(original.messages, localized.messages);

    let mut bundle = FluentBundle::new(vec!["en-US".parse().unwrap()]);
    bundle.set_use_isolating(false);
    bundle
        .add_resource(FluentResource::try_new(pseudo).unwrap())
        .unwrap();
    let format = |id: &str, attribute: Option<&str>, args: &[(&str, fluent::FluentValue)]| {
        let message = bundle.get_message(id).unwrap();
        let pattern = match attribute {
            Some(name) => message.get_attribute(name).unwrap().value(),
            None => message.value().unwrap(),
        };
        let args = FluentArgs::from_iter(args.iter().cloned());
        let mut errors = Vec::new();
        let text = bundle
            .format_pattern(pattern, Some(&args), &mut errors)
            .into_owned();
        assert!(errors.is_empty(), "{id}: {errors:?}");
        text
    };
    // Letters: "Welcome back" = 11 → ⌈3.3⌉ = 4 `~`.
    assert_eq!(
        format("greeting", None, &[("name", "Ada".into())]),
        "[Ŵéļçöɱé ƀàçķ, Ada!~~~~]"
    );
    assert_eq!(format("greeting", Some("title"), &[]), "[Ĥî~]");
    // Longest variant " items" = 5 → 2 `~`; `[one]` still selects.
    let one = [("n", 1.into()), ("item", "key".into())];
    assert_eq!(format("count", None, &one), "[Öñé key~~]");
    assert_eq!(format("count", None, &[("n", 3.into())]), "[3 îţéɱš~~]");
    assert_eq!(format("code", None, &[]), "[ƒñ ɱàîñ() { <b>42</b> }~~]");
    // 19 letters → 6 `~`; the `[` placeable starts an indented line.
    assert_eq!(
        format("notes", None, &[]),
        "[Ƒîŕšţ ļîñé\nšéçöñð ļîñé.~~~~~~]"
    );
    // Term value accented, its `.gender` still matches `[neuter]`.
    assert_eq!(format("uses-brand", None, &[]), "[Àçɱé ŕöçķš~~]");
}

#[test]
fn pseudo_locale_shows_every_string_translated() {
    let assets = assets();
    let mut problems = Vec::new();
    'pages: for page in PAGES.iter().filter(|page| page.unlocalized.is_none()) {
        // The pseudo bundle: en-US's resources, pseudo-localized, as en-XA.
        let reference_dir = assets.join(page.locales).join(REFERENCE_LOCALE);
        let (files, read_problems) = read_bundle(&assets, &reference_dir);
        assert_no_problems("en-US bundle", &read_problems);
        let pseudo_dir = format!("{}/en-XA", page.locales);
        let mut extra = Vec::new();
        let mut resources = Vec::new();
        let reference_prefix = format!("{}/{REFERENCE_LOCALE}/", page.locales);
        for (path, source) in &files {
            let resource = path.strip_prefix(&reference_prefix).unwrap();
            extra.push((format!("{pseudo_dir}/{resource}"), pseudo_localize(source)));
            resources.push(format!("{resource:?}"));
        }
        let manifest = format!("(locale: \"en-XA\", resources: [{}])", resources.join(", "));
        extra.push((format!("{pseudo_dir}/main.ftl.ron"), manifest));

        let contexts = (page.contexts)();
        let data = data_strings(&contexts);
        for (index, context) in contexts.iter().enumerate() {
            let bundle = format!("{pseudo_dir}/main.ftl.ron");
            let mut ui = match render(&assets, page, context, page.stylesheets[0], &bundle, &extra)
            {
                Ok(ui) => ui,
                Err(err) => {
                    problems.push(format!("{}: {err}", page.template));
                    continue 'pages;
                }
            };
            let runs = text_runs(&mut ui);
            assert!(
                runs.iter().any(|run| is_pseudo(run)),
                "{}: pseudo bundle not applied: {runs:?}",
                page.template
            );
            for run in runs {
                if !is_pseudo(&run) && needs_translation(&run, &data) {
                    problems.push(format!("{} (context {index}): {run:?}", page.template));
                }
            }
        }
    }
    assert_no_problems("untranslated text runs under the pseudo-locale", &problems);
}

/// The forms example's "Clear fields" looks its fields up by `id`
/// (`HtmlElements::by_id` reads the `id` attribute), so the inputs must
/// carry `id="name"`/`id="role"` (R62: with only `name=` the lookups
/// found nothing and the fields were never cleared).
#[test]
fn forms_clear_targets_are_id_attributes() {
    let page = std::fs::read_to_string("examples/assets/forms/signup.html").unwrap();
    for id in ["name", "role"] {
        assert!(
            page.contains(&format!("id=\"{id}\"")),
            "signup.html's `{id}` input needs `id=\"{id}\"`: clear-form looks it up by id"
        );
    }
}
