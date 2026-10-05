#!/usr/bin/env python3
"""Turn an authoring SVG into a publishable SVG whose text is drawn as outlines.

Why: README and documentation graphics are shown as images (GitHub, editors, browsers on any
OS). Live <text> would be rendered with whatever fonts the viewer has, so widths, wrapping and
highlight boxes would drift. Outlining the text with one pinned font (Inter 4.1, SIL OFL 1.1)
makes every viewer see the same picture.

Supported authoring markup (everything else is copied unchanged):

* ``<text x y font-size font-weight fill class text-anchor>`` with plain text and ``<tspan>``
  children. A tspan may set ``font-weight``, ``fill`` and ``class``; runs follow each other on
  the same baseline (no tspan positioning). ``font-weight`` is 400, 500, 600 or 700.
* ``<tspan data-mark="#color">`` or ``data-mark-class="name"`` draws a rounded highlight box
  behind that run (used for words shared by the heard text and the subtitles).
* ``<image data-inline="relative/path.svg" x y width height [data-viewbox]>`` is replaced by the
  referenced SVG's content as a nested <svg>, with its ids prefixed so several copies coexist.

Usage: outline_text.py --fonts DIR [--set NAME=VALUE ...] SOURCE.svg OUTPUT.svg
``--set`` replaces ``{{NAME}}`` in the source before parsing (used for light/dark variants).
DIR must contain Inter-Regular.otf, Inter-Medium.otf, Inter-SemiBold.otf and Inter-Bold.otf
(from the ``extras/otf`` folder of the Inter 4.1 release).
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
import xml.etree.ElementTree as ET

import uharfbuzz as hb
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.ttLib import TTFont

SVG_NS = "http://www.w3.org/2000/svg"
ET.register_namespace("", SVG_NS)
ET.register_namespace("xlink", "http://www.w3.org/1999/xlink")
Q = lambda tag: f"{{{SVG_NS}}}{tag}"  # noqa: E731

WEIGHT_FILES = {
    "400": "Inter-Regular.otf",
    "500": "Inter-Medium.otf",
    "600": "Inter-SemiBold.otf",
    "700": "Inter-Bold.otf",
}


def fmt(v: float) -> str:
    s = f"{v:.1f}"
    return s[:-2] if s.endswith(".0") else s


class Face:
    def __init__(self, path: Path):
        data = path.read_bytes()
        self.hb_font = hb.Font(hb.Face(hb.Blob(data)))
        self.tt = TTFont(path)
        self.glyphset = self.tt.getGlyphSet()
        self.order = self.tt.getGlyphOrder()
        self.upem = self.tt["head"].unitsPerEm

    def shape(self, text: str) -> tuple[list[tuple[str, int, int]], int]:
        """Shape one run with HarfBuzz (kerning, ligatures).

        Returns the glyphs as (glyph name, x, y) in font units from the run's origin, and the
        run's advance width in font units.
        """
        buf = hb.Buffer()
        buf.add_str(text)
        buf.guess_segment_properties()
        hb.shape(self.hb_font, buf, {"kern": True, "liga": True, "calt": True})
        glyphs: list[tuple[str, int, int]] = []
        pen_x = 0
        for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
            glyphs.append((self.order[info.codepoint], pen_x + pos.x_offset, pos.y_offset))
            pen_x += pos.x_advance
        return glyphs, pen_x

    def width(self, text: str, size: float) -> float:
        return self.shape(text)[1] * size / self.upem

    def outline(self, name: str) -> str:
        """Path data of one glyph in font units (y up, as stored in the font)."""
        pen = SVGPathPen(self.glyphset, ntos=fmt)
        self.glyphset[name].draw(pen)
        return pen.getCommands()


class Outliner:
    def __init__(self, font_dir: Path, source_dir: Path):
        self.faces = {w: Face(font_dir / f) for w, f in WEIGHT_FILES.items()}
        self.source_dir = source_dir
        self.inline_count = 0
        # Each glyph outline is stored once in <defs> and placed with <use>, which keeps
        # text-heavy illustrations small.
        self.glyph_defs: dict[str, ET.Element] = {}

    def glyph_ref(self, weight: str, face: Face, name: str) -> str | None:
        key = f"g{weight}-{face.tt.getGlyphID(name)}"
        if key not in self.glyph_defs:
            d = face.outline(name)
            if not d:
                return None
            path = ET.Element(Q("path"))
            path.set("id", key)
            path.set("d", d)
            self.glyph_defs[key] = path
        return key

    def face(self, weight: str) -> Face:
        weight = {"normal": "400", "bold": "700"}.get(weight, weight)
        if weight not in self.faces:
            sys.exit(f"unsupported font-weight {weight!r}; use 400, 500, 600 or 700")
        return self.faces[weight]

    def outline(self, text_el: ET.Element) -> ET.Element:
        size = float(text_el.get("font-size", "16"))
        base = {
            "weight": text_el.get("font-weight", "400"),
            "fill": text_el.get("fill"),
            "class": text_el.get("class"),
        }
        runs: list[tuple[str, dict]] = []
        if text_el.text:
            runs.append((text_el.text, dict(base)))
        for child in text_el:
            if child.tag != Q("tspan"):
                sys.exit(f"unsupported element inside <text>: {child.tag}")
            attrs = dict(base)
            if child.get("font-weight"):
                attrs["weight"] = child.get("font-weight")
            if child.get("fill"):
                attrs["fill"] = child.get("fill")
            if child.get("class"):
                attrs["class"] = child.get("class")
            attrs["mark"] = child.get("data-mark")
            attrs["mark_class"] = child.get("data-mark-class")
            if child.text:
                runs.append((child.text, attrs))
            if child.tail:
                runs.append((child.tail, dict(base)))
        runs = [(re.sub(r"\s+", " ", t), a) for t, a in runs]
        if runs:
            runs[0] = (runs[0][0].lstrip(), runs[0][1])
            runs[-1] = (runs[-1][0].rstrip(), runs[-1][1])

        x0 = float(text_el.get("x", "0"))
        y = float(text_el.get("y", "0"))
        # First pass measures, so text-anchor can shift the whole line.
        total = sum(self.face(a["weight"]).width(t, size) for t, a in runs)
        anchor = text_el.get("text-anchor", "start")
        x = x0 - {"start": 0, "middle": total / 2, "end": total}[anchor]

        group = ET.Element(Q("g"))
        for key in ("id", "transform", "opacity"):
            if text_el.get(key):
                group.set(key, text_el.get(key))
        marks: list[ET.Element] = []
        paths: list[ET.Element] = []
        for t, a in runs:
            face = self.face(a["weight"])
            glyphs, advance = face.shape(t)
            scale = size / face.upem
            width = advance * scale
            if a.get("mark") or a.get("mark_class"):
                lead = len(t) - len(t.lstrip())
                trail = len(t) - len(t.rstrip())
                lead_w = face.width(t[:lead], size) if lead else 0
                trail_w = face.width(t[len(t) - trail :], size) if trail else 0
                pad = size * 0.12
                rect = ET.Element(Q("rect"))
                rect.set("x", fmt(x + lead_w - pad))
                rect.set("y", fmt(y - size * 0.86))
                rect.set("width", fmt(width - lead_w - trail_w + 2 * pad))
                rect.set("height", fmt(size * 1.18))
                rect.set("rx", fmt(size * 0.22))
                if a.get("mark"):
                    rect.set("fill", a["mark"])
                if a.get("mark_class"):
                    rect.set("class", a["mark_class"])
                marks.append(rect)
            run = ET.Element(Q("g"))
            run.set("transform", f"matrix({scale:.6g} 0 0 {-scale:.6g} {fmt(x)} {fmt(y)})")
            if a.get("fill"):
                run.set("fill", a["fill"])
            if a.get("class"):
                run.set("class", a["class"])
            for name, gx, gy in glyphs:
                ref = self.glyph_ref(a["weight"], face, name)
                if ref is None:
                    continue
                use = ET.SubElement(run, Q("use"))
                use.set("href", f"#{ref}")
                if gx:
                    use.set("x", str(gx))
                if gy:
                    use.set("y", str(gy))
            if len(run):
                paths.append(run)
            x += width
        group.extend(marks + paths)
        return group

    def inline(self, image_el: ET.Element) -> ET.Element:
        self.inline_count += 1
        prefix = f"i{self.inline_count}-"
        src = (self.source_dir / image_el.get("data-inline")).read_text()
        src = re.sub(r'id="([^"]+)"', lambda m: f'id="{prefix}{m.group(1)}"', src)
        src = re.sub(r"url\(#([^)]+)\)", lambda m: f"url(#{prefix}{m.group(1)})", src)
        nested = ET.fromstring(src)
        for key in ("x", "y", "width", "height"):
            nested.set(key, image_el.get(key, "0"))
        if image_el.get("data-viewbox"):
            nested.set("viewBox", image_el.get("data-viewbox"))
        return nested

    def walk(self, parent: ET.Element) -> None:
        for i, child in enumerate(list(parent)):
            if child.tag == Q("text"):
                new = self.outline(child)
            elif child.tag == Q("image") and child.get("data-inline"):
                new = self.inline(child)
            else:
                self.walk(child)
                continue
            new.tail = child.tail
            parent.remove(child)
            parent.insert(i, new)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--fonts", required=True, type=Path)
    ap.add_argument("--set", action="append", default=[], metavar="NAME=VALUE")
    ap.add_argument("source", type=Path)
    ap.add_argument("output", type=Path)
    args = ap.parse_args()

    text = args.source.read_text()
    for item in args.set:
        name, _, value = item.partition("=")
        text = text.replace("{{" + name + "}}", value)
    leftover = re.findall(r"\{\{(\w+)\}\}", text)
    if leftover:
        sys.exit(f"unset template values: {sorted(set(leftover))}")
    root = ET.fromstring(text)
    outliner = Outliner(args.fonts, args.source.parent)
    outliner.walk(root)
    if outliner.glyph_defs:
        defs = ET.Element(Q("defs"))
        defs.extend(outliner.glyph_defs[k] for k in sorted(outliner.glyph_defs))
        root.insert(0, defs)
    ET.indent(root, space="  ")
    out = ET.tostring(root, encoding="unicode")
    args.output.write_text("<!-- Generated by assets/brand/tools/outline_text.py; edit the source in assets/brand/src/. -->\n" + out + "\n")


if __name__ == "__main__":
    main()
