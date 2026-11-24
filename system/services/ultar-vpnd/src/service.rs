//! The VPN service on the bus: `vpn.state`, `vpn.import`, `vpn.set`,
//! `vpn.remove`; the tunnels saved below the state directory (one checked
//! file per tunnel, mode 0600, holding its keys); the wish (on, which one)
//! kept across restarts.

use crate::conf::{self, TunnelConf};
use crate::tunnel::{Tunnel, TunnelError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::vpn::{
    Import, ImportOutcome, ImportReply, RemoveVpn, SetVpn, VpnOutcome, VpnProfile, VpnReply,
    VpnState, VpnStatus, VpnUnavailable, MAX_NAME, MAX_PROFILES, SERVICE,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_hal_netlink::tun;
use ultar_service::log::{Level, Record};
use ultar_svckit::{store, Bus, Service};

/// How often the tunnel's numbers are read.
pub const TICK: Duration = Duration::from_secs(2);
/// A handshake younger than this means "up".
pub const UP_WITHIN: Duration = Duration::from_secs(180);

/// What is kept across restarts.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq, Debug)]
pub struct Wish {
    /// The tunnel wanted on.
    pub on: bool,
    /// The last tunnel used.
    pub last: Option<String>,
}

/// The VPN service.
pub struct VpnService {
    root: PathBuf,
    dir: PathBuf,
    wish: Wish,
    profiles: BTreeMap<String, TunnelConf>,
    tunnel: Option<Tunnel>,
    active: Option<String>,
    failed: bool,
    index: u32,
    last: Option<VpnState>,
}

/// A name is 1–32 of `[a-z0-9_-]`.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

impl VpnService {
    /// A service with the TUN device below `root`, its files below `state`.
    pub fn new(root: PathBuf, state: PathBuf) -> VpnService {
        let dir = state.join("tunnels");
        // The service's own directory below the network service's (0700).
        let _ = create_private_dir(&state);
        let mut profiles = BTreeMap::new();
        if let Ok(list) = std::fs::read_dir(&dir) {
            for e in list.flatten().take(MAX_PROFILES) {
                let name = e.file_name().to_string_lossy().into_owned();
                if !name_ok(&name) {
                    continue;
                }
                if let Ok(c) = store::load::<TunnelConf>(&e.path()) {
                    profiles.insert(name, c);
                }
            }
        }
        VpnService {
            wish: store::load(&state.join("vpn")).unwrap_or_default(),
            root,
            dir,
            profiles,
            tunnel: None,
            active: None,
            failed: false,
            index: std::process::id(),
            last: None,
        }
    }

    fn wish_file(&self) -> PathBuf {
        self.dir.with_file_name("vpn")
    }

    fn unavailable(&self) -> Option<VpnUnavailable> {
        (!tun::control_path(&self.root).exists()).then_some(VpnUnavailable::NoTun)
    }

    /// The state as published.
    pub fn state(&self) -> VpnState {
        let stats = self.tunnel.as_ref().map(|t| t.stats()).unwrap_or_default();
        let status = match (&self.tunnel, self.failed) {
            (Some(_), _) if stats.handshake_age.is_some_and(|a| a < UP_WITHIN) => VpnStatus::Up,
            (Some(_), _) => VpnStatus::Connecting,
            (None, true) => VpnStatus::Failed,
            (None, false) => VpnStatus::Off,
        };
        let active = self.active.as_ref().and_then(|n| self.profiles.get(n));
        VpnState {
            available: self.unavailable().is_none(),
            unavailable: self.unavailable(),
            profiles: self
                .profiles
                .iter()
                .map(|(name, c)| VpnProfile {
                    name: name.clone(),
                    endpoint: c.endpoint.to_string(),
                    addresses: c
                        .addresses
                        .iter()
                        .map(|(a, p)| format!("{a}/{p}"))
                        .collect(),
                    full_tunnel: c.full_tunnel(),
                })
                .collect(),
            active: self.tunnel.as_ref().and(self.active.clone()),
            status,
            handshake_age_secs: stats.handshake_age.map(|a| a.as_secs()),
            rx_bytes: stats.rx,
            tx_bytes: stats.tx,
            dns: match (&self.tunnel, active) {
                (Some(_), Some(c)) => c.dns.iter().map(|d| d.to_string()).collect(),
                _ => Vec::new(),
            },
        }
    }

    fn publish(&mut self, bus: &Bus<'_, Self>) {
        let now = self.state();
        if self.last.as_ref() != Some(&now) {
            bus.retain(&now);
            self.last = Some(now);
        }
    }

    fn save_wish(&self, bus: &Bus<'_, Self>) {
        if store::save(&self.wish_file(), &self.wish).is_err() {
            bus.log(Record::new(Level::Warn, "vpn.not_saved"));
        }
    }

    /// Saves a tunnel from a `.conf` text.
    pub fn import(&mut self, name: &str, text: &str) -> ImportOutcome {
        if self.unavailable().is_some() {
            return ImportOutcome::Unavailable;
        }
        if !name_ok(name) {
            return ImportOutcome::BadName;
        }
        if self.profiles.contains_key(name) {
            return ImportOutcome::Exists;
        }
        if self.profiles.len() >= MAX_PROFILES {
            return ImportOutcome::TooMany;
        }
        let c = match conf::parse(text) {
            Ok(c) => c,
            Err((problem, line)) => return ImportOutcome::Invalid { problem, line },
        };
        if create_private_dir(&self.dir).is_err() || store::save(&self.dir.join(name), &c).is_err()
        {
            return ImportOutcome::Failed;
        }
        self.profiles.insert(name.to_owned(), c);
        ImportOutcome::Done
    }

    /// Switches a tunnel on or off.
    pub fn set(&mut self, on: bool, name: Option<String>) -> VpnOutcome {
        if self.unavailable().is_some() {
            return VpnOutcome::Unavailable;
        }
        self.tunnel = None;
        self.failed = false;
        if !on {
            self.wish.on = false;
            return VpnOutcome::Done;
        }
        let only = (self.profiles.len() == 1)
            .then(|| self.profiles.keys().next().cloned())
            .flatten();
        let Some(name) = name
            .or(self.wish.last.clone())
            .or(only)
            .filter(|n| self.profiles.contains_key(n))
        else {
            return VpnOutcome::NotFound;
        };
        let Some(c) = self.profiles.get(&name) else {
            return VpnOutcome::NotFound;
        };
        self.index = self.index.wrapping_add(1);
        self.wish = Wish {
            on: true,
            last: Some(name.clone()),
        };
        self.active = Some(name);
        match Tunnel::start(&self.root, c, self.index) {
            Ok(t) => {
                self.tunnel = Some(t);
                VpnOutcome::Done
            }
            Err(TunnelError::NoTun) => {
                self.failed = true;
                VpnOutcome::Unavailable
            }
            Err(_) => {
                self.failed = true;
                VpnOutcome::Failed
            }
        }
    }

    /// Deletes a tunnel (switched off first).
    pub fn remove(&mut self, name: &str) -> VpnOutcome {
        if !self.profiles.contains_key(name) {
            return VpnOutcome::NotFound;
        }
        if self.active.as_deref() == Some(name) {
            self.tunnel = None;
            self.active = None;
            self.wish.on = false;
        }
        if self.wish.last.as_deref() == Some(name) {
            self.wish.last = None;
        }
        self.profiles.remove(name);
        match std::fs::remove_file(self.dir.join(name)) {
            Ok(()) => VpnOutcome::Done,
            Err(_) => VpnOutcome::Failed,
        }
    }
}

fn create_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if !dir.exists() {
        std::fs::DirBuilder::new().mode(0o700).create(dir)?;
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

impl Service for VpnService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        if self.wish.on && self.set(true, None) != VpnOutcome::Done {
            bus.log(Record::new(Level::Warn, "vpn.not_restored"));
        }
        self.publish(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let answered = match request.method() {
            m if m == Import::METHOD => match request.decode::<Import>() {
                Ok(i) => {
                    let outcome = self.import(&i.name, i.config.expose());
                    if outcome == ImportOutcome::Done {
                        bus.log(Record::new(Level::Notice, "vpn.imported"));
                    }
                    request.reply(&ImportReply { outcome })
                }
                Err(_) => request.fail(ErrorCode::BadPayload),
            },
            m if m == SetVpn::METHOD => match request.decode::<SetVpn>() {
                Ok(s) => {
                    let outcome = self.set(s.on, s.name);
                    self.save_wish(bus);
                    let word = if s.on { "on" } else { "off" };
                    bus.log(Record::new(Level::Info, "vpn.set").word("to", word));
                    request.reply(&VpnReply { outcome })
                }
                Err(_) => request.fail(ErrorCode::BadPayload),
            },
            m if m == RemoveVpn::METHOD => match request.decode::<RemoveVpn>() {
                Ok(r) => {
                    let outcome = self.remove(&r.name);
                    self.save_wish(bus);
                    request.reply(&VpnReply { outcome })
                }
                Err(_) => request.fail(ErrorCode::BadPayload),
            },
            _ => request.fail(ErrorCode::UnknownMethod),
        };
        let _ = answered;
        self.publish(bus);
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.publish(bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_no_tun() {
        assert!(name_ok("office-1_x"));
        for bad in ["", "Office", "a b", "../x", &"x".repeat(33)] {
            assert!(!name_ok(bad), "{bad}");
        }
        let dir = std::env::temp_dir().join(format!("vpnd-unit-{}", std::process::id()));
        let mut s = VpnService::new(dir.join("root"), dir.join("state"));
        assert_eq!(s.state().unavailable, Some(VpnUnavailable::NoTun));
        assert!(!s.state().available);
        assert_eq!(s.import("x", ""), ImportOutcome::Unavailable);
        assert_eq!(s.set(true, None), VpnOutcome::Unavailable);
        assert_eq!(s.remove("x"), VpnOutcome::NotFound);
        assert_eq!(s.state().status, VpnStatus::Off);
    }
}
