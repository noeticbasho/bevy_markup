//! The windowed build (ADR 0031/0032) against an unwindowed build of the
//! same document, as a property: random documents × random steps, the two
//! stepped in lockstep and compared in every frame.
//!
//! The oracle is the crate's own full build at the same scroll offset, laid
//! out by the same engine: windowing must not change what is on screen.
//! Per frame ([`Pair::scroll`]):
//! - **placement:** every built element with an id sits where the full
//!   build puts it (within 1 px);
//! - **coverage:** every element the viewport shows is built (except the
//!   first frame after a text change: bug_0170, a known limit);
//! - **keys:** no two entities share a `WindowKey`;
//! - **tightness**, once settled: no element is built wholly outside the
//!   band (the viewport grown by the margin) — the cull is exact once the
//!   heights are measured, so over-building is a bug too (review_0006
//!   J05's slack).
//!
//! Documents mix paragraphs of three heights with optional margins,
//! sections (nested, padded, bordered, with their own gap), loose text
//! (anonymous blocks), tall blocks and rows that come and go
//! (`{% if f %}`); the root gap and the band margin vary. Steps change
//! text (`{{ v }}`, which can wrap), flip the conditional rows and
//! scroll. Found by review_0006's methods: compare by position, never by
//! counts (bug_0151 kept the counts right). The named tests below are the
//! property's finds, minimized (bug_0160–bug_0171).

mod common;
use common::*;

use bevy::prelude::*;
use bevy_markup::prelude::*;
use proptest::prelude::*;
use std::collections::BTreeMap;

/// A small deterministic generator (xorshift), so a failing seed
/// reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> usize {
        (self.next() % n.max(1)) as usize
    }
}

/// One random block; `ids` numbers the paragraphs.
fn block(rng: &mut Rng, depth: usize, ids: &mut usize, out: &mut String) {
    match rng.below(if depth < 2 { 10 } else { 7 }) {
        0..=4 => {
            *ids += 1;
            let height = ["", " h1", " h2"][rng.below(3)];
            // Margins in px or in % of the parent's content width; some
            // paragraphs have their own box (a wrapper node).
            let margin = [" m", " pm", "", "", ""][rng.below(5)];
            let boxed = if rng.below(5) == 0 { " bx" } else { "" };
            // Some rows render the context's value: the content changes
            // (a content build reuses the unchanged subtrees whole).
            let text = if rng.below(6) == 0 {
                "{{ v }}".to_owned()
            } else {
                format!("row {ids}")
            };
            let row = format!("<p id=\"p{ids}\" class=\"p{height}{margin}{boxed}\">{text}</p>");
            // Some rows come and go: the structure changes, so the
            // update's positional matching hands kept entities other
            // rows (their keys change, their sizes may not).
            match rng.below(8) {
                0 => out.push_str(&format!("{{% if f %}}{row}{{% endif %}}")),
                1 => out.push_str(&format!("{{% if not f %}}{row}{{% endif %}}")),
                _ => out.push_str(&row),
            }
        }
        5 => {
            *ids += 1;
            out.push_str(&format!("loose text {ids}"));
        }
        6 => out.push_str("<p class=\"t\">tall</p>"),
        _ => {
            let class = ["sec", "sec pad", "sec box", "sec wide"][rng.below(4)];
            out.push_str(&format!("<section class=\"{class}\">"));
            for _ in 0..1 + rng.below(8) {
                block(rng, depth + 1, ids, out);
            }
            out.push_str("</section>");
        }
    }
}

const CSS: &str = "\
.h1 { height: 37px }
.h2 { height: 81px }
.m { margin-top: 7px; margin-bottom: 3px }
.t { height: 400px }
.sec { gap: 3px }
.pad { padding-top: 60px; padding-bottom: 10px }
.box { border-width: 2px; padding: 4px; margin-top: 12px }
.wide { padding-right: 80px; border-right-width: 20px }
.pm { margin-top: 10%; margin-bottom: 4% }
.bx { padding: 3px 5px; border-width: 1px }
";

/// Each id'd element's (top, bottom) in window px.
fn rects(ui: &mut TestUi) -> BTreeMap<String, (f32, f32)> {
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

fn spawn(name: &str, page: &str, css: &str, windowed: Option<HtmlWindowed>) -> TestUi {
    let files = [("page.html", page), ("style.css", css)];
    let mut ui = TestUi::with_layout(name, &files, UVec2::new(300, 200))
        .stylesheet("style.css")
        .spawn(
            "page.html",
            TemplateContext::new().with("v", &"value").with("f", &true),
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
        .insert(ScrollPosition::default());
    if let Some(windowed) = windowed {
        ui.world_mut().entity_mut(root).insert(windowed);
    }
    ui.settle();
    ui.update(6);
    ui
}

fn scroll(ui: &mut TestUi, offset: f32) {
    let root = ui.root();
    ui.world_mut()
        .entity_mut(root)
        .insert(ScrollPosition(Vec2::new(0.0, offset)));
    ui.update(6);
}

/// One document built twice — fully and windowed — driven in step, and
/// compared after every scroll.
struct Pair {
    full: TestUi,
    windowed: TestUi,
    margin: f32,
    /// The page and settings, for the failure message.
    setup: String,
    /// What each step did, for the failure message.
    history: Vec<String>,
    /// Check tightness (off where a container is never culled inside by
    /// design: a grid or a row).
    tight: bool,
    /// Exempt a content step's first frame from coverage (bug_0170). Off
    /// where the test's changes leave every known height true.
    lag_exempt: bool,
    /// The tightness tolerance: the largest margin the page uses plus
    /// 1 px rounding.
    tight_px: f32,
}

impl Pair {
    fn new(name: &str, page: &str, css: &str, margin: f32) -> Self {
        Pair {
            full: spawn(&format!("wm-full-{name}"), page, css, None),
            windowed: spawn(
                &format!("wm-win-{name}"),
                page,
                css,
                Some(HtmlWindowed {
                    margin_viewports: margin,
                }),
            ),
            margin,
            setup: format!("{name}, margin {margin}\ncss: {css}\npage: {page}"),
            history: Vec::new(),
            tight: true,
            lag_exempt: true,
            tight_px: 31.0,
        }
    }

    /// How far the full build scrolls.
    fn range(&mut self) -> f32 {
        let root = self.full.root();
        let node = self
            .full
            .world_mut()
            .get::<ComputedNode>(root)
            .expect("laid out");
        (node.content_size.y * node.inverse_scale_factor - 200.0).max(0.0)
    }

    fn set<T: serde::Serialize + ?Sized>(&mut self, key: &'static str, value: &T, note: String) {
        for ui in [&mut self.full, &mut self.windowed] {
            let root = ui.root();
            ui.world_mut()
                .get_mut::<TemplateContext>(root)
                .unwrap()
                .insert(key, value);
        }
        self.history.push(note);
    }

    /// Both roots get a new width (the rows rewrap).
    fn resize(&mut self, width: f32) {
        for ui in [&mut self.full, &mut self.windowed] {
            let root = ui.root();
            ui.world_mut().get_mut::<Node>(root).unwrap().width = Val::Px(width);
        }
        self.history.push(format!("width {width}"));
    }

    /// The conditional rows flip.
    fn flag(&mut self, flag: bool) {
        self.set("f", &flag, format!("f = {flag}"));
    }

    /// The `{{ v }}` rows get a new value.
    fn value(&mut self, value: &str) {
        self.set("v", value, format!("v = {} chars", value.len()));
    }

    /// Scrolls both to `offset` and checks the windowed build, frame by
    /// frame (both see every change in the same frame, so a one-frame
    /// blank is a failure too):
    /// - placement: every built element with an id sits where the full
    ///   build puts it (within 1 px on screen, 3 px off it: spacers sum
    ///   fractional margins that the layout rounds per node);
    /// - keys: no two entities share a `WindowKey` (a stale key names
    ///   another node's handle: its height lands there — bug_0169);
    /// - coverage: every element the viewport shows is built — except in
    ///   the first frame after a content change (bug_0170, a known limit:
    ///   rows above the band whose text changed height move everything
    ///   below before the measurement can know; the next frame's rebuild
    ///   corrects it, and the margin absorbs shifts up to its size);
    /// - tightness, once settled: nothing is built wholly outside the
    ///   band (while heights are being measured, over-building is
    ///   expected). The cull decides by the margin box and the rects are
    ///   border boxes: allow the largest generated margin (`.pm`'s 10% of the 300 px root, 30 px) plus
    ///   1 px rounding.
    fn scroll(&mut self, offset: f32) {
        // A content change since the last scroll (text or rows): this
        // step's frame 0 is a content build that places by the changed
        // subtrees' stored, stale heights (bug_0170's limit).
        let resized = self.lag_exempt
            && self
                .history
                .iter()
                .rev()
                .take_while(|step| !step.starts_with("scroll"))
                .any(|step| !step.starts_with("width"));
        // A new root width: the build in its frame sees the last layout
        // (the old width) — every height is the old one until it lays out
        // (bug_0170's limit), so that frame's placement is exempt too.
        let rewidth = self
            .history
            .iter()
            .rev()
            .take_while(|step| !step.starts_with("scroll"))
            .any(|step| step.starts_with("width"));
        self.history.push(format!("scroll {offset}"));
        for ui in [&mut self.full, &mut self.windowed] {
            let root = ui.root();
            ui.world_mut()
                .entity_mut(root)
                .insert(ScrollPosition(Vec2::new(0.0, offset)));
        }
        for frame in 0..6 {
            self.full.update(1);
            self.windowed.update(1);
            let (expected, built) = (rects(&mut self.full), rects(&mut self.windowed));
            let context = || {
                format!(
                    "frame {frame}\n{}\nsteps: {}",
                    self.setup,
                    self.history.join("; ")
                )
            };
            for (id, rect) in built.iter().filter(|_| !(rewidth && frame == 0)) {
                let want = expected[id];
                // Off-screen built elements (refused culls far below)
                // may drift by the spacers' pixel rounding: fractional `%`
                // margins sum differently from their rounded parts.
                let visible = want.1 > 0.0 && want.0 < 200.0;
                let tolerance = if visible { 1.0 } else { 3.0 };
                assert!(
                    (rect.0 - want.0).abs() <= tolerance,
                    "{id} at {:.1}, full build {:.1}\n{}",
                    rect.0,
                    want.0,
                    context()
                );
            }
            for (id, (top, bottom)) in &expected {
                if *bottom > 0.0 && *top < 200.0 && !((resized || rewidth) && frame == 0) {
                    assert!(
                        built.contains_key(id),
                        "{id} is visible but not built\n{}",
                        context()
                    );
                }
            }
            let mut keys = std::collections::HashMap::new();
            let world = self.windowed.world_mut();
            let mut keyed = world.query::<(Entity, &bevy_markup::WindowKey)>();
            for (entity, key) in keyed.iter(world) {
                if let Some(other) = keys.insert(key.0, entity) {
                    panic!("{other} and {entity} share {:?}\n{}", key.0, context());
                }
            }
            if frame < 5 || !self.tight {
                continue;
            }
            let band = (
                -self.margin * 200.0 - self.tight_px,
                200.0 + self.margin * 200.0 + self.tight_px,
            );
            for id in built.keys() {
                let (top, bottom) = expected[id];
                assert!(
                    bottom >= band.0 && top <= band.1,
                    "{id} ({top:.0} … {bottom:.0}) is built outside the band {band:?}\n{}",
                    context()
                );
            }
        }
    }
}

fn case(seed: u64) {
    let mut rng = Rng(seed | 1);
    let mut page = String::new();
    let mut ids = 0;
    for _ in 0..20 + rng.below(40) {
        block(&mut rng, 0, &mut ids, &mut page);
    }
    let gap = [0, 2, 6][rng.below(3)];
    let css = format!("html {{ flex-direction: column; gap: {gap}px }}\n{CSS}");
    let margin = [0.0, 0.5, 1.0][rng.below(3)];
    let mut pair = Pair::new(&format!("seed {seed}"), &page, &css, margin);
    let range = pair.range();
    let mut offset = 0.0_f32;
    for _ in 0..8 {
        if rng.below(4) == 0 {
            pair.flag(rng.below(2) == 0);
        }
        // Short, or long enough to wrap (the rows' heights change).
        if rng.below(3) == 0 {
            pair.value(
                [
                    "short",
                    "a much longer value that wraps onto a second line in a 300 px wide root",
                ][rng.below(2)],
            );
        }
        offset = match rng.below(3) {
            // A small step, either way.
            0 => (offset + rng.below(80) as f32 - 40.0).clamp(0.0, range),
            // A medium step.
            1 => (offset + rng.below(600) as f32 - 300.0).clamp(0.0, range),
            // A jump anywhere — past the end, once the conditional rows
            // shrank the document (bug_0164).
            _ => rng.below(range as u64 + 1) as f32,
        };
        pair.scroll(offset);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, ..ProptestConfig::default() })]

    #[test]
    fn windowed_builds_match_the_full_build(seed in any::<u64>()) {
        case(seed);
    }
}

/// How many id'd elements are built in each of the next `frames` frames.
fn built_per_frame(ui: &mut TestUi, frames: usize) -> Vec<usize> {
    (0..frames)
        .map(|_| {
            ui.update(1);
            rects(ui).len()
        })
        .collect()
}

fn rows(range: std::ops::RangeInclusive<usize>, text: impl Fn(usize) -> String) -> String {
    range
        .map(|i| format!("<p id=\"p{i}\" class=\"p\">{}</p>", text(i)))
        .collect()
}

/// bug_0165: a child the cursor can't place must stop its frame's cull —
/// the cursor worked on a copy of the frame, so the stop was lost. Under
/// ADR 0033 a *new* element is estimated from its siblings; a child with an
/// `auto` margin is never estimated (the measurement can't resolve it, so
/// it has no height and is marked untrackable): the rows after it build,
/// or they culled by a cursor short of its height — in every frame, not
/// just the first (this test is strict in all of them).
#[test]
fn an_unmeasured_child_stops_its_frame_culling() {
    let page = format!(
        "<section class=\"sec\">{}<p id=\"auto\" class=\"h2 auto\">auto margin</p>{}</section>",
        rows(1..=10, |i| format!("row {i}")),
        rows(11..=200, |i| format!("row {i}"))
    );
    let css = format!("html {{ flex-direction: column }}\n{CSS}\n.auto {{ margin-top: auto }}");
    for offset in [500.0, 520.0, 540.0] {
        let mut pair = Pair::new("bug0165", &page, &css, 0.0);
        pair.lag_exempt = false;
        // The untrackable child stops the cull: everything after it builds.
        pair.tight = false;
        pair.scroll(offset);
        pair.scroll(offset + 7.0);
    }
}

/// bug_0166, minimized from seed 14696766847661416522 (an earlier seed's
/// case went quiet as other fixes landed): a changed child list drops its
/// subtree's heights (the pairing doesn't reach a container whose child
/// count changed), and a kept entity whose handle didn't shift and whose
/// size didn't change was never re-measured: its frame stayed invalid, so
/// rows stayed built once the band left them.
#[test]
fn a_changed_child_list_is_measured_again() {
    let page = "{% if f %}<p id=\"p7\" class=\"p pm\">row 7</p>{% endif %}{% if f %}<p id=\"p20\" class=\"p h2\">row 20</p>{% endif %}<section class=\"sec pad\"><p class=\"t\">tall</p><p class=\"t\">tall</p><p id=\"p30\" class=\"p h1 pm bx\">{{ v }}</p><section class=\"sec box\"><p class=\"t\">tall</p>{% if not f %}<p id=\"p31\" class=\"p h2 m bx\">row 31</p>{% endif %}<p id=\"p32\" class=\"p h1 pm bx\">{{ v }}</p><p id=\"p33\" class=\"p\">row 33</p><p class=\"t\">tall</p><p id=\"p34\" class=\"p h1\">row 34</p><p id=\"p35\" class=\"p h1\">row 35</p>{% if not f %}<p id=\"p36\" class=\"p h1\">row 36</p>{% endif %}</section></section><p id=\"p42\" class=\"p\">row 42</p>";
    let css = format!("html {{ flex-direction: column; gap: 6px }}\n{CSS}");
    let mut pair = Pair::new("bug0166", page, &css, 1.0);
    pair.scroll(6904.0);
    pair.flag(false);
    pair.scroll(1654.0);
}

/// bug_0167: a windowed content build whose band re-collects (here every
/// visible row renders `{{ v }}`) reuses little, which turned the
/// skeleton's pairing off — and with it the height model's remap: the
/// model cleared, and every later content build materialized the whole
/// document for a frame.
#[test]
fn a_windowed_ui_keeps_its_heights_through_low_reuse_builds() {
    let page = rows(1..=1000, |i| {
        if (480..=520).contains(&i) {
            "{{ v }}".to_owned()
        } else {
            format!("row {i}")
        }
    });
    let css = format!("html {{ flex-direction: column }}\n{CSS}");
    let mut ui = spawn(
        "bug0163",
        &page,
        &css,
        Some(HtmlWindowed {
            margin_viewports: 0.5,
        }),
    );
    scroll(&mut ui, 500.0 * 20.0);
    let before = rects(&mut ui).len();
    for (step, value) in ["alpha", "bravo", "gamma", "delta"].into_iter().enumerate() {
        let root = ui.root();
        ui.world_mut()
            .get_mut::<TemplateContext>(root)
            .unwrap()
            .insert("v", &value);
        let peaks = built_per_frame(&mut ui, 6);
        assert!(
            peaks.iter().all(|&built| built <= before * 3),
            "change {step}: {before} built before, {peaks:?} after"
        );
    }
}

/// bug_0168: one row inserted or removed above the band's end
/// materialized the whole document for a frame. The pairing zipped the
/// root's children by position (every row after the change paired with
/// its neighbour: unclean, so the cull refused it) and didn't pair a
/// nested container whose child count changed (its subtree's heights
/// dropped). Measured: 1000 rows, 20 built — 1000 the frame after. The
/// sibling alignment (common head and tail) keeps it to the band, and the
/// result matches the unwindowed twin in every frame.
#[test]
fn one_inserted_row_builds_only_the_band() {
    let at = |position: usize, condition: &str| -> String {
        let row = format!("{{% if {condition} %}}<p id=\"x\">x</p>{{% endif %}}");
        format!(
            "{}{row}{}",
            rows(1..=position, |i| format!("row {i}")),
            rows(position + 1..=1000, |i| format!("row {i}"))
        )
    };
    for (place, position) in [("top", 0), ("middle", 300), ("end", 1000)] {
        for (change, condition) in [("removed", "f"), ("inserted", "not f")] {
            for nested in [false, true] {
                let body = at(position, condition);
                let page = if nested {
                    format!("<section class=\"sec\">{body}</section>")
                } else {
                    body
                };
                let name = format!(
                    "bug0168-{place}-{change}-{}",
                    if nested { "nested" } else { "root" }
                );
                let css = format!("html {{ flex-direction: column }}\n{CSS}");
                let mut pair = Pair::new(&name, &page, &css, 0.5);
                pair.scroll(8000.0);
                let before = rects(&mut pair.windowed).len();
                pair.flag(false);
                let peaks = built_per_frame(&mut pair.windowed, 6);
                assert!(
                    peaks.iter().all(|&built| built <= before * 3),
                    "{name}: {before} built before, {peaks:?} after"
                );
                // Both in step again: placement and coverage per frame.
                pair.scroll(8000.0);
            }
        }
    }
}

/// bug_0169, minimized from seed 11449435356632411062: a subtree reused
/// whole after a handle shift (the rows after a removed one) refreshed its
/// root's `DomNode` but not its `WindowKey`. Measured under the stale
/// handle — which now names the section after it — the paragraph's height
/// and its opaque mark (a text node lays out as a row) landed on that
/// section: its frame stopped culling and the section, straddling the
/// band, reused whole the spec of a frame that had built everything.
#[test]
fn a_reused_subtree_moves_its_window_key_with_its_handle() {
    let page = "<p id=\"p2\" class=\"p h2 m\">row 2</p><p class=\"t\">tall</p><p class=\"t\">tall</p><p id=\"p3\" class=\"p h1\">row 3</p><p id=\"p4\" class=\"p h2\">row 4</p><p id=\"p5\" class=\"p h1\">row 5</p><section class=\"sec pad\"><section class=\"sec\">loose text 6<p id=\"p7\" class=\"p h2\">row 7</p></section><p class=\"t\">tall</p>{% if f %}<p id=\"p8\" class=\"p\">row 8</p>{% endif %}loose text 9<p id=\"p10\" class=\"p h1\">row 10</p><p id=\"p11\" class=\"p h1\">row 11</p></section><p id=\"p12\" class=\"p h2\">row 12</p><p id=\"p13\" class=\"p h1\">row 13</p><p id=\"p14\" class=\"p h1\">row 14</p><section class=\"sec box\"><section class=\"sec pad\">{% if f %}<p id=\"p24\" class=\"p h2\">row 24</p>{% endif %}<p id=\"p25\" class=\"p h1 m\">row 25</p>{% if not f %}<p id=\"p26\" class=\"p\">row 26</p>{% endif %}</section></section>";
    let css = format!("html {{ flex-direction: column; gap: 2px }}\n{CSS}");
    let mut pair = Pair::new("bug0165", page, &css, 0.0);
    pair.scroll(1904.0);
    pair.flag(false);
    pair.scroll(1798.0);
}

/// bug_0171: the measurement only ever added opaque marks,
/// so a container that stopped laying out its children opaquely never
/// culled them again — and it revisited a container only when its size
/// changed. A restyle (here a context `var()`) keeps the model (a content
/// change drops the mark with the pairing), and a one-column grid
/// restyled to a flex column keeps its size: only `Changed<Node>` sees it.
#[test]
fn a_container_that_stops_being_a_grid_culls_again() {
    let page = format!(
        "<section class=\"sec\">{}</section>",
        rows(1..=300, |i| format!("row {i}"))
    );
    let css =
        format!("html {{ flex-direction: column }}\n{CSS}\n.sec {{ display: var(--d, grid) }}");
    let mut ui = spawn(
        "bug0169-grid",
        &page,
        &css,
        Some(HtmlWindowed {
            margin_viewports: 0.5,
        }),
    );
    scroll(&mut ui, 3000.0);
    let grid = rects(&mut ui).len();
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("--d", &"flex");
    ui.update(6);
    scroll(&mut ui, 3100.0);
    let built = rects(&mut ui).len();
    assert!(grid > 200, "a grid's children never cull: {grid}");
    assert!(
        built < 40,
        "the column culls its children again: {built} built"
    );
}

/// bug_0173: a narrower root rewraps every row. The measurement anchored
/// the new width before the build compared it, so the model never cleared
/// and the culled rows kept their old-width heights (review_0006's
/// mutation run: no test changed a windowed root's width).
#[test]
fn a_narrower_root_rewraps_and_reculls() {
    let rows = rows(1..=200, |i| {
        format!("row {i} with enough words to wrap in a narrow root")
    });
    let css = format!("html {{ flex-direction: column; gap: 2px }}\n{CSS}");
    let mut pair = Pair::new("resize", &rows, &css, 0.0);
    pair.scroll(2000.0);
    pair.resize(120.0);
    pair.scroll(2000.0);
    pair.scroll(4000.0);
}

/// A visible row that shrinks pulls the rows below it into view: the
/// height change must rebuild (a changed height is news, not only a new
/// one), or the newly visible rows stay unbuilt with a marginless band.
#[test]
fn a_shrinking_visible_row_brings_the_next_rows_in() {
    let mut page = rows(1..=40, |i| format!("row {i}"));
    page.push_str("<p id=\"grow\" class=\"p\">{{ v }}</p>");
    page.push_str(&rows(41..=200, |i| format!("row {i}")));
    let css = format!("html {{ flex-direction: column }}\n{CSS}");
    let mut pair = Pair::new("shrink", &page, &css, 0.0);
    pair.value(&"a very long value ".repeat(12));
    pair.scroll(40.0 * 20.0);
    pair.value("short");
    pair.scroll(40.0 * 20.0);
}

/// A `tr` is a row by the UA default, not by a CSS declaration: only the
/// measurement's opaque mark keeps the vertical cursor out of its cells
/// (stacked, the third cell's y lies a row's width below the band).
#[test]
fn table_cells_are_never_culled_as_a_column() {
    let body: String = (1..=60)
        .map(|i| format!("<tr><td id=\"a{i}\" class=\"h2\">a</td><td id=\"b{i}\" class=\"h2\">b</td><td id=\"c{i}\" class=\"h2\">c</td></tr>"))
        .collect();
    let css = format!("html {{ flex-direction: column }}\n{CSS}");
    let mut pair = Pair::new("table", &format!("<table>{body}</table>"), &css, 0.0);
    pair.scroll(1500.0);
    pair.scroll(3000.0);
}

/// bug_0175: a whole document (`<html><head>…<body>`) never culled: the
/// unwrap stopped at `html`'s two children and the `body` walk-through
/// invalidated the root cursor. Walked-through wrappers now advance the
/// cursor by their children's heights.
#[test]
fn a_whole_document_culls_its_rows() {
    // A walked-through group (an unknown tag) with rows after it: its
    // height is the sum of its children's, or the rows after it misplace.
    let page = format!(
        "<html><head><title>t</title></head><!-- c --><body><x-group>{}</x-group>{}</body></html>",
        rows(1..=100, |i| format!("row {i}")),
        rows(101..=200, |i| format!("row {i}"))
    );
    let css = format!("html {{ flex-direction: column }}\n{CSS}");
    let mut pair = Pair::new("document", &page, &css, 0.0);
    pair.scroll(2000.0);
    pair.scroll(2600.0);
    pair.scroll(3000.0);
}

/// A restyle into a grid, a row or a reversed direction (a context
/// `var()`): its children must
/// not cull by the vertical cursor in the restyle's own frame — the
/// declared layout stops the cursor before the measurement re-marks it.
#[test]
fn a_restyle_into_a_grid_or_row_stops_the_cursor_at_once() {
    for (key, value, name) in [
        ("--d", "grid", "grid"),
        ("--dir", "row", "row"),
        // Reversed directions stack from the far end (bug_0177: they
        // counted as trackable columns).
        ("--dir", "column-reverse", "column-reverse"),
        ("--dir", "row-reverse", "row-reverse"),
    ] {
        let page = format!(
            "<section class=\"sec x\">{}</section>",
            rows(1..=200, |i| format!("row {i}"))
        );
        let css = format!(
            "html {{ flex-direction: column }}\n{CSS}\n.x {{ display: var(--d, flex); flex-direction: var(--dir, column); grid-template-columns: 1fr 1fr }}"
        );
        let mut pair = Pair::new(&format!("restyle-{name}"), &page, &css, 0.0);
        // A grid's or a row's children are never culled.
        pair.tight = false;
        pair.scroll(1000.0);
        pair.set(key, value, format!("{key} = {value}"));
        pair.scroll(1000.0);
        // Back at the top: the restyled section in view (a row's children
        // all sit on its first line).
        pair.scroll(0.0);
    }
}

/// bug_0174: a top-level element's `%` margins measured as 0 (its parent,
/// the `HtmlUi` root, has no `ChildOf`, and the width lookup required
/// one), so the spacer standing in for it was short by its margins and
/// every row below it sat that much too high.
#[test]
fn top_level_percent_margins_count_in_the_spacers() {
    let mut page = rows(1..=20, |i| format!("row {i}"));
    page.push_str("<p id=\"pm\" class=\"p pm bx\">percent margins</p>");
    page.push_str(&rows(21..=200, |i| format!("row {i}")));
    let css = format!("html {{ flex-direction: column }}\n{CSS}");
    let mut pair = Pair::new("bug0174", &page, &css, 0.0);
    pair.scroll(1500.0);
    pair.scroll(3000.0);
}

/// The slack, exactly: margin-less 20 px rows, a marginless band, 25 px
/// steps down then up — each step moves a materialized row out of the band
/// at one edge and a culled one in at the other, so every step must
/// rebuild, and a 1 px tightness tolerance sees a row left behind
/// (review_0006's mutation run: the slack's materialized terms survived).
#[test]
fn every_row_that_leaves_the_band_culls_at_once() {
    // A tall block holds the far edge, so the materialized rows leaving
    // are the only constraint that binds (a culled row entering at the
    // other edge would rebuild anyway): a tall tail going down, a tall
    // head going up.
    let rows_then_tall = format!(
        "{}<p class=\"t\">tail</p><p class=\"t\">tail</p>",
        rows(1..=100, |i| format!("row {i}"))
    );
    let tall_then_rows = format!(
        "<p class=\"t\">head</p><p class=\"t\">head</p>{}",
        rows(1..=100, |i| format!("row {i}"))
    );
    let nested = format!("<section class=\"sec\">{rows_then_tall}</section>");
    for (name, page, start, step) in [
        // The band [1 900, 2 100): the rows end at 2 000, the first tail
        // straddles the bottom, the second (2 400 …) is 300 px away.
        ("down", rows_then_tall, 1900.0, 25.0),
        ("down-nested", nested, 1900.0, 25.0),
        // The band [700, 900): the second head (400 … 800) straddles the
        // top, the first (… 400) is 300 px above.
        ("up", tall_then_rows, 700.0, -25.0),
    ] {
        let css = format!("html {{ flex-direction: column }}\n{CSS}");
        let mut pair = Pair::new(&format!("slack-{name}"), &page, &css, 0.0);
        pair.tight_px = 1.0;
        pair.lag_exempt = false;
        let mut offset = start;
        pair.scroll(offset);
        for _ in 0..6 {
            offset += step;
            pair.scroll(offset);
        }
    }
}

/// A `position: fixed` element pins to the viewport wherever its parent
/// is: the subtree holding it never culls (its `positioned` mark), or the
/// element vanishes with it (review_0006's mutation run: dropping the mark
/// for elements with resolvable margins passed).
#[test]
fn a_fixed_element_survives_its_section_leaving_the_band() {
    let page = format!(
        "<section class=\"sec\"><p id=\"pin\" class=\"fx\">pinned</p>{}</section>{}",
        rows(1..=20, |i| format!("row {i}")),
        rows(21..=300, |i| format!("row {i}"))
    );
    let css = format!(
        "html {{ flex-direction: column }}\n{CSS}\n.fx {{ position: fixed; top: 10px; left: 0 }}"
    );
    let mut pair = Pair::new("fixed", &page, &css, 0.0);
    pair.tight = false;
    pair.scroll(3000.0);
    pair.scroll(4000.0);
}

/// bug_0177: the root itself laid out as a row (the `HtmlUi` default
/// without `html { flex-direction: column }`) or reversed: the root
/// cursor assumed a column and culled side-by-side or bottom-up children
/// by vertical positions they don't have. Such a root windows nothing.
#[test]
fn a_root_that_is_not_a_column_windows_nothing() {
    for direction in ["row", "column-reverse"] {
        let page = rows(1..=120, |i| format!("row {i}"));
        let css = format!(
            "html {{ flex-direction: {direction}; flex-wrap: wrap }}\n{CSS}\n.p {{ width: 90px }}"
        );
        let mut pair = Pair::new(&format!("root-{direction}"), &page, &css, 0.0);
        pair.tight = false;
        pair.scroll(300.0);
        pair.scroll(900.0);
    }
}
