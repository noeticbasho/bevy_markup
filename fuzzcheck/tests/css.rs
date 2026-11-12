//! Same harnesses as `fuzz/` (libFuzzer) and `honggfuzz/`, driven by
//! fuzzcheck instead. Contract: never panics. Run with `cargo +nightly
//! fuzzcheck --test <name> fuzz_<name> --stop-after-duration 60` from this
//! directory.
use bevy_markup as _bevy_markup;
use fuzzcheck::DefaultMutator;
use fuzzcheck::SerdeSerializer;

mod common;

#[test]
fn fuzz_css() {
    let result = fuzzcheck::fuzz_test(|data: &Vec<u8>| {
        if data.len() > 64 * 1024 {
            return;
        }
        let css = String::from_utf8_lossy(data);
        let _ = bevy_markup::fuzz::cascade(&css);
    })
    .mutator(Vec::<u8>::default_mutator())
    .serializer(SerdeSerializer::default())
    .default_sensor_and_pool_with_custom_filter(common::observed)
    .arguments_from_cargo_fuzzcheck()
    .launch();
    common::finish(result);
}
