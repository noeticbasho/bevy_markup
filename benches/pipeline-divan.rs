//! Pipeline benchmarks with Divan (ADR 0019): the same workloads,
//! documents and headless app composition as the criterion harness
//! (`benches/pipeline.rs`), via `benches/shared.rs` — so the two
//! harnesses' numbers corroborate each other. What Divan adds is
//! **allocation counting** (counts and bytes per benchmark, via its
//! global allocator): the update model's whole thesis (0009, 0017) is
//! about not redoing allocating work, and allocation counts are
//! deterministic where wall time is noisy.
//!
//! Run: `cargo bench --bench pipeline-divan` (or every target with
//! `cargo bench`). `max_time` bounds each benchmark the way the
//! criterion run's `--measurement-time` does.

use bevy_markup::template::HtmlDocument;
use divan::{AllocProfiler, Bencher, black_box, black_box_drop};

mod shared;

use shared::{
    SIZES, bench_app, bench_app_loading, frame_files, set_n, specialized_files, template_path,
};

/// Divan's allocator profiler: with this as the global allocator, every
/// benchmark reports allocation counts and bytes alongside time.
#[global_allocator]
static ALLOC: AllocProfiler = AllocProfiler::system();

fn main() {
    divan::main();
}

/// Tera rendering the synthetic document (the crate's `.html` render is a
/// pass-through over this; `HtmlTemplate` has no public constructor).
#[divan::bench(args = SIZES, max_time = 2)]
fn render(bencher: Bencher, size: usize) {
    let source = shared::workload_page(size, "rebuilding");
    bencher.bench_local(|| {
        black_box(tera::Tera::one_off(&source, &shared::context(0), true).unwrap())
    });
}

/// The `tl` parse stage. `parse` takes the string; the clone is setup
/// outside the measured closure, and the result is dropped black-boxed so
/// the parse's allocations are counted but not optimized away.
#[divan::bench(args = SIZES, max_time = 2)]
fn parse(bencher: Bencher, size: usize) {
    let html = shared::rendered(&shared::workload_page(size, "rebuilding"));
    bencher.bench_local(|| {
        let document = HtmlDocument::parse(html.clone());
        black_box_drop(document.unwrap());
    });
}

/// Idle: nothing changes — the render is identical, everything skipped.
#[divan::bench(args = SIZES, max_time = 2)]
fn idle(bencher: Bencher, size: usize) {
    let files = frame_files(size, "idle");
    let (mut app, _root) = bench_app(&shared::borrowed(&files));
    bencher.bench_local(|| black_box(app.update()));
}

/// Changing: one value and one templated color differ — full
/// render/parse/cascade, a tiny diff, a transition entry advancing every
/// frame.
#[divan::bench(args = SIZES, max_time = 2)]
fn changing(bencher: Bencher, size: usize) {
    let files = frame_files(size, "changing");
    let (mut app, root) = bench_app(&shared::borrowed(&files));
    let mut n = 0_i64;
    bencher.bench_local(|| {
        n += 1;
        set_n(&mut app, root, n);
        black_box(app.update());
    });
}

/// Changing, nested: the `changing` page inside one container — the
/// container is unclean every frame, its unchanged children reuse
/// (bug_0150).
#[divan::bench(args = SIZES, max_time = 2)]
fn changing_nested(bencher: Bencher, size: usize) {
    let files = frame_files(size, "changing-nested");
    let (mut app, root) = bench_app(&shared::borrowed(&files));
    let mut n = 0_i64;
    bencher.bench_local(|| {
        n += 1;
        set_n(&mut app, root, n);
        black_box(app.update());
    });
}

/// Rebuilding: every paragraph templated — a whole-document diff.
#[divan::bench(args = SIZES, max_time = 2)]
fn rebuilding(bencher: Bencher, size: usize) {
    let files = frame_files(size, "rebuilding");
    let (mut app, root) = bench_app(&shared::borrowed(&files));
    let mut n = 0_i64;
    bencher.bench_local(|| {
        n += 1;
        set_n(&mut app, root, n);
        black_box(app.update());
    });
}

/// Selectors (ADR 0034): a selector-heavy sheet over nested cards, the
/// whole document re-collected — the matcher's workload.
#[divan::bench(args = SIZES, max_time = 2)]
fn selectors(bencher: Bencher, size: usize) {
    let files = frame_files(size, "selectors");
    let (mut app, root) = bench_app(&shared::borrowed(&files));
    let mut n = 0_i64;
    bencher.bench_local(|| {
        n += 1;
        set_n(&mut app, root, n);
        black_box(app.update());
    });
}

/// Localized: every paragraph a `data-l10n-id` message whose arg changes
/// per frame (the Fluent stage).
#[divan::bench(args = SIZES, max_time = 2)]
fn localized(bencher: Bencher, size: usize) {
    let files = specialized_files(size, "localized");
    let (mut app, root) = bench_app(&shared::borrowed(&files));
    let mut n = 0_i64;
    bencher.bench_local(|| {
        n += 1;
        set_n(&mut app, root, n);
        black_box(app.update());
    });
}

/// Markdown: a `.md` template whose Fluent values are Markdown too (the
/// pulldown-cmark stage, per frame).
#[divan::bench(args = SIZES, max_time = 2)]
fn markdown(bencher: Bencher, size: usize) {
    let files = specialized_files(size, "markdown");
    let (mut app, root) = bench_app_loading(&shared::borrowed(&files), template_path("markdown"));
    let mut n = 0_i64;
    bencher.bench_local(|| {
        n += 1;
        set_n(&mut app, root, n);
        black_box(app.update());
    });
}

// --- Interaction benches -------------------------------------------------
//
// These are possible with criterion too, but they earn their keep only
// under divan: allocation counts per frame are deterministic where wall
// time is noisy, each run is cheap (`max_time`, no warm-up), and what
// they measure is "what does this interactive system allocate and trigger
// per frame" — a coverage/metrics question, not a statistics one. They
// cover the interactive systems the criterion suite doesn't reach
// (`signals.rs`'s hover path, `tooltips.rs`, `anchor.rs`, `focus.rs`).

use bevy::prelude::*;
use bevy::ui::ComputedNode;
use bevy_markup::prelude::*;

/// A stable hover over a bound, tooltip-carrying row (the pointer parked
/// on it via a real `CursorMoved` event): `hover_signals` tracks the
/// enter, `update_pseudo_states` holds the `:hover` styles, and the
/// tooltip `HtmlUi` stays anchored — the per-frame cost every mouse-driven
/// app pays.
#[divan::bench(args = SIZES, max_time = 2)]
fn hover_stable(bencher: Bencher, size: usize) {
    let files = hover_files(size);
    let (mut app, _root) = shared::bench_app_pointer(&shared::borrowed(&files), "page.html");
    insert_tooltips(&mut app);
    let center = bound_row_center(&mut app, 0).expect("a bound row laid out");
    shared::move_pointer(&mut app, center);
    let hover_center = move |app: &mut App| shared::move_pointer(app, center);
    bencher.bench_local(|| {
        hover_center(&mut app);
        black_box(app.update());
    });
}

/// A pointer flickering between a tooltip row and the window corner
/// (nothing bound or tooltip-carrying there): enter/leave signals and
/// `:hover` pseudo-state restyles every frame, tooltip despawn and
/// respawn — the active paths.
#[divan::bench(args = SIZES, max_time = 2)]
fn hover_flicker(bencher: Bencher, size: usize) {
    let files = hover_files(size);
    let (mut app, _root) = shared::bench_app_pointer(&shared::borrowed(&files), "page.html");
    insert_tooltips(&mut app);
    let center = bound_row_center(&mut app, 0).expect("a bound row laid out");
    let away = Vec2::new(5.0, 5.0);
    shared::move_pointer(&mut app, center);
    let mut flip = false;
    bencher.bench_local(|| {
        flip = !flip;
        shared::move_pointer(&mut app, if flip { center } else { away });
        black_box(app.update());
    });
}

/// The screen center of a bound row (`index` among the elements with
/// signals): where the pointer must land to hover it.
fn bound_row_center(app: &mut App, index: usize) -> Option<Vec2> {
    let entity = app
        .world_mut()
        .query_filtered::<Entity, With<ElementSignals>>()
        .iter(app.world())
        .nth(index)?;
    let node = app.world().get::<ComputedNode>(entity)?;
    let transform = app.world().get::<bevy::ui::UiGlobalTransform>(entity)?;
    Some(transform.translation + node.size / 2.0)
}

/// Directional focus navigation every frame (the keyboard/gamepad path a
/// game menu drives): `HtmlFocus::navigate` walks the focus chain and
/// restyles `:focus-visible` per frame.
#[divan::bench(args = SIZES, max_time = 2)]
fn focus_navigation(bencher: Bencher, size: usize) {
    let files = hover_files(size);
    let (mut app, _root) = bench_app_loading(&shared::borrowed(&files), "page.html");
    app.add_systems(Update, |mut focus: HtmlFocus| {
        focus.navigate(bevy::math::CompassOctant::South);
    });
    bencher.bench_local(|| black_box(app.update()));
}

/// Ten overlays anchored to ten rows (the HUD pattern): `place_anchored`
/// reads the last frame's layout and re-places every overlay per frame.
#[divan::bench(args = SIZES, max_time = 2)]
fn anchored(bencher: Bencher, size: usize) {
    let files = hover_files(size);
    let (mut app, _root) = bench_app_loading(&shared::borrowed(&files), "page.html");
    let rows = app
        .world_mut()
        .query_filtered::<Entity, With<ElementSignals>>()
        .iter(app.world())
        .take(10)
        .collect::<Vec<_>>();
    let anchors = rows
        .into_iter()
        .enumerate()
        .map(|(i, element)| {
            (
                element,
                if i % 2 == 0 {
                    AnchorPlacement::Below
                } else {
                    AnchorPlacement::Right
                },
            )
        })
        .collect::<Vec<_>>();
    for (element, placement) in anchors {
        app.world_mut().spawn((
            Node::default(),
            HtmlAnchor::new(element, placement).with_gap(8.0),
        ));
    }
    bencher.bench_local(|| black_box(app.update()));
}

/// Inserts [`HtmlTooltips`] and runs a few frames so the tooltip template
/// loads before the bench (the tooltip `HtmlUi` spawns only when it's
/// ready).
fn insert_tooltips(app: &mut App) {
    let server = app.world().resource::<AssetServer>();
    let template = server.load("tooltip.html");
    app.insert_resource(HtmlTooltips::new(template));
    for _ in 0..20 {
        app.update();
    }
}

/// The interactive workloads' files: the hover template, the stylesheet
/// and tooltip template, and the bundle.
fn hover_files(size: usize) -> Vec<(String, String)> {
    let mut files = vec![
        ("page.html".to_owned(), shared::hover_template(size)),
        ("style.css".to_owned(), shared::CSS.to_owned()),
        (
            "tooltip.html".to_owned(),
            shared::TOOLTIP_TEMPLATE.to_owned(),
        ),
    ];
    files.extend(shared::locale_files(false));
    files
}
