//! The part of the protocol only the desktop shell uses (right
//! [`Rights::SHELL`](super::Rights::SHELL)): the window list of the bar,
//! acting on other clients' windows, the display mode, keyboard layouts and
//! the shell's own keyboard focus. Every type here is bounded like the rest
//! of the protocol.

use super::types::{OutputId, WindowState, MAX_APP_ID, MAX_TITLE};
use crate::wire::{Reader, WireError, Writer};

/// Most windows in one `WindowsChanged` (the rest is left out).
pub const MAX_SHELL_WINDOWS: usize = 64;
/// Largest side of a window thumbnail in pixels (512 × 512 × 4 = 1 MiB).
pub const MAX_THUMBNAIL_SIDE: u16 = 512;
/// Most thumbnails one client gets per second (the rest is refused with `Limit`).
pub const MAX_THUMBNAILS_PER_SECOND: u32 = 32;
/// Most workspaces.
pub const MAX_WORKSPACES: usize = 8;
/// Workspace index of a window shown on every workspace.
pub const ALL_WORKSPACES: u8 = u8::MAX;
/// Most keyboard layouts announced.
pub const MAX_LAYOUTS: usize = 8;
/// Longest layout name (`ar`, `en`, `de-neo` …).
pub const MAX_LAYOUT_NAME: usize = 32;

/// One toplevel window as the shell sees it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShellWindow {
    /// Stable id of the window while it exists (chosen by the compositor,
    /// never reused for another window of the same session).
    pub id: u32,
    /// Application id of its client (a hint for grouping, not an identity).
    pub app_id: String,
    /// Title.
    pub title: String,
    /// State.
    pub state: WindowState,
    /// Has the keyboard focus.
    pub focused: bool,
    /// The app does not answer pings (title muted).
    pub unresponsive: bool,
    /// Output holding most of it.
    pub output: OutputId,
    /// Its workspace, or [`ALL_WORKSPACES`].
    pub workspace: u8,
}

/// What the shell does to a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowAction {
    /// Restores it if minimised, raises it and gives it the focus.
    Activate,
    /// Minimises it.
    Minimize,
    /// Maximises it, or restores a maximised window.
    ToggleMaximize,
    /// Asks its app to close it (`CloseRequested`, like the close button).
    Close,
    /// Phone: the back gesture – the window's app gets Escape (press and release), which
    /// closes its dialog or goes back one step.
    Back,
}

impl WindowAction {
    pub(crate) fn encode(self, w: &mut Writer) {
        w.u8(match self {
            WindowAction::Activate => 0,
            WindowAction::Minimize => 1,
            WindowAction::ToggleMaximize => 2,
            WindowAction::Close => 3,
            WindowAction::Back => 4,
        });
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<WindowAction, WireError> {
        Ok(match r.u8()? {
            0 => WindowAction::Activate,
            1 => WindowAction::Minimize,
            2 => WindowAction::ToggleMaximize,
            3 => WindowAction::Close,
            4 => WindowAction::Back,
            _ => return Err(WireError::BadValue),
        })
    }
}

/// The screen edge a phone gesture started from (`Event::EdgeSwipe`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SwipeEdge {
    /// The bottom edge, swiped up: home; swiped up and held: recent apps.
    Bottom,
    /// A side edge, swiped inwards: back.
    Side,
}

impl SwipeEdge {
    pub(crate) fn to_u8(self) -> u8 {
        match self {
            SwipeEdge::Bottom => 0,
            SwipeEdge::Side => 1,
        }
    }

    pub(crate) fn from_u8(v: u8) -> Result<SwipeEdge, WireError> {
        match v {
            0 => Ok(SwipeEdge::Bottom),
            1 => Ok(SwipeEdge::Side),
            _ => Err(WireError::BadValue),
        }
    }
}

/// How several displays are used (DESKTOP-SPEC §8 "Modus", Super+P).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisplayModeRequest {
    /// One large desktop over all outputs.
    Extend,
    /// Every output shows what the primary shows.
    Mirror,
    /// Only this output is on.
    Single(OutputId),
}

impl DisplayModeRequest {
    pub(crate) fn encode(self, w: &mut Writer) {
        match self {
            DisplayModeRequest::Extend => w.u8(0).u32(0),
            DisplayModeRequest::Mirror => w.u8(1).u32(0),
            DisplayModeRequest::Single(o) => w.u8(2).u32(o.0),
        };
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<DisplayModeRequest, WireError> {
        let kind = r.u8()?;
        let output = OutputId(r.u32()?);
        Ok(match kind {
            0 => DisplayModeRequest::Extend,
            1 => DisplayModeRequest::Mirror,
            2 => DisplayModeRequest::Single(output),
            _ => return Err(WireError::BadValue),
        })
    }
}

/// Writes a shell window list (at most [`MAX_SHELL_WINDOWS`] entries).
pub(crate) fn encode_windows(w: &mut Writer, list: &[ShellWindow]) {
    let n = list.len().min(MAX_SHELL_WINDOWS);
    w.u8(n as u8);
    for e in &list[..n] {
        w.u32(e.id)
            .str(&e.app_id, MAX_APP_ID)
            .str(&e.title, MAX_TITLE);
        e.state.encode(w);
        w.bool(e.focused).bool(e.unresponsive).u32(e.output.0);
        w.u8(e.workspace);
    }
}

/// Reads a shell window list.
pub(crate) fn decode_windows(r: &mut Reader<'_>) -> Result<Vec<ShellWindow>, WireError> {
    let n = usize::from(r.u8()?);
    if n > MAX_SHELL_WINDOWS {
        return Err(WireError::TooLong);
    }
    (0..n)
        .map(|_| {
            Ok(ShellWindow {
                id: r.u32()?,
                app_id: r.str(MAX_APP_ID)?,
                title: r.str(MAX_TITLE)?,
                state: WindowState::decode(r)?,
                focused: r.bool()?,
                unresponsive: r.bool()?,
                output: OutputId(r.u32()?),
                workspace: workspace(r.u8()?)?,
            })
        })
        .collect()
}

/// A workspace index on the wire: below [`MAX_WORKSPACES`] or [`ALL_WORKSPACES`].
fn workspace(index: u8) -> Result<u8, WireError> {
    if usize::from(index) < MAX_WORKSPACES || index == ALL_WORKSPACES {
        Ok(index)
    } else {
        Err(WireError::BadValue)
    }
}

/// Writes keyboard layout names (at most [`MAX_LAYOUTS`]).
pub(crate) fn encode_layouts(w: &mut Writer, names: &[String], active: u8) {
    let n = names.len().min(MAX_LAYOUTS);
    w.u8(n as u8);
    for name in &names[..n] {
        w.str(name, MAX_LAYOUT_NAME);
    }
    w.u8(active);
}

/// Reads keyboard layout names and the active index.
pub(crate) fn decode_layouts(r: &mut Reader<'_>) -> Result<(Vec<String>, u8), WireError> {
    let n = usize::from(r.u8()?);
    if n > MAX_LAYOUTS {
        return Err(WireError::TooLong);
    }
    let names = (0..n)
        .map(|_| r.str(MAX_LAYOUT_NAME))
        .collect::<Result<Vec<_>, _>>()?;
    let active = r.u8()?;
    if n > 0 && usize::from(active) >= n {
        return Err(WireError::BadValue);
    }
    Ok((names, active))
}
