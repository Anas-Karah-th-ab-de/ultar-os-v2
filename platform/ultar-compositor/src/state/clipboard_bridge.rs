//! The clipboard bridge: the one client system service that may read the
//! clipboard without having the keyboard focus, and offer a selection whose
//! content it provides itself. That is the device mesh (`ultar-mesh`,
//! right `ui.clipboard-bridge`): it copies a selection to the paired devices
//! and offers entries copied there for local pasting.
//!
//! Rules (docs/integration/mesh-clipboard.md):
//! - The bridge is told of a user copy (`Event::SelectionChange`) but never
//!   while the session is locked, and a confidential selection (the content
//!   of a password field) is never offered to it – it fails closed.
//! - Text travels as `text/plain;charset=utf-8` up to [`CLIP_TEXT_MAX`];
//!   pictures as `image/png` up to [`CLIP_IMAGE_MAX`].
//! - A selection the bridge offers is marked as coming from another device.

use super::{ClientId, State};
use ultar_surface::proto::{ErrorCode, Event, Rights, CLIP_IMAGE_MIME, MAX_MIME};

/// Kind code of a password field (`ultar-ime` `Purpose::password`).
pub const PASSWORD_PURPOSE: u8 = 2;
/// Longest clipboard text the bridge reads.
pub const CLIP_TEXT_MAX: usize = 16 * 1024;
/// Longest clipboard picture the bridge reads.
pub const CLIP_IMAGE_MAX: usize = 4 * 1024 * 1024;

impl State {
    /// The kind of the focused text field (`None`: focus is not in one).
    pub(super) fn focused_field_purpose(&self) -> Option<u8> {
        let key = self.focus?;
        self.surface(key).and_then(|s| s.text_input)
    }

    /// The bridge offers a selection whose data it provides (copy came from
    /// another device). Refused without the right or while locked.
    pub(super) fn bridge_set_selection(
        &mut self,
        client: ClientId,
        mimes: Vec<String>,
    ) -> Result<(), ErrorCode> {
        if !self.has_bridge_right(client) {
            return Err(ErrorCode::Permission);
        }
        if self.locked() {
            return Err(ErrorCode::Permission);
        }
        self.transfers.set_selection(client, mimes.clone(), false, true);
        if let Some(focus) = self.focused_client() {
            self.send(focus, Event::SelectionOffer { mimes });
        }
        Ok(())
    }

    /// The bridge reads the current selection's data (copy left the device
    /// for the paired devices). A confidential selection is never given out.
    pub(super) fn bridge_read_selection(
        &mut self,
        client: ClientId,
        mime: String,
    ) -> Result<(), ErrorCode> {
        if !self.has_bridge_right(client) {
            return Err(ErrorCode::Permission);
        }
        if self.locked() {
            return Err(ErrorCode::Permission);
        }
        let confidential = self.transfers.selection_confidential();
        let offered = self.transfers.selection_mimes();
        if confidential || mime.len() > MAX_MIME || !offered.contains(&mime) {
            let transfer = self.transfers.empty();
            self.send(client, Event::TransferCancelled { transfer });
            return Ok(());
        }
        let Some(owner) = self.transfers.selection_owner() else {
            let transfer = self.transfers.empty();
            self.send(client, Event::TransferCancelled { transfer });
            return Ok(());
        };
        let limit = if mime == CLIP_IMAGE_MIME {
            CLIP_IMAGE_MAX
        } else {
            CLIP_TEXT_MAX
        };
        self.begin_transfer(owner, client, &offered, mime, limit)
    }

    fn has_bridge_right(&self, client: ClientId) -> bool {
        self.clients
            .get(&client)
            .is_some_and(|c| c.rights.contains(Rights::CLIPBOARD_BRIDGE))
    }

    /// Tells every bridge client of a user copy. Sent only from
    /// [`Request::SetSelection`], which already needs the focus, a recent
    /// user action and (indirectly) an unlocked session.
    pub(super) fn notify_bridge_selection(&mut self, mimes: Vec<String>, confidential: bool) {
        let ids: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, c)| c.rights.contains(Rights::CLIPBOARD_BRIDGE))
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            self.send(
                id,
                Event::SelectionChange {
                    mimes: mimes.clone(),
                    confidential,
                },
            );
        }
    }
}