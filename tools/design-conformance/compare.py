#!/usr/bin/env python3
"""Side-by-side pictures: board render | toolkit render | difference (red = differs).

Usage: compare.py <board.png> <toolkit.png> <out.png> [--crop x,y,w,h] [--scale 2]
Both pictures are 1x and the same size (the toolkit picture is built at the board's coordinates).
Prints the share of pixels that differ by more than 24 in any channel.
"""
import argparse

from PIL import Image, ImageChops, ImageDraw


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("board")
    ap.add_argument("impl")
    ap.add_argument("out")
    ap.add_argument("--crop")
    ap.add_argument("--scale", type=float, default=1.0)
    ap.add_argument("--labels", default="board,toolkit,diff")
    a = ap.parse_args()
    b = Image.open(a.board).convert("RGB")
    i = Image.open(a.impl).convert("RGB")
    if i.size != b.size:
        canvas = Image.new("RGB", b.size, (255, 0, 255))
        canvas.paste(i.crop((0, 0, min(i.width, b.width), min(i.height, b.height))), (0, 0))
        i = canvas
    if a.crop:
        x, y, w, h = map(int, a.crop.split(","))
        b, i = b.crop((x, y, x + w, y + h)), i.crop((x, y, x + w, y + h))
    d = ImageChops.difference(b, i).convert("L").point(lambda v: 255 if v > 24 else 0)
    share = d.histogram()[255] / (d.width * d.height)
    red = Image.new("RGB", b.size, (220, 40, 40))
    diff = Image.composite(red, Image.blend(b, Image.new("RGB", b.size, (0, 0, 0)), 0.6), d)
    w, h = b.size
    out = Image.new("RGB", (w * 3 + 16, h + 22), (40, 40, 40))
    for k, (img, label) in enumerate(zip((b, i, diff), a.labels.split(","))):
        out.paste(img, (k * (w + 8), 22))
        ImageDraw.Draw(out).text((k * (w + 8) + 4, 4), label, fill=(255, 255, 255))
    if a.scale != 1.0:
        out = out.resize((int(out.width * a.scale), int(out.height * a.scale)), Image.NEAREST)
    out.save(a.out)
    print(f"{a.out}: {share * 100:.1f}% of pixels differ")


if __name__ == "__main__":
    main()
