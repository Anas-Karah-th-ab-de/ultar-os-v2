#!/usr/bin/env python3
"""Makes the small test PDFs in tests/fixtures (run once, the results are committed).

Needs fontTools (pip install fonttools) and the fonts below; qpdf (optional) for the
encrypted one. Usage: python3 make_fixtures.py [OUTDIR]
"""
import hashlib
import io
import struct
import sys
import unicodedata
import zlib
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

NASKH = "/usr/share/fonts/truetype/noto/NotoNaskhArabic-Regular.ttf"
LATIN = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent / "fixtures"

# Letters that do not join to the following letter.
RIGHT_ONLY = set("اأإآدذرزوؤةى")


def presentation(ch, form):
    """The presentation-form character (U+FExx) of `ch` in `form`, or `ch`."""
    for cp in list(range(0xFB50, 0xFE00)) + list(range(0xFE70, 0xFF00)):
        d = unicodedata.decomposition(chr(cp)).split()
        if len(d) == 2 and d[0] == f"<{form}>" and chr(int(d[1], 16)) == ch:
            return chr(cp)
    return ch


def shape(word):
    """Logical Arabic word -> presentation-form chars in logical order."""
    out = []
    for i, ch in enumerate(word):
        joins_prev = i > 0 and word[i - 1] not in RIGHT_ONLY
        joins_next = i + 1 < len(word) and ch not in RIGHT_ONLY
        form = {(0, 0): "isolated", (0, 1): "initial", (1, 1): "medial", (1, 0): "final"}[
            (int(joins_prev), int(joins_next))
        ]
        out.append(presentation(ch, form))
    return out


def subset_font(path, chars):
    opts = subset.Options()
    opts.layout_features = []
    opts.notdef_outline = True
    opts.name_IDs = []
    opts.hinting = False
    font = TTFont(path)
    sub = subset.Subsetter(opts)
    sub.populate(text="".join(sorted(set(chars))))
    sub.subset(font)
    buf = io.BytesIO()
    font.save(buf)
    data = buf.getvalue()
    return data, TTFont(io.BytesIO(data))


class Pdf:
    def __init__(self):
        self.objs = []
        self.extra = ""

    def add(self, body):
        self.objs.append(body)
        return len(self.objs)

    def reserve(self):
        self.objs.append(None)
        return len(self.objs)

    def set(self, n, body):
        self.objs[n - 1] = body

    def stream(self, dict_extra, data, compress=True):
        if compress:
            data = zlib.compress(data, 9)
            dict_extra += " /Filter /FlateDecode"
        return f"<< {dict_extra} /Length {len(data)} >>\nstream\n".encode() + data + b"\nendstream"

    def write(self, root):
        out = bytearray(b"%PDF-1.5\n%\xe2\xe3\xcf\xd3\n")
        offsets = []
        for i, body in enumerate(self.objs, 1):
            offsets.append(len(out))
            if isinstance(body, str):
                body = body.encode()
            out += f"{i} 0 obj\n".encode() + body + b"\nendobj\n"
        xref = len(out)
        out += f"xref\n0 {len(self.objs) + 1}\n0000000000 65535 f \n".encode()
        for o in offsets:
            out += f"{o:010d} 00000 n \n".encode()
        out += f"trailer\n<< /Size {len(self.objs) + 1} /Root {root} 0 R{self.extra} >>\nstartxref\n{xref}\n%%EOF\n".encode()
        return bytes(out)


PAD = bytes.fromhex("28BF4E5E4E758A4164004E56FFFA01082E2E00B6D0683E802F0CA9FE6453697A")


def rc4(key, data):
    s = list(range(256))
    j = 0
    for i in range(256):
        j = (j + s[i] + key[i % len(key)]) % 256
        s[i], s[j] = s[j], s[i]
    out = bytearray()
    i = j = 0
    for b in data:
        i = (i + 1) % 256
        j = (j + s[i]) % 256
        s[i], s[j] = s[j], s[i]
        out.append(b ^ s[(s[i] + s[j]) % 256])
    return bytes(out)


def lock(pdf_bytes_builder, user, owner):
    """Encrypts a Pdf with RC4-40 (security handler revision 2): returns the new bytes."""
    pdf, root = pdf_bytes_builder
    up = (user.encode() + PAD)[:32]
    op = (owner.encode() + PAD)[:32]
    o = rc4(hashlib.md5(op).digest()[:5], up)
    p = -4
    ident = hashlib.md5(b"ultar-fixture").digest()
    key = hashlib.md5(up + o + struct.pack("<i", p) + ident).digest()[:5]
    u = rc4(key, PAD)
    enc = len(pdf.objs) + 1
    for n, body in enumerate(pdf.objs, 1):
        if isinstance(body, bytes) and b"stream\\n" in body:
            head, rest = body.split(b"stream\\n", 1)
            data, tail = rest.rsplit(b"\\nendstream", 1)
            ok = hashlib.md5(key + struct.pack("<I", n)[:3] + b"\\0\\0").digest()[:10]
            pdf.objs[n - 1] = head + b"stream\\n" + rc4(ok, data) + b"\\nendstream" + tail
    pdf.objs.append(
        f"<< /Filter /Standard /V 1 /R 2 /O <{o.hex()}> /U <{u.hex()}> /P {p} >>"
    )
    pdf.extra = f" /Encrypt {enc} 0 R /ID [<{ident.hex()}> <{ident.hex()}>]"
    return pdf.write(root)


def u16(s):
    return "<FEFF" + s.encode("utf-16-be").hex().upper() + ">"


def type0(pdf, name, data, font, cmap_unicode):
    """A Type0/Identity-H font object for the embedded subset `data`; returns its object number."""
    glyph_order = font.getGlyphOrder()
    ff = pdf.add(pdf.stream(f"/Length1 {len(data)}", data))
    desc = pdf.add(
        f"<< /Type /FontDescriptor /FontName /{name} /Flags 4 /FontBBox [-200 -300 1200 1000] "
        f"/ItalicAngle 0 /Ascent 900 /Descent -300 /CapHeight 700 /StemV 80 /FontFile2 {ff} 0 R >>"
    )
    upem = font["head"].unitsPerEm
    hmtx = font["hmtx"].metrics
    widths = " ".join(f"{g} [{int(hmtx[glyph_order[g]][0] * 1000 / upem)}]" for g in range(len(glyph_order)))
    cid = pdf.add(
        f"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) "
        f"/Ordering (Identity) /Supplement 0 >> /FontDescriptor {desc} 0 R /CIDToGIDMap /Identity /W [{widths}] >>"
    )
    entries = "\n".join(f"<{gid:04X}> <{ord(u):04X}>" for gid, u in sorted(cmap_unicode.items()))
    tounicode = pdf.add(
        pdf.stream(
            "",
            (
                "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n"
                "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
                "/CMapName /Adobe-Identity-UCS def /CMapType 2 def\n"
                "1 begincodespacerange <0000> <FFFF> endcodespacerange\n"
                f"{len(cmap_unicode)} beginbfchar\n{entries}\nendbfchar\nendcmap end end\n"
            ).encode(),
        )
    )
    return pdf.add(
        f"<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H "
        f"/DescendantFonts [{cid} 0 R] /ToUnicode {tounicode} 0 R >>"
    )


def to_unicode(font, cmap, chars):
    return {font.getGlyphID(gn): chr(cp) for cp, gn in cmap.items() if chr(cp) in chars}


def mixed():
    pages = [
        {"latin": ["Ultar reader test page one", "The quick brown fox"], "arabic": ["مرحبا بالعالم", "ملف تجريبي"]},
        {"latin": ["Second page of the fixture", "Fox jumps again"], "arabic": ["الصفحة الثانية", "بحث عربي"]},
    ]
    latin_chars = set(" ")
    ar_chars = set(" ")
    for p in pages:
        for line in p["latin"]:
            latin_chars |= set(line)
        for line in p["arabic"]:
            for w in line.split(" "):
                ar_chars |= set(shape(w))
    ldata, lfont = subset_font(LATIN, latin_chars)
    adata, afont = subset_font(NASKH, ar_chars)
    lcmap = lfont.getBestCmap()
    acmap = afont.getBestCmap()
    pdf = Pdf()
    catalog = pdf.reserve()
    pages_obj = pdf.reserve()
    lf = type0(pdf, "AAAAAA+DejaVuSans", ldata, lfont, to_unicode(lfont, lcmap, latin_chars))
    af = type0(pdf, "BBBBBB+NotoNaskh", adata, afont, to_unicode(afont, acmap, ar_chars))
    kids = []
    for p in pages:
        ops = ["BT"]
        y = 760
        for line in p["latin"]:
            gids = "".join(f"{lfont.getGlyphID(lcmap[ord(c)]):04X}" for c in line)
            ops.append(f"/F1 22 Tf 1 0 0 1 60 {y} Tm <{gids}> Tj")
            y -= 40
        for line in p["arabic"]:
            # Visual order, as a typesetter writes it: the words and the glyphs from left to right.
            words = line.split(" ")
            visual = []
            for wi, w in enumerate(reversed(words)):
                if wi:
                    visual.append(afont.getGlyphID(acmap[0x20]))
                visual += [afont.getGlyphID(acmap[ord(c)]) for c in reversed(shape(w))]
            gids = "".join(f"{g:04X}" for g in visual)
            ops.append(f"/F2 26 Tf 1 0 0 1 60 {y} Tm <{gids}> Tj")
            y -= 50
        ops.append("ET")
        content = pdf.add(pdf.stream("", "\n".join(ops).encode()))
        kids.append(
            pdf.add(
                f"<< /Type /Page /Parent {pages_obj} 0 R /MediaBox [0 0 595 842] /Contents {content} 0 R "
                f"/Resources << /Font << /F1 {lf} 0 R /F2 {af} 0 R >> >> >>"
            )
        )
    titles = [("Start", 0), ("مرحبا", 0), ("الصفحة الثانية", 1)]
    outlines = pdf.reserve()
    items = [pdf.reserve() for _ in titles]
    for i, ((t, pg), n) in enumerate(zip(titles, items)):
        prev = f" /Prev {items[i - 1]} 0 R" if i else ""
        nxt = f" /Next {items[i + 1]} 0 R" if i + 1 < len(items) else ""
        pdf.set(n, f"<< /Title {u16(t)} /Parent {outlines} 0 R{prev}{nxt} /Dest [{kids[pg]} 0 R /Fit] >>")
    pdf.set(outlines, f"<< /Type /Outlines /First {items[0]} 0 R /Last {items[-1]} 0 R /Count {len(items)} >>")
    pdf.set(pages_obj, f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {len(kids)} >>")
    pdf.set(catalog, f"<< /Type /Catalog /Pages {pages_obj} 0 R /Outlines {outlines} 0 R >>")
    return pdf.write(catalog)


def plain_pages(n, label):
    pdf = Pdf()
    catalog = pdf.reserve()
    pages_obj = pdf.reserve()
    font = pdf.add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    kids = []
    for i in range(n):
        content = pdf.add(pdf.stream("", f"BT /F1 24 Tf 60 760 Td ({label} {i + 1}) Tj ET".encode()))
        kids.append(
            pdf.add(
                f"<< /Type /Page /Parent {pages_obj} 0 R /MediaBox [0 0 595 842] /Contents {content} 0 R "
                f"/Resources << /Font << /F1 {font} 0 R >> >> >>"
            )
        )
    pdf.set(pages_obj, f"<< /Type /Pages /Kids [{' '.join(f'{k} 0 R' for k in kids)}] /Count {n} >>")
    pdf.set(catalog, f"<< /Type /Catalog /Pages {pages_obj} 0 R >>")
    return (pdf, catalog)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "mixed.pdf").write_bytes(mixed())
    (OUT / "plain3.pdf").write_bytes(plain_pages(3, "Secret page")[0].write(1))
    (OUT / "locked.pdf").write_bytes(lock(plain_pages(3, "Secret page"), "ultar", "owner"))
    (OUT / "broken.pdf").write_bytes(
        b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 9 0 R >>\nendobj\nthis is not a pdf any more"
    )
    for f in sorted(OUT.glob("*.pdf")):
        print(f.name, f.stat().st_size)


if __name__ == "__main__":
    main()
