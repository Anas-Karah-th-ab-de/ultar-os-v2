//! Anrufe und SMS über Samsung-IPC (Gruppen CALL 0x02 und SMS 0x04), Aufbau nach libsamsung-ipc
//! (call.h / sms.h). Reine Hilfsfunktionen ohne Ein-/Ausgabe, damit sie testbar sind.
//!
//! Anruf: Modem meldet `CALL_INCOMING` (0x0202) bzw. `CALL_STATUS` (0x0205); danach fragt der RIL die
//! `CALL_LIST` (0x0206 GET) ab – sie liefert Nummer und Zustand. Annehmen `CALL_ANSWER` (0x0204 EXEC),
//! Auflegen `CALL_RELEASE` (0x0203 EXEC), Wählen `CALL_OUTGOING` (0x0201 EXEC).
//! SMS: Modem meldet `SMS_INCOMING_MSG` (0x0402) mit PDU (SMSC vorn), RIL bestätigt mit
//! `SMS_DELIVER_REPORT` (0x0406 EXEC). Senden: `SMS_SEND_MSG` (0x0401 EXEC).

pub const CMD_CALL_OUTGOING: u16 = 0x0201;
pub const CMD_CALL_INCOMING: u16 = 0x0202;
pub const CMD_CALL_RELEASE: u16 = 0x0203;
pub const CMD_CALL_ANSWER: u16 = 0x0204;
pub const CMD_CALL_STATUS: u16 = 0x0205;
pub const CMD_CALL_LIST: u16 = 0x0206;
pub const CMD_CALL_CONT_DTMF: u16 = 0x0208;
pub const CMD_SS_USSD: u16 = 0x0C08;
pub const CMD_SMS_SEND_MSG: u16 = 0x0401;
pub const CMD_SMS_INCOMING_MSG: u16 = 0x0402;
pub const CMD_SMS_DELIVER_REPORT: u16 = 0x0406;

/// Zustand eines Anrufs laut CALL_LIST (libsamsung-ipc IPC_CALL_LIST_ENTRY_STATUS_*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallStatus {
    Active,
    Holding,
    Dialing,
    Alerting,
    Incoming,
    Waiting,
    Other(u8),
}

impl CallStatus {
    pub fn from_u8(v: u8) -> CallStatus {
        match v {
            1 => CallStatus::Active,
            2 => CallStatus::Holding,
            3 => CallStatus::Dialing,
            4 => CallStatus::Alerting,
            5 => CallStatus::Incoming,
            6 => CallStatus::Waiting,
            o => CallStatus::Other(o),
        }
    }
    /// Schlüssel für /run/ultar/radio/call
    pub fn key(&self) -> &'static str {
        match self {
            CallStatus::Active | CallStatus::Holding => "active",
            CallStatus::Dialing | CallStatus::Alerting => "dialing",
            CallStatus::Incoming | CallStatus::Waiting => "incoming",
            CallStatus::Other(_) => "active",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallEntry {
    pub id: u8,
    /// 0 = abgehend (MO), 1 = ankommend (MT)
    pub term: u8,
    pub status: CallStatus,
    pub number: String,
}

/// CALL_LIST: `count`, dann je Eintrag `type id term status mpty number_len unknown number[number_len]`.
pub fn parse_call_list(d: &[u8]) -> Vec<CallEntry> {
    let mut out = Vec::new();
    let Some(&count) = d.first() else { return out };
    let mut o = 1;
    for _ in 0..count {
        if d.len() < o + 7 {
            break;
        }
        let (id, term, status, nlen) = (d[o + 1], d[o + 2], d[o + 3], d[o + 5] as usize);
        let num = d.get(o + 7..o + 7 + nlen).unwrap_or(&[]);
        let number: String = num
            .iter()
            .filter(|b| b.is_ascii_graphic())
            .map(|b| *b as char)
            .collect();
        out.push(CallEntry {
            id,
            term,
            status: CallStatus::from_u8(status),
            number,
        });
        o += 7 + nlen;
    }
    out
}

/// CALL_OUTGOING wie libsec-ril `IpcProtocol41Call::IpcTxCallOriginate` (Nachricht 99 Byte → Nutzdaten 92):
/// `anruftyp:u16` (0x0100 = Sprache), `clir` (0 = Netzvorgabe), `länge`, `präfix` (0x11 mit „+", sonst 0x21),
/// Nummer als ASCII (max 87, „+" bleibt drin), letztes Byte 0.
/// Am A30 bestätigt: das alte libsamsung-ipc-Format wurde mit Fehler 0x0064 abgelehnt.
pub fn build_outgoing(number: &str) -> Vec<u8> {
    let num: Vec<u8> = number
        .bytes()
        .filter(|b| b.is_ascii_digit() || matches!(b, b'+' | b'*' | b'#'))
        .take(87)
        .collect();
    let mut b = vec![0u8; 92];
    b[0] = 0x00;
    b[1] = 0x01;
    b[2] = 0x00;
    b[3] = num.len() as u8;
    b[4] = if num.first() == Some(&b'+') {
        0x11
    } else {
        0x21
    };
    b[5..5 + num.len()].copy_from_slice(&num);
    b
}

// ------------------------------------------------------------------------------------------- SMS

/// Eine empfangene SMS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InSms {
    pub from: String,
    pub text: String,
    /// Teil einer langen SMS: (Referenz, Anzahl, Nummer)
    pub part: Option<(u16, u8, u8)>,
}

/// SMS_INCOMING_MSG: Kopf `type msg_type sim_index:u16 tpid length` (6 Byte), dann PDU. Liefert (tpid, PDU).
pub fn split_incoming(d: &[u8]) -> Option<(u8, &[u8])> {
    if d.len() < 7 {
        return None;
    }
    let len = d[5] as usize;
    let pdu = d.get(6..6 + len).or_else(|| d.get(6..))?;
    Some((d[4], pdu))
}

/// SMS_DELIVER_REPORT (Bestätigung): type 0x02, error 0x0000, tpid, 0 – dazu leere RP-ACK-PDU.
pub fn build_deliver_report(tpid: u8) -> Vec<u8> {
    vec![0x02, 0x00, 0x00, tpid, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
}

fn semi_octets(b: &[u8], digits: usize) -> String {
    let mut s = String::new();
    for x in b {
        for n in [x & 0x0f, x >> 4] {
            if s.len() >= digits {
                break;
            }
            s.push(match n {
                0..=9 => (b'0' + n) as char,
                0xa => '*',
                0xb => '#',
                _ => continue,
            });
        }
    }
    s
}

/// GSM-03.38-Grundtabelle (ohne Erweiterung bis auf häufige Zeichen).
const GSM7: [char; 128] = [
    '@', '£', '$', '¥', 'è', 'é', 'ù', 'ì', 'ò', 'Ç', '\n', 'Ø', 'ø', '\r', 'Å', 'å', 'Δ', '_',
    'Φ', 'Γ', 'Λ', 'Ω', 'Π', 'Ψ', 'Σ', 'Θ', 'Ξ', '\u{1b}', 'Æ', 'æ', 'ß', 'É', ' ', '!', '"', '#',
    '¤', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', '0', '1', '2', '3', '4', '5', '6',
    '7', '8', '9', ':', ';', '<', '=', '>', '?', '¡', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I',
    'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'Ä', 'Ö',
    'Ñ', 'Ü', '§', '¿', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o',
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'ä', 'ö', 'ñ', 'ü', 'à',
];

fn gsm7_ext(c: u8) -> char {
    match c {
        0x0a => '\u{c}',
        0x14 => '^',
        0x28 => '{',
        0x29 => '}',
        0x2f => '\\',
        0x3c => '[',
        0x3d => '~',
        0x3e => ']',
        0x40 => '|',
        0x65 => '€',
        _ => '?',
    }
}

/// 7-Bit-Septette entpacken; `skip_bits` = Füllbits nach einem Benutzerdatenkopf.
fn unpack7(ud: &[u8], septets: usize, skip_bits: usize) -> String {
    let mut s = String::new();
    let mut esc = false;
    for i in 0..septets {
        let bit = skip_bits + i * 7;
        let (byte, sh) = (bit / 8, bit % 8);
        let lo = *ud.get(byte).unwrap_or(&0) as u16;
        let hi = *ud.get(byte + 1).unwrap_or(&0) as u16;
        let v = (((hi << 8) | lo) >> sh) as u8 & 0x7f;
        if esc {
            s.push(gsm7_ext(v));
            esc = false;
        } else if v == 0x1b {
            esc = true;
        } else {
            s.push(GSM7[v as usize]);
        }
    }
    s
}

/// SMS-DELIVER-PDU (mit SMSC-Vorspann) dekodieren: Absender, Text (GSM-7, UCS-2 für Arabisch, 8 Bit), Teilnummer.
pub fn decode_deliver(pdu: &[u8]) -> Option<InSms> {
    let mut o = 1 + *pdu.first()? as usize; // SMSC überspringen
    let fo = *pdu.get(o)?;
    if fo & 0x03 != 0x00 {
        return None; // kein SMS-DELIVER (z. B. Statusbericht)
    }
    let udhi = fo & 0x40 != 0;
    o += 1;
    let oa_digits = *pdu.get(o)? as usize;
    let toa = *pdu.get(o + 1)?;
    let oa_bytes = oa_digits.div_ceil(2);
    let oa = pdu.get(o + 2..o + 2 + oa_bytes)?;
    let from = if toa & 0x70 == 0x50 {
        unpack7(oa, oa_digits * 4 / 7, 0) // alphanumerischer Absender (z. B. „Syriatel")
    } else {
        let n = semi_octets(oa, oa_digits);
        if toa & 0x70 == 0x10 {
            format!("+{n}")
        } else {
            n
        }
    };
    o += 2 + oa_bytes;
    let _pid = *pdu.get(o)?;
    let dcs = *pdu.get(o + 1)?;
    o += 2 + 7; // SCTS
    let udl = *pdu.get(o)? as usize;
    let ud = pdu.get(o + 1..)?;
    let mut part = None;
    let (mut hdr_len, mut body) = (0usize, ud);
    if udhi {
        let udhl = *ud.first()? as usize;
        hdr_len = udhl + 1;
        let h = ud.get(1..1 + udhl)?;
        let mut i = 0;
        while i + 2 <= h.len() {
            let (iei, l) = (h[i], h[i + 1] as usize);
            let v = h.get(i + 2..i + 2 + l).unwrap_or(&[]);
            match (iei, v.len()) {
                (0x00, 3) => part = Some((v[0] as u16, v[1], v[2])),
                (0x08, 4) => part = Some((u16::from_be_bytes([v[0], v[1]]), v[2], v[3])),
                _ => {}
            }
            i += 2 + l;
        }
        body = ud.get(hdr_len..)?;
    }
    // Alphabet aus DCS (allgemeine Gruppe 00xx und Gruppe F)
    let alpha = if dcs & 0xc0 == 0x00 || dcs & 0xf0 == 0xf0 {
        (dcs >> 2) & 0x03
    } else if dcs & 0xf0 == 0xe0 {
        2
    } else {
        0
    };
    let text = match alpha {
        2 => {
            let n = udl.saturating_sub(hdr_len).min(body.len());
            let u: Vec<u16> = body[..n]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&u)
        }
        1 => String::from_utf8_lossy(&body[..udl.saturating_sub(hdr_len).min(body.len())])
            .into_owned(),
        _ => {
            // Septette: udl zählt Septette inkl. Kopf; Kopf belegt ceil(hdr_len*8/7) Septette
            let hdr_septets = (hdr_len * 8).div_ceil(7);
            let fill = hdr_septets * 7 - hdr_len * 8;
            unpack7(body, udl.saturating_sub(hdr_septets), fill)
        }
    };
    Some(InSms { from, text, part })
}

fn is_gsm7(s: &str) -> bool {
    s.chars().all(|c| GSM7.contains(&c) && c != '\u{1b}')
}

/// SMS-SUBMIT-PDU (ohne SMSC) bauen. GSM-7, wenn möglich, sonst UCS-2 (Arabisch). Nur Einzel-SMS
/// (≤ 160 bzw. ≤ 70 Zeichen); längere Texte werden gekürzt – der Aufrufer teilt vorher.
pub fn encode_submit(to: &str, text: &str) -> Vec<u8> {
    let intl = to.starts_with('+');
    let digits: Vec<u8> = to.bytes().filter(|b| b.is_ascii_digit()).collect();
    let mut p = vec![0x01, 0x00]; // SMS-SUBMIT, Referenz 0 (Modem vergibt)
    p.push(digits.len() as u8);
    p.push(if intl { 0x91 } else { 0x81 });
    for ch in digits.chunks(2) {
        let lo = ch[0] - b'0';
        let hi = ch.get(1).map(|c| c - b'0').unwrap_or(0x0f);
        p.push((hi << 4) | lo);
    }
    p.push(0x00); // PID
    if is_gsm7(text) {
        let sept: Vec<u8> = text
            .chars()
            .take(160)
            .map(|c| GSM7.iter().position(|g| *g == c).unwrap_or(0x3f) as u8)
            .collect();
        p.push(0x00); // DCS GSM-7
        p.push(sept.len() as u8);
        let mut acc: u32 = 0;
        let mut bits = 0;
        for s in sept {
            acc |= (s as u32) << bits;
            bits += 7;
            while bits >= 8 {
                p.push(acc as u8);
                acc >>= 8;
                bits -= 8;
            }
        }
        if bits > 0 {
            p.push(acc as u8);
        }
    } else {
        let u: Vec<u16> = text.encode_utf16().take(70).collect();
        p.push(0x08); // DCS UCS-2
        p.push((u.len() * 2) as u8);
        for c in u {
            p.extend_from_slice(&c.to_be_bytes());
        }
    }
    p
}

/// SMS_SEND_MSG: Kopf `type 0x02 (abgehend), msg_type 0x01 (einzeln), länge, smsc_len` + SMSC + PDU.
pub fn build_send(smsc: &[u8], pdu: &[u8]) -> Vec<u8> {
    let mut b = vec![
        0x02,
        0x01,
        (1 + smsc.len() + pdu.len()) as u8,
        smsc.len() as u8,
    ];
    b.extend_from_slice(smsc);
    b.extend_from_slice(pdu);
    b
}

// ------------------------------------------------------------------------------------------- USSD

/// GSM-7 packen (wie libsec-ril `PackToGsm7bit`).
pub fn pack7(text: &str) -> (Vec<u8>, usize) {
    let sept: Vec<u8> = text
        .chars()
        .map(|c| GSM7.iter().position(|g| *g == c).unwrap_or(0x3f) as u8)
        .collect();
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for s in &sept {
        acc |= (*s as u32) << bits;
        bits += 7;
        while bits >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            bits -= 8;
        }
    }
    if bits > 0 {
        out.push(acc as u8);
    }
    (out, sept.len())
}

/// SS_USSD (0x0C08 EXEC) wie libsec-ril `IpcTxSsSendUssd`: Nutzdaten 185 Byte =
/// `zustand` (1 = neue Anfrage, 2 = Antwort in laufender Sitzung), `dcs` 0x0F, `länge`, GSM-7 gepackt (max 182).
pub fn build_ussd(code: &str, in_session: bool) -> Vec<u8> {
    let (packed, _) = pack7(code);
    let n = packed.len().min(182);
    let mut b = vec![0u8; 185];
    b[0] = if in_session { 2 } else { 1 };
    b[1] = 0x0f;
    b[2] = n as u8;
    b[3..3 + n].copy_from_slice(&packed[..n]);
    b
}

/// USSD-Sitzung beenden (`IpcTxSsCancelUssd`: Zustand 3).
pub fn build_ussd_cancel() -> Vec<u8> {
    let mut b = vec![0u8; 185];
    b[0] = 0x03;
    b
}

/// Antwort des Netzes (SS_USSD NOTI): `zustand dcs länge daten`. Zustand 1 = fertig, 2 = Antwort erwartet,
/// 3 = vom Netz beendet, 4 = anderer Teilnehmer, 5 = nicht unterstützt, 6 = Zeitüberschreitung.
pub fn parse_ussd(d: &[u8]) -> Option<(u8, String)> {
    if d.len() < 3 {
        return None;
    }
    let (state, dcs, len) = (d[0], d[1], d[2] as usize);
    let data = d.get(3..3 + len).unwrap_or(&d[3..]);
    let ucs2 = dcs & 0x0c == 0x08 || dcs == 0x11 || dcs & 0xf0 == 0x40 && dcs & 0x0c == 0x08;
    let text = if ucs2 {
        let u: Vec<u16> = data
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u)
    } else if dcs & 0x0c == 0x04 {
        String::from_utf8_lossy(data).into_owned()
    } else {
        unpack7(data, data.len() * 8 / 7, 0)
            .trim_end_matches('\r')
            .to_string()
    };
    Some((state, text))
}

/// Ist die Eingabe ein USSD-/MMI-Code (z. B. `*111#`, `#123*1#`)?
pub fn is_ussd(s: &str) -> bool {
    let s = s.trim();
    s.len() >= 2
        && (s.starts_with('*') || s.starts_with('#'))
        && s.ends_with('#')
        && s.chars()
            .all(|c| c.is_ascii_digit() || c == '*' || c == '#')
}

/// Text sicher für eine Zeile kodieren (Hex von UTF-8).
pub fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

pub fn unhex(s: &str) -> String {
    let b: Vec<u8> = s
        .as_bytes()
        .chunks(2)
        .filter_map(|c| u8::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok())
        .collect();
    String::from_utf8_lossy(&b).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn deliver_gsm7() {
        // Beispiel aus 3GPP/Literatur: „hellohello" von +31641600986
        let pdu = h("07911326040000F0040B911346610089F60000208062917314080CC8F71D14969741F977FD07");
        let s = decode_deliver(&pdu).unwrap();
        assert_eq!(s.from, "+31641600986");
        assert_eq!(s.text, "How are you?");
    }

    #[test]
    fn deliver_ucs2_arabisch() {
        // SMSC leer, Absender 963944123456 (international), DCS 08, Text „مرحبا"
        let mut pdu = h("00040C916993442143650008711102000000000A");
        for c in "مرحبا".encode_utf16() {
            pdu.extend_from_slice(&c.to_be_bytes());
        }
        let s = decode_deliver(&pdu).unwrap();
        assert_eq!(s.from, "+963944123456");
        assert_eq!(s.text, "مرحبا");
    }

    #[test]
    fn submit_rundlauf_gsm7() {
        let p = encode_submit("+491701234567", "Hallo");
        assert_eq!(&p[..4], &[0x01, 0x00, 12, 0x91]);
        // als DELIVER verpacken und zurücklesen
        let mut d = vec![0x00, 0x04, 12, 0x91];
        d.extend_from_slice(&p[4..10]);
        d.extend_from_slice(&[0x00, 0x00]);
        d.extend_from_slice(&[0; 7]);
        d.extend_from_slice(&p[12..]);
        assert_eq!(decode_deliver(&d).unwrap().text, "Hallo");
    }

    #[test]
    fn anrufliste() {
        // 1 Eintrag: type 1, id 1, term 1 (MT), status 5 (eingehend), mpty 0, len 10, unk 0, "0944123456"
        let mut d = vec![1, 1, 1, 1, 5, 0, 10, 0];
        d.extend_from_slice(b"0944123456");
        let l = parse_call_list(&d);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].status, CallStatus::Incoming);
        assert_eq!(l[0].number, "0944123456");
        assert!(parse_call_list(&[0]).is_empty());
    }

    #[test]
    fn ussd_rundlauf() {
        let b = build_ussd("*111#", false);
        assert_eq!((b[0], b[1], b.len()), (1, 0x0f, 185));
        // Antwort GSM-7 zurücklesen
        let (p, _) = pack7("Guthaben: 500 SYP");
        let mut d = vec![1, 0x0f, p.len() as u8];
        d.extend_from_slice(&p);
        assert_eq!(parse_ussd(&d).unwrap().1, "Guthaben: 500 SYP");
        // UCS-2 (Arabisch)
        let mut d = vec![2, 0x48, 0];
        for c in "رصيدك".encode_utf16() {
            d.extend_from_slice(&c.to_be_bytes());
        }
        d[2] = (d.len() - 3) as u8;
        assert_eq!(parse_ussd(&d).unwrap(), (2, "رصيدك".to_string()));
        assert!(is_ussd("*111#") && is_ussd("#123*1#") && !is_ussd("0944123456"));
    }

    #[test]
    fn waehlen() {
        let b = build_outgoing("+963944123456");
        assert_eq!(
            (b.len(), b[0], b[1], b[3], b[4]),
            (92, 0x00, 0x01, 13, 0x11)
        );
        assert_eq!(&b[5..18], b"+963944123456");
        let b = build_outgoing("0992762987");
        assert_eq!((b[3], b[4]), (10, 0x21));
    }
}
