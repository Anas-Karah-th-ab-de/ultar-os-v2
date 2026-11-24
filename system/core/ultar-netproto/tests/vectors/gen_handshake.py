#!/usr/bin/env python3
"""Independent reference for the WPA2-Personal tests of ultar-netproto.

Writes `handshake.txt`: a recorded 4-way and group-key handshake between an
authenticator (this script) and the expected supplicant answers, computed with
Python's hashlib/hmac (PBKDF2-HMAC-SHA1, HMAC-SHA1) and the `cryptography`
package (RFC 3394 AES key wrap) – reviewed implementations independent of the
Rust code under test. Frame layout per IEEE 802.11-2016 12.7.2 / 12.7.6.
Run: python3 -I gen_handshake.py > handshake.txt
"""
import hashlib, hmac, struct
from cryptography.hazmat.primitives.keywrap import aes_key_wrap

SSID = b"UltarTest"
PASS = b"correct horse battery"
AA = bytes.fromhex("020000000001")   # authenticator (AP) address
SPA = bytes.fromhex("020000000002")  # supplicant address
ANONCE = bytes(range(0x10, 0x30))
SNONCE = bytes(range(0x80, 0xa0))
GTK = bytes.fromhex("00112233445566778899aabbccddeeff")
GTK2 = bytes.fromhex("ffeeddccbbaa99887766554433221100")
RSN_AP = bytes.fromhex("30180100000fac040200000fac04000fac020100000fac020c00")
RSN_STA = bytes.fromhex("30140100000fac040100000fac040100000fac020000")

def prf(key, label, data, nbytes):
    out = b""
    i = 0
    while len(out) < nbytes:
        out += hmac.new(key, label + b"\x00" + data + bytes([i]), hashlib.sha1).digest()
        i += 1
    return out[:nbytes]

def frame(info, keylen, replay, nonce, rsc, keydata, version=2):
    body = struct.pack(">BHH", 2, info, keylen) + struct.pack(">Q", replay) + nonce
    body += bytes(16) + rsc + bytes(8) + bytes(16) + struct.pack(">H", len(keydata)) + keydata
    return struct.pack(">BBH", version, 3, len(body)) + body

def with_mic(kck, f):
    mic = hmac.new(kck, f, hashlib.sha1).digest()[:16]
    return f[:81] + mic + f[97:]

def pad(data):
    if len(data) < 16 or len(data) % 8:
        data += b"\xdd"
        while len(data) < 16 or len(data) % 8:
            data += b"\x00"
    return data

pmk = hashlib.pbkdf2_hmac("sha1", PASS, SSID, 4096, 32)
ptk = prf(pmk, b"Pairwise key expansion",
          min(AA, SPA) + max(AA, SPA) + min(ANONCE, SNONCE) + max(ANONCE, SNONCE), 48)
kck, kek, tk = ptk[:16], ptk[16:32], ptk[32:48]
rsc = bytes.fromhex("0500000000000000")
m1 = frame(0x008a, 16, 1, ANONCE, bytes(8), b"")
m2 = with_mic(kck, frame(0x010a, 0, 1, SNONCE, bytes(8), RSN_STA, version=2))
gtk_kde = bytes([0xdd, 6 + len(GTK), 0x00, 0x0f, 0xac, 0x01, 0x01, 0x00]) + GTK
m3_plain = pad(RSN_AP + gtk_kde)
m3 = with_mic(kck, frame(0x13ca, 16, 2, ANONCE, rsc, aes_key_wrap(kek, m3_plain)))
m4 = with_mic(kck, frame(0x030a, 0, 2, bytes(32), bytes(8), b""))
g_kde = bytes([0xdd, 6 + len(GTK2), 0x00, 0x0f, 0xac, 0x01, 0x02, 0x00]) + GTK2
g1 = with_mic(kck, frame(0x1382, 0, 3, bytes(32), bytes.fromhex("0900000000000000"),
                         aes_key_wrap(kek, pad(g_kde))))
g2 = with_mic(kck, frame(0x0302, 0, 3, bytes(32), bytes(8), b""))
for name, value in [("ssid", SSID), ("passphrase", PASS), ("aa", AA), ("spa", SPA),
                    ("anonce", ANONCE), ("snonce", SNONCE), ("pmk", pmk), ("kck", kck),
                    ("kek", kek), ("tk", tk), ("gtk", GTK), ("gtk2", GTK2),
                    ("rsn_ap", RSN_AP), ("rsn_sta", RSN_STA),
                    ("m1", m1), ("m2", m2), ("m3", m3), ("m4", m4), ("g1", g1), ("g2", g2)]:
    print(f"{name} {value.hex()}")
