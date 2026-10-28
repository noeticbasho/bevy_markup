//! The testbench's workloads: what each scenario generates (templates and
//! stylesheets written into the in-memory `testbench://` source, so they
//! go through the real loaders and their pre-scans), how its model churns
//! per frame, and the context each workload UI renders.

use std::path::Path;

use bevy::asset::io::memory::Dir;
use bevy::prelude::*;
use serde_json::{Value, json};

/// The scenario list, grouped as the shell shows it.
pub const GROUPS: [(&str, &[&str]); 3] = [
    (
        "perf",
        &[
            "big-document",
            "all-dirty",
            "restyle-storm",
            "markdown",
            "localized",
            "many-uis",
            "calc-layout",
            "selector-list",
            "text-features",
        ],
    ),
    (
        "security",
        &[
            "nested-rules",
            "deep-dom",
            "var-bombs",
            "grid-bombs",
            "hostile-args",
            "args-injection",
            "remote-sources",
            "markdown-bombs",
            "selector-bombs",
            "sibling-bombs",
            "entity-bombs",
            "function-bombs",
            "selector-engine",
        ],
    ),
    (
        "correctness",
        &[
            "reuse-vs-fresh",
            "indent-vs-flat",
            "selectors-vs-fresh",
            "restyle-vs-full",
            "order-independence",
            "idle-quiet",
            "scaling",
        ],
    ),
];

/// How a correctness scenario's comparison is made (review_0005's
/// methods; see the README).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TwinKind {
    /// A fresh UI of the same state, spawned for the comparison: the
    /// incremental paths (reuse, keyed pairing) against a from-scratch
    /// build.
    Fresh,
    /// The scenario's own second UI (a different template of the same
    /// document).
    Static,
    /// A fresh UI of the same rows in reverse order, compared per row id:
    /// a row's style may depend only on the row (the sheet is
    /// order-independent), so a cache keyed on too little — the first
    /// element of a shape deciding for later ones (review_0005 I06) —
    /// shows as a difference. A fresh twin shares such a bug; the
    /// metamorphic reversal doesn't.
    Reversed,
    /// The same UI before and after a forced full restyle (its
    /// stylesheet re-inserted): ADR 0029's scoped interaction restyles
    /// against the from-scratch cascade, with the interaction state held.
    FullRestyle,
}

/// The scenario's comparison, if it has one.
pub fn twin_kind(scenario: &str) -> Option<TwinKind> {
    match scenario {
        "reuse-vs-fresh" | "selectors-vs-fresh" => Some(TwinKind::Fresh),
        "indent-vs-flat" => Some(TwinKind::Static),
        "order-independence" => Some(TwinKind::Reversed),
        "restyle-vs-full" => Some(TwinKind::FullRestyle),
        _ => None,
    }
}

/// How many model rows a scenario renders at `size`: the scaling
/// scenario's large UI renders four times the small one's.
pub fn model_rows(scenario: &str, size: usize) -> usize {
    if scenario == "scaling" {
        size * 4
    } else {
        size
    }
}

/// Every scenario id, in the shell's order.
pub fn all() -> Vec<&'static str> {
    GROUPS
        .iter()
        .flat_map(|(_, ids)| ids.iter().copied())
        .collect()
}

/// The sizes the − / + controls step through (rows, cards or UIs).
pub const SIZES: [usize; 9] = [10, 50, 100, 250, 500, 1000, 2000, 5000, 10000];

/// One row of the row-based workloads' model.
#[derive(Clone, serde::Serialize)]
pub struct Row {
    pub id: u64,
    pub name: String,
    pub value: u64,
    pub hot: bool,
    pub pct: u32,
    pub px: u32,
}

/// The mutable state the workloads render.
#[derive(Default)]
pub struct Model {
    pub rows: Vec<Row>,
    pub tick: u64,
    pub next_id: u64,
    rng: u64,
}

impl Model {
    pub fn new(size: usize) -> Self {
        let mut model = Self {
            rng: 0x9e37_79b9_7f4a_7c15,
            ..default()
        };
        for _ in 0..size {
            let row = model.row();
            model.rows.push(row);
        }
        model
    }

    /// xorshift: deterministic, so a run is reproducible.
    pub fn random(&mut self, below: usize) -> usize {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % below.max(1) as u64) as usize
    }

    fn row(&mut self) -> Row {
        let id = self.next_id;
        self.next_id += 1;
        let pct = 10 + self.random(20) as u32;
        Row {
            id,
            name: format!("unit-{id}"),
            value: self.random(1000) as u64,
            hot: false,
            pct,
            px: self.random(12) as u32,
        }
    }

    /// The accent colour the restyle storm cycles through (a context
    /// `var()`).
    pub fn accent(&self) -> String {
        let hue = (self.tick * 7 % 360) as f32;
        let color = Color::hsl(hue, 0.7, 0.55).to_srgba();
        format!(
            "#{:02x}{:02x}{:02x}",
            (color.red * 255.0) as u8,
            (color.green * 255.0) as u8,
            (color.blue * 255.0) as u8
        )
    }
}

/// How a workload UI is laid out in the stage, and what it is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// The scenario's (first) UI; its context follows the model.
    Main,
    /// The second UI the checks compare with the main one.
    Twin,
    /// Card `index` of the many-UIs scenario.
    Card(usize),
    /// A fixed UI a check reads (`probe` names it).
    Probe(&'static str),
}

/// One workload UI to spawn.
pub struct Spec {
    pub template: String,
    pub stylesheet: Option<String>,
    pub role: Role,
    /// A fixed context (security scenarios); `None` = from the model.
    pub context: Option<Value>,
}

/// What a scenario needs: its UIs, the stage's layout for them, and the
/// checks the shell shows.
pub struct Plan {
    pub specs: Vec<Spec>,
    /// Lay the UIs out in a wrapping row (cards, side-by-side twins)
    /// instead of a column.
    pub row: bool,
    pub checks: &'static [&'static str],
}

/// The workloads' shared stylesheet.
const BENCH_CSS: &str = "\
html { color: #dfe6ee; font-size: 13px; font-family: sans-serif; flex-direction: column; gap: 2px; }
h1 { color: #f0b429; font-size: 18px; }
h2 { color: #9fb3c8; font-size: 15px; }
.row { flex-direction: row; gap: 8px; padding: 1px 4px; border-width: 1px; border-color: var(--accent, #2c3542); }
.row.hot { background-color: #3a2f12; }
.k { color: #c7d2de; }
.v { color: #7fdbff; font-family: monospace; }
.wrap { flex-direction: row; flex-wrap: wrap; gap: 4px; }
.card { padding: 4px; border-width: 1px; border-color: #2c3542; background-color: #1a2029; }
";

const ROWS_HTML: &str = "{% for row in rows %}<div class=\"row{% if row.hot %} hot{% endif %}\" id=\"row-{{ row.id }}\"><p class=\"k\">{{ row.name }}</p><p class=\"v\">{{ row.value }}</p></div>\n{% endfor %}";

const DIRTY_HTML: &str = "{% for row in rows %}<div class=\"row\" id=\"row-{{ row.id }}\"><p class=\"k\">{{ row.name }}</p><p class=\"v\">{{ row.value }} / {{ tick }}</p></div>\n{% endfor %}";

/// The Markdown document, indented the way authors write Tera (ADR 0024).
const DOC_INDENTED_MD: &str = "# Report {{ tick }}

{% for row in rows %}
    {% if row.hot %}
        ## {{ row.name }} (hot)
    {% else %}
        ## {{ row.name }}
    {% endif %}

    - value **{{ row.value }}**
    - id {{ row.id }}

{% endfor %}
";

/// The same document without indentation: must build the same UI.
const DOC_FLAT_MD: &str = "# Report {{ tick }}

{% for row in rows %}
{% if row.hot %}
## {{ row.name }} (hot)
{% else %}
## {{ row.name }}
{% endif %}

- value **{{ row.value }}**
- id {{ row.id }}

{% endfor %}
";

const LOCALIZED_HTML: &str = "{% for row in rows %}<p data-l10n-id=\"tb-w-line\" data-l10n-args='{{ {\"n\": row.value, \"who\": row.name} }}'>x</p>
<p data-l10n-id=\"tb-w-count\" data-l10n-args='{{ {\"n\": row.value} }}'>x</p>
{% endfor %}";

const CARD_HTML: &str =
    "<div class=\"card\"><p class=\"k\">UI {{ id }}</p><p class=\"v\">{{ value }}</p></div>";

const CALC_HTML: &str = "<div class=\"wrap\">{% for row in rows %}<div class=\"card\" style=\"width: calc({{ row.pct }}% - {{ row.px }}px)\"><p class=\"v\">{{ row.value }}</p></div>{% endfor %}</div>";

/// (A sibling combinator must sit next to the subject: `.a + .b .k` skips
/// its rule — the documented limit — so the sibling rules style the rows
/// themselves.)
/// ADR 0028/0030's selectors over a nested list (bug_0150: reuse inside a
/// container), a table and an ordered list: structural pseudo-classes,
/// sibling combinators, attribute selectors and `:not()` in subject,
/// ancestor and sibling positions. The attributes follow the row's id and
/// flags, so the value churn changes text only and the rows keep reusing;
/// the random edits of `selectors-vs-fresh` toggle the flags too.
const SELECTOR_LIST_HTML: &str = "<section class=\"list\">{% for row in rows %}<div class=\"item{% if row.hot %} hot{% endif %}\" id=\"row-{{ row.id }}\" data-state=\"{% if row.hot %}hot{% else %}cold{% endif %}\" data-kind=\"k{{ row.id % 3 }}\"><p class=\"k\">{{ row.name }}</p><p class=\"v\">{{ row.value }}</p></div>
{% endfor %}</section>
<ol class=\"ranks\">{% for row in rows %}{% if loop.index0 < 20 %}<li data-state=\"{% if row.hot %}hot{% else %}cold{% endif %}\">{{ row.name }} <b>{{ row.value }}</b> <i>pts</i></li>{% endif %}{% endfor %}</ol>
<table class=\"grid\"><tr><th>name</th><th>value</th></tr>{% for row in rows %}{% if loop.index0 < 20 %}<tr><td>{{ row.name }}</td><td>{{ row.value }}</td></tr>{% endif %}{% endfor %}</table>";

const SELECTOR_CSS: &str = "\
.item { flex-direction: row; gap: 8px; padding: 1px 4px; border-width: 1px; border-color: #2c3542; }
.list > .item:nth-child(odd) { background-color: #1a2029; }
.list > .item:nth-child(-n+3) { border-color: #f0b429; }
.item[data-state=\"hot\"] .v { color: #ff6b6b; }
.item.hot + .item { border-color: #f0b429; }
.item.hot ~ .item { border-color: #7fdbff; }
.item.hot ~ .item:last-child { background-color: #2a1f3a; }
.item:not([data-state=\"hot\"]):first-of-type .k { color: #7fdb8f; }
[data-kind=\"k0\"] > .k { text-decoration: underline; }
.list .item:nth-of-type(3n+1) .v { font-family: monospace; }
li:nth-of-type(even) { color: #9fb3c8; }
li[data-state=\"hot\"] b + i { color: #ff6b6b; }
tr:first-child th { color: #f0b429; }
td:last-child { text-align: right; }
.item:empty { display: none; }
";

/// ADR 0027/0028's text and box properties on one page: alignment,
/// line height, letter spacing, case transforms, white-space, list
/// markers, a table, a tiled background image and an `overflow: auto`
/// panel holding the rows. The panel's class flips every frame onto a
/// background colour, so the panel is updated in place — its `Node`
/// rewritten from the spec (the axis back to clipping) — with its layout
/// unchanged: the shape that stopped `overflow: auto` from scrolling
/// (review_0005 I09). (A `border-radius` flip would not do: the radius is
/// resolved into `ComputedNode`, whose change re-runs the promotion.)
const TEXT_FEATURES_HTML: &str = "<section class=\"tf\">
<div class=\"panel{% if tick % 2 == 0 %} even{% endif %}\" id=\"panel\">{% for row in rows %}<p class=\"t{{ loop.index0 % 5 }}\" id=\"row-{{ row.id }}\">{{ row.name }}   value  {{ row.value }}</p>{% endfor %}</div>
<ul class=\"bullets\">{% for row in rows %}{% if loop.index0 < 10 %}<li>{{ row.name }}</li>{% endif %}{% endfor %}</ul>
<ol class=\"ranks\">{% for row in rows %}{% if loop.index0 < 10 %}<li>{{ row.name }}</li>{% endif %}{% endfor %}</ol>
<table class=\"grid\"><tr><th>name</th><th>value</th></tr>{% for row in rows %}{% if loop.index0 < 10 %}<tr><td>{{ row.name }}</td><td>{{ row.value }}</td></tr>{% endif %}{% endfor %}</table>
</section>";

const TEXT_FEATURES_CSS: &str = "\
.tf { gap: 6px; }
.panel { height: 160px; overflow-y: auto; padding: 4px; background-image: url(\"tile.png\"); background-repeat: repeat-x; background-clip: padding-box; }
.panel.even { background-color: #11161d; }
.t0 { text-align: center; text-transform: uppercase; }
.t1 { text-align: right; letter-spacing: 0.1em; }
.t2 { line-height: 1.6; text-transform: capitalize; }
.t3 { white-space: pre; font-family: monospace; }
.t4 { text-align: justify; line-height: 20px; letter-spacing: 1px; }
.bullets { list-style-type: square; }
.ranks li:nth-child(odd) { color: #7fdbff; }
.grid td:last-child { text-align: right; }
";

/// The tile the text-features panel repeats (the examples' seal).
const TILE_PNG: &[u8] = include_bytes!("../assets/shared/cat.png");

/// An order-independent sheet over rows that differ only in what the
/// selectors read (`data-*`, a class): every row's style depends on the
/// row alone, so reversing the rows must not change any row's result.
/// I06's shape: each row is the only child of an id-less slot, so rows
/// share tag, classes, position and ancestor chain — an `id` or a
/// distinct position would key them apart and hide a cache that forgets
/// the dataset. The comparison finds a row by its name.
const ORDER_HTML: &str = "<section class=\"list\">{% for row in rows %}<div class=\"slot\"><div class=\"item{% if row.hot %} hot{% endif %}\" data-state=\"{% if row.hot %}hot{% else %}cold{% endif %}\" data-kind=\"k{{ row.value % 3 }}\"><p class=\"k\">{{ row.name }}</p><p class=\"v\">{{ row.value }}</p></div></div>
{% endfor %}</section>";

const ORDER_CSS: &str = "\
.item { flex-direction: row; gap: 8px; padding: 1px 4px; }
[data-kind=\"k0\"] { background-color: #1a2029; }
[data-kind=\"k1\"] .v { color: #7fdbff; }
.item[data-state=\"hot\"] .k { color: #ff6b6b; }
.item:not(.hot) .v { font-family: monospace; }
.list [data-kind=\"k2\"]:not([data-state=\"hot\"]) > .k { text-decoration: underline; }
";

/// Focusable rows (no `HtmlNoFocus` on this scenario's UI): the churn
/// moves `InputFocus`, and the sheet reacts through `:focus`,
/// `:focus-visible`, `:focus-within`, `:not()` and the sibling
/// combinators — the shapes of review_0005 I07/I08 under ADR 0029's
/// scoped restyles. `.w` is styled by a `:not(:focus)` rule alone: an
/// element another pseudo rule also targets re-cascades anyway, which
/// would hide a `:not()` that isn't flip-reactive.
const FOCUS_HTML: &str = "<section class=\"list\">{% for row in rows %}<div class=\"item\" id=\"row-{{ row.id }}\" tabindex=\"0\"><p class=\"k\">{{ row.name }}</p><p class=\"v\">{{ row.value }}</p><p class=\"w\">idle</p></div>
{% endfor %}</section>";

const FOCUS_CSS: &str = "\
.item { flex-direction: row; gap: 8px; padding: 1px 4px; border-width: 1px; border-color: #2c3542; }
.item:focus { border-color: #f0b429; }
.item:focus-visible .k { color: #f0b429; }
.item:focus + .item { border-color: #ff6b6b; }
.item:focus ~ .item:last-child { background-color: #2a1f3a; }
.list:focus-within .item:not(:focus) .k { color: #9fb3c8; }
.item:not(:focus):nth-child(odd) .v { font-family: monospace; }
.item:not(:focus) .w { color: #7fdb8f; }
";

/// Everything that could loop on its own writes (calc() widths, an
/// `overflow: auto` panel, a background image, transitions, structural
/// selectors): an idle UI must settle and stay quiet.
const QUIET_HTML: &str = "<section class=\"list\"><div class=\"panel\">{% for row in rows %}<div class=\"item\" id=\"row-{{ row.id }}\" style=\"width: calc({{ row.pct }}% + {{ row.px }}px)\"><p class=\"k\">{{ row.name }}</p><p class=\"v\">{{ row.value }}</p></div>{% endfor %}</div></section>";

const QUIET_CSS: &str = "\
.panel { height: 200px; overflow-y: auto; background-image: url(\"tile.png\"); }
.item { flex-direction: row; gap: 8px; transition: background-color 0.2s; }
.item:nth-child(odd) { background-color: #1a2029; }
.item:last-child .k { color: #f0b429; }
";

/// One container of many siblings with unterminated numeric references
/// (review_0005 I03, I04: both were quadratic).
const SCALING_HTML: &str = "<section class=\"list\">{% for row in rows %}<p class=\"x{% if row.hot %} hot{% endif %}\">&#1{{ row.value }} &am {{ row.name }}</p>{% endfor %}</section>";

const SCALING_CSS: &str = "\
.hot ~ p { color: #f0b429; }
p:nth-child(3n+1) { color: #7fdbff; }
p:last-of-type { color: #ff6b6b; }
";

/// Writes `files` under `testbench://<prefix>/` and returns their asset
/// paths in order.
fn write(dir: &Dir, prefix: &str, files: &[(&str, String)]) -> Vec<String> {
    files
        .iter()
        .map(|(name, text)| {
            let path = format!("{prefix}/{name}");
            dir.insert_asset_text(Path::new(&path), text);
            format!("testbench://{path}")
        })
        .collect()
}

/// Generates `scenario`'s assets under a fresh `prefix` (a new path per
/// start, so every start loads anew) and returns its plan.
#[hotpath::measure]
pub fn plan(scenario: &str, dir: &Dir, prefix: &str) -> Plan {
    let main = |template: &str, sheet: &str, role: Role| Spec {
        template: template.to_owned(),
        stylesheet: Some(sheet.to_owned()),
        role,
        context: None,
    };
    let probe = |template: &str, sheet: &str, name: &'static str, context: Value| Spec {
        template: template.to_owned(),
        stylesheet: Some(sheet.to_owned()),
        role: Role::Probe(name),
        context: Some(context),
    };
    let column = |specs, checks| Plan {
        specs,
        row: false,
        checks,
    };
    match scenario {
        "big-document" | "restyle-storm" | "reuse-vs-fresh" => {
            let paths = write(
                dir,
                prefix,
                &[
                    ("rows.html", ROWS_HTML.into()),
                    ("bench.css", BENCH_CSS.into()),
                ],
            );
            let checks: &'static [&'static str] = match scenario {
                "reuse-vs-fresh" => &["built", "twins", "budget"],
                "big-document" => &["built", "reuse", "budget"],
                _ => &["built", "budget"],
            };
            column(vec![main(&paths[0], &paths[1], Role::Main)], checks)
        }
        "all-dirty" => {
            let paths = write(
                dir,
                prefix,
                &[
                    ("rows.html", DIRTY_HTML.into()),
                    ("bench.css", BENCH_CSS.into()),
                ],
            );
            column(
                vec![main(&paths[0], &paths[1], Role::Main)],
                &["built", "budget"],
            )
        }
        "markdown" => {
            let paths = write(
                dir,
                prefix,
                &[
                    ("doc.md", DOC_INDENTED_MD.into()),
                    ("bench.css", BENCH_CSS.into()),
                ],
            );
            column(
                vec![main(&paths[0], &paths[1], Role::Main)],
                &["built", "budget"],
            )
        }
        "indent-vs-flat" => {
            let paths = write(
                dir,
                prefix,
                &[
                    ("indented.md", DOC_INDENTED_MD.into()),
                    ("flat.md", DOC_FLAT_MD.into()),
                    ("bench.css", BENCH_CSS.into()),
                ],
            );
            Plan {
                specs: vec![
                    main(&paths[0], &paths[2], Role::Main),
                    main(&paths[1], &paths[2], Role::Twin),
                ],
                row: true,
                checks: &["built", "twins", "budget"],
            }
        }
        "localized" => {
            let paths = write(
                dir,
                prefix,
                &[
                    ("rows.html", LOCALIZED_HTML.into()),
                    ("bench.css", BENCH_CSS.into()),
                ],
            );
            column(
                vec![main(&paths[0], &paths[1], Role::Main)],
                &["built", "budget"],
            )
        }
        "many-uis" => {
            let paths = write(
                dir,
                prefix,
                &[
                    ("card.html", CARD_HTML.into()),
                    ("bench.css", BENCH_CSS.into()),
                ],
            );
            // Card UIs are created by the caller from the model's rows.
            Plan {
                specs: vec![main(&paths[0], &paths[1], Role::Card(0))],
                row: true,
                checks: &["built", "budget"],
            }
        }
        "calc-layout" => {
            let paths = write(
                dir,
                prefix,
                &[
                    ("cards.html", CALC_HTML.into()),
                    ("bench.css", BENCH_CSS.into()),
                ],
            );
            column(
                vec![main(&paths[0], &paths[1], Role::Main)],
                &["built", "budget"],
            )
        }
        "nested-rules" => {
            let css = format!(
                "html {{ color: #ff6b6b }}\np {{ {} }}\n",
                "a {".repeat(20_000)
            );
            let paths = write(
                dir,
                prefix,
                &[
                    (
                        "page.html",
                        "<p>The sheet must fail its load: this text keeps the default colour.</p>"
                            .into(),
                    ),
                    ("bomb.css", css),
                ],
            );
            column(
                vec![probe(&paths[0], &paths[1], "sheet", json!({}))],
                &["sheet-refused", "built", "budget"],
            )
        }
        "deep-dom" => {
            let html = format!(
                "<p>Before the deep tree.</p>{}<p>deep</p>{}<p>After the deep tree.</p>",
                "<div>".repeat(5_000),
                "</div>".repeat(5_000)
            );
            let paths = write(
                dir,
                prefix,
                &[("page.html", html), ("bench.css", BENCH_CSS.into())],
            );
            column(
                vec![probe(&paths[0], &paths[1], "page", json!({}))],
                &["built", "budget"],
            )
        }
        "var-bombs" => {
            let mut css = String::from(
                "html { color: #dfe6ee; --c0: 1px; --d0: x; --f0: 1px; --x: var(--y, 1px); --y: var(--x, 2px); ",
            );
            for i in 1..2000 {
                css.push_str(&format!("--c{i}: var(--c{}); ", i - 1));
            }
            for i in 1..40 {
                css.push_str(&format!("--d{i}: var(--d{0}, a) var(--d{0}, b); ", i - 1));
            }
            for i in 1..64 {
                css.push_str(&format!(
                    "--f{i}: {}var(--f{}){}; ",
                    "calc(".repeat(20),
                    i - 1,
                    " + 1px)".repeat(20)
                ));
            }
            css.push_str("}\n.chain { width: var(--c1999, 40px) }\n.double { padding-left: var(--d39, 3px) }\n.cycle { width: var(--x, 120px) }\n.fn { width: var(--f63, 80px) }\n");
            let html = "<p class=\"chain\">a 2 000-link chain</p><p class=\"double\">a doubling fallback chain</p><p class=\"cycle\">a cycle</p><p class=\"fn\">nested functions on every link</p>";
            let paths = write(
                dir,
                prefix,
                &[("page.html", html.into()), ("bomb.css", css)],
            );
            column(
                vec![probe(&paths[0], &paths[1], "page", json!({}))],
                &["built", "budget"],
            )
        }
        "grid-bombs" => {
            let names: Vec<String> = (0..1000).map(|i| format!("a{i}")).collect();
            let areas = "\"a\" ".repeat(40_000);
            let css = format!(
                "html {{ color: #dfe6ee }}\n.g1 {{ display: grid; grid-template-columns: repeat(4000000000, 1px) }}\n.g2 {{ display: grid; grid-template-rows: repeat(1431655766, [a] 1px 1px 1px) }}\n.g3 {{ display: grid; grid-template-areas: {areas} }}\n.g4 {{ display: grid; grid-template-columns: repeat(1000, [{}] 1px) }}\n",
                names.join(" ")
            );
            let html = "<div class=\"g1\"><p>repeat(4 000 000 000)</p></div><div class=\"g2\"><p>a 32-bit wrap</p></div><div class=\"g3\"><p>40 000 area rows</p></div><div class=\"g4\"><p>1 000 names per repeat</p></div>";
            let paths = write(
                dir,
                prefix,
                &[("page.html", html.into()), ("bomb.css", css)],
            );
            column(
                vec![probe(&paths[0], &paths[1], "page", json!({}))],
                &["built", "budget"],
            )
        }
        "hostile-args" => {
            let html = "{% for p in payloads %}<p data-l10n-id=\"tb-w-hostile\" data-l10n-args='{{ {\"n\": 2, \"who\": p} }}'>x</p>
<p data-l10n-id=\"tb-w-hostile-cached\" data-l10n-args='{{ {\"who\": p} }}'>x</p>
{% endfor %}";
            let paths = write(
                dir,
                prefix,
                &[("page.html", html.into()), ("bench.css", BENCH_CSS.into())],
            );
            let payloads = json!([
                "<a href=x data-on-click=boom>Continue</a>",
                "[Continue](https://evil.example) ![x](../../save.png)",
                "\u{1}bevy-markup\u{1}<b data-x=' class=\"\u{e9}'>x</b>",
                "*emphasis* **strong** `code` ~~strike~~ | a | b |",
                "line\n\n# Heading\n\n- item\n\n    indented",
                "{{ secret }} {% raw %} \u{f73c}b\u{f73e}",
                "Ada \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} Jose\u{301}",
            ]);
            column(
                vec![probe(
                    &paths[0],
                    &paths[1],
                    "page",
                    json!({ "payloads": payloads }),
                )],
                &["built", "no-forged", "no-fallback", "budget"],
            )
        }
        "args-injection" => {
            let html = "<p id=\"literal\" data-l10n-id=\"tb-w-greet\" data-l10n-args='{{ {\"name\": name} }}'>x</p>
<p id=\"splice\" data-l10n-id=\"tb-w-greet\" data-l10n-args='{\"name\": \"{{ name }}\"}'>x</p>";
            let paths = write(
                dir,
                prefix,
                &[("page.html", html.into()), ("bench.css", BENCH_CSS.into())],
            );
            column(
                vec![probe(
                    &paths[0],
                    &paths[1],
                    "page",
                    json!({ "name": "Ada\", \"extra\": \"INJECTED" }),
                )],
                &["built", "map-literal", "budget"],
            )
        }
        "remote-sources" => {
            let paths = write(
                dir,
                prefix,
                &[
                    ("remote.html", "{% include \"https://evil.example/x.html\" %}".into()),
                    ("framed.html", "<p>The remote border-image was skipped; the sheet still applies.</p>".into()),
                    ("remote.css", "html { color: #7fdb8f; border-image: url(\"https://evil.example/f.png\") 4 stretch; border-width: 4px }".into()),
                    ("bench.css", BENCH_CSS.into()),
                ],
            );
            column(
                vec![
                    probe(&paths[0], &paths[3], "include", json!({})),
                    probe(&paths[1], &paths[2], "border", json!({})),
                ],
                &["include-refused", "border-skipped", "budget"],
            )
        }
        "markdown-bombs" => {
            let mut md = String::new();
            md.push_str(&format!("{}a\n\n", "*".repeat(20_000)));
            md.push_str(&format!("{}a\n\n", "[".repeat(20_000)));
            md.push_str(&format!("{}quote\n\n", "> ".repeat(2_000)));
            md.push_str("\u{1}bevy-markup\u{1}<b data-x=' class=\"\u{e9}'>x</b>\n\n");
            for depth in 0..30 {
                md.push_str(&format!("{}{{% if true %}}\n", "    ".repeat(depth)));
            }
            md.push_str(&format!("{}deep **text**\n", "    ".repeat(30)));
            for depth in (0..30).rev() {
                md.push_str(&format!("{}{{% endif %}}\n", "    ".repeat(depth)));
            }
            md.push_str(&"    an indented line\n".repeat(10_000));
            let paths = write(
                dir,
                prefix,
                &[("bomb.md", md), ("bench.css", BENCH_CSS.into())],
            );
            column(
                vec![probe(&paths[0], &paths[1], "page", json!({}))],
                &["survived", "budget"],
            )
        }
        "selector-list" | "selectors-vs-fresh" => {
            let css = format!("{BENCH_CSS}{SELECTOR_CSS}");
            let paths = write(
                dir,
                prefix,
                &[("list.html", SELECTOR_LIST_HTML.into()), ("bench.css", css)],
            );
            let checks: &'static [&'static str] = if scenario == "selectors-vs-fresh" {
                &["built", "twins", "budget"]
            } else {
                &["built", "reuse", "budget"]
            };
            column(vec![main(&paths[0], &paths[1], Role::Main)], checks)
        }
        "text-features" => {
            dir.insert_asset(Path::new(&format!("{prefix}/tile.png")), TILE_PNG.to_vec());
            let css = format!("{BENCH_CSS}{TEXT_FEATURES_CSS}");
            let paths = write(
                dir,
                prefix,
                &[("page.html", TEXT_FEATURES_HTML.into()), ("bench.css", css)],
            );
            // No reuse check: the panel's class flips every frame, and an
            // element whose attributes change rebuilds its subtree
            // (ADR 0021: an ancestor's classes can change descendants'
            // matches).
            column(
                vec![main(&paths[0], &paths[1], Role::Main)],
                &["built", "auto-scroll", "budget"],
            )
        }
        "order-independence" => {
            let css = format!("{BENCH_CSS}{ORDER_CSS}");
            let paths = write(
                dir,
                prefix,
                &[("list.html", ORDER_HTML.into()), ("bench.css", css)],
            );
            column(
                vec![main(&paths[0], &paths[1], Role::Main)],
                &["built", "twins", "budget"],
            )
        }
        "restyle-vs-full" => {
            let css = format!("{BENCH_CSS}{FOCUS_CSS}");
            let paths = write(
                dir,
                prefix,
                &[("list.html", FOCUS_HTML.into()), ("bench.css", css)],
            );
            column(
                vec![main(&paths[0], &paths[1], Role::Main)],
                &["built", "twins", "budget"],
            )
        }
        "idle-quiet" => {
            dir.insert_asset(Path::new(&format!("{prefix}/tile.png")), TILE_PNG.to_vec());
            let css = format!("{BENCH_CSS}{QUIET_CSS}");
            let paths = write(
                dir,
                prefix,
                &[("list.html", QUIET_HTML.into()), ("bench.css", css)],
            );
            column(
                vec![main(&paths[0], &paths[1], Role::Main)],
                &["built", "quiet", "budget"],
            )
        }
        "scaling" => {
            let css = format!("{BENCH_CSS}{SCALING_CSS}");
            let paths = write(
                dir,
                prefix,
                &[("list.html", SCALING_HTML.into()), ("bench.css", css)],
            );
            Plan {
                specs: vec![
                    main(&paths[0], &paths[1], Role::Main),
                    main(&paths[0], &paths[1], Role::Twin),
                ],
                row: true,
                checks: &["built", "linear"],
            }
        }
        "selector-bombs" => {
            let mut css = String::from("html { color: #dfe6ee }\n");
            // Extreme An+B (review_0005 I02 overflowed i32).
            for anb in [
                "n-2147483647",
                "-n+2147483647",
                "-2147483648n-2147483648",
                "2147483647n+2147483647",
                "+n+3",
                "-n+3",
            ] {
                css.push_str(&format!("p:nth-child({anb}) {{ letter-spacing: 1px }}\np:nth-of-type({anb}) {{ line-height: 1.3 }}\n"));
            }
            // Combinator characters, `]` and quotes inside quoted values
            // (bug_0144): the rule must parse and match the probe.
            css.push_str("p[data-x=\"a] b ~ c + d > e\"] { background-color: #2a6f2a }\n");
            css.push_str("p[data-y='say \"hi\"'] { color: #7fdb8f }\n");
            // A 10 000-character attribute value, and 1 000 sibling rules
            // over the same list.
            css.push_str(&format!(
                "p[data-z=\"{}\"] {{ color: #ff6b6b }}\n",
                "z".repeat(10_000)
            ));
            for i in 0..1000 {
                css.push_str(&format!(
                    ".c{i} ~ p:not(.c{}) {{ padding-left: {}px }}\n",
                    i + 1,
                    i % 7
                ));
            }
            // Nested `:not()` (ADR 0034's allowlist drops the rule) and a
            // long compound. Six deep: past the loader's paren guard (8,
            // bug_0179) the whole sheet is refused, which the
            // function-bombs scenario covers.
            css.push_str(&format!(
                "p{}x{} {{ color: red }}\n",
                ":not(".repeat(6),
                ")".repeat(6)
            ));
            css.push_str(&format!(
                "p{} {{ color: #9fb3c8 }}\n",
                (0..500).map(|i| format!(".q{i}")).collect::<String>()
            ));
            let mut html = String::from("<section>");
            html.push_str("<p id=\"probe\" data-x=\"a] b ~ c + d > e\">the quoted-value probe</p>");
            html.push_str("<p id=\"quotes\" data-y='say \"hi\"'>the escaped-quote probe</p>");
            for i in 0..200 {
                html.push_str(&format!(
                    "<p class=\"c{i}\" data-z=\"{}\">sibling {i}</p>",
                    if i % 50 == 0 {
                        "z".repeat(10_000)
                    } else {
                        String::new()
                    }
                ));
            }
            html.push_str("</section>");
            let paths = write(dir, prefix, &[("page.html", html), ("bomb.css", css)]);
            column(
                vec![probe(&paths[0], &paths[1], "page", json!({}))],
                &["built", "quoted-match", "budget"],
            )
        }
        "sibling-bombs" => {
            let css = format!(
                "{BENCH_CSS}{SCALING_CSS}.x:nth-child(-n+5) {{ background-color: #1a2029 }}\n"
            );
            let paths = write(
                dir,
                prefix,
                &[("list.html", SCALING_HTML.into()), ("bench.css", css)],
            );
            column(
                vec![main(&paths[0], &paths[1], Role::Main)],
                &["built", "budget"],
            )
        }
        "entity-bombs" => {
            let mut html = String::from("<p id=\"decoded\">&#x41;&#66;&amp;&#+65;&#x110000;</p>\n");
            html.push_str(&format!("<p>{}</p>\n", "&#1".repeat(50_000)));
            html.push_str(&format!("<p>{}</p>\n", "&#x1".repeat(50_000)));
            html.push_str(&format!("<p>{}</p>\n", "&am".repeat(50_000)));
            html.push_str(&format!("<p>&{};</p>\n", "a".repeat(100_000)));
            html.push_str(&format!(
                "<p title=\"{}\">attributes decode too</p>\n",
                "&#9".repeat(20_000)
            ));
            let paths = write(
                dir,
                prefix,
                &[("page.html", html), ("bench.css", BENCH_CSS.into())],
            );
            column(
                vec![probe(&paths[0], &paths[1], "page", json!({}))],
                &["built", "decoded", "budget"],
            )
        }
        "function-bombs" => {
            // bug_0179: lightningcss parses nested `light-dark(` and
            // `calc(` around a `var()` in exponential time — 30 deep took
            // minutes and hours. The paren guard (8) refuses the sheet and
            // drops the `style` attribute at once.
            let light_dark = "light-dark(".repeat(30);
            let calc_var = (0..30).fold("var(--x)".to_owned(), |v, _| format!("calc({v} - 1px)"));
            let bomb = format!(
                "html {{ color: #ff6b6b }}\np {{ color: {light_dark} }}\n.x {{ width: {calc_var} }}\n"
            );
            let inline = format!(
                "<p id=\"inline\" style=\"color: #ff6b6b; width: {calc_var}\">a style attribute with 30 nested calc() around a var(): dropped, the sheet's green stays</p>\
                 <p style=\"color: {light_dark}\">a style attribute with 30 nested light-dark(</p>"
            );
            let paths = write(
                dir,
                prefix,
                &[
                    (
                        "page.html",
                        "<p>The sheet must fail its load at once: this text keeps the default colour.</p>"
                            .into(),
                    ),
                    ("bomb.css", bomb),
                    ("inline.html", inline),
                    (
                        "inline.css",
                        "html { color: #dfe6ee }\n#inline { color: #7fdb8f }\n".into(),
                    ),
                ],
            );
            column(
                vec![
                    probe(&paths[0], &paths[1], "sheet", json!({})),
                    probe(&paths[2], &paths[3], "inline", json!({})),
                ],
                &["sheet-refused", "inline-dropped", "built", "budget"],
            )
        }
        "selector-engine" => {
            // ADR 0034's allowlist and bug_0180: the forms parcel's matcher
            // panics on (`:nth-child(… of S)`, `:has()`, `:nth-col()`,
            // nested `&` rules), the `:not\0()` that panicked the old
            // parser, a nested `:not()` and a far sibling combinator all
            // skip their rule while the sheet loads; `:nth-child(0)`
            // matches nothing (it coloured the root, so the whole page);
            // and the Level 4 forms match.
            let css = "html { color: #dfe6ee }\n\
                :nth-child(0) { color: #ff6b6b }\n\
                :nth-child(-n+0) { color: #ff6b6b }\n\
                :not\u{0}() { color: #ff6b6b }\n\
                li:nth-child(2 of .a) { color: #ff6b6b }\n\
                p:has(> b) { color: #ff6b6b }\n\
                li:nth-col(2), li:nth-last-col(1) { color: #ff6b6b }\n\
                .a { & > p { color: #ff6b6b } }\n\
                p:not(:not(#plain)) { color: #ff6b6b }\n\
                section + section p { color: #ff6b6b }\n\
                #ok { color: #7fdb8f }\n\
                :is(section, .none) > p:only-of-type[data-k^=\"ab\"] { background-color: #2a6f2a }\n";
            let html = "<section><p id=\"plain\">no rule colours this paragraph: it keeps the page colour</p>\
                <p id=\"ok\">a valid rule after the unsupported ones applies</p></section>\
                <section><p id=\"level4\" data-k=\"abc\">matched by :is(), :only-of-type and ^=</p></section>";
            let paths = write(
                dir,
                prefix,
                &[("page.html", html.into()), ("engine.css", css.into())],
            );
            column(
                vec![probe(&paths[0], &paths[1], "page", json!({}))],
                &["built", "engine-rules", "budget"],
            )
        }
        _ => column(Vec::new(), &[]),
    }
}

/// What a frame's churn changed.
pub enum Changed {
    None,
    All,
    Card(usize),
}

/// One frame of `scenario`'s churn on the model.
#[hotpath::measure]
pub fn churn(scenario: &str, model: &mut Model) -> Changed {
    model.tick += 1;
    let len = model.rows.len();
    if len == 0 {
        return Changed::None;
    }
    // About one row in a hundred.
    let sparse = (len / 100).max(1);
    match scenario {
        "text-features" => {
            // Values every fourth frame; the panel's class flips every
            // frame (the template reads `tick`): the frames between
            // rewrite the panel's `Node` with nothing else changing.
            if model.tick % 4 == 0 {
                for _ in 0..sparse {
                    let index = model.random(len);
                    model.rows[index].value = model.random(1000) as u64;
                }
            }
            Changed::All
        }
        "big-document" | "localized" | "markdown" | "indent-vs-flat" | "selector-list" => {
            for _ in 0..sparse {
                let index = model.random(len);
                model.rows[index].value = model.random(1000) as u64;
            }
            if scenario == "indent-vs-flat" {
                let index = model.random(len);
                model.rows[index].hot ^= true;
            }
            Changed::All
        }
        "all-dirty" => {
            for row in &mut model.rows {
                row.value += 1;
            }
            Changed::All
        }
        "restyle-storm" => {
            let tick = model.tick as usize;
            model.rows[(tick + len - 1) % len].hot = false;
            model.rows[tick % len].hot = true;
            Changed::All
        }
        "calc-layout" => {
            for _ in 0..sparse {
                let index = model.random(len);
                model.rows[index].pct = 10 + model.random(20) as u32;
                model.rows[index].px = model.random(12) as u32;
            }
            Changed::All
        }
        "many-uis" => {
            let index = model.random(len);
            model.rows[index].value += 1;
            Changed::Card(index)
        }
        "idle-quiet" => {
            // The same values written every frame: a re-render with an
            // identical source must not rebuild or restyle anything.
            Changed::All
        }
        "scaling" => {
            for row in &mut model.rows {
                row.value = (row.value + 1) % 1000;
            }
            let index = model.random(len);
            model.rows[index].hot ^= true;
            Changed::All
        }
        "sibling-bombs" => {
            // The `.hot` element moves: `~` restyles everything after it.
            let tick = model.tick as usize;
            model.rows[(tick + len - 1) % len].hot = false;
            model.rows[tick % len].hot = true;
            Changed::All
        }
        "restyle-vs-full" => {
            // The focus moves in the app (main.rs); the rows stay.
            Changed::None
        }
        "selectors-vs-fresh" => {
            // Count-preserving edits (value, flag, swap): a length change
            // makes the pairing skip the list (and strikes the skeleton),
            // so nothing would be reused — and nothing could go stale.
            for _ in 0..sparse {
                match model.random(3) {
                    0 => {
                        let index = model.random(len);
                        model.rows[index].value = model.random(1000) as u64;
                    }
                    1 => {
                        let index = model.random(len);
                        model.rows[index].hot ^= true;
                    }
                    _ if len > 1 => {
                        let index = model.random(len - 1);
                        model.rows.swap(index, index + 1);
                    }
                    _ => {}
                }
            }
            Changed::All
        }
        "reuse-vs-fresh" | "order-independence" => {
            // A random edit: change, insert, remove, move or highlight —
            // the shapes the keyed/positional pairing and the reuse cache
            // have to get right.
            match model.random(5) {
                0 => {
                    let index = model.random(len);
                    model.rows[index].value = model.random(1000) as u64;
                }
                1 if len < 4 * SIZES[SIZES.len() - 1] => {
                    let index = model.random(len + 1);
                    let row = model.row();
                    model.rows.insert(index, row);
                }
                2 if len > 1 => {
                    let index = model.random(len);
                    model.rows.remove(index);
                }
                3 if len > 1 => {
                    let index = model.random(len - 1);
                    model.rows.swap(index, index + 1);
                }
                _ => {
                    let index = model.random(len);
                    model.rows[index].hot ^= true;
                }
            }
            Changed::All
        }
        _ => Changed::None,
    }
}

/// The context of a model-driven workload UI. Only the restyle storm
/// carries the `--accent` var (a per-frame `var()` change restyles every
/// UI whose sheet reads it).
#[hotpath::measure]
pub fn context(scenario: &str, model: &Model, role: Role) -> Value {
    match role {
        Role::Card(index) => {
            let row = &model.rows[index.min(model.rows.len().saturating_sub(1))];
            json!({ "id": index, "value": row.value })
        }
        _ if scenario == "restyle-storm" => {
            json!({ "rows": model.rows, "tick": model.tick, "--accent": model.accent() })
        }
        // The scaling scenario's small UI renders the first quarter.
        Role::Main if scenario == "scaling" => {
            json!({ "rows": &model.rows[..model.rows.len() / 4], "tick": model.tick })
        }
        _ => json!({ "rows": model.rows, "tick": model.tick }),
    }
}
