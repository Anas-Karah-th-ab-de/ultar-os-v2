//! The lock screen program: connected all session (the compositor locks only while a lock
//! screen client is there, COMPOSITOR.md §8a); shows itself when the session locks, locks on
//! `lock.lock`, checks the password with `auth.verify` on a worker thread and only then
//! sends `Unlock`.

use super::{AppError, Options};
use crate::host::{Host, HostEvent};
use crate::lock::{view, LockEffect, LockState, Stage};
use crate::model::notes::Notes;
use crate::model::time::{Clock, LocalTime, SystemClock};
use crate::ports::bus::BusPorts;
use crate::ports::AuthPort;
use crate::view::{Look, Target};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::auth::{Outcome as AuthOutcome, Secret};
use ultar_bus::messages::lock::{Lock, LockReply, SERVICE};
use ultar_bus::messages::notify::Posted;
use ultar_bus::{Client, ClientOptions, IncomingRequest, ServiceEndpoint, TypedSubscription};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{Event, Request, Role, SurfaceId};
use ultar_ui::event::{ActionKind, Key};
use ultar_ui::geom::Size;
use ultar_ui::tokens::ThemeKind;

/// Application id of the lock screen.
pub const APP_ID: &str = "org.ultar.lock";

/// The running lock screen.
pub struct LockApp {
    host: Host,
    /// The state (tests).
    pub state: LockState,
    look: Look,
    surface: Option<SurfaceId>,
    ports: Option<BusPorts>,
    endpoint: Option<ServiceEndpoint>,
    posted: Option<TypedSubscription<Posted>>,
    notes: Notes,
    waiting: Vec<IncomingRequest>,
    verify: Option<Receiver<Option<AuthOutcome>>>,
    clock: Arc<dyn Clock>,
    user: Option<String>,
}

impl std::fmt::Debug for LockApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockApp")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl LockApp {
    /// Connects; `user` is the session's user (the first account when `None`).
    pub fn start(
        options: &Options,
        user: Option<String>,
        clock: Arc<dyn Clock>,
    ) -> Result<LockApp, AppError> {
        // The bus first: once the compositor knows a lock provider, `lock.lock` is served.
        let client = options
            .bus
            .as_deref()
            .and_then(|p| Client::connect(p, ClientOptions::named(APP_ID)).ok());
        let endpoint = client.as_ref().and_then(|c| c.register(SERVICE).ok());
        let host = Host::connect(&options.compositor, APP_ID)?;
        let posted = client.as_ref().and_then(|c| c.subscribe::<Posted>().ok());
        let theme = if options.light {
            ThemeKind::Light
        } else {
            ThemeKind::Dark
        };
        Ok(LockApp {
            host,
            state: LockState::default(),
            look: Look::new(options.lang, theme),
            surface: None,
            ports: client.map(BusPorts::new),
            endpoint,
            posted,
            notes: Notes::default(),
            waiting: Vec::new(),
            verify: None,
            clock,
            user,
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
        let tick = if self.state.stage == Stage::Hidden {
            500
        } else {
            250
        };
        for event in self.host.dispatch(Some(Duration::from_millis(tick)))? {
            self.event(event)?;
        }
        self.bus()?;
        if let Some(rx) = &self.verify {
            if let Ok(outcome) = rx.try_recv() {
                self.verify = None;
                let now = self.clock.now_ms();
                let effect = self.state.verified(outcome, now);
                self.run_effect(effect)?;
                self.show()?;
            }
        }
        if self.state.stage == Stage::SignIn && self.state.wait_ms(self.clock.now_ms()) > 0 {
            self.show()?;
        }
        self.host.draw()?;
        self.answer_waiting();
        Ok(())
    }

    fn bus(&mut self) -> Result<(), AppError> {
        while let Some(Ok(Some(p))) = self.posted.as_ref().map(|s| s.try_recv()) {
            self.notes.add(p);
        }
        while let Some(Ok(Some(request))) = self.endpoint.as_ref().map(|e| e.try_recv()) {
            if request.decode::<Lock>().is_ok() {
                self.state.locked();
                self.show()?;
                self.waiting.push(request);
            } else {
                let _ = request.fail(ultar_bus::ErrorCode::UnknownMethod);
            }
        }
        Ok(())
    }

    /// `lock.lock` is answered once the lock surface has its first frame (that frame
    /// locks the compositor).
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

    fn event(&mut self, event: HostEvent) -> Result<(), AppError> {
        match event {
            HostEvent::Compositor(Event::SessionLocked { locked: true }) => {
                self.state.locked();
                self.show()
            }
            HostEvent::Compositor(Event::SessionLocked { locked: false }) => {
                self.state.unlocked();
                self.show()
            }
            HostEvent::Compositor(Event::KeyboardLayouts { names, active }) => {
                self.state.layouts = (names, active);
                self.show()
            }
            HostEvent::Action(_, action) => self.action(action.node, &action.kind),
            HostEvent::Key(_, key, _) => self.key(&key),
            HostEvent::Resized(..) => self.show(),
            _ => Ok(()),
        }
    }

    fn key(&mut self, key: &Key) -> Result<(), AppError> {
        match (self.state.stage, key) {
            (Stage::Cover, _) => self.state.show_sign_in(),
            (Stage::SignIn, Key::Escape) => self.state.back(),
            _ => return Ok(()),
        }
        self.refresh_user();
        self.show()
    }

    fn action(&mut self, node: ultar_ui::ui::NodeId, kind: &ActionKind) -> Result<(), AppError> {
        let Some(id) = self.surface else {
            return Ok(());
        };
        let target = self.host.view(id).and_then(|v| v.target(node)).cloned();
        match (target, kind) {
            (Some(Target::Unlock), ActionKind::Activated) => {
                self.state.show_sign_in();
                self.refresh_user();
            }
            (Some(Target::Submit), ActionKind::Activated)
            | (Some(Target::Password), ActionKind::Submitted) => self.submit()?,
            (Some(Target::Method), ActionKind::Activated) => self.state.switch_method(),
            (Some(Target::Back), ActionKind::Activated) => self.state.back(),
            (Some(Target::Keyboard), ActionKind::Activated) => {
                let (names, active) = &self.state.layouts;
                let next = (usize::from(*active) + 1) % names.len().max(1);
                self.run_effect(Some(LockEffect::Layout(next as u8)))?;
                return Ok(());
            }
            _ => return Ok(()),
        }
        self.show()
    }

    /// Takes the typed secret out of the field (the field is wiped) and checks it.
    fn submit(&mut self) -> Result<(), AppError> {
        let Some(id) = self.surface else {
            return Ok(());
        };
        let Some(view) = self.host.view_mut(id) else {
            return Ok(());
        };
        let Some(field) = view.node(&Target::Password) else {
            return Ok(());
        };
        let secret = Secret::new(view.ui.text(field).unwrap_or_default().to_owned());
        let _ = view.ui.set_text(field, "");
        let effect = self.state.submit(secret, self.clock.now_ms());
        self.run_effect(effect)
    }

    fn run_effect(&mut self, effect: Option<LockEffect>) -> Result<(), AppError> {
        match effect {
            Some(LockEffect::Verify {
                user,
                method,
                secret,
            }) => {
                let (tx, rx) = channel();
                let ports = self.ports.clone();
                std::thread::spawn(move || {
                    let outcome = ports.and_then(|p| p.verify(&user, method, secret).ok());
                    let _ = tx.send(outcome);
                });
                self.verify = Some(rx);
            }
            Some(LockEffect::Unlock) => {
                if let Some(surface) = self.surface {
                    self.host.client.send(&Request::Unlock { surface })?;
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

    fn refresh_user(&mut self) {
        let Some(ports) = &self.ports else {
            return;
        };
        if let Ok(users) = ports.users() {
            let wanted = self.user.clone();
            self.state.user = users
                .iter()
                .find(|u| Some(&u.name) == wanted.as_ref())
                .or_else(|| users.first())
                .cloned();
            self.state.users = users;
        }
    }

    fn screen(&self) -> Size {
        super::primary(self.host.client.outputs()).map_or(Size::new(1280.0, 800.0), |o| {
            let s = o.scale_milli.max(1) as f32 / 1000.0;
            Size::new(o.width as f32 / s, o.height as f32 / s)
        })
    }

    /// Shows the current stage (or removes the surface when hidden).
    fn show(&mut self) -> Result<(), AppError> {
        self.state.caps_lock = self.host.caps_lock();
        self.state.notes = self.notes.counts_by_app();
        let now = self.clock.now_ms();
        let time = LocalTime::at(now, self.clock.offset_s());
        let size = self
            .surface
            .and_then(|id| self.host.view(id))
            .map_or_else(|| self.screen(), |v| v.ui.size());
        let built = view::build(
            &self.state,
            (now, time),
            self.look,
            size,
            &crate::i18n::translator_or_keys(self.look.lang),
        )?;
        match (built, self.surface) {
            (Some(v), Some(id)) => self.host.replace(id, v, None),
            (Some(v), None) => {
                let config = WindowConfig::with_role(Role::Lock, "", false);
                self.surface = Some(self.host.open(&config, v, None)?);
            }
            (None, Some(id)) => {
                self.host.close(id)?;
                self.surface = None;
            }
            (None, None) => {}
        }
        Ok(())
    }
}

/// Runs the lock screen on the system clock.
pub fn run(
    options: &Options,
    user: Option<String>,
    stop: &dyn Fn() -> bool,
) -> Result<(), AppError> {
    let mut app = LockApp::start(options, user, Arc::new(SystemClock::default()))?;
    app.run(stop)
}
