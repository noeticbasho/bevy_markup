#!/bin/sh
# Golden images (tests/golden.rs, `#[ignore]`d in plain `cargo test`):
# renders the tests/golden/<scene>/ pages offscreen with Mesa's lavapipe
# (software Vulkan, bit-identical run to run) and compares them with
# tests/golden/<scene>/expected.png. Failures write the actual image and a
# diff under target/tmp/golden/.
# Usage: scripts/golden.sh [--update]
#   --update  rewrite the references (look at the PNGs, then commit them)
#
# lavapipe: an installed ICD (Arch `vulkan-swrast`, Debian/Ubuntu
# `mesa-vulkan-drivers`) is used if present. Otherwise, on Arch, the
# vulkan-swrast package matching the installed `mesa` is downloaded once
# from archive.archlinux.org (HTTPS) into target/golden-lavapipe/ — no root
# needed, it uses the system's libLLVM. A preset VK_DRIVER_FILES wins (e.g.
# a hardware ICD from /usr/share/vulkan/icd.d/; the tolerance in
# tests/golden.rs covers the NVIDIA and RADV drivers it was measured on).
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"

if [ -z "${VK_DRIVER_FILES:-}" ]; then
    icd=
    for f in /usr/share/vulkan/icd.d/lvp_icd*.json; do
        [ -e "$f" ] && icd=$f && break
    done
    if [ -z "$icd" ]; then
        if ! version=$(pacman -Q mesa 2>/dev/null); then
            echo "golden: no lavapipe Vulkan driver found; install Mesa's (vulkan-swrast /" >&2
            echo "mesa-vulkan-drivers) or point VK_DRIVER_FILES at another ICD json" >&2
            exit 1
        fi
        version=${version#mesa }
        # No ':' in the path (epoch "1:"): VK_DRIVER_FILES is ':'-separated.
        cache="$root/target/golden-lavapipe/$(echo "$version" | tr : _)"
        if [ ! -e "$cache/lvp_icd.json" ]; then
            name="vulkan-swrast-$(echo "$version" | sed 's/:/%3A/')-$(uname -m).pkg.tar.zst"
            echo "golden: fetching $name" >&2
            mkdir -p "$cache"
            curl -sSfL "https://archive.archlinux.org/packages/v/vulkan-swrast/$name" \
                | tar --zstd -x -C "$cache" usr/lib/libvulkan_lvp.so usr/share/vulkan/icd.d/lvp_icd.json
            # The packaged ICD names the library relative to the system path.
            sed "s|\"library_path\": *\"[^\"]*\"|\"library_path\": \"$cache/usr/lib/libvulkan_lvp.so\"|" \
                "$cache/usr/share/vulkan/icd.d/lvp_icd.json" > "$cache/lvp_icd.json"
        fi
        icd="$cache/lvp_icd.json"
    fi
    export VK_DRIVER_FILES="$icd"
fi
export WGPU_BACKEND=vulkan
unset WGPU_ADAPTER_NAME
if [ "${1:-}" = --update ]; then
    export BEVY_MARKUP_UPDATE_GOLDEN=1
fi
cd "$root"
exec cargo test --features svg --test golden -- --ignored --nocapture
