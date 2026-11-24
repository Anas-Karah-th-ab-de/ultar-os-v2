//! The network service: state from the kernel, radio switches, the bus side.

use crate::internet::{self, Prober, Target};
use crate::wired::{self, Fixed};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::net::{
    Internet, LinkState, NetOutcome, NetReply, NetState, SetAirplane, SetIpv4, SetWifi, Wired,
    SERVICE,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest, Topic};
use ultar_hal::HalError;
use ultar_hal_linux::net::Interfaces;
use ultar_hal_linux::rfkill::{RadioKind, Rfkill};
use ultar_service::log::{Level, Record};
use ultar_svckit::{store, Bus, Service};

/// A Wi-Fi adapter the kernel lists under no interface yet (only its radio switch).
const WIFI_PRESENT: ultar_bus::messages::net::WifiLink = ultar_bus::messages::net::WifiLink {
    phase: ultar_bus::messages::net::WifiPhase::Disconnected,
    ssid: None,
    signal_percent: None,
    address: None,
    router: None,
    dns: Vec::new(),
};

/// How often the links and switches are read again.
pub const TICK: Duration = Duration::from_secs(3);
const RADIOS: [RadioKind; 3] = [RadioKind::Wlan, RadioKind::Bluetooth, RadioKind::Wwan];

/// What is kept across restarts.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Saved {
    /// Wi-Fi radio wanted on.
    pub wifi_on: bool,
    /// Airplane mode wanted on.
    pub airplane: bool,
}

impl Default for Saved {
    fn default() -> Self {
        Self {
            wifi_on: true,
            airplane: false,
        }
    }
}

/// The network service.
pub struct NetService {
    interfaces: Interfaces,
    rfkill: Rfkill,
    state_file: PathBuf,
    saved: Saved,
    last: Option<NetState>,
    pub(crate) wifi: crate::calls::WifiSide,
    fixed: Fixed,
    fixed_file: PathBuf,
    target: Target,
    prober: Option<Prober>,
}

fn outcome(result: Result<(), HalError>) -> NetOutcome {
    match result {
        Ok(()) => NetOutcome::Done,
        Err(HalError::NotSupported) => NetOutcome::NoDevice,
        Err(_) => NetOutcome::Failed,
    }
}

impl NetService {
    /// A service reading the kernel below `root`, keeping its wishes below `state_dir`.
    pub fn new(root: PathBuf, state_dir: PathBuf) -> Self {
        let state_file = state_dir.join("net");
        let fixed_file = state_dir.join("ipv4");
        Self {
            interfaces: Interfaces::new(&root),
            rfkill: Rfkill::new(&root),
            saved: store::load(&state_file).unwrap_or_default(),
            wifi: crate::calls::WifiSide::new(state_dir.join("wifi"), None)
                .on_kernel(root == std::path::Path::new("/")),
            state_file,
            last: None,
            fixed: store::load(&fixed_file).unwrap_or_default(),
            fixed_file,
            target: Target::parse(internet::DEFAULT_TARGET).unwrap_or(Target {
                host: "ultar.code-craft.tech".into(),
                port: 443,
            }),
            prober: None,
        }
    }

    /// The address the internet check connects to (`host:port`).
    pub fn with_probe(mut self, target: Option<Target>) -> Self {
        if let Some(t) = target {
            self.target = t;
        }
        self
    }

    /// Where `resolv.conf` is written (`--run`; `None`: not written).
    pub fn with_run_dir(mut self, run: Option<PathBuf>) -> Self {
        self.wifi.run_dir = run;
        self
    }

    /// The state as the kernel shows it now.
    pub fn read(&self) -> NetState {
        let list = self.interfaces.list();
        let radios = self.rfkill.radios();
        let wlan: Vec<_> = radios
            .iter()
            .filter(|r| r.kind == RadioKind::Wlan)
            .collect();
        let wifi_present = list.iter().any(|i| i.wireless) || !wlan.is_empty();
        let wifi_on = wifi_present && wlan.iter().all(|r| !r.soft && !r.hard);
        let wired = list.iter().find(|i| !i.wireless && i.up).map(|i| Wired {
            speed_mbit: i.speed_mbit,
        });
        let wifi_present = wifi_present || self.wifi.present();
        let ethernet: Vec<_> = list
            .iter()
            .filter(|i| !i.wireless)
            .map(wired::ethernet)
            .collect();
        let wifi_link = if wifi_present {
            wired::wifi(list.iter().find(|i| i.wireless), &self.wifi.link, wifi_on)
                .or_else(|| wired::wifi(None, &WIFI_PRESENT, wifi_on))
        } else {
            None
        };
        let online = ethernet
            .iter()
            .chain(wifi_link.iter())
            .any(|l| l.state == LinkState::Connected);
        let internet = match (&self.prober, online) {
            (_, false) => Internet::Offline,
            (Some(p), true) => match p.result() {
                Internet::Offline => Internet::Checking,
                r => r,
            },
            (None, true) => Internet::Checking,
        };
        NetState {
            wifi_present,
            wifi_on,
            wifi_switch_off: wlan.iter().any(|r| r.hard),
            ssid: self.wifi.link.ssid.clone(),
            signal_percent: self.wifi.link.signal_percent,
            wired,
            airplane: !radios.is_empty() && radios.iter().all(|r| r.soft || r.hard),
            airplane_possible: !radios.is_empty(),
            metered: false,
            networks_supported: self.wifi.present(),
            ethernet,
            wifi_link,
            internet,
        }
    }

    /// The network the internet check runs on: the wired interface with the default route,
    /// another connected one, or Wi-Fi.
    fn in_use(state: &NetState) -> Option<internet::Network> {
        state
            .ethernet
            .iter()
            .filter(|l| l.state == LinkState::Connected)
            .chain(
                state
                    .wifi_link
                    .iter()
                    .filter(|l| l.state == LinkState::Connected),
            )
            .next()
            .map(|l| internet::Network {
                dns: l.dns.clone(),
                gateway: l.gateway.clone(),
            })
    }

    /// Sets a fixed address, DHCP or a renewal on a wired interface and keeps the wish.
    pub fn set_ipv4(&mut self, c: &SetIpv4) -> NetOutcome {
        let Some(request) = wired::request(&c.config) else {
            return NetOutcome::Failed;
        };
        if !self
            .interfaces
            .list()
            .iter()
            .any(|i| i.name == c.interface && !i.wireless)
        {
            return NetOutcome::NoDevice;
        }
        match self.interfaces.set_ipv4(&c.interface, &request) {
            Ok(()) => {
                self.fixed.note(&c.interface, &c.config);
                NetOutcome::Done
            }
            Err(HalError::NotSupported) => NetOutcome::Unsupported,
            Err(HalError::NotFound) => NetOutcome::NoDevice,
            Err(_) => NetOutcome::Failed,
        }
    }

    /// Fixed addresses the kernel does not hold (after a restart, a re-plugged card) go back.
    fn keep_fixed(&mut self) {
        let list = self.interfaces.list();
        for (name, config) in &self.fixed.entries {
            let lost = list
                .iter()
                .any(|i| &i.name == name && i.ipv4.as_ref().is_some_and(|v| !v.fixed));
            if let (true, Some(r)) = (lost, wired::request(config)) {
                let _ = self.interfaces.set_ipv4(name, &r);
            }
        }
    }

    pub(crate) fn publish(&mut self, bus: &Bus<'_, Self>) {
        let now = self.read();
        self.wifi.radio(now.wifi_on);
        if let Some(p) = &self.prober {
            p.network(Self::in_use(&now));
        }
        let wired_dns: Vec<String> = now
            .ethernet
            .iter()
            .filter(|l| l.state == LinkState::Connected)
            .flat_map(|l| l.dns.iter().cloned())
            .collect();
        self.wifi.wired_dns(wired_dns, bus);
        if self.last.as_ref() != Some(&now) {
            bus.retain(&now);
            self.last = Some(now);
        }
    }

    fn save(&self, bus: &Bus<'_, Self>) {
        if store::save(&self.state_file, &self.saved).is_err() {
            bus.log(Record::new(Level::Warn, "netd.not_saved"));
        }
    }

    /// Switches the Wi-Fi radio (and leaves airplane mode when it is switched on).
    pub fn set_wifi(&mut self, on: bool) -> NetOutcome {
        let wlan: Vec<_> = self
            .rfkill
            .radios()
            .into_iter()
            .filter(|r| r.kind == RadioKind::Wlan)
            .collect();
        if wlan.is_empty() {
            return NetOutcome::NoDevice;
        }
        if on && wlan.iter().any(|r| r.hard) {
            return NetOutcome::SwitchOff;
        }
        let result = outcome(self.rfkill.set(RadioKind::Wlan, !on));
        if result == NetOutcome::Done {
            self.saved.wifi_on = on;
            if on {
                self.saved.airplane = false;
            }
        }
        result
    }

    /// Switches airplane mode: every radio off, or back as wanted before.
    pub fn set_airplane(&mut self, on: bool) -> NetOutcome {
        let radios = self.rfkill.radios();
        if radios.is_empty() {
            return NetOutcome::NoDevice;
        }
        for kind in RADIOS {
            if !radios.iter().any(|r| r.kind == kind) {
                continue;
            }
            let block = on || (kind == RadioKind::Wlan && !self.saved.wifi_on);
            if let Err(e) = self.rfkill.set(kind, block) {
                return outcome(Err(e));
            }
        }
        self.saved.airplane = on;
        NetOutcome::Done
    }
}

impl Service for NetService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        // The kernel starts the radios as its drivers like; the wanted state comes back.
        let saved = self.saved;
        if !self.rfkill.radios().is_empty() {
            if saved.airplane {
                let _ = self.set_airplane(true);
            } else if !saved.wifi_on {
                let _ = self.set_wifi(false);
            }
        }
        self.wifi.start(bus);
        self.keep_fixed();
        self.prober = Some(Prober::start(self.target.clone()));
        self.publish(bus);
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![ultar_bus::messages::vpn::VpnState::TOPIC]
    }

    fn event(&mut self, event: ultar_bus::RawEvent, bus: &Bus<'_, Self>) {
        if let Ok(v) = event.decode::<ultar_bus::messages::vpn::VpnState>() {
            self.wifi.vpn_dns(v.dns, bus);
        }
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let result = match request.method() {
            m if m == SetWifi::METHOD => request.decode::<SetWifi>().map(|c| self.set_wifi(c.on)),
            m if m == SetAirplane::METHOD => request
                .decode::<SetAirplane>()
                .map(|c| self.set_airplane(c.on)),
            m if m == SetIpv4::METHOD => request.decode::<SetIpv4>().map(|c| {
                let outcome = self.set_ipv4(&c);
                if outcome == NetOutcome::Done
                    && store::save(&self.fixed_file, &self.fixed).is_err()
                {
                    bus.log(Record::new(Level::Warn, "netd.not_saved"));
                }
                outcome
            }),
            _ => {
                crate::calls::wifi_request(self, request, bus);
                return;
            }
        };
        match result {
            Ok(outcome) => {
                // Kept before it is confirmed: a crash right after the answer loses nothing.
                self.save(bus);
                let _ = request.reply(&NetReply { outcome });
                self.publish(bus);
            }
            Err(_) => {
                let _ = request.fail(ErrorCode::BadPayload);
            }
        }
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.keep_fixed();
        self.publish(bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &std::path::Path, path: &str, value: &str) {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, value).unwrap();
    }

    fn laptop(tag: &str, hard: bool) -> PathBuf {
        let root = std::env::temp_dir().join(format!("netd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        put(&root, "sys/class/net/eth0/type", "1");
        put(&root, "sys/class/net/eth0/operstate", "up");
        put(&root, "sys/class/net/eth0/carrier", "1");
        put(&root, "sys/class/net/eth0/speed", "100");
        put(&root, "sys/class/net/wlan0/type", "1");
        std::fs::create_dir_all(root.join("sys/class/net/wlan0/wireless")).unwrap();
        put(&root, "sys/class/rfkill/rfkill0/type", "wlan");
        put(&root, "sys/class/rfkill/rfkill0/soft", "0");
        put(
            &root,
            "sys/class/rfkill/rfkill0/hard",
            if hard { "1" } else { "0" },
        );
        put(&root, "sys/class/rfkill/rfkill1/type", "bluetooth");
        put(&root, "sys/class/rfkill/rfkill1/soft", "0");
        put(&root, "dev/rfkill", "");
        root
    }

    #[test]
    fn state_and_switches() {
        let root = laptop("sw", false);
        let mut n = NetService::new(root.clone(), root.clone());
        let s = n.read();
        assert!(
            s.wifi_present
                && s.wifi_on
                && !s.airplane
                && s.airplane_possible
                && !s.networks_supported,
            "no Wi-Fi worker in a unit test: networks not supported"
        );
        assert_eq!(
            s.wired,
            Some(Wired {
                speed_mbit: Some(100)
            })
        );
        assert_eq!(n.set_wifi(false), NetOutcome::Done);
        assert!(!n.saved.wifi_on);
        assert_eq!(n.set_airplane(true), NetOutcome::Done);
        // Two radio types switched: wlan and bluetooth, both blocked.
        let written = std::fs::read(root.join("dev/rfkill")).unwrap();
        assert_eq!(written.len(), 8 * 3);
        assert_eq!(&written[16..24], &[0, 0, 0, 0, 2, 3, 1, 0]);
        assert_eq!(n.set_wifi(true), NetOutcome::Done);
        assert!(!n.saved.airplane, "Wi-Fi on leaves airplane mode");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_wish_is_stored() {
        let root = laptop("store", false);
        let mut n = NetService::new(root.clone(), root.clone());
        assert_eq!(n.set_airplane(true), NetOutcome::Done);
        store::save(&n.state_file, &n.saved).unwrap();
        let again = NetService::new(root.clone(), root.clone());
        assert!(again.saved.airplane);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hardware_switch_and_no_radios() {
        let root = laptop("hard", true);
        let mut n = NetService::new(root.clone(), root.clone());
        assert!(n.read().wifi_switch_off && !n.read().wifi_on);
        assert_eq!(n.set_wifi(true), NetOutcome::SwitchOff);
        let none = std::env::temp_dir().join(format!("netd-none-{}", std::process::id()));
        let mut empty = NetService::new(none.clone(), none);
        assert_eq!(empty.set_wifi(true), NetOutcome::NoDevice);
        assert_eq!(empty.set_airplane(true), NetOutcome::NoDevice);
        assert_eq!(empty.read(), NetState::default());
        let _ = std::fs::remove_dir_all(&root);
    }
}
