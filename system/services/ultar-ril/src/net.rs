//! Netz-Befehle: Power-On/Attach, Registrierungsstatus, Operator, Signal, Netzscan/-wahl.
//! NET-/DISP-/PWR-Kommando-IDs sind // VERIFY (net.h/disp.h/pwr.h waren nicht abrufbar);
//! Rumpf-Layouts firmwareabhängig – defensiv geparst, VERIFY am Gerät.

use crate::ipc::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegState {
    NotRegistered,
    Home,
    Searching,
    Denied,
    Roaming,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rat {
    Gsm,
    Edge,
    Umts,
    Hspa,
    Lte,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetState {
    pub reg: RegState,
    pub rat: Rat,
    /// PLMN als MCCMNC-Ziffern (z. B. "26201"), falls bekannt.
    pub plmn: Option<String>,
    /// Signal in dBm (Näherung), falls bekannt.
    pub signal_dbm: Option<i32>,
    /// Signal in Balken 0..5, falls bekannt.
    pub bars: Option<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plmn {
    pub mcc_mnc: String,
    pub rat: Rat,
    /// 0 unbekannt, 1 verfügbar, 2 aktuell, 3 gesperrt (Samsung-Status). // VERIFY
    pub status: u8,
}

fn map_reg(status: u8) -> RegState {
    match status {
        0 => RegState::NotRegistered,
        1 => RegState::Home,
        2 => RegState::Searching,
        3 => RegState::Denied,
        5 => RegState::Roaming,
        _ => RegState::Unknown,
    }
}

fn map_rat(act: u8) -> Rat {
    match act {
        0 | 1 => Rat::Gsm,
        3 => Rat::Edge,
        2 => Rat::Umts,
        4..=6 => Rat::Hspa,
        7 => Rat::Lte,
        _ => Rat::Unknown,
    }
}

/// Modem nach dem Boot in den Normalbetrieb bringen und ans Netz anmelden lassen.
/// VERIFY: exakte Sub-Werte der PWR/ATTACH-Nachrichten.
pub fn power_on<L: ModemLink>(ril: &mut Ril<L>, timeout_ms: u32) -> std::io::Result<()> {
    let _ = ril.exec(IPC_PWR_PHONE_PWR_UP, &[], timeout_ms)?;
    let _ = ril.exec(IPC_NET_POWER_ON_ATTACH, &[], timeout_ms)?;
    Ok(())
}

/// Registrierungsstatus + Zugangstechnik lesen (IPC_NET_REGIST).
/// Rumpf (VERIFY): [domain][status][edge][act][lac:2][cid:4]…
pub fn regist<L: ModemLink>(ril: &mut Ril<L>, timeout_ms: u32) -> std::io::Result<(RegState, Rat)> {
    match ril.get(IPC_NET_REGIST, timeout_ms)? {
        Some(m) if m.data.len() >= 4 => Ok((map_reg(m.data[1]), map_rat(m.data[3]))),
        _ => Ok((RegState::Unknown, Rat::Unknown)),
    }
}

/// Aktuellen Operator/PLMN lesen (IPC_NET_SERVING_NETWORK). Rumpf enthält 6 ASCII-PLMN-Ziffern. // VERIFY
pub fn serving_plmn<L: ModemLink>(
    ril: &mut Ril<L>,
    timeout_ms: u32,
) -> std::io::Result<Option<String>> {
    match ril.get(IPC_NET_SERVING_NETWORK, timeout_ms)? {
        Some(m) => Ok(extract_plmn(&m.data)),
        None => Ok(None),
    }
}

/// Signalstärke lesen (IPC_DISP_RSSI_INFO). // VERIFY: Rohwert->dBm/Balken.
pub fn signal<L: ModemLink>(
    ril: &mut Ril<L>,
    timeout_ms: u32,
) -> std::io::Result<(Option<i32>, Option<u8>)> {
    match ril.get(IPC_DISP_RSSI_INFO, timeout_ms)? {
        Some(m) if !m.data.is_empty() => {
            let raw = m.data[0];
            // Samsung liefert meist 0..5 Balken; manche Firmwares 0..31 (ASU).
            if raw <= 5 {
                let dbm = -113 + (raw as i32) * 10; // grobe Näherung Balken->dBm
                Ok((Some(dbm), Some(raw)))
            } else if raw <= 31 {
                let dbm = -113 + 2 * raw as i32; // ASU
                let bars = raw * 5 / 31;
                Ok((Some(dbm), Some(bars)))
            } else {
                Ok((None, None))
            }
        }
        _ => Ok((None, None)),
    }
}

/// Gesamtzustand des Netzes bequem zusammenstellen.
pub fn network_state<L: ModemLink>(ril: &mut Ril<L>, timeout_ms: u32) -> std::io::Result<NetState> {
    let (reg, rat) = regist(ril, timeout_ms)?;
    let plmn = serving_plmn(ril, timeout_ms).unwrap_or(None);
    let (signal_dbm, bars) = signal(ril, timeout_ms).unwrap_or((None, None));
    Ok(NetState {
        reg,
        rat,
        plmn,
        signal_dbm,
        bars,
    })
}

/// Verfügbare Netze suchen (IPC_NET_PLMN_LIST). Kann mehrere Frames umfassen (Transport reassembliert). // VERIFY
pub fn scan_networks<L: ModemLink>(
    ril: &mut Ril<L>,
    timeout_ms: u32,
) -> std::io::Result<Vec<Plmn>> {
    match ril.exec(IPC_NET_PLMN_LIST, &[], timeout_ms)? {
        Some(m) => Ok(parse_plmn_list(&m.data)),
        None => Ok(Vec::new()),
    }
}

/// Netz wählen: automatisch oder manuell ein bestimmtes PLMN/RAT.
/// Rumpf (VERIFY): [mode:u8][act:u8][plmn 6 ASCII]. mode 0=auto, 1=manuell.
pub fn select_network<L: ModemLink>(
    ril: &mut Ril<L>,
    manual: Option<(&str, Rat)>,
    timeout_ms: u32,
) -> std::io::Result<bool> {
    let mut body = Vec::new();
    match manual {
        None => {
            body.push(0); // auto
            body.push(0);
            body.extend_from_slice(b"000000");
        }
        Some((plmn, rat)) => {
            body.push(1); // manuell
            body.push(rat_to_act(rat));
            let mut p = plmn.bytes().take(6).collect::<Vec<u8>>();
            while p.len() < 6 {
                p.push(b'0');
            }
            body.extend_from_slice(&p);
        }
    }
    Ok(ril
        .request(IPC_NET_PLMN_SEL, IPC_TYPE_SET, &body, timeout_ms)?
        .is_some())
}

fn rat_to_act(r: Rat) -> u8 {
    match r {
        Rat::Gsm | Rat::Edge => 0,
        Rat::Umts | Rat::Hspa => 2,
        Rat::Lte => 7,
        Rat::Unknown => 0xff,
    }
}

/// Erste zusammenhängende 5–6 ASCII-Ziffern als PLMN herausziehen.
fn extract_plmn(data: &[u8]) -> Option<String> {
    let mut run = String::new();
    let mut best = String::new();
    for &b in data {
        if b.is_ascii_digit() {
            run.push(b as char);
            if run.len() > best.len() {
                best = run.clone();
            }
        } else {
            run.clear();
        }
    }
    if best.len() >= 5 {
        Some(best.chars().take(6).collect())
    } else {
        None
    }
}

/// PLMN-Liste best-effort parsen: Einträge à {status:u8, plmn 6 ASCII, act:u8}. // VERIFY
fn parse_plmn_list(data: &[u8]) -> Vec<Plmn> {
    let mut out = Vec::new();
    // Oft: [count:u16/u8][entries…]. Wir suchen robust nach 6-Ziffern-Blöcken.
    let mut i = 0;
    while i + 8 <= data.len() {
        // Kandidat: data[i]=status, data[i+1..i+7]=plmn, data[i+7]=act
        let plmn: String = data[i + 1..i + 7]
            .iter()
            .filter(|b| b.is_ascii_digit())
            .map(|b| *b as char)
            .collect();
        if plmn.len() >= 5 {
            out.push(Plmn {
                mcc_mnc: plmn,
                rat: map_rat(data[i + 7]),
                status: data[i],
            });
            i += 8;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockLink;

    #[test]
    fn regist_home_umts() {
        let body = [0x01, 0x01, 0x00, 0x02, 0, 0, 0, 0, 0, 0];
        let resp = encode(IPC_NET_REGIST, IPC_TYPE_CFRM, 1, &body);
        let mut ril = Ril::new(MockLink::with_frame(FMT_CHANNEL, resp));
        let (reg, rat) = regist(&mut ril, 1000).unwrap();
        assert_eq!(reg, RegState::Home);
        assert_eq!(rat, Rat::Umts);
    }

    #[test]
    fn plmn_extract() {
        let resp = encode(
            IPC_NET_SERVING_NETWORK,
            IPC_TYPE_CFRM,
            1,
            &[0xAA, b'2', b'6', b'2', b'0', b'1', 0x00],
        );
        let mut ril = Ril::new(MockLink::with_frame(FMT_CHANNEL, resp));
        assert_eq!(
            serving_plmn(&mut ril, 1000).unwrap().as_deref(),
            Some("26201")
        );
    }

    #[test]
    fn signal_bars() {
        let resp = encode(IPC_DISP_RSSI_INFO, IPC_TYPE_CFRM, 1, &[4]);
        let mut ril = Ril::new(MockLink::with_frame(FMT_CHANNEL, resp));
        let (dbm, bars) = signal(&mut ril, 1000).unwrap();
        assert_eq!(bars, Some(4));
        assert!(dbm.unwrap() > -113);
    }
}
