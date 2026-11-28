//! Calls of the person's other devices on this desktop (mesh item C2): the banner of an
//! incoming call (answer, decline) and, while a call is led here, the banner with its
//! controls (mute, hang up). Both are the shell's own notices ([`Notes::post_call`]): they
//! stay until they are answered, the call is over, or - for the call in progress - for as
//! long as it lasts.

use super::notes::{Notes, LOCAL_IDS};
use super::services::Call;
use super::Desktop;
use ultar_bus::messages::mesh::MeshState;
use ultar_bus::messages::mesh_calls::{CallIncoming, CallPhase, CallProgress, MeshCallState};
use ultar_bus::messages::notify::Action;
use ultar_bus::{Text, TextArg};

/// The shell's own notices of calls count down from here (below the notices of [`Notes`]).
const BASE: u64 = LOCAL_IDS - 2_048;
/// Id of the notice of the call in progress.
pub const ACTIVE: u64 = BASE;
/// Most calls that ring at once (one notice each).
pub const MAX_RINGING: usize = 4;
/// Most offers whose caller is remembered.
const MAX_KNOWN: usize = 8;
/// Sender of the notices.
const APP: &str = "org.ultar.shell";

/// An offer that rings here.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ring {
    offer: u64,
    note: u64,
}

/// The call in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Active {
    offer: u64,
    device: String,
    muted: bool,
}

/// What the shell knows of calls offered here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Calls {
    ringing: Vec<Ring>,
    active: Option<Active>,
    /// Who called, per offer (the call in progress is named after it).
    callers: Vec<(u64, String)>,
}

/// Whether `id` is the id of a notice of this module.
pub fn is_call_note(id: u64) -> bool {
    id <= BASE && id >= BASE - MAX_RINGING as u64
}

fn action(id: &str, key: &str) -> Action {
    Action {
        id: id.to_owned(),
        label: Text::key(key),
    }
}

fn verbatim(s: &str) -> TextArg {
    TextArg::Verbatim(s.to_owned())
}

impl Calls {
    /// The notice of the call in progress and whether the microphone is muted.
    pub fn in_progress(&self) -> Option<(u64, bool)> {
        self.active.as_ref().map(|a| (a.offer, a.muted))
    }

    fn caller_of(&self, offer: u64) -> String {
        self.callers
            .iter()
            .find(|(o, _)| *o == offer)
            .map(|(_, c)| c.clone())
            .unwrap_or_default()
    }

    fn free_note(&self) -> Option<u64> {
        (1..=MAX_RINGING as u64)
            .map(|slot| BASE - slot)
            .find(|id| !self.ringing.iter().any(|r| r.note == *id))
    }

    /// What a button of a notice of this module does: the call to make. The banner of an
    /// answered or declined offer goes; the one of the call in progress stays.
    pub fn pressed(&mut self, note: u64, button: &str, notes: &mut Notes) -> Option<Call> {
        if note == ACTIVE {
            let active = self.active.as_ref()?;
            return match button {
                "mute" => Some(Call::CallMute(true)),
                "unmute" => Some(Call::CallMute(false)),
                "hangup" => Some(Call::CallEnd(active.offer)),
                _ => None,
            };
        }
        let at = self.ringing.iter().position(|r| r.note == note)?;
        let offer = self.ringing[at].offer;
        let accept = match button {
            "answer" => true,
            "decline" => false,
            _ => return None,
        };
        self.ringing.remove(at);
        notes.dismiss(note);
        Some(Call::CallAnswer(offer, accept))
    }
}

/// A paired device offers a call: a banner that stays until the person answers.
pub fn incoming(d: &mut Desktop, call: CallIncoming, now: u64) {
    let calls = &mut d.calls;
    if calls.ringing.iter().any(|r| r.offer == call.id) {
        return;
    }
    let Some(note) = calls.free_note() else {
        return;
    };
    calls.callers.retain(|(o, _)| *o != call.id);
    calls.callers.push((call.id, call.caller.clone()));
    if calls.callers.len() > MAX_KNOWN {
        calls.callers.remove(0);
    }
    d.notes.post_call(
        note,
        APP,
        Text::key("desk.call.incoming").with(verbatim(&call.caller)),
        Text::key("desk.call.incoming.body").with(verbatim(&call.from_name)),
        vec![
            action("answer", "desk.call.answer"),
            action("decline", "desk.call.decline"),
        ],
        now,
    );
    calls.ringing.push(Ring {
        offer: call.id,
        note,
    });
}

/// An offer ended or was taken elsewhere: its banner goes.
pub fn progress(d: &mut Desktop, p: &CallProgress) {
    if p.phase == CallPhase::Offered {
        return;
    }
    let calls = &mut d.calls;
    if let Some(at) = calls.ringing.iter().position(|r| r.offer == p.id) {
        let ring = calls.ringing.remove(at);
        d.notes.dismiss(ring.note);
    }
    if matches!(
        p.phase,
        CallPhase::Declined | CallPhase::Ended | CallPhase::Failed
    ) && calls.active.as_ref().is_some_and(|a| a.offer == p.id)
    {
        calls.active = None;
        d.notes.dismiss(ACTIVE);
    }
}

/// The name of the device `id` (its id when the mesh does not show it).
fn device_name(svc: &Option<MeshState>, id: &str) -> String {
    svc.as_ref()
        .and_then(|s| s.peers.iter().find(|p| p.id == id))
        .map_or_else(|| id.to_owned(), |p| p.name.clone())
}

/// The call this device leads (`call.mesh`): the banner with its controls, or none.
pub fn led(d: &mut Desktop, state: &MeshCallState, now: u64) {
    let Some(offer) = state.id else {
        if d.calls.active.take().is_some() {
            d.notes.dismiss(ACTIVE);
        }
        return;
    };
    let calls = &mut d.calls;
    let new = Active {
        offer,
        device: state.device.clone(),
        muted: state.muted,
    };
    if calls.active.as_ref() == Some(&new) {
        return;
    }
    // Answered here: the offer's own banner goes.
    if let Some(at) = calls.ringing.iter().position(|r| r.offer == offer) {
        let ring = calls.ringing.remove(at);
        d.notes.dismiss(ring.note);
    }
    calls.active = Some(new);
    post_active(d, now);
}

fn post_active(d: &mut Desktop, now: u64) {
    let Some(a) = d.calls.active.clone() else {
        return;
    };
    let name = device_name(&d.svc.mesh.state, &a.device);
    let body_key = if a.muted {
        "desk.call.active.muted"
    } else {
        "desk.call.active.body"
    };
    let (mute, label) = if a.muted {
        ("unmute", "desk.call.unmute")
    } else {
        ("mute", "desk.call.mute")
    };
    d.notes.post_call(
        ACTIVE,
        APP,
        Text::key("desk.call.active").with(verbatim(&d.calls.caller_of(a.offer))),
        Text::key(body_key).with(verbatim(&name)),
        vec![action(mute, label), action("hangup", "desk.call.hangup")],
        now,
    );
}

/// While a call is led here its banner stays: one that was closed comes back.
pub fn keep(d: &mut Desktop, now: u64) {
    if d.calls.active.is_some() && !d.notes.has(ACTIVE) {
        post_active(d, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::notify::Urgency;

    fn offer(id: u64) -> CallIncoming {
        CallIncoming {
            id,
            from: "phone".into(),
            from_name: "Anas phone".into(),
            caller: "Samer".into(),
            video: false,
        }
    }

    fn title_of(d: &Desktop, id: u64) -> Option<Text> {
        d.notes
            .list()
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.title.clone())
    }

    #[test]
    fn an_offer_rings_until_it_is_answered_declined_or_taken_elsewhere() {
        let mut d = Desktop::new(ultar_i18n::Lang::En);
        incoming(&mut d, offer(5), 10);
        incoming(&mut d, offer(5), 11);
        assert_eq!(d.calls.ringing.len(), 1, "one banner per offer");
        let note = d.calls.ringing[0].note;
        let shown = d.notes.banners(10);
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].urgency, Urgency::High, "stays until it is closed");
        let title = title_of(&d, note).unwrap();
        assert_eq!(title.key, "desk.call.incoming");
        assert_eq!(title.args, vec![verbatim("Samer")]);
        let acts: Vec<_> = shown[0].actions.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(acts, ["answer", "decline"]);
        assert!(is_call_note(note) && is_call_note(ACTIVE));

        // Declined: the call is made, the banner goes.
        let call = d.calls.pressed(note, "decline", &mut d.notes);
        assert_eq!(call, Some(Call::CallAnswer(5, false)));
        assert!(d.notes.list().is_empty());
        // Another device answered first.
        incoming(&mut d, offer(6), 20);
        let gone = CallProgress {
            id: 6,
            device: "phone".into(),
            phase: CallPhase::Ended,
        };
        progress(&mut d, &gone);
        assert!(d.notes.list().is_empty() && d.calls.ringing.is_empty());
        // An unknown button or note does nothing.
        incoming(&mut d, offer(7), 30);
        let note = d.calls.ringing[0].note;
        assert_eq!(d.calls.pressed(note, "dance", &mut d.notes), None);
        assert_eq!(d.calls.pressed(ACTIVE, "hangup", &mut d.notes), None);
    }

    #[test]
    fn only_a_few_calls_ring_at_once() {
        let mut d = Desktop::new(ultar_i18n::Lang::En);
        for id in 0..10 {
            incoming(&mut d, offer(id), 0);
        }
        assert_eq!(d.calls.ringing.len(), MAX_RINGING);
        let mut notes: Vec<u64> = d.calls.ringing.iter().map(|r| r.note).collect();
        notes.sort_unstable();
        notes.dedup();
        assert_eq!(notes.len(), MAX_RINGING, "every banner has its own id");
    }

    #[test]
    fn the_call_in_progress_has_controls_that_follow_the_mute_and_stay() {
        let mut d = Desktop::new(ultar_i18n::Lang::En);
        incoming(&mut d, offer(5), 0);
        let state = MeshCallState {
            id: Some(5),
            device: "phone".into(),
            muted: false,
        };
        led(&mut d, &state, 1);
        assert!(
            d.calls.ringing.is_empty(),
            "answered: the ring banner is gone"
        );
        let title = title_of(&d, ACTIVE).unwrap();
        assert_eq!(
            (title.key.as_str(), title.args),
            ("desk.call.active", vec![verbatim("Samer")])
        );
        let acts = |d: &Desktop| -> Vec<String> {
            d.notes
                .list()
                .iter()
                .find(|n| n.id == ACTIVE)
                .map(|n| n.actions.iter().map(|a| a.id.clone()).collect())
                .unwrap_or_default()
        };
        assert_eq!(acts(&d), ["mute", "hangup"]);
        assert_eq!(
            d.calls.pressed(ACTIVE, "mute", &mut d.notes),
            Some(Call::CallMute(true))
        );
        // The call service reports the mute: the button turns into "unmute".
        led(
            &mut d,
            &MeshCallState {
                muted: true,
                ..state.clone()
            },
            2,
        );
        assert_eq!(acts(&d), ["unmute", "hangup"]);
        assert_eq!(
            d.calls.pressed(ACTIVE, "unmute", &mut d.notes),
            Some(Call::CallMute(false))
        );
        assert_eq!(
            d.calls.pressed(ACTIVE, "hangup", &mut d.notes),
            Some(Call::CallEnd(5))
        );
        // Closed by mistake: it comes back while the call lasts.
        d.notes.dismiss(ACTIVE);
        keep(&mut d, 3);
        assert!(d.notes.has(ACTIVE));
        // Over: the banner goes for good.
        led(&mut d, &MeshCallState::default(), 4);
        keep(&mut d, 5);
        assert!(!d.notes.has(ACTIVE) && d.calls.in_progress().is_none());
    }

    #[test]
    fn the_end_of_the_call_in_progress_removes_its_banner() {
        let mut d = Desktop::new(ultar_i18n::Lang::En);
        let state = MeshCallState {
            id: Some(9),
            device: "phone".into(),
            muted: false,
        };
        led(&mut d, &state, 0);
        assert_eq!(d.calls.in_progress(), Some((9, false)));
        let ended = CallProgress {
            id: 9,
            device: "phone".into(),
            phase: CallPhase::Ended,
        };
        progress(&mut d, &ended);
        assert!(d.calls.in_progress().is_none() && !d.notes.has(ACTIVE));
    }
}
