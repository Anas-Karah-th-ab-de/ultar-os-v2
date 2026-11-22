# A30 RIL – Subpaket 1: IPC-Transport (SIPC5)

Unterste Schicht der Mobilfunk-Software. Verbindet den AP mit dem Shannon-Modem (SS310AP, Treiber
`modem_v1`, CONFIG_UMTS_MODEM_SS310AP) über die Zeichengeräte, NACHDEM der CP gebootet und ONLINE ist
(siehe crates/ultar-modem). Quelle verifiziert in `/home/claude/a30k/drivers/misc/modem_v1`.

## Wichtigste Erkenntnis
**Der Kernel rahmt/entrahmt SIPC5 selbst** – je Knoten anhand seiner `iod,id`. Der Userspace
schreibt/liest nur die **rohen IPC-Nutzdaten**:
- `write(/dev/umts_ipc0, payload)` → Kernel (`misc_write` → `sipc5_build_config`/`sipc5_build_header`)
  setzt den SIPC5-Header mit Kanal 235 davor.
- `read(/dev/umts_ipc0)` → Kernel liefert EIN entrahmtes IPC-Paket (Header bereits entfernt).

Man muss SIPC5 also für den normalen Betrieb NICHT selbst rahmen. Das `sipc5`-Modul ist für
Verständnis, Mitschnitt-Interpretation und Tests.

## SIPC5-Headeraufbau (include/sipc5.h)
```
Byte0 cfg : obere 5 Bit Startmarke 0b11111000 (0xF8); untere 3 Bit Flags:
            0x04 PADDING_EXIST, 0x02 EXT_FIELD_EXIST, 0x01 CTL_FIELD_EXIST
Byte1 ch  : Kanal-ID
Byte2..3  : len (u16 LE) = Header+Payload OHNE Padding
optional  : +1 Byte CTL (Multiframe, cfg&0b11==0b11) ODER +2 Byte ext_len (EXT ohne Multi)
```
Min-Header 4 Byte. `ld->aligned` → Padding auf 8-Byte-Grenze (arm64). Einfacher FMT-Frame:
`cfg=0xF8` (ohne Flags) bzw. `0xFC` (mit Padding), `ch`, `len=4+payload`.

## Kanaltabelle (modem_v1.h + modem-s327ap-sipc-pdata.dtsi)
| Kanal | ID | Knoten | Zweck |
|---|---|---|---|
| FMT_0 | 235 | /dev/umts_ipc0 | **RIL: SIM/Netz** (Hauptkanal, app="RIL", Puffer 4096) |
| FMT_1 | 236 | /dev/umts_ipc1 | zweiter RIL-Kanal |
| RFS_0 | 245 | /dev/umts_rfs0 | Remote File System |
| ROUTER | 25 | /dev/umts_router | Router |
| PDP_0..14 | 10..24 | rmnet0..7 | **Datenpfad** (Netzgeräte, kein Zeichenknoten) |
| BOOT_0 | 215 | umts_boot0 | CP-Boot (Subpaket Modem) |

RIL-2 (SIM/Netz) nutzt FMT_0. RIL-3 (Daten) aktiviert einen PDP-Kontext über FMT und bringt dann rmnet hoch.

## Schnittstelle (für RIL-2/RIL-3)
```rust
pub struct SipcFrame { pub channel: u8, pub payload: Vec<u8> }
pub trait ModemLink {
    fn send(&mut self, channel: u8, payload: &[u8]) -> std::io::Result<()>;
    fn recv(&mut self, timeout_ms: u32) -> std::io::Result<Option<SipcFrame>>;
}
```
Implementierungen: `UmtsLink` (echte Knoten, nichtblockierend via poll) und `MockLink` (Tests).
`UmtsLink::open()` öffnet FMT_0; `ensure(ch)` öffnet weitere Kanäle; `send` schreibt rohe Nutzdaten.

## Gerätetest (nach CP = ONLINE)
1. `upush.ps1 ultar-ril /data/ultar-ril`
2. `ucmd.ps1 "ls -la /dev/umts_ipc0; /data/ultar-ril --nodes"`
3. Lauschen: `ucmd.ps1 "/data/ultar-ril --probe 5"` → der CP sendet nach dem Boot unaufgefordert
   Status-/Indication-Frames auf FMT_0 → Hexdump. (Blockiert nie: poll + Timeout.)
4. Senden (sobald RIL-2 ein IPC-Kommando baut): `ultar-ril --send <hex>` schickt rohe Nutzdaten auf FMT_0
   und zeigt die Antwort.

Erwartung: nach `modem_status`→ONLINE liefert `--probe` Frames auf ch235 (IPC-FMT). Kommen keine,
ist der CP noch nicht ONLINE oder umts_ipc0 nicht bereit (dmesg prüfen).

## Grenze dieses Subpakets
Dies ist NUR der Transport. Der Inhalt der FMT-Nutzdaten (Samsung-IPC: main/sub command, SIM-Status,
Netzregistrierung) ist Subpaket **RIL-2**. Der Datenpfad (PDP→rmnet) ist Subpaket **RIL-3**.
