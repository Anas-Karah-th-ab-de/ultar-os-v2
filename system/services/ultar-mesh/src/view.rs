//! Remote view (mesh item C1): a paired device streams its screen and takes the pointer
//! and keys of another device.
//!
//! The *viewer* asks (`ViewStart`); the *source* streams. The source refuses a device it
//! has not granted [`Right::Control`] (off after pairing), then asks its person. While a
//! session runs, the source sends only damaged rectangles, losslessly QOI-compressed
//! ([`crate::codec`]), and takes back the viewer's input. Flow control: the source keeps
//! at most [`WINDOW`] frames in flight, the viewer acknowledges in order, so a slow link
//! *drops* frames instead of buffering without limit. Either side may stop; a lost link
//! ends the session.

use crate::codec::{self, BYTES_PER_PIXEL};
use crate::device;
use crate::node::{Effect, Node};
use crate::wire::{self, Frame};
use ultar_bus::messages::mesh::Right;

/// Frames the source keeps in flight before it drops new ones (slow-link flow control).
pub const WINDOW: u32 = 4;
/// Longest input event the source takes from a viewer.
pub const MAX_INPUT: usize = 256;
/// Largest rectangle a frame may carry (in pixels); the source tiles below this.
pub const MAX_REGION_PIXELS: usize = 128 * 128;

/// How a session ended, for the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewEnd {
    /// The source refused (no right, busy, or its person declined).
    Refused,
    /// One side stopped it.
    Stopped,
    /// The link went away (or the source locked).
    Lost,
}

/// A session this device watches (it is the viewer).
#[derive(Debug)]
pub(crate) struct Viewing {
    pub(crate) peer: String,
    pub(crate) id: u64,
}

/// A session this device serves (it is the source).
#[derive(Debug)]
pub(crate) struct Served {
    pub(crate) peer: String,
    pub(crate) id: u64,
    pub(crate) seq: u32,
    pub(crate) acked: u32,
    pub(crate) streaming: bool,
}

impl Node {
    /// Asks the paired, reachable `device` to show and take this device's input. The id
    /// of the session, or `None` when it cannot be sent (already viewing, not paired).
    pub fn view_start(&mut self, device: &str) -> Option<(u64, Vec<Effect>)> {
        if self.viewing.is_some() || !self.paired(device) {
            return None;
        }
        self.view_id += 1;
        let id = self.view_id;
        self.viewing = Some(Viewing {
            peer: device.to_owned(),
            id,
        });
        Some((id, vec![Effect::Send(device.to_owned(), Frame::ViewStart { id })]))
    }

    /// Sends the viewer's pointer/keys to the source; `None` when no session runs or the
    /// event is empty.
    pub fn view_input(&mut self, input: &str) -> Option<Vec<Effect>> {
        let input = input.trim().to_owned();
        let v = self.viewing.as_ref()?;
        if input.is_empty() || input.len() > MAX_INPUT {
            return None;
        }
        Some(vec![Effect::Send(
            v.peer.clone(),
            Frame::ViewInput { id: v.id, input },
        )])
    }

    /// The viewer ends the session.
    pub fn view_stop(&mut self) -> Option<Vec<Effect>> {
        let v = self.viewing.take()?;
        Some(vec![
            Effect::Send(v.peer.clone(), Frame::ViewStop { id: v.id }),
            Effect::ViewEnded {
                peer: v.peer,
                id: v.id,
                end: ViewEnd::Stopped,
            },
        ])
    }

    /// The person on the source accepted the session `id` of `peer`; the stream begins at
    /// `width` × `height`. A zero size refuses.
    pub fn view_accept(&mut self, peer: &str, id: u64, width: u32, height: u32) -> Vec<Effect> {
        let taken = self
            .served
            .as_ref()
            .is_some_and(|s| s.peer == peer && s.id == id && !s.streaming);
        if !taken {
            return Vec::new();
        }
        if width == 0 || height == 0 {
            self.served = None;
            return vec![Effect::Send(peer.to_owned(), Frame::ViewRefuse { id })];
        }
        if let Some(s) = self.served.as_mut() {
            s.streaming = true;
        }
        vec![
            Effect::ViewStarted {
                peer: peer.to_owned(),
                id,
            },
            Effect::Send(
                peer.to_owned(),
                Frame::ViewAccept {
                    id,
                    width,
                    height,
                },
            ),
        ]
    }

    /// The person on the source declined the session `id` of `peer`.
    pub fn view_refuse(&mut self, peer: &str, id: u64) -> Vec<Effect> {
        let taken = self
            .served
            .as_ref()
            .is_some_and(|s| s.peer == peer && s.id == id && !s.streaming);
        if !taken {
            return Vec::new();
        }
        self.served = None;
        vec![Effect::Send(peer.to_owned(), Frame::ViewRefuse { id })]
    }

    /// A damaged rectangle of the served picture, as raw RGBA bytes. Encodes and sends it
    /// under flow control (dropped when the window is full or it would not fit a frame).
    pub fn view_frame(&mut self, region: [u32; 4], raw: &[u8]) -> Vec<Effect> {
        let (peer, id, seq, acked) = match &self.served {
            Some(s) if s.streaming => (s.peer.clone(), s.id, s.seq, s.acked),
            _ => return Vec::new(),
        };
        let [_, _, w, h] = region;
        if w == 0 || h == 0 {
            return Vec::new();
        }
        let pixels = w as usize * h as usize;
        if pixels > MAX_REGION_PIXELS || raw.len() != pixels * BYTES_PER_PIXEL {
            return Vec::new();
        }
        if seq - acked >= WINDOW {
            return Vec::new(); // a slow link drops frames, it never buffers without limit
        }
        let Some(encoded) = codec::encode(w, h, raw) else {
            return Vec::new();
        };
        let frame = Frame::ViewFrame {
            id,
            seq: seq + 1,
            region,
            data: device::encode(&encoded),
        };
        if wire::pack(&frame).is_none() {
            return Vec::new();
        }
        if let Some(s) = self.served.as_mut() {
            s.seq += 1;
        }
        vec![Effect::Send(peer, frame)]
    }

    /// The source ends the session (its person pressed Stop, or the screen locked).
    pub fn view_end(&mut self) -> Vec<Effect> {
        let Some(s) = self.served.take() else {
            return Vec::new();
        };
        vec![
            Effect::ViewEnded {
                peer: s.peer.clone(),
                id: s.id,
                end: ViewEnd::Stopped,
            },
            Effect::Send(s.peer, Frame::ViewStop { id: s.id }),
        ]
    }

    /// A link to `peer` is gone: any session with it ends as lost.
    pub(crate) fn view_clear(&mut self, peer: &str) -> Vec<Effect> {
        let mut effects = Vec::new();
        if let Some(v) = self.viewing.take() {
            if v.peer == peer {
                effects.push(Effect::ViewEnded {
                    peer: v.peer,
                    id: v.id,
                    end: ViewEnd::Lost,
                });
            } else {
                self.viewing = Some(v);
            }
        }
        if let Some(s) = self.served.take() {
            if s.peer == peer {
                effects.push(Effect::ViewEnded {
                    peer: s.peer,
                    id: s.id,
                    end: ViewEnd::Lost,
                });
            } else {
                self.served = Some(s);
            }
        }
        effects
    }

    pub(crate) fn on_view_frame(&mut self, peer: &str, frame: Frame) -> Vec<Effect> {
        match frame {
            Frame::ViewStart { id } => {
                if self.served.is_some() || !self.allows(peer, Right::Control) {
                    return vec![Effect::Send(peer.to_owned(), Frame::ViewRefuse { id })];
                }
                self.served = Some(Served {
                    peer: peer.to_owned(),
                    id,
                    seq: 0,
                    acked: 0,
                    streaming: false,
                });
                vec![Effect::ViewIncoming {
                    peer: peer.to_owned(),
                    id,
                }]
            }
            Frame::ViewRefuse { id } => {
                if self
                    .viewing
                    .as_ref()
                    .is_some_and(|v| v.peer == peer && v.id == id)
                {
                    self.viewing = None;
                    vec![Effect::ViewEnded {
                        peer: peer.to_owned(),
                        id,
                        end: ViewEnd::Refused,
                    }]
                } else {
                    Vec::new()
                }
            }
            Frame::ViewAccept {
                id,
                width: _,
                height: _,
            } => {
                if self
                    .viewing
                    .as_ref()
                    .is_some_and(|v| v.peer == peer && v.id == id)
                {
                    vec![Effect::ViewStarted {
                        peer: peer.to_owned(),
                        id,
                    }]
                } else {
                    Vec::new()
                }
            }
            Frame::ViewFrame {
                id,
                seq,
                region,
                data,
            } => {
                let watching = self
                    .viewing
                    .as_ref()
                    .is_some_and(|v| v.peer == peer && v.id == id);
                if !watching {
                    return Vec::new();
                }
                let [_, _, w, h] = region;
                let Some(raw) = device::decode(&data).and_then(|b| codec::decode(&b)) else {
                    return Vec::new();
                };
                if raw.0 != w || raw.1 != h {
                    return Vec::new();
                }
                vec![
                    Effect::ViewFrame {
                        peer: peer.to_owned(),
                        id,
                        region,
                        data: raw.2,
                    },
                    // Flow control: everything up to `seq` is now rendered.
                    Effect::Send(peer.to_owned(), Frame::ViewAck { id, seq }),
                ]
            }
            Frame::ViewAck { id, seq } => {
                if let Some(s) = self.served.as_mut() {
                    if s.peer == peer && s.id == id {
                        s.acked = s.acked.max(seq);
                    }
                }
                Vec::new()
            }
            Frame::ViewInput { id, input } => {
                let serving = self.served.as_ref().is_some_and(|s| {
                    s.peer == peer && s.id == id && s.streaming && !input.trim().is_empty()
                });
                if serving {
                    vec![Effect::ViewInput {
                        peer: peer.to_owned(),
                        id,
                        input,
                    }]
                } else {
                    Vec::new()
                }
            }
            Frame::ViewStop { id } => {
                if self
                    .viewing
                    .as_ref()
                    .is_some_and(|v| v.peer == peer && v.id == id)
                {
                    self.viewing = None;
                    vec![Effect::ViewEnded {
                        peer: peer.to_owned(),
                        id,
                        end: ViewEnd::Stopped,
                    }]
                } else if self
                    .served
                    .as_ref()
                    .is_some_and(|s| s.peer == peer && s.id == id)
                {
                    self.served = None;
                    vec![Effect::ViewEnded {
                        peer: peer.to_owned(),
                        id,
                        end: ViewEnd::Stopped,
                    }]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }
}