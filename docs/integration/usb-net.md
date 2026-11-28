# USB-Netzwerk (RNDIS + CDC-ECM) – `drivers/gen_xhci/{rndis,cdc_ecm,net_common}.rs`

Stand 28.11.2026 · Zweig `claude/usb-net` (noch nicht auf `main`).

Ziel: ein Ultar-Gerät per USB-C ans Netz bringen – Handy-USB-Tethering (RNDIS) und USB-C-zu-Ethernet-
Dongles (CDC-ECM) – damit man künftig über diese Verbindung mit dem Gerät arbeiten kann.

## Aufbau

- `net_common.rs`: gemeinsamer Unterbau beider Treiber – dauerhaft armierte Bulk-IN-Übertragungen
  (`InEp`, je eine DMA-Seite pro Übertragung) und die 8/16/32-Bit-Zugriffe auf DMA-Puffer.
- `rndis.rs`: Remote NDIS (Steuerung als eingekapselte Befehle auf EP0, Ethernet-Rahmen mit 44-Byte-
  RNDIS-Kopf über Bulk-IN/OUT). Der Weg für Android-USB-Tethering und QEMUs `usb-net`.
- `cdc_ecm.rs`: Ethernet Networking Control Model (Steuer-Schnittstelle CDC/0x06, rohe Ethernet-Rahmen
  über die Bulk-Endpunkte der Daten-Schnittstelle). Wählt per `SET_INTERFACE` die Daten-Alternative,
  MAC aus dem „Ethernet Networking Functional Descriptor" (iMACAddress-Zeichenkette), Linkzustand/
  Geschwindigkeit aus Interrupt-Meldungen (`NETWORK_CONNECTION`/`SPEED_CHANGE`).
- Konfigurationswahl in `gen_xhci.rs`: pro Gerät werden bis zu zwei Konfigurationen gelesen; gewählt
  wird zuerst eine RNDIS-Konfiguration (Steuer-Schnittstelle 0x02/0x02/0xFF), sonst eine ECM-
  Konfiguration (0x02/0x06), sonst die erste. Wie Linux bevorzugt das also RNDIS, wenn ein Gerät
  (QEMU `usb-net`) beide anbietet.
- Abziehen: `rndis::remove`/`cdc_ecm::remove` nehmen die Netzkarte **sauber** aus `NET` (ein
  Platzhalter hält den Listenplatz, damit Indizes aktiv/ DHCP-Zustand stabil bleiben; `driver::remove_net`)
  und geben die DMA-Seiten frei. Ein erneutes Anstecken bindet eine neue Karte (`usb1`, `usb2`, …).

## Stand

| Teil | Stand |
|---|---|
| Binden (RNDIS) | ✅ QEMU (Grundlage `main`): Daten-Schnittstelle (0x0A) einer RNDIS-Funktion erkannt, `usb0` erscheint |
| RNDIS-Steuerung | ✅ QEMU (Grundlage `main`): INITIALIZE → `INIT_CMPLT`, Paketfilter, MAC über SEND/GET_ENCAPSULATED auf EP0 |
| Senden (RNDIS) | ✅ QEMU (Grundlage `main`): DHCP-DISCOVER erscheint im netdev-Mitschnitt, korrekt als RNDIS-Paket über Bulk-OUT |
| Empfang (RNDIS) | ⚠️ **in QEMU noch nicht bestätigt**: QEMUs `usb-net` ist Full-Speed (Bulk 64 Byte); der armiert wie der synchron gelesene Bulk-IN liefert in QEMU keine Completion, obwohl slirp die Antwort übergibt. Ursache ist noch **nicht bewiesen** – siehe „RX-Ursache (Schritt 1)" |
| CDC-ECM | 🆕 Treiber fertig (Bind, Daten-Alternative, MAC-Zeichenkette, Interrupt-Meldungen); in QEMU nicht vollständig prüfbar, weil die Konfigurationswahl RNDIS bevorzugt. Braucht die Prüfung an einem echten ECM-Dongle |
| Abziehen/Anstecken | 🆕 Netzkarte wird sauber entfernt und wieder neu gebunden (`usb-net-e2e.sh` prüft das) – noch nicht in QEMU bestätigt |

## RX-Ursache (Schritt 1) – was zu tun ist

Nicht „QEMU-Eigenheit" annehmen, sondern messen. Mit QEMU-Ereignisspuren:

```bash
TRACE=1 LOG=/tmp/un.log TRACE_LOG=/tmp/un-trace.log tools/kern-test/usb-net-qemu.sh
grep -a -e usb_xhci -e usb_packet /tmp/un-trace.log
```

Dabei prüfen und mit dem vergleichen, was Linux `xhci-hcd` für einen Full-Speed-Bulk-Endpunkt setzt:

- Endpoint-Kontext: Max Packet Size, EP-Typ, Average TRB Length, CErr, Max Burst, Interval
  (liegt in `config_ep_x` in `gen_xhci.rs`).
- Transfer-TRB: ISP/IOC-Flags, Transfer Length, TD-Size.
- Slot-Kontext: Port-Speed, Route String, TT-Felder (für Full-/Low-Speed hinter High-Speed-Hubs).
- Ob QEMU nach einem NAK den IN-Paket wieder an das Gerät stellt und warum die Completion ausbleibt.

Ist es unser Fehler: beheben. Ist es QEMU: die genaue QEMU-Stelle aus den Spuren belegen und hier
dokumentieren – und den Treiber trotzdem für echte High-Speed-Geräte korrekt halten.

## Prüfen (QEMU)

```bash
tools/kern-test/usb-net-qemu.sh            # bind + RNDIS + TX; RX (Full Speed) noch offen
TRACE=1 tools/kern-test/usb-net-qemu.sh    # + QEMU-Spur für die RX-Ursache
IMG=out/ultar-kern-esp.img tools/kern-test/usb-net-e2e.sh
#   RNDIS: bind, MAC, DHCP-Lease, ping 10.0.2.2, TCP zum Host (nettest), device_del/device_add
#   CDC-ECM: in QEMU übersprungen (RNDIS-Konfiguration wird zuerst gewählt) → "USBNET: TEILWEISE"
```

Finalzeile: `USBNET: ALLES OK` nur wenn alles lief; sonst `USBNET: TEILWEISE <liste>` (nie ein
gefälschtes OK). In `ci.sh` läuft `usb-net-e2e.sh` neben `netz-e2e.sh` und akzeptiert `ALLES OK`/`TEILWEISE`.

## Checkliste echte Hardware

1. **Android-USB-Tethering (RNDIS)**: Handy per USB-C an einen Ultar-Rechner, USB-Tethering an;
   `netz`/`dhcp` in der Kern-Shell. Dort läuft RNDIS mit High Speed – der RX-Pfad sollte tragen.
2. **ECM-Dongles**: übliche USB-C-zu-Ethernet-Dongles mit CDC-ECM (z. B. mit VID:PID der Chips
   AX88772/AX8817x, LAN7800, RTL8152 in CDC-ECM-Modus). Prüfen: `usb0` erscheint als `CDC-ECM`,
   MAC stimmt mit dem Aufkleber, `dhcp` holt eine Lease.
3. **Eigene Treiber (nächster Schritt)**: ASIX AX88179 (USB 3.0/Vendor-Klasse) und Realtek RTL8153
   sprechen **kein** CDC-ECM/NCM und brauchen je einen eigenen Treiber – als nächstes eintakten,
   nicht hier.
4. **CDC-NCM**: nur falls Zeit bleibt; sonst als nicht unterstützt dokumentieren.

## Offene Punkte

1. RX-Ursache in QEMU mit den Spuren beweisen (oben) und ggf. beheben.
2. RNDIS/ECM an echter Hardware bestätigen; erst dann nach `main`.
3. Eigene Treiber für AX88179 / RTL8153.