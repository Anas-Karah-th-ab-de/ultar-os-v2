#!/usr/bin/env python3
"""Per-board manifests of the product surfaces (settings, installer …) for the design
conformance tests of the apps.

Usage: make_board_manifest.py <repo-root> <run-dir> [--check]
<run-dir> holds `inventory.json` and `boards/<Board>.png` from board_inventory.py.
Writes `design/conformance/boards/<Board>.tsv`: one row per clickable element of the board
with its action, role, accessible name, flags, box (board pixels at 1x) and the board's pixel
colour at the sample point (6 points in from the left edge of the box, vertical middle; the
centre for boxes narrower than 16). `--check` fails when a manifest differs from the boards.
"""
import json
import pathlib
import sys

from PIL import Image

BOARDS = {
    # board: (theme, lang, offset of the app surface: title bar drawn by the compositor)
    "SettingsDisplay": ("dark", "ar", 40),
    "SettingsDisplayLight": ("light", "en", 40),
    "SettingsDrivers": ("dark", "ar", 40),
    "Account": ("dark", "ar", 40),
    "CompactSettings": ("dark", "ar", 44),
    "CompactDisplay": ("dark", "ar", 44),
    "Installer": ("dark", "ar", 0),
    "InstallerDisk": ("dark", "ar", 0),
    "InstallerDrivers": ("dark", "ar", 0),
    "InstallerSummary": ("dark", "ar", 0),
    "SurfaceStates": ("dark", "ar", 0),
    # The hand-over of an activity (mesh items A5, A6): desktop dialog and toasts, phone sheets.
    "MeshSend": ("dark", "ar", 0),
    "MeshIncoming": ("dark", "ar", 0),
    "MeshStates": ("dark", "ar", 0),
    "CompactMeshSend": ("dark", "ar", 0),
    "CompactMeshIncoming": ("dark", "ar", 0),
}
# Boards that show the surface over a desktop or phone shell: only the elements of the surface
# itself are listed (the window, bar and dock behind it belong to other manifests).
ONLY_PREFIX = {
    "MeshSend": ("mesh.",),
    "MeshIncoming": ("mesh.",),
    "MeshStates": ("mesh.", "toast."),
    "CompactMeshSend": ("mesh.",),
    "CompactMeshIncoming": ("mesh.",),
}
# Boards drawn with an open modal dialog: everything behind it lies under the scrim, so their
# colours are not the colours of the surface (no colour sample is taken).
SCRIMMED = {"SettingsDisplay"}
HEAD = "# board\ti\tact\trole\tname\tdisabled\tchecked\tx\ty\tw\th\trgb\tboard_w\tboard_h\ttheme\tlang\toffset"


def sample(img, box):
    x, y, w, h = box["x"], box["y"], box["w"], box["h"]
    px = x + w / 2 if w < 16 else x + 6
    py = y + h / 2
    if not (0 <= px < img.width and 0 <= py < img.height):
        return "-"
    r, g, b = img.getpixel((int(px), int(py)))[:3]
    return f"{r},{g},{b}"


# A sample point can lie on the edge of a glyph (the typed value of a left-to-right field):
# there the anti-aliased colour differs a little between Chrome builds. Everything else of a
# row must be equal; the colour may differ by this much per channel.
RGB_TOLERANCE = 12
RGB_COLUMN = 11


def same_row(a, b):
    if a == b:
        return True
    x, y = a.split("\t"), b.split("\t")
    if len(x) != len(y) or len(x) <= RGB_COLUMN or x[:RGB_COLUMN] != y[:RGB_COLUMN] or x[RGB_COLUMN + 1:] != y[RGB_COLUMN + 1:]:
        return False
    try:
        c, d = [int(v) for v in x[RGB_COLUMN].split(",")], [int(v) for v in y[RGB_COLUMN].split(",")]
    except ValueError:
        return False
    return len(c) == len(d) and all(abs(m - n) <= RGB_TOLERANCE for m, n in zip(c, d))


def rows(board, data, img, meta):
    theme, lang, offset = meta
    out = [HEAD]
    only = ONLY_PREFIX.get(board)
    for a in data["acts"]:
        if only and not a["act"].startswith(only):
            continue
        b = a["box"]
        name = a["name"].replace("\t", " ").replace("\n", " ")
        out.append("\t".join(str(v) for v in [
            board, a["i"], a["act"], a["role"] or a["tag"], name, int(bool(a["disabled"])),
            "-" if a["checked"] is None else str(a["checked"]).lower(), b["x"], b["y"], b["w"], b["h"],
            "-" if board in SCRIMMED else sample(img, b), data["width"], data["height"], theme, lang, offset]))
    return "\n".join(out) + "\n"


def main():
    root = pathlib.Path(sys.argv[1])
    run = pathlib.Path(sys.argv[2])
    check = "--check" in sys.argv
    inv = json.loads((run / "inventory.json").read_text())
    target = root / "design/conformance/boards"
    target.mkdir(parents=True, exist_ok=True)
    stale = []
    for board, meta in BOARDS.items():
        if board not in inv:
            stale.append(f"{board}: not in the inventory")
            continue
        img = Image.open(run / "boards" / f"{board}.png").convert("RGB")
        text = rows(board, inv[board], img, meta)
        path = target / f"{board}.tsv"
        if check:
            have = path.read_text().splitlines() if path.exists() else []
            want = text.splitlines()
            pairs = [(a, b) for a, b in zip(have, want) if not same_row(a, b)]
            if pairs or len(have) != len(want):
                stale.append(f"{path}: stale")
                # which rows differ (manifest | boards), so a log says why
                for a, b in pairs[:5]:
                    stale.append(f"  manifest: {a}\n  boards:   {b}")
                stale.append(f"  {len(pairs)} rows differ, {len(have)} rows in the manifest, {len(want)} on the board")
        else:
            path.write_text(text)
    if stale:
        print("\n".join(stale), file=sys.stderr)
        sys.exit(1 if check else 0)


if __name__ == "__main__":
    main()
