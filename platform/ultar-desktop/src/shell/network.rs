//! Clicks of the Wi-Fi and VPN parts of the quick settings (CLICKS qs.wifi.list, qs.vpn,
//! qs.vpn.list): the networks page, the join dialog and the VPN connections. The typed
//! password never enters the shell's state: the app takes it out of the field and hands it
//! to [`Shell::join_with`], which puts it into the one `net.join` call.

use super::bar::{launch, SETTINGS};
use super::sheets::service;
use super::{Effect, Opening, Outcome, Shell};
use crate::model::quick::TileId;
use crate::model::services::Call;
use crate::model::wifi::{self, Joining, NetworkClick};
use crate::model::{Open, QuickPage};
use crate::view::Target;
use ultar_bus::messages::auth::Secret;
use ultar_bus::messages::net::JoinOutcome;
use ultar_ui::event::ActionKind;

/// Settings > Network > VPN (where `.conf` files are imported).
pub const VPN_SETTINGS: &str = "network/vpn";

fn show(s: &mut Shell, page: QuickPage, calls: Vec<Call>) -> Outcome {
    s.d.open = Open::Quick(page);
    let mut out = service(None);
    out.effects.extend(calls.into_iter().map(Effect::Service));
    out
}

/// qs.wifi.list / qs.vpn.list: the page of the tile's list part.
pub(super) fn more(s: &mut Shell, id: TileId) -> Option<Outcome> {
    match id {
        // The networks page asks for a fresh list and the saved names (without the network
        // service the page only says so).
        TileId::Wifi => Some(wifi_page(s)),
        TileId::Vpn => Some(show(s, QuickPage::Vpn, Vec::new())),
        _ => None,
    }
}

/// The Wi-Fi page; it asks the network service for the networks when there is one.
pub(super) fn wifi_page(s: &mut Shell) -> Outcome {
    let calls = if s.d.svc.net.is_some() {
        vec![Call::Networks, Call::Known]
    } else {
        Vec::new()
    };
    show(s, QuickPage::Wifi, calls)
}

/// shell.status on the network symbol: the Wi-Fi page directly (again: closed).
pub(super) fn wifi_from_bar(s: &mut Shell) -> Outcome {
    if s.d.open == Open::Quick(QuickPage::Wifi) {
        return s.close();
    }
    let mut out = wifi_page(s);
    out.bar = true;
    out
}

/// qs.vpn: the last used connection on or off; without one, Settings > Network > VPN;
/// without the VPN service its page, which says so.
pub(super) fn vpn_tile(s: &mut Shell) -> Outcome {
    match &s.d.svc.vpn {
        Some(v) if v.profiles.is_empty() => {
            launch(s, SETTINGS, Opening::settings_page(VPN_SETTINGS))
        }
        Some(v) => service(Some(Call::SetVpn(!wifi::vpn_on(v), None))),
        None => show(s, QuickPage::Vpn, Vec::new()),
    }
}

/// A click on the networks page, the join dialog or the VPN page; `None`: not one of them.
pub(super) fn on_popup(s: &mut Shell, target: &Target, kind: &ActionKind) -> Option<Outcome> {
    Some(match (target, kind) {
        // Back in the join dialog: to the networks.
        (Target::QuickBack, ActionKind::Activated)
            if s.d.open == Open::Quick(QuickPage::WifiJoin) =>
        {
            cancel(s)
        }
        (Target::WifiRadio, ActionKind::Toggled(on)) => service(Some(Call::SetWifi(*on))),
        (Target::WifiNetwork(ssid), ActionKind::Activated) => network(s, ssid),
        (Target::WifiForget(ssid), ActionKind::Activated) => {
            let mut out = service(Some(Call::Forget(ssid.clone())));
            out.effects.push(Effect::Service(Call::Known));
            out.effects.push(Effect::Service(Call::Networks));
            out
        }
        (Target::WifiSettings, ActionKind::Activated) => {
            launch(s, SETTINGS, Opening::settings_page("wifi"))
        }
        (Target::JoinRemember, ActionKind::Toggled(on)) => {
            if let Some(j) = s.d.svc.wifi.joining.as_mut() {
                j.remember = *on;
            }
            service(None)
        }
        (Target::JoinCancel, ActionKind::Activated) => cancel(s),
        (Target::VpnSwitch, ActionKind::Toggled(on)) => service(Some(Call::SetVpn(*on, None))),
        (Target::VpnProfile(name), ActionKind::Activated) => {
            service(Some(Call::SetVpn(true, Some(name.clone()))))
        }
        (Target::VpnSettings, ActionKind::Activated) => {
            launch(s, SETTINGS, Opening::settings_page(VPN_SETTINGS))
        }
        _ => return None,
    })
}

/// A network row: join (open or saved), ask for the password, or leave the connected one.
fn network(s: &mut Shell, ssid: &str) -> Outcome {
    let networks = s.d.svc.wifi.networks.as_deref().unwrap_or_default();
    match wifi::click(networks, ssid) {
        NetworkClick::Join(ssid) => service(Some(Call::Join(ssid, None, true))),
        NetworkClick::AskPassword(ssid, security) => {
            s.d.svc.wifi.joining = Some(Joining {
                ssid,
                security,
                remember: true,
                busy: false,
                failed: None,
            });
            show(s, QuickPage::WifiJoin, Vec::new())
        }
        NetworkClick::Disconnect => service(Some(Call::Disconnect)),
        NetworkClick::Nothing => Outcome::none(),
    }
}

/// The join dialog closes without joining (Cancel, Back, Esc).
pub(super) fn cancel(s: &mut Shell) -> Outcome {
    s.d.svc.wifi.joining = None;
    show(s, QuickPage::Wifi, Vec::new())
}

impl Shell {
    /// Join dialog: Connect or Enter with the typed password (taken out of the field by
    /// the app; an empty one is not sent). Nothing while a try runs.
    pub fn join_with(&mut self, secret: Secret) -> Outcome {
        let Some(j) = self.d.svc.wifi.joining.as_mut() else {
            return Outcome::none();
        };
        if j.busy || secret.is_empty() {
            return Outcome::none();
        }
        j.busy = true;
        j.failed = None;
        let call = Call::Join(j.ssid.clone(), Some(secret), j.remember);
        service(Some(call))
    }

    /// The answer of `net.join` (`None`: the service did not answer). Connected: back to
    /// the networks, which are asked for again; otherwise the dialog says why. A join
    /// without the dialog that needs a password opens the dialog. The flag: the shell must
    /// say in a notice that joining failed (no dialog shows it).
    pub fn joined(&mut self, ssid: &str, outcome: Option<JoinOutcome>) -> (Outcome, bool) {
        let out = self.joined_outcome(ssid, outcome);
        let silent = out.is_some();
        (out.unwrap_or_else(|| service(None)), !silent)
    }

    fn joined_outcome(&mut self, ssid: &str, outcome: Option<JoinOutcome>) -> Option<Outcome> {
        let outcome = outcome.unwrap_or(JoinOutcome::Failed);
        if outcome == JoinOutcome::Connected {
            let shown = self.d.open == Open::Quick(QuickPage::WifiJoin);
            self.d.svc.wifi.joining = None;
            if shown {
                return Some(show(
                    self,
                    QuickPage::Wifi,
                    vec![Call::Networks, Call::Known],
                ));
            }
            return Some(service(Some(Call::Networks)));
        }
        match self.d.svc.wifi.joining.as_mut() {
            Some(j) if j.ssid == ssid => {
                j.busy = false;
                j.failed = Some(outcome);
                Some(service(None))
            }
            _ if matches!(
                outcome,
                JoinOutcome::PasswordNeeded | JoinOutcome::WrongPassword
            ) =>
            {
                let security = self
                    .d
                    .svc
                    .wifi
                    .networks
                    .as_deref()
                    .and_then(|n| n.iter().find(|n| n.ssid == ssid))
                    .map_or(ultar_bus::messages::net::WifiSecurity::Wpa2Personal, |n| {
                        n.security
                    });
                self.d.svc.wifi.joining = Some(Joining {
                    ssid: ssid.to_owned(),
                    security,
                    remember: true,
                    busy: false,
                    failed: Some(outcome),
                });
                Some(show(self, QuickPage::WifiJoin, Vec::new()))
            }
            _ => None,
        }
    }
}
