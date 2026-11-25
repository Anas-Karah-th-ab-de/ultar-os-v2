//! The phone shell at work: input events and gestures change the [`Mobile`] model, the tree
//! is rebuilt, and what only the platform can do is returned as [`Effect`]s (start an app,
//! check the password, switch a radio …). No bus, no compositor: the host program wires
//! the effects to the services, so that everything here is tested without them.

use crate::click::Click;
use crate::model::quick::Toggle;
use crate::model::time::LocalTime;
use crate::model::{Mobile, Screen};
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
    /// The power menu (restart, power off).
    Power,
    /// A notification was dismissed.
    Dismiss(u64),
    /// All notifications were dismissed.
    ClearNotes,
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
        let built = view::build(&mut ui, &model)?;
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
        self.built = view::build(&mut self.ui, &self.model)?;
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
        let (dx, dy) = (delta.x, delta.y);
        let vertical = dy.abs() >= dx.abs();
        let mut effects = Vec::new();
        match self.model.screen {
            Screen::Lock if vertical && dy <= -SWIPE => effects.push(Effect::Unlock),
            Screen::Home if vertical && dy >= SWIPE && from.y <= view::metrics::status() * 2.0 => {
                self.model.go(Screen::Quick);
            }
            Screen::Home if vertical && dy <= -SWIPE => self.model.go(Screen::Search),
            Screen::Home if !vertical && dx.abs() >= SWIPE => {
                // Pages run in reading direction: in Arabic a swipe to the right goes on.
                let rtl = self.ui.env().direction == ultar_ui::Direction::Rtl;
                let forward = (dx < 0.0) != rtl;
                let per = view::per_page(self.ui.size().h);
                self.model.turn_page(forward, per);
            }
            Screen::Quick if vertical && dy <= -SWIPE => self.model.go(Screen::Home),
            _ => return Ok(effects),
        }
        self.rebuild()?;
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
                ActionKind::Activated | ActionKind::Toggled(_) => {
                    if let Some(click) = self.built.click(node).cloned() {
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
        if let Some(effect) = platform_only(&click) {
            effects.push(effect);
            return false;
        }
        match click {
            Click::Torch => self.switch(Toggle::Torch, effects),
            Click::Toggle(t) => self.switch(t, effects),
            Click::Dismiss(id) => {
                self.model.dismiss(id);
                effects.push(Effect::Dismiss(id));
            }
            Click::ClearAll => {
                self.model.notes.clear();
                effects.push(Effect::ClearNotes);
            }
            Click::CloseRecent(id) => {
                self.model.forget_recent(&id);
                effects.push(Effect::Close(id));
            }
            Click::ClearRecents => {
                for id in std::mem::take(&mut self.model.recents) {
                    effects.push(Effect::Close(id));
                }
            }
            Click::App(id) => {
                effects.push(Effect::Launch(id.clone()));
                self.model.started(&id);
            }
            other => self.navigate(other),
        }
        true
    }

    fn switch(&mut self, t: Toggle, effects: &mut Vec<Effect>) {
        if let Some(on) = self.model.toggle(t) {
            effects.push(Effect::Set(t, on));
        }
    }

    /// Clicks that only move between the screens.
    fn navigate(&mut self, click: Click) {
        let m = &mut self.model;
        let toggle = |m: &Mobile, s: Screen| if m.screen == s { Screen::Home } else { s };
        match click {
            Click::Status => m.go(toggle(m, Screen::Quick)),
            Click::Recents => m.go(toggle(m, Screen::Recents)),
            Click::Search => m.go(Screen::Search),
            Click::Back => m.back(),
            Click::Home => {
                m.go(Screen::Home);
                m.page = 0;
            }
            Click::Page(p) => {
                let per = view::per_page(self.ui.size().h);
                self.model.show_page(p, per);
            }
            _ => {}
        }
    }
}

/// What a click asks of the platform alone, without changing the shell.
fn platform_only(click: &Click) -> Option<Effect> {
    Some(match click {
        Click::Unlock => Effect::Unlock,
        Click::Emergency => Effect::Emergency,
        Click::Camera => Effect::Camera,
        Click::Voice => Effect::Voice,
        Click::ToggleMore(t) => Effect::Settings(Some(settings_page(*t))),
        Click::Settings => Effect::Settings(None),
        Click::Power => Effect::Power,
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
