//! Samsung-IPC-Befehlsschicht (IPC v4.x) über den FMT-Kanal des Shannon-Modems.
//!
//! Aufbau: Die Transportschicht (`link.rs`, Subpaket RIL-1) liefert/sendet rohe SIPC5-Rahmen
//! über /dev/umts_ipc0. Dieser Modul verpackt/entpackt die Samsung-IPC-FMT-Nachricht, die im
//! Payload des FMT-Kanals (235) steckt.
//!
//! FMT-Nachrichtenkopf (ipc_fmt_header, 7 Byte, little-endian):
//!   length:u16  (Gesamtlänge = 7 + Rumpf)
//!   mseq:u8     (Sequenz der Anfrage)
//!   aseq:u8     (Ack-Sequenz; in Antworten = mseq der Anfrage)
//!   group:u8    (Befehlsgruppe = High-Byte des Kommandos)
//!   index:u8    (Befehlsindex = Low-Byte)
//!   type:u8     (IPC_TYPE_*)
//! command = (group << 8) | index.
//!
//! Quellen der Konstanten: Replicant libsamsung-ipc (öffentlich). SEC/MISC sind aus den
//! Headern bestätigt; NET/DISP/PWR mit // VERIFY markiert (net.h/disp.h waren zeitweise nicht
//! abrufbar – Werte gegen die Quelle/Gerät bestätigen).

// SipcFrame + ModemLink kommen kanonisch aus link.rs (zusammengeführt).
pub use crate::link::{ModemLink, SipcFrame};

/// FMT-Kanal (bestätigt: 235).
pub const FMT_CHANNEL: u8 = 235;

// ---- Nachrichtentypen (IPC_TYPE_*) ----
pub const IPC_TYPE_EXEC: u8 = 0x01;
pub const IPC_TYPE_GET: u8 = 0x02;
pub const IPC_TYPE_SET: u8 = 0x03;
pub const IPC_TYPE_CFRM: u8 = 0x04; // Antwort/Bestätigung
pub const IPC_TYPE_EVENT: u8 = 0x05; // unaufgeforderte Meldung

// ---- Gruppen (High-Byte des Kommandos) ----
pub const G_PWR: u8 = 0x01;
pub const G_CALL: u8 = 0x02;
pub const G_SMS: u8 = 0x04;
pub const G_SEC: u8 = 0x05; // bestätigt
pub const G_DISP: u8 = 0x07;
pub const G_NET: u8 = 0x08;
pub const G_SND: u8 = 0x09;
pub const G_MISC: u8 = 0x0A; // bestätigt
pub const G_GPRS: u8 = 0x0D;

// ---- Kommandos command=(group<<8)|index ----
// SEC (SIM) – aus include/sec.h bestätigt:
pub const IPC_SEC_PIN_STATUS: u16 = 0x0501; // SIM-/PIN-Status (Status-/Kartenzustand)
pub const IPC_SEC_PHONE_LOCK: u16 = 0x0502;
pub const IPC_SEC_RSIM_ACCESS: u16 = 0x0505; // Zugriff auf SIM-Dateien (ICCID, IMSI über EF)
pub const IPC_SEC_GSIM_ACCESS: u16 = 0x0506;
pub const IPC_SEC_SIM_ICC_TYPE: u16 = 0x0507; // Kartentyp (SIM/USIM)
pub const IPC_SEC_LOCK_INFOMATION: u16 = 0x0508;
// MISC – aus include/misc.h bestätigt:
pub const IPC_MISC_ME_VERSION: u16 = 0x0A01;
pub const IPC_MISC_ME_IMSI: u16 = 0x0A02;
pub const IPC_MISC_ME_SN: u16 = 0x0A03; // Seriennr./IMEI (Subtyp SERIAL=0x01)
pub const IPC_MISC_TIME_INFO: u16 = 0x0A05;
// PWR – VERIFY (pwr.h nicht abrufbar; PWR_UP ist etabliert):
pub const IPC_PWR_PHONE_PWR_UP: u16 = 0x0101; // VERIFY
pub const IPC_PWR_PHONE_STATE: u16 = 0x0107; // VERIFY
pub const IPC_PWR_PHONE_RESET: u16 = 0x0103; // VERIFY
// NET – VERIFY (net.h war nicht abrufbar; bekannte libsamsung-ipc-Werte):
pub const IPC_NET_PREF_PLMN: u16 = 0x0801; // VERIFY
pub const IPC_NET_PLMN_SEL: u16 = 0x0802; // VERIFY – manuelle/auto Netzwahl
pub const IPC_NET_SERVICE_DOMAIN_CONFIG: u16 = 0x0808; // VERIFY
pub const IPC_NET_POWER_ON_ATTACH: u16 = 0x0809; // VERIFY
pub const IPC_NET_MODE_SEL: u16 = 0x080A; // VERIFY – RAT-Wahl (2G/3G/4G/auto)
pub const IPC_NET_REGIST: u16 = 0x0805; // VERIFY – Registrierungsstatus
pub const IPC_NET_PLMN_LIST: u16 = 0x0804; // VERIFY – verfügbare Netze (Scan)
pub const IPC_NET_SERVING_NETWORK: u16 = 0x0803; // VERIFY – aktueller Operator/PLMN
pub const IPC_NET_BAND_SEL: u16 = 0x0807; // VERIFY
// DISP – VERIFY (disp.h nicht abrufbar):
pub const IPC_DISP_ICON_INFO: u16 = 0x0701; // VERIFY – u.a. Signalbalken
pub const IPC_DISP_RSSI_INFO: u16 = 0x0706; // VERIFY – RSSI

/// Entschlüsselte FMT-Nachricht.
#[derive(Clone, Debug, PartialEq)]
pub struct IpcMessage {
    pub command: u16,
    pub msg_type: u8,
    pub mseq: u8,
    pub aseq: u8,
    pub data: Vec<u8>,
}

/// FMT-Nachricht kodieren (7-Byte-Kopf + Rumpf), fertig für `ModemLink::send(FMT_CHANNEL, …)`.
pub fn encode(command: u16, msg_type: u8, mseq: u8, data: &[u8]) -> Vec<u8> {
    let len = 7 + data.len();
    let mut v = Vec::with_capacity(len);
    v.extend_from_slice(&(len as u16).to_le_bytes());
    v.push(mseq);
    v.push(0); // aseq=0 bei Anfragen
    v.push((command >> 8) as u8); // group
    v.push((command & 0xff) as u8); // index
    v.push(msg_type);
    v.extend_from_slice(data);
    v
}

/// FMT-Nachricht aus dem FMT-Kanal-Payload entschlüsseln.
pub fn decode(payload: &[u8]) -> Option<IpcMessage> {
    if payload.len() < 7 {
        return None;
    }
    let length = u16::from_le_bytes([payload[0], payload[1]]) as usize;
    let mseq = payload[2];
    let aseq = payload[3];
    let group = payload[4];
    let index = payload[5];
    let msg_type = payload[6];
    let end = length.clamp(7, payload.len());
    let data = payload.get(7..end).unwrap_or(&[]).to_vec();
    Some(IpcMessage { command: ((group as u16) << 8) | index as u16, msg_type, mseq, aseq, data })
}

/// Zustandsbehafteter RIL-Client über eine Transportverbindung.
pub struct Ril<L: ModemLink> {
    link: L,
    seq: u8,
}

impl<L: ModemLink> Ril<L> {
    pub fn new(link: L) -> Self {
        Self { link, seq: 1 }
    }

    pub fn link_mut(&mut self) -> &mut L {
        &mut self.link
    }

    fn next_seq(&mut self) -> u8 {
        let s = self.seq;
        self.seq = self.seq.wrapping_add(1);
        if self.seq == 0 {
            self.seq = 1;
        }
        s
    }

    /// Anfrage senden und auf die passende Antwort (gleiches Kommando) warten.
    /// Unaufgeforderte Meldungen (EVENT) werden dabei übersprungen.
    pub fn request(&mut self, command: u16, msg_type: u8, data: &[u8], timeout_ms: u32) -> std::io::Result<Option<IpcMessage>> {
        let seq = self.next_seq();
        let payload = encode(command, msg_type, seq, data);
        self.link.send(FMT_CHANNEL, &payload)?;
        let start = std::time::Instant::now();
        loop {
            let elapsed = start.elapsed().as_millis() as u32;
            if elapsed >= timeout_ms {
                return Ok(None);
            }
            let left = timeout_ms - elapsed;
            match self.link.recv(left)? {
                Some(f) if f.channel == FMT_CHANNEL => {
                    if let Some(m) = decode(&f.payload) {
                        // Antwort auf unsere Anfrage: gleiches Kommando (aseq==seq, wo gesetzt)
                        if m.command == command && (m.aseq == seq || m.msg_type == IPC_TYPE_CFRM) {
                            return Ok(Some(m));
                        }
                        // sonst: Event/andere Antwort -> weiter warten
                    }
                }
                Some(_) => {} // anderer Kanal -> ignorieren
                None => {}    // Timeout eines einzelnen recv -> Schleife prüft Gesamtzeit
            }
        }
    }

    /// GET-Anfrage (lesen).
    pub fn get(&mut self, command: u16, timeout_ms: u32) -> std::io::Result<Option<IpcMessage>> {
        self.request(command, IPC_TYPE_GET, &[], timeout_ms)
    }

    /// EXEC/SET-Anfrage mit Rumpf.
    pub fn exec(&mut self, command: u16, data: &[u8], timeout_ms: u32) -> std::io::Result<Option<IpcMessage>> {
        self.request(command, IPC_TYPE_EXEC, data, timeout_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let body = [0xDE, 0xAD, 0xBE, 0xEF];
        let bytes = encode(IPC_MISC_ME_IMSI, IPC_TYPE_GET, 7, &body);
        // Kopf prüfen
        assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]) as usize, 7 + body.len());
        assert_eq!(bytes[2], 7); // mseq
        assert_eq!(bytes[4], G_MISC); // group
        assert_eq!(bytes[5], 0x02); // index
        assert_eq!(bytes[6], IPC_TYPE_GET);
        let m = decode(&bytes).unwrap();
        assert_eq!(m.command, IPC_MISC_ME_IMSI);
        assert_eq!(m.mseq, 7);
        assert_eq!(m.data, body);
    }

    #[test]
    fn decode_rejects_short() {
        assert!(decode(&[0, 1, 2]).is_none());
    }

    #[test]
    fn command_split() {
        assert_eq!(((G_SEC as u16) << 8) | 0x01, IPC_SEC_PIN_STATUS);
        assert_eq!(((G_MISC as u16) << 8) | 0x02, IPC_MISC_ME_IMSI);
    }
}
