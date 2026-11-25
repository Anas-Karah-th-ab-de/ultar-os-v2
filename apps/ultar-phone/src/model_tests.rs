use super::*;
use ultar_bus::messages::call::{Direction, Lines, Media};

fn ready() -> CallState {
    CallState {
        lines: Lines {
            cellular: Cellular::Ready,
            sims: vec![1],
            ultar: Ultar::Ready,
            own_number: Some("UOS-1111-2222".into()),
        },
        ..CallState::default()
    }
}

fn call(id: u32, phase: Phase) -> CallInfo {
    CallInfo {
        id,
        way: Way::Ultar,
        direction: Direction::Incoming,
        number: "UOS-4821-7730".into(),
        name: Some("ليلى حداد".into()),
        phase,
        media: Media::default(),
        encrypted: true,
        active_since_ms: Some(1000),
        safety: None,
        key_changed: false,
    }
}

#[test]
fn digits_are_ascii_and_ultar_numbers_are_grouped() {
    assert_eq!(ascii_digit('٣'), Some('3'));
    assert_eq!(ascii_digit('۷'), Some('7'));
    assert_eq!(ascii_digit('a'), None);
    let mut m = Model::default();
    m.set_state(ready(), 0);
    assert_eq!(m.current_way(), Way::Ultar);
    for k in "48217730".chars() {
        assert!(m.push_key(k));
    }
    assert_eq!(m.digits, "4821-7730");
    assert!(!m.push_key('9'), "Ultar numbers have 8 digits");
    assert!(!m.push_key('*'));
    assert_eq!(m.number(), "UOS-4821-7730");
    assert!(m.backspace());
    assert!(m.backspace());
    assert!(m.backspace());
    assert!(m.backspace());
    assert_eq!(m.digits, "4821", "the dash goes with the digit behind it");
    m.set_typed("UOS-٤٨٢١٧٧");
    assert_eq!(m.digits, "4821-77");
    m.choose_way(Way::Cellular(1));
    assert_eq!(m.number(), "4821-77".replace('-', ""));
    assert!(m.push_key('*') && m.push_key('#') && m.push_key('+'));
    m.digits.clear();
    assert!(!m.backspace());
}

#[test]
fn default_way_follows_the_account() {
    let mut m = Model::default();
    let mut st = ready();
    st.lines.ultar = Ultar::SignedOut;
    m.set_state(st, 0);
    assert_eq!(m.current_way(), Way::Cellular(1));
    assert_eq!(m.cellular_way(), Way::Cellular(1));
}

#[test]
fn screens_follow_the_calls() {
    let mut m = Model::default();
    m.set_state(ready(), 0);
    assert_eq!(m.screen(), Screen::Dial);
    let mut st = ready();
    st.calls = vec![call(1, Phase::Ringing)];
    m.set_state(st.clone(), 0);
    assert_eq!(m.screen(), Screen::Incoming(1));
    st.calls = vec![call(1, Phase::Held), call(2, Phase::Waiting)];
    m.set_state(st.clone(), 0);
    assert_eq!(m.screen(), Screen::InCall(1));
    assert_eq!(m.other_call(1).map(|c| c.id), Some(2));
    st.calls = vec![call(1, Phase::Held), call(2, Phase::Active)];
    m.set_state(st, 0);
    assert_eq!(m.screen(), Screen::InCall(2));
}

#[test]
fn availability_and_emergency_calls() {
    let mut m = Model::default();
    let mut st = ready();
    m.set_state(st.clone(), 0);
    assert!(!m.can_call(), "nothing typed");
    m.push_key('1');
    assert!(m.can_call() && m.can_video());
    m.choose_way(Way::Cellular(1));
    assert!(!m.can_video(), "no video on the mobile network");
    for (state, why) in [
        (Cellular::NoSim, Failure::NoSim),
        (Cellular::Airplane, Failure::Airplane),
        (Cellular::NoNetwork, Failure::NoNetwork),
        (Cellular::EmergencyOnly, Failure::EmergencyOnly),
    ] {
        st.lines.cellular = state;
        m.set_state(st.clone(), 0);
        assert_eq!(m.way_problem(Way::Cellular(1)), Some(why));
        m.set_typed("0933");
        assert!(!m.can_call());
        m.set_typed("112");
        assert!(m.can_call(), "emergency calls work in {state:?}");
    }
    st.lines.cellular = Cellular::NoModem;
    m.set_state(st.clone(), 0);
    assert!(!m.cellular_selectable() && !m.can_call());
    st.lines.ultar = Ultar::Offline;
    m.set_state(st, 0);
    assert_eq!(m.way_problem(Way::Ultar), Some(Failure::Offline));
}

#[test]
fn duration_ended_notice_and_suggestion() {
    let mut m = Model::default();
    let mut st = ready();
    st.calls = vec![call(1, Phase::Active)];
    st.clock_ms = 61_000;
    m.set_state(st.clone(), 5_000);
    m.now_ms = 8_000;
    assert_eq!(m.duration_s(&st.calls[0]), 63);
    assert_eq!(format_duration(63), "01:03");
    assert_eq!(format_duration(3723), "1:02:03");
    assert_eq!(m.ended(), None);
    st.calls.clear();
    st.ended = Some(call(1, Phase::Ended(End::Busy)));
    m.set_state(st.clone(), 0);
    assert!(m.ended().is_some());
    m.dismissed = Some(1);
    assert!(m.ended().is_none());
    st.ended = Some(call(2, Phase::Ended(End::Blocked)));
    m.set_state(st, 0);
    assert!(m.ended().is_none(), "blocked calls are not announced");
    m.log = vec![LogEntry {
        id: 1,
        way: Way::Ultar,
        direction: Direction::Incoming,
        missed: true,
        number: "UOS-4821-7730".into(),
        name: Some("ليلى حداد".into()),
        when_s: 0,
    }];
    m.set_typed("4821");
    assert_eq!(m.suggestion().map(|e| e.id), Some(1));
    m.filter = Filter::Missed;
    assert_eq!(m.entries().len(), 1);
    m.set_typed("9");
    assert!(m.suggestion().is_none());
}

#[test]
fn share_dialog_closes_when_its_call_is_gone() {
    let mut m = Model {
        dialog: Some(Dialog::Share(7)),
        keypad: true,
        tones: "1".into(),
        ..Model::default()
    };
    m.set_state(ready(), 0);
    assert_eq!(m.dialog, None);
    assert!(!m.keypad && m.tones.is_empty());
}
