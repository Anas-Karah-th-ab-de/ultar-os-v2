//! The Wi-Fi core: commands of the service, events of the radio, timers.

use super::driver::{IpLayer, WifiDriver};
use super::join::{Join, Stage};
use super::link::Link;
use super::saved::Saved;
use ultar_bus::messages::net::{JoinOutcome, NetOutcome, WifiLink, WifiPhase};
use ultar_hal_netlink::nl80211::{signal_percent, Bss, WifiEvent, WifiInterface};
use ultar_netproto::ie::{self, Security};
use zeroize::Zeroizing;

/// Scan results younger than this are used for a join.
pub const SCAN_FRESH_MS: u64 = 10_000;
/// Longest wait for a scan.
pub const SCAN_WAIT_MS: u64 = 10_000;
/// First pause before an automatic reconnection; it doubles up to
/// [`RECONNECT_MAX_MS`].
pub const RECONNECT_FIRST_MS: u64 = 5_000;
/// Longest pause between automatic reconnections.
pub const RECONNECT_MAX_MS: u64 = 300_000;
/// Calls waiting for one scan.
const MAX_WAITING: usize = 8;

/// One network of a scan, as the service reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    /// SSID bytes.
    pub ssid: Vec<u8>,
    /// The access point.
    pub bss: Bss,
    /// Its security.
    pub security: Security,
    /// Saved.
    pub known: bool,
    /// Connected to it.
    pub connected: bool,
}

/// Answer to a scan.
pub type ScanAnswer = Result<Vec<Seen>, NetOutcome>;

/// Fills a buffer with random bytes (`false`: no randomness available).
pub type Random = Box<dyn FnMut(&mut [u8]) -> bool + Send>;

/// Where an answer goes (called once, on the worker's thread).
pub type Reply<T> = Box<dyn FnOnce(T) + Send>;

/// What the service asks.
pub enum Command {
    /// List the networks around.
    Scan(Reply<ScanAnswer>),
    /// Join a network.
    Join {
        /// SSID.
        ssid: Vec<u8>,
        /// Password (wiped when dropped).
        secret: Option<Zeroizing<String>>,
        /// Save it.
        remember: bool,
        /// Where the outcome goes.
        reply: Reply<JoinOutcome>,
    },
    /// Leave the network (no automatic reconnection until the next join).
    Disconnect(Reply<NetOutcome>),
    /// Remove a saved network.
    Forget(Vec<u8>, Reply<NetOutcome>),
    /// The saved networks.
    Known(Reply<Vec<String>>),
    /// The radio switch changed.
    Radio(bool),
}

impl Command {
    /// Answers the command without running it (the worker has ended).
    pub fn refuse(self) {
        match self {
            Command::Scan(r) => r(Err(NetOutcome::NoDevice)),
            Command::Join { reply, .. } => reply(JoinOutcome::NoDevice),
            Command::Disconnect(r) | Command::Forget(_, r) => r(NetOutcome::NoDevice),
            Command::Known(r) => r(Vec::new()),
            Command::Radio(_) => {}
        }
    }
}

#[derive(Default)]
struct ScanState {
    results: Vec<Bss>,
    at: Option<u64>,
    since: Option<u64>,
    waiting: Vec<Reply<ScanAnswer>>,
}

/// The Wi-Fi core.
pub struct WifiCore {
    pub(super) driver: Option<Box<dyn WifiDriver>>,
    pub(super) ip: Box<dyn IpLayer>,
    pub(super) saved: Saved,
    pub(super) iface: Option<WifiInterface>,
    pub(super) radio_on: bool,
    scan: ScanState,
    pub(super) join: Option<Join>,
    pub(super) link: Option<Link>,
    reconnect_at: Option<u64>,
    backoff: u64,
    paused: bool,
    pub(super) random: Random,
}

impl WifiCore {
    /// A core over `driver` (`None`: the kernel has no Wi-Fi) and `ip`,
    /// saved networks in `saved`, `random` filling nonces and ids (`false`:
    /// no randomness – a handshake is not started).
    pub fn new(
        driver: Option<Box<dyn WifiDriver>>,
        ip: Box<dyn IpLayer>,
        saved: Saved,
        random: Random,
    ) -> WifiCore {
        let mut core = WifiCore {
            driver,
            ip,
            saved,
            iface: None,
            radio_on: true,
            scan: ScanState::default(),
            join: None,
            link: None,
            reconnect_at: Some(0),
            backoff: RECONNECT_FIRST_MS,
            paused: false,
            random,
        };
        core.find_interface();
        core
    }

    fn find_interface(&mut self) {
        if self.iface.is_none() {
            if let Some(d) = self.driver.as_mut() {
                self.iface = d.interfaces().ok().and_then(|l| l.into_iter().next());
            }
        }
    }

    /// A Wi-Fi interface exists.
    pub fn present(&self) -> bool {
        self.iface.is_some()
    }

    /// The connection as `net.wifi` shows it.
    pub fn status(&self) -> WifiLink {
        let phase = match (&self.iface, self.radio_on, &self.join, &self.link) {
            (None, ..) => WifiPhase::NoDevice,
            (_, false, ..) => WifiPhase::Off,
            (_, _, Some(j), _) => j.phase(),
            (_, _, None, Some(l)) => l.phase(),
            _ if self.scan.since.is_some() => WifiPhase::Scanning,
            _ => WifiPhase::Disconnected,
        };
        let mut status = WifiLink {
            phase,
            ..WifiLink::default()
        };
        if let (None, Some(l)) = (&self.join, &self.link) {
            l.describe(&mut status);
        }
        status
    }

    /// When [`WifiCore::tick`] must run.
    pub fn next_deadline(&self) -> Option<u64> {
        [
            self.scan.since.map(|s| s + SCAN_WAIT_MS),
            self.join.as_ref().map(|j| j.deadline),
            self.link.as_ref().map(|l| l.deadline()),
            self.reconnect_at.filter(|_| self.may_reconnect()),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    fn may_reconnect(&self) -> bool {
        self.iface.is_some()
            && self.radio_on
            && !self.paused
            && self.join.is_none()
            && self.link.is_none()
            && !self.saved.is_empty()
    }

    /// One command.
    pub fn command(&mut self, command: Command, now: u64) {
        match command {
            Command::Scan(reply) => self.scan_for(reply, now),
            Command::Join {
                ssid,
                secret,
                remember,
                reply,
            } => self.join_for(ssid, secret, remember, Some(reply), now),
            Command::Disconnect(reply) => {
                let outcome = if self.iface.is_none() {
                    NetOutcome::NoDevice
                } else {
                    self.paused = true;
                    self.leave(JoinOutcome::Failed);
                    NetOutcome::Done
                };
                reply(outcome);
            }
            Command::Forget(ssid, reply) => {
                if self.link.as_ref().is_some_and(|l| l.ssid == ssid) {
                    self.leave(JoinOutcome::Failed);
                }
                let outcome = match self.saved.forget(&ssid) {
                    Ok(true) => NetOutcome::Done,
                    Ok(false) | Err(_) => NetOutcome::Failed,
                };
                reply(outcome);
            }
            Command::Known(reply) => reply(self.saved.names()),
            Command::Radio(on) => {
                self.radio_on = on;
                if !on {
                    self.leave(JoinOutcome::RadioOff);
                    self.end_scan(Err(NetOutcome::SwitchOff));
                } else {
                    self.reconnect_at = Some(now);
                }
            }
        }
    }

    fn refusal(&self) -> Option<NetOutcome> {
        if self.iface.is_none() {
            Some(NetOutcome::NoDevice)
        } else if !self.radio_on {
            Some(NetOutcome::SwitchOff)
        } else {
            None
        }
    }

    fn scan_for(&mut self, reply: Reply<ScanAnswer>, now: u64) {
        if let Some(no) = self.refusal() {
            return reply(Err(no));
        }
        if self.scan.waiting.len() >= MAX_WAITING {
            return reply(Err(NetOutcome::Failed));
        }
        self.scan.waiting.push(reply);
        self.start_scan(now);
    }

    /// Starts a scan unless one runs.
    pub(super) fn start_scan(&mut self, now: u64) {
        if self.scan.since.is_some() {
            return;
        }
        let (Some(d), Some(i)) = (self.driver.as_mut(), self.iface.as_ref()) else {
            return;
        };
        match d.trigger_scan(i.index) {
            Ok(()) => self.scan.since = Some(now),
            Err(_) => self.end_scan(Err(NetOutcome::Failed)),
        }
    }

    fn scan_done(&mut self, now: u64) {
        let (Some(d), Some(i)) = (self.driver.as_mut(), self.iface.as_ref()) else {
            return;
        };
        match d.scan_results(i.index) {
            Ok(list) => {
                self.scan.results = list;
                self.scan.at = Some(now);
                let seen = self.seen();
                self.end_scan(Ok(seen));
            }
            Err(_) => self.end_scan(Err(NetOutcome::Failed)),
        }
        self.join_after_scan(now);
    }

    fn end_scan(&mut self, answer: ScanAnswer) {
        self.scan.since = None;
        for w in self.scan.waiting.drain(..) {
            w(answer.clone());
        }
    }

    /// The networks of the last scan, strongest first.
    pub fn seen(&self) -> Vec<Seen> {
        let connected = self.link.as_ref().map(|l| l.bssid);
        let mut list: Vec<Seen> = self
            .scan
            .results
            .iter()
            .filter_map(|b| {
                let ssid = ie::ssid(&b.ies)?;
                Some(Seen {
                    known: self.saved.get(&ssid).is_some(),
                    connected: connected == Some(b.bssid),
                    security: ie::security(&b.ies, b.capability & 0x10 != 0),
                    ssid,
                    bss: b.clone(),
                })
            })
            .collect();
        list.sort_by_key(|s| std::cmp::Reverse(s.bss.signal_mbm));
        list
    }

    /// Fresh scan results.
    pub(super) fn fresh(&self, now: u64) -> bool {
        self.scan
            .at
            .is_some_and(|t| now.saturating_sub(t) < SCAN_FRESH_MS)
    }

    /// One event of the radio.
    pub fn event(&mut self, event: WifiEvent, now: u64) {
        let mine = |i: u32| self.iface.as_ref().is_some_and(|f| f.index == i);
        match event {
            WifiEvent::ScanDone(i) if mine(i) => self.scan_done(now),
            WifiEvent::ScanAborted(i) if mine(i) => {
                self.end_scan(Err(NetOutcome::Failed));
                self.join_after_scan(now);
            }
            WifiEvent::Connected {
                ifindex, status, ..
            } if mine(ifindex) => self.associated(status, now),
            WifiEvent::Disconnected { ifindex, reason } if mine(ifindex) => {
                self.disconnected(reason, now)
            }
            WifiEvent::Eapol {
                ifindex,
                from,
                frame,
            } if mine(ifindex) => self.eapol(from, &frame, now),
            _ => {}
        }
    }

    /// Timers.
    pub fn tick(&mut self, now: u64) {
        self.find_interface();
        if self.scan.since.is_some_and(|s| now >= s + SCAN_WAIT_MS) {
            self.end_scan(Err(NetOutcome::Failed));
            self.join_after_scan(now);
        }
        if self.join.as_ref().is_some_and(|j| now >= j.deadline) {
            self.join_timed_out(now);
        }
        self.dhcp(now);
        if self.may_reconnect() && self.reconnect_at.is_some_and(|t| now >= t) {
            self.reconnect_at = Some(now + self.backoff);
            self.backoff = (self.backoff * 2).min(RECONNECT_MAX_MS);
            self.auto_join(now);
        }
    }

    fn auto_join(&mut self, now: u64) {
        if !self.fresh(now) {
            self.join = Some(Join::auto(now));
            self.start_scan(now);
            return;
        }
        let best = self
            .seen()
            .into_iter()
            .filter(|s| s.known && s.security.joinable())
            .min_by_key(|s| (self.saved.rank(&s.ssid), -s.bss.signal_mbm));
        if let Some(s) = best {
            self.join_for(s.ssid, None, false, None, now);
        }
    }

    /// A join or the link succeeded: the pause starts small again.
    pub(super) fn reset_backoff(&mut self) {
        self.backoff = RECONNECT_FIRST_MS;
        self.paused = false;
    }

    /// After a lost link: try again soon.
    pub(super) fn reconnect_soon(&mut self, now: u64) {
        self.reconnect_at = Some(now + 1_000);
    }

    pub(super) fn stage_of_join(&self) -> Option<&Stage> {
        self.join.as_ref().map(|j| &j.stage)
    }

    /// The signal of the access point in %.
    pub(super) fn percent(mbm: i32) -> u8 {
        signal_percent(mbm)
    }

    /// The signal of a scanned network in %.
    pub fn signal_of(s: &Seen) -> u8 {
        signal_percent(s.bss.signal_mbm)
    }
}

#[cfg(test)]
#[path = "core_fakes.rs"]
mod fakes;
#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
