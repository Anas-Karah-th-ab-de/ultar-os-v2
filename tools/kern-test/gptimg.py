#!/usr/bin/env python3
"""Builds a GPT disk image from partition image files (no sgdisk needed).
   gptimg.py OUT.img [--size MiB] NAME:TYPE:FILE ...   TYPE: esp | linux | raw
   Partitions start at 1 MiB and are 1 MiB aligned; the file is copied to the partition start."""
import os, struct, sys, uuid, zlib

TYPES = {"esp": "C12A7328-F81F-11D2-BA4B-00A0C93EC93B", "linux": "0FC63DAF-8483-4772-8E79-3D69D8477DE4",
         "raw": "0FC63DAF-8483-4772-8E79-3D69D8477DE4"}
MIB = 2048

def header(cur, alt, first, last, disk, ent_lba, ent_crc):
    h = bytearray(92)
    h[0:8] = b"EFI PART"; h[8:12] = struct.pack("<I", 0x10000); h[12:16] = struct.pack("<I", 92)
    h[24:32] = struct.pack("<Q", cur); h[32:40] = struct.pack("<Q", alt)
    h[40:48] = struct.pack("<Q", first); h[48:56] = struct.pack("<Q", last); h[56:72] = disk
    h[72:80] = struct.pack("<Q", ent_lba); h[80:84] = struct.pack("<I", 128); h[84:88] = struct.pack("<I", 128)
    h[88:92] = struct.pack("<I", ent_crc)
    h[16:20] = struct.pack("<I", zlib.crc32(bytes(h)) & 0xffffffff)
    return bytes(h) + bytes(512 - 92)

def main():
    args = sys.argv[1:]
    out = args.pop(0)
    size_mib = None
    if args and args[0] == "--size":
        args.pop(0); size_mib = int(args.pop(0))
    parts, pos = [], MIB
    for spec in args:
        name, typ, path = spec.split(":", 2)
        sectors = (os.path.getsize(path) + 511) // 512
        sectors = (sectors + MIB - 1) // MIB * MIB
        parts.append((name, typ, path, pos, sectors)); pos += sectors
    total = max(pos + MIB, (size_mib or 0) * MIB)
    ents = bytearray(128 * 128)
    for i, (name, typ, path, first, sectors) in enumerate(parts):
        e = uuid.UUID(TYPES[typ]).bytes_le + uuid.uuid4().bytes_le + struct.pack("<QQQ", first, first + sectors - 1, 0)
        e += name.encode("utf-16-le").ljust(72, b"\0")
        ents[i * 128:(i + 1) * 128] = e
    crc = zlib.crc32(bytes(ents)) & 0xffffffff
    disk = uuid.uuid4().bytes_le
    with open(out, "wb") as f:
        f.truncate(total * 512)
        mbr = bytearray(512)
        mbr[446:462] = bytes([0, 0, 2, 0, 0xEE, 0xFF, 0xFF, 0xFF]) + struct.pack("<II", 1, min(total - 1, 0xFFFFFFFF))
        mbr[510:512] = b"\x55\xAA"
        f.seek(0); f.write(mbr)
        f.write(header(1, total - 1, 34, total - 34, disk, 2, crc)); f.write(ents)
        f.seek((total - 33) * 512); f.write(ents)
        f.write(header(total - 1, 1, 34, total - 34, disk, total - 33, crc))
        for (name, typ, path, first, sectors) in parts:
            with open(path, "rb") as src:
                f.seek(first * 512)
                while True:
                    chunk = src.read(1 << 20)
                    if not chunk:
                        break
                    f.write(chunk)

if __name__ == "__main__":
    main()
