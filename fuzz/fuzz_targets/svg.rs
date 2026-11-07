//! Fuzzes the `svg` loader (ADR 0038): roxmltree parse → recursion-depth
//! check → usvg conversion (refusing image resolver) → render-cost
//! estimate → resvg draw, within a 256 px side and a budget scaled to it.
//!
//! Contract: never panics, overflows the stack or aborts; refusals are
//! fine. The call is timed: the budget allows a few milliseconds of
//! drawing, so a slow input means the estimate missed a cost (or a parse
//! step is superlinear) and fails here instead of passing as a mere
//! refusal.
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::time::{Duration, Instant};

fuzz_target!(|data: &[u8]| {
    if data.len() > 64 * 1024 {
        return;
    }
    let started = Instant::now();
    let outcome = bevy_markup::fuzz::svg(data);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "svg took {elapsed:?} ({outcome:?}): the depth check or the cost estimate missed a cost"
    );
});
