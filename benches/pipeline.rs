//! Pipeline benchmarks (ADR 0018): where a content change spends its
//! time. Criterion measures three things:
//!
//! - **render**: Tera rendering the synthetic document (the crate's
//!   `HtmlTemplate::render` is a pass-through over this for `.html` —
//!   render, then the Markdown branch skipped — so `tera` is measured
//!   directly; `HtmlTemplate` has no public constructor);
//! - **parse**: `HtmlDocument::parse` (the `tl` stage);
//! - **frame** workloads: one `app.update()` per iteration over a headless
//!   app (the `tests/common` layout composition, without schedule
//!   shuffling):
//!   - `idle` — the render is identical: skipped (0009's "nothing
//!     changing" column);
//!   - `changing` — one templated value and one templated color differ:
//!     the whole render/parse/cascade runs, the diff is one paragraph, and
//!     the color change keeps a transition entry advancing every frame;
//!   - `rebuilding` — every paragraph templated: the whole document
//!     re-collected;
//!   - `localized` — every paragraph is a `data-l10n-id` message whose
//!     arg changes per frame (the Fluent stage);
//!   - `markdown` — a `.md` template with Markdown `markdown: true` Fluent
//!     values (the pulldown-cmark stage, per frame).
//!
//! The markup and stylesheet are a kitchen sink (fonts via registered
//! families, a grid, a flex row, `var()` + `calc()`, a transition,
//! positioning, a `border-image` frame) so the cascade's mapping arms run.
//! The same suite runs under Divan as `benches/pipeline-divan.rs` (ADR
//! 0019, allocation counts) — both measure `benches/shared.rs`.
//!
//! These are 0009's throwaway-benchmark method, formalized — the numbers
//! 0017 has to beat.
//!
//! Run: `cargo bench` (or `cargo bench --bench pipeline`). Criterion saves
//! per-benchmark baselines under `target/criterion/` (gitignored) and
//! flags regressions on the next run.

use bevy_markup::template::HtmlDocument;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

mod shared;

use shared::{SIZES, bench_app, bench_app_loading, rendered, set_n};

fn render_and_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("render");
    for size in SIZES {
        let source = shared::workload_page(size, "rebuilding");
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| black_box(tera::Tera::one_off(&source, &shared::context(0), true).unwrap()))
        });
    }
    group.finish();

    let mut group = c.benchmark_group("parse");
    for size in SIZES {
        let html = rendered(&shared::workload_page(size, "rebuilding"));
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            // `parse` takes the string; the clone is excluded with a batch.
            b.iter_batched(
                || html.clone(),
                |html| black_box(HtmlDocument::parse(html)),
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn frames(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame");

    // The kitchen-sink workloads at both sizes; the specialized ones at
    // 600 (the largest document, where per-frame cost matters).
    for size in SIZES {
        // Idle: nothing changes — the render is identical, everything
        // skipped (0009's "nothing changing" column).
        let idle_files = shared::frame_files(size, "idle");
        let files = shared::borrowed(&idle_files);
        let (mut app, _root) = bench_app(&files);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_function(BenchmarkId::new("idle", size), |b| {
            b.iter(|| black_box(app.update()))
        });

        // Changing: one value and one templated color differ — full
        // render/parse/cascade, a tiny diff, a transition entry advancing
        // every frame (0009's method: "one templated value changing per
        // frame", plus the advance system's active path).
        let changing_files = shared::frame_files(size, "changing");
        let files = shared::borrowed(&changing_files);
        let (mut app, root) = bench_app(&files);
        let mut n = 0_i64;
        group.bench_function(BenchmarkId::new("changing", size), |b| {
            b.iter(|| {
                n += 1;
                set_n(&mut app, root, n);
                black_box(app.update())
            })
        });

        // Changing, nested: the same inside one container (bug_0150).
        let nested_files = shared::frame_files(size, "changing-nested");
        let files = shared::borrowed(&nested_files);
        let (mut app, root) = bench_app(&files);
        let mut n = 0_i64;
        group.bench_function(BenchmarkId::new("changing-nested", size), |b| {
            b.iter(|| {
                n += 1;
                set_n(&mut app, root, n);
                black_box(app.update())
            })
        });

        // Rebuilding: every paragraph templated — a whole-document diff.
        let rebuilding_files = shared::frame_files(size, "rebuilding");
        let files = shared::borrowed(&rebuilding_files);
        let (mut app, root) = bench_app(&files);
        let mut n = 0_i64;
        group.bench_function(BenchmarkId::new("rebuilding", size), |b| {
            b.iter(|| {
                n += 1;
                set_n(&mut app, root, n);
                black_box(app.update())
            })
        });

        // Selectors (ADR 0034): a selector-heavy sheet over nested cards,
        // the whole document re-collected — the matcher's workload.
        let selector_files = shared::frame_files(size, "selectors");
        let files = shared::borrowed(&selector_files);
        let (mut app, root) = bench_app(&files);
        let mut n = 0_i64;
        group.bench_function(BenchmarkId::new("selectors", size), |b| {
            b.iter(|| {
                n += 1;
                set_n(&mut app, root, n);
                black_box(app.update())
            })
        });
    }

    // Localized (the Fluent stage) and markdown (the pulldown-cmark
    // stage): every paragraph a data-l10n-id message whose arg changes per
    // frame — at both sizes.
    for size in SIZES {
        let localized_files = shared::specialized_files(size, "localized");
        let files = shared::borrowed(&localized_files);
        let (mut app, root) = bench_app(&files);
        let mut n = 0_i64;
        group.bench_function(BenchmarkId::new("localized", size), |b| {
            b.iter(|| {
                n += 1;
                set_n(&mut app, root, n);
                black_box(app.update())
            })
        });

        // Markdown: a `.md` template (converted at render) whose Fluent
        // values are Markdown too (converted at localize, `markdown: true`).
        let markdown_files = shared::specialized_files(size, "markdown");
        let files = shared::borrowed(&markdown_files);
        let (mut app, root) = bench_app_loading(&files, shared::template_path("markdown"));
        let mut n = 0_i64;
        group.bench_function(BenchmarkId::new("markdown", size), |b| {
            b.iter(|| {
                n += 1;
                set_n(&mut app, root, n);
                black_box(app.update())
            })
        });
    }

    group.finish();
}

criterion_group!(benches, render_and_parse, frames);
criterion_main!(benches);
