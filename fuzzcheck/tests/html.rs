//! Same harnesses as `fuzz/` (libFuzzer) and `honggfuzz/`, driven by
//! fuzzcheck instead. Contract: never panics. Run with `cargo +nightly
//! fuzzcheck --test <name> fuzz_<name> --stop-after-duration 60` from this
//! directory.
use bevy_markup as _bevy_markup;
use fuzzcheck::DefaultMutator;
use fuzzcheck::SerdeSerializer;

mod common;

/// The template source and the context JSON.
type Input = (Vec<u8>, Vec<u8>);

#[test]
fn fuzz_html() {
    // Two fields, not one delimited buffer as in the byte-oriented
    // drivers: fuzzcheck's `Vec<u8>` mutator almost never produces the
    // `\n---\n` delimiter, so the context stayed empty.
    let _: fuzzcheck::FuzzingResult<Input> = fuzzcheck::fuzz_test(|input: &Input| {
        let (source, context) = input;
        if source.len() + context.len() > 64 * 1024 {
            return;
        }
        let context = String::from_utf8_lossy(context);
        // An empty second field: render with an empty context.
        let context = if context.is_empty() { "{}" } else { &*context };
        let _ = bevy_markup::fuzz::render_html(&String::from_utf8_lossy(source), context);
    })
    .mutator(Input::default_mutator())
    .serializer(SerdeSerializer::default())
    .default_sensor_and_pool_with_custom_filter(common::observed)
    .arguments_from_cargo_fuzzcheck()
    .launch();
}
