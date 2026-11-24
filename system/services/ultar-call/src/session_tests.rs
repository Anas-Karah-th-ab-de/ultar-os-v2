use super::*;
use crate::fake::{Command, FakeAudio, FakeControl, FakeLine, ManualClock};
use ultar_bus::messages::call::{Cellular, Dial, Others, SendText, Ultar};

const SIM: Way = Way::Cellular(1);

struct Rig {
    s: Session,
    ultar: FakeControl,
    cell: FakeControl,
    audio: FakeAudio,
    clock: ManualClock,
}

fn rig() -> Rig {
    let (u, ultar) = FakeLine::new(&[Way::Ultar]);
    let (c, cell) = FakeLine::new(&[Way::Cellular(1), Way::Cellular(2)]);
    let audio = FakeAudio::new(&[Output::Earpiece, Output::Speaker]);
    let clock = ManualClock::default();
    let mut s = Session::new(
        vec![Box::new(u), Box::new(c)],
        Box::new(audio.clone()),
        Box::new(clock.clone()),
    );
    ultar.set_ultar(Ultar::Ready, Some("UOS-1111-2222"));
    cell.set_cellular(Cellular::Ready, &[1, 7]);
    assert!(s.pump());
    Rig {
        s,
        ultar,
        cell,
        audio,
        clock,
    }
}

fn dial(way: Way, number: &str) -> Dial {
    Dial {
        way,
        number: number.into(),
        video: false,
    }
}

fn phase(s: &Session, id: u32) -> Phase {
    s.state()
        .calls
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.phase)
        .unwrap_or_else(|| s.state().ended.unwrap().phase)
}

#[test]
fn status_reports_reach_the_state_and_bad_slots_are_dropped() {
    let r = rig();
    let st = r.s.state();
    assert_eq!(st.lines.ultar, Ultar::Ready);
    assert_eq!(st.lines.own_number.as_deref(), Some("UOS-1111-2222"));
    assert_eq!(st.lines.sims, vec![1]);
    assert_eq!(st.lines.cellular, Cellular::Ready);
}

#[test]
fn outgoing_call_dials_alerts_connects_and_ends_locally() {
    let mut r = rig();
    let id = r.s.dial(&dial(Way::Ultar, "UOS-4821-7730")).unwrap();
    assert_eq!(phase(&r.s, id), Phase::Dialing);
    assert!(r.s.state().calls[0].encrypted);
    r.s.pump();
    assert_eq!(phase(&r.s, id), Phase::Alerting);
    r.clock.advance(1000);
    r.ultar.connect(r.ultar.last_dial().unwrap());
    r.s.pump();
    assert_eq!(phase(&r.s, id), Phase::Active);
    assert_eq!(r.s.state().calls[0].active_since_ms, Some(1000));
    r.s.hangup(id).unwrap();
    assert_eq!(phase(&r.s, id), Phase::Ended(End::Local));
    assert!(r.s.state().calls.is_empty());
    assert_eq!(r.s.log(false).len(), 1);
}

#[test]
fn availability_states_refuse_calls_with_the_reason() {
    let mut r = rig();
    for (state, failure) in [
        (Cellular::NoSim, Failure::NoSim),
        (Cellular::Airplane, Failure::Airplane),
        (Cellular::NoNetwork, Failure::NoNetwork),
        (Cellular::EmergencyOnly, Failure::EmergencyOnly),
    ] {
        r.cell.set_cellular(state, &[1]);
        r.s.pump();
        assert_eq!(
            r.s.dial(&dial(SIM, "0933 123 456")),
            Err(CallError::Unavailable(failure))
        );
        // Emergency numbers always go out on the mobile network.
        let id = r.s.dial(&dial(SIM, "112")).unwrap();
        r.s.hangup(id).unwrap();
    }
    r.cell.set_cellular(Cellular::NoModem, &[]);
    r.s.pump();
    assert!(r.s.dial(&dial(SIM, "112")).is_err());
    r.ultar.set_ultar(Ultar::Offline, Some("UOS-1"));
    r.s.pump();
    assert_eq!(
        r.s.dial(&dial(Way::Ultar, "UOS-1")),
        Err(CallError::Unavailable(Failure::Offline))
    );
    r.ultar.set_ultar(Ultar::SignedOut, None);
    r.s.pump();
    assert_eq!(
        r.s.dial(&dial(Way::Ultar, "UOS-1")),
        Err(CallError::Unavailable(Failure::SignedOut))
    );
}

#[test]
fn bad_requests_are_refused() {
    let mut r = rig();
    assert_eq!(r.s.dial(&dial(SIM, "")), Err(CallError::BadNumber));
    assert_eq!(
        r.s.dial(&dial(Way::Ultar, "0933")),
        Err(CallError::BadNumber)
    );
    let video = Dial {
        way: SIM,
        number: "0933".into(),
        video: true,
    };
    assert_eq!(r.s.dial(&video), Err(CallError::Unsupported));
    assert_eq!(
        r.s.dial(&dial(Way::Cellular(2), "0933")),
        Err(CallError::Unavailable(Failure::NoSim))
    );
    assert_eq!(r.s.hangup(99), Err(CallError::UnknownCall));
    assert_eq!(r.s.mute(true), Err(CallError::WrongPhase));
    assert_eq!(
        r.s.set_output(Output::Bluetooth),
        Err(CallError::Unsupported)
    );
    let id = r.s.dial(&dial(SIM, "0933")).unwrap();
    assert_eq!(r.s.answer(id, Others::Hold), Err(CallError::WrongPhase));
    assert_eq!(r.s.decline(id), Err(CallError::WrongPhase));
    assert_eq!(r.s.hold(id, true), Err(CallError::WrongPhase));
    assert_eq!(r.s.dtmf(id, b'5'), Err(CallError::WrongPhase));
    assert_eq!(r.s.dtmf(id, b'x'), Err(CallError::BadNumber));
    assert_eq!(r.s.set_video(id, true), Err(CallError::Unsupported));
}

#[test]
fn failed_dial_is_shown_as_ended_and_logged() {
    let mut r = rig();
    r.cell
        .fail_next(crate::line::LineError::Unavailable(Failure::Rejected));
    assert_eq!(
        r.s.dial(&dial(SIM, "0933")),
        Err(CallError::Unavailable(Failure::Rejected))
    );
    let ended = r.s.state().ended.unwrap();
    assert_eq!(ended.phase, Phase::Ended(End::Failed(Failure::Rejected)));
    r.cell.fail_next(crate::line::LineError::Backend);
    assert_eq!(r.s.dial(&dial(SIM, "0933")), Err(CallError::Backend));
    assert_eq!(
        r.s.state().ended.unwrap().phase,
        Phase::Ended(End::Failed(Failure::Rejected))
    );
}

#[test]
fn busy_and_remote_end() {
    let mut r = rig();
    let id = r.s.dial(&dial(SIM, "0933")).unwrap();
    r.cell.end(r.cell.last_dial().unwrap(), End::Busy);
    r.s.pump();
    assert_eq!(phase(&r.s, id), Phase::Ended(End::Busy));
    let id = r.s.dial(&dial(SIM, "0933")).unwrap();
    let key = r.cell.last_dial().unwrap();
    r.cell.connect(key);
    r.cell.end(key, End::Remote);
    r.s.pump();
    assert_eq!(phase(&r.s, id), Phase::Ended(End::Remote));
    // Events for calls the session does not know change nothing.
    r.cell.connect(999);
    r.cell.end(999, End::Remote);
    assert!(!r.s.pump());
}

#[test]
fn incoming_answer_mute_speaker_hold_dtmf_hangup() {
    let mut r = rig();
    let key = r.cell.ring(SIM, "+963 11 555 0142", Some("ليلى حداد"));
    r.s.pump();
    let call = r.s.state().calls[0].clone();
    assert_eq!(call.phase, Phase::Ringing);
    assert_eq!(call.name.as_deref(), Some("ليلى حداد"));
    assert!(!call.encrypted);
    r.s.answer(call.id, Others::Hold).unwrap();
    assert_eq!(phase(&r.s, call.id), Phase::Active);
    r.s.mute(true).unwrap();
    r.s.set_output(Output::Speaker).unwrap();
    assert_eq!(r.audio.current(), (true, Output::Speaker));
    r.s.dtmf(call.id, b'#').unwrap();
    r.s.hold(call.id, true).unwrap();
    assert_eq!(phase(&r.s, call.id), Phase::Held);
    assert_eq!(r.s.hold(call.id, true), Err(CallError::WrongPhase));
    r.s.hold(call.id, false).unwrap();
    assert_eq!(phase(&r.s, call.id), Phase::Active);
    r.s.hangup(call.id).unwrap();
    assert!(!r.s.state().audio.muted, "mute resets after the last call");
    let cmds = r.cell.commands();
    for c in [
        Command::Answer(key),
        Command::Dtmf(key, b'#'),
        Command::Hold(key, true),
        Command::Hold(key, false),
        Command::Release(key),
    ] {
        assert!(cmds.contains(&c), "{c:?}");
    }
}

#[test]
fn missed_declined_and_blocked_calls() {
    let mut r = rig();
    let key = r.cell.ring(SIM, "0933", None);
    r.s.pump();
    r.cell.end(key, End::Remote);
    r.s.pump();
    assert_eq!(r.s.log(true).len(), 1, "missed");
    r.cell.ring(SIM, "0944", None);
    r.s.pump();
    let id = r.s.state().calls[0].id;
    r.s.decline(id).unwrap();
    assert_eq!(phase(&r.s, id), Phase::Ended(End::Declined));
    r.s.block("0955", true).unwrap();
    let key = r.cell.ring(SIM, "0955", None);
    assert!(!r.s.pump());
    assert!(r.s.state().calls.is_empty());
    assert!(r.cell.commands().contains(&Command::Release(key)));
    let entries = r.s.log(false);
    assert_eq!(entries.len(), 2);
    r.s.forget(entries[0].id).unwrap();
    assert_eq!(r.s.log(false).len(), 1);
}

#[test]
fn second_incoming_call_waits_and_can_hold_or_end_the_first() {
    let mut r = rig();
    r.cell.ring(SIM, "0933", None);
    r.s.pump();
    let first = r.s.state().calls[0].id;
    r.s.answer(first, Others::Hold).unwrap();
    assert_eq!(r.s.dial(&dial(SIM, "0944")).map(|_| ()), Ok(()));
    let second_out = r.s.state().calls[1].id;
    assert_eq!(
        phase(&r.s, first),
        Phase::Held,
        "dialling holds the active call"
    );
    r.s.hangup(second_out).unwrap();
    r.s.hold(first, false).unwrap();
    r.ultar.ring(Way::Ultar, "UOS-2290-1184", None);
    r.s.pump();
    let second = r.s.state().calls[1].clone();
    assert_eq!(second.phase, Phase::Waiting);
    assert_eq!(r.s.dial(&dial(SIM, "0955")), Err(CallError::TooManyCalls));
    // A third call is refused (busy) and logged as missed.
    r.cell.ring(SIM, "0966", None);
    assert!(!r.s.pump());
    assert_eq!(r.s.state().calls.len(), 2);
    r.s.answer(second.id, Others::Hold).unwrap();
    assert_eq!(phase(&r.s, first), Phase::Held);
    assert_eq!(phase(&r.s, second.id), Phase::Active);
    // Swap back.
    r.s.hold(first, false).unwrap();
    assert_eq!(phase(&r.s, second.id), Phase::Held);
    r.s.hangup(first).unwrap();
    r.ultar.ring(Way::Ultar, "UOS-3", None);
    r.s.pump();
    let third = r.s.state().calls[1].id;
    r.s.answer(third, Others::End).unwrap();
    let st = r.s.state();
    assert_eq!(st.calls.len(), 1);
    assert_eq!(st.calls[0].id, third);
}

#[test]
fn waiting_call_rings_when_the_other_ends() {
    let mut r = rig();
    r.cell.ring(SIM, "0933", None);
    r.s.pump();
    let first = r.s.state().calls[0].id;
    r.s.answer(first, Others::Hold).unwrap();
    r.cell.ring(SIM, "0944", None);
    r.s.pump();
    r.s.hangup(first).unwrap();
    assert_eq!(r.s.state().calls[0].phase, Phase::Ringing);
}

#[test]
fn video_camera_share_on_ultar_calls() {
    let mut r = rig();
    let id =
        r.s.dial(&Dial {
            way: Way::Ultar,
            number: "UOS-1".into(),
            video: true,
        })
        .unwrap();
    assert_eq!(r.s.share(id, true), Err(CallError::WrongPhase));
    r.ultar.connect(r.ultar.last_dial().unwrap());
    r.s.pump();
    assert!(r.s.state().calls[0].media.camera_on);
    r.s.set_video(id, false).unwrap();
    r.s.next_camera(id).unwrap();
    r.s.share(id, true).unwrap();
    let m = r.s.state().calls[0].media;
    assert!(!m.camera_on && m.sharing && m.camera == 1 && m.cameras == 2);
    r.s.next_camera(id).unwrap();
    assert_eq!(r.s.state().calls[0].media.camera, 0);
    r.ultar.set_cameras(1);
    r.s.hangup(id).unwrap();
    let id = r.s.dial(&dial(Way::Ultar, "UOS-1")).unwrap();
    assert_eq!(r.s.next_camera(id), Err(CallError::Unsupported));
}

#[test]
fn texts_are_checked_and_sent() {
    let mut r = rig();
    let t = SendText {
        way: SIM,
        number: "0933".into(),
        text: "سأتصل بك لاحقاً".into(),
    };
    r.s.send_text(&t).unwrap();
    let mut bad = t.clone();
    bad.text = String::new();
    assert_eq!(r.s.send_text(&bad), Err(CallError::BadText));
    assert!(matches!(r.cell.commands().last(), Some(Command::Text(..))));
}
