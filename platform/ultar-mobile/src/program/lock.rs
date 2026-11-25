//! `ultar-mobile-lock`: the phone's lock screen. It stays connected all session; on
//! `SessionLocked` it shows the cover (clock, waiting notifications, unlock), a tap on
//! unlock or a swipe up shows the sign-in, and only after `auth.verify` said yes – and the
//! shared lock state agreed that this person may lift this lock – it sends `Unlock`.
//!
//! The decisions are the shared lock state's (`ultar-lock-core`, the same as the desktop's
//! lock screen); this program only wires them to the compositor, the bus and the views.

use super::lock_bus::LockLink;
use super::{local_now, now_ms, screen_size, zone_offset, Options, ProgramError};
use crate::click::Click;
use crate::model::apps;
use crate::model::Mobile;
use crate::view::{self, lock::Cover, Built};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use ultar_bus::messages::auth::{Outcome, Secret};
use ultar_bus::messages::lock::LockReply;
use ultar_bus::IncomingRequest;
use ultar_i18n::Translator;
use ultar_lock_core::{LockEffect, LockState, Stage};
use ultar_shell_host::{Host, HostEvent};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{Event, Request, Role, SurfaceId};
use ultar_ui::event::{ActionKind, Key};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};

/// Application id of the phone's lock screen.
pub const APP_ID: &str = "org.ultar.mobile-lock";

/// The running lock screen.
pub struct LockProgram {
    host: Host<Ui>,
    /// The shared lock state (tests).
    pub state: LockState,
    env: PaintEnv,
    tr: Translator,
    link: Option<LockLink>,
    surface: Option<SurfaceId>,
    built: Built,
    waiting: Vec<IncomingRequest>,
    verify: Option<Receiver<Option<Outcome>>>,
    /// The last frame showed a wait after a wrong try (one more frame when it ends).
    wait_shown: bool,
    /// The minute the cover shows.
    minute: u64,
    /// The torch: `None` without one, else whether it is on.
    torch: Option<bool>,
    /// This lock already asked whether the phone has any account.
    accounts_checked: bool,
}

impl std::fmt::Debug for LockProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockProgram")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl LockProgram {
    /// Connects: the bus first (once the compositor knows a lock provider, `lock.lock` is
    /// served), then the compositor.
    pub fn start(options: &Options) -> Result<LockProgram, ProgramError> {
        let link = options
            .bus
            .as_deref()
            .and_then(|p| LockLink::connect(p, APP_ID));
        let host = Host::connect(&options.compositor, APP_ID)?;
        let theme = if options.light {
            ThemeKind::Light
        } else {
            ThemeKind::Dark
        };
        Ok(LockProgram {
            host,
            state: LockState {
                owner: options.user.clone(),
                ..LockState::default()
            },
            env: PaintEnv::new(options.lang, theme),
            tr: crate::i18n::translator_or_keys(options.lang),
            link,
            surface: None,
            built: Built::default(),
            waiting: Vec::new(),
            verify: None,
            wait_shown: false,
            minute: 0,
            torch: None,
            accounts_checked: false,
        })
    }

    /// Runs until `stop`.
    pub fn run(&mut self, stop: &dyn Fn() -> bool) -> Result<(), ProgramError> {
        while !stop() {
            self.turn()?;
        }
        Ok(())
    }

    /// One turn.
    pub fn turn(&mut self) -> Result<(), ProgramError> {
        let tick = if self.state.stage == Stage::Hidden {
            500
        } else {
            250
        };
        for event in self.host.dispatch(Some(Duration::from_millis(tick)))? {
            self.event(event)?;
        }
        self.bus()?;
        if let Some(outcome) = self.verify.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.verify = None;
            let effect = self.state.verified(outcome, now_ms());
            self.effect(effect)?;
            self.show()?;
        }
        let waiting = self.state.stage == Stage::SignIn && self.state.wait_ms(now_ms()) > 0;
        let minute = now_ms() / 60_000;
        let new_minute = self.state.stage == Stage::Cover && minute != self.minute;
        if waiting || self.wait_shown || new_minute {
            // The countdown, its end, and the cover's clock.
            self.minute = minute;
            self.show()?;
        }
        self.wait_shown = waiting;
        self.host.draw()?;
        self.answer_waiting();
        self.open_without_accounts()
    }

    fn bus(&mut self) -> Result<(), ProgramError> {
        let Some(link) = self.link.as_mut() else {
            return Ok(());
        };
        let notes = link.poll_notes();
        let mut requests = Vec::new();
        while let Some(r) = link.lock_request() {
            requests.push(r);
        }
        if !requests.is_empty() {
            self.state.locked();
            self.torch = link.torch(None);
            self.waiting.extend(requests);
            self.show()?;
        } else if notes && self.state.stage == Stage::Cover {
            self.show()?;
        }
        Ok(())
    }

    /// `lock.lock` is answered once the lock surface has its first frame (that frame locks
    /// the compositor).
    fn answer_waiting(&mut self) {
        let drawn = self
            .surface
            .and_then(|id| self.host.surface(id))
            .is_some_and(|s| s.window.configure().is_some());
        if drawn || self.state.stage == Stage::Hidden {
            for r in self.waiting.drain(..) {
                let _ = r.reply(&LockReply { locked: drawn });
            }
        }
    }

    fn event(&mut self, event: HostEvent) -> Result<(), ProgramError> {
        match event {
            HostEvent::Compositor(Event::SessionLocked { locked: true }) => {
                self.state.locked();
                self.torch = self.link.as_ref().and_then(|l| l.torch(None));
                self.show()
            }
            HostEvent::Compositor(Event::SessionLocked { locked: false }) => {
                self.state.unlocked();
                self.show()
            }
            HostEvent::Action(_, action) => self.action(action.node, &action.kind),
            HostEvent::Key(_, key, _) => {
                match (self.state.stage, key) {
                    (Stage::Cover, _) => self.sign_in(),
                    (Stage::SignIn, Key::Escape) => self.state.back(),
                    _ => return Ok(()),
                }
                self.show()
            }
            HostEvent::Resized(..) => self.show(),
            // A swipe up on the cover shows the sign-in.
            HostEvent::Swipe(_, delta) if -delta.y >= delta.x.abs() => {
                self.sign_in();
                self.show()
            }
            _ => Ok(()),
        }
    }

    fn action(&mut self, node: NodeId, kind: &ActionKind) -> Result<(), ProgramError> {
        let field = self.built.password == Some(node);
        match (self.built.click(node).cloned(), kind) {
            (Some(Click::Unlock), ActionKind::Activated) => self.sign_in(),
            (Some(Click::Torch), ActionKind::Activated | ActionKind::Toggled(_)) => {
                let want = !self.torch.unwrap_or(false);
                self.torch = self.link.as_ref().and_then(|l| l.torch(Some(want)));
            }
            (Some(Click::Submit), ActionKind::Activated) => return self.submit(),
            (None, ActionKind::Submitted) if field => return self.submit(),
            (Some(Click::Method), ActionKind::Activated) => self.state.switch_method(),
            (Some(Click::SignInBack), ActionKind::Activated) => self.state.back(),
            _ => return Ok(()),
        }
        self.show()
    }

    /// Where the node of `click` is on the screen (points; tests).
    pub fn locate(&self, click: &Click) -> Option<ultar_ui::geom::Rect> {
        let node = self.built.node(click)?;
        self.host.view(self.surface?)?.rect(node)
    }

    /// How many characters the password field holds (tests; never the characters).
    pub fn typed(&self) -> usize {
        let field = self.built.password;
        self.surface
            .and_then(|id| self.host.view(id))
            .zip(field)
            .and_then(|(ui, f)| ui.text(f).ok().map(|t| t.chars().count()))
            .unwrap_or(0)
    }

    /// Where the password field is (tests).
    pub fn field(&self) -> Option<ultar_ui::geom::Rect> {
        self.host.view(self.surface?)?.rect(self.built.password?)
    }

    fn sign_in(&mut self) {
        self.state.show_sign_in();
        if let Some(users) = self.link.as_ref().and_then(LockLink::users) {
            let wanted = self.state.owner.clone();
            self.state.user = users
                .iter()
                .find(|u| Some(&u.name) == wanted.as_ref())
                .or_else(|| users.first())
                .cloned();
            self.state.users = users;
            if self.state.user.is_some()
                && self.state.error == Some(ultar_lock_core::SignInError::NoUser)
            {
                self.state.error = None;
            }
        }
    }

    /// A phone set up without any account (the A30 image has no installer) has nobody to sign
    /// in: the authentication service answers with an empty list – not an error, not a damaged
    /// store – and the lock opens. Any account, or no answer, keeps it closed.
    fn open_without_accounts(&mut self) -> Result<(), ProgramError> {
        if self.state.stage == Stage::Hidden {
            self.accounts_checked = false;
            return Ok(());
        }
        let drawn = self
            .surface
            .and_then(|id| self.host.surface(id))
            .is_some_and(|s| s.window.configure().is_some());
        if self.accounts_checked || !drawn {
            return Ok(());
        }
        self.accounts_checked = true;
        let none = self
            .link
            .as_ref()
            .and_then(LockLink::users)
            .is_some_and(|users| users.is_empty());
        if none {
            self.effect(Some(LockEffect::Unlock))?;
            return self.show();
        }
        Ok(())
    }

    /// Takes the typed secret out of the field (the field is wiped) and checks it.
    fn submit(&mut self) -> Result<(), ProgramError> {
        let (Some(id), Some(field)) = (self.surface, self.built.password) else {
            return Ok(());
        };
        let Some(ui) = self.host.view_mut(id) else {
            return Ok(());
        };
        let secret = Secret::new(ui.text(field).unwrap_or_default().to_owned());
        let _ = ui.set_text(field, "");
        let minute = local_now(zone_offset(super::tz::DEFAULT))
            .map_or(12 * 60, |t| u16::from(t.hour) * 60 + u16::from(t.minute));
        let effect = self.state.submit(secret, now_ms(), minute);
        self.effect(effect)?;
        self.show()
    }

    fn effect(&mut self, effect: Option<LockEffect>) -> Result<(), ProgramError> {
        match effect {
            Some(LockEffect::Verify {
                user,
                method,
                secret,
            }) => {
                let (tx, rx) = channel();
                let check = self.link.as_ref().map(LockLink::verifier);
                std::thread::spawn(move || {
                    let outcome = check.and_then(|c| c(user, method, secret));
                    let _ = tx.send(outcome);
                });
                self.verify = Some(rx);
            }
            Some(LockEffect::Unlock) => {
                if let Some(surface) = self.surface {
                    self.host.client.send(&Request::Unlock { surface })?;
                }
                // Hidden at once: every frame of a lock surface locks again (COMPOSITOR.md §8a).
                self.state.unlocked();
                if let (Some(link), Some(user)) = (&self.link, self.state.user.as_ref()) {
                    link.signed_in(&user.name);
                }
            }
            Some(LockEffect::Layout(index)) => {
                self.host
                    .client
                    .send(&Request::SetKeyboardLayout { index })?;
            }
            None => {}
        }
        Ok(())
    }

    /// Shows the current stage (or removes the surface when hidden).
    fn show(&mut self) -> Result<(), ProgramError> {
        self.state.caps_lock = self.host.caps_lock();
        if self.state.stage == Stage::Hidden {
            if let Some(id) = self.surface.take() {
                self.host.close(id)?;
            }
            self.built = Built::default();
            return Ok(());
        }
        let size = self
            .surface
            .and_then(|id| self.host.view(id))
            .map_or_else(|| screen_size(self.host.client.outputs()), |ui| ui.size());
        let mut ui = Ui::new(self.env, InputMode::Touch, size, self.tr.clone())?;
        let all = apps::system_apps();
        let counts = self.link.as_ref().map(LockLink::counts).unwrap_or_default();
        let cover = Cover {
            now: local_now(zone_offset(super::tz::DEFAULT)),
            counts,
            torch: self.torch,
            apps: &all,
            camera: false,
            emergency: false,
        };
        let status = Mobile {
            now: cover.now,
            ..Mobile::default()
        };
        self.built = view::build_lock(&mut ui, &self.state, &cover, &status, now_ms())?;
        match self.surface {
            Some(id) => self.host.replace(id, ui, None),
            None => {
                let config = WindowConfig::with_role(Role::Lock, "", false);
                self.surface = Some(self.host.open(&config, ui, None)?);
            }
        }
        Ok(())
    }
}

/// Runs the phone's lock screen.
pub fn run(options: &Options, stop: &dyn Fn() -> bool) -> Result<(), ProgramError> {
    LockProgram::start(options)?.run(stop)
}
