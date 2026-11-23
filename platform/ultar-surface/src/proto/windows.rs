//! The window list a privileged client reads (`Request::ListWindows`, needs
//! the capture right): which windows exist, whether they are shown and how
//! their pixels arrive. Test and diagnosis tools count windows with it
//! instead of guessing from visible pixels.

use super::types::{MAX_APP_ID, MAX_TITLE};
use crate::wire::{Reader, WireError, Writer};

/// Most windows in one `WindowList` (the rest is left out).
pub const MAX_LISTED_WINDOWS: usize = 64;

/// One toplevel window of the list.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WindowEntry {
    /// Application id of its client (a hint, not an identity).
    pub app_id: String,
    /// Title.
    pub title: String,
    /// Shown (has content), even when other windows cover it.
    pub mapped: bool,
    /// Its last committed pixels came from a shared-memory buffer.
    pub shared_memory: bool,
}

/// Writes a window list (at most [`MAX_LISTED_WINDOWS`] entries).
pub(crate) fn encode_list(w: &mut Writer, list: &[WindowEntry]) {
    let n = list.len().min(MAX_LISTED_WINDOWS);
    w.u8(n as u8);
    for e in &list[..n] {
        w.str(&e.app_id, MAX_APP_ID).str(&e.title, MAX_TITLE);
        w.bool(e.mapped).bool(e.shared_memory);
    }
}

/// Reads a window list.
pub(crate) fn decode_list(r: &mut Reader<'_>) -> Result<Vec<WindowEntry>, WireError> {
    let n = usize::from(r.u8()?);
    if n > MAX_LISTED_WINDOWS {
        return Err(WireError::TooLong);
    }
    (0..n)
        .map(|_| {
            Ok(WindowEntry {
                app_id: r.str(MAX_APP_ID)?,
                title: r.str(MAX_TITLE)?,
                mapped: r.bool()?,
                shared_memory: r.bool()?,
            })
        })
        .collect()
}
