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

## Messwerte (QEMU, slirp-Netz, 16 MiB per scp)

| Rechner | hoch (PC → Gerät) | herunter | hoch ohne Platte (`cat > /dev/null`) |
|---|---|---|---|
| .12, vorher | 52,8–58,9 s | 3,4–5,0 s | 43,7–55,7 s |
| .12, jetzt | 5,8–6,0 s | 4,6–5,0 s | 8,8–9,1 s |
| .8, jetzt | 2,3 s | 1,6 s | 7,2 s |

**Ursache des langsamen Empfangs (gemessen mit einem Mitschnitt des Gast-Netzes):** von 28 412 Datensegmenten
zum Gerät waren 10 921 Wiederholungen, dazu Pausen von 1,4–3 s (Zeitablauf des Senders). Der eigene TCP-Stapel
verwarf jedes Segment, das nach einem Verlust vorauseilte – ein Verlust kostete so das ganze Fenster und einen
Zeitablauf. Jetzt (`kernel/ultar-kern/src/socket.rs`):
- vorauseilende Segmente werden begrenzt aufbewahrt (`ooo`, höchstens 64 je Verbindung, innerhalb des Fensters)
  und beim Schließen der Lücke in den Strom übernommen; Überlappungen von Wiederholungen werden richtig zugeschnitten;
- das angekündigte Fenster ist der freie Empfangspuffer (statt fest 65535) – der Sender schickt nie mehr, als Platz ist;
- öffnet sich ein geschrumpftes Fenster wieder, geht sofort eine Fenster-Meldung hinaus.
`ssh-e2e.sh` prüft das: Hochladen darf nicht weit hinter dem Herunterladen liegen (≤ 3 × herunter + 5 s).

## Offen

1. Weiterhin viele Wiederholungen im Mitschnitt (der erste Verlust selbst – Empfangsring/Abfragetakt ~100 Hz);
   jetzt billig, aber eine Ursache für noch mehr Tempo. Fensterskalierung (> 64 KiB) fehlt.
2. Schlüssel auf ein frisch installiertes Gerät bringen: heute über die Web-Verwaltung oder das Abbild;
   ein Schritt im Installer fehlt.
3. Testplatte (`make-test-disk.sh`): auf Rechnern mit Rust-Coreutils (ein 11-MB-Programm für alle Befehle,
   Ubuntu 26.04) wird es jetzt nur einmal kopiert, die anderen Namen verweisen darauf – vorher lief die Platte über.
