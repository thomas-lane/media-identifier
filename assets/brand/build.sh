#!/usr/bin/env bash
# Rebuild every published brand file from the SVG sources in assets/brand.
#
#   assets/brand/build.sh icons      app icons in src-tauri/icons and assets/brand/icon-1024.png
#   assets/brand/build.sh graphics   README header and documentation illustrations in docs/images
#   assets/brand/build.sh all        both
#
# icons needs: rsvg-convert (brew install librsvg), oxipng (brew install oxipng), python3 with
# Pillow, and the UI dependencies (npm --prefix ui ci) for the Tauri icon generator.
# graphics needs: INTER_DIR pointing at the extras/otf folder of the Inter 4.1 release, and
# BRAND_PYTHON (default python3) with the fonttools and uharfbuzz packages.
# See assets/brand/README.md for details.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BRAND="$ROOT/assets/brand"
ICONS="$ROOT/src-tauri/icons"
IMAGES="$ROOT/docs/images"

build_icons() {
  local tmp
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' RETURN

  # 1. The 1024 px master: macOS-style tile with transparent margin.
  rsvg-convert -w 1024 "$BRAND/icon.svg" -o "$BRAND/icon-1024.png"

  # 2. Tauri's generator makes the macOS .icns and the PNG sizes from the master.
  (cd "$ROOT" && npm --prefix ui exec -- tauri icon "$BRAND/icon-1024.png" -o "$tmp/tauri" >/dev/null 2>&1)
  for f in 64x64.png 128x128.png 128x128@2x.png icon.png icon.icns; do
    cp "$tmp/tauri/$f" "$ICONS/$f"
  done

  # 3. Windows and small sizes. Windows shows icons edge to edge, so the .ico uses the tile
  #    without the macOS margin; 16-32 px use the simplified small-size drawing, whose strokes
  #    sit on whole pixels.
  sed 's/viewBox="0 0 1024 1024"/viewBox="100 100 824 824"/' "$BRAND/icon.svg" >"$tmp/full-bleed.svg"
  for s in 16 24 32; do rsvg-convert -w "$s" "$BRAND/icon-small.svg" -o "$tmp/ico-$s.png"; done
  for s in 48 64 256; do rsvg-convert -w "$s" "$tmp/full-bleed.svg" -o "$tmp/ico-$s.png"; done
  python3 - "$tmp" "$ICONS/icon.ico" <<'PY'
import sys
from PIL import Image
tmp, out = sys.argv[1], sys.argv[2]
sizes = [16, 24, 32, 48, 64, 256]
images = [Image.open(f"{tmp}/ico-{s}.png").convert("RGBA") for s in sizes]
images[-1].save(out, format="ICO", sizes=[(s, s) for s in sizes], append_images=images[:-1])
PY
  cp "$tmp/ico-32.png" "$ICONS/32x32.png"

  oxipng -q -o max --strip safe "$BRAND/icon-1024.png" "$ICONS"/*.png
}

build_graphics() {
  : "${INTER_DIR:?set INTER_DIR to the extras/otf folder of the Inter 4.1 release}"
  local py=${BRAND_PYTHON:-python3}
  local tool="$BRAND/tools/outline_text.py"
  mkdir -p "$IMAGES"
  "$py" "$tool" --fonts "$INTER_DIR" --set TEXT='#1d2430' "$BRAND/src/header.svg" "$IMAGES/header-light.svg"
  "$py" "$tool" --fonts "$INTER_DIR" --set TEXT='#e7eaee' "$BRAND/src/header.svg" "$IMAGES/header-dark.svg"
  "$py" "$tool" --fonts "$INTER_DIR" "$BRAND/src/identification.svg" "$IMAGES/identification.svg"
}

case "${1:-all}" in
  icons) build_icons ;;
  graphics) build_graphics ;;
  all) build_icons; build_graphics ;;
  *) echo "usage: $0 [icons|graphics|all]" >&2; exit 2 ;;
esac
