# Brand assets

The app icon, the README header and the documentation illustrations are drawn as SVG sources in
this folder. `build.sh` turns them into the files the app and the documentation use. Edit the
sources and rebuild; never edit the outputs by hand.

## Files

| File | What it is |
|---|---|
| `icon.svg` | The app icon master: a 1024 × 1024 canvas with an 824 × 824 tile and a 100 px transparent margin, the macOS icon grid |
| `icon-small.svg` | The icon redrawn for 16–32 px: edge-to-edge tile, three bars, strokes on whole pixels at 16 and 32 px |
| `icon-1024.png` | `icon.svg` rendered at 1024 px; the input to Tauri's icon generator (built) |
| `src/header.svg` | README header source: the icon and the name "Media Identifier" |
| `src/identification.svg` | Source of the "how a file is identified" figure in `docs/identification.md` and `README.md` |
| `tools/outline_text.py` | Converts a source SVG's `<text>` into glyph outlines (see [Graphics](#graphics)) |
| `build.sh` | Rebuilds everything below from the sources |
| `concepts/` | Reduced copies of the Codex-generated concept images the icon was redrawn from; not used by the app |

Built outputs:

| Output | Built from | Used by |
|---|---|---|
| `src-tauri/icons/icon.icns` | `icon-1024.png` via `tauri icon` | the macOS app bundle, Finder and the Dock |
| `src-tauri/icons/icon.png`, `64x64.png`, `128x128.png`, `128x128@2x.png` | `icon-1024.png` via `tauri icon` | the window icon and bundles that take PNG icons |
| `src-tauri/icons/icon.ico` | 16, 24, 32 px from `icon-small.svg`; 48, 64, 256 px from `icon.svg` without its margin | the Windows executable, taskbar and NSIS installer |
| `src-tauri/icons/32x32.png` | `icon-small.svg` at 32 px | bundles that take a 32 px PNG |
| `docs/images/header-light.svg`, `header-dark.svg` | `src/header.svg` with dark or light text | the README header |
| `docs/images/identification.svg` | `src/identification.svg` | `docs/identification.md`, `README.md` |

`src-tauri/tauri.conf.json` (`bundle.icon`) lists the icon files the bundler reads; the file
names above must stay as they are.

## The icon

A white TV frame holds a sound waveform, and a magnifying glass overlaps its lower right corner:
the app listens to video files and identifies them. The tile is the UI's accent blue
(`#2f6fde`), drawn as a gradient from `#3a7bf0` to `#1f56b8`. White on a saturated blue tile
reads against both light and dark desktops, docks and taskbars, so one icon serves both themes.

The magnifier is cut out of the frame and bars with an SVG mask rather than drawn over a blue
ring, because a solid ring would not match the gradient behind it.

Two drawings are needed because the five-bar master turns into a smear at 16 and 24 px and is
soft at 32 px.
`icon-small.svg` keeps the same frame, bars and magnifier with fewer, heavier shapes, placed on a
16 px grid (64 units = 1 px at 16 px), so the strokes land on whole pixels at 16 and 32 px.

macOS icons keep a transparent margin around the tile because macOS draws every app icon on the
same grid; without it the icon would look larger than its neighbours in the Dock. Windows icons
fill their square, so `icon.ico` is built from the tile without the margin.

## Graphics

The README header and the identification figure are shown as images by GitHub, editors and
browsers on any operating system. Live SVG `<text>` would be drawn with whatever fonts the viewer
has, so widths and highlight boxes would drift. `tools/outline_text.py` therefore replaces each
`<text>` element with the outlines of the glyphs in one pinned font, Inter 4.1 (SIL Open Font
License 1.1), shaped with HarfBuzz so kerning matches normal text rendering. Each glyph is stored
once in `<defs>` and placed with `<use>`, which keeps the figure near 65 KB. The tool's docstring
lists the markup it understands: `<text>` with `<tspan>` runs, `data-mark-class` highlight boxes
behind a run, and `<image data-inline>` to embed another SVG such as the icon.

Light and dark:

- The header has a transparent background, so it is built twice (dark text and light text) and
  the README chooses one with `<picture>` and `prefers-color-scheme`.
- The identification figure carries its own background and switches its whole palette with a
  `prefers-color-scheme` media query inside the SVG, so it stays legible even when the page's
  theme and the system theme differ.

The colors are the UI's tokens from the approved mockup: text `#1d2430` / `#e7eaee`, muted
`#5b6575` / `#a2aab6`, accent `#2f6fde` / `#6ea1ff`, panels `#f6f7f9` / `#24282e`, and the
warning colors for the "Check" verdict.

The identification figure follows the steps in `docs/identification.md` using the Schoolhouse
Rock sample from the UI mockup (`title_t11.mkv`, "Lucky Seven Sampson", chapter 11 of the
play-all). Its percentages and bars are sample values, not measured results. When the matching
steps or signals change, update `src/identification.svg` and rebuild.

## Rebuilding

Prerequisites (macOS):

```bash
brew install librsvg oxipng                      # rsvg-convert renders SVG; oxipng compresses PNG losslessly
npm --prefix ui ci                               # provides the Tauri CLI used for `tauri icon`
python3 -c "import PIL"                          # Pillow writes icon.ico; install it with pip if missing
python3 -m venv /tmp/mi-brand && /tmp/mi-brand/bin/pip install fonttools uharfbuzz
curl -L -o /tmp/inter.zip https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip
unzip -q /tmp/inter.zip -d /tmp/inter
```

Then, from the repository root:

```bash
assets/brand/build.sh icons                      # icon-1024.png and src-tauri/icons
INTER_DIR=/tmp/inter/extras/otf BRAND_PYTHON=/tmp/mi-brand/bin/python \
  assets/brand/build.sh graphics                 # docs/images
```

`build.sh all` runs both. Rebuilding without source changes reproduces every output byte for byte except
`icon.icns`, whose bytes `tauri icon` varies from run to run although the images inside are the
same. `tauri icon` also writes Android, iOS and Windows Store images; the
script builds into a temporary folder and copies only the files listed above, because the app
ships only for macOS and Windows desktop.

## How the artwork was generated

The icon concept came from the image generation tool of the Codex CLI (codex-cli 0.160.0,
model `gpt-6-luna`, 2026-10-05), run non-interactively from an empty folder:

```bash
codex exec --skip-git-repo-check -s workspace-write -C <folder> - < prompt.txt
```

Codex saves generated images under `~/.codex/generated_images/<session>/` and, as the prompts
ask, copied each into the working folder. The prompts, verbatim:

**Concept A** (`concepts/codex-logo-a.png`):

> Use your image generation tool to create ONE app icon image, then save the resulting PNG file
> into the current working directory as logo-a.png (copy it from wherever the tool stores it).
> Do not write any code to draw it; use the image generation tool.
>
> Image description (app icon for "Media Identifier", a desktop app that listens to TV episode
> files and identifies them):
> A macOS-style app icon, 1024x1024, square canvas with a rounded-square (squircle) tile filling
> about 82% of the canvas, centered, with transparent or plain white margin around the tile.
> Tile: solid deep blue (#2f6fde) with a very subtle top-to-bottom gradient to a slightly darker
> blue (#1f56b8).
> Symbol in white, centered: a simple TV screen outline (rounded rectangle with a short stand)
> and inside the screen a bold audio waveform of 5 vertical rounded bars of varying heights; a
> small magnifying glass overlapping the bottom-right corner of the screen.
> Flat vector style, thick strokes, very simple geometry so it stays legible at 16 pixels. No
> text, no letters, no numbers, no extra decoration, no shadows except an optional very soft one
> under the tile.

**Concept B** (`concepts/codex-logo-b.png`), the one the icon follows:

> Use your image generation tool to create ONE app icon image, then save the resulting PNG file
> into the current working directory as logo-b.png (copy it from wherever the tool stores it).
> Do not write any code to draw it; use the image generation tool.
>
> Image description (app icon for "Media Identifier", a desktop app that listens to TV episode
> files and identifies them):
> A macOS-style app icon, 1024x1024. A rounded-square (squircle) tile filling about 82% of the
> canvas, centered, on a plain white margin.
> Tile: deep blue (#2f6fde) with a subtle vertical gradient to #1f56b8.
> Symbol in white, centered and large: a widescreen TV / film frame drawn as a thick
> rounded-rectangle outline (no stand), with a bold audio waveform of 5 rounded vertical bars
> inside it, symmetric, tallest in the middle. Below-right, a bold magnifying glass whose lens
> ring overlaps the frame corner, separated from the frame by a thin gap of tile blue.
> Flat vector style, very thick strokes, minimal geometry so it stays legible at 16 pixels. No
> text, no letters, no numbers, no shadows, no gloss.

**Header concept** (`concepts/codex-header-a.png`):

> Use your image generation tool to create ONE wide banner image, then save the resulting PNG
> file into the current working directory as header-a.png (copy it from wherever the tool
> stores it). Do not write any code to draw it; use the image generation tool.
>
> Image description: a clean, wide (about 4:1) README header banner for an open-source desktop
> app called "Media Identifier". Light background (#f6f7f9). On the left, a blue (#2f6fde)
> rounded-square app icon with a white TV screen outline containing an audio waveform of 5
> vertical bars and a small magnifying glass at the bottom right. To the right of the icon, the
> words "Media Identifier" in a clean bold sans-serif typeface, dark text (#1d2430). Flat,
> minimal, plenty of whitespace, no other text, no slogans.

The generated images are 1254 px rasters on an opaque white background with uneven stroke
weights, so they cannot serve as icons directly: the icon needs transparency, exact symmetry and
strokes tuned for small sizes. `icon.svg` redraws concept B by hand; concept B was chosen over
A because the stand under the screen adds detail that disappears at small sizes. The generated
header spelled the name correctly, but its icon differs from the app icon (it added antennas)
and it has no dark version, so `src/header.svg` composes the real icon with outlined Inter text.
The identification figure was drawn by hand from the start, because it has to show the
matching steps exactly.

## Install figures

`docs/images/install-*.svg` are simplified drawings of the macOS and Windows first-launch
dialogs, marking where to click, for `docs/install.md`. They are drawn by
`tools/install_figures.py` (plain Python, no dependencies); run
`python3 assets/brand/tools/install_figures.py` from the repository root after editing it.
