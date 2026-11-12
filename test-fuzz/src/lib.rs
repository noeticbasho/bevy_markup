//! The same harnesses as `fuzz/` (libFuzzer), `honggfuzz/` and
//! `fuzzcheck/`, as test-fuzz targets. test-fuzz seeds its corpus from
//! ordinary `cargo test` runs, then drives them with AFLplus:
//!
//! ```sh
//! cargo test                                     # from this directory: seeds the corpus
//! cargo +nightly test-fuzz fuzz_html             # target-name substring (also: css, ftl, svg)
//! cargo +nightly test-fuzz fuzz_html --max-total-time 60
//! ```
//!
//! Contract: never panics. (Needs `cargo install cargo-test-fuzz cargo-afl`
//! and a nightly toolchain for AFL instrumentation.)
#[cfg(test)]
mod tests {
    // test-fuzz writes the deserialized argument through the target; keep
    // the byte-level interface of the other drivers.
    #[test_fuzz::test_fuzz]
    fn fuzz_html(x: Vec<u8>) {
        if x.len() > 64 * 1024 {
            return;
        }
        let text = String::from_utf8_lossy(&x);
        let (source, context_json) = match text.split_once("\n---\n") {
            Some((source, context)) => (source, context),
            // No delimiter: render with an empty context.
            None => (text.as_ref(), "{}"),
        };
        let _ = bevy_markup::fuzz::render_html(source, context_json);
    }

    #[test_fuzz::test_fuzz]
    fn fuzz_css(x: Vec<u8>) {
        if x.len() > 64 * 1024 {
            return;
        }
        let css = String::from_utf8_lossy(&x);
        let _ = bevy_markup::fuzz::cascade(&css);
    }

    #[test_fuzz::test_fuzz]
    fn fuzz_ftl(x: Vec<u8>) {
        if x.len() > 64 * 1024 {
            return;
        }
        let text = String::from_utf8_lossy(&x);
        let mut parts = text.split("\n---\n");
        let (Some(ftl), Some(id), Some(args), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return;
        };
        let _ = bevy_markup::fuzz::translate(ftl, id, args);
    }

    #[test_fuzz::test_fuzz]
    fn fuzz_svg(x: Vec<u8>) {
        bevy_markup::fuzz::svg_target(&x);
    }

    /// Seeds `fuzz_svg`'s corpus: random bytes almost never parse as XML,
    /// so the committed cargo-fuzz seeds run through the target once
    /// (`#[test_fuzz]` records each call's argument under target/corpus).
    #[test]
    fn seed_svg() {
        let seeds = concat!(env!("CARGO_MANIFEST_DIR"), "/../fuzz/seeds/svg");
        let mut count = 0;
        for entry in std::fs::read_dir(seeds).expect("fuzz/seeds/svg") {
            let bytes = std::fs::read(entry.expect("a seed").path()).expect("a seed's bytes");
            fuzz_svg(bytes);
            count += 1;
        }
        assert!(count > 0, "no seeds in {seeds}");
    }
}
