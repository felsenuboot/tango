#!/usr/bin/env python3
"""Renders text in a font file to a flat symbolic SVG (paths only, tight viewBox).

    tools/calligraphy.py YujiSyuku-Regular.ttf 単語 \
        data/icons/hicolor/scalable/apps/io.github.felsenuboot.Tango-calligraphy-symbolic.svg

Needs pango-view, rsvg-convert and Pillow. The font is not installed anywhere: a throwaway
fontconfig file points at its directory. cairo writes glyphs as <symbol>/<use> pairs with
style attributes; GTK's symbolic recolouring wants plain <path> elements, so they are
flattened, rounded to two decimals and cropped to the ink's bounding box.
"""

import io
import os
import re
import subprocess
import sys
import tempfile

from PIL import Image

font, text, out = sys.argv[1:4]
family = re.search(r'([A-Za-z]+)', os.path.basename(font)).group(1)
family = re.sub(r'(?<=[a-z])(?=[A-Z])', ' ', family)  # YujiSyuku -> Yuji Syuku
with tempfile.TemporaryDirectory() as tmp:
    conf = os.path.join(tmp, 'fonts.conf')
    with open(conf, 'w') as f:
        f.write('<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "fonts.dtd"><fontconfig>'
                '<include ignore_missing="yes">/etc/fonts/fonts.conf</include>'
                f'<dir>{os.path.dirname(os.path.abspath(font))}</dir><cachedir>{tmp}</cachedir></fontconfig>')
    raw = os.path.join(tmp, 'raw.svg')
    subprocess.run(['pango-view', f'--font={family} 200', '-t', text, '--margin=0', '-q', '-o', raw],
                   env={**os.environ, 'FONTCONFIG_FILE': conf}, check=True)
    src = open(raw).read()

glyphs = {m.group(1): m.group(2) for m in re.finditer(r'<g id="([^"]+)">\s*<path d="([^"]+)"', src)}
uses = re.findall(r'<use xlink:href="#([^"]+)" x="([^"]+)" y="([^"]+)"/>', src)
w, h = re.search(r'width="(\d+)" height="(\d+)"', src).groups()


def rounded(d):
    return re.sub(r'-?\d+\.\d+', lambda m: ('%.2f' % float(m.group())).rstrip('0').rstrip('.'), d)


paths = ['<path transform="translate(%s %s)" d="%s"/>' % (x, y, rounded(glyphs[g])) for g, x, y in uses]
full = '<svg xmlns="http://www.w3.org/2000/svg" width="%s" height="%s" viewBox="0 0 %s %s">%s</svg>' % (
    w, h, w, h, ''.join(paths))
png = subprocess.run(['rsvg-convert', '-f', 'png'], input=full.encode(), capture_output=True, check=True).stdout
x0, y0, x1, y1 = Image.open(io.BytesIO(png)).getbbox()
pad = 2
vx, vy, vw, vh = x0 - pad, y0 - pad, x1 - x0 + 2 * pad, y1 - y0 + 2 * pad
with open(out, 'w') as f:
    f.write('<?xml version="1.0" encoding="UTF-8"?>\n'
            f'<!-- {text} set in {family} (SIL Open Font License 1.1), glyph outlines only;'
            ' made by tools/calligraphy.py. -->\n'
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{vw}" height="{vh}" viewBox="{vx} {vy} {vw} {vh}"'
            ' fill="#000000">\n' + '\n'.join(paths) + '\n</svg>\n')
print(f'{out}: {vw}x{vh}')
