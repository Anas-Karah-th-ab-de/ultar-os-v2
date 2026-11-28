# USB-Netzwerk (RNDIS) – `drivers/gen_xhci/rndis.rs`

Stand 28.11.2026 · Zweig `claude/usb-net` (noch nicht auf `main`).

Ziel: ein Ultar-Gerät per USB-C ans Netz bringen (Handy-USB-Tethering, später USB-C-zu-Ethernet-Dongles),
damit man künftig über diese Verbindung mit dem Gerät arbeiten kann.

## Stand

| Teil | Stand |
|---|---|
| Binden | ✅ QEMU: die CDC-Daten-Schnittstelle (Klasse 0x0A) einer RNDIS-Funktion (Steuer-Schnittstelle 0x02/0x02/0xFF) wird erkannt und gebunden (`usb0` erscheint) |
| RNDIS-Steuerung | ✅ QEMU: INITIALIZE → INIT_CMPLT, Paketfilter setzen, MAC abfragen – über SEND/GET_ENCAPSULATED auf EP0 |
| Senden (TX) | ✅ QEMU: DHCP-DISCOVER erscheint im Mitschnitt des netdev (`filter-dump`), also korrekt als RNDIS-Paket über Bulk-OUT verpackt und zugestellt |
| Empfang (RX) | ⚠️ **in QEMU nicht bestätigt**: slirp übergibt das Antwortpaket an das Gerät (keine „Failed to send packet" mehr), aber der Bulk-IN-Endpunkt (dci 5) liefert in QEMU **keine** Completion – weder armiert noch synchron. Ursache: QEMUs `usb-net` ist ein **Full-Speed**-Gerät (speed=1, Bulk 64 Byte); das ist eine Eigenheit der QEMU-Full-Speed-Emulation. Echte RNDIS-Tethers laufen mit High Speed |
| An `main` | ❌ erst nach einer Prüfung an echter Hardware (Android-USB-Tethering), damit keine Netzkarte angemeldet wird, die nicht empfängt |

## Aufbau

- Empfang wie `bt`: `TDS` Bulk-IN-Übertragungen sind dauerhaft armiert; eine Completion geht über die
  Ereignisschleife (`pump`) nach `rndis::report`, das den Ethernet-Rahmen aus dem 44-Byte-RNDIS-Kopf
  nimmt, in die Warteschlange legt und neu armiert. `recv` läutet die IN-Türklingel, pumpt und nimmt
  aus der Warteschlange. `send` verpackt den Rahmen und schickt ihn synchron über Bulk-OUT.
- Bindung an die Kern-`NetDev`-Schnittstelle (wie virtio-net/ixgbe), also DHCP/ARP/ICMP/UDP laufen
  darüber, sobald der Empfang trägt.

## Nächste Schritte

1. **An echter Hardware prüfen**: ein Android-Handy per USB-C an einen Ultar-Rechner, USB-Tethering an;
   `netz`/`dhcp` in der Kern-Shell. Dort läuft RNDIS mit High Speed – der RX-Pfad sollte tragen.
2. **CDC-ECM/NCM** als Geschwister-Modul für USB-C-zu-Ethernet-Dongles (Norm, viele Dongles): gleiche
   `NetDev`-Bindung, ohne RNDIS-Kopf, mit `SET_INTERFACE` auf die Daten-Alternative; MAC aus dem
   ECM-Funktions-Deskriptor. Für Vendor-Chips (AX88179, RTL8153) je ein eigener Treiber.
3. Abzug aus `NET` beim Abziehen (heute bleibt die Karte in der Liste; `rndis::remove` gibt nur die
   Empfangsfläche frei).

## Prüfen (QEMU, Teilpfad)

```bash
tools/kern-test/usb-net-qemu.sh   # bind + RNDIS + TX; RX in QEMU (Full Speed) noch offen
# Mitschnitt des netdev: -object filter-dump,id=f,netdev=un,file=un.pcap → DISCOVER (udp 68->67) sichtbar
```
