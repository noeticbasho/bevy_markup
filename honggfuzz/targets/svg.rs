//! Same harness as `fuzz/` (libFuzzer), driven by honggfuzz instead: the
//! `svg` loader, timed (`bevy_markup::fuzz::svg_target`; hfuzz builds with
//! debug assertions, so the known upstream debug panic is excused there
//! as in the cargo-fuzz target).
//! Contract: never panics. Build+run with
//! `cargo +nightly hfuzz run svg` (from the repo root).
fn main() {
    loop {
        honggfuzz::fuzz!(|data: &[u8]| {
            bevy_markup::fuzz::svg_target(data);
        });
    }
}
