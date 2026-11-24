//! RIL-3: Mobile Daten (PDP-Kontext → rmnet) für das Shannon-Modem (A30 / s327ap, modem_v1).
//!
//! Baut auf der Transportschicht (Subpaket RIL-1, `sipc5`/`UmtsLink`) über das Trait `ModemLink`
//! auf und spricht auf dem FMT-Kanal (235) das Samsung-IPC-Protokoll (Gruppe GPRS = 0x0D).
//! Nach erfolgreicher PDP-Aktivierung liefert das Modem IP/DNS/Gateway/Maske; der Datenverkehr
//! läuft über die rmnet-Schnittstelle, deren Nummer der PDP-Kanal-ID folgt:
//!   rmnet0 = Kanal 10 (SIPC_CH_ID_PDP_0) … rmnet7 = Kanal 17   (aus modem-s327ap-sipc-pdata.dtsi)
//! Der PDP-Kontext bekommt eine cid (1..n); cid N ist konventionell rmnet(N-1).
//!
//! HINWEIS für den Koordinator: Das Trait `ModemLink`/`SipcFrame` ist hier lokal definiert, damit
//! die Datei eigenständig kompiliert, solange RIL-1 noch im Bau ist. Beim Zusammenführen das lokale
//! `mod link` entfernen und `use crate::sipc5::{ModemLink, SipcFrame};` (bzw. den finalen Pfad von
//! RIL-1) verwenden. Die Message-Kodierung (`fmt`-Modul) sollte mit RIL-2 (`ipc.rs`) vereinheitlicht
//! werden; sie ist hier bewusst minimal und nur für die GPRS-Befehle enthalten.

use std::io;
use std::net::Ipv4Addr;

// ------------------------------------------------------------------------------------------------
// Transport-Schnittstelle (identisch zu RIL-1; lokal, bis zusammengeführt wird)
// ------------------------------------------------------------------------------------------------
pub use crate::link::{ModemLink, SipcFrame};

// ------------------------------------------------------------------------------------------------
// Kanäle
// ------------------------------------------------------------------------------------------------
/// FMT-Kanal (Steuerbefehle, umts_ipc0).
pub const CH_FMT: u8 = 235;
/// Erster PDP-Datenkanal (rmnet0).
pub const CH_PDP_0: u8 = 10;
/// Anzahl der PDP/rmnet-Kanäle (10..=17).
pub const PDP_COUNT: u8 = 8;

/// rmnet-Name für eine PDP-cid (cid 1 → rmnet0 → Kanal 10).
pub fn ifname_for_cid(cid: u8) -> String {
    format!("rmnet{}", cid.saturating_sub(1))
}
/// Datenkanal (SIPC-Kanal-ID) für eine cid (cid 1 → Kanal 10).
pub fn channel_for_cid(cid: u8) -> u8 {
    CH_PDP_0 + cid.saturating_sub(1)
}

// ------------------------------------------------------------------------------------------------
// Samsung-IPC FMT-Nachricht (Gruppe/Index/Typ). Minimal für GPRS.
// ------------------------------------------------------------------------------------------------
pub mod fmt {
    /// IPC-Gruppen (hier nur GPRS).
    pub const GROUP_GPRS: u8 = 0x0D;
    // Index innerhalb GPRS (= niederwertiges Byte der libsamsung-ipc IPC_GPRS_* IDs)
    pub const IDX_DEFINE_PDP_CONTEXT: u8 = 0x01; // 0x0D01
    pub const IDX_PDP_CONTEXT: u8 = 0x04; // 0x0D04 (Aktivieren/Deaktivieren)
    pub const IDX_IP_CONFIGURATION: u8 = 0x09; // 0x0D09
    pub const IDX_CALL_STATUS: u8 = 0x10; // 0x0D10

    // IPC-Nachrichtentypen
    pub const TYPE_EXEC: u8 = 0x01;
    pub const TYPE_GET: u8 = 0x02;
    pub const TYPE_SET: u8 = 0x03;
    pub const TYPE_CFRM: u8 = 0x04; // Bestätigung/Antwort auf GET/SET
    pub const TYPE_EVENT: u8 = 0x05; // unaufgefordertes Ereignis (Notification)

    /// FMT-Header (libsamsung-ipc): len(2, LE, inkl. Header) | mseq | aseq | group | index | type.
    pub const HDR_LEN: usize = 7;

    /// Baut eine komplette FMT-Nachricht (Header + Body). `mseq` = eigene Sequenznummer.
    pub fn message(mseq: u8, group: u8, index: u8, mtype: u8, body: &[u8]) -> Vec<u8> {
        let total = (HDR_LEN + body.len()) as u16;
        let mut m = Vec::with_capacity(total as usize);
        m.extend_from_slice(&total.to_le_bytes());
        m.push(mseq);
        m.push(0); // aseq (Antwort-Sequenz; 0 bei Anfrage)
        m.push(group);
        m.push(index);
        m.push(mtype);
        m.extend_from_slice(body);
        m
    }

    /// Zerlegt eine empfangene FMT-Nachricht in (group, index, type, body). None bei zu kurz.
    pub fn parse(buf: &[u8]) -> Option<(u8, u8, u8, &[u8])> {
        if buf.len() < HDR_LEN {
            return None;
        }
        let len = u16::from_le_bytes([buf[0], buf[1]]) as usize;
        let end = len.min(buf.len());
        // Header: len(2) | mseq(1) | aseq(1) | group(1) | index(1) | type(1)
        Some((buf[4], buf[5], buf[6], &buf[HDR_LEN..end.max(HDR_LEN)]))
    }
}

// ------------------------------------------------------------------------------------------------
// GPRS-Befehlsstrukturen (Layout aus libsamsung-ipc, __packed, little-endian Bytes)
// ------------------------------------------------------------------------------------------------

/// struct ipc_gprs_define_pdp_context_data (127 Byte): enable, cid, magic, apn[124].
pub fn build_define_pdp(cid: u8, apn: &str) -> Vec<u8> {
    let mut b = vec![0u8; 127];
    b[0] = 1; // enable
    b[1] = cid;
    b[2] = 1; // magic (fix)
    let apn_bytes = apn.as_bytes();
    let n = apn_bytes.len().min(123); // 124 Feld, 1 Byte für \0 lassen
    b[3..3 + n].copy_from_slice(&apn_bytes[..n]);
    b
}

/// struct ipc_gprs_pdp_context_request_set_data (103 Byte): enable, cid, magic1[4],
/// username[32], password[32], unknown[32], magic2.
pub fn build_pdp_context(
    cid: u8,
    activate: bool,
    user: Option<&str>,
    pass: Option<&str>,
) -> Vec<u8> {
    let mut b = vec![0u8; 103];
    b[0] = if activate { 1 } else { 0 }; // enable (1=aktivieren, 0=deaktivieren)
    b[1] = cid;
    // magic1[4] (2..6) bleibt 0; username[32] ab 6; password[32] ab 38; unknown[32] ab 70; magic2 @102
    if let Some(u) = user {
        let ub = u.as_bytes();
        let n = ub.len().min(31);
        b[6..6 + n].copy_from_slice(&ub[..n]);
    }
    if let Some(p) = pass {
        let pb = p.as_bytes();
        let n = pb.len().min(31);
        b[38..38 + n].copy_from_slice(&pb[..n]);
    }
    b
}

/// struct ipc_gprs_ip_configuration_data (27 Byte): cid, field_flag, fail_cause,
/// ip[4], dns1[4], dns2[4], gateway[4], subnet_mask[4], unknown[4].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IpConfig {
    pub cid: u8,
    pub fail_cause: u8,
    pub ip: Ipv4Addr,
    pub dns1: Ipv4Addr,
    pub dns2: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub mask: Ipv4Addr,
}

pub fn parse_ip_config(b: &[u8]) -> Option<IpConfig> {
    if b.len() < 27 {
        return None;
    }
    let a = |o: usize| Ipv4Addr::new(b[o], b[o + 1], b[o + 2], b[o + 3]);
    Some(IpConfig {
        cid: b[0],
        fail_cause: b[2],
        ip: a(3),
        dns1: a(7),
        dns2: a(11),
        gateway: a(15),
        mask: a(19),
    })
}

// ------------------------------------------------------------------------------------------------
// Öffentliche API
// ------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataSession {
    pub cid: u8,
    pub ifname: String,
    pub ip: Ipv4Addr,
    pub mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub dns: Vec<Ipv4Addr>,
}

/// Fortlaufende Sequenznummer je Link (einfacher Zähler).
fn next_seq() -> u8 {
    use std::sync::atomic::{AtomicU8, Ordering};
    static S: AtomicU8 = AtomicU8::new(1);
    let v = S.fetch_add(1, Ordering::Relaxed);
    if v == 0 {
        1
    } else {
        v
    }
}

/// Mobile Daten aktivieren: DEFINE_PDP_CONTEXT (APN) → PDP_CONTEXT(enable) → IP_CONFIGURATION lesen.
/// Danach optional die rmnet-Schnittstelle konfigurieren (configure=true).
pub fn activate_data<L: ModemLink>(
    l: &mut L,
    cid: u8,
    apn: &str,
    user: Option<&str>,
    pass: Option<&str>,
    configure: bool,
) -> io::Result<DataSession> {
    // 1) APN definieren
    let m = fmt::message(
        next_seq(),
        fmt::GROUP_GPRS,
        fmt::IDX_DEFINE_PDP_CONTEXT,
        fmt::TYPE_SET,
        &build_define_pdp(cid, apn),
    );
    l.send(CH_FMT, &m)?;
    let _ = wait_cfrm(l, fmt::IDX_DEFINE_PDP_CONTEXT, 3000)?;

    // 2) PDP-Kontext aktivieren
    let m = fmt::message(
        next_seq(),
        fmt::GROUP_GPRS,
        fmt::IDX_PDP_CONTEXT,
        fmt::TYPE_SET,
        &build_pdp_context(cid, true, user, pass),
    );
    l.send(CH_FMT, &m)?;

    // 3) Auf IP-Konfiguration warten (kommt als CFRM auf PDP_CONTEXT oder als eigenes IP_CONFIGURATION/EVENT)
    let cfg = wait_ip_config(l, cid, 15000)?;
    if cfg.fail_cause != 0 {
        return Err(io::Error::other(format!(
            "PDP-Aktivierung abgelehnt (fail_cause={})",
            cfg.fail_cause
        )));
    }
    let mut dns = Vec::new();
    if !cfg.dns1.is_unspecified() {
        dns.push(cfg.dns1);
    }
    if !cfg.dns2.is_unspecified() {
        dns.push(cfg.dns2);
    }
    let sess = DataSession {
        cid,
        ifname: ifname_for_cid(cid),
        ip: cfg.ip,
        mask: if cfg.mask.is_unspecified() {
            Ipv4Addr::new(255, 255, 255, 0)
        } else {
            cfg.mask
        },
        gateway: cfg.gateway,
        dns,
    };
    if configure {
        configure_iface(&sess)?;
    }
    Ok(sess)
}

/// Mobile Daten deaktivieren (PDP_CONTEXT enable=0) und die Schnittstelle herunterfahren.
pub fn deactivate_data<L: ModemLink>(l: &mut L, cid: u8) -> io::Result<()> {
    let m = fmt::message(
        next_seq(),
        fmt::GROUP_GPRS,
        fmt::IDX_PDP_CONTEXT,
        fmt::TYPE_SET,
        &build_pdp_context(cid, false, None, None),
    );
    l.send(CH_FMT, &m)?;
    let _ = wait_cfrm(l, fmt::IDX_PDP_CONTEXT, 5000);
    let _ = set_iface_down(&ifname_for_cid(cid));
    Ok(())
}

/// Vorhandene Datenschnittstellen (rmnet*) auflisten.
pub fn list_data_ifaces() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir("/sys/class/net")
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("rmnet"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

// ------------------------------------------------------------------------------------------------
// Empfangs-Hilfen
// ------------------------------------------------------------------------------------------------

/// Wartet auf eine Bestätigung (CFRM) zu einem Index auf dem FMT-Kanal.
fn wait_cfrm<L: ModemLink>(l: &mut L, index: u8, timeout_ms: u32) -> io::Result<Vec<u8>> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "keine Antwort vom Modem",
            ));
        }
        if let Some(f) = l.recv(left.as_millis() as u32)? {
            if f.channel == CH_FMT {
                if let Some((g, i, t, body)) = fmt::parse(&f.payload) {
                    if g == fmt::GROUP_GPRS
                        && i == index
                        && (t == fmt::TYPE_CFRM || t == fmt::TYPE_EVENT)
                    {
                        return Ok(body.to_vec());
                    }
                }
            }
        }
    }
}

/// Wartet auf die IP-Konfiguration zur cid (als IP_CONFIGURATION oder im PDP_CONTEXT-CFRM).
fn wait_ip_config<L: ModemLink>(l: &mut L, cid: u8, timeout_ms: u32) -> io::Result<IpConfig> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "keine IP-Konfiguration vom Modem",
            ));
        }
        if let Some(f) = l.recv(left.as_millis() as u32)? {
            if f.channel != CH_FMT {
                continue;
            }
            let Some((g, i, _t, body)) = fmt::parse(&f.payload) else {
                continue;
            };
            if g != fmt::GROUP_GPRS {
                continue;
            }
            if i == fmt::IDX_IP_CONFIGURATION {
                if let Some(c) = parse_ip_config(body) {
                    if c.cid == cid {
                        return Ok(c);
                    }
                }
            } else if i == fmt::IDX_PDP_CONTEXT {
                // Manche Firmwares liefern die IP im PDP_CONTEXT-CFRM (gleiches 27-Byte-Layout am Ende)
                if body.len() >= 27 {
                    if let Some(c) = parse_ip_config(&body[body.len() - 27..]) {
                        if c.cid == cid {
                            return Ok(c);
                        }
                    }
                }
            } else if i == fmt::IDX_CALL_STATUS && body.len() >= 3 && body[0] == cid && body[1] == 0
            {
                // status 0 = getrennt/abgelehnt
                return Err(io::Error::other(format!(
                    "PDP getrennt (fail_cause={})",
                    body[2]
                )));
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------
// rmnet-Konfiguration per ioctl (gleicher Stil wie ultar-net)
// ------------------------------------------------------------------------------------------------

pub fn configure_iface(s: &DataSession) -> io::Result<()> {
    set_addr(&s.ifname, libc::SIOCSIFADDR, s.ip)?;
    set_addr(&s.ifname, libc::SIOCSIFNETMASK, s.mask)?;
    set_iface_up_flag(&s.ifname, true)?;
    if !s.gateway.is_unspecified() {
        if let Err(e) = set_default_route(&s.ifname, s.gateway) {
            if e.raw_os_error() != Some(libc::EEXIST) {
                return Err(e);
            }
        }
    }
    if !s.dns.is_empty() {
        let txt: String = s.dns.iter().map(|d| format!("nameserver {d}\n")).collect();
        let _ = std::fs::write("/run/ultar/resolv.conf", &txt);
        let _ = std::fs::write("/etc/resolv.conf", &txt);
    }
    Ok(())
}

pub fn set_iface_down(name: &str) -> io::Result<()> {
    set_iface_up_flag(name, false)
}

fn ctl_sock() -> io::Result<i32> {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

fn ifreq(name: &str) -> libc::ifreq {
    let mut r: libc::ifreq = unsafe { std::mem::zeroed() };
    for (i, b) in name.bytes().take(libc::IFNAMSIZ - 1).enumerate() {
        r.ifr_name[i] = b as libc::c_char;
    }
    r
}

fn sockaddr(ip: Ipv4Addr) -> libc::sockaddr {
    let sin = libc::sockaddr_in {
        sin_family: libc::AF_INET as u16,
        sin_port: 0,
        sin_addr: libc::in_addr {
            s_addr: u32::from(ip).to_be(),
        },
        sin_zero: [0; 8],
    };
    unsafe { std::mem::transmute(sin) }
}

fn set_addr(name: &str, req: libc::Ioctl, ip: Ipv4Addr) -> io::Result<()> {
    let fd = ctl_sock()?;
    let mut r = ifreq(name);
    r.ifr_ifru.ifru_addr = sockaddr(ip);
    let ret = unsafe { libc::ioctl(fd, req, &r) };
    let e = io::Error::last_os_error();
    unsafe { libc::close(fd) };
    if ret < 0 {
        Err(e)
    } else {
        Ok(())
    }
}

fn set_iface_up_flag(name: &str, up: bool) -> io::Result<()> {
    let fd = ctl_sock()?;
    let mut r = ifreq(name);
    unsafe {
        if libc::ioctl(fd, libc::SIOCGIFFLAGS, &mut r) < 0 {
            let e = io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        if up {
            r.ifr_ifru.ifru_flags |= (libc::IFF_UP | libc::IFF_RUNNING) as i16;
        } else {
            r.ifr_ifru.ifru_flags &= !(libc::IFF_UP as i16);
        }
        let ret = libc::ioctl(fd, libc::SIOCSIFFLAGS, &r);
        let e = io::Error::last_os_error();
        libc::close(fd);
        if ret < 0 {
            return Err(e);
        }
    }
    Ok(())
}

fn set_default_route(name: &str, gw: Ipv4Addr) -> io::Result<()> {
    let fd = ctl_sock()?;
    let mut dev: Vec<u8> = name.bytes().collect();
    dev.push(0);
    let mut rt: libc::rtentry = unsafe { std::mem::zeroed() };
    rt.rt_dst = sockaddr(Ipv4Addr::UNSPECIFIED);
    rt.rt_genmask = sockaddr(Ipv4Addr::UNSPECIFIED);
    rt.rt_gateway = sockaddr(gw);
    rt.rt_flags = libc::RTF_UP | libc::RTF_GATEWAY;
    rt.rt_dev = dev.as_mut_ptr() as *mut libc::c_char;
    let ret = unsafe { libc::ioctl(fd, libc::SIOCADDRT, &rt) };
    let e = io::Error::last_os_error();
    unsafe { libc::close(fd) };
    if ret < 0 {
        Err(e)
    } else {
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------------
// Tests (ohne Gerät, über einen Mock-ModemLink)
// ------------------------------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    /// Mock: beantwortet eine PDP-Aktivierung mit einer IP-Konfiguration.
    struct MockLink {
        sent: Vec<(u8, Vec<u8>)>,
        inbox: std::collections::VecDeque<SipcFrame>,
        reply_ip: bool,
    }
    impl ModemLink for MockLink {
        fn send(&mut self, channel: u8, payload: &[u8]) -> io::Result<()> {
            self.sent.push((channel, payload.to_vec()));
            // Auf DEFINE: CFRM; auf PDP_CONTEXT SET: IP_CONFIGURATION liefern
            if let Some((g, i, t, _body)) = fmt::parse(payload) {
                if g == fmt::GROUP_GPRS && i == fmt::IDX_DEFINE_PDP_CONTEXT {
                    let m = fmt::message(0, g, i, fmt::TYPE_CFRM, &[]);
                    self.inbox.push_back(SipcFrame {
                        channel: CH_FMT,
                        payload: m,
                    });
                }
                if g == fmt::GROUP_GPRS
                    && i == fmt::IDX_PDP_CONTEXT
                    && t == fmt::TYPE_SET
                    && self.reply_ip
                {
                    let mut body = vec![0u8; 27];
                    body[0] = 1; // cid
                    body[1] = 0x07; // field_flag
                    body[2] = 0; // fail_cause ok
                    body[3..7].copy_from_slice(&[10, 20, 30, 40]); // ip
                    body[7..11].copy_from_slice(&[8, 8, 8, 8]); // dns1
                    body[11..15].copy_from_slice(&[1, 1, 1, 1]); // dns2
                    body[15..19].copy_from_slice(&[10, 20, 30, 1]); // gateway
                    body[19..23].copy_from_slice(&[255, 255, 255, 0]); // mask
                    let m = fmt::message(0, g, fmt::IDX_IP_CONFIGURATION, fmt::TYPE_CFRM, &body);
                    self.inbox.push_back(SipcFrame {
                        channel: CH_FMT,
                        payload: m,
                    });
                }
            }
            Ok(())
        }
        fn recv(&mut self, _timeout_ms: u32) -> io::Result<Option<SipcFrame>> {
            Ok(self.inbox.pop_front())
        }
    }

    #[test]
    fn channel_mapping() {
        assert_eq!(channel_for_cid(1), 10);
        assert_eq!(ifname_for_cid(1), "rmnet0");
        assert_eq!(channel_for_cid(8), 17);
        assert_eq!(ifname_for_cid(8), "rmnet7");
    }

    #[test]
    fn define_pdp_layout() {
        let b = build_define_pdp(2, "internet");
        assert_eq!(b.len(), 127);
        assert_eq!(b[0], 1); // enable
        assert_eq!(b[1], 2); // cid
        assert_eq!(&b[3..11], b"internet");
    }

    #[test]
    fn pdp_context_layout() {
        let b = build_pdp_context(1, true, Some("user"), Some("pass"));
        assert_eq!(b.len(), 103);
        assert_eq!(b[0], 1);
        assert_eq!(b[1], 1);
        assert_eq!(&b[6..10], b"user");
        assert_eq!(&b[38..42], b"pass");
    }

    #[test]
    fn fmt_roundtrip() {
        let m = fmt::message(
            5,
            fmt::GROUP_GPRS,
            fmt::IDX_PDP_CONTEXT,
            fmt::TYPE_SET,
            &[1, 2, 3],
        );
        let (g, i, t, body) = fmt::parse(&m).unwrap();
        assert_eq!((g, i, t), (0x0D, 0x04, fmt::TYPE_SET));
        assert_eq!(body, &[1, 2, 3]);
    }

    #[test]
    fn ip_config_parse() {
        let mut body = vec![0u8; 27];
        body[0] = 3;
        body[3..7].copy_from_slice(&[100, 64, 1, 2]);
        body[15..19].copy_from_slice(&[100, 64, 1, 1]);
        let c = parse_ip_config(&body).unwrap();
        assert_eq!(c.cid, 3);
        assert_eq!(c.ip, Ipv4Addr::new(100, 64, 1, 2));
        assert_eq!(c.gateway, Ipv4Addr::new(100, 64, 1, 1));
    }

    #[test]
    fn activate_flow_without_configure() {
        let mut l = MockLink {
            sent: Vec::new(),
            inbox: Default::default(),
            reply_ip: true,
        };
        let s = activate_data(&mut l, 1, "internet", None, None, false).unwrap();
        assert_eq!(s.ifname, "rmnet0");
        assert_eq!(s.ip, Ipv4Addr::new(10, 20, 30, 40));
        assert_eq!(s.gateway, Ipv4Addr::new(10, 20, 30, 1));
        assert_eq!(
            s.dns,
            vec![Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(1, 1, 1, 1)]
        );
        // zwei Sendungen: DEFINE + PDP_CONTEXT
        assert_eq!(l.sent.len(), 2);
    }
}
