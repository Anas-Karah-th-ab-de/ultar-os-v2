# SSH auf den Ultar-Kern – `system/services/ultar-sshd`

Stand 28.11.2026.

Ziel: vom PC per SSH auf einen Rechner mit Ultar OS (eigener Kern) – Befehle, Dateien kopieren,
Weiterleitungen – über Ethernet oder USB-C (`docs/integration/usb-net.md`).

## Aufbau

- `ultar-sshd` ist reines Rust (russh, russh-sftp, tokio), statisch gebaut und läuft direkt auf dem
  Ultar-Kern (Linux-ABI) – **kein Ubuntu-Bereich, kein OpenSSH**.
- **Ein** Dienst: `rootfs/etc/ultar/services/entwickler.toml` startet ihn (Port **2222**). Die früheren
  `sshd.toml` der Server-/Desktop-Ausgabe (OpenSSH aus dem Ubuntu-Bereich) sind entfernt – zwei
  Dienste hätten um denselben Port gestritten.
- Ein/Aus: Einstellungen → System → Entwickleroptionen, `ultar-sshd an|aus`, `/data/ultar/entwickler/an`
  bzw. `aus`, `/etc/ultar/entwickler` im Abbild oder Startparameter `ultar.entwickler=1`. „aus" gewinnt.
- **Server-Ausgabe**: liefert `/etc/ultar/entwickler` mit – der Zugang ist dort standardmäßig an, nimmt
  aber nur eingetragene Schlüssel an. Ohne Schlüssel kommt niemand hinein.
- Schlüssel: `/data/ultar/entwickler/authorized_keys` (Web-Verwaltung → SSH-Schlüssel schreibt jetzt
  genau dorthin, `system/services/ultar-admin`) und `/etc/ultar/entwickler.keys` (Abbild).
- Nur Schlüssel, kein Kennwort, keine Tastatur-Abfrage; Weiterleitung nur zu 127.0.0.1.

## Vom PC verbinden

```bash
ssh -p 2222 root@<adresse>                      # Befehle / Shell
scp -P 2222 datei root@<adresse>:/data/          # Dateien
sftp -P 2222 root@<adresse>
ssh -p 2222 -L 8080:127.0.0.1:8080 root@<adresse> # Gerätedienst auf den PC holen
```

## Geprüft (`tools/kern-test/ssh-e2e.sh`, Schlusszeile `SSH: ALLES OK`, in `ci.sh`)

QEMU mit dem Ultar-Kern, frischer Test-Schlüssel: erreichbar; Befehl + `uname` (Ultar-Kern); Exit-Code 7
kommt an; scp 1 MiB hin und zurück (sha256 gleich); sftp `ls`; fremder Schlüssel abgewiesen;
Kennwort abgewiesen; 3 Sitzungen nacheinander und 2 gleichzeitig. Grün auf .12 (mehrfach, auch im
Gesamtlauf von `ci.sh`).

## Messwerte (QEMU, slirp-Netz)

| Richtung | 16 MiB |
|---|---|
| Gerät → PC (scp herunter) | 3,4–5,0 s |
| PC → Gerät (scp hoch) | 52,8–58,9 s |
| PC → Gerät ohne Platte (`cat > /dev/null`) | 43,7–55,7 s |

## Offen

1. **Empfangen ist ~15× langsamer als Senden** (≈ 0,3 MB/s). Die Platte ist es nicht (Messung ohne Platte
   gleich langsam); 256 statt 64 Rahmen je Abfrage und ein größerer virtio-Empfangsring ändern nichts.
   Verdächtig im eigenen TCP (`kernel/ultar-kern/src/socket.rs`): das Fenster ist fest 65535 (auch bei
   vollem Empfangspuffer), Segmente außer der Reihe werden verworfen, Bestätigungen nur beim
   Netz-Abfragen (~100 Hz). Nächster Schritt: Fenster/Segmente je Abfrage messen, dann beheben.
2. Auf `.8` scheitert `ssh-e2e.sh` vor dem Start beim Bauen der Testplatte (`mkfs.ext4: Could not allocate
   block`), auf `.12` nicht – Ursache in der Testumgebung (Host-Werkzeuge in der Platte), nicht im Kern.
3. Schlüssel auf ein frisch installiertes Gerät bringen: heute über die Web-Verwaltung oder das Abbild;
   ein Schritt im Installer fehlt.
