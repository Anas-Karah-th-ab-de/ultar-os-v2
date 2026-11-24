//! Keys and pointer: browser shortcuts first, then the page (while it has the focus and
//! nothing is open above it), then the window's widgets.

use super::{AppError, Browser, Overlay};
use crate::engine::{KeyInput, PointerInput, WebEngine};
use ultar_ui::event::{Event, Key, PointerButton};

/// Modifier bit: Shift.
pub const SHIFT: u8 = 1;
/// Modifier bit: Ctrl.
pub const CTRL: u8 = 2;
/// Modifier bit: Alt.
pub const ALT: u8 = 4;

/// Linux key codes the browser uses for shortcuts.
mod code {
    pub const ESC: u16 = 1;
    pub const W: u16 = 17;
    pub const T: u16 = 20;
    pub const TAB: u16 = 15;
    pub const L: u16 = 38;
    pub const N: u16 = 49;
    pub const F: u16 = 33;
    pub const F5: u16 = 63;
    pub const F10: u16 = 68;
    pub const DOWN: u16 = 108;
    pub const LEFT: u16 = 105; // physical arrow key
    pub const RIGHT: u16 = 106; // physical arrow key
    pub const MENU: u16 = 127;
}

fn button_number(button: PointerButton) -> u8 {
    match button {
        PointerButton::Primary => 1,
        PointerButton::Middle => 2,
        PointerButton::Secondary => 3,
    }
}

impl<E: WebEngine> Browser<E> {
    /// A key from the compositor (`ui_event` is the toolkit's form of it, if it has one).
    pub fn key(&mut self, key: KeyInput, ui_event: Option<Event>) -> Result<(), AppError> {
        if key.pressed && self.shortcut(key)? {
            return Ok(());
        }
        let page = self.chrome.page.filter(|p| self.ui.focused() == Some(*p));
        let tab = self.state.tabs.active().map(|t| t.engine);
        if let (Some(_), Some(tab), Overlay::None) = (page, tab, &self.overlay) {
            if self.ui.menu_owner().is_none() {
                self.engine.key(tab, key)?;
                return Ok(());
            }
        }
        if let Some(event) = ui_event {
            let response = self.ui.handle(event);
            self.handle_actions(&response.actions)?;
        }
        Ok(())
    }

    fn shortcut(&mut self, key: KeyInput) -> Result<bool, AppError> {
        let m = key.modifiers & (SHIFT | CTRL | ALT);
        let index = self.state.tabs.active_index();
        match (m, key.code) {
            (CTRL, code::T) => self.new_tab(false)?,
            (x, code::N) if x == CTRL | SHIFT => self.new_tab(true)?,
            (CTRL, code::W) => self.close_tab(index)?,
            (CTRL, code::L) => {
                let _ = self.ui.focus(self.chrome.address);
            }
            (CTRL, code::TAB) => {
                let next = (index + 1) % self.state.tabs.len().max(1);
                self.select_tab(next)?
            }
            (0, code::F5) => self.reload_or_stop()?,
            (ALT, code::LEFT) => self.history_step(true)?, // physical arrow key
            (ALT, code::RIGHT) => self.history_step(false)?, // physical arrow key
            (0, code::F10) | (0, code::MENU) | (ALT, code::F) => self.open_menu()?,
            (0, code::DOWN) if self.ui.focused() == Some(self.chrome.address) => {
                return self.open_suggestions();
            }
            (0, code::ESC) if self.chrome.is_loading() && matches!(self.overlay, Overlay::None) => {
                self.reload_or_stop()?
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn history_step(&mut self, back: bool) -> Result<(), AppError> {
        if let Some(tab) = self.state.tabs.active().map(|t| t.engine) {
            if back {
                self.engine.back(tab)?;
            } else {
                self.engine.forward(tab)?;
            }
        }
        Ok(())
    }

    /// A pointer or touch event. Over the page (with nothing open above it) the event goes
    /// to the engine in page pixels and a press gives the page the keyboard focus; else to
    /// the widgets. A secondary click on the tab strip or the toolbar opens the menu.
    pub fn pointer(&mut self, event: Event) -> Result<(), AppError> {
        if let Some(input) = self.page_input(&event) {
            if let (Some(tab), Some(page)) =
                (self.state.tabs.active().map(|t| t.engine), self.chrome.page)
            {
                if matches!(input, PointerInput::Down { .. }) {
                    let _ = self.ui.focus(page);
                }
                self.engine.pointer(tab, input)?;
                return Ok(());
            }
        }
        if let Event::PointerDown {
            pos,
            button: PointerButton::Secondary,
        } = event
        {
            let bars = [self.chrome.strip, self.chrome.toolbar];
            if self.ui.node_at(pos).is_some_and(|n| bars.contains(&n)) {
                return self.open_menu();
            }
        }
        let response = self.ui.handle(event);
        self.handle_actions(&response.actions)
    }

    fn page_input(&self, event: &Event) -> Option<PointerInput> {
        if !matches!(self.overlay, Overlay::None) || self.ui.menu_owner().is_some() {
            return None;
        }
        let rect = self.ui.rect(self.chrome.page?)?;
        let s = self.ui.env().scale;
        let (pos, kind) = match *event {
            Event::PointerMove { pos } => (pos, 0),
            Event::PointerDown { pos, button } => (pos, 10 + button_number(button)),
            Event::PointerUp { pos, button } => (pos, 20 + button_number(button)),
            Event::Scroll { pos, lines } => (pos, if lines == 0.0 { return None } else { 30 }),
            _ => return None,
        };
        if !rect.contains(pos) {
            return None;
        }
        let (x, y) = ((pos.x - rect.x) * s, (pos.y - rect.y) * s);
        Some(match (kind, event) {
            (0, _) => PointerInput::Move { x, y },
            (30, Event::Scroll { lines, .. }) => PointerInput::Scroll {
                x,
                y,
                dx: 0.0,
                dy: lines * ultar_ui::tokens::size::SCROLL_LINE * s,
            },
            (k, _) if k < 20 => PointerInput::Down {
                x,
                y,
                button: k - 10,
            },
            (k, _) => PointerInput::Up {
                x,
                y,
                button: k - 20,
            },
        })
    }

    /// Advances the window's clock and handles what it caused (toast timeouts).
    pub fn tick(&mut self, now_ms: u64) -> Result<(), AppError> {
        self.ui.advance_to(now_ms);
        let actions = self.ui.take_actions();
        self.handle_actions(&actions)
    }

    /// Escape key for tests and the window: closes what is open.
    pub fn escape(&mut self) -> Result<(), AppError> {
        let response = self.ui.handle(Event::Key {
            key: Key::Escape,
            modifiers: ultar_ui::event::Modifiers::NONE,
        });
        self.handle_actions(&response.actions)
    }
}
