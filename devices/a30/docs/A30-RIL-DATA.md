# A30 RIL-3 – Mobile Daten (PDP → rmnet)

Datenschicht des Mobilfunk-Stacks: aktiviert einen PDP-Kontext über das Samsung-IPC-Protokoll
(FMT-Kanal 235) und bringt die zugehörige rmnet-Schnittstelle hoch. Setzt auf RIL-1 (Transport,
`ModemLink`) und RIL-2 (SIM/Netz, muss registriert sein) auf.

## Kanal-/rmnet-Zuordnung (aus /home/claude/a30k, modem-s327ap-sipc-pdata.dtsi)
- `umts_ipc0` = **Kanal 235** (FMT, Steuerbefehle)
- `rmnet0` = **Kanal 10** (SIPC_CH_ID_PDP_0), `rmnet1`=11, … `rmnet7`=**17**
- Konvention: PDP-`cid` N ⇒ `rmnet(N-1)` ⇒ Kanal `10+(N-1)`. (`channel_for_cid`, `ifname_for_cid`)
- umts_rfs0=245, umts_router=25, umts_dm0=28 (nicht für Daten)

## IPC-GPRS-Befehle (Quelle: libsamsung-ipc, Gruppe 0x0D)
| Befehl | ID | genutzt |
|---|---|---|
| IPC_GPRS_DEFINE_PDP_CONTEXT | 0x0D01 | APN setzen (SET) |
| IPC_GPRS_PDP_CONTEXT | 0x0D04 | aktivieren/deaktivieren (SET) |
| IPC_GPRS_IP_CONFIGURATION | 0x0D09 | IP/DNS/GW/Maske (Antwort) |
| IPC_GPRS_CALL_STATUS | 0x0D10 | Trenn-/Fehlerstatus (EVENT) |

FMT-Message-Header (7 Byte, little-endian): `len(2, inkl. Header) | mseq | aseq | group | index | type`.
Typen: EXEC 0x01, GET 0x02, SET 0x03, CFRM 0x04 (Antwort), EVENT 0x05 (unaufgefordert).

### Struct-Layouts (__packed, aus libsamsung-ipc)
- `ipc_gprs_define_pdp_context_data` (127 B): enable(1), cid(1), magic=1(1), apn[124]
- `ipc_gprs_pdp_context_request_set_data` (103 B): enable(1), cid(1), magic1[4], username[32], password[32], unknown[32], magic2(1)
- `ipc_gprs_ip_configuration_data` (27 B): cid(1), field_flag(1), fail_cause(1), ip[4], dns1[4], dns2[4], gateway[4], subnet_mask[4], unknown[4]

## Ablauf (activate_data)
1. DEFINE_PDP_CONTEXT(SET) mit APN → auf CFRM warten.
2. PDP_CONTEXT(SET, enable=1, user/pass) senden.
3. Auf IP_CONFIGURATION warten (kommt als eigener CFRM/EVENT oder im PDP_CONTEXT-CFRM; fail_cause=0 = ok).
4. `DataSession{ ifname, ip, mask, gw, dns }`; bei `configure=true` rmnet per ioctl setzen
   (SIOCSIFADDR/NETMASK/FLAGS + Default-Route, resolv.conf) — gleicher Stil wie ultar-net.

## Geräte-Testanleitung (für den Koordinator, nach CP ONLINE + Netz registriert)
Voraussetzung: Modem gebootet (ultar-modem --arm → ONLINE), SIM lesbar, im Netz registriert (RIL-2).
1. rmnet-Interfaces da? `ucmd.ps1 "ls /sys/class/net | grep rmnet"` → rmnet0..7.
2. PDP aktivieren (in ultar-ril eingebaut, sobald RIL-1/2 zusammengeführt): APN des Anbieters,
   `activate_data(link, cid=1, apn, None, None, true)`.
3. Erfolg: `ucmd.ps1 "ifconfig rmnet0"` zeigt IP; `ucmd.ps1 "ping -c1 -I rmnet0 8.8.8.8"`.
4. Fehler: fail_cause ≠ 0 → falscher APN/kein Datenabo; CALL_STATUS status=0 → getrennt.

## Offene Punkte / VERIFY
- Ob die IP im eigenen IP_CONFIGURATION-CFRM oder am Ende des PDP_CONTEXT-CFRM kommt, ist firmware-
  abhängig → `wait_ip_config` deckt beide ab; am Gerät den tatsächlichen Frame per Hexdump (RIL-1
  `--probe`) bestätigen und ggf. das Offset anpassen.
- `magic1/magic2`-Felder aus libsamsung-ipc sind geräteabhängig 0; falls Aktivierung abgelehnt wird,
  die Magic-Bytes gegen eine Referenz (tcpdump der Stock-RIL) prüfen.
