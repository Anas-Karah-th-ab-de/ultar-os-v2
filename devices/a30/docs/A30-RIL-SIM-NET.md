# A30 RIL – SIM- und Netz-Befehlsschicht (Samsung IPC)

Befehlsschicht über dem FMT-Kanal (235) des Shannon-Modems. Transport = Subpaket RIL-1
(SIPC5 über /dev/umts_ipc0). Protokoll = Samsung IPC v4.x (wie Replicant libsamsung-ipc).

## FMT-Nachrichtenformat (im Payload des FMT-Kanals)
7-Byte-Kopf, little-endian, dann Rumpf:

| Offset | Feld   | Größe | Bedeutung |
|--------|--------|-------|-----------|
| 0      | length | 2     | Gesamtlänge = 7 + Rumpf |
| 2      | mseq   | 1     | Sequenz der Anfrage |
| 3      | aseq   | 1     | Ack-Sequenz (in Antworten = mseq der Anfrage) |
| 4      | group  | 1     | Befehlsgruppe (High-Byte) |
| 5      | index  | 1     | Befehlsindex (Low-Byte) |
| 6      | type   | 1     | IPC_TYPE_* (EXEC 1, GET 2, SET 3, CFRM 4, EVENT 5) |
| 7..    | data   | n     | befehlsspezifisch |

`command = (group << 8) | index`. Implementiert in `crates/ultar-ril/src/ipc.rs`
(`encode`/`decode`, `Ril::request/get/exec` mit Sequenz- und Antwortabgleich).

## Verwendete Kommandos
Bestätigt aus libsamsung-ipc-Headern:
- SEC (SIM), Gruppe 0x05: IPC_SEC_PIN_STATUS 0x0501 (SIM-/Kartenstatus),
  IPC_SEC_RSIM_ACCESS 0x0505, IPC_SEC_SIM_ICC_TYPE 0x0507.
  Statuswerte: 0x00 READY, 0x01 SIM-Sperre, 0x80 CARD_NOT_PRESENT, 0x81 CARD_ERROR,
  0x82 INIT_COMPLETE, 0x83 PB_INIT_COMPLETE. Kartentyp: 0x01 SIM, 0x02 USIM.
- MISC, Gruppe 0x0A: IPC_MISC_ME_IMSI 0x0A02 (Längen-Byte + ASCII), IPC_MISC_ME_SN 0x0A03
  (Seriennr./IMEI, Subtyp SERIAL 0x01), IPC_MISC_ME_VERSION 0x0A01.

VERIFY (Header waren beim Bau nicht abrufbar – gegen libsamsung-ipc net.h/disp.h/pwr.h prüfen):
- NET, Gruppe 0x08: REGIST 0x0806, SERVING_NETWORK 0x0808, PLMN_LIST 0x0807, PLMN_SEL 0x0802,
  MODE_SEL 0x0805, POWER_ON_ATTACH 0x0804.
- DISP, Gruppe 0x07: RSSI_INFO 0x0702, ICON_INFO 0x0701.
- PWR, Gruppe 0x01: PHONE_PWR_UP 0x0101.

Quelle: Replicant libsamsung-ipc, https://git.replicant.us/replicant-next/hardware_replicant_libsamsung-ipc/tree/include/

## API (crates/ultar-ril)
- `sim::sim_status(ril,t) -> SimStatus{present,state,raw}`
- `sim::imsi(ril,t) -> Option<String>`, `sim::card_type`, `sim::imei`
- `net::power_on(ril,t)` (Normalbetrieb + Attach)
- `net::network_state(ril,t) -> NetState{reg,rat,plmn,signal_dbm,bars}`
- `net::scan_networks(ril,t) -> Vec<Plmn>` und `net::select_network(ril, manual, t)` (manuelle Netz-/Funkturmwahl)

## Geräte-Testanleitung (nach CP-ONLINE)
1. CP booten: `/data/ultar-modem --fw /data/cp.bin --arm` bis STATUS=ONLINE.
2. Rohframes ansehen (RIL-1): `/data/ultar-ril --probe` → Hexdump der FMT-Frames vom CP.
3. Mit den echten Antworten die VERIFY-Felder (NET-IDs, Rumpf-Offsets, RSSI-Skala) feinjustieren.
4. In ultar-shell einbinden (siehe RIL_SIMNET_INTEGRATION.md): SIM-Status/IMSI/Netz in den
   Telefon-/Netz-Einstellungen anzeigen; manuelle Netzwahl per scan+select.

OFFEN (Folgepakete): Anruf (CALL 0x02) und SMS (SMS 0x04), Daten/PDP (Subpaket RIL-3, rmnet).
