//! The bus side of the screenshot service.

use crate::grab::{encode_png, grab, GrabError};
use crate::save::save;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use ultar_bus::messages::account_local::LocalUsers;
use ultar_bus::messages::capture::{Screenshot, ScreenshotReply, ShotOutcome, SERVICE};
use ultar_bus::messages::session::SessionState;
use ultar_bus::{Call, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_service::log::{Level, Record};
use ultar_svckit::serve::Done;
use ultar_svckit::{Bus, Service};

/// The screenshot service.
pub struct CaptureService {
    compositor: PathBuf,
    homes: PathBuf,
    user: Option<String>,
    uids: BTreeMap<String, u32>,
    /// Hand files to the person (`false` in tests that run as one user).
    chown: bool,
    busy: Arc<AtomicBool>,
}

fn reply(request: IncomingRequest, outcome: ShotOutcome) {
    let _ = request.reply(&ScreenshotReply { outcome });
}

impl CaptureService {
    /// Reads from the compositor at `compositor`, saves below `homes/<name>`.
    pub fn new(compositor: PathBuf, homes: PathBuf, chown: bool) -> Self {
        Self {
            compositor,
            homes,
            user: None,
            uids: BTreeMap::new(),
            chown,
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    fn shoot(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let Ok(call) = request.decode::<Screenshot>() else {
            let _ = request.fail(ErrorCode::BadPayload);
            return;
        };
        let Some(user) = self.user.clone() else {
            return reply(request, ShotOutcome::NoUser);
        };
        let uid = self.uids.get(&user).copied();
        if self.chown && uid.is_none() {
            return reply(request, ShotOutcome::NoUser);
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            return reply(request, ShotOutcome::Busy);
        }
        let busy = self.busy.clone();
        let socket = self.compositor.clone();
        let home = self.homes.join(&user);
        let owner = if self.chown { uid } else { None };
        let started = bus.later(move |_| {
            let outcome = match grab(&socket, call.output) {
                Err(GrabError::NoDisplay) => ShotOutcome::NoDisplay,
                Err(GrabError::Failed) => ShotOutcome::Failed,
                Ok(frame) => match encode_png(&frame).ok().map(|png| save(&home, owner, &png)) {
                    Some(Ok(path)) => ShotOutcome::Saved(path.display().to_string()),
                    _ => ShotOutcome::Failed,
                },
            };
            let saved = matches!(outcome, ShotOutcome::Saved(_));
            // Free again before the answer: a second press right after it is not
            // "busy".
            busy.store(false, Ordering::SeqCst);
            reply(request, outcome);
            let done: Done<Self> = Box::new(move |_: &mut Self, bus: &Bus<'_, Self>| {
                bus.log(Record::new(Level::Info, "captured.shot").flag("saved", saved));
            });
            done
        });
        if !started {
            self.busy.store(false, Ordering::SeqCst);
        }
    }
}

/// Only plain account names become folder names.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

impl Service for CaptureService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![SessionState::TOPIC, LocalUsers::TOPIC]
    }

    fn start(&mut self, _bus: &Bus<'_, Self>) {}

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        if request.method() == Screenshot::METHOD {
            self.shoot(request, bus);
        } else {
            let _ = request.fail(ErrorCode::UnknownMethod);
        }
    }

    fn event(&mut self, event: RawEvent, _bus: &Bus<'_, Self>) {
        if let Ok(state) = event.decode::<SessionState>() {
            self.user = state.user.filter(|u| plain(u));
        } else if let Ok(users) = event.decode::<LocalUsers>() {
            self.uids = users.users.into_iter().map(|u| (u.name, u.uid)).collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::plain;

    #[test]
    fn folder_names_are_plain() {
        assert!(plain("amal"));
        for bad in ["", "..", "a/b", "-x", "Amal"] {
            assert!(!plain(bad), "{bad}");
        }
    }
}
