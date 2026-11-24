//! Offers this device received: asked, answered, received and opened.

use crate::device;
use crate::handoff::{self, ASK_SECONDS};
use crate::node::{Effect, Node};
use crate::offers::{progress, send, MAX_PENDING, TRANSFER_SILENCE};
use crate::transfer::{self, Step};
use crate::wire::{FileMeta, Frame};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh::{
    Capability, DeviceKind, HandoffIncoming, OfferState, Right, Stage,
};

/// An offer another device made.
pub(crate) struct Received {
    pub(crate) peer: String,
    pub(crate) remote: u64,
    pub(crate) app: String,
    pub(crate) uri: Option<String>,
    pub(crate) stage: Stage,
    pub(crate) meta: Option<FileMeta>,
    pub(crate) file: Option<transfer::Incoming>,
    pub(crate) state: OfferState,
    pub(crate) deadline: Instant,
}

impl Node {
    fn refuse(peer: &str, id: u64) -> Vec<Effect> {
        vec![send(peer, Frame::Answer { id, accept: false })]
    }

    pub(crate) fn on_offer(&mut self, peer: &str, frame: Frame, now: Instant) -> Vec<Effect> {
        let Frame::Offer {
            id,
            title,
            app,
            stage,
            uri,
            file,
        } = frame
        else {
            return Vec::new();
        };
        let right = if app.is_empty() {
            Right::Files
        } else {
            Right::Handoff
        };
        let open = self.offers.received.values().filter(|r| r.peer == peer);
        let stage = handoff::stage_from(&stage);
        let stores = file.is_none() || self.config.capabilities.contains(&Capability::Files);
        let uri_fine = uri.as_deref().is_none_or(handoff::uri_ok);
        let (Some(stage), true) = (
            stage,
            self.allows(peer, right) && stores && uri_fine && open.count() < MAX_PENDING,
        ) else {
            return Self::refuse(peer, id);
        };
        self.offers.next += 1;
        let local = self.offers.next;
        let device = self.links.get(peer).and_then(|l| l.device.as_ref());
        let from_name = device.map_or_else(String::new, |d| d.name.clone());
        let from_kind = device.map_or(DeviceKind::Other, |d| d.kind);
        let incoming = HandoffIncoming {
            offer: local,
            from: peer.to_owned(),
            from_name,
            title: device::clean_name(&title),
            app: app.clone(),
            stage,
            from_kind,
            size: file.as_ref().map(|f| f.size),
        };
        self.offers.received.insert(
            local,
            Received {
                peer: peer.to_owned(),
                remote: id,
                app,
                uri,
                stage,
                meta: file,
                file: None,
                state: OfferState::Asked,
                deadline: now + Duration::from_secs(ASK_SECONDS),
            },
        );
        vec![Effect::Incoming(incoming)]
    }

    /// The person answered the incoming offer `offer`.
    pub fn answer(&mut self, offer: u64, accept: bool, now: Instant) -> (bool, Vec<Effect>) {
        let inbox = self.config.inbox.clone();
        let Some(got) = self.offers.received.get_mut(&offer) else {
            return (false, Vec::new());
        };
        if got.state != OfferState::Asked {
            return (false, Vec::new());
        }
        let (peer, id) = (got.peer.clone(), got.remote);
        if !accept {
            self.offers.received.remove(&offer);
            let effects = vec![
                send(&peer, Frame::Answer { id, accept: false }),
                progress(offer, &peer, false, OfferState::Declined),
            ];
            return (true, effects);
        }
        got.state = OfferState::Accepted;
        got.deadline = now + TRANSFER_SILENCE;
        let mut effects = vec![
            send(&peer, Frame::Answer { id, accept: true }),
            progress(offer, &peer, false, OfferState::Accepted),
        ];
        match got.meta.clone() {
            Some(meta) => match transfer::Incoming::create(&inbox, &meta) {
                Ok(file) => got.file = Some(file),
                Err(_) => effects.extend(self.finish(offer, false)),
            },
            None => effects.push(Self::open(offer, got, None)),
        }
        (true, effects)
    }

    fn open(offer: u64, got: &Received, arrived: Option<&PathBuf>) -> Effect {
        let app = if got.stage == Stage::File {
            String::new()
        } else {
            got.app.clone()
        };
        Effect::Open {
            offer,
            app,
            uri: handoff::arrived_uri(got.uri.as_deref(), arrived.map(PathBuf::as_path)),
        }
    }

    pub(crate) fn finish(&mut self, offer: u64, ok: bool) -> Vec<Effect> {
        let Some(mut got) = self.offers.received.remove(&offer) else {
            return Vec::new();
        };
        if let Some(file) = got.file.as_mut() {
            file.abandon();
        }
        let (word, state) = if ok {
            ("opened", OfferState::Opened)
        } else {
            ("failed", OfferState::Failed)
        };
        let frame = Frame::Outcome {
            id: got.remote,
            state: word.to_owned(),
        };
        vec![
            send(&got.peer, frame),
            progress(offer, &got.peer, false, state),
        ]
    }

    /// What [`Effect::Open`] led to: the activity is open, or it could not be opened.
    pub fn opened(&mut self, offer: u64, ok: bool) -> Vec<Effect> {
        self.finish(offer, ok)
    }

    pub(crate) fn on_chunk(
        &mut self,
        peer: &str,
        id: u64,
        index: u32,
        data: &str,
        now: Instant,
    ) -> Vec<Effect> {
        let found = self
            .offers
            .received
            .iter_mut()
            .find(|(_, r)| r.peer == peer && r.remote == id && r.state == OfferState::Accepted);
        let Some((&offer, got)) = found else {
            return Vec::new();
        };
        let step = device::decode(data)
            .zip(got.file.as_mut())
            .map(|(bytes, file)| file.write(index, &bytes));
        match step {
            Some(Ok(Step::More)) => {
                got.deadline = now + TRANSFER_SILENCE;
                vec![send(peer, Frame::ChunkAck { id, index })]
            }
            Some(Ok(Step::Done(path))) => {
                got.file = None;
                got.deadline = now + TRANSFER_SILENCE;
                vec![
                    send(peer, Frame::ChunkAck { id, index }),
                    Self::open(offer, got, Some(&path)),
                ]
            }
            _ => self.finish(offer, false),
        }
    }

    pub(crate) fn on_cancel(&mut self, peer: &str, id: u64) -> Vec<Effect> {
        let found = self
            .offers
            .received
            .iter()
            .find(|(_, r)| r.peer == peer && r.remote == id)
            .map(|(offer, _)| *offer);
        let Some(offer) = found else {
            return Vec::new();
        };
        if let Some(mut got) = self.offers.received.remove(&offer) {
            if let Some(file) = got.file.as_mut() {
                file.abandon();
            }
        }
        vec![progress(offer, peer, false, OfferState::Expired)]
    }
}
