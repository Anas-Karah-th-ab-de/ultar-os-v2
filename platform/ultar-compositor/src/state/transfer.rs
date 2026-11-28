//! Clipboard and drag and drop between processes.
//!
//! Data flows through the compositor in bounded chunks; there is no
//! descriptor passing (the Ultar kernel has none). Security rules:
//! offering (copy, drag start) and reading (paste, drop) need a serial of a
//! recent user action delivered to that client; paste needs the keyboard
//! focus; a drop can only be read by the client it was dropped on, once,
//! within the transfer time limit.

use super::clipboard_bridge::PASSWORD_PURPOSE;
use super::input::PointerGrab;
use super::{ClientId, State, SurfaceKey};
use crate::decor::Part;
use std::collections::{HashMap, VecDeque};
use ultar_surface::proto::{ErrorCode, Event, Request, SurfaceId, TransferId, MAX_MIME};

/// One transfer in flight. Flow control is per transfer: the source is
/// asked for each next chunk (`SendRequest`) only when the reader has
/// room; chunks it sends unasked while the reader is behind are held (up
/// to [`crate::config::TransferLimits::held`]), beyond that the transfer
/// is cancelled. The source itself is always read (its other requests –
/// commits, pongs – are never held up by a slow reader).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Transfer {
    source: ClientId,
    dest: ClientId,
    mime: String,
    bytes: usize,
    deadline: u64,
    held: VecDeque<(Vec<u8>, bool)>,
    held_bytes: usize,
    asked: bool,
    /// Longest this transfer may carry (clipboard entries are smaller than
    /// a 16 MiB paste).
    limit: usize,
}

/// The current clipboard selection.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct Selection {
    /// Who provides the data.
    owner: ClientId,
    /// Offered types.
    mimes: Vec<String>,
    /// The content of a password field: never leaves the device.
    confidential: bool,
    /// Offered on behalf of another device (source: "other device").
    remote: bool,
}

/// A drop waiting to be read.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Dropped {
    source: ClientId,
    target: ClientId,
    mimes: Vec<String>,
    deadline: u64,
}

/// Clipboard owner, transfers, drag and drop.
#[derive(Debug, Default)]
pub struct Transfers {
    selection: Option<Selection>,
    drag: Option<(ClientId, Vec<String>)>,
    dropped: Option<Dropped>,
    active: HashMap<u32, Transfer>,
    next: u32,
    cancelled: Vec<(ClientId, TransferId)>,
}

impl Transfers {
    /// Types the clipboard offers now (empty = nothing).
    pub fn selection_mimes(&self) -> Vec<String> {
        self.selection
            .as_ref()
            .map(|s| s.mimes.clone())
            .unwrap_or_default()
    }

    /// The current selection is a password's content.
    pub(crate) fn selection_confidential(&self) -> bool {
        self.selection.as_ref().is_some_and(|s| s.confidential)
    }

    /// The current selection was offered on behalf of another device.
    pub(crate) fn selection_remote(&self) -> bool {
        self.selection.as_ref().is_some_and(|s| s.remote)
    }

    /// Who owns the current selection (provides its data).
    pub(crate) fn selection_owner(&self) -> Option<ClientId> {
        self.selection.as_ref().map(|s| s.owner)
    }

    /// Replaces the current selection.
    pub(crate) fn set_selection(
        &mut self,
        owner: ClientId,
        mimes: Vec<String>,
        confidential: bool,
        remote: bool,
    ) {
        self.selection = (!mimes.is_empty()).then_some(Selection {
            owner,
            mimes,
            confidential,
            remote,
        });
    }

    /// Forgets everything of a client; its transfers are cancelled.
    pub fn client_gone(&mut self, client: ClientId) {
        if self.selection.as_ref().is_some_and(|s| s.owner == client) {
            self.selection = None;
        }
        if self.drag.as_ref().is_some_and(|d| d.0 == client) {
            self.drag = None;
        }
        if self
            .dropped
            .as_ref()
            .is_some_and(|d| d.source == client || d.target == client)
        {
            self.dropped = None;
        }
        let gone: Vec<u32> = self
            .active
            .iter()
            .filter(|(_, t)| t.source == client || t.dest == client)
            .map(|(&id, _)| id)
            .collect();
        for id in gone {
            if let Some(t) = self.active.remove(&id) {
                if t.dest != client {
                    self.cancelled.push((t.dest, TransferId(id)));
                }
            }
        }
    }

    /// Ends a drag without a drop (the session was locked).
    pub fn cancel_drag(&mut self) {
        self.drag = None;
    }

    /// Transfers whose destination must be told they ended.
    pub fn take_cancelled(&mut self) -> Vec<(ClientId, TransferId)> {
        std::mem::take(&mut self.cancelled)
    }

    /// Cancels transfers past their deadline.
    pub fn expire(&mut self, now: u64) {
        let late: Vec<u32> = self
            .active
            .iter()
            .filter(|(_, t)| t.deadline <= now)
            .map(|(&id, _)| id)
            .collect();
        for id in late {
            if let Some(t) = self.active.remove(&id) {
                self.cancelled.push((t.dest, TransferId(id)));
            }
        }
        if self.dropped.as_ref().is_some_and(|d| d.deadline <= now) {
            self.dropped = None;
        }
    }

    /// Earliest deadline.
    pub fn next_deadline(&self) -> Option<u64> {
        self.active
            .values()
            .map(|t| t.deadline)
            .chain(self.dropped.iter().map(|d| d.deadline))
            .min()
    }

    /// A transfer number that is answered at once with `TransferCancelled` (nothing to read).
    pub(crate) fn empty(&mut self) -> TransferId {
        self.next = self.next.wrapping_add(1).max(1);
        TransferId(self.next)
    }

    fn start(
        &mut self,
        (source, dest): (ClientId, ClientId),
        mime: &str,
        deadline: u64,
        limit: usize,
    ) -> TransferId {
        self.next = self.next.wrapping_add(1).max(1);
        self.active.insert(
            self.next,
            Transfer {
                source,
                dest,
                mime: mime.to_owned(),
                bytes: 0,
                deadline,
                held: VecDeque::new(),
                held_bytes: 0,
                asked: true,
                limit,
            },
        );
        TransferId(self.next)
    }
}

impl State {
    fn user_action(&self, client: ClientId, serial: u32) -> bool {
        let max = self.config.limits.serial_ms;
        self.clients
            .get(&client)
            .is_some_and(|c| c.recent_serial(serial, self.now, max))
    }

    pub(super) fn focused_client(&self) -> Option<ClientId> {
        self.focus.map(|k| k.client)
    }

    pub(super) fn apply_transfer(
        &mut self,
        client: ClientId,
        request: Request,
    ) -> Result<(), ErrorCode> {
        match request {
            Request::SetSelection { serial, mimes } => {
                if !self.user_action(client, serial) || self.focused_client() != Some(client) {
                    return Err(ErrorCode::Permission);
                }
                let confidential = self.focused_field_purpose() == Some(PASSWORD_PURPOSE);
                self.transfers
                    .set_selection(client, mimes.clone(), confidential, false);
                if let Some(focus) = self.focused_client() {
                    self.send(
                        focus,
                        Event::SelectionOffer {
                            mimes: mimes.clone(),
                        },
                    );
                }
                self.notify_bridge_selection(mimes, confidential);
                Ok(())
            }
            Request::ReadSelection { serial, mime } => {
                if !self.user_action(client, serial) || self.focused_client() != Some(client) {
                    return Err(ErrorCode::Permission);
                }
                // An empty clipboard, or one without the wanted type, is not the reader's fault:
                // it gets a cancelled transfer instead of losing its connection (found in
                // VMware: Ctrl+V with nothing copied ended the app).
                let offered = self
                    .transfers
                    .selection
                    .clone()
                    .filter(|s| mime.len() <= MAX_MIME && s.mimes.contains(&mime));
                let Some(selection) = offered else {
                    let transfer = self.transfers.empty();
                    self.send(client, Event::TransferCancelled { transfer });
                    return Ok(());
                };
                self.begin_transfer(
                    selection.owner,
                    client,
                    &selection.mimes,
                    mime,
                    self.config.limits.transfer.bytes,
                )
            }
            Request::BridgeSetSelection { mimes } => self.bridge_set_selection(client, mimes),
            Request::BridgeReadSelection { mime } => self.bridge_read_selection(client, mime),
            Request::SendData {
                transfer,
                data,
                last,
            } => self.forward_data(client, transfer, data, last),
            Request::StartDrag { serial, mimes } => {
                let pressed_here = matches!(self.pointer.grab, Some(PointerGrab::Implicit(k)) if k.client == client);
                if !self.user_action(client, serial) || !pressed_here || mimes.is_empty() {
                    return Err(ErrorCode::Permission);
                }
                self.transfers.drag = Some((client, mimes));
                self.pointer.grab = Some(PointerGrab::Drag(client));
                Ok(())
            }
            Request::ReadDrop { mime } => {
                let Some(d) = self
                    .transfers
                    .dropped
                    .clone()
                    .filter(|d| d.target == client)
                else {
                    return Err(ErrorCode::Permission);
                };
                self.transfers.dropped = None;
                self.begin_transfer(
                    d.source,
                    client,
                    &d.mimes,
                    mime,
                    self.config.limits.transfer.bytes,
                )
            }
            _ => Err(ErrorCode::Malformed),
        }
    }

    /// True while a transfer of `client` waits for its reader (data held
    /// back, or the reader has more than
    /// [`crate::config::TransferLimits::queue`] bytes queued). The source
    /// is still read; only that transfer waits.
    pub fn throttled(&self, client: ClientId) -> bool {
        self.transfers
            .active
            .values()
            .any(|t| t.source == client && (!t.held.is_empty() || self.reader_full(t.dest)))
    }

    fn reader_full(&self, dest: ClientId) -> bool {
        let limit = self.config.limits.transfer.queue;
        self.clients
            .get(&dest)
            .is_none_or(|c| c.conn.pending() > limit)
    }

    pub(crate) fn begin_transfer(
        &mut self,
        source: ClientId,
        dest: ClientId,
        offered: &[String],
        mime: String,
        limit: usize,
    ) -> Result<(), ErrorCode> {
        if mime.len() > MAX_MIME || !offered.contains(&mime) {
            return Err(ErrorCode::UnknownObject);
        }
        if self.transfers.active.values().any(|t| t.dest == dest) {
            return Err(ErrorCode::Limit);
        }
        let deadline = self.now + self.config.limits.transfer.ms;
        let id = self.transfers.start((source, dest), &mime, deadline, limit);
        self.send(source, Event::SendRequest { transfer: id, mime });
        Ok(())
    }

    fn forward_data(
        &mut self,
        client: ClientId,
        transfer: TransferId,
        data: Vec<u8>,
        last: bool,
    ) -> Result<(), ErrorCode> {
        let limits = self.config.limits.transfer;
        let Some(t) = self.transfers.active.get_mut(&transfer.0) else {
            // Late data for a cancelled transfer is dropped quietly.
            return Ok(());
        };
        if t.source != client {
            return Err(ErrorCode::Permission);
        }
        t.bytes += data.len();
        t.asked = false;
        if t.bytes > t.limit || t.held_bytes + data.len() > limits.held {
            // Too big, or sent unasked faster than the reader reads.
            self.cancel_transfer(transfer);
            return Ok(());
        }
        t.held_bytes += data.len();
        t.held.push_back((data, last));
        self.pump_transfer(transfer.0);
        Ok(())
    }

    /// Ends a transfer early; both sides are told.
    fn cancel_transfer(&mut self, transfer: TransferId) {
        if let Some(t) = self.transfers.active.remove(&transfer.0) {
            self.send(t.dest, Event::TransferCancelled { transfer });
            self.send(t.source, Event::TransferCancelled { transfer });
        }
    }

    /// Moves held chunks of one transfer to its reader while the reader has
    /// room, then asks the source for the next chunk.
    fn pump_transfer(&mut self, id: u32) {
        loop {
            let Some(t) = self.transfers.active.get(&id) else {
                return;
            };
            let (dest, source) = (t.dest, t.source);
            if self.reader_full(dest) {
                return;
            }
            let Some(t) = self.transfers.active.get_mut(&id) else {
                return;
            };
            let Some((data, last)) = t.held.pop_front() else {
                if !t.asked {
                    t.asked = true;
                    let mime = t.mime.clone();
                    self.send(
                        source,
                        Event::SendRequest {
                            transfer: TransferId(id),
                            mime,
                        },
                    );
                }
                return;
            };
            t.held_bytes -= data.len();
            if last {
                self.transfers.active.remove(&id);
            }
            let transfer = TransferId(id);
            self.send(
                dest,
                Event::Data {
                    transfer,
                    data,
                    last,
                },
            );
        }
    }

    /// The pointer was released during a drag: offer the data to the
    /// surface under it.
    pub(super) fn drop_on_pointer(&mut self, source: ClientId) {
        let Some((_, mimes)) = self.transfers.drag.take() else {
            return;
        };
        let Some((key, Part::Content, content)) = self.hit_test(self.pointer.x, self.pointer.y)
        else {
            return;
        };
        let x = ((self.pointer.x - f64::from(content.x)) * 256.0) as i32;
        let y = ((self.pointer.y - f64::from(content.y)) * 256.0) as i32;
        self.transfers.dropped = Some(Dropped {
            source,
            target: key.client,
            mimes: mimes.clone(),
            deadline: self.now + self.config.limits.transfer.ms,
        });
        self.send(
            key.client,
            Event::DropOffer {
                surface: key.surface,
                x,
                y,
                mimes,
            },
        );
    }

    /// `Move`/`Resize` from a client (its own header area): only while the
    /// press with that serial is still held on that surface.
    pub(super) fn start_move_request(
        &mut self,
        client: ClientId,
        surface: SurfaceId,
        serial: u32,
        edges: Option<u8>,
    ) -> Result<(), ErrorCode> {
        let key = SurfaceKey { client, surface };
        if self.window_index(key).is_none() {
            return Err(ErrorCode::UnknownObject);
        }
        let held_here = matches!(self.pointer.grab, Some(PointerGrab::Implicit(k)) if k == key);
        if !held_here || !self.user_action(client, serial) {
            // Not an error: the press may have ended before the request came.
            return Ok(());
        }
        match edges {
            None => self.begin_move(key),
            Some(e) => {
                let grab = self.resize_grab(key, e & 15);
                self.pointer.grab = Some(grab);
            }
        }
        Ok(())
    }

    /// Sends cancellations, expires transfers and moves held data on to
    /// readers that have room again (called by the loop).
    pub fn tick_transfers(&mut self) {
        self.transfers.expire(self.now);
        for (to, id) in self.transfers.take_cancelled() {
            self.send(to, Event::TransferCancelled { transfer: id });
        }
        let ids: Vec<u32> = self.transfers.active.keys().copied().collect();
        for id in ids {
            self.pump_transfer(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfers_expire_and_clients_are_forgotten() {
        let mut t = Transfers {
            selection: Some(Selection {
                owner: 1,
                mimes: vec!["text/plain".into()],
                confidential: false,
                remote: false,
            }),
            ..Transfers::default()
        };
        assert_eq!(t.selection_mimes(), vec!["text/plain".to_string()]);
        let a = t.start((1, 2), "text/plain", 100, 1000);
        let _b = t.start((3, 4), "text/plain", 50, 1000);
        assert_eq!(t.next_deadline(), Some(50));
        t.expire(60);
        assert_eq!(t.take_cancelled(), vec![(4, TransferId(2))]);
        t.client_gone(1);
        assert!(t.selection_mimes().is_empty());
        assert_eq!(t.take_cancelled(), vec![(2, a)]);
        assert_eq!(t.next_deadline(), None);
    }
}
