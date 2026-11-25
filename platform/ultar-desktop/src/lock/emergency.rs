//! The emergency part of the lock screen (DESKTOP-SPEC §12.3): the button on the cover,
//! the panel with the contact and medical details the person chose to show without
//! unlocking, and the call to the emergency number (`call.emergency` – the call service
//! reads and validates the number itself). While the call runs, the screen flashes SOS
//! when `emergency.sos_light` is on: the screen is the light the lock screen owns.
//!
//! Nothing else is revealed here: no account names beyond the chosen contact, no
//! notification content, no settings.

use super::LockState;
use crate::view::{Target, View};
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Label, Text};

/// Where the emergency call is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CallPhase {
    /// No call.
    #[default]
    Idle,
    /// `call.emergency` is on its way.
    Calling,
    /// The call was placed.
    Active,
    /// The call service could not place it (no modem, refused).
    Failed,
}

/// The emergency panel's state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmergencyState {
    /// The panel is open (only while `emergency.on_lock_screen` is on).
    pub open: bool,
    /// The call's phase.
    pub phase: CallPhase,
    /// The call service's identifier of the emergency call (to see it end in `call.state`).
    pub call_id: Option<u32>,
    /// When the call started (ms since the epoch; the SOS light's clock).
    pub started_ms: u64,
    /// `call.state` listed the call once (an older retained state without it does not
    /// mean it ended).
    pub seen_live: bool,
}

impl EmergencyState {
    /// Opens or closes the panel – only when the person switched the button on.
    pub fn flip(&mut self, on_lock_screen: bool) {
        if on_lock_screen {
            self.open = !self.open;
        }
    }

    /// The call is being placed.
    pub fn calling(&mut self, now_ms: u64) {
        self.phase = CallPhase::Calling;
        self.started_ms = now_ms;
        self.call_id = None;
        self.seen_live = false;
    }

    /// The call service answered: the call's id, or `None` (it could not).
    pub fn answered(&mut self, id: Option<u32>) {
        self.call_id = id;
        self.phase = if id.is_some() {
            CallPhase::Active
        } else {
            CallPhase::Failed
        };
    }

    /// The call ended (`call.state` no longer lists it).
    pub fn ended(&mut self) {
        self.phase = CallPhase::Idle;
        self.call_id = None;
        self.seen_live = false;
    }

    /// Takes a `call.state`: marks the call seen, and ends it once it is gone.
    pub fn follow_call(&mut self, state: &ultar_bus::messages::call::CallState) {
        let Some(id) = self.call_id else { return };
        if state.calls.iter().any(|c| c.id == id) {
            self.seen_live = true;
        } else if self.seen_live || state.ended.as_ref().is_some_and(|c| c.id == id) {
            self.ended();
        }
    }

    /// The SOS light runs while the call is being placed or is active.
    pub fn sos_active(&self) -> bool {
        matches!(self.phase, CallPhase::Calling | CallPhase::Active)
    }
}

/// One morse unit of the SOS light (ms).
const UNIT_MS: u64 = 200;

/// SOS (··· −−− ···) as on/off units, then a seven-unit pause before it repeats:
/// three dots (on/off ×3), three-unit letter gap, three dashes (on×3/off ×3),
/// three-unit letter gap, three dots, seven-unit word gap.
const SOS: [bool; 34] = [
    true, false, true, false, true, // ···
    false, false, false, // letter gap
    true, true, true, false, true, true, true, false, true, true, true, // −−−
    false, false, false, // letter gap
    true, false, true, false, true, // ···
    false, false, false, false, false, false, false, // word gap
];

/// Whether the SOS light is on `elapsed_ms` after the call started.
pub fn sos_light_on(elapsed_ms: u64) -> bool {
    let unit = usize::try_from(elapsed_ms / UNIT_MS).unwrap_or(0);
    SOS[unit % SOS.len()]
}

/// Width of the panel (`g(100)`, as the notification cards).
const WIDTH: f32 = 100.0 * Space::S4.points();

/// The open panel: only what the person stored for an emergency, and the call button.
pub(super) fn panel(view: &mut View, middle: NodeId, s: &LockState) -> Result<(), UiError> {
    let flex = Flex::column()
        .gap(Space::S12)
        .padding(Padding::symmetric(Space::S16, Space::S16));
    let card = Card::padded().flex(flex);
    let width = view.ui.size().w.min(WIDTH);
    let card = view.ui.add_sized(middle, card, Sizing::width(width))?;
    let title = TextSpec::new(TypeSize::S16).weight(ultar_ui::text::Weight::Semibold);
    view.ui
        .add(card, Text::new(Label::key("lock.emergency")).spec(title))?;
    if !details(view, card, &s.prefs)? {
        view.ui.add(
            card,
            Text::new(Label::key("lock.emergency.empty")).color(ColorRole::TextMuted),
        )?;
    }
    call_row(view, card, s)
}

/// The stored contact and medical details – data, shown exactly as stored; `false`
/// when the person stored nothing.
fn details(view: &mut View, card: NodeId, p: &super::prefs::LockPrefs) -> Result<bool, UiError> {
    let mut any = false;
    for (key, value) in [
        ("lock.emergency.contact", &p.contact_name),
        ("lock.emergency.phone", &p.contact_phone),
        ("lock.emergency.medical", &p.medical),
    ] {
        if value.trim().is_empty() {
            continue;
        }
        any = true;
        let line = view.ui.add(card, Flex::column().gap(Space::S4))?;
        view.ui.add(
            line,
            Text::new(Label::key(key))
                .spec(TextSpec::new(TypeSize::S12))
                .color(ColorRole::TextMuted),
        )?;
        view.ui.add(line, Text::new(Label::data(value)))?;
    }
    Ok(any)
}

/// The call's status line, the call button and back.
fn call_row(view: &mut View, card: NodeId, s: &LockState) -> Result<(), UiError> {
    let p = &s.prefs;
    let status = match s.emergency.phase {
        CallPhase::Calling => Some(Label::key_with(
            "lock.emergency.calling",
            &[("number", &p.number)],
        )),
        CallPhase::Active => Some(Label::key("lock.emergency.active")),
        CallPhase::Failed => Some(Label::key("lock.emergency.failed")),
        CallPhase::Idle => None,
    };
    if let Some(status) = status {
        view.ui
            .add(card, Text::new(status).spec(TextSpec::new(TypeSize::S12)))?;
    }
    let row = view
        .ui
        .add(card, Flex::row().gap(Space::S12).cross(CrossAlign::Center))?;
    let call = Button::new(Label::key_with(
        "lock.emergency.call",
        &[("number", &p.number)],
    ))
    .variant(ButtonVariant::Primary);
    let call = view.ui.add(row, call)?;
    view.mark(call, Target::EmergencyCall);
    view.ui.set_disabled(call, s.emergency.sos_active())?;
    view.ui
        .set_loading(call, s.emergency.phase == CallPhase::Calling)?;
    let back = Button::new(Label::key("lock.back")).variant(ButtonVariant::Quiet);
    let back = view.ui.add(row, back)?;
    view.mark(back, Target::EmergencyBack);
    let _ = view.ui.focus(call);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_opens_only_when_switched_on_and_the_call_has_phases() {
        let mut e = EmergencyState::default();
        e.flip(false);
        assert!(!e.open, "off: no panel");
        e.flip(true);
        assert!(e.open);
        e.calling(1000);
        assert_eq!(e.phase, CallPhase::Calling);
        assert!(e.sos_active());
        e.answered(None);
        assert_eq!(e.phase, CallPhase::Failed);
        assert!(!e.sos_active());
        e.calling(2000);
        e.answered(Some(7));
        assert_eq!((e.phase, e.call_id), (CallPhase::Active, Some(7)));
        assert!(e.sos_active());
        e.ended();
        assert_eq!((e.phase, e.call_id), (CallPhase::Idle, None));
        assert!(!e.sos_active());
        e.flip(true);
        assert!(!e.open);
    }

    #[test]
    fn the_call_ends_when_call_state_no_longer_lists_it() {
        use ultar_bus::messages::call::{CallInfo, CallState, Direction, Media, Phase, Way};
        let info = |id: u32| CallInfo {
            id,
            way: Way::Cellular(1),
            direction: Direction::Outgoing,
            number: "112".into(),
            name: None,
            phase: Phase::Active,
            media: Media::default(),
            encrypted: false,
            active_since_ms: Some(0),
        };
        let mut e = EmergencyState::default();
        e.calling(0);
        e.answered(Some(7));
        // An older retained state without the call does not end it.
        e.follow_call(&CallState::default());
        assert_eq!(e.phase, CallPhase::Active);
        let live = CallState {
            calls: vec![info(7)],
            ..CallState::default()
        };
        e.follow_call(&live);
        assert!(e.seen_live);
        e.follow_call(&CallState::default());
        assert_eq!(e.phase, CallPhase::Idle, "gone after it was seen");
        // The ended marker alone is enough too.
        e.calling(0);
        e.answered(Some(8));
        let ended = CallState {
            ended: Some(info(8)),
            ..CallState::default()
        };
        e.follow_call(&ended);
        assert_eq!(e.phase, CallPhase::Idle);
    }

    #[test]
    fn the_sos_light_is_three_short_three_long_three_short() {
        let on = |ms: u64| sos_light_on(ms);
        // First dot: one unit on, one off.
        assert!(on(0) && on(199) && !on(200) && !on(399));
        // Third dot ends at 5 units; the letter gap is 3 units off.
        assert!(on(4 * 200) && !on(5 * 200) && !on(7 * 200));
        // First dash: three units on from unit 8.
        assert!(on(8 * 200) && on(10 * 200) && !on(11 * 200));
        // The word gap (units 27–33) is off, then it repeats.
        assert!(!on(27 * 200) && !on(33 * 200));
        assert!(on(34 * 200), "the pattern repeats");
        assert_eq!(on(34 * 200 + 100), on(100));
    }
}
