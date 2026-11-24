//! Sharing the clipboard: text copied on one device can be pasted on the paired devices.
//! A confidential entry (a password) never leaves the device, and text that came from
//! another device is not sent back.

use crate::node::{Effect, Node};
use crate::wire::Frame;
use ultar_bus::messages::mesh::{Capability, ClipboardRemote, Right};

/// Longest clipboard text that travels.
pub const MAX_CLIP: usize = 16 * 1024;

impl Node {
    /// The clipboard of this device changed.
    pub fn clipboard_set(&mut self, text: &str, confidential: bool) -> Vec<Effect> {
        let echo = self.clip_remote.as_deref() == Some(text);
        if confidential || echo || text.is_empty() || text.len() > MAX_CLIP {
            return Vec::new();
        }
        if !self.config.capabilities.contains(&Capability::Clipboard) {
            return Vec::new();
        }
        self.clip_seq += 1;
        let seq = self.clip_seq;
        self.links
            .iter()
            .filter(|(id, l)| {
                self.trust.knows(id, &l.key)
                    && l.device
                        .as_ref()
                        .is_some_and(|d| d.can(Capability::Clipboard))
            })
            .map(|(id, _)| {
                let text = text.to_owned();
                Effect::Send(id.clone(), Frame::Clip { seq, text })
            })
            .collect()
    }

    pub(crate) fn on_clip(&mut self, peer: &str, text: String) -> Vec<Effect> {
        let fits = !text.is_empty() && text.len() <= MAX_CLIP;
        let wanted = self.config.capabilities.contains(&Capability::Clipboard);
        if !fits || !wanted || !self.allows(peer, Right::Clipboard) {
            return Vec::new();
        }
        self.clip_remote = Some(text.clone());
        vec![Effect::Clipboard(ClipboardRemote {
            from: peer.to_owned(),
            text,
        })]
    }
}
