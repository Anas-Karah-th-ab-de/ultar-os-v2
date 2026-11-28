//! The rest of the shared clipboard inside the node: pictures, the two-minute life of an
//! entry that came from another device, and the notice "copied on <device>" (at most one
//! in ten seconds). The text path is in [`crate::clipboard`].
//!
//! The right [`Right::Clipboard`] ("Share the clipboard") is one switch per paired device
//! and works both ways: this device sends its copies only to devices that have it, and
//! takes entries only from them. It is off for a newly paired device.

use crate::clip_image::{self, Assembly, Piece, Taken};
use crate::node::{Effect, Node};
use crate::wire::Frame;
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh::{Capability, Right};

/// How long an entry that came from another device stays in the clipboard.
pub const LIFE: Duration = Duration::from_secs(120);
/// Least time between two notices.
pub const NOTICE_GAP: Duration = Duration::from_secs(10);

/// What the node remembers of the shared clipboard besides the text.
#[derive(Default)]
pub(crate) struct ClipState {
    /// SHA-256 of the last picture that came from another device (no echo).
    pub(crate) image: Option<String>,
    /// When the entry from another device arrived.
    pub(crate) arrived: Option<Instant>,
    /// When the last notice was given.
    pub(crate) noticed: Option<Instant>,
    /// The picture that is arriving.
    pub(crate) assembly: Option<Assembly>,
}

impl Node {
    /// The paired, connected devices this device shares its clipboard with.
    pub(crate) fn clip_targets(&self) -> Vec<String> {
        if !self.config.capabilities.contains(&Capability::Clipboard) {
            return Vec::new();
        }
        self.links
            .iter()
            .filter(|(id, l)| {
                self.trust.knows(id, &l.key)
                    && self.allows(id, Right::Clipboard)
                    && l.device
                        .as_ref()
                        .is_some_and(|d| d.can(Capability::Clipboard))
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// A picture was copied on this device (`image/png`; a password is never a picture the
    /// bridge reads).
    pub fn clipboard_image_set(&mut self, png: &[u8]) -> Vec<Effect> {
        let hash = clip_image::digest(png);
        if self.clip.image.as_deref() == Some(hash.as_str()) {
            return Vec::new();
        }
        let targets = self.clip_targets();
        if targets.is_empty() {
            return Vec::new();
        }
        self.clip_seq += 1;
        let pieces = clip_image::pieces(self.clip_seq, png);
        if pieces.is_empty() {
            return Vec::new();
        }
        self.clip_remote = None;
        self.clip.image = None;
        let mut effects = Vec::new();
        for id in targets {
            for piece in &pieces {
                effects.push(Effect::Send(id.clone(), Frame::ClipImage(piece.clone())));
            }
        }
        effects
    }

    pub(crate) fn on_clip_image(&mut self, peer: &str, piece: Piece, now: Instant) -> Vec<Effect> {
        if !self.clip_targets().iter().any(|id| id == peer) {
            self.clip.assembly = None;
            return Vec::new();
        }
        match clip_image::take(&mut self.clip.assembly, peer, piece, now) {
            Taken::Done(png) => {
                self.clip.image = Some(clip_image::digest(&png));
                self.clip_remote = None;
                let mut effects = vec![Effect::ClipboardImage {
                    from: peer.to_owned(),
                    png,
                }];
                effects.extend(self.clip_arrived(peer, now));
                effects
            }
            Taken::More | Taken::Refused => Vec::new(),
        }
    }

    /// An entry from `peer` was put into the clipboard: it lives for [`LIFE`], and the person
    /// is told (at most once in [`NOTICE_GAP`]).
    pub(crate) fn clip_arrived(&mut self, peer: &str, now: Instant) -> Vec<Effect> {
        self.clip.arrived = Some(now);
        let recent = self
            .clip
            .noticed
            .is_some_and(|t| now.saturating_duration_since(t) < NOTICE_GAP);
        if recent {
            return Vec::new();
        }
        self.clip.noticed = Some(now);
        let from_name = self
            .links
            .get(peer)
            .and_then(|l| l.device.as_ref())
            .map(|d| d.name.clone())
            .or_else(|| self.trust.get(peer).map(|p| p.name.clone()))
            .unwrap_or_default();
        vec![Effect::ClipNotice { from_name }]
    }

    /// Regular work: an entry from another device that is too old goes away, a picture
    /// that never completes is dropped.
    pub(crate) fn clip_tick(&mut self, now: Instant) -> Vec<Effect> {
        if self.clip.assembly.as_ref().is_some_and(|a| a.stale(now)) {
            self.clip.assembly = None;
        }
        let old = self
            .clip
            .arrived
            .is_some_and(|t| now.saturating_duration_since(t) >= LIFE);
        if !old {
            return Vec::new();
        }
        self.clip.arrived = None;
        self.clip.image = None;
        self.clip_remote = None;
        vec![Effect::ClipboardClear]
    }
}
