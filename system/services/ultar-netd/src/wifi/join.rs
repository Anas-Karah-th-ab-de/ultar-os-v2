//! Joining a network and keeping the link: association, the WPA2 handshake,
//! keys, the port, DHCP, signal, and every way it ends.

use super::core::{Reply, WifiCore, SCAN_WAIT_MS};
use ultar_bus::messages::net::{JoinOutcome, WifiPhase};
use ultar_hal_netlink::nl80211::{Bss, ConnectParams};
use ultar_netproto::ie::{self, Security};
use ultar_netproto::wpa::rsn::supplicant_element;
use ultar_netproto::wpa::{Pmk, Step, Supplicant};
use zeroize::Zeroizing;

/// Longest wait for the association.
pub const ASSOCIATE_MS: u64 = 8_000;
/// Longest wait for the handshake.
pub const HANDSHAKE_MS: u64 = 8_000;

/// Where a join is.
pub(super) enum Stage {
    WaitScan,
    Associating {
        bss: Bss,
        pmk: Option<Pmk>,
    },
    Handshake {
        bss: Bss,
        pmk: Pmk,
        sup: Box<Supplicant>,
        msg1: bool,
    },
}

/// A join in progress.
pub(super) struct Join {
    pub ssid: Vec<u8>,
    pub secret: Option<Zeroizing<String>>,
    pub remember: bool,
    pub reply: Option<Reply<JoinOutcome>>,
    pub stage: Stage,
    pub deadline: u64,
}

impl Join {
    /// An automatic reconnection waiting for a scan.
    pub fn auto(now: u64) -> Join {
        Join {
            ssid: Vec::new(),
            secret: None,
            remember: false,
            reply: None,
            stage: Stage::WaitScan,
            deadline: now + SCAN_WAIT_MS + 1_000,
        }
    }

    pub fn phase(&self) -> WifiPhase {
        match self.stage {
            Stage::WaitScan => WifiPhase::Scanning,
            Stage::Associating { .. } => WifiPhase::Associating,
            Stage::Handshake { .. } => WifiPhase::Authenticating,
        }
    }

    pub(super) fn finish(self, outcome: JoinOutcome) {
        if let Some(r) = self.reply {
            r(outcome);
        }
    }
}

impl WifiCore {
    /// Starts joining `ssid` (`reply`: `None` for an automatic one).
    pub(super) fn join_for(
        &mut self,
        ssid: Vec<u8>,
        secret: Option<Zeroizing<String>>,
        remember: bool,
        reply: Option<Reply<JoinOutcome>>,
        now: u64,
    ) {
        let refuse = |r: Option<Reply<JoinOutcome>>, o| {
            if let Some(r) = r {
                r(o);
            }
        };
        if self.iface.is_none() {
            return refuse(reply, JoinOutcome::NoDevice);
        }
        if !self.radio_on {
            return refuse(reply, JoinOutcome::RadioOff);
        }
        if self.join.as_ref().is_some_and(|j| j.reply.is_some()) {
            return refuse(reply, JoinOutcome::Busy);
        }
        if ssid.is_empty() || ssid.len() > ie::MAX_SSID {
            return refuse(reply, JoinOutcome::NotFound);
        }
        if let Some(old) = self.join.take() {
            self.end_join(old, JoinOutcome::Busy);
        }
        self.join = Some(Join {
            ssid,
            secret,
            remember,
            reply,
            stage: Stage::WaitScan,
            deadline: now + SCAN_WAIT_MS + 1_000,
        });
        if self.fresh(now) {
            self.select(now);
        } else {
            self.start_scan(now);
        }
    }

    /// A scan ended: a join waiting for it goes on.
    pub(super) fn join_after_scan(&mut self, now: u64) {
        let Some(j) = &self.join else { return };
        if !matches!(j.stage, Stage::WaitScan) {
            return;
        }
        if j.ssid.is_empty() {
            self.join = None;
            if self.fresh(now) {
                self.auto_join_now(now);
            }
            return;
        }
        if self.fresh(now) {
            self.select(now);
        } else if let Some(j) = self.join.take() {
            j.finish(JoinOutcome::Failed);
        }
    }

    fn auto_join_now(&mut self, now: u64) {
        let best = self
            .seen()
            .into_iter()
            .filter(|s| s.known && s.security.joinable())
            .min_by_key(|s| (self.saved.rank(&s.ssid), -s.bss.signal_mbm));
        if let Some(s) = best {
            self.join_for(s.ssid, None, false, None, now);
        }
    }

    /// Whether `j` needs a secured network: a password was given or the saved entry has a key.
    fn secured(&self, j: &Join) -> bool {
        j.secret.is_some() || self.saved.get(&j.ssid).is_some_and(|e| e.pmk.is_some())
    }

    fn key_for(&self, j: &Join, security: Security) -> Result<Option<Pmk>, JoinOutcome> {
        match security {
            Security::Open if self.secured(j) => Err(JoinOutcome::NotFound),
            Security::Open => Ok(None),
            Security::Wpa2Personal | Security::Wpa2Wpa3Personal => match &j.secret {
                Some(s) => Pmk::from_passphrase(s, &j.ssid)
                    .map(Some)
                    .map_err(|_| JoinOutcome::BadPassword),
                None => self
                    .saved
                    .get(&j.ssid)
                    .and_then(|e| e.pmk.clone())
                    .map(Some)
                    .ok_or(JoinOutcome::PasswordNeeded),
            },
            _ => Err(JoinOutcome::Unsupported),
        }
    }

    /// Chooses the strongest access point of the name and associates.
    fn select(&mut self, now: u64) {
        let Some(mut j) = self.join.take() else {
            return;
        };
        // A network known with a key (or joined with a password now) is never joined
        // through an open access point of the same name (review security S-2: an "evil
        // twin" without encryption would get the traffic of a saved WPA2 network).
        let secured = self.secured(&j);
        let Some(best) = self
            .seen()
            .into_iter()
            .find(|s| s.ssid == j.ssid && !(secured && s.security == Security::Open))
        else {
            return j.finish(JoinOutcome::NotFound);
        };
        let pmk = match self.key_for(&j, best.security) {
            Ok(k) => k,
            Err(o) => return j.finish(o),
        };
        self.drop_link();
        let params = ConnectParams {
            ssid: j.ssid.clone(),
            bssid: best.bss.bssid,
            freq_mhz: best.bss.freq_mhz,
            rsn: pmk.as_ref().map(|_| supplicant_element()),
        };
        let (Some(d), Some(i)) = (self.driver.as_mut(), self.iface.as_ref()) else {
            return j.finish(JoinOutcome::NoDevice);
        };
        if d.connect(i.index, &params).is_err() {
            return j.finish(JoinOutcome::Failed);
        }
        j.stage = Stage::Associating { bss: best.bss, pmk };
        j.deadline = now + ASSOCIATE_MS;
        self.join = Some(j);
    }

    pub(super) fn hang_up(&mut self) {
        if let (Some(d), Some(i)) = (self.driver.as_mut(), self.iface.as_ref()) {
            let _ = d.disconnect(i.index);
        }
    }

    fn end_join(&mut self, j: Join, outcome: JoinOutcome) {
        if !matches!(j.stage, Stage::WaitScan) {
            self.hang_up();
        }
        j.finish(outcome);
    }

    /// The association result.
    pub(super) fn associated(&mut self, status: u16, now: u64) {
        let Some(j) = self.join.take() else { return };
        match (j.stage, status) {
            (Stage::Associating { bss, pmk: None }, 0) => {
                let j = Join {
                    stage: Stage::WaitScan,
                    ..j
                };
                self.link_up(j, bss, None, None, now);
            }
            (
                Stage::Associating {
                    bss,
                    pmk: Some(pmk),
                },
                0,
            ) => {
                let j = Join {
                    stage: Stage::WaitScan,
                    ..j
                };
                self.start_handshake(j, bss, pmk, now);
            }
            (stage @ Stage::Associating { .. }, s) => {
                let j = Join { stage, ..j };
                let o = if s == u16::MAX {
                    JoinOutcome::Timeout
                } else {
                    JoinOutcome::Refused
                };
                self.end_join(j, o);
            }
            (stage, _) => self.join = Some(Join { stage, ..j }),
        }
    }

    fn start_handshake(&mut self, mut j: Join, bss: Bss, pmk: Pmk, now: u64) {
        let Some(mac) = self.iface.as_ref().map(|i| i.mac) else {
            return;
        };
        let mut snonce = [0u8; 32];
        if !(self.random)(&mut snonce) {
            return self.end_join(j, JoinOutcome::Failed);
        }
        let ap_rsn = ie::find_raw(&bss.ies, ie::RSN).unwrap_or_default();
        let sup = Supplicant::new(
            pmk.clone(),
            bss.bssid,
            mac,
            ap_rsn,
            supplicant_element(),
            snonce,
        );
        j.stage = Stage::Handshake {
            bss,
            pmk,
            sup: Box::new(sup),
            msg1: false,
        };
        j.deadline = now + HANDSHAKE_MS;
        self.join = Some(j);
    }

    /// An EAPOL frame of the access point.
    pub(super) fn eapol(&mut self, from: [u8; 6], frame: &[u8], now: u64) {
        if let Some(Stage::Associating { bss, pmk: Some(_) }) = self.stage_of_join() {
            if bss.bssid == from {
                // The frame came before the association event.
                self.associated(0, now);
            }
        }
        if let Some(Stage::Handshake { bss, .. }) = self.stage_of_join() {
            if bss.bssid == from {
                return self.handshake(frame, now);
            }
        }
        self.rekey(from, frame);
    }

    fn handshake(&mut self, frame: &[u8], now: u64) {
        let Some(mut j) = self.join.take() else {
            return;
        };
        let Stage::Handshake {
            bss,
            pmk,
            mut sup,
            msg1,
        } = j.stage
        else {
            self.join = Some(j);
            return;
        };
        let step = sup.receive(frame);
        let (Some(d), Some(i)) = (self.driver.as_mut(), self.iface.as_ref()) else {
            return;
        };
        let to = bss.bssid;
        match step {
            Ok(Step::Send(reply)) => {
                let _ = d.send_eapol(i.index, to, &reply, false);
                j.stage = Stage::Handshake {
                    bss,
                    pmk,
                    sup,
                    msg1: true,
                };
                self.join = Some(j);
            }
            Ok(Step::Complete { reply, tk, group }) => {
                let ok = d.send_eapol(i.index, to, &reply, false).is_ok()
                    && d.install_key(i.index, Some(to), 0, &tk, &[0; 8]).is_ok()
                    && d.install_key(i.index, None, group.index, &group.key, &group.rsc)
                        .is_ok()
                    && d.authorize(i.index, to).is_ok();
                j.stage = Stage::WaitScan;
                if ok {
                    self.link_up(j, bss, Some(sup), Some(pmk), now);
                } else {
                    let _ = d.disconnect(i.index);
                    j.finish(JoinOutcome::Failed);
                }
            }
            Ok(Step::Rekey { .. }) | Err(_) => {
                j.stage = Stage::Handshake {
                    bss,
                    pmk,
                    sup,
                    msg1,
                };
                self.join = Some(j);
            }
        }
    }

    /// The access point or the kernel ended the connection.
    pub(super) fn disconnected(&mut self, _reason: u16, now: u64) {
        if let Some(j) = self.join.take() {
            let outcome = match &j.stage {
                Stage::Handshake { msg1: true, .. } => JoinOutcome::WrongPassword,
                Stage::Handshake { .. } | Stage::Associating { .. } => JoinOutcome::Refused,
                Stage::WaitScan => {
                    self.join = Some(j);
                    return;
                }
            };
            return j.finish(outcome);
        }
        if self.link.is_some() {
            self.drop_link();
            self.reconnect_soon(now);
        }
    }

    /// A join took too long.
    pub(super) fn join_timed_out(&mut self, _now: u64) {
        let Some(j) = self.join.take() else { return };
        let outcome = match &j.stage {
            Stage::Handshake { msg1: true, .. } => JoinOutcome::WrongPassword,
            _ => JoinOutcome::Timeout,
        };
        self.end_join(j, outcome);
    }

    /// Ends a join (answered with `outcome`) and the link.
    pub(super) fn leave(&mut self, outcome: JoinOutcome) {
        if let Some(j) = self.join.take() {
            self.end_join(j, outcome);
        }
        self.drop_link();
    }
}
