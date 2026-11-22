//! SIM-Befehle: Status/PIN, IMSI, Kartentyp. Baut auf `ipc::Ril` + `ModemLink` auf.

use crate::ipc::*;

/// Zustand der SIM-Karte (aus IPC_SEC_PIN_STATUS abgeleitet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimState {
    Ready,
    PinRequired,
    PukRequired,
    NetworkLocked,
    Absent,
    Error,
    Initializing,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardType {
    Unknown,
    Sim,
    Usim,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimStatus {
    pub present: bool,
    pub state: SimState,
    pub raw: u8,
}

// Statuswerte aus include/sec.h (bestätigt):
const PIN_READY: u8 = 0x00;
const PIN_SIM_LOCK_REQUIRED: u8 = 0x01; // PIN/SIM-Sperre
const PIN_LOCK_SC: u8 = 0x03; // SIM-PIN (SC)
const PIN_LOCK_PN: u8 = 0x05; // Netzsperre
const PIN_CARD_NOT_PRESENT: u8 = 0x80;
const PIN_CARD_ERROR: u8 = 0x81;
const PIN_INIT_COMPLETE: u8 = 0x82;
const PIN_PB_INIT_COMPLETE: u8 = 0x83;

fn map_status(raw: u8) -> SimStatus {
    let state = match raw {
        PIN_READY | PIN_INIT_COMPLETE | PIN_PB_INIT_COMPLETE => SimState::Ready,
        PIN_SIM_LOCK_REQUIRED | PIN_LOCK_SC => SimState::PinRequired,
        0x02 => SimState::Error, // INSIDE_PF_ERROR
        PIN_LOCK_PN | 0x06 | 0x07 | 0x08 => SimState::NetworkLocked,
        PIN_CARD_NOT_PRESENT => SimState::Absent,
        PIN_CARD_ERROR => SimState::Error,
        _ => SimState::Unknown,
    };
    let present = !matches!(state, SimState::Absent);
    SimStatus {
        present,
        state,
        raw,
    }
}

/// SIM-/PIN-Status lesen.
pub fn sim_status<L: ModemLink>(ril: &mut Ril<L>, timeout_ms: u32) -> std::io::Result<SimStatus> {
    match ril.get(IPC_SEC_PIN_STATUS, timeout_ms)? {
        // Antwortrumpf: {type, key/status, …}. Das Statusbyte ist je nach Firmware data[0] oder data[1];
        // wir nehmen das erste Byte, das einem bekannten Code entspricht. // VERIFY gegen Gerät.
        Some(m) => {
            // Karten-Status (>=0x80: absent/error/init) hat Vorrang vor Facility-Sperren (0x01..0x08).
            let raw = m
                .data
                .iter()
                .copied()
                .find(|b| *b >= 0x80 && is_known_status(*b))
                .or_else(|| m.data.iter().copied().find(|b| is_known_status(*b)))
                .or_else(|| m.data.first().copied())
                .unwrap_or(0xFF);
            Ok(map_status(raw))
        }
        None => Ok(SimStatus {
            present: false,
            state: SimState::Unknown,
            raw: 0xFF,
        }),
    }
}

fn is_known_status(b: u8) -> bool {
    matches!(
        b,
        PIN_READY
            | PIN_SIM_LOCK_REQUIRED
            | 0x02
            | PIN_LOCK_SC
            | 0x04
            | PIN_LOCK_PN
            | 0x06
            | 0x07
            | 0x08
            | PIN_CARD_NOT_PRESENT
            | PIN_CARD_ERROR
            | PIN_INIT_COMPLETE
            | PIN_PB_INIT_COMPLETE
    )
}

/// SIM-PIN eingeben (IPC_SEC_PIN_STATUS, Typ SET).
/// Rumpf nach libsamsung-ipc `ipc_sec_pin_status_request_data`:
/// [type=PIN1 0x03][len1][len2][pin1 8 Byte][pin2 8 Byte]. // VERIFY am Gerät (Antwort = CFRM/GEN_PHONE_RES).
/// Liefert true, wenn das Modem geantwortet hat; den neuen Zustand danach mit `sim_status` prüfen.
pub fn enter_pin<L: ModemLink>(
    ril: &mut Ril<L>,
    pin: &str,
    timeout_ms: u32,
) -> std::io::Result<bool> {
    let digits: Vec<u8> = pin.bytes().filter(|b| b.is_ascii_digit()).collect();
    if !(4..=8).contains(&digits.len()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "PIN muss 4–8 Ziffern haben",
        ));
    }
    Ok(ril
        .request(
            IPC_SEC_PIN_STATUS,
            IPC_TYPE_SET,
            &pin_body(&digits),
            timeout_ms,
        )?
        .is_some())
}

fn pin_body(digits: &[u8]) -> Vec<u8> {
    const PIN_TYPE_PIN1: u8 = 0x03;
    let mut b = vec![PIN_TYPE_PIN1, digits.len() as u8, 0];
    let mut p1 = [0u8; 8];
    p1[..digits.len()].copy_from_slice(digits);
    b.extend_from_slice(&p1);
    b.extend_from_slice(&[0u8; 8]);
    b
}

/// Kartentyp lesen (IPC_SEC_SIM_ICC_TYPE).
pub fn card_type<L: ModemLink>(ril: &mut Ril<L>, timeout_ms: u32) -> std::io::Result<CardType> {
    match ril.get(IPC_SEC_SIM_ICC_TYPE, timeout_ms)? {
        Some(m) => Ok(match m.data.first().copied().unwrap_or(0) {
            0x01 => CardType::Sim,
            0x02 => CardType::Usim,
            _ => CardType::Unknown,
        }),
        None => Ok(CardType::Unknown),
    }
}

/// IMSI lesen (IPC_MISC_ME_IMSI). Antwort: Längen-Byte + ASCII-Ziffern.
pub fn imsi<L: ModemLink>(ril: &mut Ril<L>, timeout_ms: u32) -> std::io::Result<Option<String>> {
    match ril.get(IPC_MISC_ME_IMSI, timeout_ms)? {
        Some(m) if !m.data.is_empty() => {
            let len = m.data[0] as usize;
            let bytes = m.data.get(1..1 + len).unwrap_or(&m.data[1..]);
            let s: String = bytes
                .iter()
                .filter(|b| b.is_ascii_digit())
                .map(|b| *b as char)
                .collect();
            Ok(if s.is_empty() { None } else { Some(s) })
        }
        _ => Ok(None),
    }
}

/// Seriennummer/IMEI (IPC_MISC_ME_SN, Subtyp SERIAL=0x01). // VERIFY: Rumpfaufbau firmwareabhängig.
pub fn imei<L: ModemLink>(ril: &mut Ril<L>, timeout_ms: u32) -> std::io::Result<Option<String>> {
    // EXEC mit Subtyp im ersten Byte.
    match ril.exec(IPC_MISC_ME_SN, &[0x01], timeout_ms)? {
        Some(m) => {
            // Rumpf: [subtyp][länge][ascii…] – defensiv die längste Ziffernkette nehmen.
            let digits: String = m
                .data
                .iter()
                .filter(|b| b.is_ascii_digit())
                .map(|b| *b as char)
                .collect();
            Ok(if digits.len() >= 14 {
                Some(digits)
            } else {
                None
            })
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockLink;

    #[test]
    fn pin_body_layout() {
        let b = pin_body(b"1234");
        assert_eq!(b.len(), 19);
        assert_eq!(&b[..3], &[0x03, 4, 0]);
        assert_eq!(&b[3..7], b"1234");
        assert!(b[7..].iter().all(|x| *x == 0));
    }

    #[test]
    fn sim_ready() {
        // Antwort IPC_SEC_PIN_STATUS, type CFRM, Rumpf mit Status READY (0x00) an [1]
        let resp = encode(
            IPC_SEC_PIN_STATUS,
            IPC_TYPE_CFRM,
            1,
            &[0x00, PIN_INIT_COMPLETE],
        );
        let mut ril = Ril::new(MockLink::with_frame(FMT_CHANNEL, resp));
        let st = sim_status(&mut ril, 1000).unwrap();
        assert_eq!(st.state, SimState::Ready);
        assert!(st.present);
    }

    #[test]
    fn sim_absent() {
        let resp = encode(
            IPC_SEC_PIN_STATUS,
            IPC_TYPE_CFRM,
            1,
            &[0x01, PIN_CARD_NOT_PRESENT],
        );
        let mut ril = Ril::new(MockLink::with_frame(FMT_CHANNEL, resp));
        let st = sim_status(&mut ril, 1000).unwrap();
        assert_eq!(st.state, SimState::Absent);
        assert!(!st.present);
    }

    #[test]
    fn imsi_parse() {
        let mut body = vec![6u8];
        body.extend_from_slice(b"310260");
        let resp = encode(IPC_MISC_ME_IMSI, IPC_TYPE_CFRM, 1, &body);
        let mut ril = Ril::new(MockLink::with_frame(FMT_CHANNEL, resp));
        assert_eq!(imsi(&mut ril, 1000).unwrap().as_deref(), Some("310260"));
    }
}
