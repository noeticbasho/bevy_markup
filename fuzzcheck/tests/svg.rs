//! Same harness as `fuzz/` (libFuzzer) and `honggfuzz/`, driven by
//! fuzzcheck instead: the `svg` loader, timed
//! (`bevy_markup::fuzz::svg_target`). Contract: never panics. Run with
//! `scripts/fuzz-fuzzcheck.sh svg`, which copies the committed seeds
//! (`fuzz/seeds/svg`) into the corpus: the corpus files are the raw bytes
//! (`ByteSerializer`), as in the byte-oriented drivers.
use bevy_markup as _bevy_markup;
use fuzzcheck::ByteSerializer;
use fuzzcheck::DefaultMutator;

mod common;

#[test]
fn fuzz_svg() {
    let result = fuzzcheck::fuzz_test(|data: &Vec<u8>| {
        bevy_markup::fuzz::svg_target(data);
    })
    .mutator(Vec::<u8>::default_mutator())
    .serializer(ByteSerializer::new("svg"))
    .default_sensor_and_pool_with_custom_filter(common::observed)
    .arguments_from_cargo_fuzzcheck()
    .launch();
    common::finish(result);
}
