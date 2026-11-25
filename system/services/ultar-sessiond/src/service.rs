//! The bus side of the session service.

use crate::parental::{self, Due, Parental};
use crate::sessions::Sessions;
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::account_local::{LocalUsers, SetSession};
use ultar_bus::messages::auth::Users as AuthUsers;
use ultar_bus::messages::lock::Lock;
use ultar_bus::messages::session::{
    SessionOutcome, SessionReply, SessionUser, SignOut, SignedIn, SwitchUser, Users, UsersReply,
    SERVICE,
};
use ultar_bus::messages::settings::Changed;
use ultar_bus::{Call, Client, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_service::log::{Level, Record};
use ultar_svckit::serve::Done;
use ultar_svckit::{store, Access, Bus, Service};

/// Pseudo method of the sign-in screen: the only identity that reports sign-ins.
pub const SIGNIN_METHOD: &str = "signin";
/// Longest wait for another service.
pub(crate) const WAIT: Duration = Duration::from_secs(5);
/// How often the parental window is checked (and the settings read again until the
/// settings service answers).
pub const TICK: Duration = Duration::from_secs(30);

/// The session service.
pub struct SessionService {
    pub(crate) sessions: Sessions,
    access: Access,
    policy: Option<ultar_bus::Policy>,
    state_file: PathBuf,
    /// `users.parental`: the time window of standard users.
    pub(crate) parental: Parental,
    /// The settings service answered at least once.
    pub(crate) settings_known: bool,
    /// Standard users already warned about the end of the window (once per window).
    pub(crate) warned: Vec<String>,
}

fn reply(request: IncomingRequest, outcome: SessionOutcome) {
    let _ = request.reply(&SessionReply { outcome });
}

/// The accounts: name, display name, administrator.
pub(crate) fn accounts(client: &Client) -> Option<Vec<(String, String, bool)>> {
    let users = client.call(&AuthUsers {}, WAIT).ok()?.users;
    Some(
        users
            .into_iter()
            .map(|u| (u.name, u.display_name, u.admin))
            .collect(),
    )
}

pub(crate) fn lock(client: &Client) -> bool {
    client.call(&Lock {}, WAIT).is_ok_and(|r| r.locked)
}

impl SessionService {
    /// Opens the state below `state_dir` (a damaged or missing file means nobody is
    /// signed in: fail closed, everyone signs in again).
    pub fn new(state_dir: PathBuf, policy: Option<ultar_bus::Policy>) -> Self {
        let state_file = state_dir.join("sessions");
        let sessions = store::load::<Sessions>(&state_file)
            .ok()
            .filter(Sessions::is_valid)
            .unwrap_or_default();
        Self {
            sessions,
            access: Access::new(SERVICE, policy.clone()),
            policy,
            state_file,
            parental: Parental::default(),
            settings_known: false,
            warned: Vec::new(),
        }
    }

    fn may_sign_in(&self, caller: &str) -> bool {
        let Some(policy) = &self.policy else {
            return false;
        };
        ultar_bus::Identity::new(caller)
            .is_ok_and(|id| policy.may_call(&id, SERVICE, SIGNIN_METHOD))
    }

    pub(crate) fn changed(&mut self, bus: &Bus<'_, Self>) {
        if store::save(&self.state_file, &self.sessions).is_err() {
            bus.log(Record::new(Level::Warn, "sessiond.state_not_saved"));
        }
        bus.retain(&self.sessions.state());
        let user = self.sessions.active().map(str::to_owned);
        // The account service learns who is in front (it never removes that person).
        bus.later(move |client| {
            let _ = client.call(&SetSession { user }, WAIT);
            let done: Done<Self> = Box::new(|_: &mut Self, _: &Bus<'_, Self>| {});
            done
        });
    }

    fn signed_in(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        if !self.may_sign_in(request.caller()) {
            return reply(request, SessionOutcome::Refused);
        }
        let Ok(call) = request.decode::<SignedIn>() else {
            let _ = request.fail(ErrorCode::BadPayload);
            return;
        };
        let started = bus.later(move |client| {
            let known = accounts(client);
            let done: Done<Self> = Box::new(move |s: &mut Self, bus: &Bus<'_, Self>| {
                let outcome = match known {
                    None => SessionOutcome::Unavailable,
                    Some(list) if !list.iter().any(|(n, _, _)| *n == call.user) => {
                        SessionOutcome::Unknown
                    }
                    Some(list) => {
                        // users.parental: a standard user signs in only in the window
                        // (the lock screen refuses too; this is the second check).
                        let admin = list
                            .iter()
                            .find(|(n, _, _)| *n == call.user)
                            .is_some_and(|(_, _, a)| *a);
                        if !admin && s.parental.due(parental::minute_now()) == Due::End {
                            SessionOutcome::Refused
                        } else {
                            match s.sessions.sign_in(&call.user) {
                                Ok(()) => {
                                    s.warned.retain(|w| *w != call.user);
                                    s.changed(bus);
                                    bus.log(Record::new(Level::Info, "sessiond.signed_in"));
                                    SessionOutcome::Done
                                }
                                Err(_) => SessionOutcome::Invalid,
                            }
                        }
                    }
                };
                reply(request, outcome);
            });
            done
        });
        if !started {
            // The request moved into the closure that did not start; the caller times out.
            bus.log(Record::new(Level::Warn, "sessiond.busy"));
        }
    }

    fn switch_user(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let Ok(call) = request.decode::<SwitchUser>() else {
            let _ = request.fail(ErrorCode::BadPayload);
            return;
        };
        bus.later(move |client| {
            let outcome = match accounts(client) {
                None => SessionOutcome::Unavailable,
                Some(list) if !list.iter().any(|(n, _, _)| *n == call.user) => {
                    SessionOutcome::Unknown
                }
                Some(_) if lock(client) => SessionOutcome::Done,
                Some(_) => SessionOutcome::Unavailable,
            };
            let done: Done<Self> = Box::new(move |s: &mut Self, bus: &Bus<'_, Self>| {
                if outcome == SessionOutcome::Done {
                    s.sessions.leave_front();
                    s.changed(bus);
                }
                reply(request, outcome);
            });
            done
        });
    }

    fn sign_out(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        if request.decode::<SignOut>().is_err() {
            let _ = request.fail(ErrorCode::BadPayload);
            return;
        }
        if self.sessions.active().is_none() {
            return reply(request, SessionOutcome::Invalid);
        }
        bus.later(move |client| {
            // The sign-in screen must cover the desktop before the session ends.
            let locked = lock(client);
            let done: Done<Self> = Box::new(move |s: &mut Self, bus: &Bus<'_, Self>| {
                if !locked {
                    return reply(request, SessionOutcome::Unavailable);
                }
                let outcome = match s.sessions.sign_out() {
                    Ok(_) => {
                        s.changed(bus);
                        bus.log(Record::new(Level::Info, "sessiond.signed_out"));
                        SessionOutcome::Done
                    }
                    Err(_) => SessionOutcome::Invalid,
                };
                reply(request, outcome);
            });
            done
        });
    }

    fn users(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let signed_in = self.sessions.signed_in().to_vec();
        let active = self.sessions.active().map(str::to_owned);
        bus.later(move |client| {
            let names = accounts(client).unwrap_or_default();
            let users = signed_in
                .into_iter()
                .map(|name| SessionUser {
                    display_name: names
                        .iter()
                        .find(|(n, _, _)| *n == name)
                        .map_or_else(|| name.clone(), |(_, d, _)| d.clone()),
                    active: active.as_deref() == Some(name.as_str()),
                    name,
                })
                .collect();
            let done: Done<Self> = Box::new(move |_: &mut Self, _: &Bus<'_, Self>| {
                let _ = request.reply(&UsersReply { users });
            });
            done
        });
    }
}

impl Service for SessionService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![LocalUsers::TOPIC, Changed::TOPIC]
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        bus.retain(&self.sessions.state());
        self.read_settings(bus);
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.read_settings(bus);
        self.enforce_parental(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let method = request.method().to_owned();
        if method == SignedIn::METHOD {
            return self.signed_in(request, bus);
        }
        let caller = request.caller().to_owned();
        if !self.access.is_system(&caller) && !self.access.is_admin(&caller) {
            bus.log(Record::new(Level::Warn, "sessiond.refused"));
            if method == Users::METHOD {
                let _ = request.fail(ErrorCode::PermissionDenied);
            } else {
                reply(request, SessionOutcome::Refused);
            }
            return;
        }
        match method.as_str() {
            m if m == SwitchUser::METHOD => self.switch_user(request, bus),
            m if m == SignOut::METHOD => self.sign_out(request, bus),
            m if m == Users::METHOD => self.users(request, bus),
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
            }
        }
    }

    fn event(&mut self, event: RawEvent, bus: &Bus<'_, Self>) {
        if event.topic == Changed::TOPIC {
            if let Ok(changed) = event.decode::<Changed>() {
                if self.parental.on_changed(&changed.key, &changed.value) {
                    self.warned.clear();
                    // A new window applies at once, not only at the next tick.
                    self.enforce_parental(bus);
                }
            }
            return;
        }
        let Ok(users) = event.decode::<LocalUsers>() else {
            return;
        };
        let names: Vec<String> = users.users.into_iter().map(|u| u.name).collect();
        let before = self.sessions.clone();
        self.sessions.keep_only(&names);
        if self.sessions != before {
            self.changed(bus);
        }
    }
}
