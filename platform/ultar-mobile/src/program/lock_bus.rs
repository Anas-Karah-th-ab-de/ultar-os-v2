//! The phone lock screen on the bus: it offers `lock.lock`, asks the authentication service
//! (`auth.users`, `auth.verify`), tells the session service who signed in, and counts the
//! waiting notifications per app (never their content).

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;
use ultar_bus::messages::auth::{self, Method, Outcome, Secret, UserInfo};
use ultar_bus::messages::lock::{Lock, SERVICE};
use ultar_bus::messages::{notify, power, session};
use ultar_bus::{Client, ClientOptions, IncomingRequest, ServiceEndpoint, TypedSubscription};

/// How long one call may take.
const CALL: Duration = Duration::from_secs(3);
/// `auth.verify` may take longer (the hash is slow on purpose).
const VERIFY: Duration = Duration::from_secs(10);

/// The lock screen's bus connection.
pub struct LockLink {
    client: Client,
    endpoint: Option<ServiceEndpoint>,
    posted: Option<TypedSubscription<notify::Posted>>,
    dismissed: Option<TypedSubscription<notify::Dismissed>>,
    /// Waiting notifications: id → app.
    notes: BTreeMap<u64, String>,
}

impl std::fmt::Debug for LockLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockLink").finish_non_exhaustive()
    }
}

impl LockLink {
    /// Connects, offers `lock` and follows the notifications; `None` without a bus.
    pub fn connect(path: &Path, name: &str) -> Option<LockLink> {
        let client = Client::connect(path, ClientOptions::named(name)).ok()?;
        let endpoint = client.register(SERVICE).ok();
        // Like the desktop's lock screen: only what is posted while it runs is counted
        // (its rights allow no list of notifications and no content).
        let posted = client.subscribe().ok();
        let dismissed = client.subscribe().ok();
        let notes = BTreeMap::new();
        Some(LockLink {
            client,
            endpoint,
            posted,
            dismissed,
            notes,
        })
    }

    /// Waiting notifications per app, by app id.
    pub fn counts(&self) -> Vec<(String, usize)> {
        let mut by_app: BTreeMap<&str, usize> = BTreeMap::new();
        for app in self.notes.values() {
            *by_app.entry(app.as_str()).or_default() += 1;
        }
        by_app.into_iter().map(|(a, n)| (a.to_owned(), n)).collect()
    }

    /// Takes new and dismissed notifications; `true` when the counts changed.
    pub fn poll_notes(&mut self) -> bool {
        let mut changed = false;
        while let Some(Ok(Some(p))) = self.posted.as_ref().map(|s| s.try_recv()) {
            self.notes
                .insert(p.id, super::bus::shell_id(&p.post.app_id));
            changed = true;
        }
        while let Some(Ok(Some(d))) = self.dismissed.as_ref().map(|s| s.try_recv()) {
            changed |= self.notes.remove(&d.id).is_some();
        }
        changed
    }

    /// The next `lock.lock` request; other methods are refused.
    pub fn lock_request(&self) -> Option<IncomingRequest> {
        loop {
            let request = self.endpoint.as_ref()?.try_recv().ok().flatten()?;
            if request.decode::<Lock>().is_ok() {
                return Some(request);
            }
            let _ = request.fail(ultar_bus::ErrorCode::UnknownMethod);
        }
    }

    /// Every account that can sign in.
    pub fn users(&self) -> Option<Vec<UserInfo>> {
        self.client
            .call(&auth::Users {}, CALL)
            .ok()
            .map(|r| r.users)
    }

    /// Switches the torch (`Some`) or asks (`None`): `None` without a torch.
    pub fn torch(&self, on: Option<bool>) -> Option<bool> {
        let r = self.client.call(&power::Torch { on }, CALL).ok()?;
        r.present.then_some(r.on)
    }

    /// The person who signed in (the session service keeps them in front).
    pub fn signed_in(&self, user: &str) {
        let _ = self.client.call(
            &session::SignedIn {
                user: user.to_owned(),
            },
            CALL,
        );
    }

    /// One check off the loop over the same connection (the hash takes its time).
    pub fn verifier(&self) -> impl FnOnce(String, Method, Secret) -> Option<Outcome> + Send {
        let client = self.client.clone();
        move |user, method, secret| {
            let request = auth::Verify {
                user,
                method,
                secret,
            };
            client.call(&request, VERIFY).ok().map(|r| r.outcome)
        }
    }
}
