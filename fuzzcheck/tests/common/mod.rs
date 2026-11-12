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
