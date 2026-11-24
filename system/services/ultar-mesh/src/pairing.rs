//! Pairing two devices with a code (the exchange itself is in [`crate::trust`]).

use crate::device;
use crate::node::{Effect, Node};
use crate::trust::{Joiner, Shower, Shown, CODE_SECONDS};
use crate::wire::Frame;
use std::time::Instant;
use ultar_bus::messages::mesh::PairCode;

impl Node {
    /// Shows a pairing code on this device.
    pub fn pair_begin(&mut self, now: Instant) -> PairCode {
        let shown = Shown::new(now);
        let code = shown.code().to_owned();
        self.shown = Some(shown);
        PairCode {
            code,
            seconds: CODE_SECONDS,
        }
    }

    /// Pairs with `peer`, which shows `code`; `false` when `peer` is not linked.
    pub fn pair_join(&mut self, peer: &str, code: &str) -> (bool, Vec<Effect>) {
        let key = self.key.clone();
        let Some(link) = self.links.get_mut(peer) else {
            return (false, Vec::new());
        };
        let (joiner, msg) = Joiner::start(code, &key, &link.key);
        link.joiner = Some(joiner);
        let frame = Frame::PairStart {
            msg: device::encode(&msg),
        };
        (true, vec![Effect::Send(peer.to_owned(), frame)])
    }

    fn remember(&mut self, peer: &str) -> bool {
        let Some(link) = self.links.get(peer) else {
            return false;
        };
        let (name, kind) = link.device.as_ref().map_or_else(
            || ("device".to_owned(), "other"),
            |d| (d.name.clone(), device::kind_word(d.kind)),
        );
        let key = link.key.clone();
        self.trust.add(peer, &key, &name, kind)
    }

    fn refused(peer: &str) -> Vec<Effect> {
        vec![Effect::Send(peer.to_owned(), Frame::PairRefused)]
    }

    fn pair_start(&mut self, peer: &str, their_key: &[u8], msg: &str, now: Instant) -> Vec<Effect> {
        let my_key = self.key.clone();
        let code = self
            .shown
            .as_mut()
            .and_then(|s| s.attempt(now))
            .map(str::to_owned);
        let answer = code
            .zip(device::decode(msg))
            .and_then(|(code, msg)| Shower::answer(&code, &my_key, their_key, &msg));
        let Some((shower, reply, proof)) = answer else {
            return Self::refused(peer);
        };
        if let Some(link) = self.links.get_mut(peer) {
            link.shower = Some(shower);
        }
        let frame = Frame::PairReply {
            msg: device::encode(&reply),
            proof: device::encode(&proof),
        };
        vec![Effect::Send(peer.to_owned(), frame)]
    }

    fn pair_reply(&mut self, peer: &str, msg: &str, proof: &str) -> Vec<Effect> {
        let joiner = self.links.get_mut(peer).and_then(|l| l.joiner.take());
        let mine = joiner
            .zip(device::decode(msg))
            .zip(device::decode(proof))
            .and_then(|((j, m), p)| j.finish(&m, &p));
        let Some(mine) = mine else {
            return vec![Effect::Changed];
        };
        self.remember(peer);
        let proof = device::encode(&mine);
        vec![
            Effect::Send(peer.to_owned(), Frame::PairConfirm { proof }),
            Effect::Changed,
        ]
    }

    fn pair_confirm(&mut self, peer: &str, proof: &str) -> Vec<Effect> {
        let shower = self.links.get_mut(peer).and_then(|l| l.shower.take());
        let ok = shower
            .zip(device::decode(proof))
            .is_some_and(|(s, p)| s.confirmed(&p));
        if !ok {
            return Self::refused(peer);
        }
        self.shown = None;
        self.remember(peer);
        vec![Effect::Changed]
    }

    pub(crate) fn on_pairing(&mut self, peer: &str, frame: Frame, now: Instant) -> Vec<Effect> {
        let Some(their_key) = self.links.get(peer).map(|l| l.key.clone()) else {
            return Vec::new();
        };
        match frame {
            Frame::PairStart { msg } => self.pair_start(peer, &their_key, &msg, now),
            Frame::PairReply { msg, proof } => self.pair_reply(peer, &msg, &proof),
            Frame::PairConfirm { proof } => self.pair_confirm(peer, &proof),
            _ => {
                // Refused: the code this device typed was wrong or is used up.
                if let Some(link) = self.links.get_mut(peer) {
                    link.joiner = None;
                }
                vec![Effect::Changed]
            }
        }
    }
}
