//! Sharing the clipboard: text copied on one device can be pasted on the paired devices.
//! A confidential entry (a password) never leaves the device, and text that came from
//! another device is not sent back. Pictures and the life of an entry: [`crate::clip_share`].

use crate::node::{Effect, Node};
use crate::wire::Frame;
use std::time::Instant;
use ultar_bus::messages::mesh::{Capability, ClipboardRemote, Right};

/// Longest clipboard text that travels.
pub const MAX_CLIP: usize = 16 * 1024;

impl Node {
    /// The clipboard of this device changed.
    pub fn clipboard_set(&mut self, text: &str, confidential: bool) -> Vec<Effect> {
        let echo = self.clip.remote.as_deref() == Some(text);
        if confidential || echo || text.is_empty() || text.len() > MAX_CLIP {
            return Vec::new();
        }
        let targets = self.clip_targets();
        if targets.is_empty() {
            return Vec::new();
        }
        self.clip.seq += 1;
        self.clip.image = None;
        let seq = self.clip.seq;
        targets
            .into_iter()
            .map(|id| {
                let text = text.to_owned();
                Effect::Send(id, Frame::Clip { seq, text })
            })
            .collect()
    }

    pub(crate) fn on_clip(&mut self, peer: &str, text: String, now: Instant) -> Vec<Effect> {
        let fits = !text.is_empty() && text.len() <= MAX_CLIP;
        let wanted = self.config.capabilities.contains(&Capability::Clipboard);
        if !fits || !wanted || !self.allows(peer, Right::Clipboard) {
            return Vec::new();
        }
        self.clip.remote = Some(text.clone());
        self.clip.image = None;
        let mut effects = vec![Effect::Clipboard(ClipboardRemote {
            from: peer.to_owned(),
            text,
        })];
        effects.extend(self.clip_arrived(peer, now));
        effects
    }
}
