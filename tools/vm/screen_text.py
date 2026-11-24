"""Text on a screenshot (tesseract, Arabic + English): words with their boxes, and checks that compare
text the way a reader does (Arabic diacritics and tatweel ignored). Used by the real-input gate to find
the Settings sidebar entries and to make sure a page shows no "not available" message.
`python3 screen_text.py IMAGE [x0 y0 x1 y1]` prints the words found (for looking at a run)."""
import re
import subprocess
import sys

# Arabic short vowels, shadda, sukun, superscript alef, tatweel: not drawn the same by every font
_MARKS = re.compile("[ً-ْٰـ]")


def norm(text):
    return _MARKS.sub("", text).replace("أ", "ا").replace("إ", "ا").replace("آ", "ا").replace("ى", "ي")


def words(png, box=None, psm=11):
    """[(text, (x0, y0, x1, y1), conf)] of the image (inside box when given)."""
    args = ["tesseract", png, "-", "-l", "ara+eng", "--psm", str(psm), "tsv"]
    if box:
        from PIL import Image
        x0, y0, x1, y1 = box
        crop = png + ".crop.png"
        img = Image.open(png).crop(box)
        # twice the size: the UI's 13-15 point text is small for the recogniser
        img.resize((img.width * 2, img.height * 2)).save(crop)
        args[1] = crop
    out = subprocess.run(args, capture_output=True, text=True, timeout=120).stdout
    found = []
    for line in out.splitlines()[1:]:
        f = line.split("\t")
        if len(f) < 12 or not f[11].strip():
            continue
        x, y, w, h = map(int, f[6:10])
        if box:
            x, y, w, h = box[0] + x // 2, box[1] + y // 2, w // 2, h // 2
        found.append((f[11].strip(), (x, y, x + w, y + h), float(f[10])))
    return found


def text(png, box=None):
    return norm(" ".join(w for w, _, _ in words(png, box, psm=3)))


def find(png, phrase, box=None):
    """Centre of the first place where the words of `phrase` appear in a row, or None."""
    want = [norm(p) for p in phrase.split()]
    ws = words(png, box)
    for i, (w, b, _) in enumerate(ws):
        if want[0] not in norm(w):
            continue
        if all(i + k < len(ws) and want[k] in norm(ws[i + k][0]) for k in range(len(want))):
            last = ws[i + len(want) - 1][1]
            x0, y0 = min(b[0], last[0]), min(b[1], last[1])
            x1, y1 = max(b[2], last[2]), max(b[3], last[3])
            return ((x0 + x1) // 2, (y0 + y1) // 2)
    return None


if __name__ == "__main__":
    box = tuple(map(int, sys.argv[2:6])) if len(sys.argv) >= 6 else None
    for w, b, c in words(sys.argv[1], box):
        print(f"{b} {c:5.1f} {w}")
