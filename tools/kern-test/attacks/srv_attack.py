#!/usr/bin/env python3
"""Adversarial requests against a running ultraos-server (security review).
usage: srv_attack.py <host> <port> <alice.json> <mallory.json>
Prints one line per probe; 'BEFUND' marks a defect, 'ok' an attack that failed as it must."""
import json, socket, sys, time, base64, hmac, hashlib, threading

HOST, PORT = sys.argv[1], int(sys.argv[2])
ALICE = json.load(open(sys.argv[3]))["token"]
MALLORY = json.load(open(sys.argv[4]))["token"]
findings = 0
_ip = [0]


def fresh_ip():
    """each probe comes from its own client address (we are the trusted proxy on loopback)"""
    _ip[0] += 1
    return f"198.18.{(_ip[0] >> 8) & 255}.{_ip[0] & 255}"


def raw(data: bytes, timeout=5.0, read_all=True):
    s = socket.create_connection((HOST, PORT), timeout=timeout)
    try:
        s.sendall(data)
        out = b""
        while True:
            try:
                d = s.recv(65536)
            except socket.timeout:
                break
            if not d:
                break
            out += d
            if not read_all and b"\r\n\r\n" in out:
                break
        return out
    finally:
        s.close()


def req(method, target, headers=None, body=b"", ip=None, version="1.1"):
    h = {"Host": "ultar.code-craft.tech", "Connection": "close", "X-Forwarded-For": ip or fresh_ip()}
    if headers:
        h.update(headers)
    if body or method in ("POST", "PUT"):
        h.setdefault("Content-Length", str(len(body)))
    lines = [f"{method} {target} HTTP/{version}"] + [f"{k}: {v}" for k, v in h.items() if v is not None]
    data = ("\r\n".join(lines) + "\r\n\r\n").encode("latin-1") + body
    out = raw(data)
    head, _, rest = out.partition(b"\r\n\r\n")
    status = int(head.split(b" ")[1]) if head.startswith(b"HTTP/") and len(head.split(b" ")) > 1 else 0
    hdrs = {}
    for line in head.split(b"\r\n")[1:]:
        k, _, v = line.partition(b":")
        hdrs[k.decode().lower()] = v.strip().decode("latin-1")
    return status, hdrs, rest


def report(name, bad, detail=""):
    global findings
    if bad:
        findings += 1
    print(f"{'BEFUND' if bad else 'ok    '} {name}{(' -> ' + detail) if detail else ''}", flush=True)


def alive():
    try:
        return req("GET", "/v1/health")[0] == 200
    except Exception:
        return False


def bearer(t):
    return {"Authorization": "Bearer " + t}


# ---------------------------------------------------------------- 1. premium package without entitlement
PRO = "/v1/store/apps/com.kmcc.pro/download"
PRO_SHA = json.loads(req("GET", "/v1/store/apps/com.kmcc.pro")[2])["sha256"]
st, h, b = req("GET", PRO, bearer(ALICE))
report("Kontrolle: berechtigtes Konto lädt das Premium-Paket", st != 200 or len(b) != 200000, f"{st} {len(b)} bytes")
leak = lambda st, b: st in (200, 206, 304) or len(b) > 2000
variants = [
    ("GET", PRO, {}), ("HEAD", PRO, {}), ("GET", PRO, bearer(MALLORY)), ("HEAD", PRO, bearer(MALLORY)),
    ("GET", PRO, {"Range": "bytes=0-99"}), ("GET", PRO, {"Range": "bytes=-1"}), ("GET", PRO, {**bearer(MALLORY), "Range": "bytes=0-0"}),
    ("GET", PRO, {"If-None-Match": f'"{PRO_SHA}"'}), ("GET", PRO, {"If-None-Match": "*"}), ("GET", PRO, {**bearer(MALLORY), "If-None-Match": "*"}),
    ("GET", PRO, {"If-Range": f'"{PRO_SHA}"', "Range": "bytes=0-10"}),
    ("GET", PRO, {"Authorization": "Bearer"}), ("GET", PRO, {"Authorization": "Bearer "}), ("GET", PRO, {"Authorization": "bearer " + MALLORY}),
    ("GET", PRO, {"Authorization": "Basic YWxpY2U6cGFzc3dvcnQxMjM="}), ("GET", PRO, {"Authorization": "Bearer " + ALICE[:-2]}),
    ("GET", PRO, {"Authorization": "Bearer " + ALICE + "A"}), ("GET", PRO, {"Authorization": "Bearer " + ALICE.split(".")[0] + "."}),
    ("GET", PRO, {"Authorization": "Bearer " + ALICE.split(".")[0]}), ("GET", PRO, {"Authorization": "Bearer ." + ALICE.split(".")[1]}),
    ("GET", PRO, {"Authorization": "Bearer " + MALLORY.split(".")[0].replace("Mjo", "MTo") + "." + MALLORY.split(".")[1]}),
    ("GET", PRO + "?version=2.0.0", {}), ("GET", PRO + "?version=2.0.0&version=1.0.0", {}), ("GET", PRO + "?premium=false", {}),
    ("GET", "/v1/store/apps/COM.KMCC.PRO/download", {}), ("GET", "/v1/store/apps/com.kmcc.Pro/download", {}),
    ("GET", "/v1/store/apps/com%2Ekmcc%2Epro/download", {}), ("GET", "/v1/store/apps/com.kmcc.pro%00/download", {}),
    ("GET", "/v1/store/apps/com.kmcc.sheets/../com.kmcc.pro/download", {}), ("GET", "/v1/store/apps/com.kmcc.sheets%2F..%2Fcom.kmcc.pro/download", {}),
    ("GET", "//v1/store/apps/com.kmcc.pro/download", {}), ("GET", "/v1/store/apps/com.kmcc.pro/download/", {}), ("GET", "/v1/store/apps//com.kmcc.pro/download", {}),
    ("GET", "/v1/store/apps/com.kmcc.pro/./download", {}), ("GET", "/v1/store/apps/com.kmcc.pro/download;x=1", {}), ("GET", "/v1/store/apps/com.kmcc.pro/DOWNLOAD", {}),
    ("GET", "/V1/store/apps/com.kmcc.pro/download", {}), ("POST", PRO, {}), ("OPTIONS", PRO, {}), ("GET", PRO + "#x", {}),
    ("GET", "http://127.0.0.1" + PRO, {}), ("GET", f"/v1/store/apps/{PRO_SHA}/download", {}), ("GET", f"/v1/store/files/{PRO_SHA}", {}), ("GET", f"/files/{PRO_SHA}", {}),
    ("GET", f"/{PRO_SHA}", {}), ("GET", PRO, {"X-Ultar-Entitled": "1", "X-Forwarded-User": "alice", "X-Account-Id": "1"}),
    ("GET", "/v1/store/apps/com.kmcc.sheets/download?version=2.0.0", {}),
]
for method, target, hd in variants:
    st, h, b = req(method, target, hd)
    hint = " ".join(f"{k}={v[:24]}" for k, v in hd.items())
    report(f"Premium ohne Zugang: {method} {target[:70]} {hint[:60]}", leak(st, b), f"{st}")

# ---------------------------------------------------------------- 2. gated channel
st, h, b = req("GET", "/v1/update/file/2", bearer(ALICE))
report("Kontrolle: berechtigtes Konto lädt das Abbild des geschützten Kanals", st != 200 or len(b) != 400000, f"{st} {len(b)}")
gated = [
    ("GET", "/v1/update/latest?channel=lts", {}), ("GET", "/v1/update/latest", {"x-ultar-channel": "lts"}), ("GET", "/v1/update/latest?channel=lts", bearer(MALLORY)),
    ("HEAD", "/v1/update/latest?channel=lts", {}), ("GET", "/v1/update/latest?channel=stable&channel=lts", {}), ("GET", "/v1/update/latest?channel=lts&channel=stable", {}),
    ("GET", "/v1/update/latest?channel=LTS", {}), ("GET", "/v1/update/latest?channel=lts%00", {}), ("GET", "/v1/update/latest?channel=stable", {"x-ultar-channel": "lts"}),
    ("GET", "/v1/update/file/2", {}), ("HEAD", "/v1/update/file/2", {}), ("GET", "/v1/update/file/2", bearer(MALLORY)), ("GET", "/v1/update/file/2", {"Range": "bytes=0-9"}),
    ("GET", "/v1/update/file/2", {"If-None-Match": "*"}), ("GET", "/v1/update/file/02", {}), ("GET", "/v1/update/file/000000000002", {}), ("GET", "/v1/update/file/2/", {}),
    ("GET", "/v1/update/file/%32", {}), ("GET", "/v1/update/file/1/../2", {}), ("GET", "/v1/update/file/2?channel=stable", {}), ("GET", "/v1/update/file/2", {"x-ultar-channel": "stable"}),
    ("GET", "/v1/update/file/+2", {}), ("GET", "/v1/update/file/2.0", {}), ("GET", "/v1/update/file/٢", {}),
]
for method, target, hd in gated:
    try:
        st, h, b = req(method, target.encode("utf-8").decode("latin-1"), hd)
    except Exception as e:
        st, b = -1, b""
    hint = " ".join(f"{k}={v[:16]}" for k, v in hd.items())
    report(f"Geschützter Kanal ohne Zugang: {method} {target} {hint}", st in (200, 206, 304) and (b"roothash" in b or len(b) > 2000 or st == 304 or method == "HEAD"), f"{st}")

# ---------------------------------------------------------------- 3. forged tokens
def b64(x):
    return base64.urlsafe_b64encode(x).rstrip(b"=").decode()

far = int(time.time()) + 10**6
forged = [
    b64(f"1:{far}".encode()) + ".", b64(f"1:{far}".encode()) + "." + b64(b"\0" * 32), b64(f"1:{far}".encode()) + "." + b64(hmac.new(b"", f"1:{far}".encode(), hashlib.sha256).digest()),
    b64(f"1:{far}".encode()) + "." + b64(hmac.new(b"\0" * 32, f"1:{far}".encode(), hashlib.sha256).digest()),
    b64(f"1:{far}".encode()) + "." + b64(hmac.new(b"ultraos", f"1:{far}".encode(), hashlib.sha256).digest()),
    MALLORY.split(".")[0] + "=." + MALLORY.split(".")[1], b64(b"1:99999999999999999999") + "." + MALLORY.split(".")[1], b64(b" 1:9") + ".", "..", ".", "a.b.c",
]
for t in forged:
    st, h, b = req("GET", "/v1/account/entitlements", bearer(t))
    report(f"Gefälschter Token {t[:40]}", st == 200 and b"com.kmcc.pro" in b, f"{st}")
st, h, b = req("GET", "/v1/account/entitlements", bearer(MALLORY))
report("Kontrolle: fremdes Konto sieht nur die eigenen Zugänge", b"com.kmcc.pro" in b or st != 200, f"{st} {b[:60]!r}")

# ---------------------------------------------------------------- 4. injection and malformed input
inj = ["' OR 1=1 --", "\"; DROP TABLE accounts; --", "%' ESCAPE '", "\\", "%", "_", "a' UNION SELECT pw_hash,1,1,1,1,1,1,1,1,1 FROM accounts --", "\x00", "ج" * 100, "%00", "{{7*7}}"]
import urllib.parse
for s in inj:
    q = urllib.parse.quote(s)
    for t in (f"/v1/store/catalog?q={q}", f"/v1/store/catalog?category={q}", f"/v1/store/catalog?filetype={q}", f"/v1/store/apps/{q}", f"/v1/store/apps/{q}/download", f"/v1/store/apps/com.kmcc.sheets/download?version={q}",
              f"/v1/update/latest?channel={q}", f"/v1/update/latest?arch={q}&edition={q}", f"/v1/update/file/{q}"):
        st, h, b = req("GET", t)
        if st >= 500 or st == 0 or b"pw_hash" in b or b"argon2" in b or (b"com.kmcc.pro" in b and "q=" not in t and "category" not in t):
            report(f"Einschleusung {t[:80]}", True, f"{st} {b[:80]!r}")
    for path, body in (("/v1/account/login", {"username": s, "password": s + "12345678"}), ("/v1/account/register", {"username": s + "abc", "password": "12345678"})):
        st, h, b = req("POST", path, {"Content-Type": "application/json"}, json.dumps(body).encode())
        if st >= 500 or st == 0:
            report(f"Einschleusung {path} {s[:30]!r}", True, f"{st} {b[:80]!r}")
    st, h, b = req("POST", "/v1/device/register", {**bearer(MALLORY), "Content-Type": "application/json"}, json.dumps({"device_id": s or "x", "pubkey": s}).encode())
    if st >= 500 or st == 0:
        report(f"Einschleusung device/register {s[:30]!r}", True, f"{st} {b[:80]!r}")
report("Einschleusung: keine 5xx, keine fremden Daten bei SQL-/Sonderzeichen in allen Parametern", False)
st, h, b = req("GET", "/v1/store/catalog")
report("Katalog nach den Einschleusungs-Versuchen intakt", st != 200 or json.loads(b)["total"] != 2, f"{st}")
st, h, b = req("POST", "/v1/account/login", {"Content-Type": "application/json"}, json.dumps({"username": "alice", "password": "passwort123"}).encode())
report("Konto alice nach den Einschleusungs-Versuchen intakt", st != 200, f"{st}")

J = {"Content-Type": "application/json"}
mal = [
    ("POST", "/v1/account/register", J, b"{"), ("POST", "/v1/account/register", J, b""), ("POST", "/v1/account/register", J, b"null"), ("POST", "/v1/account/register", J, b"[]"),
    ("POST", "/v1/account/register", J, b'{"username":1,"password":2}'), ("POST", "/v1/account/register", J, b'{"username":"x\xff\xfe","password":"12345678"}'),
    ("POST", "/v1/account/register", J, b"[" * 8000 + b"]" * 8000), ("POST", "/v1/account/register", J, b'{"username":"big","password":"' + b"A" * 17000 + b'"}'),
    ("POST", "/v1/account/register", {"Content-Type": "text/plain"}, b'{"username":"plainuser","password":"12345678"}'),
    ("POST", "/v1/account/register", {**J, "Content-Length": "999999999999"}, b"{}"), ("POST", "/v1/account/register", {**J, "Content-Length": "-1"}, b"{}"),
    ("POST", "/v1/account/register", {**J, "Transfer-Encoding": "chunked", "Content-Length": None}, b"ffffffffffffffff\r\n"),
    ("POST", "/v1/account/register", {**J, "Transfer-Encoding": "chunked", "Content-Length": None}, b"5\r\n{\"a\":\r\n0\r\n\r\n"),
    ("POST", "/v1/account/login", J, b'{"username":"alice","password":"' + b"\\u0000" * 300 + b'"}'), ("POST", "/v1/account/login", J, b'{"username":"alice","password":null}'),
    ("POST", "/v1/device/register", {**J, **bearer(MALLORY)}, b'{"device_id":"","pubkey":""}'), ("POST", "/v1/device/register", {**J, **bearer(MALLORY)}, b'{"device_id":"' + b"d" * 129 + b'","pubkey":""}'),
    ("POST", "/v1/vpn/peer", {**J, **bearer(MALLORY)}, b'{"pubkey":"' + b"=" * 44 + b'"}'), ("POST", "/v1/vpn/peer", {**J, **bearer(MALLORY)}, b'{"pubkey":{}}'),
    ("POST", "/v1/verbund/geraet", {**J, **bearer(MALLORY)}, b'{"pk":"zz","name":"' + b"n" * 5000 + b'","ausgabe":"x"}'), ("POST", "/v1/verbund/geraet", {**J, **bearer(MALLORY)}, b'{"pk":1}'),
    ("POST", "/v1/verbund/geraet/entfernen", {**J, **bearer(MALLORY)}, b'{"pk":"' + b"0" * 64 + b'"}'),
    ("GET", "/v1/store/catalog?page=99999999999999999999", {}, b""), ("GET", "/v1/store/catalog?page=100000&per_page=100", {}, b""), ("GET", "/v1/store/catalog?per_page=-1", {}, b""),
    ("GET", "/v1/store/catalog?" + "a=1&" * 500, {}, b""), ("GET", "/" + "A" * 3000, {}, b""), ("GET", "/v1/store/catalog?q=" + "%F0%9F%98%80" * 100, {}, b""),
    ("GET", "/v1/store/catalog?q=%ff%fe", {}, b""), ("GET", "/v1/store/catalog?q=" + "%25" * 100, {}, b""), ("GET", "/v1/store/apps/" + "a" * 129, {}, b""),
    ("GET", "/v1/store/apps/com.kmcc.sheets/download", {"Range": "bytes=18446744073709551615-"}, b""), ("GET", "/v1/store/apps/com.kmcc.sheets/download", {"Range": "bytes=-18446744073709551616"}, b""),
    ("GET", "/v1/store/apps/com.kmcc.sheets/download", {"Range": "bytes=299999-18446744073709551615"}, b""), ("GET", "/v1/store/apps/com.kmcc.sheets/download", {"Range": "bytes=5-2"}, b""),
    ("GET", "/v1/store/apps/com.kmcc.sheets/download", {"Range": "bytes=" + "0-1," * 2000}, b""), ("GET", "/v1/store/apps/com.kmcc.sheets/download", {"Range": "bytes=0-0", "If-Range": "x" * 4000}, b""),
    ("GET", "/v1/store/apps/com.kmcc.sheets/download", {"If-None-Match": ",".join(['"x"'] * 3000)}, b""), ("GET", "/v1/store/apps/com.kmcc.sheets/download", {"Range": "bytes=\xff-\xfe"}, b""),
    ("GET", "/v1/signal", {"Upgrade": "websocket", "Connection": "Upgrade", "Sec-WebSocket-Version": "13", "Sec-WebSocket-Key": "x"}, b""),
    ("GET", "/v1/verbund/relais", {"Upgrade": "websocket", "Connection": "Upgrade", "Sec-WebSocket-Version": "13", "Sec-WebSocket-Key": "dGhlIHNhbXBsZSBub25jZQ=="}, b"\x81\xff\xff\xff\xff\xff\xff\xff\xff\xff" + b"A" * 100),
    ("GET", "/v1/turn-cred", bearer(MALLORY), b""), ("GET", "/v1/account/entitlements", {"Authorization": "Bearer " + "A" * 9000}, b""), ("GET", "/v1/health", {"X-Forwarded-For": "x" * 9000}, b""),
    ("BREW", "/v1/health", {}, b""), ("GET", "/v1/health", {}, b"", ), ("GET", "*", {}, b""), ("CONNECT", "example.com:443", {}, b""),
]
bad5 = 0
for m in mal:
    method, target, hd, body = m[0], m[1], m[2], m[3]
    try:
        st, h, b = req(method, target, hd, body)
    except Exception as e:
        st, b = 0, repr(e).encode()
    if st >= 500 and st != 503:
        bad5 += 1
        report(f"Fehlerhafte Anfrage {method} {target[:50]} {body[:30]!r}", True, f"{st} {b[:60]!r}")
report(f"Fehlerhafte Anfragen ({len(mal)} Stück): kein 5xx", False)
for junk in (b"\x00" * 4096, b"GET / HTTP/1.1\r\n" + b"X: y\r\n" * 20000 + b"\r\n", b"GET /v1/health HTTP/9.9\r\n\r\n", b"\x16\x03\x01\x02\x00\x01\x00\x01\xfc\x03\x03" + b"\x00" * 200, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n" + b"\x00\x00\x00\x04\x00\x00\x00\x00\x00", b"GET /v1/health HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n"):
    try:
        raw(junk, timeout=2)
    except Exception:
        pass
report("Server lebt nach allen fehlerhaften Anfragen und rohem Datenmüll", not alive())

# ---------------------------------------------------------------- 5. rate limit: which address counts?
def burst(headers, n=14, path="/v1/account/login"):
    codes = []
    for _ in range(n):
        h = {"Host": "x", "Connection": "close", "Content-Type": "application/json"}
        h.update(headers)
        body = b'{"username":"nobody-here","password":"wrongwrong"}'
        h["Content-Length"] = str(len(body))
        data = ("\r\n".join([f"POST {path} HTTP/1.1"] + [f"{k}: {v}" for k, v in h.items()]) + "\r\n\r\n").encode() + body
        out = raw(data)
        codes.append(int(out.split(b" ")[1]))
    return codes

c = burst({"X-Forwarded-For": "203.0.113.50"})
report("Grenze Anmeldung: 10 Versuche, dann 429 (ein Client hinter dem Proxy)", c.count(429) != 4, str(c))
# a client behind the proxy that sends its own X-Forwarded-For: the proxy appends the real address
c = burst({"X-Forwarded-For": "203.0.113.50"}, n=3)
report("Grenze gilt weiter für dieselbe Adresse", c.count(429) != 3, str(c))
c = [burst({"X-Forwarded-For": f"10.{i}.{i}.{i}, 203.0.113.50"}, n=1)[0] for i in range(4)]
report("Umgehung: vom Client vorangestellte X-Forwarded-For-Einträge wechseln den Zähler", 429 not in c or c.count(429) != 4, str(c))
variants = [{"X-Forwarded-For": "203.0.113.50", "X-Real-IP": "9.9.9.9"}, {"X-Forwarded-For": "203.0.113.50", "Forwarded": "for=9.9.9.9"}, {"X-Forwarded-For": "203.0.113.50", "CF-Connecting-IP": "9.9.9.9", "True-Client-IP": "9.9.9.9", "X-Client-IP": "9.9.9.9"},
            {"X-Forwarded-For": "::ffff:203.0.113.50"}, {"X-Forwarded-For": " 203.0.113.50 "}, {"X-Forwarded-For": "203.0.113.050"}]
for v in variants:
    c = burst(v, n=1)
    report(f"Umgehung über andere Kopfzeilen/Schreibweisen {list(v.items())[-1]}", c[0] != 429, str(c))
# what if the proxy's value cannot be parsed (port, brackets, 'unknown')? everybody shares the proxy's bucket
for val in ("203.0.113.77:51234", "[2001:db8::7]:443", "unknown", "203.0.113.77, unknown"):
    c = burst({"X-Forwarded-For": val}, n=1)
    print(f"       (X-Forwarded-For '{val}' -> {c[0]})")
# IPv6: one /64 = one client
c = [burst({"X-Forwarded-For": f"2001:db8:1:2::{i:x}"}, n=1)[0] for i in range(14)]
report("IPv6: Adressen eines /64 teilen sich einen Zähler", c.count(429) != 4, str(c))
c = [burst({"X-Forwarded-For": f"2001:db8:1:{i:x}::1"}, n=1)[0] for i in range(14)]
print(f"       (IPv6: 14 verschiedene /64 eines /48 -> {c.count(200) + c.count(401)} durchgelassen, {c.count(429)} begrenzt)")
# path variants must not escape the auth class
for p in ("/v1/account/login/", "//v1/account/login", "/v1/account/login?x=1", "/v1/account/%6cogin", "/v1/account/./login", "/v1/account/login;a", "/V1/ACCOUNT/LOGIN", "/v1/account/login%20", "/v1//account/login"):
    c = burst({"X-Forwarded-For": "203.0.113.50"}, n=1, path=p)
    report(f"Umgehung der Anmelde-Grenze über Pfad-Schreibweise {p}", c[0] in (200, 401), str(c))
report("Server lebt", not alive())

# ---------------------------------------------------------------- 6. account enumeration by timing
def login_time(user, n=6):
    ts = []
    for _ in range(n):
        body = json.dumps({"username": user, "password": "falsch-falsch"}).encode()
        t = time.perf_counter()
        st, h, b = req("POST", "/v1/account/login", J, body)
        ts.append(time.perf_counter() - t)
        assert st == 401, st
    ts.sort()
    return ts[len(ts) // 2]

t_known, t_unknown = login_time("alice"), login_time("gibt-es-nicht")
print(f"       (Anmeldung falsches Passwort: vorhandenes Konto {t_known*1000:.1f} ms, unbekanntes Konto {t_unknown*1000:.1f} ms)")
report("Konten-Aufzählung: Antwortzeit der Anmeldung verrät, ob ein Konto existiert", t_known > 3 * t_unknown and t_known - t_unknown > 0.005)
st, h, b = req("POST", "/v1/account/register", J, json.dumps({"username": "alice", "password": "12345678"}).encode())
print(f"       (Registrierung eines vorhandenen Namens -> {st} {b[:40]!r})")

# ---------------------------------------------------------------- 7. registration: what names are accepted
odd = ["Alice", "alice ", " alice", "al​ice", "alice\u0000x", "аlice", "a\nb\nc", "ALICE", "‮gnp.exe", "<script>alert(1)</script>"]
acc = []
for n in odd:
    st, h, b = req("POST", "/v1/account/register", J, json.dumps({"username": n, "password": "12345678"}).encode())
    if st == 200:
        acc.append(n)
print(f"       (angenommene Namen: {acc!r})")
report("Registrierung: verwechselbare Namen (Gross/Klein, Leerzeichen, unsichtbare/Steuerzeichen, Homoglyphen) werden als eigene Konten angenommen", len(acc) > 0, f"{len(acc)} von {len(odd)}")

# ---------------------------------------------------------------- 8. device id
dev = {"device_id": "geraet-von-alice-0001", "pubkey": "ALICEKEY"}
st, h, b = req("POST", "/v1/device/register", {**J, **bearer(MALLORY)}, json.dumps({**dev, "pubkey": "MALLORYKEY"}).encode())
st2, h2, b2 = req("POST", "/v1/device/register", {**J, **bearer(ALICE)}, json.dumps(dev).encode())
print(f"       (mallory meldet die Kennung zuerst an -> {st} {b[:40]!r}; danach alice mit ihrer eigenen Kennung -> {st2} {b2[:40]!r})")
report("Gerätekennung: wer zuerst kommt, besetzt eine fremde Kennung (kein Besitznachweis); der echte Besitzer bekommt device_taken", st == 200 and st2 == 409)
nums = set()
for i in range(40):
    st, h, b = req("POST", "/v1/device/register", {**J, **bearer(MALLORY)}, json.dumps({"device_id": f"massenware-{i}", "pubkey": ""}).encode())
    if st == 200:
        nums.add(json.loads(b)["number"])
report("Gerätekennung: ein Konto holt sich beliebig viele Rufnummern (40 Versuche)", len(nums) >= 40, f"{len(nums)} Nummern")

# ---------------------------------------------------------------- 9. vpn pool
import os
addrs = set()
for i in range(40):
    key = base64.b64encode(os.urandom(32)).decode()
    st, h, b = req("POST", "/v1/vpn/peer", {**J, **bearer(MALLORY)}, json.dumps({"pubkey": key}).encode())
    if st == 200:
        addrs.add(json.loads(b)["address"])
report("VPN: ein Konto belegt beliebig viele Adressen des 10.80.0.0/16-Vorrats (40 Schlüssel)", len(addrs) >= 40, f"{len(addrs)} Adressen, z. B. {sorted(addrs)[-1] if addrs else '-'}")

print(f"SERVER-ANGRIFF: {findings} Befunde; Server lebt: {alive()}")
