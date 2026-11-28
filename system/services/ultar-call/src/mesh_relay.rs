//! Calls between the person's devices (mesh item C2): a call that rings on the phone is
//! offered to the paired devices that may carry it; the first that answers takes it, the voice
//! is relayed over the end-to-end encrypted mesh channel and either side can hang up.
//!
//! One [`MeshRelay`] serves both roles of a device:
//! * **Host** (the phone, which keeps the SIM or Ultar-number leg): offers a ringing call
//!   through `mesh.call_offer`, answers it on the line when a device accepts, ends the other
//!   offers, and carries the voice between the line's [`MeshTap`] and `mesh.call_audio`.
//! * **Guest** (the desk, which answers): after the person accepted (`mesh.call_answer`), it
//!   carries the voice between the sound card and `mesh.call_audio`, with mute
//!   (`call.mesh_mute`) and the retained `call.mesh` for the shell.
//!
//! Nothing here touches a server: the bus carries requests to the local mesh service, which
//! sends them end to end encrypted to the other device.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::mesh_tap::MeshTap;
use crate::mesh_voice::Voice;
use crate::session::Session;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};
use ultar_bus::messages::call::{Direction, Others, Phase, Way};
use ultar_bus::messages::mesh::{Done, MeshState, Right};
use ultar_bus::messages::mesh_calls::{
    CallAudio, CallEnd, CallIncoming, CallMeshMute, CallOffer, CallPhase, CallProgress,
    MeshCallState,
};
use ultar_bus::{Client, IncomingRequest, TypedSubscription};

/// How long the mesh service may take to take one request.
const MESH_WAIT: Duration = Duration::from_millis(300);
/// Most incoming offers remembered (a guest that never answers must not grow it).
const MAX_KNOWN: usize = 8;
/// Steps between tries to subscribe again (the mesh service may start later).
const RESUBSCRIBE: u32 = 40;

/// The sound devices a relay uses.
#[derive(Default)]
pub struct Taps {
    /// Host: the voice of an Ultar-number call ([`crate::mesh_tap::Bridge`]).
    pub ultar: Option<Box<dyn MeshTap>>,
    /// Host: the voice of a mobile-network call (the modem's voice stream). Without it no
    /// mobile call is offered: answering it elsewhere would be a silent call.
    pub cellular: Option<Box<dyn MeshTap>>,
    /// Guest: the sound card of this device.
    pub guest: Option<Box<dyn MeshTap>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Ultar,
    Cellular,
    Guest,
}

fn way_side(way: Way) -> Side {
    match way {
        Way::Ultar => Side::Ultar,
        Way::Cellular(_) => Side::Cellular,
    }
}

/// The call that is carried now.
struct Active {
    voice: Voice,
    /// The phone's call (host only).
    call: Option<u32>,
    side: Side,
}

/// Offers, bridges and the voice of the calls between devices.
pub struct MeshRelay {
    bus: Client,
    taps: Taps,
    state: Option<MeshState>,
    sub_state: Option<TypedSubscription<MeshState>>,
    sub_incoming: Option<TypedSubscription<CallIncoming>>,
    sub_progress: Option<TypedSubscription<CallProgress>>,
    sub_audio: Option<TypedSubscription<CallAudio>>,
    steps: u32,
    /// Host: phone call -> offers (offer id, device).
    offers: BTreeMap<u32, Vec<(u64, String)>>,
    /// Host: calls already offered (once per call).
    offered: BTreeSet<u32>,
    /// Guest: offers made to this device (offer id -> device).
    known: BTreeMap<u64, String>,
    /// Host: offers ended because another device was first (bounded).
    turned_away: BTreeSet<u64>,
    active: Option<Active>,
    muted: bool,
    published: Option<MeshCallState>,
}

impl MeshRelay {
    /// A relay over `bus` with its sound devices.
    pub fn new(bus: Client, taps: Taps) -> MeshRelay {
        let mut relay = MeshRelay {
            bus,
            taps,
            state: None,
            sub_state: None,
            sub_incoming: None,
            sub_progress: None,
            sub_audio: None,
            steps: 0,
            offers: BTreeMap::new(),
            offered: BTreeSet::new(),
            known: BTreeMap::new(),
            turned_away: BTreeSet::new(),
            active: None,
            muted: false,
            published: None,
        };
        relay.subscribe();
        relay
    }

    fn subscribe(&mut self) {
        self.sub_state = self.sub_state.take().or_else(|| self.bus.subscribe().ok());
        self.sub_incoming = self
            .sub_incoming
            .take()
            .or_else(|| self.bus.subscribe().ok());
        self.sub_progress = self
            .sub_progress
            .take()
            .or_else(|| self.bus.subscribe().ok());
        self.sub_audio = self.sub_audio.take().or_else(|| self.bus.subscribe().ok());
    }

    /// A call is carried now: the service steps often so that the voice keeps its 20 ms beat.
    pub fn busy(&self) -> bool {
        self.active.is_some()
    }

    /// The call this device leads for another device, as the shell sees it.
    pub fn mesh_state(&self) -> MeshCallState {
        match self.active.as_ref().filter(|a| a.side == Side::Guest) {
            Some(a) => MeshCallState {
                id: Some(a.voice.id),
                device: a.voice.device.clone(),
                muted: self.muted,
            },
            None => MeshCallState::default(),
        }
    }

    fn can_carry(&self, side: Side) -> bool {
        match side {
            Side::Ultar => self.taps.ultar.is_some(),
            Side::Cellular => self.taps.cellular.is_some(),
            Side::Guest => self.taps.guest.is_some(),
        }
    }

    /// The devices a call may be offered to: paired, reachable, and granted "Anrufe und Ton"
    /// here (their voice is only taken from a device that has the right).
    fn eligible(&self) -> Vec<String> {
        self.state
            .iter()
            .filter(|s| s.enabled)
            .flat_map(|s| s.peers.iter())
            .filter(|p| p.paired && p.reachable && p.rights.contains(&Right::Call))
            .map(|p| p.id.clone())
            .collect()
    }

    /// `call.mesh_mute`; gives the request back when it is another method.
    pub fn request(&mut self, request: IncomingRequest) -> Option<IncomingRequest> {
        if request.method() != "mesh_mute" {
            return Some(request);
        }
        let leading = self.active.as_ref().is_some_and(|a| a.side == Side::Guest);
        let mut mute = None;
        let _ = request.respond::<CallMeshMute>(|m| {
            if leading {
                mute = Some(m.on);
            }
            Ok(Done { ok: leading })
        });
        if let Some(on) = mute {
            self.muted = on;
            self.publish_state();
        }
        None
    }

    /// One step: takes what the mesh reported, offers, bridges and moves the voice.
    pub fn step(&mut self, session: &mut Session, now: Instant) {
        self.steps = self.steps.wrapping_add(1);
        if self.steps % RESUBSCRIBE == 1 {
            self.subscribe();
        }
        while let Some(Ok(Some(s))) = self.sub_state.as_ref().map(|s| s.try_recv()) {
            self.state = Some(s);
        }
        while let Some(Ok(Some(i))) = self.sub_incoming.as_ref().map(|s| s.try_recv()) {
            if self.known.len() < MAX_KNOWN {
                self.known.insert(i.id, i.from);
            }
        }
        while let Some(Ok(Some(p))) = self.sub_progress.as_ref().map(|s| s.try_recv()) {
            match p.phase {
                CallPhase::Offered => {}
                CallPhase::Accepted => self.accepted(session, &p, now),
                CallPhase::Declined | CallPhase::Ended | CallPhase::Failed => {
                    self.gone(session, &p);
                }
            }
        }
        self.offer_ringing(session);
        self.end_stale(session);
        self.voice(session, now);
        self.publish_state();
    }

    fn offer_ringing(&mut self, session: &Session) {
        let calls = session.state().calls;
        self.offered.retain(|id| calls.iter().any(|c| c.id == *id));
        if self.active.is_some() {
            return;
        }
        let eligible = self.eligible();
        for info in calls
            .iter()
            .filter(|c| c.direction == Direction::Incoming && c.phase == Phase::Ringing)
        {
            // A call nobody can be asked about yet is looked at again while it rings.
            if eligible.is_empty()
                || !self.can_carry(way_side(info.way))
                || !self.offered.insert(info.id)
            {
                continue;
            }
            let caller = info
                .name
                .clone()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| info.number.clone());
            let mut made = Vec::new();
            for device in eligible.iter().cloned() {
                let offer = CallOffer {
                    device: device.clone(),
                    caller: caller.clone(),
                    video: info.media.video_call,
                };
                if let Ok(r) = self.bus.call(&offer, MESH_WAIT) {
                    if r.id != 0 {
                        made.push((r.id, device));
                    }
                }
            }
            if !made.is_empty() {
                self.offers.insert(info.id, made);
            }
        }
    }

    /// Offers of calls that are not ringing any more end; so does a bridge whose call is gone.
    fn end_stale(&mut self, session: &Session) {
        let live: BTreeMap<u32, Phase> = session
            .state()
            .calls
            .into_iter()
            .map(|c| (c.id, c.phase))
            .collect();
        let stale: Vec<u32> = self
            .offers
            .keys()
            .filter(|id| live.get(id) != Some(&Phase::Ringing))
            .copied()
            .collect();
        for id in stale {
            for (offer, _) in self.offers.remove(&id).unwrap_or_default() {
                self.hang_up(offer);
            }
        }
        let gone = self
            .active
            .as_ref()
            .and_then(|a| a.call.map(|c| (a.voice.id, c)))
            .filter(|(_, call)| !live.contains_key(call));
        if let Some((offer, _)) = gone {
            self.stop();
            self.hang_up(offer);
        }
    }

    fn hang_up(&self, id: u64) {
        let _ = self.bus.call(&CallEnd { id }, MESH_WAIT);
    }

    fn accepted(&mut self, session: &mut Session, p: &CallProgress, now: Instant) {
        let host = self
            .offers
            .iter()
            .find_map(|(call, list)| list.iter().any(|(id, _)| *id == p.id).then_some(*call));
        if let Some(call) = host {
            self.host_accepted(session, call, p, now);
        } else if self.turned_away.contains(&p.id) {
            // An answer that crossed our hang-up: the offer stays ended.
            self.hang_up(p.id);
        } else if self.known.contains_key(&p.id) && self.taps.guest.is_some() {
            if self.active.is_none() {
                self.begin(Active {
                    voice: Voice::new(p.id, p.device.clone(), now),
                    call: None,
                    side: Side::Guest,
                });
            } else {
                // One call at a time: the offer stays unanswered here.
                self.hang_up(p.id);
            }
        }
    }

    fn host_accepted(&mut self, session: &mut Session, call: u32, p: &CallProgress, now: Instant) {
        let side = session
            .state()
            .calls
            .iter()
            .find(|c| c.id == call)
            .map(|c| way_side(c.way));
        let answered = match side {
            Some(side) if self.active.is_none() && session.answer(call, Others::Hold).is_ok() => {
                Some(side)
            }
            _ => None,
        };
        let Some(side) = answered else {
            // Somebody else was first, or the call is gone: this device is not needed.
            self.drop_offer(call, p.id);
            self.hang_up(p.id);
            return;
        };
        // The first device that answers takes the call: the other offers end.
        for (offer, _) in self.offers.remove(&call).unwrap_or_default() {
            if offer != p.id {
                self.hang_up(offer);
                if self.turned_away.len() >= MAX_KNOWN {
                    self.turned_away.pop_first();
                }
                self.turned_away.insert(offer);
            }
        }
        self.begin(Active {
            voice: Voice::new(p.id, p.device.clone(), now),
            call: Some(call),
            side,
        });
    }

    fn tap(&mut self, side: Side) -> Option<&mut Box<dyn MeshTap>> {
        match side {
            Side::Ultar => self.taps.ultar.as_mut(),
            Side::Cellular => self.taps.cellular.as_mut(),
            Side::Guest => self.taps.guest.as_mut(),
        }
    }

    fn begin(&mut self, active: Active) {
        let side = active.side;
        self.muted = false;
        self.active = Some(active);
        if let Some(tap) = self.tap(side) {
            tap.begin();
        }
    }

    fn stop(&mut self) {
        if let Some(a) = self.active.take() {
            if let Some(tap) = self.tap(a.side) {
                tap.end();
            }
        }
        self.muted = false;
    }

    fn drop_offer(&mut self, call: u32, offer: u64) {
        if let Some(list) = self.offers.get_mut(&call) {
            list.retain(|(id, _)| *id != offer);
            if list.is_empty() {
                self.offers.remove(&call);
            }
        }
    }

    fn gone(&mut self, session: &mut Session, p: &CallProgress) {
        self.known.remove(&p.id);
        for call in self.offers.keys().copied().collect::<Vec<_>>() {
            self.drop_offer(call, p.id);
        }
        let Some(call) = self
            .active
            .as_ref()
            .filter(|a| a.voice.id == p.id)
            .map(|a| a.call)
        else {
            return;
        };
        self.stop();
        // The other device hung up: the call ends. A lost link only moves the call back to
        // the phone's own sound.
        if let (Some(call), CallPhase::Ended) = (call, p.phase) {
            let _ = session.hangup(call);
        }
    }

    fn voice(&mut self, session: &Session, now: Instant) {
        let mut heard = Vec::new();
        while let Some(Ok(Some(a))) = self.sub_audio.as_ref().map(|s| s.try_recv()) {
            heard.push(a);
        }
        let phone_muted = session.state().audio.muted;
        let Some(active) = self.active.as_mut() else {
            return;
        };
        for audio in &heard {
            active.voice.hear(audio);
        }
        let side = active.side;
        // The guest's mute stops its microphone; the phone's own mute button stops the voice
        // that goes into the call it keeps.
        let (send, play) = match side {
            Side::Guest => (!self.muted, true),
            _ => (true, !phone_muted),
        };
        let tap = match side {
            Side::Ultar => self.taps.ultar.as_mut(),
            Side::Cellular => self.taps.cellular.as_mut(),
            Side::Guest => self.taps.guest.as_mut(),
        };
        if let Some(tap) = tap {
            active.voice.pump(&self.bus, tap.as_mut(), send, play, now);
        }
    }

    fn publish_state(&mut self) {
        let now = self.mesh_state();
        if self.published.as_ref() != Some(&now) {
            let _ = self.bus.publish(&now);
            self.published = Some(now);
        }
    }
}
