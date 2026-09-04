#!/usr/bin/env bash
# Builds the release binary and installs it with the desktop entry and icons for the current user.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
APP=io.github.felsenuboot.Tango
BIN=~/.local/bin
APPS=~/.local/share/applications
ICONS=~/.local/share/icons/hicolor
mkdir -p "$BIN" "$APPS" "$ICONS/scalable/apps" "$ICONS/symbolic/apps"
(cd "$HERE" && cargo build --release)
install -m755 "$HERE/target/release/tango" "$BIN/tango"
# Absolute Exec path: launchers do not necessarily have ~/.local/bin in PATH.
sed "s|^Exec=.*|Exec=$BIN/tango|" "$HERE/data/$APP.desktop" > "$APPS/$APP.desktop"
chmod 644 "$APPS/$APP.desktop"
install -m644 "$HERE/data/icons/hicolor/scalable/apps/$APP.svg" "$ICONS/scalable/apps/$APP.svg"
install -m644 "$HERE/data/icons/hicolor/symbolic/apps/$APP-symbolic.svg" "$ICONS/symbolic/apps/$APP-symbolic.svg"
# Fixed-size PNGs for docks and taskbars that do not rasterise SVG themselves.
if command -v magick >/dev/null 2>&1; then
  for s in 16 22 24 32 48 64 96 128 256 512; do
    mkdir -p "$ICONS/${s}x${s}/apps"
    magick -background none "$HERE/data/icons/hicolor/scalable/apps/$APP.svg" -resize "${s}x${s}" "$ICONS/${s}x${s}/apps/$APP.png"
  done
fi
gtk4-update-icon-cache -q -t -f "$ICONS" 2>/dev/null || gtk-update-icon-cache -q -t -f "$ICONS" 2>/dev/null || true
update-desktop-database -q "$APPS" 2>/dev/null || true
echo "Installed. Run 'tango' or launch Tango from your app launcher."
