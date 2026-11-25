//! `ultar-mobile-dialog`: the phone's system dialogs. It answers `sysdialog.permission`,
//! `sysdialog.authenticate` (administrator password through `auth.verify`),
//! `sysdialog.bt_pair` and `sysdialog.handoff` after the person decided, and asks "keep these
//! display settings?" for every pending display change – one question at a time, each in a
//! `SystemDialog` surface that the compositor frames with its trusted title bar. A question
//! nobody answers in time gets the safe answer.
//!
//! The decisions are the shared dialog state's (`ultar-dialog-core`, the same as the
//! desktop's `ultar-sysdialog`); this program wires them to the compositor, the bus and the
//! phone's sheet ([`crate::dialog`]).

use super::dialog_bus::{reply, DialogLink};
use super::{now_ms, screen_size, Options, ProgramError};
use crate::dialog::{self, Choice, DialogBuilt, DialogClick};
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use ultar_bus::messages::auth::{Outcome, Secret};
use ultar_bus::IncomingRequest;
use ultar_dialog_core::{DialogEffect, Dialogs, Question};
use ultar_i18n::Translator;
use ultar_shell_host::{Host, HostEvent};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{Role, SurfaceId};
use ultar_ui::event::{ActionKind, Key};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};

/// Application id of the phone's system dialogs.
pub const APP_ID: &str = "org.ultar.mobile-dialog";

/// What the shown tree was built from (rebuilt only when it changes).
type Signature = (u64, u64, bool, u64);

/// The running dialog program.
pub struct DialogProgram {
    host: Host<Ui>,
    /// The questions (tests).
    pub dialogs: Dialogs,
    env: PaintEnv,
    tr: Translator,
    link: Option<DialogLink>,
    calls: BTreeMap<u64, IncomingRequest>,
    next_id: u64,
    verify: Option<Receiver<Option<Outcome>>>,
    /// The surface and the question it shows.
    shown: Option<(SurfaceId, u64)>,
    shown_as: Option<Signature>,
    built: DialogBuilt,
}

impl std::fmt::Debug for DialogProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogProgram")
            .field("questions", &self.dialogs.queue.len())
            .finish_non_exhaustive()
    }
}

impl DialogProgram {
    /// Connects to the compositor and the bus.
    pub fn start(options: &Options) -> Result<DialogProgram, ProgramError> {
        let host = Host::connect(&options.compositor, APP_ID)?;
        let link = options
            .bus
            .as_deref()
            .and_then(|p| DialogLink::connect(p, APP_ID));
        let theme = if options.light {
            ThemeKind::Light
        } else {
            ThemeKind::Dark
        };
        Ok(DialogProgram {
            host,
            dialogs: Dialogs::default(),
            env: PaintEnv::new(options.lang, theme),
            tr: crate::i18n::translator_or_keys(options.lang),
            link,
            calls: BTreeMap::new(),
            next_id: 1,
            verify: None,
            shown: None,
            shown_as: None,
            built: DialogBuilt::default(),
        })
    }

    /// Runs until `stop`.
    pub fn run(&mut self, stop: &dyn Fn() -> bool) -> Result<(), ProgramError> {
        while !stop() {
            self.turn()?;
        }
        Ok(())
    }

    /// One turn: compositor events, the bus, the password check, deadlines, the view.
    pub fn turn(&mut self) -> Result<(), ProgramError> {
        for event in self.host.dispatch(Some(Duration::from_millis(250)))? {
            self.event(event);
        }
        self.bus();
        if let Some(outcome) = self.verify.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.verify = None;
            let effects = self.dialogs.verified(outcome, now_ms());
            self.run_effects(effects);
        }
        let late = self.dialogs.tick(now_ms());
        self.run_effects(late);
        self.show()?;
        self.host.draw()?;
        Ok(())
    }

    fn bus(&mut self) {
        let Some(incoming) = self.link.as_ref().map(DialogLink::poll) else {
            return;
        };
        for (question, call) in incoming.questions {
            if matches!(question, Question::Authenticate(_)) {
                self.dialogs.admin = self.link.as_ref().and_then(DialogLink::admin);
            }
            let id = self.next_id;
            self.next_id += 1;
            if let Some(call) = call {
                self.calls.insert(id, call);
            }
            let effects = self.dialogs.ask(id, question, now_ms());
            self.run_effects(effects);
        }
        for key in incoming.resolved {
            self.dialogs.resolved(&key);
        }
        if incoming.pairing_over {
            let effects = self.dialogs.pairing_over();
            self.run_effects(effects);
        }
    }

    fn run_effects(&mut self, effects: Vec<DialogEffect>) {
        for effect in effects {
            self.run_one(effect);
        }
    }

    fn run_one(&mut self, effect: DialogEffect) {
        let link = self.link.as_ref();
        match effect {
            DialogEffect::Reply(id, answer) => {
                if let Some(call) = self.calls.remove(&id) {
                    reply(call, answer);
                }
            }
            DialogEffect::Verify { user, secret } => {
                let (tx, rx) = channel();
                let check = link.map(DialogLink::verifier);
                std::thread::spawn(move || {
                    let _ = tx.send(check.and_then(|c| c(user, secret)));
                });
                self.verify = Some(rx);
            }
            DialogEffect::Confirm(key) => link.iter().for_each(|l| l.confirm(&key)),
            DialogEffect::Revert(key) => link.iter().for_each(|l| l.revert(&key)),
            DialogEffect::Again(key, value) => link.iter().for_each(|l| l.set(&key, value.clone())),
        }
    }

    fn event(&mut self, event: HostEvent) {
        let effects = match event {
            HostEvent::Action(_, action) => self.action(action.node, &action.kind),
            HostEvent::Key(_, Key::Escape, _) => self.dialogs.dismiss(),
            HostEvent::Resized(..) => {
                // Built again for the new size.
                self.shown_as = None;
                Vec::new()
            }
            _ => Vec::new(),
        };
        self.run_effects(effects);
    }

    fn action(&mut self, node: NodeId, kind: &ActionKind) -> Vec<DialogEffect> {
        let field = self.built.password == Some(node);
        let click = match (self.built.click(node), kind) {
            (Some(click), ActionKind::Activated) => click,
            (None, ActionKind::Submitted) if field => DialogClick::Authenticate,
            _ => return Vec::new(),
        };
        match dialog::choose(&mut self.dialogs, click, now_ms()) {
            Choice::Effects(effects) => effects,
            Choice::Submit => self.submit(),
        }
    }

    /// Takes the password out of the field (wiped) for `auth.verify`.
    fn submit(&mut self) -> Vec<DialogEffect> {
        let (Some((surface, _)), Some(field)) = (self.shown, self.built.password) else {
            return Vec::new();
        };
        let Some(ui) = self.host.view_mut(surface) else {
            return Vec::new();
        };
        let secret = Secret::new(ui.text(field).unwrap_or_default().to_owned());
        let _ = ui.set_text(field, "");
        self.dialogs.submit_password(secret, now_ms())
    }

    /// Where the node of `click` is on the dialog's surface (points; tests).
    pub fn locate(&self, click: DialogClick) -> Option<ultar_ui::geom::Rect> {
        let node = self.built.node(click)?;
        self.host.view(self.shown?.0)?.rect(node)
    }

    /// What the shown tree depends on: the question, the keep countdown, the check and the
    /// wait after a wrong password.
    fn signature(&self, now: u64) -> Option<Signature> {
        self.dialogs.current().map(|e| {
            let remaining = match e.question {
                Question::Keep { .. } => e.deadline_ms.saturating_sub(now).div_ceil(1000),
                _ => 0,
            };
            let wrong = self
                .dialogs
                .wrong_until
                .map_or(0, |t| t.saturating_sub(now).div_ceil(1000) + 1);
            (e.id, remaining, self.dialogs.checking, wrong)
        })
    }

    /// Shows the current question: a new surface for a new question (its frame title names
    /// the requester), the same surface while it changes, none when nothing is asked.
    fn show(&mut self) -> Result<(), ProgramError> {
        let now = now_ms();
        let signature = self.signature(now);
        if signature == self.shown_as {
            return Ok(());
        }
        self.shown_as = signature;
        let current = self.dialogs.current().map(|e| e.id);
        let same = self.shown.filter(|(_, q)| Some(*q) == current);
        if let Some((surface, _)) = self.shown.filter(|_| same.is_none()) {
            self.host.close(surface)?;
            self.shown = None;
            self.built = DialogBuilt::default();
        }
        if current.is_none() {
            return Ok(());
        }
        let size = same
            .and_then(|(s, _)| self.host.view(s))
            .map_or_else(|| screen_size(self.host.client.outputs()), |ui| ui.size());
        let mut ui = Ui::new(self.env, InputMode::Touch, size, self.tr.clone())?;
        ui.set_transparent_background();
        let Some(built) = dialog::build(&mut ui, &self.dialogs, now)? else {
            return Ok(());
        };
        self.built = built;
        match same {
            Some((surface, _)) => self.host.replace(surface, ui, None),
            None => {
                let title = dialog::frame_title(&self.dialogs, &self.tr);
                let config = WindowConfig::with_role(Role::SystemDialog, &title, true);
                let surface = self.host.open(&config, ui, None)?;
                self.shown = current.map(|c| (surface, c));
            }
        }
        Ok(())
    }
}

/// Runs the phone's system dialogs.
pub fn run(options: &Options, stop: &dyn Fn() -> bool) -> Result<(), ProgramError> {
    DialogProgram::start(options)?.run(stop)
}
