//! The phone shell at work: input events and gestures change the [`Mobile`] model, the tree
//! is rebuilt, and what only the platform can do is returned as [`Effect`]s (start an app,
//! check the password, switch a radio …). No bus, no compositor: the host program wires
//! the effects to the services, so that everything here is tested without them.

use crate::click::Click;
use crate::model::mesh::Act;
use crate::model::quick::Toggle;
use crate::model::time::LocalTime;
use crate::model::{Mobile, Screen, WinAction};
use crate::view::{self, Built};
use ultar_ui::event::{ActionKind, Event, Response};
use ultar_ui::geom::{Point, Size};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{Ui, UiError};
use ultar_ui::Lang;

/// How the shell looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Language (direction follows).
    pub lang: Lang,
    /// Theme.
    pub theme: ThemeKind,
    /// Device pixels per point.
    pub scale: f32,
    /// Finger or pointer.
    pub mode: InputMode,
}

impl Look {
    /// A phone: touch, scale 1.
    pub fn phone(lang: Lang, theme: ThemeKind) -> Look {
        Look {
            lang,
            theme,
            scale: 1.0,
            mode: InputMode::Touch,
        }
    }
}

/// What the platform has to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Ask for the password, PIN or fingerprint; on success call [`MobileApp::unlocked`].
    Unlock,
    /// Start or bring up an app.
    Launch(String),
    /// Close an app (from the recent apps).
    Close(String),
    /// The emergency dialer (works while locked).
    Emergency,
    /// The camera (works while locked).
    Camera,
    /// Voice search (the assistant).
    Voice,
    /// Switch a radio or mode.
    Set(Toggle, bool),
    /// Screen brightness in percent.
    Brightness(u8),
    /// Media volume in percent.
    Volume(u8),
    /// Open Settings, on a page (`network/wifi`) or the start page.
    Settings(Option<&'static str>),
    /// A notification was dismissed.
    Dismiss(u64),
    /// All notifications were dismissed (their numbers).
    ClearNotes(Vec<u64>),
    /// Restart the phone.
    Restart,
    /// Switch the phone off.
    PowerOff,
    /// Lock the screen now (`lock.lock`).
    LockNow,
    /// Act on an app's window.
    Window(u32, crate::model::WinAction),
    /// Continue `app` on another device: `device` chosen, or `None` – wherever it can go
    /// (`mesh.handoff_send`; the answer goes back through [`crate::model::mesh::Mesh::sent`]).
    MeshSend {
        /// The app whose activity is offered.
        app: String,
        /// The chosen device.
        device: Option<String>,
    },
    /// Take a waiting offer back (`mesh.offer_cancel`).
    MeshCancel(u64),
    /// Open a file that arrived from another device (in the files app).
    Open(String),
}

/// Errors of the shell.
pub type AppError = UiError;

/// Minimal distance of a swipe (points).
pub const SWIPE: f32 = 48.0;

/// The phone shell: model, tree and what the tree's nodes mean.
pub struct MobileApp {
    /// The state.
    pub model: Mobile,
    ui: Ui,
    built: Built,
}

impl MobileApp {
    /// A shell of `size` points showing `model`.
    pub fn new(look: Look, size: Size, model: Mobile) -> Result<MobileApp, AppError> {
        let mut env = PaintEnv::new(look.lang, look.theme);
        env.scale = look.scale;
        let tr = crate::i18n::translator_or_keys(look.lang);
        let mut ui = Ui::new(env, look.mode, size, tr)?;
        let built = view::build(&mut ui, &model, view::Part::Full)?;
        Ok(MobileApp { model, ui, built })
    }

    /// The tree (painting, tests).
    pub fn ui(&mut self) -> &mut Ui {
        &mut self.ui
    }

    /// The nodes of the current tree.
    pub fn built(&self) -> &Built {
        &self.built
    }

    /// Builds the tree again after the model changed.
    pub fn rebuild(&mut self) -> Result<(), AppError> {
        self.built = view::build(&mut self.ui, &self.model, view::Part::Full)?;
        if self.model.screen == Screen::Search {
            if let Some(id) = self.built.search {
                self.ui.focus(id)?;
            }
        }
        Ok(())
    }

    /// The surface got a new size.
    pub fn resize(&mut self, size: Size) -> Result<(), AppError> {
        self.ui.set_size(size)?;
        self.rebuild()
    }

    /// A new minute (or a new time zone).
    pub fn set_now(&mut self, now: Option<LocalTime>) -> Result<(), AppError> {
        if self.model.now != now {
            self.model.now = now;
            self.rebuild()?;
        }
        Ok(())
    }

    /// The platform recognised the person.
    pub fn unlocked(&mut self) -> Result<(), AppError> {
        self.model.unlocked();
        self.rebuild()
    }

    /// The screen locks (power button, time-out).
    pub fn lock(&mut self) -> Result<(), AppError> {
        self.model.lock();
        self.rebuild()
    }

    /// One input event; returns what the platform has to do.
    pub fn handle(&mut self, event: Event) -> Result<Vec<Effect>, AppError> {
        let response = self.ui.handle(event);
        self.apply(response)
    }

    /// A swipe the host recognised, from `from` by `delta` points (the toolkit reports
    /// presses on nothing operable that were released elsewhere).
    pub fn swipe(&mut self, from: Point, delta: Point) -> Result<Vec<Effect>, AppError> {
        let rtl = self.ui.env().direction == ultar_ui::Direction::Rtl;
        let per = view::per_page(self.ui.size().h);
        let (changed, effects) = apply_swipe(&mut self.model, from, delta, rtl, per);
        if changed {
            self.rebuild()?;
        }
        Ok(effects)
    }

    fn apply(&mut self, response: Response) -> Result<Vec<Effect>, AppError> {
        let mut effects = Vec::new();
        let mut changed = false;
        for action in response.actions {
            let node = action.node;
            match action.kind {
                ActionKind::ValueChanged(v) if Some(node) == self.built.brightness => {
                    let v = v.clamp(0.0, 100.0) as u8;
                    self.model.quick.brightness = v;
                    effects.push(Effect::Brightness(v));
                }
                ActionKind::ValueChanged(v) if Some(node) == self.built.volume => {
                    let v = v.clamp(0.0, 100.0) as u8;
                    self.model.quick.volume = v;
                    effects.push(Effect::Volume(v));
                }
                ActionKind::TextChanged if Some(node) == self.built.search => {
                    self.model.query = self.ui.text(node)?.to_owned();
                    changed = true;
                }
                // A press beside the power menu or Escape closes it.
                ActionKind::Closed if self.model.power_menu => {
                    self.model.power_menu = false;
                    changed = true;
                }
                ActionKind::Selected(i) if Some(node) == self.built.targets => {
                    changed |= self.click(Click::MeshChoose(i), &mut effects);
                }
                ActionKind::Activated | ActionKind::Toggled(_) => {
                    if let Some(click) = self.built.click(node).cloned() {
                        changed |= self.click(click, &mut effects);
                    }
                }
                ActionKind::DragEnded { delta, .. } => {
                    if let Some(click) = drag_click(self.built.click(node), delta) {
                        changed |= self.click(click, &mut effects);
                    }
                }
                _ => {}
            }
        }
        if changed {
            let focus = self.ui.focused();
            self.rebuild()?;
            if self.model.screen == Screen::Search {
                if let (Some(id), Some(_)) = (self.built.search, focus) {
                    self.ui.focus(id)?;
                }
            }
        }
        Ok(effects)
    }

    /// Carries out `click`; `true` when the model changed.
    pub fn click(&mut self, click: Click, effects: &mut Vec<Effect>) -> bool {
        let per = view::per_page(self.ui.size().h);
        apply_click(&mut self.model, click, per, effects)
    }
}

/// A swipe on the shell: `rtl` = Arabic reading direction, `per_page` = apps on a grid page.
/// Returns whether the model changed and what the platform has to do.
pub fn apply_swipe(
    m: &mut Mobile,
    from: Point,
    delta: Point,
    rtl: bool,
    per_page: usize,
) -> (bool, Vec<Effect>) {
    let (dx, dy) = (delta.x, delta.y);
    let vertical = dy.abs() >= dx.abs();
    match m.screen {
        Screen::Lock if vertical && dy <= -SWIPE => return (false, vec![Effect::Unlock]),
        Screen::Home if vertical && dy >= SWIPE && from.y <= view::metrics::status() * 2.0 => {
            m.go(Screen::Quick);
        }
        Screen::Home if vertical && dy <= -SWIPE => m.go(Screen::Search),
        Screen::Home if !vertical && dx.abs() >= SWIPE => {
            // Pages run in reading direction: in Arabic a swipe to the right goes on.
            let forward = (dx < 0.0) != rtl;
            m.turn_page(forward, per_page);
        }
        Screen::Quick if vertical && dy <= -SWIPE => m.go(Screen::Home),
        _ => return (false, Vec::new()),
    }
    (true, Vec::new())
}

/// Carries out `click` on the model; `true` when it changed. What only the platform can do
/// goes to `effects`.
pub fn apply_click(
    m: &mut Mobile,
    click: Click,
    per_page: usize,
    effects: &mut Vec<Effect>,
) -> bool {
    if let Some(effect) = platform_only(&click) {
        effects.push(effect);
        if matches!(
            click,
            Click::PowerRestart | Click::PowerOff | Click::LockNow
        ) {
            m.power_menu = false;
            return true;
        }
        return false;
    }
    match click {
        Click::Torch => switch(m, Toggle::Torch, effects),
        Click::Toggle(t) => switch(m, t, effects),
        Click::Dismiss(id) => {
            m.dismiss(id);
            effects.push(Effect::Dismiss(id));
        }
        Click::ClearAll => {
            let ids = m.notes.drain(..).map(|n| n.id).collect();
            effects.push(Effect::ClearNotes(ids));
        }
        Click::CloseRecent(id) => close_app(m, &id, effects),
        Click::ClearRecents => {
            for id in m.recents.clone() {
                close_app(m, &id, effects);
            }
        }
        Click::App(id) => {
            // Its window to the front when it has one, else start it.
            match m.windows_of(&id).last() {
                Some(w) => effects.push(Effect::Window(*w, WinAction::Activate)),
                None => effects.push(Effect::Launch(id.clone())),
            }
            m.started(&id);
        }
        Click::Power => m.power_menu = true,
        c @ (Click::ToggleMore(Toggle::Mesh)
        | Click::MeshSend(_)
        | Click::MeshChoose(_)
        | Click::MeshConfirm
        | Click::MeshCancel
        | Click::MeshBack
        | Click::MeshOfferCancel
        | Click::MeshRetry
        | Click::MeshOpen
        | Click::MeshNoticeClose) => mesh_click(m, c, effects),
        Click::PowerCancel => m.power_menu = false,
        other => navigate(m, other, per_page, effects),
    }
    true
}

/// The device mesh clicks: send the app in front or a recent one, choose a device, confirm.
fn mesh_click(m: &mut Mobile, click: Click, effects: &mut Vec<Effect>) {
    match click {
        Click::ToggleMore(Toggle::Mesh) => match m
            .front()
            .map(|w| w.app.clone())
            .or_else(|| m.recents.first().cloned())
        {
            Some(app) if m.mesh.can_send() => effects.push(Effect::MeshSend { app, device: None }),
            _ => {}
        },
        Click::MeshSend(app) => {
            if m.mesh.can_send() {
                effects.push(Effect::MeshSend { app, device: None });
            }
        }
        Click::MeshChoose(i) => {
            if let Some(c) = m.mesh.choice.as_mut() {
                c.chosen = i.min(c.targets.len().saturating_sub(1));
            }
        }
        Click::MeshConfirm => {
            if let Some(c) = m.mesh.choice.take() {
                let device = c.targets.get(c.chosen).map(|t| t.device.clone());
                effects.push(Effect::MeshSend { app: c.app, device });
            }
            m.go(Screen::Home);
        }
        Click::MeshCancel | Click::MeshBack => m.go(Screen::Home),
        Click::MeshOfferCancel => {
            if let Some(Act::Cancel(offer)) = m.mesh.take_act() {
                effects.push(Effect::MeshCancel(offer));
            }
        }
        Click::MeshRetry => {
            if let (Some(Act::Retry), Some((app, device))) =
                (m.mesh.take_act(), m.mesh.last.clone())
            {
                effects.push(Effect::MeshSend { app, device });
            }
        }
        Click::MeshOpen => {
            if let Some(Act::Open(uri)) = m.mesh.take_act() {
                effects.push(Effect::Open(uri));
            }
        }
        Click::MeshNoticeClose => m.mesh.notice = None,
        _ => {}
    }
}

fn switch(m: &mut Mobile, t: Toggle, effects: &mut Vec<Effect>) {
    if let Some(on) = m.toggle(t) {
        effects.push(Effect::Set(t, on));
    }
}

/// Closes an app from the recent apps: its windows, or the app itself without one.
fn close_app(m: &mut Mobile, app: &str, effects: &mut Vec<Effect>) {
    let windows = m.windows_of(app);
    if windows.is_empty() {
        effects.push(Effect::Close(app.to_owned()));
    }
    for w in windows {
        effects.push(Effect::Window(w, WinAction::Close));
    }
    m.forget_recent(app);
}

/// Clicks that only move between the screens (and put the apps away for the home screen).
fn navigate(m: &mut Mobile, click: Click, per_page: usize, effects: &mut Vec<Effect>) {
    let toggle = |m: &Mobile, s: Screen| if m.screen == s { Screen::Home } else { s };
    match click {
        Click::Status => m.go(toggle(m, Screen::Quick)),
        Click::Recents => m.go(toggle(m, Screen::Recents)),
        Click::Search => m.go(Screen::Search),
        Click::Back if m.power_menu => m.power_menu = false,
        Click::Back if m.screen == Screen::Home => {
            // Nothing of the shell open: the app in front goes back one step.
            if let Some(w) = m.front() {
                effects.push(Effect::Window(w.id, WinAction::Back));
            }
        }
        Click::Back => m.back(),
        Click::Home => {
            m.go(Screen::Home);
            m.launcher.page = 0;
            for w in m.windows.iter().filter(|w| !w.minimized) {
                effects.push(Effect::Window(w.id, WinAction::Minimize));
            }
        }
        Click::Page(p) => m.show_page(p, per_page),
        _ => {}
    }
}

/// A recent app held and swiped up (the gesture of the first Ultar phone): continue it on
/// another device.
pub fn drag_click(click: Option<&Click>, delta: Point) -> Option<Click> {
    match click {
        Some(Click::App(id)) if delta.y <= -SWIPE && delta.y.abs() >= delta.x.abs() => {
            Some(Click::MeshSend(id.clone()))
        }
        _ => None,
    }
}

/// What a click asks of the platform alone, without changing the shell.
fn platform_only(click: &Click) -> Option<Effect> {
    Some(match click {
        Click::Unlock => Effect::Unlock,
        Click::Emergency => Effect::Emergency,
        Click::Camera => Effect::Camera,
        Click::Voice => Effect::Voice,
        Click::ToggleMore(t) if *t != Toggle::Mesh => Effect::Settings(Some(settings_page(*t))),
        Click::Settings => Effect::Settings(None),
        Click::PowerRestart => Effect::Restart,
        Click::PowerOff => Effect::PowerOff,
        Click::LockNow => Effect::LockNow,
        _ => return None,
    })
}

/// The settings page behind the "more" part of a large tile.
fn settings_page(t: Toggle) -> &'static str {
    match t {
        Toggle::Wifi => "network/wifi",
        Toggle::MobileData => "network/mobile",
        Toggle::Bluetooth => "bluetooth",
        _ => "system",
    }
}
