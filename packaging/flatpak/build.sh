#!/usr/bin/env bash
# Build the current checkout, including uncommitted changes, on Linux.
set -euo pipefail
if [[ "$(uname -s)" != Linux ]]; then
  echo "Flatpak builds require Linux. Run this script on Fedora or in a Linux VM; see docs/FLATPAK.md." >&2
  exit 1
fi
for tool in flatpak flatpak-builder python3; do
  command -v "$tool" >/dev/null || { echo "Missing $tool; see docs/FLATPAK.md." >&2; exit 1; }
done
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
python3 packaging/flatpak/generate-sources.py --check
mkdir -p dist
flatpak-builder --user --install-deps-from=flathub --force-clean \
  --repo=build-flatpak/repo build-flatpak/app \
  packaging/flatpak/io.github.felsenuboot.Tango.json
# A headless check of the installed executable and its shared libraries.
flatpak-builder --run build-flatpak/app \
  packaging/flatpak/io.github.felsenuboot.Tango.json tango --version
flatpak build-bundle --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo \
  build-flatpak/repo "dist/Tango-$(flatpak --default-arch).flatpak" io.github.felsenuboot.Tango
echo "Bundle created in $ROOT/dist"
