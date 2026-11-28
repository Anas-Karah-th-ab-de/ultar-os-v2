//! Remote view (mesh item C1): a paired device streams its screen and takes the pointer
//! and keys of another device.
//!
//! The *viewer* asks (`ViewStart`); the *source* streams. The source refuses a device it
//! has not granted [`Right::Control`] (off after pairing), then asks its person. While a
//! session runs, the source sends only damaged rectangles, losslessly QOI-compressed
//! ([`crate::codec`]), and takes back the viewer's input. Flow control: the source keeps
//! at most [`WINDOW`] frames in flight, the viewer acknowledges in order, so a slow link
//! *drops* frames instead of buffering without limit. Either side may stop; a lost link
//! ends the session. The bus side (screend, the viewer app, the journal) is `viewbus`.

use crate::codec::{self, BYTES_PER_PIXEL, MAX_PIXELS};
use crate::device;
use crate::node::{Effect, Node};
use crate::wire::{self, Frame};
use ultar_bus::messages::mesh::Right;

/// Frames the source keeps in flight before it drops new ones (slow-link flow control).
pub const WINDOW: u32 = 4;
/// Longest input event the source takes from a viewer.
pub const MAX_INPUT: usize = 256;
/// Largest rectangle a frame may carry (in pixels); the source tiles below this.
pub const MAX_REGION_PIXELS: usize = MAX_PIXELS;
/// Largest picture side the source may announce (8K, as the screenshot service).
pub const MAX_SIDE: u32 = 8192;

/// How a session ended, for the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewEnd {
    /// The source refused (no right, busy, or its person declined).
    Refused,
    /// One side stopped it.
    Stopped,
    /// The link went away.
    Lost,
    /// The source's screen locked; the source ended the session.
    Locked,
}

/// A session this device watches (it is the viewer).
#[derive(Debug)]
pub(crate) struct Viewing {
    pub(crate) peer: String,
    pub(crate) id: u64,
    /// The picture size the source accepted with (`None` until it accepts).
    pub(crate) size: Option<(u32, u32)>,
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

/// The sessions of a node: the one it watches, the one it serves, the last id it gave.
#[derive(Debug, Default)]
pub(crate) struct Views {
    pub(crate) viewing: Option<Viewing>,
    pub(crate) served: Option<Served>,
    pub(crate) id: u64,
}

/// Who this device views or serves right now (for the shell's indicator and the journal).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ViewNow {
    /// The device whose screen this device watches.
    pub watching: Option<String>,
    /// The device that watches this device's screen (its person said yes).
    pub watched_by: Option<String>,
}

impl Node {
    /// Who this device watches and who watches it.
    pub fn view_now(&self) -> ViewNow {
        ViewNow {
            watching: self.views.viewing.as_ref().map(|v| v.peer.clone()),
            watched_by: self
                .views
                .served
                .as_ref()
                .filter(|s| s.streaming)
                .map(|s| s.peer.clone()),
        }
    }

    /// Asks the paired, reachable `device` to show and take this device's input. The id
    /// of the session, or `None` when it cannot be sent (already viewing, not paired).
    pub fn view_start(&mut self, device: &str) -> Option<(u64, Vec<Effect>)> {
        if self.views.viewing.is_some() || !self.paired(device) {
            return None;
        }
        self.views.id += 1;
        let id = self.views.id;
        self.views.viewing = Some(Viewing {
            peer: device.to_owned(),
            id,
            size: None,
        });
        Some((
            id,
            vec![Effect::Send(device.to_owned(), Frame::ViewStart { id })],
        ))
    }

    /// Sends the viewer's pointer/keys to the source; `None` when no accepted session
    /// runs or the event is empty or too long.
    pub fn view_input(&mut self, input: &str) -> Option<Vec<Effect>> {
        let input = input.trim().to_owned();
        let v = self.views.viewing.as_ref().filter(|v| v.size.is_some())?;
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
        let v = self.views.viewing.take()?;
        Some(vec![
            Effect::Send(
                v.peer.clone(),
                Frame::ViewStop {
                    id: v.id,
                    locked: false,
                },
            ),
            Effect::ViewEnded {
                peer: v.peer,
                id: v.id,
                end: ViewEnd::Stopped,
            },
        ])
    }

    fn view_pending(&self, peer: &str, id: u64) -> bool {
        self.views
            .served
            .as_ref()
            .is_some_and(|s| s.peer == peer && s.id == id && !s.streaming)
    }

    /// The person on the source accepted the session `id` of `peer`; the stream begins at
    /// `width` × `height`. A zero or absurd size refuses.
    pub fn view_accept(&mut self, peer: &str, id: u64, width: u32, height: u32) -> Vec<Effect> {
        if !self.view_pending(peer, id) {
            return Vec::new();
        }
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            self.views.served = None;
            return vec![Effect::Send(peer.to_owned(), Frame::ViewRefuse { id })];
        }
        if let Some(s) = self.views.served.as_mut() {
            s.streaming = true;
        }
        let started = Effect::ViewStarted {
            peer: peer.to_owned(),
            id,
            width,
            height,
        };
        let accept = Frame::ViewAccept { id, width, height };
        vec![started, Effect::Send(peer.to_owned(), accept)]
    }

    /// The person on the source declined the session `id` of `peer`.
    pub fn view_refuse(&mut self, peer: &str, id: u64) -> Vec<Effect> {
        if !self.view_pending(peer, id) {
            return Vec::new();
        }
        self.views.served = None;
        vec![
            Effect::Send(peer.to_owned(), Frame::ViewRefuse { id }),
            Effect::ViewEnded {
                peer: peer.to_owned(),
                id,
                end: ViewEnd::Refused,
            },
        ]
    }

    /// `true` when a frame sent now is under the window (the source may capture and send).
    pub fn view_room(&self) -> bool {
        self.views
            .served
            .as_ref()
            .is_some_and(|s| s.streaming && s.seq.saturating_sub(s.acked) < WINDOW)
    }

    /// A damaged rectangle of the served picture, as raw pixel bytes. Encodes and sends it
    /// under flow control (dropped when the window is full or it would not fit a frame).
    pub fn view_frame(&mut self, region: [u32; 4], raw: &[u8]) -> Vec<Effect> {
        if !self.view_room() {
            return Vec::new(); // a slow link drops frames, it never buffers without limit
        }
        let Some((peer, id, seq)) = self
            .views
            .served
            .as_ref()
            .map(|s| (s.peer.clone(), s.id, s.seq))
        else {
            return Vec::new();
        };
        let [_, _, w, h] = region;
        let pixels = w as usize * h as usize;
        if pixels == 0 || pixels > MAX_REGION_PIXELS || raw.len() != pixels * BYTES_PER_PIXEL {
            return Vec::new();
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
        if let Some(s) = self.views.served.as_mut() {
            s.seq += 1;
        }
        vec![Effect::Send(peer, frame)]
    }

    /// The source ends the session (its person pressed Stop).
    pub fn view_end(&mut self) -> Vec<Effect> {
        self.view_finish(ViewEnd::Stopped)
    }

    /// The source's screen locked: the session ends at once.
    pub fn view_locked(&mut self) -> Vec<Effect> {
        self.view_finish(ViewEnd::Locked)
    }

    fn view_finish(&mut self, end: ViewEnd) -> Vec<Effect> {
        let Some(s) = self.views.served.take() else {
            return Vec::new();
        };
        vec![
            Effect::ViewEnded {
                peer: s.peer.clone(),
                id: s.id,
                end,
            },
            Effect::Send(
                s.peer,
                Frame::ViewStop {
                    id: s.id,
                    locked: end == ViewEnd::Locked,
                },
            ),
        ]
    }

    /// A link to `peer` is gone: any session with it ends as lost.
    pub(crate) fn view_clear(&mut self, peer: &str) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.views.viewing.as_ref().is_some_and(|v| v.peer == peer) {
            if let Some(v) = self.views.viewing.take() {
                effects.push(Effect::ViewEnded {
                    peer: v.peer,
                    id: v.id,
                    end: ViewEnd::Lost,
                });
            }
        }
        if self.views.served.as_ref().is_some_and(|s| s.peer == peer) {
            if let Some(s) = self.views.served.take() {
                effects.push(Effect::ViewEnded {
                    peer: s.peer,
                    id: s.id,
                    end: ViewEnd::Lost,
                });
            }
        }
        effects
    }

    /// The right to control this device was taken from `peer` (or the pairing ended): the
    /// session it watches ends now. With `also_viewing` our own watching of `peer` ends too.
    pub(crate) fn view_revoke(&mut self, peer: &str, also_viewing: bool) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.views.served.as_ref().is_some_and(|s| s.peer == peer) {
            if let Some(s) = self.views.served.take() {
                effects.push(Effect::ViewEnded {
                    peer: s.peer.clone(),
                    id: s.id,
                    end: ViewEnd::Stopped,
                });
                effects.push(Effect::Send(
                    s.peer,
                    Frame::ViewStop {
                        id: s.id,
                        locked: false,
                    },
                ));
            }
        }
        if also_viewing && self.views.viewing.as_ref().is_some_and(|v| v.peer == peer) {
            if let Some(v) = self.views.viewing.take() {
                effects.push(Effect::ViewEnded {
                    peer: v.peer.clone(),
                    id: v.id,
                    end: ViewEnd::Stopped,
                });
                effects.push(Effect::Send(
                    v.peer,
                    Frame::ViewStop {
                        id: v.id,
                        locked: false,
                    },
                ));
            }
        }
        effects
    }

    fn watching(&self, peer: &str, id: u64) -> bool {
        self.views
            .viewing
            .as_ref()
            .is_some_and(|v| v.peer == peer && v.id == id)
    }

    fn on_view_start(&mut self, peer: &str, id: u64) -> Vec<Effect> {
        if self.views.served.is_some() || !self.allows(peer, Right::Control) {
            return vec![Effect::Send(peer.to_owned(), Frame::ViewRefuse { id })];
        }
        self.views.served = Some(Served {
            peer: peer.to_owned(),
            id,
            seq: 0,
            acked: 0,
            streaming: false,
        });
        let peer = peer.to_owned();
        vec![Effect::ViewIncoming { peer, id }]
    }

    fn on_view_picture(
        &mut self,
        peer: &str,
        id: u64,
        seq: u32,
        region: [u32; 4],
        data: &str,
    ) -> Vec<Effect> {
        let size = self
            .views
            .viewing
            .as_ref()
            .filter(|v| v.peer == peer && v.id == id)
            .and_then(|v| v.size);
        let Some((full_w, full_h)) = size else {
            return Vec::new();
        };
        let [x, y, w, h] = region;
        let inside = x.checked_add(w).is_some_and(|e| e <= full_w)
            && y.checked_add(h).is_some_and(|e| e <= full_h);
        let decoded = device::decode(data).and_then(|b| codec::decode(&b));
        match decoded {
            Some((dw, dh, raw)) if inside && dw == w && dh == h => vec![
                Effect::ViewFrame {
                    peer: peer.to_owned(),
                    id,
                    region,
                    data: raw,
                },
                // Flow control: everything up to `seq` is now rendered.
                Effect::Send(peer.to_owned(), Frame::ViewAck { id, seq }),
            ],
            _ => Vec::new(),
        }
    }

    /// The viewer's input: only into a streaming session of this peer, only while it may control.
    fn on_view_input(&mut self, peer: &str, id: u64, input: String) -> Vec<Effect> {
        let fits = !input.trim().is_empty() && input.len() <= MAX_INPUT;
        let serving = (self.views.served.as_ref())
            .is_some_and(|s| s.peer == peer && s.id == id && s.streaming);
        if serving && fits && self.allows(peer, Right::Control) {
            let peer = peer.to_owned();
            vec![Effect::ViewInput { peer, id, input }]
        } else {
            Vec::new()
        }
    }

    /// The source accepted: its screen has this size (a size that is not sane is ignored).
    fn on_view_accept(&mut self, peer: &str, id: u64, width: u32, height: u32) -> Vec<Effect> {
        let sane = (1..=MAX_SIDE).contains(&width) && (1..=MAX_SIDE).contains(&height);
        if !sane {
            return Vec::new();
        }
        if let Some(v) = self.views.viewing.as_mut() {
            v.size = Some((width, height));
        }
        let peer = peer.to_owned();
        vec![Effect::ViewStarted {
            peer,
            id,
            width,
            height,
        }]
    }

    /// The viewer took pictures up to `seq`; it cannot acknowledge what was never sent.
    fn on_view_ack(&mut self, peer: &str, id: u64, seq: u32) {
        if let Some(s) = self.views.served.as_mut() {
            if s.peer == peer && s.id == id {
                s.acked = s.acked.max(seq.min(s.seq));
            }
        }
    }

    /// The other side ends the session (`locked`: the source's screen locked).
    fn on_view_stop(&mut self, peer: &str, id: u64, locked: bool) -> Vec<Effect> {
        let mine = self.watching(peer, id);
        let served = (self.views.served.as_ref()).is_some_and(|s| s.peer == peer && s.id == id);
        if mine {
            self.views.viewing = None;
        } else if served {
            self.views.served = None;
        } else {
            return Vec::new();
        }
        let peer = peer.to_owned();
        let end = if mine && locked {
            ViewEnd::Locked
        } else {
            ViewEnd::Stopped
        };
        vec![Effect::ViewEnded { peer, id, end }]
    }

    pub(crate) fn on_view_frame(&mut self, peer: &str, frame: Frame) -> Vec<Effect> {
        match frame {
            Frame::ViewStart { id } => self.on_view_start(peer, id),
            Frame::ViewRefuse { id } if self.watching(peer, id) => {
                self.views.viewing = None;
                let peer = peer.to_owned();
                let end = ViewEnd::Refused;
                vec![Effect::ViewEnded { peer, id, end }]
            }
            Frame::ViewAccept { id, width, height } if self.watching(peer, id) => {
                self.on_view_accept(peer, id, width, height)
            }
            Frame::ViewFrame {
                id,
                seq,
                region,
                data,
            } => self.on_view_picture(peer, id, seq, region, &data),
            Frame::ViewAck { id, seq } => {
                self.on_view_ack(peer, id, seq);
                Vec::new()
            }
            Frame::ViewInput { id, input } => self.on_view_input(peer, id, input),
            Frame::ViewStop { id, locked } => self.on_view_stop(peer, id, locked),
            _ => Vec::new(),
        }
    }
}
