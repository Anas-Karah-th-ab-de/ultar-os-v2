# USB-Netzwerk (RNDIS + CDC-ECM) – `drivers/gen_xhci/{rndis,cdc_ecm,net_common}.rs`

Stand 28.11.2026.

Ziel: ein Ultar-Gerät per USB-C ans Netz bringen – Handy-USB-Tethering (RNDIS) und USB-C-zu-Ethernet-
Dongles (CDC-ECM) – und darüber per SSH mit dem Gerät arbeiten (`docs/integration/ssh.md`).

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
  Konfiguration (0x02/0x06), sonst die erste – wie Linux, wenn ein Gerät (QEMU `usb-net`) beide anbietet.
- Abziehen: `rndis::remove`/`cdc_ecm::remove` nehmen die Netzkarte sauber aus `NET` (ein Platzhalter
  `NetGone` hält den Listenplatz, `driver::remove_net`) und geben die DMA-Seiten frei. Die abgezogene
  Karte gibt ihre Adresse ab (`dhcp::tick`), damit die Standardroute auf eine vorhandene Karte wechselt.
  Ein erneutes Anstecken bindet eine neue Karte (`usb1`, `usb2`, …).

## Empfang: die echte Ursache (behoben)

Früher hieß es, QEMUs Full-Speed-`usb-net` liefere keinen Bulk-IN. Falsch: **unser** RNDIS-`SET`
(Paketfilter) war 28 statt 32 Byte lang und hatte den Puffer am falschen Ort. Nach RNDIS zählt
`InformationBufferOffset` ab der RequestID (+8): Kopf 28 Byte (mit `DeviceVcHandle`), Filter bei Byte 28.
QEMU wies den Befehl ab, `rndis_state` blieb `INITIALIZED`, und jeder Rahmen vom Wirt wurde verworfen
(„Slirp: Failed to send packet"). Mit dem korrekten Befehl trägt der Empfang.

## Stand

| Teil | Stand |
|---|---|
| RNDIS Binden, Steuerung, Senden, **Empfang** | ✅ QEMU: `usb0: RNDIS`, DHCP-Lease 10.0.2.15, DNS-Antwort, TCP zum Wirt, statische Adresse, erneutes DHCP – `NETTEST: ALLES OK` |
| Abziehen / Anstecken | ✅ QEMU: `device_del` → „usb0: entfernt", `device_add` → `usb1: RNDIS`, Lease + **Standardroute auf usb1**, zweites `NETTEST: ALLES OK` |
| CDC-ECM | 🆕 Treiber da (Bind, Daten-Alternative, MAC-Zeichenkette, Interrupt-Meldungen); in QEMU nicht prüfbar, weil QEMUs `usb-net` zuerst die RNDIS-Konfiguration anbietet → braucht einen echten ECM-Dongle |
| Abzieh-Erkennung im Leerlauf | ⚠️ Die Kern-Konsole bemerkt Ein-/Abstecken erst beim nächsten Netz-Abfragen (z. B. einem Befehl); mit laufenden Diensten wird regelmäßig abgefragt |

## Prüfen (QEMU)

```bash
IMG=out/ultar-kern-esp.img tools/kern-test/usb-net-e2e.sh
#   RNDIS: bind, MAC, DHCP, DNS, ping 10.0.2.2, TCP zum Host (nettest), device_del/device_add, zweiter nettest
#   CDC-ECM: in QEMU übersprungen → Schlusszeile "USBNET: TEILWEISE cdc-ecm (…)"
tools/kern-test/usb-net-qemu.sh            # interaktiv: dhcp/netz in der Kern-Shell
```

Schlusszeile: `USBNET: ALLES OK` nur wenn alles lief; sonst `USBNET: TEILWEISE <liste>` (nie ein
gefälschtes OK). `ci.sh` führt `usb-net-e2e.sh` neben `netz-e2e.sh` aus.
`qtest.py` kennt dafür `PREARGS` (Geräte, die vor der Netzkarte stehen müssen – hier der USB-Controller).

## Checkliste echte Hardware

1. **Android-USB-Tethering (RNDIS)**: Handy per USB-C an einen Ultar-Rechner, USB-Tethering an;
   `netz`/`dhcp` in der Kern-Shell, danach `ssh -p 2222 root@<adresse>` vom PC.
2. **ECM-Dongles**: USB-C-zu-Ethernet-Dongles mit CDC-ECM. Prüfen: `usb0` erscheint als `CDC-ECM`,
   MAC stimmt mit dem Aufkleber, `dhcp` holt eine Lease.
3. **Eigene Treiber (nächster Schritt)**: ASIX AX88179 und Realtek RTL8153 sprechen ohne Umschaltung
   kein CDC-ECM/NCM und brauchen je einen eigenen Treiber.
4. **CDC-NCM**: noch nicht unterstützt.
