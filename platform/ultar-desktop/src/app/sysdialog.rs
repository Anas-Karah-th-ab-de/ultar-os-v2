//! The system dialog program: answers `sysdialog.permission` / `sysdialog.authenticate`
//! after the user decided, and asks "keep these display settings?" for every pending
//! display change of the settings service – each in a `SystemDialog` surface, which the
//! compositor frames with its lock mark and the requester's name.

use super::{AppError, Options};
use crate::dialog::{view, Answer, DialogEffect, Dialogs, Question};
use crate::host::{Host, HostEvent};
use crate::model::time::{Clock, SystemClock};
use crate::ports::bus::BusPorts;
use crate::ports::{AuthPort, SettingsPort};
use crate::view::{Look, Target};
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::auth::{Method, Outcome as AuthOutcome, Secret};
use ultar_bus::messages::bluetooth::{BluetoothState, PairConfirm, PairConfirmReply};
use ultar_bus::messages::mesh::{AskHandoff, HandoffDecision};
use ultar_bus::messages::settings::{PendingConfirmation, PendingResolved};
use ultar_bus::messages::sysdialog::{
    AskPermission, Authenticate, AuthenticateReply, Decision, PermissionReply, SERVICE,
};
use ultar_bus::{Client, ClientOptions, IncomingRequest, ServiceEndpoint, TypedSubscription};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{Role, SurfaceId};
use ultar_ui::event::{ActionKind, Key};
use ultar_ui::geom::Size;
use ultar_ui::tokens::ThemeKind;

/// Application id of the dialog service.
pub const APP_ID: &str = "org.ultar.sysdialog";

/// The running dialog service.
pub struct DialogApp {
    host: Host,
    /// The questions (tests).
    pub dialogs: Dialogs,
    look: Look,
    shown: Option<(SurfaceId, u64)>,
    ports: Option<BusPorts>,
    endpoint: Option<ServiceEndpoint>,
    pending: Option<TypedSubscription<PendingConfirmation>>,
    resolved: Option<TypedSubscription<PendingResolved>>,
    /// `bluetooth.state`: a pairing that ends closes its "type this number" dialog.
    bluetooth: Option<TypedSubscription<BluetoothState>>,
    calls: BTreeMap<u64, IncomingRequest>,
    next_id: u64,
    verify: Option<Receiver<Option<AuthOutcome>>>,
    clock: Arc<dyn Clock>,
    shown_as: Option<(u64, u64, bool, u64)>,
}

impl std::fmt::Debug for DialogApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogApp")
            .field("dialogs", &self.dialogs.queue.len())
            .finish_non_exhaustive()
    }
}

impl DialogApp {
    /// Connects to the compositor and the bus.
    pub fn start(options: &Options, clock: Arc<dyn Clock>) -> Result<DialogApp, AppError> {
        let host = Host::connect(&options.compositor, APP_ID)?;
        let client = options
            .bus
            .as_deref()
            .and_then(|p| Client::connect(p, ClientOptions::named(APP_ID)).ok());
        let theme = if options.light {
            ThemeKind::Light
        } else {
            ThemeKind::Dark
        };
        Ok(DialogApp {
            host,
            dialogs: Dialogs::default(),
            look: Look::new(options.lang, theme),
            shown: None,
            endpoint: client.as_ref().and_then(|c| c.register(SERVICE).ok()),
            pending: client
                .as_ref()
                .and_then(|c| c.subscribe::<PendingConfirmation>().ok()),
            bluetooth: client
                .as_ref()
                .and_then(|c| c.subscribe::<BluetoothState>().ok()),
            resolved: client
                .as_ref()
                .and_then(|c| c.subscribe::<PendingResolved>().ok()),
            ports: client.map(BusPorts::new),
            calls: BTreeMap::new(),
            next_id: 1,
            verify: None,
            clock,
            shown_as: None,
        })
    }

    /// Runs until `stop`.
    pub fn run(&mut self, stop: &dyn Fn() -> bool) -> Result<(), AppError> {
        while !stop() {
            self.turn()?;
        }
        Ok(())
    }

    /// One turn.
    pub fn turn(&mut self) -> Result<(), AppError> {
        for event in self.host.dispatch(Some(Duration::from_millis(250)))? {
            self.event(event)?;
        }
        self.bus();
        if let Some(rx) = &self.verify {
            if let Ok(outcome) = rx.try_recv() {
                self.verify = None;
                let effects = self.dialogs.verified(outcome, self.clock.now_ms());
                self.run_effects(effects);
            }
        }
        let late = self.dialogs.tick(self.clock.now_ms());
        self.run_effects(late);
        self.show()?;
        self.host.draw()?;
        Ok(())
    }

    fn ask(&mut self, question: Question, call: Option<IncomingRequest>) {
        let id = self.next_id;
        self.next_id += 1;
        if let Some(call) = call {
            self.calls.insert(id, call);
        }
        let effects = self.dialogs.ask(id, question, self.clock.now_ms());
        self.run_effects(effects);
    }

    fn bus(&mut self) {
        while let Some(Ok(Some(request))) = self.endpoint.as_ref().map(|e| e.try_recv()) {
            if let Ok(p) = request.decode::<AskPermission>() {
                self.ask(Question::Permission(p), Some(request));
            } else if let Ok(p) = request.decode::<PairConfirm>() {
                self.ask(Question::BtPair(p), Some(request));
            } else if let Ok(p) = request.decode::<AskHandoff>() {
                self.ask(Question::Handoff(p), Some(request));
            } else if let Ok(a) = request.decode::<Authenticate>() {
                self.refresh_admin();
                self.ask(Question::Authenticate(a), Some(request));
            } else {
                let _ = request.fail(ultar_bus::ErrorCode::UnknownMethod);
            }
        }
        while let Some(Ok(Some(p))) = self.pending.as_ref().map(|s| s.try_recv()) {
            if p.key.starts_with("display.") {
                self.ask(
                    Question::Keep {
                        key: p.key,
                        value: p.value,
                    },
                    None,
                );
            }
        }
        while let Some(Ok(Some(r))) = self.resolved.as_ref().map(|s| s.try_recv()) {
            self.dialogs.resolved(&r.key);
        }
        while let Some(Ok(Some(b))) = self.bluetooth.as_ref().map(|s| s.try_recv()) {
            let running = b
                .pairing
                .is_some_and(|p| p.phase == ultar_bus::messages::bluetooth::PairPhase::Running);
            if !running {
                let effects = self.dialogs.pairing_over();
                self.run_effects(effects);
            }
        }
    }

    fn refresh_admin(&mut self) {
        if let Some(users) = self.ports.as_ref().and_then(|p| p.users().ok()) {
            self.dialogs.admin = users.into_iter().find(|u| u.admin);
        }
    }

    fn run_effects(&mut self, effects: Vec<DialogEffect>) {
        for effect in effects {
            self.run_one(effect);
        }
    }

    fn run_one(&mut self, effect: DialogEffect) {
        let ports = self.ports.clone();
        match effect {
            DialogEffect::Reply(id, answer) => {
                if let Some(call) = self.calls.remove(&id) {
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
            }
            DialogEffect::Verify { user, secret } => {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let outcome =
                        ports.and_then(|p| p.verify(&user, Method::Password, secret).ok());
                    let _ = tx.send(outcome);
                });
                self.verify = Some(rx);
            }
            DialogEffect::Confirm(key) => {
                let _ = ports.map(|p| p.confirm(&key));
            }
            DialogEffect::Revert(key) => {
                let _ = ports.map(|p| p.revert(&key));
            }
            DialogEffect::Again(key, value) => {
                let _ = ports.map(|p| p.set(&key, value));
            }
        }
    }

    fn event(&mut self, event: HostEvent) -> Result<(), AppError> {
        let effects = match event {
            HostEvent::Action(id, action) => self.action(id, action.node, &action.kind),
            HostEvent::Key(_, Key::Escape, _) => self.dialogs.dismiss(),
            _ => Vec::new(),
        };
        self.run_effects(effects);
        Ok(())
    }

    fn action(
        &mut self,
        id: SurfaceId,
        node: ultar_ui::ui::NodeId,
        kind: &ActionKind,
    ) -> Vec<DialogEffect> {
        let target = self.host.view(id).and_then(|v| v.target(node)).cloned();
        let keep = matches!(
            self.dialogs.current().map(|e| &e.question),
            Some(Question::Keep { .. })
        );
        let now = self.clock.now_ms();
        let pairing = matches!(
            self.dialogs.current().map(|e| &e.question),
            Some(Question::BtPair(_))
        );
        let offered = matches!(
            self.dialogs.current().map(|e| &e.question),
            Some(Question::Handoff(_))
        );
        match (target, kind) {
            (Some(Target::AllowOnce), ActionKind::Activated) if pairing => {
                self.dialogs.answer(Answer::BtPair(true))
            }
            (Some(Target::AllowOnce), ActionKind::Activated) if offered => {
                self.dialogs.answer(Answer::Handoff(true))
            }
            (Some(Target::AllowOnce), ActionKind::Activated) => {
                self.dialogs.answer(Answer::Permission(Decision::AllowOnce))
            }
            (Some(Target::AllowAlways), ActionKind::Activated) => self
                .dialogs
                .answer(Answer::Permission(Decision::AllowAlways)),
            (Some(Target::Deny), ActionKind::Activated) => self.dialogs.dismiss(),
            (Some(Target::MoreTime), ActionKind::Activated) => self.dialogs.more_time(now),
            (Some(Target::Confirm), ActionKind::Activated) if keep => {
                self.dialogs.answer(Answer::Keep(true))
            }
            (Some(Target::Confirm), ActionKind::Activated)
            | (Some(Target::Password), ActionKind::Submitted) => self.submit(id, now),
            _ => Vec::new(),
        }
    }

    /// Takes the password out of the field (wiped) for `auth.verify`.
    fn submit(&mut self, id: SurfaceId, now: u64) -> Vec<DialogEffect> {
        let Some(view) = self.host.view_mut(id) else {
            return Vec::new();
        };
        let Some(field) = view.node(&Target::Password) else {
            return Vec::new();
        };
        let secret = Secret::new(view.ui.text(field).unwrap_or_default().to_owned());
        let _ = view.ui.set_text(field, "");
        self.dialogs.submit_password(secret, now)
    }

    /// Shows the current question (a new surface for a new question: its frame title
    /// names the requester).
    fn show(&mut self) -> Result<(), AppError> {
        let now = self.clock.now_ms();
        let signature = self.dialogs.current().map(|e| {
            let remaining = e.deadline_ms.saturating_sub(now).div_ceil(1000);
            let remaining = if matches!(e.question, Question::Keep { .. }) {
                remaining
            } else {
                0
            };
            let wrong = self
                .dialogs
                .wrong_until
                .map_or(0, |t| t.saturating_sub(now).div_ceil(1000) + 1);
            (e.id, remaining, self.dialogs.checking, wrong)
        });
        if signature.is_some() && signature == self.shown_as {
            return Ok(());
        }
        self.shown_as = signature;
        let current = self.dialogs.current().map(|e| e.id);
        let tr = crate::i18n::translator_or_keys(self.look.lang);
        let built = view::build(&self.dialogs, self.clock.now_ms(), self.look, &tr)?;
        match (built, self.shown) {
            (Some(v), Some((surface, shown))) if Some(shown) == current => {
                let size = v.ui.size();
                let focus = self.host.view(surface).and_then(|old| old.ui.focused());
                self.host.replace(surface, v, Some(size));
                restore_focus(&mut self.host, surface, focus);
            }
            (Some(v), shown) => {
                if let Some((surface, _)) = shown {
                    self.host.close(surface)?;
                }
                let size: Size = v.ui.size();
                let title = view::frame_title(&self.dialogs, &tr);
                let config = WindowConfig::with_role(Role::SystemDialog, &title, false);
                let surface = self.host.open(&config, v, Some(size))?;
                self.shown = current.map(|c| (surface, c));
            }
            (None, Some((surface, _))) => {
                self.host.close(surface)?;
                self.shown = None;
            }
            (None, None) => {}
        }
        Ok(())
    }
}

/// Keeps the keyboard where it was when a dialog's tree is rebuilt (the countdown).
fn restore_focus(host: &mut Host, surface: SurfaceId, focus: Option<ultar_ui::ui::NodeId>) {
    if let (Some(view), Some(node)) = (host.view_mut(surface), focus) {
        let _ = view.ui.focus(node);
    }
}

/// Runs the dialog service on the system clock.
pub fn run(options: &Options, stop: &dyn Fn() -> bool) -> Result<(), AppError> {
    let mut app = DialogApp::start(options, Arc::new(SystemClock::default()))?;
    app.run(stop)
}

#[cfg(test)]
mod tests {
    #[test]
    fn auth_results_are_distinct() {
        use ultar_bus::messages::sysdialog::AuthResult;
        assert_ne!(AuthResult::Authorized, AuthResult::Cancelled);
    }
}
