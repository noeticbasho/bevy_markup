#!/bin/sh
# fuzzcheck (its own coverage sensor over `-C instrument-coverage`).
# Usage: scripts/fuzz-fuzzcheck.sh <html|css|ftl> [seconds]
#
# Uses the vendored, patched fuzzcheck (see fuzzcheck/vendor/) and the
# linker override in fuzzcheck/.cargo/config.toml.
set -eu
cd "$(dirname "$0")/.."

case "${1:-}" in
    html|css|ftl) target=$1 ;;
    *) echo "usage: $0 <html|css|ftl> [seconds=60]" >&2; exit 2 ;;
esac
duration=${2:-60}
# CI pins a dated nightly (BEVY_MARKUP_NIGHTLY, see nightly.yml); locally, the
# current nightly.
toolchain=${BEVY_MARKUP_NIGHTLY:-nightly}

cd fuzzcheck
# `--stop-after-first-failure`: fuzzcheck then saves the failing input under
# fuzz/fuzz_<target>/artifacts/ and exits non-zero (without the flag it
# records the failure only in the run's stats and fuzzes on). Count the files before and after (earlier
# runs' failures are not this run's), like the other drivers' scripts.
failures() {
    find "fuzz/fuzz_$target/artifacts" -type f 2>/dev/null | wc -l
}
before=$(failures)
set +e
cargo "+$toolchain" fuzzcheck --test "$target" "fuzz_$target" \
    --stop-after-duration "$duration" --stop-after-first-failure
status=$?
set -e
after=$(failures)
if [ "$after" -gt "$before" ]; then
    echo "fuzzcheck found $((after - before)) new failing input(s) in fuzzcheck/fuzz/fuzz_$target/artifacts/" >&2
    exit 1
fi
exit $status
