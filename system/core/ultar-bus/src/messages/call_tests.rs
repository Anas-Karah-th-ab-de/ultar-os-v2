use super::*;
use crate::message::{decode, encode, Call, Topic};
use crate::topic::{is_valid_name, is_valid_segment_name};

fn check<C: Call>() -> &'static str {
    assert!(is_valid_name(C::SERVICE));
    assert!(is_valid_segment_name(C::METHOD), "{}", C::METHOD);
    C::METHOD
}

#[test]
fn every_call_has_a_valid_unique_name() {
    let mut names = vec![
        check::<Dial>(),
        check::<Emergency>(),
        check::<Answer>(),
        check::<Decline>(),
        check::<Hangup>(),
        check::<Hold>(),
        check::<Mute>(),
        check::<SetOutput>(),
        check::<Dtmf>(),
        check::<SetVideo>(),
        check::<NextCamera>(),
        check::<Share>(),
        check::<SendText>(),
        check::<Log>(),
        check::<Forget>(),
        check::<ClearLog>(),
        check::<Block>(),
        check::<Snapshot>(),
    ];
    let n = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), n);
    assert!(is_valid_name(CallState::TOPIC));
    const { assert!(CallState::RETAINED) };
}

#[test]
fn emergency_carries_no_number_and_answers_like_dial() {
    let call = Emergency {};
    assert_eq!(decode::<Emergency>(&encode(&call).unwrap()).unwrap(), call);
    assert_eq!(Emergency::METHOD, "emergency");
    let reply = DialReply { id: 7 };
    assert_eq!(
        decode::<DialReply>(&encode(&reply).unwrap()).unwrap(),
        reply
    );
}

#[test]
fn error_codes_round_trip_and_unknown_codes_are_none() {
    let mut all = vec![
        CallError::UnknownCall,
        CallError::BadNumber,
        CallError::BadText,
        CallError::TooManyCalls,
        CallError::WrongPhase,
        CallError::Unsupported,
        CallError::Backend,
        CallError::Full,
    ];
    all.extend(FAILURES.iter().map(|f| CallError::Unavailable(*f)));
    for e in all {
        assert_eq!(CallError::from_code(e.code()), Some(e), "{e}");
    }
    for bad in [0, 9, 99, 110, u32::MAX] {
        assert_eq!(CallError::from_code(bad), None);
    }
}

#[test]
fn numbers_are_checked() {
    for good in ["UOS-4821-7730", "+963 11 555 0142", "112", "*111#", "#"] {
        assert_eq!(check_number(good), Ok(()), "{good}");
    }
    let long = "1".repeat(MAX_NUMBER_LEN + 1);
    for bad in ["", "abc", "12\n3", "UOS-", "--", long.as_str(), "١٢٣"] {
        assert_eq!(check_number(bad), Err(CallError::BadNumber), "{bad:?}");
    }
}

#[test]
fn texts_are_checked() {
    assert_eq!(check_text("سأتصل بك لاحقاً"), Ok(()));
    let long = "a".repeat(MAX_TEXT_LEN + 1);
    for bad in ["", "   ", "a\u{7}b", long.as_str()] {
        assert_eq!(check_text(bad), Err(CallError::BadText));
    }
}

#[test]
fn emergency_numbers_and_phases() {
    assert!(is_emergency("112"));
    assert!(is_emergency(" 911 "));
    assert!(!is_emergency("1120"));
    assert!(Phase::Ringing.is_incoming() && Phase::Waiting.is_incoming());
    assert!(!Phase::Active.is_incoming());
    assert!(Phase::Held.is_live());
    assert!(!Phase::Ended(End::Busy).is_live());
}

#[test]
fn state_round_trips_on_the_wire() {
    let state = CallState {
        lines: Lines {
            cellular: Cellular::EmergencyOnly,
            sims: vec![1],
            ultar: Ultar::Ready,
            own_number: Some("UOS-1234-5678".into()),
        },
        calls: vec![CallInfo {
            id: 3,
            way: Way::Cellular(1),
            direction: Direction::Incoming,
            number: "+963 11 555 0142".into(),
            name: Some("ليلى حداد".into()),
            phase: Phase::Waiting,
            media: Media::default(),
            encrypted: false,
            active_since_ms: None,
        }],
        ended: None,
        audio: Audio {
            muted: true,
            output: Output::Speaker,
            outputs: vec![Output::Earpiece, Output::Speaker],
        },
        clock_ms: 77,
    };
    let bytes = encode(&state).unwrap();
    assert_eq!(decode::<CallState>(&bytes).unwrap(), state);
}
