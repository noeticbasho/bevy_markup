//! The SVG benchmarks (ADR 0038) under Divan, with allocation counts: the
//! same stages and frame workloads as `benches/svg.rs` (criterion), from
//! `benches/shared/svg.rs`.
//!
//! Run: `cargo bench --features fuzzing --bench svg-divan`.

use divan::{AllocProfiler, Bencher, black_box};

mod shared;

use shared::svg::{documents, files, img_page, inline_page};
use shared::{SIZES, bench_app, borrowed, set_n};

#[global_allocator]
static ALLOC: AllocProfiler = AllocProfiler::system();

fn main() {
    divan::main();
}

const DOCUMENTS: [&str; 4] = ["icon", "illustration", "shadowed", "deep"];

fn document(name: &str) -> String {
    documents()
        .into_iter()
        .find(|(document, _)| *document == name)
        .expect("a document")
        .1
}

#[divan::bench(args = DOCUMENTS, max_time = 2)]
fn depth(bencher: Bencher, name: &str) {
    let source = document(name);
    bencher.bench_local(|| black_box(bevy_markup::fuzz::svg_depth(&source).unwrap()));
}

#[divan::bench(args = DOCUMENTS, max_time = 2)]
fn parse(bencher: Bencher, name: &str) {
    let source = document(name);
    bencher.bench_local(|| black_box(bevy_markup::fuzz::svg_parse(source.as_bytes()).unwrap()));
}

#[divan::bench(args = DOCUMENTS, max_time = 2)]
fn draw_1x(bencher: Bencher, name: &str) {
    let tree = bevy_markup::fuzz::svg_parse(document(name).as_bytes()).unwrap();
    let (width, height) = tree.intrinsic();
    bencher.bench_local(|| black_box(bevy_markup::fuzz::svg_draw(&tree, width, height).unwrap()));
}

#[divan::bench(args = DOCUMENTS, max_time = 2)]
fn draw_2x(bencher: Bencher, name: &str) {
    let tree = bevy_markup::fuzz::svg_parse(document(name).as_bytes()).unwrap();
    let (width, height) = tree.intrinsic();
    bencher.bench_local(|| {
        black_box(bevy_markup::fuzz::svg_draw(&tree, width * 2, height * 2).unwrap())
    });
}

#[divan::bench(args = SIZES, max_time = 2)]
fn inline_idle(bencher: Bencher, size: usize) {
    let files = files(inline_page(size, false));
    let (mut app, _) = bench_app(&borrowed(&files));
    bencher.bench_local(|| black_box(app.update()));
}

#[divan::bench(args = SIZES, max_time = 2)]
fn inline_recolor(bencher: Bencher, size: usize) {
    let files = files(inline_page(size, true));
    let (mut app, root) = bench_app(&borrowed(&files));
    let mut n = 0;
    bencher.bench_local(|| {
        n += 1;
        set_n(&mut app, root, n);
        black_box(app.update())
    });
}

#[divan::bench(args = SIZES, max_time = 2)]
fn img_idle(bencher: Bencher, size: usize) {
    let files = files(img_page(size));
    let (mut app, _) = bench_app(&borrowed(&files));
    bencher.bench_local(|| black_box(app.update()));
}
