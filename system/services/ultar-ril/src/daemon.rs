//! Funkdienst (`ultar-ril --daemon`): hält den Mobilfunk-Zustand für die Oberfläche aktuell.
//!
//! Schreibt `/run/ultar/radio/state` (ultar-radio-Format), liest die Vorgaben aus
//! `/data/ultar/radio.conf` und führt Einmal-Befehle aus `/run/ultar/radio/cmd` aus.
//!
//! Ehrlicher Stand der Modem-Befehle (Shannon/Samsung-IPC):
//! - SIM-Status, Registrierung, Netz (PLMN), Signal: vorhanden (NET/DISP-IDs // VERIFY).
//! - SIM-PIN: `sim::enter_pin` (// VERIFY).
//! - Nachbarzellen-Liste und Zellsperre: Samsung-IPC-Befehl noch nicht bestimmt → die Oberfläche
//!   zeigt dann nur die verbundene Zelle und markiert Manuell als „nur bevorzugt".
//!   Smart läuft trotzdem mit (Hysterese), sobald Nachbarzellen geliefert werden.

use std::time::{Duration, Instant};
use ultar_radio as radio;
use ultar_ril::engine::{Engine, State};
use ultar_ril::ipc::IPC_TYPE_SET;
use ultar_ril::link::{UmtsLink, CH_FMT_0, CH_FMT_1, CH_RFS_0};
use ultar_ril::rfs::Rfs;
use ultar_ril::tel::CallStatus;

const POLL: Duration = Duration::from_secs(3);
const RETRY: Duration = Duration::from_secs(5);

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn offline(note: &str) {
    let s = radio::RadioState { modem: radio::Modem::Offline, updated: now(), note: note.into(), ..Default::default() };
    let _ = radio::write_state(&s);
}



/// Ordner mit der Arbeitskopie der Modem-Einstellungen (nv_data.bin) – nie die EFS-Partition selbst.
const MODEM_DIR: &str = "/data/ultar/modem";
const IPC_LOG: &str = "/run/ultar/radio/ipc.log";
/// Flugmodus: Datei vorhanden = Funk aus.
pub const AIRPLANE_FLAG: &str = "/data/ultar/airplane.on";
/// Mobile Daten gewünscht (Schnelleinstellungen schreiben die Datei).
pub const MOBILE_DATA_FLAG: &str = "/run/ultar/mobiledata.on";
/// Zugangsdaten: Zeilen `apn=…`, `user=…`, `pass=…` (fehlt die Datei: leerer APN = Netz-Vorgabe).
pub const APN_CONF: &str = "/data/ultar/apn.conf";

struct Apn {
    apn: String,
    user: Option<String>,
    pass: Option<String>,
}

fn read_apn() -> Apn {
    let txt = std::fs::read_to_string(APN_CONF).unwrap_or_default();
    let get = |k: &str| {
        txt.lines().filter_map(|l| l.split_once('=')).find(|(a, _)| a.trim() == k).map(|(_, v)| v.trim().to_string()).filter(|v| !v.is_empty())
    };
    Apn { apn: get("apn").unwrap_or_default(), user: get("user"), pass: get("pass") }
}

/// Hinweis bei fehlender Karte: wie viele Steckplätze dem Modem gemeldet wurden (ultar-modem, wie cbd).
fn no_sim_note() -> String {
    match std::fs::read_to_string("/run/ultar/radio/simslots").ok().and_then(|s| s.trim().parse::<u8>().ok()) {
        Some(n) => format!("Keine SIM erkannt (Modem kennt {n} Steckplatz/-plätze)"),
        None => "Keine SIM erkannt (SIM-Steckplätze dem Modem nicht gemeldet)".into(),
    }
}

/// Zustand des Modems laut Kernel (sysfs des Modem-Treibers); fehlt die Datei, gilt das Modem als erreichbar.
fn modem_online() -> bool {
    match std::fs::read_to_string("/sys/devices/platform/10000.mif_pdata/modem_state") {
        Ok(s) => s.to_ascii_uppercase().contains("ONLINE") || s.trim() == "4",
        Err(_) => true,
    }
}

/// SIM-Status aus PIN_STATUS (libsamsung-ipc sec.h): 0x00 bereit, 0x03 PIN/PUK (Sperre SC), 0x01/0x04–0x08 Netz-/
/// Geräte-Sperren, 0x80 keine Karte, 0x81/0x02 Kartenfehler, 0x82 startet, 0x83 Telefonbuch fertig (bereit).
/// Am A30 bestätigt: SIM 2 eingelegt → `00 00`, leerer Steckplatz → `80 00`.
fn sim_state(raw: Option<u8>) -> radio::SimState {
    match raw {
        Some(0x00) | Some(0x83) => radio::SimState::Ready,
        Some(0x03) => radio::SimState::Pin,
        Some(0x01) | Some(0x04..=0x08) => radio::SimState::Locked,
        Some(0x80) => radio::SimState::Absent,
        Some(0x81) | Some(0x02) => radio::SimState::Error,
        _ => radio::SimState::Unknown,
    }
}

fn rat_name(act: u8) -> &'static str {
    match act {
        0 | 1 => "GSM",
        2 => "GPRS",
        3 => "EDGE",
        4 => "UMTS",
        5..=7 => "HSPA",
        8 | 0x21 => "LTE", // 0x21: Shannon meldet LTE am A30 so (gemessen)
        _ => "",
    }
}

/// Anbietername zur Netzkennung (MCC+MNC); unbekannte Netze zeigen die Kennung.
pub fn operator_name(plmn: &str) -> String {
    match plmn {
        "41701" => "Syriatel",
        "41702" => "MTN Syria",
        "41709" => "Wafa Telecom",
        "26201" | "26206" => "Telekom.de",
        "26202" | "26204" | "26209" => "Vodafone.de",
        "26203" | "26207" | "26208" | "26211" => "o2 - de",
        "26223" => "1&1",
        "28601" => "Turkcell",
        "28602" => "Vodafone TR",
        "28603" => "Türk Telekom",
        "41501" => "Alfa",
        "41503" => "touch",
        "41601" => "Zain JO",
        "41677" => "Orange JO",
        "41603" => "Umniah",
        other => other,
    }
    .to_string()
}

fn to_radio(st: &State, note: &str) -> radio::RadioState {
    let mut s = radio::RadioState { modem: radio::Modem::Online, updated: now(), note: note.into(), ..Default::default() };
    s.sims.push(radio::Sim { slot: 1, state: sim_state(st.sim), operator: String::new(), number: String::new() });
    // paketvermittelte Anmeldung zeigt die Datentechnik, sonst leitungsvermittelt
    let reg = if matches!(st.reg_ps.status, 2 | 6) { &st.reg_ps } else { &st.reg_cs };
    // Netz gefunden, aber ohne SIM bzw. ohne Anmeldung: nur Notrufe – Anbieter trotzdem zeigen
    if st.reg_cs.status == 4 && !matches!(st.reg_ps.status, 2 | 6) {
        s.plmn = st.plmn.clone();
        s.operator = operator_name(&st.plmn);
        s.rat = rat_name(st.reg_cs.act).into();
        if s.note.is_empty() {
            s.note = "Nur Notrufe".into();
        }
    }
    // Netz bekannt (SERVING_NETWORK liefert PLMN), Anmeldestatus aber noch nicht gedeutet: Anbieter trotzdem zeigen
    if s.operator.is_empty() && !st.plmn.is_empty() && st.plmn.bytes().any(|b| b != b'0') {
        s.plmn = st.plmn.clone();
        s.operator = operator_name(&st.plmn);
        if reg.act == 0x21 || st.lte.is_some() {
            s.rat = "LTE".into(); // // VERIFY: Shannon meldet LTE am A30 als 0x21
        }
    }
    // Signal der verbundenen Zelle: LTE-Messung (RSRP/RSRQ/SINR) des Modems
    if !s.operator.is_empty() && s.cells.is_empty() {
        if let Some((rsrp, rsrq, sinr10)) = st.lte {
            s.serving = Some(1);
            s.cells.push(radio::Cell { id: 1, rat: s.rat.clone(), rsrp: Some(rsrp), rsrq: Some(rsrq), sinr: Some(sinr10 / 10), ..Default::default() });
        }
    }
    if matches!(reg.status, 2 | 6) {
        s.roaming = reg.status == 6;
        s.rat = rat_name(reg.act).into();
        s.plmn = st.plmn.clone();
        s.operator = operator_name(&st.plmn);
        s.serving = Some(1);
        // RSSI des Modems: 0..31 wie bei AT+CSQ → dBm = −113 + 2·n (grobe Näherung, bis die Zellmessung steht)
        let dbm = st.lte.map(|l| l.0).or_else(|| st.rssi.filter(|r| *r <= 31).map(|r| -113 + 2 * r as i32));
        let (rsrq, sinr) = st.lte.map(|l| (Some(l.1), Some(l.2 / 10))).unwrap_or((None, None));
        s.cells.push(radio::Cell { id: 1, rat: s.rat.clone(), rsrp: dbm, rsrq, sinr, ..Default::default() });
    }
    s
}

fn flush_log(lines: &mut Vec<String>) {
    use std::io::Write;
    if lines.is_empty() {
        return;
    }
    if std::fs::metadata(IPC_LOG).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(IPC_LOG, format!("{IPC_LOG}.alt"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(IPC_LOG) {
        let t = now();
        for l in lines.drain(..) {
            let _ = writeln!(f, "{t} {l}");
        }
    }
    lines.clear();
}

fn pin_body(pin: &str) -> Option<Vec<u8>> {
    let d: Vec<u8> = pin.bytes().filter(|b| b.is_ascii_digit()).collect();
    if !(4..=8).contains(&d.len()) {
        return None;
    }
    // ipc_sec_pin_status_request_data: typ (PIN1 = 3), Länge PIN, Länge PUK, PIN[8], PUK[8]
    let mut b = vec![0x03, d.len() as u8, 0];
    let mut p = [0u8; 8];
    p[..d.len()].copy_from_slice(&d);
    b.extend_from_slice(&p);
    b.extend_from_slice(&[0u8; 8]);
    Some(b)
}

pub fn run() -> ! {
    eprintln!("ultar-ril: Funkdienst gestartet");
    let _ = std::fs::create_dir_all("/run/ultar/radio");
    loop {
        // Beide Kanäle müssen offen sein: erst dann meldet der Kernel dem Modem „Funkdienst bereit"
        let mut link = UmtsLink::empty();
        if let Err(e) = link.ensure(CH_FMT_0).and_then(|_| link.ensure(CH_RFS_0)) {
            offline(&format!("Modem nicht bereit ({e})"));
            std::thread::sleep(RETRY);
            continue;
        }
        // SIM 2 (Dual-SIM): eigener FMT-Kanal /dev/umts_ipc1 – wie Androids zweiter rild (`rild -c 2`)
        let mut link2 = UmtsLink::empty();
        let has2 = link2.ensure(CH_FMT_1).is_ok();
        // Kanäle lassen sich auch bei ausgeschaltetem Modem öffnen – erst senden, wenn der Kernel „ONLINE" meldet
        let mut waited = 0;
        while !modem_online() {
            offline(if waited < 6 { "Modem startet …" } else { "Modem ist aus (Dienst modem prüfen)" });
            std::thread::sleep(RETRY);
            waited += 1;
        }
        let mut s1 = Slot::new(Engine::new(link, Rfs::new(MODEM_DIR)));
        let mut s2 = has2.then(|| Slot::new(Engine::with_fmt(link2, Rfs::new(format!("{MODEM_DIR}/sim2")), CH_FMT_1)));
        let mut note = String::new();
        let mut sms_join: Vec<(u8, ultar_ril::tel::InSms, Instant)> = Vec::new();
        let mut last_call: Option<(u8, ultar_ril::tel::CallEntry)> = None;
        write_call(None);
        let mut voice = CallAudio::default();
        let mut last_list = Instant::now();
        let (mut last_query, mut last_write, mut last_alive) = (Instant::now() - POLL, Instant::now() - POLL, Instant::now());
        let mut last_state = (State::default(), State::default());
        // Mobile Daten: Steckplatz, angefordert seit, eingerichtete Adresse
        let (mut data_slot, mut data_req, mut data_cfg): (u8, Option<Instant>, Option<std::net::Ipv4Addr>) = (0, None, None);
        'run: loop {
            if last_alive.elapsed() >= POLL {
                last_alive = Instant::now();
                if !modem_online() {
                    offline("Modem ist ausgefallen – wird neu verbunden");
                    break;
                }
            }
            let on = !std::path::Path::new(AIRPLANE_FLAG).exists();
            for (i, sl) in std::iter::once(&mut s1).chain(s2.as_mut()).enumerate() {
                if let Err(e) = sl.eng.step(if i == 0 { 300 } else { 100 }) {
                    offline(&format!("Verbindung zum Modem verloren ({e})"));
                    break 'run;
                }
                sl.housekeeping(on);
            }
            for c in radio::take_cmds() {
                let mut it = c.split_whitespace();
                match (it.next(), it.next(), it.next()) {
                    (Some("pin"), slot, Some(code)) => match pin_body(code) {
                        Some(b) => {
                            let sl = if slot == Some("2") { s2.as_mut().unwrap_or(&mut s1) } else { &mut s1 };
                            let _ = sl.eng.send(ultar_ril::engine::CMD_SEC_PIN_STATUS, IPC_TYPE_SET, &b);
                            note = "PIN gesendet".into();
                        }
                        None => note = "PIN muss 4–8 Ziffern haben".into(),
                    },
                    (Some("select"), Some(_id), _) => note = "Zellsperre: Modem-Befehl noch nicht verfügbar – Zelle gilt als bevorzugt".into(),
                    (Some("raw"), Some(cmd), data) => {
                        // Entwicklung: "raw <cmd hex> <typ><daten hex>" (SIM 1) bzw. "raw2 …" nicht nötig: "raw" geht an SIM 1
                        let bytes: Vec<u8> = data.unwrap_or("").as_bytes().chunks(2).filter_map(|c| u8::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok()).collect();
                        if let (Ok(cmd), Some((ty, rest))) = (u16::from_str_radix(cmd, 16), bytes.split_first()) {
                            let _ = s1.eng.send(cmd, *ty, rest);
                        }
                    }
                    (Some("dial"), slot, Some(num)) => {
                        let n = tel_slot(slot, radio::read_prefs().call_sim, &s1, s2.as_ref());
                        let sl = if n == 2 { s2.as_mut().unwrap_or(&mut s1) } else { &mut s1 };
                        let _ = sl.eng.dial(num);
                        note = format!("Wähle {num} (SIM {n})");
                    }
                    (Some("answer"), _, _) => {
                        for sl in std::iter::once(&mut s1).chain(s2.as_mut()) {
                            if sl.eng.st.calls.iter().any(|c| matches!(c.status, CallStatus::Incoming | CallStatus::Waiting)) {
                                let _ = sl.eng.answer();
                            }
                        }
                    }
                    (Some("mute"), _, Some(on)) => {
                        // SND_MIC_MUTE_CTRL: Mikrofon im Gespräch stumm (1) / an (0)
                        let v = u8::from(on == "1");
                        for sl in std::iter::once(&mut s1).chain(s2.as_mut()) {
                            if !sl.eng.st.calls.is_empty() {
                                let _ = sl.eng.send(0x0902, IPC_TYPE_SET, &[v]);
                            }
                        }
                    }
                    (Some("hangup"), _, _) => {
                        for sl in std::iter::once(&mut s1).chain(s2.as_mut()) {
                            if !sl.eng.st.calls.is_empty() {
                                let _ = sl.eng.hangup();
                            }
                        }
                    }
                    (Some("sms"), slot, Some(num)) => {
                        let text = ultar_ril::tel::unhex(it.next().unwrap_or(""));
                        let n = tel_slot(slot, radio::read_prefs().sms_sim, &s1, s2.as_ref());
                        let sl = if n == 2 { s2.as_mut().unwrap_or(&mut s1) } else { &mut s1 };
                        let _ = sl.eng.send_sms(num, &text, &read_smsc());
                        note = format!("SMS an {num} gesendet (SIM {n})");
                    }
                    (Some("ussd"), slot, Some(code)) => {
                        let code = ultar_ril::tel::unhex(code);
                        let n = tel_slot(slot, radio::read_prefs().call_sim, &s1, s2.as_ref());
                        let sl = if n == 2 { s2.as_mut().unwrap_or(&mut s1) } else { &mut s1 };
                        let _ = sl.eng.send_ussd(&code);
                        write_ussd(n, 9, "");
                    }
                    (Some("ussd_cancel"), _, _) => {
                        for sl in std::iter::once(&mut s1).chain(s2.as_mut()) {
                            if sl.eng.ussd_open {
                                let _ = sl.eng.cancel_ussd();
                            }
                        }
                        write_ussd(0, 1, "");
                    }
                    (Some("raw2"), Some(cmd), data) => {
                        let bytes: Vec<u8> = data.unwrap_or("").as_bytes().chunks(2).filter_map(|c| u8::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok()).collect();
                        if let (Some(sl), Ok(cmd), Some((ty, rest))) = (s2.as_mut(), u16::from_str_radix(cmd, 16), bytes.split_first()) {
                            let _ = sl.eng.send(cmd, *ty, rest);
                        }
                    }
                    _ => {}
                }
            }
            // Aktiver Steckplatz (Netz/Daten in der Oberfläche): erste bereite SIM, sonst die mit Anmeldung, sonst SIM 1
            let ready = |sl: &Slot| sim_state(sl.eng.st.sim) == radio::SimState::Ready;
            let want_slot = radio::read_prefs().data_sim;
            let act_no: u8 = if want_slot == 2 && s2.as_ref().is_some_and(ready) {
                2
            } else if want_slot == 1 && ready(&s1) {
                1
            } else if ready(&s1) {
                1
            } else if s2.as_ref().is_some_and(ready) {
                2
            } else if s2.as_ref().is_some_and(|sl| sl.eng.st.sim.is_some() && sl.eng.st.sim != Some(0x80)) && s1.eng.st.sim == Some(0x80) {
                2
            } else {
                1
            };
            // Mobile Daten wie der Samsung-RIL: erst bei bereiter SIM + Paketdienst-Anmeldung den Kontext aufbauen
            if data_slot != 0 && data_slot != act_no {
                let old = if data_slot == 2 { s2.as_mut().unwrap_or(&mut s1) } else { &mut s1 };
                let _ = old.eng.data_disconnect();
                let _ = ultar_ril::pdp::set_iface_down(&ultar_ril::pdp::ifname_for_cid(ultar_ril::engine::DATA_CID));
                (data_slot, data_req, data_cfg) = (0, None, None);
            }
            let act = if act_no == 2 { s2.as_mut().unwrap_or(&mut s1) } else { &mut s1 };
            let eng = &mut act.eng;
            let want_data = std::path::Path::new(MOBILE_DATA_FLAG).exists() && on;
            let can_data = eng.st.phone_state == 2
                && sim_state(eng.st.sim) == radio::SimState::Ready
                && matches!(eng.st.reg_ps.status, 2 | 6);
            if want_data && can_data {
                if eng.st.data_ip.is_none() && data_req.map_or(true, |t| t.elapsed() >= Duration::from_secs(30)) {
                    let a = read_apn();
                    let _ = eng.data_connect(&a.apn, a.user.as_deref(), a.pass.as_deref());
                    data_req = Some(Instant::now());
                    data_slot = act_no;
                    note = format!("Mobile Daten werden verbunden (SIM {act_no}) …");
                }
                if let Some(c) = eng.st.data_ip.clone() {
                    if c.fail_cause != 0 {
                        note = format!("Mobile Daten abgelehnt (Grund {})", c.fail_cause);
                        eng.st.data_ip = None;
                    } else if data_cfg != Some(c.ip) {
                        let ifname = ultar_ril::pdp::ifname_for_cid(c.cid.max(1));
                        let dns: Vec<_> = [c.dns1, c.dns2].into_iter().filter(|d| !d.is_unspecified()).collect();
                        let mask = if c.mask.is_unspecified() { std::net::Ipv4Addr::new(255, 255, 255, 0) } else { c.mask };
                        let s = ultar_ril::pdp::DataSession { cid: c.cid, ifname: ifname.clone(), ip: c.ip, mask, gateway: c.gateway, dns };
                        note = match ultar_ril::pdp::configure_iface(&s) {
                            Ok(_) => {
                                data_cfg = Some(c.ip);
                                format!("Mobile Daten: {ifname} {}", c.ip)
                            }
                            Err(e) => format!("Mobile Daten: {ifname} nicht einrichtbar ({e})"),
                        };
                    }
                }
            } else if !want_data && (data_req.is_some() || data_cfg.is_some()) {
                let _ = eng.data_disconnect();
                let _ = ultar_ril::pdp::set_iface_down(&ultar_ril::pdp::ifname_for_cid(ultar_ril::engine::DATA_CID));
                (data_slot, data_req, data_cfg) = (0, None, None);
                note.clear();
            } else if want_data && !can_data && data_cfg.is_some() {
                // Anmeldung verloren: neu aufbauen, sobald wieder möglich
                data_cfg = None;
                data_req = None;
            }
            // SMS-Posteingang und Anrufzustand für die Oberfläche
            for (i, sl) in std::iter::once(&mut s1).chain(s2.as_mut()).enumerate() {
                for m in sl.eng.inbox.drain(..) {
                    sms_join.push((i as u8 + 1, m, Instant::now()));
                }
            }
            deliver_sms(&mut sms_join);
            for (i, sl) in std::iter::once(&mut s1).chain(s2.as_mut()).enumerate() {
                for (state, text) in sl.eng.ussd.drain(..) {
                    write_ussd(i as u8 + 1, state, &text);
                }
            }
            let cur_call = std::iter::once(&s1).chain(s2.as_ref()).enumerate().find_map(|(i, sl)| sl.eng.st.calls.first().map(|c| (i as u8 + 1, c.clone())));
            if cur_call != last_call {
                write_call(cur_call.as_ref());
                last_call = cur_call;
            }
            // Gesprächston: Modem-Takt und -Tonweg schalten, ultar-audio verbindet Modem und Tonchip im ABOX
            let talking = std::iter::once(&s1).chain(s2.as_ref()).enumerate().find_map(|(i, sl)| {
                sl.eng.st.calls.iter().any(|c| matches!(c.status, CallStatus::Active | CallStatus::Dialing | CallStatus::Alerting | CallStatus::Holding)).then_some(i as u8 + 1)
            });
            for (slot, cmd, ty, data) in voice.step(talking) {
                let sl = if slot == 2 { s2.as_mut().unwrap_or(&mut s1) } else { &mut s1 };
                let _ = sl.eng.send(cmd, ty, &data);
            }
            if last_call.is_some() && last_list.elapsed() >= Duration::from_secs(2) {
                last_list = Instant::now();
                for sl in std::iter::once(&mut s1).chain(s2.as_mut()) {
                    if !sl.eng.st.calls.is_empty() {
                        let _ = sl.eng.send(ultar_ril::tel::CMD_CALL_LIST, ultar_ril::ipc::IPC_TYPE_GET, &[]);
                    }
                }
            }
            if last_query.elapsed() >= POLL * 2 {
                last_query = Instant::now();
                for sl in std::iter::once(&mut s1).chain(s2.as_mut()) {
                    if sl.eng.st.powered {
                        let _ = sl.eng.query();
                    }
                }
            }
            flush_log(&mut s1.eng.log);
            if let Some(sl) = s2.as_mut() {
                let mut l: Vec<String> = sl.eng.log.drain(..).map(|x| format!("[SIM2] {x}")).collect();
                flush_log(&mut l);
            }
            let st2 = s2.as_ref().map(|sl| sl.eng.st.clone()).unwrap_or_default();
            let cur = (State { rx: 0, ..s1.eng.st.clone() }, State { rx: 0, ..st2.clone() });
            if cur != last_state || last_write.elapsed() >= POLL {
                last_state = cur;
                last_write = Instant::now();
                let act_st = if act_no == 2 { &st2 } else { &s1.eng.st };
                let none = |st: &State| st.sim == Some(0x80) || st.sim.is_none();
                let n = if act_st.phone_state == 1 {
                    "Funk aus (Flugmodus)".to_string()
                } else if none(&s1.eng.st) && (!has2 || none(&st2)) && note.is_empty() {
                    no_sim_note()
                } else {
                    note.clone()
                };
                let mut rs = to_radio(act_st, &n);
                rs.sims.clear();
                let op = |st: &State| if sim_state(st.sim) == radio::SimState::Ready && !st.plmn.is_empty() { operator_name(&st.plmn) } else { String::new() };
                rs.sims.push(radio::Sim { slot: 1, state: sim_state(s1.eng.st.sim), operator: op(&s1.eng.st), number: String::new() });
                if has2 {
                    rs.sims.push(radio::Sim { slot: 2, state: sim_state(st2.sim), operator: op(&st2), number: String::new() });
                }
                if rs.note.is_empty() {
                    rs.note = sim_summary(&rs.sims);
                }
                auto_sim_roles(&rs.sims);
                let _ = radio::write_state(&rs);
            }
        }
        std::thread::sleep(RETRY);
    }
}

/// Steckplatz für Anruf/SMS: ausdrücklich angegeben, sonst Vorgabe, sonst die bereite SIM.
fn tel_slot(arg: Option<&str>, pref: u8, s1: &Slot, s2: Option<&Slot>) -> u8 {
    let ready = |sl: &Slot| sim_state(sl.eng.st.sim) == radio::SimState::Ready;
    match arg.and_then(|a| a.parse::<u8>().ok()) {
        Some(n @ 1..=2) => n,
        _ if pref == 2 && s2.is_some_and(ready) => 2,
        _ if pref == 1 && ready(s1) => 1,
        _ if s2.is_some_and(ready) && !ready(s1) => 2,
        _ => 1,
    }
}

/// SMSC (Kurzmitteilungszentrale) aus /data/ultar/apn.conf `smsc=+963…`, als Adressfeld (Länge, Typ, BCD);
/// fehlt sie, leer (Modem nimmt die der SIM).
fn read_smsc() -> Vec<u8> {
    let txt = std::fs::read_to_string(APN_CONF).unwrap_or_default();
    let Some(n) = txt.lines().filter_map(|l| l.split_once('=')).find(|(k, _)| k.trim() == "smsc").map(|(_, v)| v.trim().to_string()) else {
        return Vec::new();
    };
    let d: Vec<u8> = n.bytes().filter(|b| b.is_ascii_digit()).map(|b| b - b'0').collect();
    if d.is_empty() {
        return Vec::new();
    }
    let mut v = vec![(1 + d.len().div_ceil(2)) as u8, if n.starts_with('+') { 0x91 } else { 0x81 }];
    for c in d.chunks(2) {
        v.push((c.get(1).copied().unwrap_or(0x0f) << 4) | c[0]);
    }
    v
}

pub const USSD_FILE: &str = "/run/ultar/radio/ussd";

/// USSD-Zustand für die Oberfläche: `state=` (9 = gesendet/wartet, 1 fertig, 2 Antwort erwartet, 3–6 Ende/Fehler),
/// `slot=`, `text=` (Hex von UTF-8). Antworten des Netzes erscheinen zusätzlich als Benachrichtigung.
fn write_ussd(slot: u8, state: u8, text: &str) {
    use std::io::Write;
    let t = format!("state={state}\nslot={slot}\ntext={}\n", ultar_ril::tel::hex(text));
    let tmp = format!("{USSD_FILE}.tmp");
    if std::fs::write(&tmp, t).is_ok() {
        let _ = std::fs::rename(&tmp, USSD_FILE);
    }
    if !text.is_empty() {
        let clean = text.replace(['\t', '\n', '\r'], " ");
        let line = format!("{}\tnormal\tsystem\tphone\tUSSD\t{}\n", now(), clean);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/run/ultar/notify.log") {
            let _ = f.write_all(line.as_bytes());
        }
    } else {
        wake_shell();
    }
}

pub const CALL_FILE: &str = "/run/ultar/radio/call";
pub const SMS_INBOX: &str = "/data/ultar/sms-in.tsv";

/// Anrufzustand für die Oberfläche (`state=idle|incoming|dialing|active`, `number=`, `slot=`).
/// Klingelt es, wird die Oberfläche über die Benachrichtigungsdatei geweckt (auch bei Bildschirm aus).
fn write_call(c: Option<&(u8, ultar_ril::tel::CallEntry)>) {
    let txt = match c {
        Some((slot, e)) => format!("state={}\nnumber={}\nslot={}\nid={}\n", e.status.key(), e.number, slot, e.id),
        None => "state=idle\nnumber=\nslot=\nid=\n".to_string(),
    };
    let tmp = format!("{CALL_FILE}.tmp");
    if std::fs::write(&tmp, txt).is_ok() {
        let _ = std::fs::rename(&tmp, CALL_FILE);
    }
    if matches!(c, Some((_, e)) if e.status.key() == "incoming") {
        wake_shell();
    }
}

/// Oberfläche wecken, ohne eine sichtbare Benachrichtigung zu schreiben: Änderungszeit der Datei setzen.
fn wake_shell() {
    let p = "/run/ultar/notify.log";
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
}

/// Teile langer SMS zusammensetzen (höchstens 2 min warten), dann in den Posteingang + Benachrichtigung.
fn deliver_sms(q: &mut Vec<(u8, ultar_ril::tel::InSms, Instant)>) {
    use std::io::Write;
    let mut done: Vec<(u8, String, String)> = Vec::new();
    let mut keep = Vec::new();
    let mut taken = vec![false; q.len()];
    for i in 0..q.len() {
        if taken[i] {
            continue;
        }
        let (slot, m, t) = &q[i];
        match m.part {
            None => {
                taken[i] = true;
                done.push((*slot, m.from.clone(), m.text.clone()));
            }
            Some((r, total, _)) => {
                let idx: Vec<usize> = (0..q.len()).filter(|&j| !taken[j] && q[j].1.from == m.from && q[j].1.part.map(|p| p.0) == Some(r)).collect();
                if idx.len() >= total as usize || t.elapsed() >= Duration::from_secs(120) {
                    let mut parts: Vec<&ultar_ril::tel::InSms> = idx.iter().map(|&j| &q[j].1).collect();
                    parts.sort_by_key(|p| p.part.map(|x| x.2).unwrap_or(0));
                    done.push((*slot, m.from.clone(), parts.iter().map(|p| p.text.as_str()).collect()));
                    for j in idx {
                        taken[j] = true;
                    }
                }
            }
        }
    }
    for (j, e) in q.drain(..).enumerate() {
        if !taken[j] {
            keep.push(e);
        }
    }
    *q = keep;
    for (slot, from, text) in done {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(SMS_INBOX) {
            let _ = writeln!(f, "{}\t{}\t{}\t{}", now(), slot, from, ultar_ril::tel::hex(&text));
        }
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        let line = format!("{}\tnormal\tsms\tmessages\t{}\t{}\n", now(), clean(&from), clean(&text));
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/run/ultar/notify.log") {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

/// Kurzer Hinweis, welche SIM wo steckt (z. B. „SIM 2: Syriatel · Steckplatz 1 leer“).
fn sim_summary(sims: &[radio::Sim]) -> String {
    let parts: Vec<String> = sims
        .iter()
        .map(|s| match s.state {
            radio::SimState::Ready if !s.operator.is_empty() => format!("SIM {}: {}", s.slot, s.operator),
            radio::SimState::Ready => format!("SIM {}: bereit", s.slot),
            radio::SimState::Absent => format!("Steckplatz {} leer", s.slot),
            radio::SimState::Pin | radio::SimState::Puk => format!("SIM {}: PIN nötig", s.slot),
            radio::SimState::Locked => format!("SIM {}: gesperrt", s.slot),
            radio::SimState::Error => format!("SIM {}: Kartenfehler", s.slot),
            radio::SimState::Unknown => format!("SIM {}: wird geprüft", s.slot),
        })
        .collect();
    parts.join(" · ")
}

/// Wie Android: Ist genau eine SIM bereit, laufen Daten, Anrufe und SMS automatisch über sie.
/// Bei zwei bereiten SIMs bleibt die Wahl des Nutzers; zeigt eine Rolle auf einen leeren Steckplatz,
/// wird sie auf die bereite SIM umgestellt.
fn auto_sim_roles(sims: &[radio::Sim]) {
    let ready: Vec<u8> = sims.iter().filter(|s| s.state == radio::SimState::Ready).map(|s| s.slot).collect();
    if ready.is_empty() {
        return;
    }
    let mut p = radio::read_prefs();
    let fix = |v: u8, ask_ok: bool| -> u8 {
        if ready.len() == 1 {
            ready[0]
        } else if (v == 0 && ask_ok) || ready.contains(&v) {
            v
        } else {
            ready[0]
        }
    };
    let n = radio::Prefs { data_sim: fix(p.data_sim, false), call_sim: fix(p.call_sim, true), sms_sim: fix(p.sms_sim, true), ..p.clone() };
    if (n.data_sim, n.call_sim, n.sms_sim) != (p.data_sim, p.call_sim, p.sms_sim) {
        p = n;
        let _ = radio::write_prefs(&p);
        eprintln!("ultar-ril: SIM-Rollen automatisch: Daten SIM {}, Anrufe SIM {}, SMS SIM {}", p.data_sim, p.call_sim, p.sms_sim);
    }
}

/// Ein SIM-Steckplatz mit eigenem FMT-Kanal und eigener Ablaufsteuerung.
struct Slot {
    eng: Engine<UmtsLink>,
    want_radio: Option<bool>,
    asked_imei: bool,
    opened: Instant,
}

impl Slot {
    fn new(eng: Engine<UmtsLink>) -> Slot {
        Slot { eng, want_radio: None, asked_imei: false, opened: Instant::now() }
    }

    /// Einschalten erkennen, IMEI holen, Funk an/aus nach Flugmodus.
    fn housekeeping(&mut self, on: bool) {
        let eng = &mut self.eng;
        // Lief das Modem schon (Dienst neu gestartet), kommt keine Einschaltmeldung mehr: nach 2 s davon ausgehen
        if !eng.st.powered && self.opened.elapsed() >= Duration::from_secs(2) {
            eng.st.powered = true;
            let _ = eng.send(ultar_ril::engine::CMD_PWR_PHONE_STATE, ultar_ril::ipc::IPC_TYPE_GET, &[]);
        }
        if eng.st.powered && eng.st.imei.is_empty() && !self.asked_imei {
            self.asked_imei = true;
            let _ = eng.send(ultar_ril::engine::CMD_MISC_ME_SN, ultar_ril::ipc::IPC_TYPE_GET, &[0x01]);
        }
        if eng.st.powered && self.want_radio != Some(on) {
            self.want_radio = Some(on);
            let _ = eng.set_radio(on);
        }
    }
}

// ---- Gesprächston ----------------------------------------------------------------------------
// Wie Samsungs RIL/Audio-HAL: SND_CLOCK_CTRL (0x0909, EXEC) startet den Sprach-Takt des Modems,
// SND_AUDIO_PATH_CTRL (0x0903, SET [weg, 0]) wählt Hörer/Lautsprecher/Kopfhörer, SND_SPKR_VOLUME_CTRL
// (0x0901, SET [art, stufe]) die Lautstärke. Den ABOX-Teil (Basisband-Strecken + Codec-Wege aus
// mixer_paths.xml) erledigt `ultar-audio --call`, gesteuert über /run/ultar/callaudio.

const CALL_AUDIO: &str = "/run/ultar/callaudio";
const SND_SPKR_VOLUME: u16 = 0x0901;
const SND_AUDIO_PATH: u16 = 0x0903;
const SND_CLOCK: u16 = 0x0909;

#[derive(Default)]
struct CallAudio {
    slot: u8,
    dev: String,
    child: Option<std::process::Child>,
}

fn headset_plugged() -> bool {
    std::fs::read_to_string("/sys/class/audio/earjack/state").is_ok_and(|s| !matches!(s.trim(), "" | "0"))
}

/// Gewünschter Weg: Datei der Oberfläche, „auto" = Kopfhörer falls eingesteckt, sonst Hörmuschel.
fn call_dev() -> String {
    let s = std::fs::read_to_string(CALL_AUDIO).unwrap_or_default();
    match s.trim() {
        "speaker" => "speaker".into(),
        "handset" => "handset".into(),
        "headset" | "headphone" => "headset".into(),
        _ if headset_plugged() => "headset".into(),
        _ => "handset".into(),
    }
}

/// (Modem-Weg, Lautstärke-Art) – Werte wie libsamsung-ipc: Hörer 1/0x01, Kopfhörer 2/0x31, Lautsprecher 6/0x11
fn modem_route(dev: &str) -> (u8, u8) {
    match dev {
        "speaker" => (6, 0x11),
        "headset" => (2, 0x31),
        _ => (1, 0x01),
    }
}

impl CallAudio {
    /// Liefert die an das Modem zu sendenden Befehle (Steckplatz, Befehl, Typ, Daten).
    fn step(&mut self, talking: Option<u8>) -> Vec<(u8, u16, u8, Vec<u8>)> {
        use ultar_ril::ipc::IPC_TYPE_EXEC;
        let mut out = Vec::new();
        if let Some(c) = self.child.as_mut() {
            if !matches!(c.try_wait(), Ok(None)) {
                self.child = None;
            }
        }
        match talking {
            Some(slot) => {
                if self.slot == 0 {
                    let cur = std::fs::read_to_string(CALL_AUDIO).unwrap_or_default();
                    if matches!(cur.trim(), "" | "off") {
                        let _ = std::fs::write(CALL_AUDIO, "auto");
                    }
                    self.slot = slot;
                    self.dev.clear();
                    out.push((slot, SND_CLOCK, IPC_TYPE_EXEC, vec![1]));
                }
                if self.child.is_none() {
                    let log = std::fs::OpenOptions::new().create(true).append(true).open("/run/ultar/log/callaudio.log").ok();
                    let mut cmd = std::process::Command::new("/usr/bin/ultar-audio");
                    cmd.arg("--call").stdin(std::process::Stdio::null());
                    if let Some(f) = log {
                        if let Ok(f2) = f.try_clone() {
                            cmd.stdout(f).stderr(f2);
                        }
                    }
                    self.child = cmd.spawn().ok();
                }
                let dev = call_dev();
                if dev != self.dev {
                    let (path, vty) = modem_route(&dev);
                    out.push((self.slot, SND_AUDIO_PATH, IPC_TYPE_SET, vec![path, 0]));
                    out.push((self.slot, SND_SPKR_VOLUME, IPC_TYPE_SET, vec![vty, 4]));
                    self.dev = dev;
                }
            }
            None if self.slot != 0 => {
                let _ = std::fs::write(CALL_AUDIO, "off");
                out.push((self.slot, SND_CLOCK, IPC_TYPE_EXEC, vec![0]));
                self.slot = 0;
                self.dev.clear();
            }
            None => {}
        }
        out
    }
}
