//! The desktop shell's part of the compositor (right `ui.shell`): popups
//! attached to its panels (sheets, menus, overview), the keyboard for an
//! open sheet, the shell's own shortcuts (Super alone, Super+key), the window
//! list of the bar, acting on windows, the display mode and keyboard layouts.
//!
//! Everything is default deny: only a client holding `ui.shell` may use it,
//! and only on its own surfaces; the one exception is the overview's
//! thumbnails of windows (`workspaces.rs`: never of system dialogs, never
//! while locked, rate-limited); nothing here lets the shell read input.

use super::{ClientId, State, SurfaceKey};
use crate::geom::Rect;
use crate::keymap::Layout as KeyLayout;
use crate::output::DisplayMode;
use ultar_hal::KeyState;
use ultar_surface::proto::{
    DisplayModeRequest, ErrorCode, Event, Modifiers, Rights, Role, ShellWindow, SurfaceId,
    WindowAction, WindowState,
};

/// Left and right Super (logo) key codes.
const SUPER_KEYS: [u16; 2] = [125, 126];

/// What the compositor keeps for the shell.
#[derive(Debug, Default)]
pub struct ShellSession {
    /// The shell surface that has the keyboard (an open sheet or menu).
    pub focus: Option<SurfaceKey>,
    /// The window list as last sent to shell clients.
    sent: Option<Vec<ShellWindow>>,
    /// The active layout as last sent.
    layout_sent: Option<usize>,
    /// Super went down and nothing else happened since (a tap opens the
    /// overview).
    super_tap: bool,
    /// The workspaces.
    pub(super) workspaces: super::Workspaces,
    /// The workspaces as last sent.
    workspaces_sent: Option<super::Workspaces>,
    /// Thumbnails per client in the current second: (second, count).
    pub(super) thumbs: std::collections::HashMap<ClientId, (u64, u32)>,
    /// Phone mode and the edge gesture under way.
    pub phone: super::phone::Phone,
}

impl ShellSession {
    /// A compositor shortcut used the Super key: no tap any more.
    pub(super) fn super_tap_used(&mut self) {
        self.super_tap = false;
    }
}

/// Wire name of a keyboard layout.
fn layout_name(layout: KeyLayout) -> &'static str {
    layout.name()
}

impl State {
    /// True for a panel of a shell client or a popup attached to one.
    pub fn is_shell_surface(&self, key: SurfaceKey) -> bool {
        let Some(c) = self.clients.get(&key.client) else {
            return false;
        };
        if !c.rights.contains(Rights::SHELL) {
            return false;
        }
        match c.surfaces.get(&key.surface).map(|s| s.role) {
            Some(Role::Panel { .. }) => true,
            Some(Role::Popup { parent, .. }) => c
                .surfaces
                .get(&parent)
                .is_some_and(|p| matches!(p.role, Role::Panel { .. })),
            _ => false,
        }
    }

    /// Where a popup of a shell panel goes: relative to the panel, kept on
    /// the panel's output (it may cover windows, never another display).
    pub(super) fn panel_popup_rect(
        &self,
        parent: SurfaceKey,
        (x, y): (i32, i32),
        (w, h): (i32, i32),
    ) -> Option<Rect> {
        let base = self.layer_rect(parent)?;
        let screen = self.layout.primary()?.logical_rect();
        let (w, h) = (w.min(screen.w).max(1), h.min(screen.h).max(1));
        let cx = base
            .x
            .saturating_add(x)
            .clamp(screen.x, (screen.max_x() - w).max(screen.x));
        let cy = base
            .y
            .saturating_add(y)
            .clamp(screen.y, (screen.max_y() - h).max(screen.y));
        Some(Rect::new(cx, cy, w, h))
    }

    /// `ShellFocus`: gives the keyboard to an own panel or panel popup, or
    /// back to the focused window (`surface` 0).
    pub(super) fn shell_focus(
        &mut self,
        client: ClientId,
        surface: SurfaceId,
    ) -> Result<(), ErrorCode> {
        let rights = self.clients.get(&client).map(|c| c.rights);
        if !rights.is_some_and(|r| r.contains(Rights::SHELL)) {
            return Err(ErrorCode::Permission);
        }
        if surface.0 == 0 {
            if self.session.shell.focus.is_some_and(|k| k.client == client) {
                self.session.shell.focus = None;
                self.update_ime();
            }
            return Ok(());
        }
        let key = SurfaceKey { client, surface };
        if !self.is_shell_surface(key) {
            return Err(ErrorCode::UnknownObject);
        }
        if !self.locked() {
            self.session.shell.focus = Some(key);
            self.update_ime();
        }
        Ok(())
    }

    /// Takes the keyboard from the shell surface that has it (tells it).
    pub(super) fn drop_shell_focus(&mut self) {
        if let Some(key) = self.session.shell.focus.take() {
            self.send(
                key.client,
                Event::ShellFocusLost {
                    surface: key.surface,
                },
            );
            self.update_ime();
        }
    }

    /// A press on `hit` (None: on nothing): the shell keeps the keyboard
    /// only while the user presses on its own surfaces.
    pub(super) fn shell_press(&mut self, hit: Option<SurfaceKey>) {
        let Some(focus) = self.session.shell.focus else {
            return;
        };
        let own = hit.is_some_and(|k| k.client == focus.client && self.is_shell_surface(k));
        if !own {
            self.drop_shell_focus();
        }
    }

    /// The shell client that gets shortcuts: holds `ui.shell` and shows a
    /// panel.
    fn shell_client(&self) -> Option<ClientId> {
        self.clients
            .iter()
            .find(|(_, c)| {
                c.hello
                    && c.rights.contains(Rights::SHELL)
                    && c.surfaces
                        .values()
                        .any(|s| matches!(s.role, Role::Panel { .. }))
            })
            .map(|(&id, _)| id)
    }

    /// Shell shortcuts: Super alone (on release, if nothing else happened)
    /// and Super with a key the compositor does not use itself go to the
    /// shell, never to an app. True if the key was taken.
    pub(super) fn shell_key(&mut self, code: u16, state: KeyState) -> bool {
        if self.locked() {
            self.session.shell.super_tap = false;
            return false;
        }
        let is_super = SUPER_KEYS.contains(&code);
        match state {
            KeyState::Pressed if is_super => {
                self.session.shell.super_tap = true;
                false
            }
            KeyState::Released if is_super => {
                let tap = std::mem::take(&mut self.session.shell.super_tap);
                if tap {
                    self.send_shell_key(code);
                }
                false
            }
            KeyState::Pressed => {
                self.session.shell.super_tap = false;
                let m = Modifiers(self.keyboard.modifiers);
                if !m.has(Modifiers::SUPER) || self.session.shell.focus.is_some() {
                    return false;
                }
                if self.send_shell_key(code) {
                    // Its release is not forwarded either.
                    self.session.swallowed.push(code);
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn send_shell_key(&mut self, code: u16) -> bool {
        let Some(client) = self.shell_client() else {
            return false;
        };
        let serial = self.user_serial(client);
        let modifiers = Modifiers(self.keyboard.modifiers | self.caps_bit());
        self.send(
            client,
            Event::ShellKey {
                code,
                modifiers,
                serial,
            },
        );
        true
    }

    /// The Caps Lock bit for key events.
    pub(super) fn caps_bit(&self) -> u8 {
        if self.keyboard.caps_lock {
            Modifiers::CAPS_LOCK
        } else {
            0
        }
    }

    /// `WindowAction` of the shell on any toplevel by id. A window that
    /// went in the meantime is ignored (not an error: the list was older).
    pub(super) fn window_action(
        &mut self,
        client: ClientId,
        id: u32,
        action: WindowAction,
    ) -> Result<(), ErrorCode> {
        self.require(client, Rights::SHELL)?;
        let Some(i) = self.windows.iter().position(|w| w.id == id && !w.secure) else {
            return Ok(());
        };
        let key = self.windows[i].key;
        match action {
            WindowAction::Activate => {
                self.reveal_window(i);
                self.drop_shell_focus();
                self.unminimize(i);
                self.user_focus(key);
            }
            WindowAction::Minimize => self.set_window_state(i, WindowState::Minimized, None),
            // Phone mode: every window stays maximised.
            WindowAction::ToggleMaximize if self.session.shell.phone.on => {}
            WindowAction::Back => self.window_back(key),
            WindowAction::ToggleMaximize => {
                let state = if self.windows[i].state == WindowState::Maximized {
                    WindowState::Normal
                } else {
                    WindowState::Maximized
                };
                self.set_window_state(i, state, None);
            }
            WindowAction::Close => self.request_close(key),
        }
        Ok(())
    }

    /// `SetDisplayMode` of the shell (Super+P). An output that went in the
    /// meantime is ignored.
    pub(super) fn set_display_mode(
        &mut self,
        client: ClientId,
        mode: DisplayModeRequest,
    ) -> Result<(), ErrorCode> {
        self.require(client, Rights::SHELL)?;
        let mode = match mode {
            DisplayModeRequest::Extend => DisplayMode::Extend,
            DisplayModeRequest::Mirror => DisplayMode::Mirror,
            DisplayModeRequest::Single(o) => DisplayMode::Single(o),
        };
        if self.layout.mode() != mode && self.layout.set_mode(mode) {
            self.outputs_changed();
        }
        Ok(())
    }

    /// `SetKeyboardLayout` of the shell or the lock screen.
    pub(super) fn set_keyboard_layout(
        &mut self,
        client: ClientId,
        index: u8,
    ) -> Result<(), ErrorCode> {
        let rights = self.clients.get(&client).map_or(Rights::NONE, |c| c.rights);
        if !(rights.contains(Rights::SHELL) || rights.contains(Rights::LOCK_SCREEN)) {
            return Err(ErrorCode::Permission);
        }
        if usize::from(index) < self.config.layouts.len() {
            self.keyboard.layout_index = usize::from(index);
        }
        Ok(())
    }

    /// `SetKeyboardLayouts` of the shell (the user's input languages from the settings
    /// service). Unknown names and repeats are left out; a list with no known name changes
    /// nothing. The active layout stays active when it is still in the list.
    pub(super) fn set_keyboard_layouts(
        &mut self,
        client: ClientId,
        names: &[String],
    ) -> Result<(), ErrorCode> {
        self.require(client, Rights::SHELL)?;
        let mut layouts: Vec<KeyLayout> = Vec::new();
        for l in names.iter().filter_map(|n| KeyLayout::parse(n)) {
            if !layouts.contains(&l) {
                layouts.push(l);
            }
        }
        if layouts.is_empty() || layouts == self.config.layouts {
            return Ok(());
        }
        let active = self.key_layout();
        self.keyboard.layout_index = layouts.iter().position(|l| *l == active).unwrap_or(0);
        self.config.layouts = layouts;
        // The list changed: shell and lock screen hear of it even with the same index.
        self.session.shell.layout_sent = None;
        Ok(())
    }

    fn require(&self, client: ClientId, needed: Rights) -> Result<(), ErrorCode> {
        match self.clients.get(&client) {
            Some(c) if c.rights.contains(needed) => Ok(()),
            Some(_) => Err(ErrorCode::Permission),
            None => Err(ErrorCode::UnknownObject),
        }
    }

    /// The window list as the shell sees it, bottom to top (system dialogs
    /// are not listed).
    pub fn shell_windows(&self) -> Vec<ShellWindow> {
        self.windows
            .iter()
            .filter(|w| w.mapped && !w.secure)
            .map(|w| {
                let s = self.surface(w.key);
                ShellWindow {
                    id: w.id,
                    app_id: self
                        .clients
                        .get(&w.key.client)
                        .map(|c| c.app_id.clone())
                        .unwrap_or_default(),
                    title: s.map(|s| s.title.clone()).unwrap_or_default(),
                    state: w.state,
                    focused: self.focus == Some(w.key),
                    unresponsive: w.unresponsive,
                    output: self.window_output(w.frame).unwrap_or_default(),
                    workspace: w.place.workspace,
                }
            })
            .collect()
    }

    /// The clients with the shell right (edge gestures go only to them).
    pub(super) fn clients_with_shell(&self) -> Vec<ClientId> {
        self.clients_with(Rights::SHELL)
    }

    fn clients_with(&self, any: Rights) -> Vec<ClientId> {
        self.clients
            .iter()
            .filter(|(_, c)| c.hello && c.rights.0 & any.0 != 0)
            .map(|(&id, _)| id)
            .collect()
    }

    fn layouts_event(&self) -> Event {
        Event::KeyboardLayouts {
            names: self
                .config
                .layouts
                .iter()
                .map(|l| layout_name(*l).to_owned())
                .collect(),
            active: self.keyboard.layout_index.min(u8::MAX as usize) as u8,
        }
    }

    /// Tells shell clients about changed windows and shell/lock clients
    /// about a changed layout. Called once per loop turn.
    pub fn shell_refresh(&mut self) {
        let windows = self.shell_windows();
        if self.session.shell.sent.as_ref() != Some(&windows) {
            for client in self.clients_with(Rights::SHELL) {
                let event = Event::WindowsChanged {
                    windows: windows.clone(),
                };
                self.send(client, event);
            }
            self.session.shell.sent = Some(windows);
        }
        let ws = self.session.shell.workspaces;
        if self.session.shell.workspaces_sent != Some(ws) {
            let event = self.workspaces_event();
            for client in self.clients_with(Rights::SHELL) {
                self.send(client, event.clone());
            }
            self.session.shell.workspaces_sent = Some(ws);
        }
        if self.session.shell.layout_sent != Some(self.keyboard.layout_index) {
            let event = self.layouts_event();
            for client in self.clients_with(Rights::SHELL.union(Rights::LOCK_SCREEN)) {
                self.send(client, event.clone());
            }
            self.session.shell.layout_sent = Some(self.keyboard.layout_index);
        }
    }

    /// What a new shell or lock screen client learns right after `Welcome`.
    pub(super) fn shell_welcome(&mut self, client: ClientId) {
        let rights = self.clients.get(&client).map_or(Rights::NONE, |c| c.rights);
        if rights.contains(Rights::SHELL) {
            let windows = self.shell_windows();
            self.send(client, Event::WindowsChanged { windows });
            let event = self.workspaces_event();
            self.send(client, event);
        }
        if rights.contains(Rights::SHELL) || rights.contains(Rights::LOCK_SCREEN) {
            let event = self.layouts_event();
            self.send(client, event);
            if self.session.locked {
                // A lock screen that attaches to a locked session (the old one died) shows
                // itself at once (§8a).
                self.send(client, Event::SessionLocked { locked: true });
            }
        }
    }

    /// Forgets a shell surface that went (its popups go with the panel).
    pub(super) fn shell_surface_gone(&mut self, key: SurfaceKey) {
        if self.session.shell.focus == Some(key) {
            self.session.shell.focus = None;
        }
    }
}
