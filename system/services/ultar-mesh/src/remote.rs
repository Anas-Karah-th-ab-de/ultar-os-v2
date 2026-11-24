//! Acting on another device: the helper here asks the helper of a paired device to do
//! something there.
//!
//! The other device takes such a request only from a device it granted
//! [`Right::Control`] (off after pairing), acts with its own helper's rights, and its
//! person confirms anything dangerous there. A device that is carrying out such a request
//! passes none on: requests neither chain nor circle.

use crate::node::{Effect, Node};
use crate::wire::Frame;
use ultar_bus::messages::agent::{Step, MAX_REQUEST};
use ultar_bus::messages::mesh::Right;
use ultar_bus::messages::mesh_remote::MAX_STEPS;

impl Node {
    /// Asks the helper of the paired, reachable `device` to do `text`; the id of the
    /// request, or `None` when it cannot be sent.
    pub fn act(&mut self, device: &str, text: &str) -> Option<(u64, Vec<Effect>)> {
        let fits = !text.trim().is_empty() && text.len() <= MAX_REQUEST;
        if !fits || self.serving > 0 || !self.paired(device) {
            return None;
        }
        self.thinking += 1;
        let id = self.thinking;
        let frame = Frame::Act {
            id,
            text: text.to_owned(),
        };
        Some((id, vec![Effect::Send(device.to_owned(), frame)]))
    }

    /// The local helper is done with (or never got) `peer`'s request `id`.
    pub fn acted(&mut self, peer: &str, id: u64, ok: bool, mut steps: Vec<Step>) -> Vec<Effect> {
        self.serving = self.serving.saturating_sub(1);
        steps.truncate(MAX_STEPS);
        vec![Effect::Send(
            peer.to_owned(),
            Frame::Acted { id, ok, steps },
        )]
    }

    fn not_taken(peer: &str, id: u64) -> Vec<Effect> {
        let frame = Frame::Acted {
            id,
            ok: false,
            steps: Vec::new(),
        };
        vec![Effect::Send(peer.to_owned(), frame)]
    }

    pub(crate) fn on_remote_frame(&mut self, peer: &str, frame: Frame) -> Vec<Effect> {
        match frame {
            Frame::Act { id, text } => {
                let fits = !text.trim().is_empty() && text.len() <= MAX_REQUEST;
                if !fits || self.serving > 0 || !self.allows(peer, Right::Control) {
                    return Self::not_taken(peer, id);
                }
                self.serving += 1;
                vec![Effect::Act {
                    peer: peer.to_owned(),
                    id,
                    text,
                }]
            }
            Frame::Acted { id, ok, mut steps } => {
                steps.truncate(MAX_STEPS);
                vec![Effect::Acted {
                    id,
                    peer: peer.to_owned(),
                    ok,
                    steps,
                }]
            }
            _ => Vec::new(),
        }
    }
}
