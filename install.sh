#!/usr/bin/env bash
# Installs Tango.
#   Arch Linux: builds the tango-git package from the committed state of this checkout
#               (packaging/arch/PKGBUILD) and installs it with pacman.
#   Elsewhere:  release build into ~/.local/bin with the desktop entry and icons for this user.
#   --user:     that per-user install on Arch too: no sudo, no package. Docks match a window's
#               app id against installed desktop entries, so this also gives `cargo run`
#               windows their icon instead of a generic one (#81).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
APP=io.github.felsenuboot.Tango
USER_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --user) USER_ONLY=1 ;;
    *) echo "usage: $0 [--user]" >&2; exit 2 ;;
  esac
done

if [ "$USER_ONLY" = 0 ] && command -v makepkg >/dev/null 2>&1 && [ -r /etc/arch-release ]; then
  echo "Arch Linux: building the tango-git package from the committed state of $HERE"
  echo "(uncommitted changes are not part of it; commit or stash first if you need them)."
  cd "$HERE/packaging/arch"
  TANGO_GIT_URL="file://$HERE" makepkg --syncdeps --install --force
  exit 0
fi

BIN=~/.local/bin
APPS=~/.local/share/applications
ICONS=~/.local/share/icons/hicolor
METAINFO=~/.local/share/metainfo
mkdir -p "$BIN" "$APPS" "$ICONS/scalable/apps" "$ICONS/symbolic/apps" "$METAINFO"
(cd "$HERE" && cargo build --release)
install -m755 "$HERE/target/release/tango" "$BIN/tango"
# Absolute Exec path: launchers do not necessarily have ~/.local/bin in PATH.
sed "s|^Exec=.*|Exec=$BIN/tango %U|" "$HERE/data/$APP.desktop" > "$APPS/$APP.desktop"
chmod 644 "$APPS/$APP.desktop"
install -m644 "$HERE/data/$APP.metainfo.xml" "$METAINFO/$APP.metainfo.xml"
install -m644 "$HERE/data/icons/hicolor/scalable/apps/$APP.svg" "$ICONS/scalable/apps/$APP.svg"
install -m644 "$HERE/data/icons/hicolor/symbolic/apps/$APP-symbolic.svg" "$ICONS/symbolic/apps/$APP-symbolic.svg"
# Fixed-size PNGs for docks and taskbars that do not rasterise SVG themselves.
if command -v rsvg-convert >/dev/null 2>&1; then
  for s in 16 22 24 32 48 64 96 128 256 512; do
    mkdir -p "$ICONS/${s}x${s}/apps"
    rsvg-convert -w "$s" -h "$s" -o "$ICONS/${s}x${s}/apps/$APP.png" "$HERE/data/icons/hicolor/scalable/apps/$APP.svg"
  done
fi
gtk4-update-icon-cache -q -t -f "$ICONS" 2>/dev/null || gtk-update-icon-cache -q -t -f "$ICONS" 2>/dev/null || true
update-desktop-database -q "$APPS" 2>/dev/null || true
echo "Installed. Run 'tango' or launch Tango from your app launcher."
