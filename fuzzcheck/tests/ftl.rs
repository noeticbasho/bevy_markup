//! Same harnesses as `fuzz/` (libFuzzer) and `honggfuzz/`, driven by
//! fuzzcheck instead. Contract: never panics. Run with `cargo +nightly
//! fuzzcheck --test <name> fuzz_<name> --stop-after-duration 60` from this
//! directory.
use bevy_markup as _bevy_markup;
use fuzzcheck::DefaultMutator;
use fuzzcheck::SerdeSerializer;

mod common;

/// The FTL resource, the message id and the `data-l10n-args` JSON.
type Input = (Vec<u8>, Vec<u8>, Vec<u8>);

#[test]
fn fuzz_ftl() {
    // Three fields, not one delimited buffer as in the byte-oriented
    // drivers: fuzzcheck's `Vec<u8>` mutator almost never produces the two
    // `\n---\n` delimiters, so every input returned before the library.
    let result: fuzzcheck::FuzzingResult<Input> = fuzzcheck::fuzz_test(|input: &Input| {
        let (ftl, id, args) = input;
        if ftl.len() + id.len() + args.len() > 64 * 1024 {
            return;
        }
        let _ = bevy_markup::fuzz::translate(
            &String::from_utf8_lossy(ftl),
            &String::from_utf8_lossy(id),
            &String::from_utf8_lossy(args),
        );
    })
    .mutator(Input::default_mutator())
    .serializer(SerdeSerializer::default())
    .default_sensor_and_pool_with_custom_filter(common::observed)
    .arguments_from_cargo_fuzzcheck()
    .launch();
    common::finish(result);
}
