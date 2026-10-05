"""Draws the simplified install figures in docs/images/install-*.svg.

They show where to click in the macOS and Windows dialogs that appear the first time an unsigned
app is opened. They are drawings, not screenshots: the real dialogs' wording changes between
system versions, and screenshots would need a fresh, unsigned install on each system.

Run from the repository root: python3 assets/brand/tools/install_figures.py
"""

from pathlib import Path
from xml.sax.saxutils import escape

OUT = Path("docs/images")
FONT = "-apple-system, 'Segoe UI', Helvetica, Arial, sans-serif"
ACCENT = "#2f6fde"
RING = "#e8590c"


def text(x, y, s, size=13, weight=400, fill="#1d1d1f", anchor="start"):
    return (f'<text x="{x}" y="{y}" font-family="{FONT}" font-size="{size}" '
            f'font-weight="{weight}" fill="{fill}" text-anchor="{anchor}">{escape(s)}</text>')


def lines(x, y, rows, size=12, fill="#3a3a3c", anchor="start", gap=17):
    return "".join(text(x, y + i * gap, r, size, 400, fill, anchor) for i, r in enumerate(rows))


def button(x, y, w, label, primary=False, ring=False, h=28, radius=7):
    fill, color = (ACCENT, "#ffffff") if primary else ("#e5e5ea", "#1d1d1f")
    out = f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{radius}" fill="{fill}"/>'
    out += text(x + w / 2, y + h / 2 + 4.5, label, 13, 500, color, "middle")
    if ring:
        out += (f'<rect x="{x - 5}" y="{y - 5}" width="{w + 10}" height="{h + 10}" rx="{radius + 4}" '
                f'fill="none" stroke="{RING}" stroke-width="3"/>')
    return out


def app_icon(x, y, size=48):
    return (f'<rect x="{x}" y="{y}" width="{size}" height="{size}" rx="{size * 0.22}" fill="{ACCENT}"/>'
            f'<rect x="{x + size * 0.2}" y="{y + size * 0.27}" width="{size * 0.6}" height="{size * 0.46}" '
            f'rx="4" fill="none" stroke="#fff" stroke-width="3"/>')


def svg(w, h, body, background="#f2f2f7"):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">'
            f'<rect width="{w}" height="{h}" rx="12" fill="{background}"/>{body}</svg>\n')


def mac_dialog(title, body, buttons):
    w, h = 300, 300
    out = '<rect x="20" y="16" width="260" height="268" rx="14" fill="#ffffff" stroke="#d1d1d6"/>'
    out += app_icon(126, 36)
    out += lines(150, 112, title, 14, "#1d1d1f", "middle", 18).replace('font-weight="400"', 'font-weight="650"')
    out += lines(150, 112 + 18 * len(title) + 8, body, 11.5, "#3a3a3c", "middle", 15)
    y = 284 - 16 - 28 * len(buttons) - 8 * (len(buttons) - 1)
    for label, primary, ring in buttons:
        out += button(40, y, 220, label, primary, ring)
        y += 36
    return svg(w, h, out)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    figures = {
        "install-macos-not-opened.svg": mac_dialog(
            ["“Media Identifier”", "Not Opened"],
            ["Apple could not verify", "“Media Identifier” is free of", "malware that may harm your Mac."],
            [("Move to Trash", False, False), ("Done", True, True)],
        ),
        "install-macos-confirm.svg": mac_dialog(
            ["Open “Media Identifier”?"],
            ["Apple could not verify", "“Media Identifier” is free of", "malware. Open it only if you",
             "trust where it came from."],
            [("Open Anyway", False, True), ("Done", True, False)],
        ),
    }

    # System Settings > Privacy & Security, scrolled down to Security.
    w, h = 560, 220
    body = '<rect x="16" y="16" width="528" height="188" rx="12" fill="#ffffff" stroke="#d1d1d6"/>'
    body += '<rect x="16" y="16" width="150" height="188" rx="12" fill="#ececf0"/>'
    body += lines(30, 46, ["Network", "General", "Privacy & Security"], 12.5, "#1d1d1f", "start", 26)
    body += f'<rect x="24" y="84" width="134" height="24" rx="6" fill="{ACCENT}" opacity="0.18"/>'
    body += text(186, 48, "Privacy & Security", 16, 650)
    body += text(186, 82, "Security", 13, 650)
    body += '<rect x="186" y="94" width="340" height="88" rx="8" fill="#f5f5f7"/>'
    body += lines(200, 120, ["“Media Identifier” was blocked to protect", "your Mac."], 12.5, "#1d1d1f")
    body += button(410, 142, 104, "Open Anyway", False, True, 26, 6)
    figures["install-macos-open-anyway.svg"] = svg(w, h, body)

    # Windows SmartScreen, before and after "More info".
    def smartscreen(after):
        w, h = 520, 300
        b = '<rect x="16" y="16" width="488" height="268" fill="#1f5fa8"/>'
        b += text(40, 66, "Windows protected your PC", 22, 600, "#ffffff")
        b += lines(40, 102, ["Microsoft Defender SmartScreen prevented an unrecognized app",
                             "from starting. Running this app might put your PC at risk."],
                   12.5, "#ffffff")
        if after:
            b += lines(40, 160, ["App:        Media Identifier_0.1.0_x64-setup.exe",
                                 "Publisher:  Unknown publisher"], 12.5, "#ffffff")
            b += button(256, 236, 112, "Run anyway", False, True, 30, 0)
            b += button(380, 236, 108, "Don’t run", False, False, 30, 0)
        else:
            b += text(40, 150, "More info", 12.5, 600, "#ffffff")
            b += '<line x1="40" y1="153" x2="98" y2="153" stroke="#ffffff"/>'
            b += f'<rect x="33" y="134" width="72" height="26" rx="5" fill="none" stroke="{RING}" stroke-width="3"/>'
            b += button(380, 236, 108, "Don’t run", False, False, 30, 0)
        return svg(w, h, b, "#e9eef5")

    figures["install-windows-more-info.svg"] = smartscreen(False)
    figures["install-windows-run-anyway.svg"] = smartscreen(True)
    for name, content in figures.items():
        (OUT / name).write_text(content, encoding="utf-8")
        print(OUT / name)


if __name__ == "__main__":
    main()
