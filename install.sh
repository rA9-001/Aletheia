#!/usr/bin/env bash
#
# Install Aletheia for the current user (no root needed).
#
# Installs the release binary, its icons, and a .desktop launcher so the app
# appears in your application menu with its proper icon — handy on Arch/CachyOS
# where the generated .deb/.rpm aren't native. Run ./build.sh first.
#
# Uninstall with:  ./install.sh --uninstall

set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
BIN_SRC="$ROOT/src-tauri/target/release/aletheia"
ICON_DIR_SRC="$ROOT/src-tauri/icons"

BIN_DST="$HOME/.local/bin/aletheia"
DESKTOP="$HOME/.local/share/applications/aletheia.desktop"
ICON_BASE="$HOME/.local/share/icons/hicolor"

if [[ "${1:-}" == "--uninstall" ]]; then
  rm -f "$BIN_DST" "$DESKTOP"
  for s in 32 128 512; do rm -f "$ICON_BASE/${s}x${s}/apps/aletheia.png"; done
  command -v update-desktop-database >/dev/null && update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
  echo "✓ Uninstalled Aletheia."
  exit 0
fi

if [[ ! -x "$BIN_SRC" ]]; then
  echo "Release binary not found at:"
  echo "  $BIN_SRC"
  echo "Build it first:  ./build.sh    (or: cd src-tauri && cargo build --release)"
  exit 1
fi

# Binary
install -Dm755 "$BIN_SRC" "$BIN_DST"

# Icons (map Tauri's generated sizes to hicolor)
declare -A ICONS=( [32]="32x32.png" [128]="128x128.png" [512]="icon.png" )
for size in "${!ICONS[@]}"; do
  src="$ICON_DIR_SRC/${ICONS[$size]}"
  [[ -f "$src" ]] && install -Dm644 "$src" "$ICON_BASE/${size}x${size}/apps/aletheia.png"
done

# Desktop launcher (StartupWMClass must match the app's WM_CLASS: aletheia)
mkdir -p "$(dirname "$DESKTOP")"
cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=Aletheia
Comment=Local browser privacy audit: logged-in sites, saved-credential sites, cache usage
Exec=$BIN_DST
Icon=aletheia
Terminal=false
Categories=Utility;Security;
StartupWMClass=aletheia
EOF

command -v update-desktop-database >/dev/null && update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -f -t "$ICON_BASE" 2>/dev/null || true

echo "✓ Installed Aletheia."
echo "  Launch it from your app menu, or run: $BIN_DST"
echo "  (If ~/.local/bin is on your PATH, just: aletheia)"
