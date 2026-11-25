//! The Ultar account of this device (DESKTOP-SPEC §12.1): the link to the account server
//! (`ultraos-server`, default `ultar.code-craft.tech`).
//!
//! [`Link`] holds the state and decides; the server is a port ([`Server`], real one in
//! [`crate::link_cloud`]) and the files are in [`crate::link_store`]. The device stays fully
//! usable without an account (§12.5): nothing here is needed to start or use the device.
//!
//! * Signing in or creating an account registers this device with its own key and gets its
//!   Ultar number; the account token stays in this service's folder (`0600`), apps never see it.
//! * The device token for the Verbund relay is written to a file only the mesh service may read
//!   (`--relay-token`), and removed on sign-out.
//! * Only a refused password uses up one of [`SIGN_IN_ATTEMPTS`]; then sign-in waits
//!   [`LOCK_MINUTES`]. A server that cannot be reached leaves all attempts.
//! * Signed in and the server away: phase `Offline` with the last known state.

use crate::link_store::{Saved, Store};
use crate::refusal::{refuse, Key};
use std::time::{Duration, Instant};
use ultar_bus::messages::account_device::{
    DeviceAnswer, DeviceReply, CALL_SIGN_LABEL, MAX_CHALLENGE,
};
use ultar_bus::messages::account_service::{
    AccountService, AccountState, ChangePassword, Create, Delete, DeviceEntry, DeviceRemove,
    DeviceSignOut, Phase, Refusal, Reply, SetServer, SetService, SignIn, SignOut, Status,
    MAX_DEVICES, MAX_PASSWORD, MAX_SERVER, MAX_USER,
};
use ultar_bus::{Call, Text};

/// The server the account uses unless the person chose another (§12.1).
pub const DEFAULT_SERVER: &str = "ultar.code-craft.tech";

/// Sign-in attempts before the sign-in is locked for a while. Nothing but a refused password
/// uses one up: a server that cannot be reached leaves all of them (owner's VMware finding:
/// "0 attempts left" after the first try without a server).
pub const SIGN_IN_ATTEMPTS: u32 = 5;

/// Minutes a locked sign-in waits.
pub const LOCK_MINUTES: u32 = 15;

/// Why the server said no (or could not be asked).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    /// User name or password wrong.
    WrongPassword,
    /// The user name is taken.
    NameTaken,
    /// The server refused the form of the request (user name or password not allowed).
    Rejected,
    /// The server cannot be reached.
    Unreachable,
    /// Too many requests; the server asks to wait.
    Throttled,
    /// The token is no longer valid (password changed elsewhere, device removed).
    SessionEnded,
    /// This device may not manage the account's other devices (yet).
    NotManager,
    /// Anything else the server answered.
    Server,
}

/// What this device is registered as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enrolled {
    /// The Ultar number `UOS-XXXX-XXXX`.
    pub number: String,
    /// A device token (for the Verbund relay).
    pub device_token: String,
}

/// The account server as the link needs it. `server` is a host name (with `:port` if any).
pub trait Server {
    /// The server answers.
    fn health(&self, server: &str) -> Result<(), LinkError>;
    /// Account token for user and password.
    fn login(&self, server: &str, user: &str, password: &str) -> Result<String, LinkError>;
    /// Creates the account; its token.
    fn register(&self, server: &str, user: &str, password: &str) -> Result<String, LinkError>;
    /// Registers this device in the account (once; again later gives the same number) and
    /// opens a device session.
    fn enroll(&self, server: &str, token: &str, name: &str) -> Result<Enrolled, LinkError>;
    /// The account's devices, asked as this device.
    fn devices(&self, server: &str, token: &str) -> Result<Vec<RemoteDevice>, LinkError>;
    /// New password; the new account token.
    fn change_password(
        &self,
        server: &str,
        token: &str,
        old: &str,
        new: &str,
    ) -> Result<String, LinkError>;
    /// Removes another device from the account (its tokens end at once).
    fn remove_device(&self, server: &str, token: &str, device: &str) -> Result<(), LinkError>;
    /// The device key's first frame on the signalling socket.
    fn signal_hello(&self, server: &str) -> Result<String, LinkError>;
    /// The device key's answer to the signalling socket's challenge (only for `server`).
    fn signal_answer(&self, server: &str, challenge: &str) -> Result<String, LinkError>;
    /// The device's public key.
    fn device_key(&self) -> Result<[u8; 32], LinkError>;
    /// Signature over `message` with the device key.
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, LinkError>;
    /// TURN credentials: `host:port`, user, password.
    fn turn(&self, server: &str, token: &str) -> Result<(String, String, String), LinkError>;
}

/// One device of the account as the server lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteDevice {
    /// Device id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// It is this device.
    pub current: bool,
    /// Last contact, Unix seconds (`0` = unknown).
    pub last_seen: u64,
}

/// A call of `account.*` (decoded by the daemon).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkCall {
    /// `account.status`
    Status,
    /// `account.sign_in`
    SignIn(SignIn),
    /// `account.sign_out`
    SignOut,
    /// `account.create`
    Create(Create),
    /// `account.set_server`
    SetServer(SetServer),
    /// `account.set_service`
    SetService(SetService),
    /// `account.change_password`
    ChangePassword(ChangePassword),
    /// `account.delete`
    Delete(Delete),
    /// `account.device_sign_out`
    DeviceSignOut(DeviceSignOut),
    /// `account.device_remove`
    DeviceRemove(DeviceRemove),
}

/// True for the methods of the Ultar account (`account_service`).
pub fn is_link_method(method: &str) -> bool {
    [
        Status::METHOD,
        SignIn::METHOD,
        SignOut::METHOD,
        Create::METHOD,
        SetServer::METHOD,
        SetService::METHOD,
        ChangePassword::METHOD,
        Delete::METHOD,
        DeviceSignOut::METHOD,
        DeviceRemove::METHOD,
    ]
    .contains(&method)
}

/// The Ultar account link of this device.
pub struct Link<S: Server> {
    server: S,
    default_server: String,
    store: Store,
    device_name: String,
    kind: String,
    saved: Saved,
    token: Option<String>,
    devices: Vec<RemoteDevice>,
    phase: Phase,
    error: Option<Text>,
    attempts: u32,
    locked_until: Option<Instant>,
}

fn text(key: Key) -> Text {
    Text::key(key.key())
}

fn reason(e: &LinkError) -> Key {
    match e {
        LinkError::WrongPassword => Key::LinkWrongPassword,
        LinkError::NameTaken => Key::LinkNameTaken,
        LinkError::Rejected => Key::LinkRejected,
        LinkError::Unreachable => Key::Unreachable,
        LinkError::Throttled => Key::LinkThrottled,
        LinkError::SessionEnded => Key::SessionEnded,
        LinkError::NotManager => Key::NotManager,
        LinkError::Server => Key::ServerError,
    }
}

fn valid_host(server: &str) -> bool {
    !server.is_empty()
        && server.len() <= MAX_SERVER
        && server
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-.:[]".contains(&b))
}

impl<S: Server> Link<S> {
    /// The link as the files left it. `default_server` is used until the person sets another;
    /// `kind` is this device's kind word (`desktop`, `phone`…).
    pub fn open(
        server: S,
        default_server: &str,
        store: Store,
        device_name: &str,
        kind: &str,
    ) -> Self {
        let saved = store.load();
        let token = store.token();
        let phase = if token.is_some() {
            Phase::Offline
        } else {
            Phase::SignedOut
        };
        Self {
            server,
            default_server: default_server.to_owned(),
            store,
            device_name: device_name.to_owned(),
            kind: kind.to_owned(),
            saved,
            token,
            devices: Vec::new(),
            phase,
            error: None,
            attempts: SIGN_IN_ATTEMPTS,
            locked_until: None,
        }
    }

    fn host(&self) -> String {
        if self.saved.server.is_empty() {
            self.default_server.clone()
        } else {
            self.saved.server.clone()
        }
    }

    fn signed_in(&self) -> bool {
        self.token.is_some()
    }

    /// The account as this device knows it.
    pub fn state(&self, now: Instant) -> AccountState {
        let locked = self.locked_until.filter(|t| *t > now);
        let phase = if locked.is_some() && !self.signed_in() {
            Phase::Locked
        } else {
            self.phase
        };
        let devices = self
            .devices
            .iter()
            .take(MAX_DEVICES)
            .map(|d| DeviceEntry {
                id: d.id.clone(),
                name: d.name.clone(),
                kind: if d.current {
                    self.kind.clone()
                } else {
                    "desktop".into()
                },
                this_device: d.current,
                last_seen: if d.current { 0 } else { d.last_seen },
            })
            .collect();
        AccountState {
            phase,
            user: if self.signed_in() {
                self.saved.user.clone()
            } else {
                String::new()
            },
            server: self.host(),
            number: if self.signed_in() {
                self.saved.number.clone()
            } else {
                String::new()
            },
            services: if self.signed_in() {
                self.saved.services.clone()
            } else {
                Vec::new()
            },
            devices,
            error: self.error.clone(),
            attempts_remaining: self.attempts,
            locked_minutes: locked.map_or(0, |t| (t - now).as_secs().div_ceil(60) as u32),
        }
    }

    /// Asks the server again (at start and on `status` while offline): device session, relay
    /// token and the device list.
    pub fn refresh(&mut self) {
        let Some(token) = self.token.clone() else {
            return;
        };
        let host = self.host();
        let outcome = self
            .server
            .enroll(&host, &token, &self.device_name)
            .and_then(|e| {
                let list = self.server.devices(&host, &token)?;
                Ok((e, list))
            });
        match outcome {
            Ok((enrolled, list)) => {
                self.store.write_relay_token(&enrolled.device_token);
                self.saved.number = enrolled.number;
                self.store.save(&self.saved);
                self.devices = list;
                self.phase = Phase::SignedIn;
                self.error = None;
            }
            Err(LinkError::SessionEnded) => self.end_session(Some(Key::SessionEnded)),
            Err(e) => {
                self.phase = Phase::Offline;
                self.error = Some(text(reason(&e)));
            }
        }
    }

    fn end_session(&mut self, why: Option<Key>) {
        self.token = None;
        self.store.clear_token();
        self.store.clear_relay_token();
        self.devices.clear();
        self.phase = Phase::SignedOut;
        self.error = why.map(text);
    }

    /// Answers one call; `now` drives the sign-in lock.
    pub fn call(&mut self, call: LinkCall, now: Instant) -> Reply {
        let outcome = match call {
            LinkCall::Status => {
                if self.phase == Phase::Offline {
                    self.refresh();
                }
                Ok(())
            }
            LinkCall::SignIn(s) => self.sign_in(&s.user, &s.password, false, now),
            LinkCall::Create(c) => self.sign_in(&c.user, &c.password, true, now),
            LinkCall::SignOut => {
                self.end_session(None);
                Ok(())
            }
            LinkCall::SetServer(s) => self.set_server(&s.server),
            LinkCall::SetService(s) => self.set_service(s.service, s.on),
            LinkCall::ChangePassword(c) => self.change_password(&c.old, &c.new),
            // The server offers no deletion of an account; signing another device out is
            // removing it (its tokens end at once).
            LinkCall::Delete(_) => Err(refuse(Key::NotOffered)),
            LinkCall::DeviceSignOut(d) => self.remove_device(&d.device),
            LinkCall::DeviceRemove(d) => self.remove_device(&d.device),
        };
        Reply {
            outcome: outcome.map(|()| self.state(now)),
        }
    }

    fn sign_in(
        &mut self,
        user: &str,
        password: &str,
        create: bool,
        now: Instant,
    ) -> Result<(), Refusal> {
        if self.signed_in() {
            return Err(refuse(Key::AlreadySignedIn));
        }
        if self.locked_until.is_some_and(|t| t > now) {
            return Err(refuse(Key::Locked));
        }
        if user.is_empty() || user.len() > MAX_USER || password.len() > MAX_PASSWORD {
            return Err(refuse(Key::LinkRejected));
        }
        let host = self.host();
        let token = if create {
            self.server.register(&host, user, password)
        } else {
            self.server.login(&host, user, password)
        };
        let token = match token {
            Ok(t) => t,
            Err(e) => return Err(self.failed(e, now)),
        };
        let enrolled = match self.server.enroll(&host, &token, &self.device_name) {
            Ok(e) => e,
            Err(e) => {
                self.error = Some(text(reason(&e)));
                return Err(refuse(reason(&e)));
            }
        };
        self.attempts = SIGN_IN_ATTEMPTS;
        self.locked_until = None;
        self.saved.user = user.to_owned();
        self.saved.number = enrolled.number;
        if !self.store.save_token(&token) {
            return Err(refuse(Key::StoreFailed));
        }
        self.store.write_relay_token(&enrolled.device_token);
        self.store.save(&self.saved);
        self.token = Some(token.clone());
        self.devices = self.server.devices(&host, &token).unwrap_or_default();
        self.phase = Phase::SignedIn;
        self.error = None;
        Ok(())
    }

    fn failed(&mut self, e: LinkError, now: Instant) -> Refusal {
        if e == LinkError::WrongPassword {
            self.attempts = self.attempts.saturating_sub(1);
            if self.attempts == 0 {
                self.attempts = SIGN_IN_ATTEMPTS;
                self.locked_until = Some(now + Duration::from_secs(u64::from(LOCK_MINUTES) * 60));
            }
        }
        self.error = Some(text(reason(&e)));
        refuse(reason(&e))
    }

    fn set_server(&mut self, server: &str) -> Result<(), Refusal> {
        if self.signed_in() {
            return Err(refuse(Key::AlreadySignedIn));
        }
        let server = server.trim().to_ascii_lowercase();
        if !valid_host(&server) {
            return Err(refuse(Key::LinkRejected));
        }
        self.server
            .health(&server)
            .map_err(|e| refuse(reason(&e)))?;
        self.saved.server = if server == self.default_server {
            String::new()
        } else {
            server
        };
        self.store.save(&self.saved);
        self.error = None;
        Ok(())
    }

    fn set_service(&mut self, service: AccountService, on: bool) -> Result<(), Refusal> {
        if !self.signed_in() {
            return Err(refuse(Key::NeedsAccount));
        }
        self.saved.services.retain(|s| *s != service);
        if on {
            self.saved.services.push(service);
        }
        self.store.save(&self.saved);
        Ok(())
    }

    fn change_password(&mut self, old: &str, new: &str) -> Result<(), Refusal> {
        let token = self
            .token
            .clone()
            .ok_or_else(|| refuse(Key::NeedsAccount))?;
        if new.is_empty() || new.len() > MAX_PASSWORD {
            return Err(refuse(Key::LinkRejected));
        }
        match self.server.change_password(&self.host(), &token, old, new) {
            Ok(new_token) => {
                if !self.store.save_token(&new_token) {
                    return Err(refuse(Key::StoreFailed));
                }
                self.token = Some(new_token);
                // every earlier token ended, the relay's too
                self.refresh();
                Ok(())
            }
            Err(LinkError::SessionEnded) => {
                self.end_session(Some(Key::SessionEnded));
                Err(refuse(Key::SessionEnded))
            }
            Err(e) => Err(refuse(reason(&e))),
        }
    }

    fn remove_device(&mut self, device: &str) -> Result<(), Refusal> {
        let token = self
            .token
            .clone()
            .ok_or_else(|| refuse(Key::NeedsAccount))?;
        if self.devices.iter().any(|d| d.current && d.id == device) {
            // this device leaves with "sign out"
            return Err(refuse(Key::NotOffered));
        }
        let host = self.host();
        match self.server.remove_device(&host, &token, device) {
            Ok(()) => {
                self.devices = self.server.devices(&host, &token).unwrap_or_default();
                Ok(())
            }
            Err(LinkError::SessionEnded) => {
                self.end_session(Some(Key::SessionEnded));
                Err(refuse(Key::SessionEnded))
            }
            Err(e) => Err(refuse(reason(&e))),
        }
    }
}

impl<S: Server> Link<S> {
    /// Answers the call service (device key and relay); only while signed in.
    pub fn device(&mut self, call: DeviceCall) -> DeviceReply {
        let outcome = match self.token.clone() {
            None => Err(refuse(Key::NeedsAccount)),
            Some(token) => self.device_answer(call, &token).map_err(|e| {
                if e == LinkError::SessionEnded {
                    self.end_session(Some(Key::SessionEnded));
                }
                refuse(reason(&e))
            }),
        };
        DeviceReply { outcome }
    }

    fn device_answer(&self, call: DeviceCall, token: &str) -> Result<DeviceAnswer, LinkError> {
        let host = self.host();
        Ok(match call {
            DeviceCall::Hello => DeviceAnswer::Frame(self.server.signal_hello(&host)?),
            DeviceCall::Answer(c) if c.len() <= MAX_CHALLENGE => {
                DeviceAnswer::Frame(self.server.signal_answer(&host, &c)?)
            }
            DeviceCall::Answer(_) => return Err(LinkError::Rejected),
            DeviceCall::Key => DeviceAnswer::Key(self.server.device_key()?),
            DeviceCall::SignCall(t) => {
                let mut m = CALL_SIGN_LABEL.to_vec();
                m.extend_from_slice(&t);
                DeviceAnswer::Signature(self.server.sign(&m)?)
            }
            DeviceCall::Turn => {
                let (server, user, password) = self.server.turn(&host, token)?;
                DeviceAnswer::Turn {
                    server,
                    user,
                    password,
                }
            }
        })
    }
}

/// A call of the call service (decoded by the daemon).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceCall {
    /// `account.device_signal_hello`
    Hello,
    /// `account.device_signal_answer`
    Answer(String),
    /// `account.device_key`
    Key,
    /// `account.device_sign_call` (the transcript hash)
    SignCall([u8; 32]),
    /// `account.device_turn`
    Turn,
}

/// True for the call service's methods (`account_device`).
pub fn is_device_method(method: &str) -> bool {
    use ultar_bus::messages::account_device as d;
    [
        d::SignalHello::METHOD,
        d::SignalAnswer::METHOD,
        d::DeviceKey::METHOD,
        d::SignCall::METHOD,
        d::Turn::METHOD,
    ]
    .contains(&method)
}

/// The device call of a bus request (`account.device_*`), if it decodes.
pub fn decode_device(r: &ultar_bus::IncomingRequest) -> Option<DeviceCall> {
    use ultar_bus::messages::account_device as d;
    let m = r.method();
    Some(match m {
        _ if m == d::SignalHello::METHOD => DeviceCall::Hello,
        _ if m == d::SignalAnswer::METHOD => {
            DeviceCall::Answer(r.decode::<d::SignalAnswer>().ok()?.challenge)
        }
        _ if m == d::DeviceKey::METHOD => DeviceCall::Key,
        _ if m == d::SignCall::METHOD => {
            DeviceCall::SignCall(r.decode::<d::SignCall>().ok()?.transcript)
        }
        _ if m == d::Turn::METHOD => DeviceCall::Turn,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
