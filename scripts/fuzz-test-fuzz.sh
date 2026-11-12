#!/bin/sh
# test-fuzz (AFLplus via cargo-afl). Corpus seeds come from ordinary
# `cargo test` runs of the test-fuzz crate.
# Usage: scripts/fuzz-test-fuzz.sh <html|css|ftl|svg> [seconds]
#
# The AFL variables skip the core-pattern and CPU-governor checks, which
# require sudo on this machine.
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

cd test-fuzz
export AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1
export AFL_SKIP_CPUFREQ=1
# A target that was already fuzzed for a while continues that session:
# without this afl-fuzz refuses to start over an output directory holding
# more than 25 minutes of results.
export AFL_AUTORESUME=1
# Build first (unbounded: a cold build can exceed the run budget); the
# timeout then bounds only the run. `timeout` exits 124 when the budget
# elapses — that is a normal, successful end for a fuzz campaign.
cargo "+$toolchain" test-fuzz --no-run "tests::fuzz_$target"
# AFL keeps fuzzing after a crash and records each one in its output's
# `crashes/` directory, so the exit status says nothing about crashes:
# count the files before and after (earlier runs' crashes are not this
# run's; review_0003 G43).
crashes() {
    find target/afl/output -path "*fuzz_$target/*/crashes/*" -type f \
        ! -name README.txt 2>/dev/null | wc -l
}
before=$(crashes)
set +e
timeout "$duration" cargo "+$toolchain" test-fuzz "tests::fuzz_$target"
status=$?
set -e
after=$(crashes)
cd ..
if [ "$after" -gt "$before" ]; then
    echo "AFL found $((after - before)) new crash(es) under test-fuzz/target/afl/output/" >&2
    exit 1
fi
case $status in
    0 | 124) exit 0 ;;
    *) exit $status ;;
esac
