//! Calls, sound and notes over the mesh: a call on the phone can be offered to a paired
//! device, answered, talked and hung up from either side; notes can be mirrored, all gated
//! by [`Right::Call`] (off after pairing), everything encrypted; two-way calls need it both ways.

use crate::audio_link;
use crate::device;
use crate::node::{Effect, Node};
use crate::wire::Frame;
use std::collections::BTreeMap;
use ultar_bus::messages::mesh::{Capability, Right};
use ultar_bus::messages::mesh_calls::{
    CallAudio, CallIncoming, CallPhase, CallProgress, NoteRemote,
};
use ultar_bus::messages::notify::Urgency;

/// Longest mirrored title.
pub const MAX_TITLE: usize = 128;
/// Longest mirrored body.
pub const MAX_TEXT: usize = 1024;
/// Longest application id of a mirrored notification.
pub const MAX_APP: usize = 64;

fn cut(mut text: String, max: usize) -> String {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

fn urgency_word(urgency: Urgency) -> u8 {
    match urgency {
        Urgency::Low => 0,
        Urgency::Normal => 1,
        Urgency::High => 2,
    }
}

fn urgency_from(word: u8) -> Urgency {
    match word {
        0 => Urgency::Low,
        2 => Urgency::High,
        _ => Urgency::Normal,
    }
}

struct Incoming {
    peer: String,
    active: bool,
}

/// The calls a node is involved in and its notification counter.
#[derive(Default)]
pub(crate) struct Calls {
    next: u64,
    out: BTreeMap<u64, String>,
    incoming: BTreeMap<u64, Incoming>,
    note_seq: u64,
}

impl Calls {
    /// Removes every call with `peer` and reports them failed (the link is gone).
    pub(crate) fn peer_down(&mut self, peer: &str) -> Vec<Effect> {
        let mut effects = Vec::new();
        let out: Vec<u64> = self
            .out
            .iter()
            .filter(|(_, p)| p.as_str() == peer)
            .map(|(id, _)| *id)
            .collect();
        for id in out {
            self.out.remove(&id);
            effects.push(Effect::CallProgress(CallProgress {
                id,
                device: peer.to_owned(),
                phase: CallPhase::Failed,
            }));
        }
        let incoming: Vec<u64> = self
            .incoming
            .iter()
            .filter(|(_, c)| c.peer == peer)
            .map(|(id, _)| *id)
            .collect();
        for id in incoming {
            self.incoming.remove(&id);
            effects.push(Effect::CallProgress(CallProgress {
                id,
                device: peer.to_owned(),
                phase: CallPhase::Failed,
            }));
        }
        effects
    }
}

impl Node {
    /// Offers the call that rings here to one paired device; `0` when no offer went out.
    pub fn call_offer(&mut self, device: &str, caller: &str, video: bool) -> (u64, Vec<Effect>) {
        let can = self.links.get(device).is_some_and(|l| {
            self.paired(device)
                && l.device
                    .as_ref()
                    .is_some_and(|d| d.can(Capability::AudioIn) && d.can(Capability::AudioOut))
        });
        if !can {
            return (0, Vec::new());
        }
        self.calls.next += 1;
        let id = self.calls.next;
        self.calls.out.insert(id, device.to_owned());
        let effects = vec![
            Effect::Send(
                device.to_owned(),
                Frame::Call {
                    id,
                    caller: device::clean_name(caller),
                    video,
                },
            ),
            Effect::CallProgress(CallProgress {
                id,
                device: device.to_owned(),
                phase: CallPhase::Offered,
            }),
        ];
        (id, effects)
    }

    /// This device answers or declines the call `id` (offered by a paired device).
    pub fn call_answer(&mut self, id: u64, accept: bool) -> Vec<Effect> {
        let Some(offered) = self.calls.incoming.get(&id) else {
            return Vec::new();
        };
        if offered.active {
            return Vec::new();
        }
        let peer = offered.peer.clone();
        if !accept {
            self.calls.incoming.remove(&id);
            return vec![
                Effect::Send(peer.clone(), Frame::CallAnswer { id, accept: false }),
                Effect::CallProgress(CallProgress {
                    id,
                    device: peer,
                    phase: CallPhase::Declined,
                }),
            ];
        }
        if let Some(offered) = self.calls.incoming.get_mut(&id) {
            offered.active = true;
        }
        vec![
            Effect::Send(peer.clone(), Frame::CallAnswer { id, accept: true }),
            Effect::CallProgress(CallProgress {
                id,
                device: peer,
                phase: CallPhase::Accepted,
            }),
        ]
    }

    /// Hangs up from this side; nothing when no such call is open here.
    pub fn call_end(&mut self, id: u64) -> Vec<Effect> {
        if let Some(peer) = self.calls.out.remove(&id) {
            return vec![
                Effect::Send(peer.clone(), Frame::CallEnd { id }),
                Effect::CallProgress(CallProgress {
                    id,
                    device: peer,
                    phase: CallPhase::Ended,
                }),
            ];
        }
        if let Some(offered) = self.calls.incoming.remove(&id) {
            return vec![
                Effect::Send(offered.peer.clone(), Frame::CallEnd { id }),
                Effect::CallProgress(CallProgress {
                    id,
                    device: offered.peer,
                    phase: CallPhase::Ended,
                }),
            ];
        }
        Vec::new()
    }

    /// Sends one voice frame toward the paired device `device`; refused for bytes of the
    /// wrong size, an unknown call, or a device that is not paired.
    pub fn call_audio(&mut self, device: &str, id: u64, seq: u64, pcm: &[u8]) -> Vec<Effect> {
        if !audio_link::is_frame(pcm) {
            return Vec::new();
        }
        let known = self.calls.out.get(&id).is_some_and(|p| p == device)
            || self
                .calls
                .incoming
                .get(&id)
                .is_some_and(|c| c.peer == device && c.active);
        let paired = self.paired(device);
        if !known || !paired {
            return Vec::new();
        }
        vec![Effect::Send(
            device.to_owned(),
            Frame::CallAudio {
                id,
                seq,
                pcm: device::encode(pcm),
            },
        )]
    }

    /// The local notifyd reports a notification to mirror; a confidential one (a password,
    /// a one-time code) never leaves this device.
    pub fn note_mirror(
        &mut self,
        title: &str,
        text: &str,
        app: &str,
        urgency: Urgency,
        confidential: bool,
        hide_on_locked: bool,
    ) -> Vec<Effect> {
        if confidential || (title.is_empty() && text.is_empty()) {
            return Vec::new();
        }
        self.calls.note_seq += 1;
        let seq = self.calls.note_seq;
        self.links
            .iter()
            .filter(|(id, l)| {
                self.paired(id) && l.device.as_ref().is_some_and(|d| d.can(Capability::Screen))
            })
            .map(|(id, _)| {
                Effect::Send(
                    id.clone(),
                    Frame::Note {
                        seq,
                        title: cut(title.to_owned(), MAX_TITLE),
                        text: cut(text.to_owned(), MAX_TEXT),
                        app: cut(app.to_owned(), MAX_APP),
                        urgency: urgency_word(urgency),
                        hide_on_locked,
                    },
                )
            })
            .collect()
    }

    pub(crate) fn on_call_frame(&mut self, peer: &str, frame: Frame) -> Vec<Effect> {
        match frame {
            Frame::Call { id, caller, video } => self.on_call(peer, id, caller, video),
            Frame::CallAnswer { id, accept } => self.on_call_answer(peer, id, accept),
            Frame::CallEnd { id } => self.on_call_end(peer, id),
            Frame::CallAudio { id, seq, pcm } => self.on_call_audio(peer, id, seq, pcm),
            Frame::Note {
                title,
                text,
                app,
                urgency,
                hide_on_locked,
                ..
            } => self.on_note(peer, title, text, app, urgency, hide_on_locked),
            _ => Vec::new(),
        }
    }

    fn on_call(&mut self, peer: &str, id: u64, caller: String, video: bool) -> Vec<Effect> {
        let can = self.config.capabilities.contains(&Capability::AudioIn)
            && self.config.capabilities.contains(&Capability::AudioOut);
        // No right: nothing rings here (the offering device learns nothing).
        if !can || !self.allows(peer, Right::Call) || self.calls.incoming.contains_key(&id) {
            return Vec::new();
        }
        let from_name = self
            .links
            .get(peer)
            .and_then(|l| l.device.as_ref())
            .map_or_else(|| "device".to_owned(), |d| d.name.clone());
        self.calls.incoming.insert(
            id,
            Incoming {
                peer: peer.to_owned(),
                active: false,
            },
        );
        vec![Effect::CallIncoming(CallIncoming {
            id,
            from: peer.to_owned(),
            from_name,
            caller,
            video,
        })]
    }

    fn on_call_answer(&mut self, peer: &str, id: u64, accept: bool) -> Vec<Effect> {
        let Some(out) = self.calls.out.get(&id) else {
            return Vec::new();
        };
        if out != peer {
            return Vec::new();
        }
        let phase = if accept {
            CallPhase::Accepted
        } else {
            self.calls.out.remove(&id);
            CallPhase::Declined
        };
        vec![Effect::CallProgress(CallProgress {
            id,
            device: peer.to_owned(),
            phase,
        })]
    }

    fn on_call_end(&mut self, peer: &str, id: u64) -> Vec<Effect> {
        if let Some(out) = self.calls.out.get(&id) {
            if out != peer {
                return Vec::new();
            }
            self.calls.out.remove(&id);
            return vec![Effect::CallProgress(CallProgress {
                id,
                device: peer.to_owned(),
                phase: CallPhase::Ended,
            })];
        }
        if let Some(offered) = self.calls.incoming.get(&id) {
            if offered.peer != peer {
                return Vec::new();
            }
            self.calls.incoming.remove(&id);
            return vec![Effect::CallProgress(CallProgress {
                id,
                device: peer.to_owned(),
                phase: CallPhase::Ended,
            })];
        }
        Vec::new()
    }

    fn on_call_audio(&mut self, peer: &str, id: u64, seq: u64, pcm: String) -> Vec<Effect> {
        let Some(bytes) = device::decode(&pcm) else {
            return Vec::new();
        };
        if !audio_link::is_frame(&bytes) {
            return Vec::new();
        }
        let known = self.calls.out.get(&id).is_some_and(|p| p == peer)
            || self
                .calls
                .incoming
                .get(&id)
                .is_some_and(|c| c.peer == peer && c.active);
        if !known || !self.allows(peer, Right::Call) {
            return Vec::new();
        }
        vec![Effect::CallAudio(CallAudio {
            id,
            seq,
            pcm,
            device: peer.to_owned(),
        })]
    }

    fn on_note(
        &mut self,
        peer: &str,
        title: String,
        text: String,
        app: String,
        urgency: u8,
        hide_on_locked: bool,
    ) -> Vec<Effect> {
        let fits = !(title.is_empty() && text.is_empty())
            && title.len() <= MAX_TITLE
            && text.len() <= MAX_TEXT
            && app.len() <= MAX_APP;
        let wanted = self.config.capabilities.contains(&Capability::Screen);
        if !fits || !wanted || !self.allows(peer, Right::Call) {
            return Vec::new();
        }
        let from_name = self
            .links
            .get(peer)
            .and_then(|l| l.device.as_ref())
            .map_or_else(|| "device".to_owned(), |d| d.name.clone());
        vec![Effect::Note(NoteRemote {
            from: peer.to_owned(),
            from_name,
            title,
            text,
            app,
            urgency: urgency_from(urgency),
            hide_on_locked,
        })]
    }
}
