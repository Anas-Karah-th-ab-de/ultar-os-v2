//! Ereignisgesteuerter Kern des Funkdienstes. Das Modem meldet seinen Zustand von selbst (NOTI); der Dienst
//! führt mit, antwortet auf RFS und stößt nur an, was nötig ist (Normalbetrieb, Abfragen).
//!
//! Mitschnitt A30 nach dem CP-Start (Kopf: länge:u16 mseq aseq gruppe index typ):
//!   13 05 NOTI "AST_POWERON" · 01 01 NOTI PWR_UP · 05 07 NOTI ICC_TYPE · 05 01 NOTI PIN_STATUS {status, sperre}
//!   01 07 NOTI PHONE_STATE {1 = Flugmodus/LPM, 2 = normal} · 08 27 / 0d 0f / 26 01 (noch nicht gedeutet)
//! Antworttypen: INDI 1, RESP 2, NOTI 3. Anfragetypen: EXEC 1, GET 2, SET 3.

use crate::ipc::*;
#[allow(unused_imports)]
use crate::link::CH_FMT_1;
use crate::link::{ModemLink, CH_FMT_0, CH_RFS_0};
use crate::rfs::Rfs;

pub const T_INDI: u8 = 1;
pub const T_RESP: u8 = 2;
pub const T_NOTI: u8 = 3;

pub const CMD_PWR_PHONE_PWR_UP: u16 = 0x0101;
pub const CMD_PWR_PHONE_STATE: u16 = 0x0107;
pub const CMD_SEC_PIN_STATUS: u16 = 0x0501;
pub const CMD_SEC_SIM_ICC_TYPE: u16 = 0x0507;
pub const CMD_DISP_ICON_INFO: u16 = 0x0701;
pub const CMD_DISP_RSSI_INFO: u16 = 0x0706;
pub const CMD_NET_PLMN_SEL: u16 = 0x0802;
pub const CMD_NET_SERVING_NETWORK: u16 = 0x0803;
pub const CMD_NET_REGIST: u16 = 0x0805;
pub const CMD_MISC_ME_SN: u16 = 0x0A03;
pub const CMD_GEN_PHONE_RES: u16 = 0x8001;
// Mobile Daten (Gruppe GPRS 0x0D, libsamsung-ipc gprs.h)
pub const CMD_GPRS_DEFINE_PDP_CONTEXT: u16 = 0x0D01;
pub const CMD_GPRS_PDP_CONTEXT: u16 = 0x0D04;
pub const CMD_GPRS_IP_CONFIGURATION: u16 = 0x0D09;
pub const CMD_GPRS_CALL_STATUS: u16 = 0x0D10;
/// Datenkontext für das Internet (cid 1 → rmnet0)
pub const DATA_CID: u8 = 1;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reg {
    pub act: u8,
    pub domain: u8,
    /// 1 keine, 2 Heimnetz, 3 Suche, 4 nur Notruf, 5 unbekannt, 6 Roaming
    pub status: u8,
    pub lac: u16,
    pub cid: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub powered: bool,
    /// 0 unbekannt, 1 LPM (Funk aus), 2 normal
    pub phone_state: u8,
    /// roher SIM-Status (0x80 keine Karte, 0x81 Kartenfehler, 0x82/0x83 bereit, 0x00.. Sperren), None = noch nichts gemeldet
    pub sim: Option<u8>,
    pub sim_lock: u8,
    pub icc_type: u8,
    pub reg_cs: Reg,
    pub reg_ps: Reg,
    pub plmn: String,
    pub rssi: Option<u8>,
    /// LTE-Messung aus DISP_RSSI_INFO (0x0706, am A30 18 Byte): RSRP dBm, RSRQ dB, SINR in 0,1 dB (// VERIFY SINR)
    pub lte: Option<(i32, i32, i32)>,
    pub imei: String,
    /// Anzahl empfangener FMT-Nachrichten
    pub rx: u64,
    /// Mobile Daten: letzte IP-Konfiguration des Modems (cid, IP, DNS, Gateway)
    pub data_ip: Option<crate::pdp::IpConfig>,
    /// Mobile Daten: Zustand laut GPRS_CALL_STATUS (0 aus, 1 an, 3 getrennt) und Fehlergrund
    pub data_status: Option<u8>,
    pub data_fail: u16,
    /// Laufende Anrufe laut CALL_LIST (leer = kein Anruf)
    pub calls: Vec<crate::tel::CallEntry>,
}

pub struct Engine<L: ModemLink> {
    pub link: L,
    pub rfs: Rfs,
    pub st: State,
    seq: u8,
    /// FMT-Kanal dieses Steckplatzes: 235 = SIM 1 (/dev/umts_ipc0), 236 = SIM 2 (/dev/umts_ipc1) – wie rild / rild -c 2
    pub fmt: u8,
    /// Mitschrift (für /run/ultar/radio/ipc.log); der Dienst leert sie regelmäßig
    pub log: Vec<String>,
    /// Empfangene SMS (der Dienst holt sie ab und legt sie im Posteingang ab)
    pub inbox: Vec<crate::tel::InSms>,
    /// USSD-Antworten des Netzes (Zustand, Text)
    pub ussd: Vec<(u8, String)>,
    /// läuft eine USSD-Sitzung (Netz erwartet Antwort)?
    pub ussd_open: bool,
}

fn hex(d: &[u8]) -> String {
    d.iter()
        .take(64)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
        + if d.len() > 64 { " …" } else { "" }
}

impl<L: ModemLink> Engine<L> {
    pub fn new(link: L, rfs: Rfs) -> Self {
        Engine {
            link,
            rfs,
            st: State::default(),
            seq: 1,
            fmt: CH_FMT_0,
            log: Vec::new(),
            inbox: Vec::new(),
            ussd: Vec::new(),
            ussd_open: false,
        }
    }

    /// Engine für einen anderen FMT-Kanal (SIM 2: CH_FMT_1).
    pub fn with_fmt(link: L, rfs: Rfs, fmt: u8) -> Self {
        Engine {
            fmt,
            ..Engine::new(link, rfs)
        }
    }

    pub fn send(&mut self, cmd: u16, ty: u8, data: &[u8]) -> std::io::Result<()> {
        let seq = self.seq;
        self.seq = if self.seq == 0xfe { 1 } else { self.seq + 1 };
        self.log
            .push(format!("TX {cmd:04x} typ {ty} seq {seq}: {}", hex(data)));
        self.link.send(self.fmt, &encode(cmd, ty, seq, data))
    }

    /// Funk einschalten (Normalbetrieb) bzw. ausschalten (LPM).
    pub fn set_radio(&mut self, on: bool) -> std::io::Result<()> {
        let v: u16 = if on { 0x0202 } else { 0x0001 };
        self.send(CMD_PWR_PHONE_STATE, IPC_TYPE_EXEC, &v.to_le_bytes())
    }

    /// Mobile Daten einschalten wie der Samsung-RIL: APN definieren (0x0D01 SET), Kontext aktivieren
    /// (0x0D04 SET). Die IP-Konfiguration meldet das Modem danach von selbst (0x0D09, 0x0D10).
    pub fn data_connect(
        &mut self,
        apn: &str,
        user: Option<&str>,
        pass: Option<&str>,
    ) -> std::io::Result<()> {
        self.st.data_ip = None;
        self.send(
            CMD_GPRS_DEFINE_PDP_CONTEXT,
            IPC_TYPE_SET,
            &crate::pdp::build_define_pdp(DATA_CID, apn),
        )?;
        self.send(
            CMD_GPRS_PDP_CONTEXT,
            IPC_TYPE_SET,
            &crate::pdp::build_pdp_context(DATA_CID, true, user, pass),
        )
    }

    /// Mobile Daten ausschalten (Kontext deaktivieren).
    pub fn data_disconnect(&mut self) -> std::io::Result<()> {
        self.st.data_ip = None;
        self.send(
            CMD_GPRS_PDP_CONTEXT,
            IPC_TYPE_SET,
            &crate::pdp::build_pdp_context(DATA_CID, false, None, None),
        )
    }

    /// Anruf wählen (CALL_OUTGOING).
    pub fn dial(&mut self, number: &str) -> std::io::Result<()> {
        self.send(
            crate::tel::CMD_CALL_OUTGOING,
            IPC_TYPE_EXEC,
            &crate::tel::build_outgoing(number),
        )?;
        self.send(crate::tel::CMD_CALL_LIST, IPC_TYPE_GET, &[])
    }
    /// Eingehenden Anruf annehmen.
    pub fn answer(&mut self) -> std::io::Result<()> {
        self.send(crate::tel::CMD_CALL_ANSWER, IPC_TYPE_EXEC, &[])
    }
    /// Halten / fortsetzen: das Modem tauscht aktiven und gehaltenen Anruf (SS_MANAGE_CALL 2).
    pub fn hold_swap(&mut self, index: u8) -> std::io::Result<()> {
        let data = crate::tel::build_manage_call(crate::tel::MANAGE_HOLD_SWAP, index);
        self.send(crate::tel::CMD_SS_MANAGE_CALL, IPC_TYPE_EXEC, &data)?;
        self.send(crate::tel::CMD_CALL_LIST, IPC_TYPE_GET, &[])
    }
    /// Ein Tastenton (Start und Stopp von CALL_CONT_DTMF).
    pub fn dtmf(&mut self, key: u8) -> std::io::Result<()> {
        let bad = || std::io::Error::from(std::io::ErrorKind::InvalidInput);
        let start = crate::tel::build_dtmf(crate::tel::DTMF_START, key).ok_or_else(bad)?;
        let stop = crate::tel::build_dtmf(crate::tel::DTMF_STOP, key).ok_or_else(bad)?;
        self.send(crate::tel::CMD_CALL_CONT_DTMF, IPC_TYPE_EXEC, &start)?;
        self.send(crate::tel::CMD_CALL_CONT_DTMF, IPC_TYPE_EXEC, &stop)
    }
    /// Mikrofon stumm schalten (SND_MIC_MUTE_CTRL).
    pub fn mute(&mut self, on: bool) -> std::io::Result<()> {
        self.send(
            crate::tel::CMD_SND_MIC_MUTE,
            IPC_TYPE_SET,
            &crate::tel::build_mute(on),
        )
    }
    /// Einen Anruf beenden (Samsung-IPC kennt nur „alle“: CALL_RELEASE; der Index wird geprüft).
    pub fn release(&mut self, index: u8) -> std::io::Result<()> {
        if !self.st.calls.iter().any(|c| c.id == index) {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        }
        self.hangup()
    }
    /// Auflegen / ablehnen.
    pub fn hangup(&mut self) -> std::io::Result<()> {
        self.send(crate::tel::CMD_CALL_RELEASE, IPC_TYPE_EXEC, &[])?;
        self.send(crate::tel::CMD_CALL_LIST, IPC_TYPE_GET, &[])
    }
    /// SMS senden; lange Texte gehen als mehrere Einzel-SMS (160 GSM-7- bzw. 70 UCS-2-Zeichen).
    pub fn send_sms(&mut self, to: &str, text: &str, smsc: &[u8]) -> std::io::Result<()> {
        let chars: Vec<char> = text.chars().collect();
        let per = if text
            .chars()
            .all(|c| crate::tel::encode_submit("0", &c.to_string())[3] == 0x00)
        {
            160
        } else {
            70
        };
        for part in chars.chunks(per) {
            let t: String = part.iter().collect();
            let pdu = crate::tel::encode_submit(to, &t);
            self.send(
                crate::tel::CMD_SMS_SEND_MSG,
                IPC_TYPE_EXEC,
                &crate::tel::build_send(smsc, &pdu),
            )?;
        }
        Ok(())
    }

    /// USSD-Code senden (`*111#`) bzw. in laufender Sitzung antworten.
    pub fn send_ussd(&mut self, code: &str) -> std::io::Result<()> {
        let open = self.ussd_open;
        self.send(
            crate::tel::CMD_SS_USSD,
            IPC_TYPE_EXEC,
            &crate::tel::build_ussd(code, open),
        )
    }
    /// USSD-Sitzung beenden.
    pub fn cancel_ussd(&mut self) -> std::io::Result<()> {
        self.ussd_open = false;
        self.send(
            crate::tel::CMD_SS_USSD,
            IPC_TYPE_EXEC,
            &crate::tel::build_ussd_cancel(),
        )
    }

    /// Zustände nachfragen (SIM, Anmeldung leitungs-/paketvermittelt, Netz).
    pub fn query(&mut self) -> std::io::Result<()> {
        self.send(CMD_SEC_PIN_STATUS, IPC_TYPE_GET, &[])?;
        if self.st.phone_state == 2 {
            self.send(CMD_NET_REGIST, IPC_TYPE_GET, &[0xff, 0x02])?;
            self.send(CMD_NET_REGIST, IPC_TYPE_GET, &[0xff, 0x03])?;
            self.send(CMD_NET_SERVING_NETWORK, IPC_TYPE_GET, &[])?;
        }
        Ok(())
    }

    /// Einen Schritt: höchstens `timeout_ms` auf einen Rahmen warten und ihn verarbeiten. true = etwas kam an.
    pub fn step(&mut self, timeout_ms: u32) -> std::io::Result<bool> {
        let Some(f) = self.link.recv(timeout_ms)? else {
            return Ok(false);
        };
        match f.channel {
            CH_RFS_0 => {
                for resp in self.rfs.feed(&f.payload) {
                    self.link.send(CH_RFS_0, &resp)?;
                }
                self.log.append(&mut self.rfs.log);
            }
            c if c == self.fmt => {
                if let Some(m) = decode(&f.payload) {
                    self.on_fmt(&m);
                }
            }
            c => self.log.push(format!("RX Kanal {c}: {}", hex(&f.payload))),
        }
        Ok(true)
    }

    fn on_fmt(&mut self, m: &IpcMessage) {
        self.st.rx += 1;
        self.log.push(format!(
            "RX {:04x} typ {} seq {}/{}: {}",
            m.command,
            m.msg_type,
            m.mseq,
            m.aseq,
            hex(&m.data)
        ));
        let d = &m.data;
        match m.command {
            CMD_PWR_PHONE_PWR_UP => self.st.powered = true,
            CMD_PWR_PHONE_STATE if !d.is_empty() => self.st.phone_state = d[0],
            CMD_SEC_PIN_STATUS if !d.is_empty() => {
                self.st.sim = Some(d[0]);
                self.st.sim_lock = d.get(1).copied().unwrap_or(0);
            }
            CMD_SEC_SIM_ICC_TYPE if !d.is_empty() => self.st.icc_type = d[0],
            CMD_NET_REGIST if d.len() >= 3 => {
                let r = Reg {
                    act: d[0],
                    domain: d[1],
                    status: d[2],
                    lac: d
                        .get(4..6)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .unwrap_or(0),
                    cid: d
                        .get(6..10)
                        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .unwrap_or(0),
                };
                if r.domain == 0x03 {
                    self.st.reg_ps = r
                } else {
                    self.st.reg_cs = r
                }
            }
            CMD_NET_SERVING_NETWORK if d.len() >= 9 => {
                self.st.plmn = d[3..9]
                    .iter()
                    .filter(|b| b.is_ascii_digit())
                    .map(|b| *b as char)
                    .collect();
            }
            CMD_DISP_ICON_INFO if d.len() >= 2 => self.st.rssi = Some(d[1]),
            // Shannon (A30): [0]=[4]=|RSRP|, [5]=|RSRQ|, [6..8]=SINR·10 (u16), [3]=111−|RSRP| (Stufe)
            CMD_DISP_RSSI_INFO if d.len() >= 8 && d[4] > 0 && d[4] < 0x90 => {
                let sinr = i16::from_le_bytes([d[6], d[7]]) as i32;
                self.st.lte = Some((-(d[4] as i32), -(d[5] as i32), sinr));
            }
            CMD_DISP_RSSI_INFO if !d.is_empty() => self.st.rssi = Some(d[0]),
            // Anruf: Meldung → Anrufliste holen (liefert Nummer + Zustand)
            crate::tel::CMD_CALL_INCOMING | crate::tel::CMD_CALL_STATUS => {
                if m.command == crate::tel::CMD_CALL_INCOMING && self.st.calls.is_empty() {
                    // sofort klingeln, die Nummer kommt mit der Anrufliste
                    self.st.calls.push(crate::tel::CallEntry {
                        id: d.get(1).copied().unwrap_or(1),
                        term: 1,
                        status: crate::tel::CallStatus::Incoming,
                        number: String::new(),
                    });
                }
                let _ = self.send(crate::tel::CMD_CALL_LIST, IPC_TYPE_GET, &[]);
            }
            crate::tel::CMD_CALL_LIST => self.st.calls = crate::tel::parse_call_list(d),
            crate::tel::CMD_SS_USSD if m.msg_type == T_NOTI || m.msg_type == T_RESP => {
                if let Some((state, text)) = crate::tel::parse_ussd(d) {
                    self.ussd_open = state == 2;
                    self.ussd.push((state, text));
                }
            }
            crate::tel::CMD_SMS_INCOMING_MSG => {
                if let Some((tpid, pdu)) = crate::tel::split_incoming(d) {
                    match crate::tel::decode_deliver(pdu) {
                        Some(sms) => {
                            self.log.push(format!(
                                "SMS von {}: {} Zeichen",
                                sms.from,
                                sms.text.chars().count()
                            ));
                            self.inbox.push(sms);
                        }
                        None => self
                            .log
                            .push("SMS: PDU nicht lesbar (Statusbericht?)".into()),
                    }
                    // Empfang bestätigen, sonst wiederholt das Netz die SMS
                    let _ = self.send(
                        crate::tel::CMD_SMS_DELIVER_REPORT,
                        IPC_TYPE_EXEC,
                        &crate::tel::build_deliver_report(tpid),
                    );
                }
            }
            CMD_GPRS_IP_CONFIGURATION => {
                if let Some(c) = crate::pdp::parse_ip_config(d) {
                    self.log.push(format!(
                        "Daten: cid {} IP {} GW {} DNS {} {} (Fehler {})",
                        c.cid, c.ip, c.gateway, c.dns1, c.dns2, c.fail_cause
                    ));
                    self.st.data_ip = Some(c);
                }
            }
            // ipc_gprs_call_status_data: cid, status, fail_cause:u16 (// VERIFY am Gerät)
            CMD_GPRS_CALL_STATUS if d.len() >= 2 => {
                self.st.data_status = Some(d[1]);
                self.st.data_fail = d
                    .get(2..4)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .unwrap_or(0);
                if d[1] != 1 {
                    self.st.data_ip = None;
                }
            }
            CMD_MISC_ME_SN if d.len() >= 3 => {
                let n = d[1] as usize;
                self.st.imei = d
                    .get(2..2 + n)
                    .unwrap_or(&[])
                    .iter()
                    .filter(|b| b.is_ascii_alphanumeric())
                    .map(|b| *b as char)
                    .collect();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::SipcFrame;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Fake {
        rx: VecDeque<SipcFrame>,
        tx: Vec<(u8, Vec<u8>)>,
    }
    impl ModemLink for Fake {
        fn send(&mut self, channel: u8, payload: &[u8]) -> std::io::Result<()> {
            self.tx.push((channel, payload.to_vec()));
            Ok(())
        }
        fn recv(&mut self, _t: u32) -> std::io::Result<Option<SipcFrame>> {
            Ok(self.rx.pop_front())
        }
    }

    fn eng(frames: &[(u8, &[u8])]) -> Engine<Fake> {
        let mut f = Fake::default();
        for (c, p) in frames {
            f.rx.push_back(SipcFrame {
                channel: *c,
                payload: p.to_vec(),
            });
        }
        let d =
            std::env::temp_dir().join(format!("ultar-eng-{}-{}", std::process::id(), frames.len()));
        Engine::new(f, Rfs::new(d))
    }

    /// Die echten Rahmen vom A30 (19.11.2026): PWR_UP, Kartentyp, „keine Karte", Telefonzustand 2.
    #[test]
    fn captured_boot_sequence() {
        let mut e = eng(&[
            (235, &[0x08, 0, 1, 0, 0x01, 0x01, 3, 0]),
            (235, &[0x08, 0, 4, 0, 0x05, 0x07, 3, 0]),
            (235, &[0x09, 0, 5, 0, 0x05, 0x01, 3, 0x80, 0]),
            (235, &[0x08, 0, 6, 0, 0x01, 0x07, 3, 2]),
        ]);
        while e.step(0).unwrap() {}
        assert!(e.st.powered);
        assert_eq!(e.st.sim, Some(0x80));
        assert_eq!(e.st.phone_state, 2);
        assert_eq!(e.st.rx, 4);
    }

    #[test]
    fn rfs_request_gets_answer_on_rfs_channel() {
        // Öffnen einer nicht vorhandenen Datei, wie vom CP gesendet
        let name = b"CP_AUDIO_SLSI_OMC_AUDIODATA.bin\0";
        let mut body = 0u32.to_le_bytes().to_vec();
        body.extend_from_slice(&(name.len() as u32).to_le_bytes());
        body.extend_from_slice(name);
        let req = crate::rfs::frame(0x11, 1, &body);
        let mut e = eng(&[(245, &req)]);
        e.step(0).unwrap();
        assert_eq!(e.link.tx.len(), 1);
        assert_eq!(e.link.tx[0].0, 245);
        assert_eq!(&e.link.tx[0].1[4..6], &[0x11, 1]);
    }

    #[test]
    fn eingehender_anruf_und_sms() {
        let mut e = eng(&[(235, &[0x0a, 0, 1, 0, 0x02, 0x02, 3, 1, 1, 0])]);
        e.step(0).unwrap();
        assert_eq!(e.st.calls.len(), 1);
        assert_eq!(e.link.tx.last().unwrap().1[4..7], [0x02, 0x06, 2]);
        // SMS: Kopf (type 1, msg 1, idx 0, tpid 7, len) + PDU „How are you?"
        let pdu: Vec<u8> = (0..76)
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(
                    &"07911326040000F0040B911346610089F60000208062917314080CC8F71D14969741F977FD07"
                        [i..i + 2],
                    16,
                )
                .unwrap()
            })
            .collect();
        let mut f = vec![0, 0, 2, 0, 0x04, 0x02, 3, 1, 1, 0, 0, 7, pdu.len() as u8];
        f.extend_from_slice(&pdu);
        let n = f.len() as u16;
        f[0..2].copy_from_slice(&n.to_le_bytes());
        e.link.rx.push_back(SipcFrame {
            channel: 235,
            payload: f,
        });
        e.step(0).unwrap();
        assert_eq!(e.inbox[0].text, "How are you?");
        assert_eq!(e.link.tx.last().unwrap().1[4..7], [0x04, 0x06, 1]);
    }

    #[test]
    fn lte_signal_wie_a30() {
        let mut e = eng(&[(
            235,
            &[
                0x19, 0, 9, 0, 0x07, 0x06, 3, 0x68, 0x63, 0x63, 0x07, 0x68, 0x0d, 0xf0, 0x00, 0x02,
                0xff, 0x01, 0xff, 0, 0, 0, 0, 0, 0,
            ],
        )]);
        e.step(0).unwrap();
        assert_eq!(e.st.lte, Some((-104, -13, 240)));
    }

    #[test]
    fn sim2_auf_eigenem_kanal() {
        let mut e = Engine::with_fmt(
            Fake::default(),
            Rfs::new(std::env::temp_dir().join("ultar-ril-test2")),
            CH_FMT_1,
        );
        e.link.rx.push_back(SipcFrame {
            channel: 236,
            payload: vec![0x09, 0, 5, 0, 0x05, 0x01, 3, 0x82, 0],
        });
        e.link.rx.push_back(SipcFrame {
            channel: 235,
            payload: vec![0x09, 0, 5, 0, 0x05, 0x01, 3, 0x80, 0],
        });
        e.step(0).unwrap();
        e.step(0).unwrap();
        assert_eq!(e.st.sim, Some(0x82));
        e.query().unwrap();
        assert_eq!(e.link.tx[0].0, 236);
    }

    #[test]
    fn mobile_daten_ablauf() {
        let mut ip = vec![1u8, 0, 0];
        ip.extend_from_slice(&[
            10, 1, 2, 3, 8, 8, 8, 8, 1, 1, 1, 1, 10, 1, 2, 1, 255, 255, 255, 0, 0, 0, 0, 0,
        ]);
        let mut f = vec![0u8, 0, 7, 0, 0x0d, 0x09, 3];
        f.extend_from_slice(&ip);
        let n = f.len() as u16;
        f[0..2].copy_from_slice(&n.to_le_bytes());
        let mut e = eng(&[(235, &f)]);
        e.data_connect("internet", None, None).unwrap();
        assert_eq!(e.link.tx.len(), 2);
        assert_eq!(&e.link.tx[0].1[4..7], &[0x0d, 0x01, 3]);
        assert_eq!(&e.link.tx[1].1[4..7], &[0x0d, 0x04, 3]);
        e.step(0).unwrap();
        let c = e.st.data_ip.clone().unwrap();
        assert_eq!(c.ip.octets(), [10, 1, 2, 3]);
        assert_eq!(c.fail_cause, 0);
    }

    #[test]
    fn regist_and_radio_on() {
        let mut e = eng(&[(
            235,
            &[
                18, 0, 9, 0, 0x08, 0x05, 3, 4, 2, 2, 0, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12, 0,
            ],
        )]);
        e.step(0).unwrap();
        assert_eq!(
            e.st.reg_cs,
            Reg {
                act: 4,
                domain: 2,
                status: 2,
                lac: 0x1234,
                cid: 0x12345678
            }
        );
        e.set_radio(true).unwrap();
        assert_eq!(e.link.tx[0].1, vec![9, 0, 1, 0, 0x01, 0x07, 1, 0x02, 0x02]);
    }
}
