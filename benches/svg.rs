//! SVG benchmarks (ADR 0038) with criterion: where an SVG spends its time.
//!
//! - **Stages**, per document (`benches/shared/svg.rs`: an icon, a 40-group
//!   illustration, a drop-shadowed panel, a pattern chain near the
//!   recursion-depth limit, a 1920×1080 background — its 2× draw is 4K —
//!   and a 1024-icon sheet):
//!   - `depth`: the XML parse and the recursion-depth bound alone, the
//!     cost the check adds before usvg;
//!   - `parse`: the loader's whole parse (XML, depth bound, usvg);
//!   - `estimate`: the render-cost estimate at the intrinsic size;
//!   - `draw`: the budgeted resvg draw at 1× and at 2× (a HiDPI redraw);
//!   - `prepare_inline`: an inline `<svg>`'s preparation.
//! - **Frames**, one `app.update()` over the shared headless app:
//!   - `inline_idle`: N inline icons, nothing changing (cache hits);
//!   - `inline_recolor`: every icon's color changes each frame, so each is
//!     a new drawing (the inline cache's worst case);
//!   - `img_idle`: N `<img>` SVGs after their display-size redraw.
//!
//! Run: `cargo bench --features fuzzing --bench svg`.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;

mod shared;

use shared::svg::{documents, files, img_page, inline_page};
use shared::{SIZES, bench_app, borrowed, set_n};

fn stages(c: &mut Criterion) {
    let mut group = c.benchmark_group("svg_stages");
    for (name, source) in documents() {
        group.bench_with_input(BenchmarkId::new("depth", name), &source, |b, source| {
            b.iter(|| black_box(bevy_markup::fuzz::svg_depth(source).unwrap()))
        });
        group.bench_with_input(BenchmarkId::new("parse", name), &source, |b, source| {
            b.iter(|| black_box(bevy_markup::fuzz::svg_parse(source.as_bytes()).unwrap()))
        });
        let tree = bevy_markup::fuzz::svg_parse(source.as_bytes()).unwrap();
        let (width, height) = tree.intrinsic();
        group.bench_with_input(BenchmarkId::new("estimate", name), &tree, |b, tree| {
            b.iter(|| black_box(bevy_markup::fuzz::svg_cost(tree, width, height)))
        });
        for scale in [1, 2] {
            group.bench_with_input(
                BenchmarkId::new(format!("draw_{scale}x"), name),
                &tree,
                |b, tree| {
                    b.iter(|| {
                        black_box(
                            bevy_markup::fuzz::svg_draw(tree, width * scale, height * scale)
                                .unwrap(),
                        )
                    })
                },
            );
        }
    }
    let raw = shared::svg::ICON.replace(r#" xmlns="http://www.w3.org/2000/svg""#, "");
    group.bench_function("prepare_inline/icon", |b| {
        b.iter(|| black_box(bevy_markup::fuzz::svg_prepare_inline(&raw)))
    });
    group.finish();
}

fn frames(c: &mut Criterion) {
    let mut group = c.benchmark_group("svg_frames");
    for size in SIZES {
        let idle = files(inline_page(size, false));
        let (mut app, _) = bench_app(&borrowed(&idle));
        group.bench_with_input(BenchmarkId::new("inline_idle", size), &size, |b, _| {
            b.iter(|| app.update())
        });

        let recolor = files(inline_page(size, true));
        let (mut app, root) = bench_app(&borrowed(&recolor));
        let mut n = 0;
        group.bench_with_input(BenchmarkId::new("inline_recolor", size), &size, |b, _| {
            b.iter(|| {
                n += 1;
                set_n(&mut app, root, n);
                app.update()
            })
        });

        let img = files(img_page(size));
        let (mut app, _) = bench_app(&borrowed(&img));
        group.bench_with_input(BenchmarkId::new("img_idle", size), &size, |b, _| {
            b.iter(|| app.update())
        });
    }
    group.finish();
}

criterion_group!(benches, stages, frames);
criterion_main!(benches);
