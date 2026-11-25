//! The phone's system dialogs on the bus: they offer `sysdialog` (permission,
//! authenticate, bt_pair, handoff – the same contract as the desktop's `ultar-sysdialog`),
//! follow the settings service's pending display changes and the Bluetooth service's
//! pairing, ask the authentication service and answer each call.

use std::path::Path;
use std::time::Duration;
use ultar_bus::messages::auth::{self, Method, Outcome, Secret, UserInfo};
use ultar_bus::messages::bluetooth::{BluetoothState, PairConfirm, PairConfirmReply, PairPhase};
use ultar_bus::messages::mesh_dialog::{AskHandoff, HandoffDecision};
use ultar_bus::messages::settings::{Confirm, PendingConfirmation, PendingResolved, Revert, Set};
use ultar_bus::messages::sysdialog::{
    AskPermission, Authenticate, AuthenticateReply, Decision, PermissionReply, SERVICE,
};
use ultar_bus::{Client, ClientOptions, IncomingRequest, ServiceEndpoint, TypedSubscription};
use ultar_dialog_core::{Answer, Question};

/// How long one call may take.
const CALL: Duration = Duration::from_secs(3);
/// `auth.verify` may take longer (the hash is slow on purpose).
const VERIFY: Duration = Duration::from_secs(10);

/// What came from the bus in one turn.
#[derive(Default)]
pub struct Incoming {
    /// New questions, with the call to answer (none for "keep?").
    pub questions: Vec<(Question, Option<IncomingRequest>)>,
    /// Display changes the settings service resolved itself.
    pub resolved: Vec<String>,
    /// The Bluetooth service's pairing is no longer running.
    pub pairing_over: bool,
}

/// The dialog program's bus connection.
pub struct DialogLink {
    client: Client,
    endpoint: Option<ServiceEndpoint>,
    pending: Option<TypedSubscription<PendingConfirmation>>,
    resolved: Option<TypedSubscription<PendingResolved>>,
    bluetooth: Option<TypedSubscription<BluetoothState>>,
}

impl std::fmt::Debug for DialogLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogLink").finish_non_exhaustive()
    }
}

impl DialogLink {
    /// Connects and offers `sysdialog`; `None` without a bus.
    pub fn connect(path: &Path, name: &str) -> Option<DialogLink> {
        let client = Client::connect(path, ClientOptions::named(name)).ok()?;
        Some(DialogLink {
            endpoint: client.register(SERVICE).ok(),
            pending: client.subscribe().ok(),
            resolved: client.subscribe().ok(),
            bluetooth: client.subscribe().ok(),
            client,
        })
    }

    /// Takes what arrived; methods it does not know are refused.
    pub fn poll(&self) -> Incoming {
        let mut out = Incoming::default();
        while let Some(Ok(Some(request))) = self.endpoint.as_ref().map(|e| e.try_recv()) {
            match decode(&request) {
                Some(question) => out.questions.push((question, Some(request))),
                None => {
                    let _ = request.fail(ultar_bus::ErrorCode::UnknownMethod);
                }
            }
        }
        while let Some(Ok(Some(p))) = self.pending.as_ref().map(|s| s.try_recv()) {
            if p.key.starts_with("display.") {
                let keep = Question::Keep {
                    key: p.key,
                    value: p.value,
                };
                out.questions.push((keep, None));
            }
        }
        while let Some(Ok(Some(r))) = self.resolved.as_ref().map(|s| s.try_recv()) {
            out.resolved.push(r.key);
        }
        while let Some(Ok(Some(b))) = self.bluetooth.as_ref().map(|s| s.try_recv()) {
            out.pairing_over |= !b.pairing.is_some_and(|p| p.phase == PairPhase::Running);
        }
        out
    }

    /// The administrator (the first account with the right), if the service answers.
    pub fn admin(&self) -> Option<UserInfo> {
        let reply = self.client.call(&auth::Users {}, CALL).ok()?;
        reply.users.into_iter().find(|u| u.admin)
    }

    /// One check off the loop over the same connection (the hash takes its time).
    pub fn verifier(&self) -> impl FnOnce(String, Secret) -> Option<Outcome> + Send {
        let client = self.client.clone();
        move |user, secret| {
            let request = auth::Verify {
                user,
                method: Method::Password,
                secret,
            };
            client.call(&request, VERIFY).ok().map(|r| r.outcome)
        }
    }

    /// `settings.confirm` of a display change.
    pub fn confirm(&self, key: &str) {
        let key = Some(key.to_owned());
        let _ = self.client.call(&Confirm { key }, CALL);
    }

    /// `settings.revert` of a display change.
    pub fn revert(&self, key: &str) {
        let key = Some(key.to_owned());
        let _ = self.client.call(&Revert { key }, CALL);
    }

    /// Sets the provisional value again (the settings service's timer restarts).
    pub fn set(&self, key: &str, value: ultar_bus::messages::settings::SettingValue) {
        let key = key.to_owned();
        let _ = self.client.call(&Set { key, value }, CALL);
    }
}

/// The question a call asks; `None` for another method.
fn decode(request: &IncomingRequest) -> Option<Question> {
    if let Ok(p) = request.decode::<AskPermission>() {
        Some(Question::Permission(p))
    } else if let Ok(p) = request.decode::<PairConfirm>() {
        Some(Question::BtPair(p))
    } else if let Ok(p) = request.decode::<AskHandoff>() {
        Some(Question::Handoff(p))
    } else if let Ok(a) = request.decode::<Authenticate>() {
        Some(Question::Authenticate(a))
    } else {
        None
    }
}

/// Answers `call` with `answer` in the reply type of its method.
pub fn reply(call: IncomingRequest, answer: Answer) {
    let _ = match answer {
        Answer::Permission(decision) => call.reply(&PermissionReply { decision }),
        Answer::Authenticate(result) => call.reply(&AuthenticateReply { result }),
        Answer::Keep(_) => call.reply(&PermissionReply {
            decision: Decision::Deny,
        }),
        Answer::BtPair(accept) => call.reply(&PairConfirmReply { accept }),
        Answer::Handoff(accept) => call.reply(&HandoffDecision { accept }),
    };
}
