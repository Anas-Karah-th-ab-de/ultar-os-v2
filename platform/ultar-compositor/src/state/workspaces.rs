//! Workspaces and window thumbnails for the desktop shell's overview (board Overview,
//! CLICKS `overview.workspace`, `overview.window`). Only a client with the shell right may
//! switch workspaces, move windows between them or read thumbnails; nothing of it works while
//! the session is locked (fail closed), and system dialogs are on every workspace and never
//! copied.

use super::{ClientId, State};
use crate::geom::Rect;
use crate::pixels::{self, Image};
use ultar_surface::proto::{
    ErrorCode, Event, Rights, ALL_WORKSPACES, MAX_THUMBNAILS_PER_SECOND, MAX_THUMBNAIL_SIDE,
    MAX_WORKSPACES,
};

/// Which workspace is shown and how many exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Workspaces {
    /// Shown, from 0.
    pub current: u8,
    /// How many (1 ..= `MAX_WORKSPACES`).
    pub count: u8,
}

impl Default for Workspaces {
    fn default() -> Workspaces {
        Workspaces {
            current: 0,
            count: 1,
        }
    }
}

/// The size of a copy of `w × h` that fits `max_w × max_h` without growing.
pub fn fit(w: u32, h: u32, max_w: u16, max_h: u16) -> (u16, u16) {
    let (mw, mh) = (
        u64::from(max_w.min(MAX_THUMBNAIL_SIDE)),
        u64::from(max_h.min(MAX_THUMBNAIL_SIDE)),
    );
    let (w, h) = (u64::from(w.max(1)), u64::from(h.max(1)));
    // scale = min(mw / w, mh / h, 1), in integers
    let (tw, th) = if w * mh <= h * mw {
        (w * mh / h, mh)
    } else {
        (mw, h * mw / w)
    };
    let (tw, th) = if tw >= w || th >= h { (w, h) } else { (tw, th) };
    (tw.clamp(1, mw) as u16, th.clamp(1, mh) as u16)
}

impl State {
    /// The workspaces now.
    pub fn workspaces(&self) -> Workspaces {
        self.session.shell.workspaces
    }

    fn shell_only(&self, client: ClientId) -> Result<(), ErrorCode> {
        match self.clients.get(&client) {
            Some(c) if !c.rights.contains(Rights::SHELL) => Err(ErrorCode::Permission),
            Some(_) if self.locked() => Err(ErrorCode::Permission),
            Some(_) => Ok(()),
            None => Err(ErrorCode::UnknownObject),
        }
    }

    /// `SwitchWorkspace`: shows `index`; one past the last adds a workspace.
    pub(super) fn switch_workspace(
        &mut self,
        client: ClientId,
        index: u8,
    ) -> Result<(), ErrorCode> {
        self.shell_only(client)?;
        self.show_workspace(index)
    }

    /// Shows workspace `index` (one past the last adds one, up to the limit).
    pub(super) fn show_workspace(&mut self, index: u8) -> Result<(), ErrorCode> {
        let ws = &mut self.session.shell.workspaces;
        if index == ws.count && usize::from(ws.count) < MAX_WORKSPACES {
            ws.count += 1;
        } else if index >= ws.count {
            return Err(ErrorCode::Limit);
        }
        ws.current = index;
        self.apply_workspaces();
        Ok(())
    }

    /// `MoveToWorkspace`: puts a window on `index` (one past the last adds one) or on all.
    pub(super) fn move_to_workspace(
        &mut self,
        client: ClientId,
        window: u32,
        index: u8,
    ) -> Result<(), ErrorCode> {
        self.shell_only(client)?;
        let Some(i) = self
            .windows
            .iter()
            .position(|w| w.id == window && !w.secure)
        else {
            return Ok(());
        };
        let ws = &mut self.session.shell.workspaces;
        if index == ws.count && usize::from(ws.count) < MAX_WORKSPACES {
            ws.count += 1;
        } else if index >= ws.count && index != ALL_WORKSPACES {
            return Err(ErrorCode::Limit);
        }
        self.windows[i].place.workspace = index;
        self.apply_workspaces();
        Ok(())
    }

    /// The workspace a new window opens on: system dialogs on all, others on the shown one.
    pub(super) fn new_window_workspace(&self, secure: bool) -> u8 {
        if secure {
            ALL_WORKSPACES
        } else {
            self.session.shell.workspaces.current
        }
    }

    /// Shows the windows of the current workspace, hides the others, drops empty workspaces
    /// at the end (never the shown one) and moves the focus when its window was hidden.
    pub(super) fn apply_workspaces(&mut self) {
        let current = self.session.shell.workspaces.current;
        for w in &mut self.windows {
            w.place.shown =
                w.secure || w.place.workspace == ALL_WORKSPACES || w.place.workspace == current;
        }
        let used = self
            .windows
            .iter()
            .filter(|w| w.place.workspace != ALL_WORKSPACES)
            .map(|w| w.place.workspace)
            .max()
            .unwrap_or(0);
        let ws = &mut self.session.shell.workspaces;
        ws.count = ws.count.min(used.max(current).saturating_add(1)).max(1);
        let focused_hidden = self
            .focus
            .and_then(|k| self.window_index(k))
            .is_some_and(|i| !self.windows[i].visible());
        if focused_hidden {
            self.focus_topmost();
        }
        self.damage_all();
    }

    /// Before activating window `i`: shows its workspace.
    pub(super) fn reveal_window(&mut self, i: usize) {
        let ws = self.windows[i].place.workspace;
        if ws != ALL_WORKSPACES && ws != self.session.shell.workspaces.current {
            self.session.shell.workspaces.current = ws;
            self.apply_workspaces();
        }
    }

    /// `WindowThumbnail`: a scaled copy of a window's content, at most
    /// `MAX_THUMBNAILS_PER_SECOND` per client.
    pub(super) fn window_thumbnail(
        &mut self,
        client: ClientId,
        window: u32,
        max: (u16, u16),
    ) -> Result<(), ErrorCode> {
        self.shell_only(client)?;
        let second = self.now / 1000;
        let budget = self
            .session
            .shell
            .thumbs
            .entry(client)
            .or_insert((second, 0));
        if budget.0 != second {
            *budget = (second, 0);
        }
        if budget.1 >= MAX_THUMBNAILS_PER_SECOND {
            return Err(ErrorCode::Limit);
        }
        budget.1 += 1;
        let Some(w) = self
            .windows
            .iter()
            .find(|w| w.id == window && !w.secure && w.mapped)
        else {
            return Ok(());
        };
        let Some(image) = self.surface(w.key).and_then(|s| s.image.as_ref()) else {
            return Ok(());
        };
        let (tw, th) = fit(image.width(), image.height(), max.0, max.1);
        let Some(mut copy) = Image::new(u32::from(tw), u32::from(th), 0xFF00_0000) else {
            return Ok(());
        };
        let dest = Rect::new(0, 0, i32::from(tw), i32::from(th));
        pixels::scale(&mut copy, dest, &dest, image, true);
        let data = copy.to_bytes(dest);
        let event = Event::WindowThumbnail {
            window,
            width: tw,
            height: th,
            data,
        };
        self.send(client, event);
        Ok(())
    }

    /// The overview's requests (thumbnails, workspaces); others are ignored.
    pub(super) fn apply_overview(
        &mut self,
        client: ClientId,
        request: ultar_surface::proto::Request,
    ) -> Result<(), ErrorCode> {
        use ultar_surface::proto::Request;
        match request {
            Request::WindowThumbnail {
                window,
                max_width,
                max_height,
            } => self.window_thumbnail(client, window, (max_width, max_height)),
            Request::SwitchWorkspace { index } => self.switch_workspace(client, index),
            Request::MoveToWorkspace { window, index } => {
                self.move_to_workspace(client, window, index)
            }
            _ => Ok(()),
        }
    }

    /// The workspaces as an event for shell clients.
    pub(super) fn workspaces_event(&self) -> Event {
        let ws = self.session.shell.workspaces;
        Event::Workspaces {
            current: ws.current,
            count: ws.count,
        }
    }

    /// A shell client that went: its thumbnail budget goes with it.
    pub(super) fn forget_thumbnail_budget(&mut self, client: ClientId) {
        self.session.shell.thumbs.remove(&client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_fits_its_box_keeps_its_shape_and_never_grows() {
        assert_eq!(fit(1440, 900, 448, 288), (448, 280));
        assert_eq!(fit(900, 1440, 448, 288), (180, 288));
        assert_eq!(fit(100, 50, 448, 288), (100, 50));
        assert_eq!(fit(0, 0, 448, 288), (1, 1));
        assert_eq!(fit(8192, 1, 512, 512), (512, 1));
        assert_eq!(fit(1440, 900, 4096, 4096), (512, 320));
    }
}
