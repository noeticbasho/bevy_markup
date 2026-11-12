//! Shared by the three fuzzcheck harnesses.
use std::path::Path;

/// Whether fuzzcheck's coverage sensor observes `file`: bevy_markup's own
/// source.
///
/// `default_options()` observes only files below the current directory —
/// this crate, i.e. the harness closures alone (about 50 counters) — so the
/// fuzzer got no feedback from the library and mutated blind.
pub fn observed(file: &Path, _function: &str) -> bool {
    let library = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the fuzzcheck crate lives in the repository")
        .join("src");
    file.starts_with(library)
}

/// Fails the test when the run found a failing input. `launch()` only
/// returns the result: discarded, a run that printed `failures(1)` still
/// passed and `cargo fuzzcheck` exited 0.
pub fn finish<T>(result: fuzzcheck::FuzzingResult<T>) {
    assert!(
        !result.found_test_failure,
        "fuzzcheck found a failing input (see fuzz/<test>/artifacts/ or the run's stats)"
    );
}
