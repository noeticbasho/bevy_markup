#!/bin/sh
# fuzzcheck (its own coverage sensor over `-C instrument-coverage`).
# Usage: scripts/fuzz-fuzzcheck.sh <html|css|ftl|svg> [seconds]
#
# Uses the vendored, patched fuzzcheck (see fuzzcheck/vendor/) and the
# linker override in fuzzcheck/.cargo/config.toml.
set -eu
cd "$(dirname "$0")/.."

case "${1:-}" in
    html|css|ftl|svg) target=$1 ;;
    *) echo "usage: $0 <html|css|ftl|svg> [seconds=60]" >&2; exit 2 ;;
esac
duration=${2:-60}
# CI pins a dated nightly (BEVY_MARKUP_NIGHTLY, see nightly.yml); locally, the
# current nightly.
toolchain=${BEVY_MARKUP_NIGHTLY:-nightly}

cd fuzzcheck
# svg's corpus files are the raw bytes, so the committed cargo-fuzz seeds
# can join it (random bytes almost never parse as XML); files already
# there are kept. The other targets' corpora are fuzzcheck's own JSON.
if [ "$target" = svg ]; then
    mkdir -p fuzz/fuzz_svg/corpus
    cp -n ../fuzz/seeds/svg/* fuzz/fuzz_svg/corpus/ 2>/dev/null || true
fi
# `--stop-after-first-failure`: fuzzcheck then saves the failing input under
# fuzz/fuzz_<target>/artifacts/ and exits non-zero (without the flag it
# records the failure only in the run's stats and fuzzes on). Count the files before and after (earlier
# runs' failures are not this run's), like the other drivers' scripts.
failures() {
    find "fuzz/fuzz_$target/artifacts" -type f 2>/dev/null | wc -l
}
before=$(failures)
# `cargo fuzzcheck` exits 0 even when the test binary failed (a failing
# input without the flag above, or a panic in fuzzcheck itself, U44), so
# the test harness's own verdict is read from the output.
log=$(mktemp)
trap 'rm -f "$log"' EXIT
set +e
cargo "+$toolchain" fuzzcheck --test "$target" "fuzz_$target" \
    --stop-after-duration "$duration" --stop-after-first-failure 2>&1 | tee "$log"
status=0
if grep -aq 'test result: FAILED' "$log"; then
    status=1
fi
if ! grep -aq 'test result:' "$log"; then
    echo "fuzzcheck printed no test result (build error?)" >&2
    status=1
fi
set -e
after=$(failures)
if [ "$after" -gt "$before" ]; then
    echo "fuzzcheck found $((after - before)) new failing input(s) in fuzzcheck/fuzz/fuzz_$target/artifacts/" >&2
    exit 1
fi
exit $status
