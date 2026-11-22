#!/usr/bin/env python3
"""Wandelt die Marken-Grafiken (assets/brand/*.png, Quelle: codecraft-platform apps/web/public/brand)
in rohe RGBA-Dateien um, die in die Oberfläche eingebettet werden. Nach Logo-Änderung neu ausführen."""
import os, struct
from PIL import Image
d = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "assets", "brand")
for name in ("logo", "mark-circuit"):
    im = Image.open(os.path.join(d, name + ".png")).convert("RGBA")
    with open(os.path.join(d, name + ".rgba"), "wb") as f:
        f.write(struct.pack("<II", im.width, im.height) + im.tobytes())
    print(name, im.size)
