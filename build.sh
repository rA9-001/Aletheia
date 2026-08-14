#!/usr/bin/env bash
#
# Build Aletheia installers (.deb / .rpm / .AppImage).
#
# Wrapper around `cargo tauri build` that sets NO_STRIP=true. On bleeding-edge
# distros (Arch/CachyOS) the old `strip` bundled inside linuxdeploy cannot parse
# the modern `.relr.dyn` ELF section that newer binutils emit, which otherwise
# aborts the AppImage step. Setting NO_STRIP skips stripping and lets all three
# bundles build.
#
# Usage:
#   ./build.sh                      # build all bundle types
#   ./build.sh --bundles appimage   # only the AppImage
#   ./build.sh --debug              # debug build
# Any extra arguments are passed straight through to `cargo tauri build`.

set -euo pipefail

cd "$(dirname "$0")/src-tauri"

echo "▶ cargo tauri build (NO_STRIP=true) ${*:-}"
NO_STRIP=true exec cargo tauri build "$@"
