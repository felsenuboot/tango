#!/usr/bin/env bash
# Build the current checkout, including uncommitted changes, on Linux.
set -euo pipefail
if [[ "$(uname -s)" != Linux ]]; then
  echo "Flatpak builds require Linux. Run this script on Fedora or in a Linux VM; see docs/FLATPAK.md." >&2
  exit 1
fi
for tool in flatpak python3; do
  command -v "$tool" >/dev/null || { echo "Missing $tool; see docs/FLATPAK.md." >&2; exit 1; }
done
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
python3 packaging/flatpak/generate-sources.py --check
# Use the same modern builder on Fedora and older CI hosts. GNOME 49 no
# longer provides the appstream-compose executable expected by builder 1.2.
BUILDER=(flatpak run --user org.flatpak.Builder//stable)
"${BUILDER[@]}" --version
flatpak info --user --show-commit org.flatpak.Builder//stable
# Fail before compiling if the SDK cannot compose AppStream metadata.
flatpak run --user --command=appstreamcli org.gnome.Sdk//49 compose --help >/dev/null
mkdir -p dist
BUNDLE="dist/Tango-$(flatpak --default-arch).flatpak"
# Never leave an older bundle looking like the output of a failed build.
rm -f "$BUNDLE"
"${BUILDER[@]}" --user --install-deps-from=flathub --force-clean \
  --repo=build-flatpak/repo build-flatpak/app \
  packaging/flatpak/io.github.felsenuboot.Tango.json
# A headless check of the installed executable and its shared libraries.
"${BUILDER[@]}" --run build-flatpak/app \
  packaging/flatpak/io.github.felsenuboot.Tango.json tango --version
flatpak build-bundle --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo \
  build-flatpak/repo "$BUNDLE" io.github.felsenuboot.Tango
test -s "$BUNDLE"
echo "Bundle created in $ROOT/dist"
