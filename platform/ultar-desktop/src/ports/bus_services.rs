//! [`ServicesPort`] over the bus: one call per control, the outcome mapped to [`Answer`].

use super::bus::{BusPorts, CALL_TIMEOUT};
use super::services::{Answer, ServicesPort};
use super::PortError;
use crate::model::services::Call;
use std::time::Duration;
use ultar_bus::messages::{
    audio, bluetooth, calendar, call, capture, media, net, notify, power, session, settings,
    volumes, vpn,
};
use ultar_bus::{BusError, ErrorCode};

/// `net.join` answers within 30 s (contract); the shell waits a little longer.
const JOIN_TIMEOUT: Duration = Duration::from_secs(32);
/// `net.networks` answers within 15 s.
const SCAN_TIMEOUT: Duration = Duration::from_secs(16);
/// `vpn.set` (the tunnel is set up before the answer).
const VPN_TIMEOUT: Duration = Duration::from_secs(10);

fn port_error(e: BusError) -> PortError {
    match e {
        BusError::Remote(
            ErrorCode::PermissionDenied | ErrorCode::BadPayload | ErrorCode::Service(_),
        ) => PortError::Refused,
        _ => PortError::Unavailable,
    }
}

fn bt_done(r: bluetooth::BtReply) -> Answer {
    done(r.outcome == bluetooth::BtOutcome::Done)
}

fn done(ok: bool) -> Answer {
    if ok {
        Answer::Done
    } else {
        Answer::NotDone
    }
}

impl BusPorts {
    fn ask<C: ultar_bus::Call>(&self, request: &C) -> Result<C::Reply, PortError> {
        self.ask_within(request, CALL_TIMEOUT)
    }

    fn ask_within<C: ultar_bus::Call>(
        &self,
        request: &C,
        timeout: Duration,
    ) -> Result<C::Reply, PortError> {
        self.client().call(request, timeout).map_err(port_error)
    }

    /// Wi-Fi networks and the VPN (docs/integration/system-services.md §3).
    fn network(&self, call: &Call) -> Result<Answer, PortError> {
        let net_done = |r: net::NetReply| done(r.outcome == net::NetOutcome::Done);
        Ok(match call {
            Call::Networks => {
                let r = self.ask_within(&net::Networks {}, SCAN_TIMEOUT)?;
                match r.outcome {
                    net::NetOutcome::Done => Answer::Networks(r.networks),
                    _ => Answer::NotDone,
                }
            }
            Call::Known => Answer::Known(self.ask(&net::Known {})?.ssids),
            Call::Join(ssid, secret, remember) => {
                let join = net::Join {
                    ssid: ssid.clone(),
                    secret: secret.clone(),
                    remember: *remember,
                };
                Answer::Joined(self.ask_within(&join, JOIN_TIMEOUT)?.outcome)
            }
            Call::Disconnect => net_done(self.ask(&net::Disconnect {})?),
            Call::Forget(ssid) => net_done(self.ask(&net::Forget { ssid: ssid.clone() })?),
            Call::SetVpn(on, name) => {
                let set = vpn::SetVpn {
                    on: *on,
                    name: name.clone(),
                };
                done(self.ask_within(&set, VPN_TIMEOUT)?.outcome == vpn::VpnOutcome::Done)
            }
            _ => Answer::NotDone,
        })
    }

    fn radios(&self, call: &Call) -> Result<Answer, PortError> {
        let net_done = |r: net::NetReply| done(r.outcome == net::NetOutcome::Done);
        Ok(match call {
            Call::SetWifi(on) => net_done(self.ask(&net::SetWifi { on: *on })?),
            Call::SetAirplane(on) => net_done(self.ask(&net::SetAirplane { on: *on })?),
            Call::SetBluetooth(on) => bt_done(self.ask(&bluetooth::Set { on: *on })?),
            Call::BtScan(on) => bt_done(self.ask(&bluetooth::Scan { on: *on })?),
            Call::BtPair(id) => bt_done(self.ask(&bluetooth::Pair { id: id.clone() })?),
            Call::BtConnect(id, true) => bt_done(self.ask(&bluetooth::Connect { id: id.clone() })?),
            Call::BtConnect(id, false) => {
                bt_done(self.ask(&bluetooth::Disconnect { id: id.clone() })?)
            }
            Call::BtForget(id) => bt_done(self.ask(&bluetooth::Forget { id: id.clone() })?),
            Call::BtDevices => {
                let r = self.ask(&bluetooth::Devices {})?;
                if r.outcome == bluetooth::BtOutcome::Done {
                    Answer::BtDevices(r.devices)
                } else {
                    Answer::NotDone
                }
            }
            _ => Answer::NotDone,
        })
    }

    fn sound(&self, call: &Call) -> Result<Answer, PortError> {
        let audio_done = |r: audio::AudioReply| done(r.outcome == audio::AudioOutcome::Done);
        Ok(match call {
            Call::SetVolume(percent) => audio_done(self.ask(&audio::SetVolume {
                percent: (*percent).min(100),
            })?),
            Call::SetMute(on) => audio_done(self.ask(&audio::SetMute { on: *on })?),
            Call::SetOutput(id) => audio_done(self.ask(&audio::SetOutput { id: id.clone() })?),
            Call::Outputs => Answer::Outputs(
                self.ask(&audio::Outputs {})?
                    .outputs
                    .into_iter()
                    .map(|o| (o.id, o.name, o.active))
                    .collect(),
            ),
            _ => Answer::NotDone,
        })
    }

    fn system(&self, call: &Call) -> Result<Answer, PortError> {
        let power_done = |r: power::ActionReply| done(r.outcome == power::ActionOutcome::Done);
        Ok(match call {
            Call::Suspend => power_done(self.ask(&power::Suspend {})?),
            Call::Restart => power_done(self.ask(&power::Restart {})?),
            Call::PowerOff => power_done(self.ask(&power::PowerOff {})?),
            Call::SignOut => {
                let r = self.ask(&session::SignOut {})?;
                done(r.outcome == session::SessionOutcome::Done)
            }
            Call::Screenshot => match self.ask(&capture::Screenshot { output: None })?.outcome {
                capture::ShotOutcome::Saved(path) => Answer::Saved(path),
                _ => Answer::NotDone,
            },
            Call::Eject(id) => {
                let r = self.ask(&volumes::Eject { id: id.clone() })?;
                done(r.outcome == volumes::EjectOutcome::Done)
            }
            _ => Answer::NotDone,
        })
    }

    fn notes(&self, call: &Call) -> Result<Answer, PortError> {
        let note_done = |r: notify::NoteReply| done(r.outcome == notify::NoteOutcome::Done);
        Ok(match call {
            Call::NoteAction(id, action) => note_done(self.ask(&notify::Invoke {
                id: *id,
                action: action.clone(),
            })?),
            Call::NoteDismiss(id) => note_done(self.ask(&notify::Dismiss { id: *id })?),
            Call::Day(year, month, day) => {
                let date = calendar::Date {
                    year: *year,
                    month: *month,
                    day: *day,
                };
                Answer::Day(self.ask(&calendar::Day { date })?.events)
            }
            _ => Answer::NotDone,
        })
    }
}

/// What the lock screen asks of the system services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockCall {
    /// `session.signed_in {user}`: who signed in on the seat (after the password check).
    SignedIn(String),
    /// `power.off` (lock.power).
    PowerOff,
    /// `media.previous` (0), `media.play_pause` (1), `media.next` (2).
    Media(u8),
    /// `settings.set_accessibility {key, value}` (login.access; only `accessibility.*`).
    Accessibility(String, bool),
    /// The same call for `region.keyboard.active` (the input language chosen at the sign-in).
    Layout(String),
}

impl BusPorts {
    /// One call of the lock screen; `false` when the service could not.
    pub fn lock_call(&self, call: &LockCall) -> Result<bool, PortError> {
        Ok(match call {
            LockCall::SignedIn(user) => {
                let r = self.ask(&session::SignedIn { user: user.clone() })?;
                r.outcome == session::SessionOutcome::Done
            }
            LockCall::PowerOff => {
                self.ask(&power::PowerOff {})?.outcome == power::ActionOutcome::Done
            }
            LockCall::Media(button) => {
                let r = match button {
                    0 => self.ask(&media::Previous {})?,
                    2 => self.ask(&media::Next {})?,
                    _ => self.ask(&media::PlayPause {})?,
                };
                r.outcome == media::MediaOutcome::Done
            }
            LockCall::Accessibility(key, on) => {
                let call = settings::SetAccessibility {
                    key: key.clone(),
                    value: settings::SettingValue::Flag(*on),
                };
                self.ask(&call).is_ok()
            }
            LockCall::Layout(name) => {
                let call = settings::SetAccessibility {
                    key: crate::app::keyboard::ACTIVE.into(),
                    value: settings::SettingValue::Text(name.clone()),
                };
                self.ask(&call).is_ok()
            }
        })
    }

    /// The input languages from `settings.accessibility` (login.access answers them after the
    /// switches): the list and the active index, `None` without a usable answer. The Keyboard
    /// page's input languages win over the region's layouts when there are any.
    pub fn login_keyboard(&self) -> Result<Option<(Vec<String>, u8)>, PortError> {
        let reply = self.ask(&settings::Accessibility {})?;
        let text = |key: &str| {
            reply.values.iter().find_map(|(k, v)| match v {
                settings::SettingValue::Text(t) if k == key => Some(t.clone()),
                _ => None,
            })
        };
        let langs = reply
            .values
            .iter()
            .find_map(|(k, v)| match v {
                settings::SettingValue::List(l) if k == crate::app::keyboard::INPUT_LANGUAGES => {
                    Some(l.clone())
                }
                _ => None,
            })
            .unwrap_or_default();
        let layouts = text(crate::app::keyboard::LAYOUTS).unwrap_or_default();
        let active = text(crate::app::keyboard::ACTIVE);
        let list = crate::app::keyboard::combined(&langs, &layouts).join("_");
        Ok(crate::app::keyboard::from_values(list, active))
    }

    /// `call.emergency`: the call service reads and dials the configured emergency
    /// number itself (the lock screen never sends a number). The call's id, or `None`
    /// when it could not be placed (no call service, no modem, refused).
    pub fn emergency_call(&self) -> Option<u32> {
        self.ask(&call::Emergency {}).ok().map(|r| r.id)
    }

    /// `session.users`: who is signed in on the seat (empty at a fresh start – the only
    /// moment automatic sign-in may happen).
    pub fn session_users(&self) -> Result<Vec<String>, PortError> {
        let reply = self.ask(&session::Users {})?;
        Ok(reply.users.into_iter().map(|u| u.name).collect())
    }

    /// `settings.accessibility`: the flags under `accessibility.*` (login.access).
    pub fn accessibility(&self) -> Result<Vec<(String, bool)>, PortError> {
        let reply = self.ask(&settings::Accessibility {})?;
        Ok(reply
            .values
            .into_iter()
            .filter(|(k, _)| k.starts_with(settings::ACCESSIBILITY_PREFIX))
            .map(|(k, v)| (k, v == settings::SettingValue::Flag(true)))
            .collect())
    }
}

impl ServicesPort for BusPorts {
    fn call(&self, call: &Call) -> Result<Answer, PortError> {
        match call {
            Call::SetWifi(_)
            | Call::SetAirplane(_)
            | Call::SetBluetooth(_)
            | Call::BtDevices
            | Call::BtScan(_)
            | Call::BtPair(_)
            | Call::BtConnect(..)
            | Call::BtForget(_) => self.radios(call),
            Call::SetVolume(_) | Call::SetMute(_) | Call::SetOutput(_) | Call::Outputs => {
                self.sound(call)
            }
            Call::NoteAction(..) | Call::NoteDismiss(_) | Call::Day(..) => self.notes(call),
            Call::Suspend
            | Call::Restart
            | Call::PowerOff
            | Call::SignOut
            | Call::Screenshot
            | Call::Eject(_) => self.system(call),
            Call::Networks
            | Call::Known
            | Call::Join(..)
            | Call::Disconnect
            | Call::Forget(_)
            | Call::SetVpn(..) => self.network(call),
            Call::SetMesh(_) | Call::MeshSend(..) | Call::AskHelper(_) => self.mesh(call),
            Call::CallAnswer(..) | Call::CallEnd(_) | Call::CallMute(_) => self.calls(call),
        }
    }
}
