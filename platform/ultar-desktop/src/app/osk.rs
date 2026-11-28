//! The on-screen keyboard program (`ultar-osk`): learns from the compositor whether a text
//! field is focused (`ImeState`), shows itself as the *input panel* when the settings want
//! it, and sends what is typed to that field (`ImeCommit`, `ImeKey`). It has no other way
//! to reach an app: the compositor checks the right `ui.input-method` and that the panel
//! is shown, and passes text only to the focused field.

use super::{AppError, Options};
use crate::host::{Host, HostEvent};
use crate::osk::{view, Keyboard, OskSettings, Out, Special, FOLLOWED};
use crate::view::{Look, Target};
use std::time::Duration;
use ultar_bus::messages::devices::Capabilities;
use ultar_bus::messages::settings::{Changed, Get};
use ultar_bus::{Client, ClientOptions, TypedSubscription};
use ultar_surface::client::WindowConfig;
use ultar_surface::proto::{Event, ImeKey, Request, Role, SurfaceId};
use ultar_ui::event::ActionKind;
use ultar_ui::geom::Size;
use ultar_ui::tokens::ThemeKind;

/// Application id of the keyboard.
pub const APP_ID: &str = "org.ultar.osk";
/// How long one settings read may take.
const BUS_WAIT: Duration = Duration::from_secs(2);
/// The size the panel is built at before the compositor sends its own.
const FIRST_SIZE: Size = Size::new(800.0, 320.0);

/// The running keyboard.
pub struct OskApp {
    host: Host,
    /// The keyboard's state (tests).
    pub keyboard: Keyboard,
    look: Look,
    surface: Option<SurfaceId>,
    size: Size,
    changes: Option<TypedSubscription<Changed>>,
    devices: Option<TypedSubscription<Capabilities>>,
    dirty: bool,
}

impl std::fmt::Debug for OskApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OskApp")
            .field("keyboard", &self.keyboard)
            .finish_non_exhaustive()
    }
}

impl OskApp {
    /// Connects to the compositor and the bus and reads the settings.
    pub fn start(options: &Options) -> Result<OskApp, AppError> {
        let host = Host::connect(&options.compositor, APP_ID)?;
        let client = options
            .bus
            .as_deref()
            .and_then(|p| Client::connect(p, ClientOptions::named(APP_ID)).ok());
        let mut settings = OskSettings::default();
        if let Some(c) = &client {
            for key in FOLLOWED {
                let reply = c.call(&Get { key: key.into() }, BUS_WAIT);
                if let Some(value) = reply.ok().and_then(|r| r.value) {
                    settings.apply(key, &value);
                }
            }
        }
        let theme = if options.light {
            ThemeKind::Light
        } else {
            ThemeKind::Dark
        };
        let mut look = Look::new(options.lang, theme);
        if let Some(c) = &client {
            super::look_follow::read(c, &mut look);
        }
        Ok(OskApp {
            host,
            keyboard: Keyboard::new(settings),
            look,
            surface: None,
            size: FIRST_SIZE,
            changes: client.as_ref().and_then(|c| c.subscribe::<Changed>().ok()),
            devices: client
                .as_ref()
                .and_then(|c| c.subscribe::<Capabilities>().ok()),
            dirty: true,
        })
    }

    /// Runs until `stop`.
    pub fn run(&mut self, stop: &dyn Fn() -> bool) -> Result<(), AppError> {
        while !stop() {
            self.turn()?;
        }
        Ok(())
    }

    /// One turn: compositor events, then the bus, then show, hide or rebuild.
    pub fn turn(&mut self) -> Result<(), AppError> {
        for event in self.host.dispatch(Some(Duration::from_millis(250)))? {
            self.event(event)?;
        }
        self.bus();
        self.show()?;
        self.host.draw()?;
        Ok(())
    }

    fn bus(&mut self) {
        while let Some(Ok(Some(c))) = self.changes.as_ref().map(|s| s.try_recv()) {
            if super::look_follow::apply(&mut self.look, &c.key, &c.value) {
                self.dirty = true;
            }
            if self.keyboard.setting(&c.key, &c.value) {
                self.dirty = true;
            }
        }
        while let Some(Ok(Some(caps))) = self.devices.as_ref().map(|s| s.try_recv()) {
            self.keyboard.hardware = caps.has("keyboard");
            self.dirty = true;
        }
    }

    fn event(&mut self, event: HostEvent) -> Result<(), AppError> {
        match event {
            HostEvent::Compositor(Event::ImeState { active, purpose }) => {
                self.keyboard.ime_state(active, purpose);
                self.dirty = true;
            }
            HostEvent::Resized(_, size) => {
                self.size = size;
                self.dirty = true;
            }
            HostEvent::Action(id, action) if action.kind == ActionKind::Activated => {
                let target = self
                    .host
                    .view(id)
                    .and_then(|v| v.target(action.node))
                    .cloned();
                let out = match target {
                    Some(Target::OskKey(r, i)) => {
                        let act = self
                            .keyboard
                            .rows()
                            .get(r)
                            .and_then(|row| row.keys.get(i))
                            .map(|k| k.act.clone());
                        act.map(|a| self.keyboard.press(&a)).unwrap_or_default()
                    }
                    Some(Target::OskSuggestion(i)) => {
                        let word = self.keyboard.suggestions().get(i).copied();
                        word.map(|w| self.keyboard.choose(w)).unwrap_or_default()
                    }
                    _ => Vec::new(),
                };
                self.send(&out)?;
                self.dirty = true;
            }
            _ => {}
        }
        Ok(())
    }

    /// Sends what was typed to the focused field.
    fn send(&mut self, out: &[Out]) -> Result<(), AppError> {
        for o in out {
            let request = match o {
                Out::Commit(text) => Request::ImeCommit { text: text.clone() },
                Out::Key(Special::Backspace) => Request::ImeKey {
                    key: ImeKey::Backspace,
                },
                Out::Key(Special::Enter) => Request::ImeKey { key: ImeKey::Enter },
            };
            self.host.client.send(&request)?;
        }
        Ok(())
    }

    /// Opens the panel when it should be seen, closes it when not, rebuilds it after a
    /// change.
    fn show(&mut self) -> Result<(), AppError> {
        let tr = crate::i18n::translator_or_keys(self.look.lang);
        match (self.keyboard.visible(), self.surface) {
            (true, None) => {
                let built = view::build(&self.keyboard, self.look, self.size, &tr)?;
                let config = WindowConfig::with_role(Role::InputPanel, "keyboard", false);
                self.surface = Some(self.host.open(&config, built, None)?);
            }
            (true, Some(surface)) if self.dirty => {
                let built = view::build(&self.keyboard, self.look, self.size, &tr)?;
                self.host.replace(surface, built, None);
            }
            (false, Some(surface)) => {
                self.host.close(surface)?;
                self.surface = None;
            }
            _ => {}
        }
        self.dirty = false;
        Ok(())
    }
}

/// Runs the keyboard.
pub fn run(options: &Options, stop: &dyn Fn() -> bool) -> Result<(), AppError> {
    let mut app = OskApp::start(options)?;
    app.run(stop)
}
