# Zelle der Seite für die Servo-Engine (`external/servo-host`, Paket `servocell`)

KM Code Craft · Ultar OS v2 · Stand 28.11.2026 · gehört zu `docs/integration/servo-engine.md` und
`docs/integration/ultar-web.md`

## Kurz

Der Engine-Host `ultar-web-engine` läuft in seiner **Zelle ohne eigenes Netz**: neuer Mount-, Prozess-,
IPC- und **Netz-Namensraum** (darin nur ein privates Loopback). Servo erreicht das Internet nur noch
über einen Loopback-Port **in der Zelle**; ein Relais im Host kopiert jede Verbindung auf den
**Unix-Socket des Tor-Clients** des Browsers (`ultar-tor`, dritte Front, Modus 0600). Das ist der einzige
Weg hinaus. WebSockets, die Servo 0.8 ohne Proxy direkt aufbaut, finden keine Route und scheitern –
sie gehen nie am Tor vorbei.

Jeder Fehler beim Bau der Zelle beendet den Host **vor** dem Start von Servo (fail closed). Auf dem
**Ultar-Kern** kann die Zelle heute nicht gebaut werden (kein Netz-Namensraum, siehe unten): dort
verweigert der Host den Start mit Zelle, statt mit Netz zu laufen; der Browser zeigt dann den Lese-Modus.

## Wege

```
Seite (Servo, in der Zelle)  ──TCP──▶  127.0.0.1:PORT (privates Loopback der Zelle)
        Relais-Thread des Hosts (src/relay.rs)  ──Unix-Socket──▶  <engine>/tor.sock
        ultar-tor (Browser-Prozess, dritte Front)  ──▶  Tor  ──▶  Seite
```

| Teil | Datei | Aufgabe |
|---|---|---|
| Tor, dritte Front | `platform/ultar-tor/src/unix.rs` | HTTP-`CONNECT` auf einem Unix-Socket (`Tor::unix_proxy(pfad)`); Pfad geprüft (absolut, sauber, ≤ 100 Byte, Ordner da); Socket wird in einem privaten Ordner (0700) gebunden, auf 0600 gesetzt und dann an den Pfad verschoben (nie ein Moment mit weiterem Modus, keine prozessweite `umask`); alter Socket wird ersetzt, alles andere am Pfad abgelehnt; kein Geheimnis nötig (der Dateimodus schützt), dieselben Abweisungen (dieses Gerät, lokales Netz) und Kreise je Website wie bei den anderen Fronten |
| Browser | `apps/ultar-web/src/cell.rs`, `src/bin/ultar-web.rs`, `src/engine/remote.rs` | `CellSpec::page`: **`network: false`**, rw nur der Engine-Ordner `<Profil>/engine` (mit `tor.sock`) und Downloads – nicht mehr das ganze Profil mit dem Tor-Zustand; `HostConfig::proxy_socket` → `--proxy-socket`; ohne Zelle (`--no-cell`, Entwicklung) bleibt `--proxy` (TCP mit Geheimnis) |
| Host: Plan | `external/servo-host/src/cell/mod.rs` | prüft die Befehlszeile (rein, getestet): `--cell-net` wird abgelehnt, `--proxy` neben einer Zelle auch, `--proxy-socket` Pflicht und innerhalb eines rw-Ordners, Pfade und Speichergrenze nach den Regeln des Browsers (`ultar_web::cell`) |
| Host: Stufen | `src/cell/stage.rs`, `src/cell/view.rs` | siehe unten |
| Host: Relais | `src/relay.rs` | lauscht auf `127.0.0.1:<freier Port>` der Zelle, kopiert jede Verbindung (höchstens 128 gleichzeitig) auf den Unix-Socket, beide Richtungen |
| Host: Selbsttest | `src/cell/selftest.rs`, `--cell-selftest` | prüft **in der fertigen Zelle** (derselbe Code wie beim echten Start bis zum Servo-Start) und meldet je Prüfung eine Zeile |
| Hilfen | `system/hal/ultar-sys/src/cellnet.rs` (+ `raw::unshare_user`) | `loopback_up` (Netlink `RTM_NEWLINK`), `enter_user_namespace`, `drop_capabilities` – additiv, sicher; `unsafe` bleibt in `raw.rs` mit `SAFETY`-Kommentar |

### Die zwei Stufen (wie der Starter, `system/core/ultar-launcher/src/cell`)

1. **Außen** (der Prozess, den der Browser startet, einfädig): Plan prüfen → Speichergruppe
   (`/sys/fs/cgroup/ultar-web-<pid>`, `memory.max`, `memory.swap.max=0`, `memory.oom.group=1`, Prozess tritt
   bei; wird am Ende verlassen und gelöscht) → Wurzelordner `$TMPDIR/ultar-web-cell-<pid>` (0700) → wenn
   nicht root: Benutzer-Namensraum (Ids auf sich selbst abgebildet) → `unshare(pid, net, ipc)` (die Einh~angungen kommen erst innen: Stufe 1 beh~alt den Dateibaum des Hosts, um am Ende aufzur~aumen) →
   Einhängungen privat → startet **sich selbst** mit `--cell-init <Wurzel>` (kein `unsafe`, kein
   `pre_exec`) und wartet; Rückgabe: dessen Exit-Code.
2. **Innen** (pid 1 der Zelle; verweigert, wenn es nicht pid 1 ist): eigener Mount-Namensraum, Einh~angungen privat, Ansicht bauen (tmpfs-Wurzel; Bündel und
   Systemordner **nur lesbar**; Profil/Downloads lesbar+schreibbar, **nie ausführbar**; privates `/tmp` 256 MiB;
   eigenes `/proc`; `/dev` mit `null zero full random urandom` und `shm`) → `pivot_root`, alte Wurzel
   abhängen, Wurzel nur lesbar → Loopback hochfahren (Netlink), einen Port binden und die Verbindung zu sich selbst beweisen → keine neuen
   Privilegien (`no_new_privs`), **alle Capabilities abgelegt** (und geprüft), Kern-Limit 0; auf dem
   Ultar-Kern zusätzlich **alle Ultar-Rechte** abgelegt und geprüft (nicht beobachtet, siehe Offen) →
   Relais starten → Servo mit `--proxy http://127.0.0.1:<Port>`.

Der Host stirbt mit seinem Elternprozess (`PR_SET_PDEATHSIG`); Fehler in einer Stufe beenden den Prozess
mit Meldung `cell failed at <Schritt>: …` und Exit 1.

## Befund Ultar-Kern: was fehlt (genau)

`unshare` im Ultar-Kern (`kernel/ultar-kern/src/isolation/calls.rs`, `sys_unshare`) nimmt nur
`CLONE_NEWNS`, `CLONE_NEWPID` und `CLONE_FS/FILES/SYSVSEM` an; `CLONE_NEW_UNSUPPORTED` nennt cgroup, uts, **ipc**,
user und **net** (`0x4000_0000`) – jedes davon gibt `EINVAL`, und weil die Flags zusammen geprüft werden,
**bewirkt der ganze Aufruf nichts**. Der Host fragt sie in einem Aufruf an, bekommt `EINVAL`, erkennt den
Ultar-Kern an der Antwort auf den Rechte-`prctl` und beendet sich mit
`cell failed at namespaces: errno 22; this is the Ultar kernel: it has no network namespace … the host
refuses to run with network instead`.

Dahinter fehlt für eine Zelle ohne Netz:

1. **Netz-Namensraum:** `socket.rs` hat **eine** globale Socket-Tabelle, **eine** Adresse (`my_ip`) und
   Loopback-Zustellung (`is_local`) für alle; ein Socket kennt keinen Namensraum. Nötig: ein Namensraum-Marker
   am Socket (und an `bind`/`connect`/`accept`/Zustellung: nur gleicher Marker sichtbar), ein eigenes
   Loopback je Namensraum, **keine** Route nach außen für neue Namensräume, `CLONE_NEWNET` in `sys_unshare`
   (gesetzt wie `CLONE_NEWPID` für den Aufrufer und seine Kinder, Recht `MOUNT` oder ein neues Recht).
   Ein Recht „nur Loopback“ (`bind`/`connect` nur auf 127.0.0.0/8) genügt **nicht**: das Loopback wäre das
   gemeinsame des ganzen Systems, die Zelle könnte alle lokalen Dienste (auch Tor-Ports) erreichen.
2. **IPC-Namensraum:** der Kern hat keine SysV-IPC-Aufrufe (`shmget`, `msgget`, `semget` sind nicht
   verteilt). Solange `CLONE_NEWIPC` mit `EINVAL` endet, scheitert auch ein sonst gültiger Aufruf; der Kern
   muss das Flag annehmen (nichts zu isolieren), oder der Host lässt `ipc` dort weg – das ist erst
   sinnvoll, wenn es `CLONE_NEWNET` gibt.
3. **Loopback hochfahren:** der Host schickt immer zuerst Netlink `RTM_NEWLINK` (`lo` hoch), bindet dann `127.0.0.1` und verbindet sich selbst mit diesem Port; erst dieser Beweis lässt die Zelle weiterlaufen (sonst Fehler "loopback", der Host endet). Der Ultar-Kern kennt `AF_NETLINK` nur für Geräteereignisse (`devices::events::socket`), nicht `NETLINK_ROUTE`: ein neuer Namensraum muss sein Loopback dort also schon **oben** haben; der Netlink-Aufruf darf fehlschlagen, der Verbindungsbeweis entscheidet.
4. **`AF_UNIX`** ist da (`sys_socket` Domäne 1, `socketpair`), auch Socket-Dateien in einer gebundenen
   Ordner-Ansicht und `SCM_RIGHTS` für die Bildpuffer – das Relais und der Link zum Browser brauchen nichts weiter.
5. **Speichergruppe:** der Host schreibt `memory.max`, `memory.swap.max`, `memory.oom.group` und `cgroup.procs`
   wie unter Linux (cgroup v2); ob die Gruppen des Ultar-Kerns diese Dateien so annehmen, ist **nicht
   geprüft** (siehe Offen).
6. **Benutzer-Namensraum** braucht der Ultar-Kern nicht (Rechte statt root); der Host nimmt ihn nur, wenn
   er unter Linux nicht root ist.

`kernel/ultar-kern/src/socket.rs` und `syscall.rs` wurden **nicht** verändert (anderes Team).

## Belege (28.11.2026, Builder 192.168.100.12, Linux 6.8, root, cgroup v2)

- Selbsttest in der Zelle (`--cell-selftest`): **13 von 13** Prüfungen ok (kein `/home`, Wurzel nur Ansicht, nur eigene Prozesse, keine Capabilities und `no_new_privs`, nur `lo`, keine Route zu 1.1.1.1 / LAN / 10.x / IPv6, Ordner wie deklariert, privates `/tmp`, nur harmlose Geräte, Relais erreicht den Browser-Socket).
- Integrationstest `tests/cell.rs` (3 Tests, auch die Verweigerung ohne Zelle/ohne Proxy): 3 von 3 Mal grün.
- Seite durch Zelle und Tor: `shot` (Servo in der Zelle, Tor-Proxy über den Unix-Socket) holte die Tor-Prüfseite; Bild `shot-cell-check-torproject.png` (Ladezeit 11,1 s, Tor-Sonde 7,6 s).
- Aufräumen: nach dem Ende bleibt weder `/sys/fs/cgroup/ultar-web-<pid>` noch `/tmp/ultar-web-cell-<pid>` stehen (vorher 0, nachher 0). Früher blieben sie stehen, weil `pivot_root` den gemeinsamen Mount-Namensraum auch für Stufe 1 umstellte; jetzt macht Stufe 2 den Namensraum selbst.
- Tests: `ultar-sys`, `ultar-tor`, `ultar-web`: alle 108 Tests grün; `clippy -D warnings`, `fmt` und `ultar-check all` ok. Im Host-Crate scheitern **schon auf der Basis** `frames::tests::only_changed_rows_are_written_and_reported` (Test) sowie clippy `input.rs:150` (`clone_on_copy`) und `persona.rs:22` (`field_reassign_with_default`); nicht Teil dieses Pakets, nicht angefasst.

## Offen (ehrlich)

- **Ultar-Kern:** die Zelle ist dort nicht baubar (siehe Befund); das Verweigern ist nur durch die
  Auswertung der Antwort `EINVAL` belegt (Code-Pfad `unshare_words`), **nicht** auf einer laufenden
  Ultar-Kern-Maschine ausgeführt. Das Ablegen der Ultar-Rechte im Host (`confine`) und die Speichergruppe
  dort sind ungeprüft.
- **Nicht root unter Linux:** der Pfad über einen Benutzer-Namensraum ist gebaut, aber **nicht bewiesen**:
  der Builder (Ubuntu 24.04) hat `kernel.apparmor_restrict_unprivileged_userns=1`; außerdem kann ein
  Nicht-root-Prozess meist keine Speichergruppe anlegen (es sei denn, die Sitzung hat eine delegierte
  Gruppe; der Host sucht sie nicht), und dann **verweigert** er den Start. Der Starter (Launcher) des
  Systems tut dasselbe nur als root. Produktiv gilt: der Browser läuft mit dem Recht, die Zelle zu bauen
  (root, oder auf dem Ultar-Kern das Recht `MOUNT`).
- **Bündel:** Servo braucht in der Zelle die CA-Zertifikate des Systems (`rustls-platform-verifier`:
  ohne `/etc/ssl/certs` bricht der Ressourcen-Thread ab: „No CA certificates were loaded from the system“)
  und die Schriftdateien/`/etc/fonts`. Das Ultar-Bündel `/usr/lib/ultar/web` muss beides tragen (und
  `SSL_CERT_FILE` bzw. den Standardpfad bedienen); im Test werden `/etc/ssl` und `/etc/fonts` zusätzlich
  eingeblendet (`ULTAR_SHOT_CELL_RO`).
- **Verwaiste Reste:** wird der Außenprozess hart beendet (`SIGKILL`), bleiben sein Wurzelordner
  `ultar-web-cell-<pid>` (leer) und die Speichergruppe stehen; der Innenprozess stirbt (pdeathsig). Beim
  nächsten Start mit gleicher pid wird der leere Ordner ersetzt; die Gruppe nicht. Kein Aufräumen beim
  Systemstart.
- **Änderung äußerhalb der erlaubten Dateien:** `tools/check-layers.toml` (eine Erlaubnis für `external` -> `ultar-sys`, nötig für die Schichtenprüfung); sonst keine; `apps/ultar-web/src/lib.rs` nennt im Kopf noch
  „mit Netz“ (Kommentar, nicht geändert, weil nicht in der Dateiliste des Pakets).
