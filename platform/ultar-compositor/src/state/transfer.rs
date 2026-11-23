//! Clipboard and drag and drop between processes.
//!
//! Data flows through the compositor in bounded chunks; there is no
//! descriptor passing (the Ultar kernel has none). Security rules:
//! offering (copy, drag start) and reading (paste, drop) need a serial of a
//! recent user action delivered to that client; paste needs the keyboard
//! focus; a drop can only be read by the client it was dropped on, once,
//! within the transfer time limit.

use super::input::PointerGrab;
use super::{ClientId, State, SurfaceKey};
use crate::decor::Part;
use std::collections::HashMap;
use ultar_surface::proto::{ErrorCode, Event, Request, SurfaceId, TransferId, MAX_MIME};

/// One transfer in flight.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Transfer {
    source: ClientId,
    dest: ClientId,
    bytes: usize,
    deadline: u64,
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
    selection: Option<(ClientId, Vec<String>)>,
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
            .map(|s| s.1.clone())
            .unwrap_or_default()
    }

    /// Forgets everything of a client; its transfers are cancelled.
    pub fn client_gone(&mut self, client: ClientId) {
        if self.selection.as_ref().is_some_and(|s| s.0 == client) {
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

    /// Readers of the transfers `source` is sending (flow control: the
    /// source is not read while one of them has too much queued).
    pub fn readers_of(&self, source: ClientId) -> Vec<ClientId> {
        self.active
            .values()
            .filter(|t| t.source == source)
            .map(|t| t.dest)
            .collect()
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

    fn start(&mut self, source: ClientId, dest: ClientId, deadline: u64) -> TransferId {
        self.next = self.next.wrapping_add(1).max(1);
        self.active.insert(
            self.next,
            Transfer {
                source,
                dest,
                bytes: 0,
                deadline,
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

    fn focused_client(&self) -> Option<ClientId> {
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
                self.transfers.selection = (!mimes.is_empty()).then(|| (client, mimes.clone()));
                if let Some(focus) = self.focused_client() {
                    self.send(focus, Event::SelectionOffer { mimes });
                }
                Ok(())
            }
            Request::ReadSelection { serial, mime } => {
                if !self.user_action(client, serial) || self.focused_client() != Some(client) {
                    return Err(ErrorCode::Permission);
                }
                let Some((owner, mimes)) = self.transfers.selection.clone() else {
                    return Err(ErrorCode::UnknownObject);
                };
                self.begin_transfer(owner, client, &mimes, mime)
            }
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
                self.begin_transfer(d.source, client, &d.mimes, mime)
            }
            _ => Err(ErrorCode::Malformed),
        }
    }

    /// True while a reader of `client`'s transfers has more than
    /// [`crate::config::Limits::transfer_queue`] bytes queued: the loop
    /// does not read `client` then (its socket fills and it waits).
    pub fn throttled(&self, client: ClientId) -> bool {
        let limit = self.config.limits.transfer_queue;
        self.transfers.readers_of(client).iter().any(|dest| {
            self.clients
                .get(dest)
                .is_some_and(|c| c.conn.pending() > limit)
        })
    }

    fn begin_transfer(
        &mut self,
        source: ClientId,
        dest: ClientId,
        offered: &[String],
        mime: String,
    ) -> Result<(), ErrorCode> {
        if mime.len() > MAX_MIME || !offered.contains(&mime) {
            return Err(ErrorCode::UnknownObject);
        }
        if self.transfers.active.values().any(|t| t.dest == dest) {
            return Err(ErrorCode::Limit);
        }
        let deadline = self.now + self.config.limits.transfer_ms;
        let id = self.transfers.start(source, dest, deadline);
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
        let limit = self.config.limits.transfer_bytes;
        let Some(t) = self.transfers.active.get_mut(&transfer.0) else {
            // Late data for a cancelled transfer is dropped quietly.
            return Ok(());
        };
        if t.source != client {
            return Err(ErrorCode::Permission);
        }
        t.bytes += data.len();
        let dest = t.dest;
        if t.bytes > limit {
            self.transfers.active.remove(&transfer.0);
            self.send(dest, Event::TransferCancelled { transfer });
            return Ok(());
        }
        if last {
            self.transfers.active.remove(&transfer.0);
        }
        self.send(
            dest,
            Event::Data {
                transfer,
                data,
                last,
            },
        );
        Ok(())
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
            deadline: self.now + self.config.limits.transfer_ms,
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

    /// Sends cancellations and expires transfers (called by the loop).
    pub fn tick_transfers(&mut self) {
        self.transfers.expire(self.now);
        for (to, id) in self.transfers.take_cancelled() {
            self.send(to, Event::TransferCancelled { transfer: id });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfers_expire_and_clients_are_forgotten() {
        let mut t = Transfers {
            selection: Some((1, vec!["text/plain".into()])),
            ..Transfers::default()
        };
        assert_eq!(t.selection_mimes(), vec!["text/plain".to_string()]);
        let a = t.start(1, 2, 100);
        let _b = t.start(3, 4, 50);
        assert_eq!(t.next_deadline(), Some(50));
        t.expire(60);
        assert_eq!(t.take_cancelled(), vec![(4, TransferId(2))]);
        t.client_gone(1);
        assert!(t.selection_mimes().is_empty());
        assert_eq!(t.take_cancelled(), vec![(2, a)]);
        assert_eq!(t.next_deadline(), None);
    }
}
