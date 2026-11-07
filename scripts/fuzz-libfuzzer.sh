#!/bin/sh
# cargo-fuzz (libFuzzer + AddressSanitizer) over bevy_markup::fuzz harnesses.
# Usage: scripts/fuzz-libfuzzer.sh <html|css|ftl|svg> [seconds] [libFuzzer flags...]
set -eu
cd "$(dirname "$0")/.."

case "${1:-}" in
    html|css|ftl|svg) target=$1 ;;
    *) echo "usage: $0 <html|css|ftl|svg> [seconds=60]" >&2; exit 2 ;;
esac
duration=${2:-60}
shift $(( $# < 2 ? $# : 2 ))
# CI pins a dated nightly (BEVY_MARKUP_NIGHTLY, see nightly.yml) so its build caches
# survive across days; locally, the current nightly.
toolchain=${BEVY_MARKUP_NIGHTLY:-nightly}

# cargo-fuzz defaults --target to the triple *it* was built for; prebuilt
# binaries (e.g. CI's install-action) are musl, which ASan can't use.
# `-timeout=10` bounds one input (libFuzzer's 1200 s default is longer
# than a whole nightly campaign, so a hang could never surface —
# review_0003 G21); a later -timeout in "$@" overrides it.
host=$(rustc "+$toolchain" -vV | sed -n 's/^host: //p')

# The working corpus (gitignored; in CI carried over between nights) comes
# first: libFuzzer writes new inputs only there. The committed seeds
# (fuzz/seeds/<target>, minimized) are read-only extra input.
mkdir -p "fuzz/corpus/$target"
exec cargo "+$toolchain" fuzz run --target "$host" "$target" \
    "fuzz/corpus/$target" "fuzz/seeds/$target" -- \
    -max_total_time="$duration" -max_len=65536 -timeout=10 "$@"
