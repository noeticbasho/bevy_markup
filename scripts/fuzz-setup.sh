#!/bin/sh
# Installs one fuzz driver's tools (unless already installed) and builds its
# three targets, so `scripts/fuzz-<driver>.sh` only has to run them.
# Usage: scripts/fuzz-setup.sh <honggfuzz|fuzzcheck|test-fuzz>
#
# CI runs it twice: cache.yml builds each driver's cache with it, and the
# nightly jobs call it again after restoring that cache (then it compiles
# nothing). Locally it is optional: the tools are installed once by hand
# and the run scripts build what they need. cargo-fuzz has no entry here:
# its jobs install the prebuilt binary.
#
# The tool versions are the ones the drivers were verified with; a
# vendored crate (honggfuzz, fuzzcheck) must match its cargo subcommand.
set -eu
cd "$(dirname "$0")/.."

driver=${1:-}
toolchain=${BEVY_MARKUP_NIGHTLY:-nightly}

# install <binary> <crate> <version>
install() {
    if command -v "$1" >/dev/null 2>&1; then
        return
    fi
    cargo "+$toolchain" install --locked "$2" --version "$3"
}

case "$driver" in
    honggfuzz)
        install cargo-hfuzz honggfuzz 0.5.62
        # = scripts/fuzz-honggfuzz.sh
        export HFUZZ_BUILD_ARGS='--manifest-path honggfuzz/Cargo.toml --config target.x86_64-unknown-linux-gnu.linker="cc"'
        for target in html css ftl; do
            cargo "+$toolchain" hfuzz build --bin "$target"
        done
        ;;
    fuzzcheck)
        install cargo-fuzzcheck cargo-fuzzcheck 0.13.0
        # cargo-fuzzcheck has no build-only mode: a one-second run builds.
        cd fuzzcheck
        for target in html css ftl; do
            cargo "+$toolchain" fuzzcheck --test "$target" "fuzz_$target" \
                --stop-after-duration 1
        done
        ;;
    test-fuzz)
        install cargo-afl cargo-afl 0.18.2
        install cargo-test-fuzz cargo-test-fuzz 8.1.1
        # AFL++ itself, built per rustc version into the user's data
        # directory; `--build` fails when it is already there.
        cargo "+$toolchain" afl config --build 2>&1 || true
        cd test-fuzz
        # The corpus seeds: `#[test_fuzz]` records the arguments of
        # ordinary test runs under target/corpus.
        cargo "+$toolchain" test
        for target in html css ftl; do
            cargo "+$toolchain" test-fuzz --no-run "tests::fuzz_$target"
        done
        ;;
    *)
        echo "usage: $0 <honggfuzz|fuzzcheck|test-fuzz>" >&2
        exit 2
        ;;
esac
