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
- Ein/Aus: `ultar-sshd an|aus`, `/data/ultar/entwickler/an` bzw. `aus`, `/etc/ultar/entwickler` im Abbild
  oder Startparameter `ultar.entwickler=1`. „aus" gewinnt. Der Schalter in den Einstellungen
  (`sharing.ssh`) ist noch **nicht** angeschlossen (siehe Offen).
- **Ohne Neustart**: der Dienst bleibt ohne offenen Port im Leerlauf, bis er eingeschaltet ist **und** ein
  Schlüssel erlaubt ist (Blick auf die Dateien alle 2 s), und öffnet dann den Port – etwa wenn der erste
  Start den Schlüssel aus dem Installer gerade erst einträgt, nach `ultar-sshd an` oder einem Schlüssel aus
  der Web-Verwaltung. Wird er zugeschaltet (`aus`, letzter Schlüssel weg), endet der Prozess (Code 75):
  Port und alle Sitzungen sind zu, `ultar-init` startet ihn neu und er wartet wieder.
- **Server-Ausgabe**: liefert `/etc/ultar/entwickler` mit – der Zugang ist dort standardmäßig an, nimmt
  aber nur eingetragene Schlüssel an. Ohne Schlüssel kommt niemand hinein.
- Schlüssel: `/data/ultar/entwickler/authorized_keys` und `/etc/ultar/entwickler.keys` (Abbild). Hinein
  kommen sie über
  - den **Installer**: Schritt „Konto", Feld „SSH-Schlüssel des PCs (freiwillig)" – eine Zeile
    `ssh-ed25519 …` oder `ecdsa-sha2-nistp256 …` (geprüft: Typ, Base64, Schlüsselblock passt zum Typ).
    Der erste Start (`ultar-firstboot --developer-dir`) trägt ihn ein (Ordner 0700, Datei 0600, keine
    Verknüpfung folgen, nie doppelt). Auf dem Server ist SSH damit direkt nach der Installation nutzbar;
  - die Web-Verwaltung → SSH-Schlüssel (`system/services/ultar-admin`);
  - `ultar-sshd schluessel "<öffentlicher Schlüssel>"` (auch RSA).
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
Kennwort abgewiesen; 3 Sitzungen nacheinander und 2 gleichzeitig; Hochladen hält mit dem Herunterladen
Schritt; `ultar-sshd aus` schließt den Zugang zur Laufzeit. Grün auf .12 und .8.
`installer-e2e.sh`: der Schlüssel wird im Installer getippt, der erste Start der installierten Platte meldet
`ssh_key=Added`.

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

1. **Schalter in den Einstellungen** (`sharing.ssh`, nur Verwalter): bewusst noch nicht angeschlossen. Der
   Dienst ist eine Verwalter-Shell; ihn über den Systembus zu schalten braucht eine eigene Bus-Kennung für
   den Dienst (Regel in `bus-policy.toml`, Sockel in `ultar-busd`) und eine eigene Sicherheitsprüfung
   (Anzeige auf dem Sperrbildschirm, wer die Schlüsselliste ändern darf). Bis dahin: Installer,
   Web-Verwaltung, `ultar-sshd an|aus`.
2. Weiterhin Wiederholungen im Mitschnitt (der erste Verlust selbst – Empfangsring/Abfragetakt ~100 Hz);
   jetzt billig. Fensterskalierung (> 64 KiB) fehlt.
3. An echter Hardware prüfen: Beelink direkt gestartet (S9), Zugang über das 10G-/USB-C-Netz.
