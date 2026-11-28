//! Handing work over: offers this device sent and offers it received.
//!
//! The rule the person set: nothing arrives unasked. The receiving device always asks
//! ([`Effect::Incoming`], answered with [`Node::answer`]); the sending device asks which
//! device is meant as soon as more than one can take the activity
//! ([`SendOutcome::Choose`]).

use crate::device;
use crate::handoff::{self, ASK_SECONDS};
use crate::node::{Effect, Node};
use crate::received::Received;
use crate::transfer;
use crate::wire::Frame;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};
use ultar_bus::messages::launcher::launcher_id;
use ultar_bus::messages::mesh::{
    Activity, Capability, HandoffProgress, OfferState, SendOutcome, Stage, Target,
};

/// Offers one device may have open here at a time.
pub const MAX_PENDING: usize = 4;
/// Apps whose report of what is open is kept.
pub const MAX_REPORTED: usize = 16;
/// A transfer without a piece for this long has failed.
pub const TRANSFER_SILENCE: Duration = Duration::from_secs(30);

struct Sent {
    peer: String,
    state: OfferState,
    file: Option<transfer::Outgoing>,
    deadline: Instant,
}

/// The open offers of a node.
#[derive(Default)]
pub struct Offers {
    pub(crate) next: u64,
    sent: BTreeMap<u64, Sent>,
    pub(crate) received: BTreeMap<u64, Received>,
}

/// The activity under the id the launcher knows the app by (surfaces say
/// `org.ultar.files`, the launcher and an app's bus name say `files`).
fn known_to_launcher(mut activity: Activity) -> Activity {
    activity.app = launcher_id(&activity.app).to_owned();
    activity
}

pub(crate) fn progress(offer: u64, peer: &str, outgoing: bool, state: OfferState) -> Effect {
    Effect::Progress(HandoffProgress {
        offer,
        device: peer.to_owned(),
        outgoing,
        state,
    })
}

pub(crate) fn send(peer: &str, frame: Frame) -> Effect {
    Effect::Send(peer.to_owned(), frame)
}

impl Node {
    /// An app reports the activity in front (`None`: nothing to hand over).
    pub fn set_activity(&mut self, activity: Option<Activity>) {
        self.activity = activity.filter(|a| !a.app.is_empty());
    }

    /// What would be handed over now.
    pub fn in_front(&self) -> Option<&Activity> {
        self.activity.as_ref()
    }

    /// The app with the bus name `name` says what is open in it (`None`: nothing). It is
    /// kept under the app's id until the app says otherwise, and is in front as long as
    /// nobody names another app. `false`: the app tried to speak for another one.
    pub fn report(&mut self, name: &str, activity: Option<Activity>) -> bool {
        let activity = activity.map(known_to_launcher);
        if activity.as_ref().is_some_and(|a| a.app != name) {
            return false;
        }
        self.reported.remove(name);
        if let Some(a) = activity
            .as_ref()
            .filter(|_| self.reported.len() < MAX_REPORTED)
        {
            self.reported.insert(name.to_owned(), a.clone());
        }
        let in_front = self.activity.as_ref().is_none_or(|a| a.app == name);
        if in_front {
            self.activity = activity;
        }
        true
    }

    /// The shell names the app in front. What that app itself reported counts (it knows
    /// the place or the file); without a report, the app is offered as it is.
    pub fn set_front(&mut self, front: Option<Activity>) {
        let front = front.map(known_to_launcher).filter(|a| !a.app.is_empty());
        let reported = front.as_ref().and_then(|a| self.reported.get(&a.app));
        self.activity = reported.cloned().or(front);
    }

    /// The devices the activity in front can go to.
    pub fn targets(&self) -> Vec<Target> {
        let Some(activity) = &self.activity else {
            return Vec::new();
        };
        self.targets_of(activity)
    }

    fn targets_of(&self, activity: &Activity) -> Vec<Target> {
        self.links
            .iter()
            .filter(|(id, _)| self.paired(id))
            .filter_map(|(id, link)| {
                let device = link.device.as_ref()?;
                let stage = handoff::stage_for(activity, device, self.config.remote_view)?;
                Some(Target {
                    device: id.clone(),
                    name: device.name.clone(),
                    kind: device.kind,
                    stage,
                })
            })
            .collect()
    }

    /// The sender takes a waiting offer back (`mesh.offer_cancel`): the question on the
    /// receiving device disappears (it is told `Frame::Cancel`).
    pub fn cancel_outgoing(&mut self, offer: u64) -> Vec<Effect> {
        let Some(sent) = self.offers.sent.remove(&offer) else {
            return Vec::new();
        };
        vec![send(&sent.peer, Frame::Cancel { id: offer })]
    }

    /// Hands the activity in front over. Without `target`: to the only possible device,
    /// or [`SendOutcome::Choose`] when there are several.
    pub fn send(&mut self, target: Option<&str>, now: Instant) -> (SendOutcome, Vec<Effect>) {
        let Some(activity) = self.activity.clone() else {
            return (SendOutcome::NoActivity, Vec::new());
        };
        let mut targets = self.targets_of(&activity);
        let chosen = match target {
            Some(id) => targets.into_iter().find(|t| t.device == id),
            None if targets.len() > 1 => return (SendOutcome::Choose(targets), Vec::new()),
            None => targets.pop(),
        };
        let Some(chosen) = chosen else {
            return (SendOutcome::NoTarget, Vec::new());
        };
        self.offer(chosen, &activity, now)
    }

    /// Sends the file at `path` to `peer` (asked there like any offer).
    pub fn send_file(
        &mut self,
        peer: &str,
        path: &str,
        now: Instant,
    ) -> (SendOutcome, Vec<Effect>) {
        let linked = self
            .links
            .get(peer)
            .and_then(|l| l.device.as_ref())
            .cloned();
        let takes = linked.filter(|d| self.paired(peer) && d.can(Capability::Files));
        let Some(device) = takes else {
            return (SendOutcome::NoTarget, Vec::new());
        };
        let name = Path::new(path).file_name().and_then(|n| n.to_str());
        let activity = Activity {
            app: String::new(),
            title: name.unwrap_or("file").to_owned(),
            uri: None,
            file: Some(path.to_owned()),
        };
        let target = Target {
            device: peer.to_owned(),
            name: device.name,
            kind: device.kind,
            stage: Stage::File,
        };
        self.offer(target, &activity, now)
    }

    fn offer(
        &mut self,
        target: Target,
        activity: &Activity,
        now: Instant,
    ) -> (SendOutcome, Vec<Effect>) {
        let carries = target.stage != Stage::RemoteView;
        let file = match activity.file.as_deref().filter(|_| carries) {
            Some(path) => {
                let real = transfer::shareable(Path::new(path), &self.config.share);
                match real.map(|p| transfer::Outgoing::open(&p)) {
                    Some(Ok(file)) => Some(file),
                    _ => return (SendOutcome::NoActivity, Vec::new()),
                }
            }
            None => None,
        };
        self.offers.next += 1;
        let id = self.offers.next;
        let frame = Frame::Offer {
            id,
            title: device::clean_name(&activity.title),
            app: activity.app.clone(),
            stage: handoff::stage_word(target.stage).to_owned(),
            uri: activity
                .uri
                .clone()
                .filter(|_| target.stage == Stage::State),
            file: file.as_ref().map(|f| f.meta.clone()),
        };
        let peer = target.device.clone();
        self.offers.sent.insert(
            id,
            Sent {
                peer: peer.clone(),
                state: OfferState::Asked,
                file,
                deadline: now + Duration::from_secs(ASK_SECONDS),
            },
        );
        let effects = vec![
            send(&peer, frame),
            progress(id, &peer, true, OfferState::Asked),
        ];
        (SendOutcome::Offered { offer: id, target }, effects)
    }

    fn piece(&mut self, id: u64, index: u32, now: Instant) -> Vec<Effect> {
        let Some(sent) = self.offers.sent.get_mut(&id) else {
            return Vec::new();
        };
        let peer = sent.peer.clone();
        let Some(file) = sent.file.as_mut() else {
            return Vec::new();
        };
        match file.piece(index) {
            Ok(Some(bytes)) => {
                sent.deadline = now + TRANSFER_SILENCE;
                let data = device::encode(&bytes);
                vec![send(&peer, Frame::Chunk { id, index, data })]
            }
            Ok(None) => Vec::new(),
            Err(_) => {
                self.offers.sent.remove(&id);
                vec![
                    send(&peer, Frame::Cancel { id }),
                    progress(id, &peer, true, OfferState::Failed),
                ]
            }
        }
    }

    fn on_answer(&mut self, peer: &str, id: u64, accept: bool, now: Instant) -> Vec<Effect> {
        let Some(sent) = self.offers.sent.get_mut(&id) else {
            return Vec::new();
        };
        if sent.peer != peer || sent.state != OfferState::Asked {
            return Vec::new();
        }
        if !accept {
            self.offers.sent.remove(&id);
            return vec![progress(id, peer, true, OfferState::Declined)];
        }
        sent.state = OfferState::Accepted;
        sent.deadline = now + TRANSFER_SILENCE;
        let mut effects = vec![progress(id, peer, true, OfferState::Accepted)];
        effects.extend(self.piece(id, 0, now));
        effects
    }

    fn on_outcome(&mut self, peer: &str, id: u64, state: &str) -> Vec<Effect> {
        let known = self.offers.sent.get(&id).is_some_and(|s| s.peer == peer);
        if !known {
            return Vec::new();
        }
        self.offers.sent.remove(&id);
        let state = if state == "opened" {
            OfferState::Opened
        } else {
            OfferState::Failed
        };
        vec![progress(id, peer, true, state)]
    }

    pub(crate) fn on_offer_frame(&mut self, peer: &str, frame: Frame, now: Instant) -> Vec<Effect> {
        match frame {
            Frame::Offer { .. } => self.on_offer(peer, frame, now),
            Frame::Answer { id, accept } => self.on_answer(peer, id, accept, now),
            Frame::Chunk { id, index, data } => self.on_chunk(peer, id, index, &data, now),
            Frame::ChunkAck { id, index } => {
                let ours = self.offers.sent.get(&id).is_some_and(|s| s.peer == peer);
                if ours {
                    self.piece(id, index.saturating_add(1), now)
                } else {
                    Vec::new()
                }
            }
            Frame::Outcome { id, state } => self.on_outcome(peer, id, &state),
            Frame::Cancel { id } => self.on_cancel(peer, id),
            _ => Vec::new(),
        }
    }

    pub(crate) fn drop_offers(&mut self, peer: &str) -> Vec<Effect> {
        let mut effects = Vec::new();
        let sent: Vec<u64> = self
            .offers
            .sent
            .iter()
            .filter(|(_, s)| s.peer == peer)
            .map(|(id, _)| *id)
            .collect();
        for id in sent {
            self.offers.sent.remove(&id);
            effects.push(progress(id, peer, true, OfferState::Failed));
        }
        let got: Vec<u64> = self
            .offers
            .received
            .iter()
            .filter(|(_, r)| r.peer == peer)
            .map(|(id, _)| *id)
            .collect();
        for id in got {
            if let Some(mut r) = self.offers.received.remove(&id) {
                if let Some(file) = r.file.as_mut() {
                    file.abandon();
                }
            }
            effects.push(progress(id, peer, false, OfferState::Failed));
        }
        effects
    }

    pub(crate) fn expire_offers(&mut self, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        let late: Vec<u64> = self
            .offers
            .sent
            .iter()
            .filter(|(_, s)| now >= s.deadline)
            .map(|(id, _)| *id)
            .collect();
        for id in late {
            if let Some(sent) = self.offers.sent.remove(&id) {
                let state = if sent.state == OfferState::Asked {
                    OfferState::Expired
                } else {
                    OfferState::Failed
                };
                effects.push(send(&sent.peer, Frame::Cancel { id }));
                effects.push(progress(id, &sent.peer, true, state));
            }
        }
        let late: Vec<u64> = self
            .offers
            .received
            .iter()
            .filter(|(_, r)| now >= r.deadline)
            .map(|(id, _)| *id)
            .collect();
        for offer in late {
            let asked = self
                .offers
                .received
                .get(&offer)
                .is_some_and(|r| r.state == OfferState::Asked);
            if !asked {
                effects.extend(self.finish(offer, false));
            } else if let Some(got) = self.offers.received.remove(&offer) {
                effects.push(send(
                    &got.peer,
                    Frame::Answer {
                        id: got.remote,
                        accept: false,
                    },
                ));
                effects.push(progress(offer, &got.peer, false, OfferState::Expired));
            }
        }
        effects
    }
}
