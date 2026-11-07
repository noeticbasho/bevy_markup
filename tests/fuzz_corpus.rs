//! Replays the cargo-fuzz inputs, the committed seeds (`fuzz/seeds/<target>/`)
//! and the local working corpus (`fuzz/corpus/<target>/`, gitignored),
//! through the same `bevy_markup::fuzz` harnesses the fuzz targets call, so coverage
//! tools can measure what fuzzing reaches alongside the tests
//! (`scripts/coverage.py`). Inputs are split exactly as in
//! `fuzz/fuzz_targets/*.rs`.
//!
//! `#[ignore]`d (thousands of inputs) and gated on the `fuzzing` feature:
//! `cargo test --features fuzzing --test fuzz_corpus -- --ignored`. The
//! replay runs on the test harness's ordinary 2 MB threads: the corpus's
//! deeply nested FTL placeables are a regression check for bug_0015.
#![cfg(feature = "fuzzing")]

use std::path::PathBuf;

/// Every input of `fuzz/seeds/<target>/` and `fuzz/corpus/<target>/` (if
/// present), lossily decoded. Over-long inputs are skipped, as the fuzz
/// targets do.
fn corpus(target: &str) -> Vec<String> {
    let mut inputs = Vec::new();
    for dir in ["fuzz/seeds", "fuzz/corpus"] {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(dir)
            .join(target);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let before = inputs.len();
        inputs.extend(
            entries
                .filter_map(|entry| std::fs::read(entry.ok()?.path()).ok())
                .filter(|bytes| bytes.len() <= 64 * 1024)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
        );
        eprintln!(
            "fuzz_corpus: {} inputs from {}",
            inputs.len() - before,
            dir.display()
        );
    }
    assert!(
        !inputs.is_empty(),
        "no inputs: fuzz/seeds/{target}/ is committed"
    );
    inputs
}

#[test]
#[ignore = "replays thousands of corpus inputs; run for coverage"]
fn replay_html_corpus() {
    for text in corpus("html") {
        let (source, context_json) = text.split_once("\n---\n").unwrap_or((&text, "{}"));
        let _ = bevy_markup::fuzz::render_html(source, context_json);
    }
}

#[test]
#[ignore = "replays thousands of corpus inputs; run for coverage"]
fn replay_css_corpus() {
    for css in corpus("css") {
        let _ = bevy_markup::fuzz::cascade(&css);
    }
}

#[test]
#[ignore = "replays thousands of corpus inputs; run for coverage"]
fn replay_ftl_corpus() {
    for text in corpus("ftl") {
        let mut parts = text.split("\n---\n");
        if let (Some(ftl), Some(id), Some(args), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        {
            let _ = bevy_markup::fuzz::translate(ftl, id, args);
        }
    }
}

#[test]
#[ignore = "replays thousands of corpus inputs; run for coverage"]
fn replay_svg_corpus() {
    for svg in corpus("svg") {
        let _ = bevy_markup::fuzz::svg(svg.as_bytes());
    }
}
