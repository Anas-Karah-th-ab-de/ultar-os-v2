//! The Wi-Fi side of the service: the worker started with the service, the
//! `net.*` Wi-Fi calls (answered by the worker when it is done, so the
//! service loop never waits), `net.wifi`, and the DNS file.

use crate::dns;
use crate::service::NetService;
use crate::wifi::core::{Command, Reply, ScanAnswer, Seen};
use crate::wifi::driver::{LinuxIp, WifiDriver};
use crate::wifi::saved::Saved;
use crate::wifi::{worker, WifiCore};
use serde::Serialize;
use std::path::PathBuf;
use ultar_bus::messages::net::{
    Connect, Disconnect, Forget, Join, JoinOutcome, JoinReply, Known, KnownReply, NetOutcome,
    NetReply, Network, NetworkInfo, Networks, NetworksReply, Scan, ScanReply, WifiLink, WifiPhase,
    WifiSecurity,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_hal_netlink::nl80211::Nl80211;
use ultar_netproto::ie::Security;
use ultar_service::log::{Level, Record};
use ultar_svckit::Bus;
use zeroize::Zeroizing;

/// The service's view of Wi-Fi.
pub struct WifiSide {
    saved: PathBuf,
    /// Where `resolv.conf` goes.
    pub run_dir: Option<PathBuf>,
    handle: Option<worker::WifiHandle>,
    /// The connection as last reported.
    pub link: WifiLink,
    radio_on: Option<bool>,
    vpn_dns: Vec<String>,
    wired_dns: Vec<String>,
    /// The running kernel's nl80211 is this service's device (see [`WifiSide::on_kernel`]).
    kernel: bool,
}

impl WifiSide {
    /// Saved networks in `saved`, DNS below `run_dir`.
    pub fn new(saved: PathBuf, run_dir: Option<PathBuf>) -> WifiSide {
        WifiSide {
            kernel: true,
            saved,
            run_dir,
            handle: None,
            link: WifiLink::default(),
            radio_on: None,
            vpn_dns: Vec::new(),
            wired_dns: Vec::new(),
        }
    }

    /// Whether the running kernel's nl80211 belongs to this service. A service started on a
    /// prepared tree (`--root` other than `/`) sees only that tree: the machine's own Wi-Fi
    /// card is not part of it and is neither listed nor switched.
    pub fn on_kernel(mut self, kernel: bool) -> WifiSide {
        self.kernel = kernel;
        self
    }

    /// A Wi-Fi interface is driven.
    pub fn present(&self) -> bool {
        self.link.phase != WifiPhase::NoDevice
    }

    /// Starts the worker (with nl80211 when the kernel has it).
    pub fn start(&mut self, bus: &Bus<'_, NetService>) {
        let driver = self
            .kernel
            .then(|| Nl80211::open().ok())
            .flatten()
            .map(|n| Box::new(n) as Box<dyn WifiDriver>);
        let core = WifiCore::new(
            driver,
            Box::new(LinuxIp::default()),
            Saved::load(self.saved.clone()),
            Box::new(|b| getrandom::getrandom(b).is_ok()),
        );
        let poster = bus.poster();
        let changed = Box::new(move |link: WifiLink| {
            let _ = poster.post(Box::new(move |svc: &mut NetService, bus| {
                svc.wifi_changed(link, bus)
            }));
        });
        match worker::start(core, changed) {
            Ok(h) => self.handle = Some(h),
            Err(_) => bus.log(Record::new(Level::Warn, "netd.wifi_not_started")),
        }
    }

    /// The radio switch as the kernel shows it.
    pub fn radio(&mut self, on: bool) {
        if !self.present() || self.radio_on == Some(on) {
            return;
        }
        self.radio_on = Some(on);
        self.send(Command::Radio(on));
    }

    fn send(&self, command: Command) {
        match &self.handle {
            Some(h) => h.send(command),
            None => command.refuse(),
        }
    }

    /// The VPN's DNS servers changed.
    pub fn vpn_dns(&mut self, dns: Vec<String>, bus: &Bus<'_, NetService>) {
        if dns != self.vpn_dns {
            self.vpn_dns = dns;
            self.write_dns(bus);
        }
    }

    /// The name servers of the wired interfaces with an address changed.
    pub fn wired_dns(&mut self, dns: Vec<String>, bus: &Bus<'_, NetService>) {
        if dns != self.wired_dns {
            self.wired_dns = dns;
            self.write_dns(bus);
        }
    }

    fn write_dns(&self, bus: &Bus<'_, NetService>) {
        let Some(dir) = &self.run_dir else { return };
        // the VPN first, then the network in use: Wi-Fi's lease, then the wired ones
        let lease: Vec<String> = self
            .link
            .dns
            .iter()
            .chain(&self.wired_dns)
            .cloned()
            .collect();
        let servers = dns::servers(&self.vpn_dns, &lease);
        if dns::write(&dir.join("resolv.conf"), &servers).is_err() {
            bus.log(Record::new(Level::Warn, "netd.dns_not_written"));
        }
    }
}

impl NetService {
    /// The worker reported a new connection state.
    pub(crate) fn wifi_changed(&mut self, link: WifiLink, bus: &Bus<'_, Self>) {
        let dns_changed = link.dns != self.wifi.link.dns;
        if link.phase != self.wifi.link.phase {
            bus.log(Record::new(Level::Info, "netd.wifi").word("phase", phase_word(link.phase)));
        }
        bus.retain(&link);
        self.wifi.link = link;
        if dns_changed {
            self.wifi.write_dns(bus);
        }
        self.publish(bus);
    }
}

/// A fixed word per phase (log records carry no free text).
pub fn phase_word(p: WifiPhase) -> &'static str {
    match p {
        WifiPhase::NoDevice => "no_device",
        WifiPhase::Off => "off",
        WifiPhase::Disconnected => "disconnected",
        WifiPhase::Scanning => "scanning",
        WifiPhase::Associating => "associating",
        WifiPhase::Authenticating => "authenticating",
        WifiPhase::ObtainingAddress => "obtaining_address",
        WifiPhase::Connected => "connected",
    }
}

fn security(s: Security) -> WifiSecurity {
    match s {
        Security::Open => WifiSecurity::Open,
        Security::Wpa2Personal => WifiSecurity::Wpa2Personal,
        Security::Wpa2Wpa3Personal => WifiSecurity::Wpa2Wpa3Personal,
        Security::Wpa3Personal => WifiSecurity::Wpa3Personal,
        Security::Enterprise => WifiSecurity::Enterprise,
        Security::Legacy => WifiSecurity::Legacy,
        Security::Other => WifiSecurity::Other,
    }
}

fn info(s: &Seen) -> NetworkInfo {
    let b = s.bss.bssid;
    NetworkInfo {
        ssid: String::from_utf8_lossy(&s.ssid).into_owned(),
        bssid: format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5]
        ),
        signal_percent: WifiCore::signal_of(s),
        frequency_mhz: s.bss.freq_mhz,
        security: security(s.security),
        known: s.known,
        joinable: s.security.joinable(),
        connected: s.connected,
    }
}

/// `net.scan`: one entry per name (the strongest), hidden networks left out.
pub fn by_name(list: &[Seen]) -> Vec<Network> {
    let mut out: Vec<Network> = Vec::new();
    for s in list.iter().filter(|s| !s.ssid.iter().all(|b| *b == 0)) {
        let ssid = String::from_utf8_lossy(&s.ssid).into_owned();
        if out.iter().any(|n| n.ssid == ssid) {
            continue;
        }
        out.push(Network {
            ssid,
            signal_percent: WifiCore::signal_of(s),
            secured: s.security.secured(),
            known: s.known,
        });
    }
    out
}

fn net_outcome(o: JoinOutcome) -> NetOutcome {
    match o {
        JoinOutcome::Connected => NetOutcome::Done,
        JoinOutcome::NoDevice => NetOutcome::NoDevice,
        JoinOutcome::RadioOff => NetOutcome::SwitchOff,
        JoinOutcome::Unsupported => NetOutcome::Unsupported,
        _ => NetOutcome::Failed,
    }
}

fn answer<T: Serialize + ultar_bus::Message>(request: IncomingRequest) -> impl FnOnce(T) + Send {
    move |reply: T| {
        let _ = request.reply(&reply);
    }
}

fn secret(s: Option<ultar_bus::messages::auth::Secret>) -> Option<Zeroizing<String>> {
    s.map(|s| Zeroizing::new(s.expose().to_owned()))
}

fn bad(request: IncomingRequest) -> Option<Command> {
    let _ = request.fail(ErrorCode::BadPayload);
    None
}

/// `net.scan` (`full`: `net.networks`).
fn scan_command(request: IncomingRequest, full: bool) -> Option<Command> {
    let reply: Reply<ScanAnswer> = if full {
        if request.decode::<Networks>().is_err() {
            return bad(request);
        }
        let send = answer::<NetworksReply>(request);
        Box::new(move |a: ScanAnswer| match a {
            Ok(list) => send(NetworksReply {
                outcome: NetOutcome::Done,
                networks: list.iter().map(info).collect(),
            }),
            Err(o) => send(NetworksReply {
                outcome: o,
                networks: Vec::new(),
            }),
        })
    } else {
        if request.decode::<Scan>().is_err() {
            return bad(request);
        }
        let send = answer::<ScanReply>(request);
        Box::new(move |a: ScanAnswer| match a {
            Ok(list) => send(ScanReply {
                outcome: NetOutcome::Done,
                networks: by_name(&list),
            }),
            Err(o) => send(ScanReply {
                outcome: o,
                networks: Vec::new(),
            }),
        })
    };
    Some(Command::Scan(reply))
}

/// `net.join` and `net.connect` (joins and remembers, coarse outcome).
fn join_command(request: IncomingRequest, connect: bool) -> Option<Command> {
    if connect {
        let Ok(c) = request.decode::<Connect>() else {
            return bad(request);
        };
        let send = answer::<NetReply>(request);
        return Some(Command::Join {
            ssid: c.ssid.into_bytes(),
            secret: secret(c.secret),
            remember: true,
            reply: Box::new(move |o| {
                send(NetReply {
                    outcome: net_outcome(o),
                })
            }),
        });
    }
    let Ok(j) = request.decode::<Join>() else {
        return bad(request);
    };
    let send = answer::<JoinReply>(request);
    Some(Command::Join {
        ssid: j.ssid.into_bytes(),
        secret: secret(j.secret),
        remember: j.remember,
        reply: Box::new(move |o| send(JoinReply { outcome: o })),
    })
}

/// `net.disconnect`, `net.forget`, `net.known`.
fn saved_command(request: IncomingRequest, method: &str) -> Option<Command> {
    if method == Forget::METHOD {
        let Ok(f) = request.decode::<Forget>() else {
            return bad(request);
        };
        let send = answer::<NetReply>(request);
        return Some(Command::Forget(
            f.ssid.into_bytes(),
            Box::new(move |o| send(NetReply { outcome: o })),
        ));
    }
    if method == Known::METHOD {
        if request.decode::<Known>().is_err() {
            return bad(request);
        }
        let send = answer::<KnownReply>(request);
        return Some(Command::Known(Box::new(move |ssids| {
            send(KnownReply { ssids })
        })));
    }
    if request.decode::<Disconnect>().is_err() {
        return bad(request);
    }
    let send = answer::<NetReply>(request);
    Some(Command::Disconnect(Box::new(move |o| {
        send(NetReply { outcome: o })
    })))
}

/// One Wi-Fi call; the answer comes from the worker.
pub fn wifi_request(svc: &mut NetService, request: IncomingRequest, _bus: &Bus<'_, NetService>) {
    let method = request.method().to_owned();
    let command = match method.as_str() {
        m if m == Scan::METHOD => scan_command(request, false),
        m if m == Networks::METHOD => scan_command(request, true),
        m if m == Join::METHOD => join_command(request, false),
        m if m == Connect::METHOD => join_command(request, true),
        m if m == Disconnect::METHOD || m == Forget::METHOD || m == Known::METHOD => {
            saved_command(request, m)
        }
        _ => {
            let _ = request.fail(ErrorCode::UnknownMethod);
            None
        }
    };
    if let Some(c) = command {
        svc.wifi.send(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_hal_netlink::nl80211::Bss;

    fn seen(ssid: &[u8], mbm: i32, security: Security) -> Seen {
        Seen {
            ssid: ssid.to_vec(),
            bss: Bss {
                bssid: [2, 0, 0, 0, 0, mbm as u8],
                freq_mhz: 2412,
                signal_mbm: mbm,
                capability: 0,
                ies: vec![],
            },
            security,
            known: false,
            connected: false,
        }
    }

    #[test]
    fn listing_by_name_and_mappings() {
        let list = vec![
            seen(b"home", -4000, Security::Wpa2Personal),
            seen(b"home", -7000, Security::Wpa2Personal),
            seen(&[0, 0], -3000, Security::Open),
            seen(b"cafe", -8000, Security::Open),
        ];
        let names = by_name(&list);
        assert_eq!(names.len(), 2);
        assert_eq!((names[0].ssid.as_str(), names[0].secured), ("home", true));
        assert!(!names[1].secured);
        let i = info(&list[0]);
        assert_eq!(i.bssid, "02:00:00:00:00:60");
        assert!(i.joinable);
        assert_eq!(security(Security::Legacy), WifiSecurity::Legacy);
        assert_eq!(net_outcome(JoinOutcome::WrongPassword), NetOutcome::Failed);
        assert_eq!(net_outcome(JoinOutcome::Connected), NetOutcome::Done);
        assert_eq!(phase_word(WifiPhase::Connected), "connected");
        let mut side = WifiSide::new(PathBuf::from("/nonexistent"), None);
        assert!(!side.present());
        side.radio(true);
        let (tx, rx) = std::sync::mpsc::channel();
        side.send(Command::Known(Box::new(move |l| tx.send(l).unwrap())));
        assert!(rx.recv().unwrap().is_empty());
    }
}
