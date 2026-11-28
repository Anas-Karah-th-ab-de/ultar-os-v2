//! The real `ultar-voiced` on a bus with stand-ins for the audio service and the helper, and
//! stand-in programs for the speech recogniser and the speech output (see `common`). The
//! audio is synthetic: these tests prove the plumbing, the off-by-default rule, the locked
//! screen and the speaker check, not the recognition of real voices (that needs a microphone
//! and a person).

mod common;

use common::*;
use std::time::Duration;
use ultar_bus::messages::voice::{
    EnrollKind, Forget, OwnerConfirmed, Phase, SetEnabled, SetLanguage, SetLocked, SetModel, Test,
    TestResult, VoiceError, VoiceOutcome,
};
use ultar_bus::Text;
use ultar_svckit::testkit::{wait_until, WAIT};

const SETTLE: Duration = Duration::from_millis(800);

#[test]
fn it_is_off_and_deaf_until_the_person_switches_it_on_and_trains_the_word() {
    let b = bench("voice-off", true);
    let state = b.until("the first state", |_| true);
    assert!(!state.enabled && state.phase == Phase::Idle);
    assert!(state.ready.stt && state.ready.tts);
    assert!(!state.wake.enrolled && !state.secret.enrolled && !state.voice.enrolled);
    // switching it on without a trained word does not open the microphone
    assert_eq!(b.ask(&SetEnabled { on: true }).outcome, VoiceOutcome::Done);
    let state = b.until("it is on, but not trained", |s| s.enabled);
    assert_eq!(state.error, Some(VoiceError::NotEnrolled));
    std::thread::sleep(Duration::from_millis(2_500));
    let capture = b.seen.capture.lock().unwrap().clone();
    assert!(
        capture.is_empty(),
        "the microphone was asked for: {capture:?}"
    );
    // audio that nobody asked for is never looked at
    b.speak(&word_then_request(400.0, &word(300.0, 1.0, 1.0)));
    assert_eq!(b.asked(), 0);
}

#[test]
fn training_listens_only_for_the_takes_and_leaves_it_off() {
    let b = bench("voice-train", true);
    b.until("up", |_| true);
    b.enrol_word(EnrollKind::Wake, 400.0);
    let state = b.until("trained", |s| s.wake.enrolled);
    assert!(!state.enabled, "training does not switch it on");
    b.until("idle", |s| s.phase == Phase::Idle);
    assert!(wait_until(WAIT, || {
        b.seen.capture.lock().unwrap().last() == Some(&false)
    }));
}

#[test]
fn the_word_the_request_and_the_answer() {
    let b = listening("voice-flow");
    assert_eq!(b.seen.capture.lock().unwrap().last(), Some(&true));
    b.set_text("open the settings");
    b.speak(&word_then_request(400.0, &word(500.0, 1.05, 2.0)));
    assert!(
        wait_until(WAIT, || b.asked() == 1),
        "the helper never heard the request"
    );
    assert_eq!(
        b.seen.asked.lock().unwrap()[0],
        ("open the settings".to_owned(), false)
    );
    assert!(b.log("started.log").contains("-l ar"));
    assert!(b.log("requests.log").contains("/inference"));
    assert!(
        b.log("requests.log").contains("True"),
        "the recording is a WAV"
    );
    // the tone first, then the answer, was played; the answer was spoken
    assert!(wait_until(WAIT, || b.seen.played.lock().unwrap().len() >= 2));
    assert!(!b.log("said.log").trim().is_empty());
    b.until("listening again", |s| s.phase == Phase::Listening);
    // another word wakes nothing
    b.speak(&word_then_request(3_500.0, &word(500.0, 1.0, 2.0)));
    std::thread::sleep(SETTLE);
    assert_eq!(b.asked(), 1);
}

#[test]
fn on_a_locked_screen_only_the_secret_word_wakes_and_the_helper_is_told() {
    let b = listening("voice-locked");
    assert_eq!(
        b.ask(&SetLocked { locked: true }).outcome,
        VoiceOutcome::Done
    );
    b.until("locked", |s| s.locked && s.phase == Phase::Listening);
    b.speak(&word_then_request(400.0, &word(500.0, 1.05, 2.0)));
    std::thread::sleep(SETTLE);
    assert_eq!(b.asked(), 0, "the wake word works on a locked screen");
    b.set_text("what time is it");
    b.speak(&word_then_request(1_500.0, &word(500.0, 1.05, 2.0)));
    assert!(wait_until(WAIT, || b.asked() == 1));
    assert_eq!(
        b.seen.asked.lock().unwrap()[0],
        ("what time is it".to_owned(), true)
    );
    b.until("listening again", |s| s.phase == Phase::Listening);
    // unlocked again: the wake word works
    assert_eq!(
        b.ask(&SetLocked { locked: false }).outcome,
        VoiceOutcome::Done
    );
    b.until("unlocked", |s| !s.locked && s.phase == Phase::Listening);
    b.set_text("open the settings");
    b.speak(&word_then_request(400.0, &word(500.0, 1.05, 2.0)));
    assert!(wait_until(WAIT, || b.asked() == 2));
    assert!(!b.seen.asked.lock().unwrap()[1].1);
}

#[test]
fn another_voice_is_refused_aloud_until_the_owner_confirmed() {
    let b = listening("voice-stranger");
    let stranger = word_then_request(400.0, &word(2_600.0, 1.0, 2.0));
    b.speak(&stranger);
    b.until("the speaker is refused", |s| s.verify_needed);
    assert!(wait_until(WAIT, || b.log("said.log").lines().count() >= 2));
    let tr = ultar_voiced::speech::translator("ar");
    let refusal = ultar_voiced::speech::say(&tr, &Text::key("voice.say.not_standard_user"));
    assert!(!refusal.is_empty());
    assert!(
        b.log("said.log").contains(refusal.trim()),
        "the refusal was not spoken: {}",
        b.log("said.log")
    );
    assert_eq!(b.asked(), 0, "a stranger reached the helper");
    b.until("listening again", |s| s.phase == Phase::Listening);
    // the owner confirms (fingerprint or face): the next request passes once
    assert_eq!(b.ask(&OwnerConfirmed {}).outcome, VoiceOutcome::Done);
    b.until("confirmed", |s| !s.verify_needed);
    b.set_text("call mum");
    b.speak(&stranger);
    assert!(wait_until(WAIT, || b.asked() == 1));
    assert_eq!(b.seen.asked.lock().unwrap()[0].0, "call mum");
    b.until("listening again", |s| s.phase == Phase::Listening);
    // a confirmation without a refusal does nothing
    assert_eq!(b.ask(&OwnerConfirmed {}).outcome, VoiceOutcome::Nothing);
}

#[test]
fn switching_it_off_stops_the_stream_and_nothing_is_heard() {
    let b = listening("voice-end");
    assert_eq!(b.ask(&SetEnabled { on: false }).outcome, VoiceOutcome::Done);
    b.until("off", |s| !s.enabled && s.phase == Phase::Idle);
    assert!(wait_until(WAIT, || {
        b.seen.capture.lock().unwrap().last() == Some(&false)
    }));
    b.speak(&word_then_request(400.0, &word(500.0, 1.05, 2.0)));
    std::thread::sleep(SETTLE);
    assert_eq!(b.asked(), 0);
}

#[test]
fn the_test_button_hears_the_word_and_forgetting_removes_it() {
    let b = bench("voice-test", true);
    b.until("up", |_| true);
    assert_eq!(b.ask(&Test {}).outcome, VoiceOutcome::NotEnrolled);
    b.enrol_word(EnrollKind::Wake, 400.0);
    assert_eq!(b.ask(&Test {}).outcome, VoiceOutcome::Done);
    b.until("waiting", |s| s.test == TestResult::Waiting);
    let mut audio = quiet(0.4);
    audio.extend(word(400.0, 1.0, 0.75));
    audio.extend(quiet(0.7));
    b.speak(&audio);
    b.until("heard", |s| s.test == TestResult::Heard);
    // forgetting the word removes it and the test cannot run any more
    let forget = Forget {
        kind: EnrollKind::Wake,
    };
    assert_eq!(b.ask(&forget).outcome, VoiceOutcome::Done);
    let state = b.until("forgotten", |s| !s.wake.enrolled);
    assert_eq!(state.wake.clips, 0);
    assert_eq!(b.ask(&Test {}).outcome, VoiceOutcome::NotEnrolled);
}

#[test]
fn choices_are_checked_and_a_missing_model_is_said() {
    let b = bench("voice-choices", false);
    let state = b.until("up", |_| true);
    assert!(!state.ready.stt, "no model installed");
    let language = |tag: &str| SetLanguage { tag: tag.into() };
    assert_eq!(b.ask(&language("xx")).outcome, VoiceOutcome::Invalid);
    assert_eq!(b.ask(&language("en")).outcome, VoiceOutcome::Done);
    b.until("language", |s| s.language == "en");
    let model = |name: &str| SetModel { name: name.into() };
    assert_eq!(b.ask(&model("huge")).outcome, VoiceOutcome::Invalid);
    assert_eq!(b.ask(&model("small")).outcome, VoiceOutcome::NotInstalled);
    assert_eq!(b.ask(&SetEnabled { on: true }).outcome, VoiceOutcome::Done);
    let state = b.until("on without a model", |s| s.enabled);
    assert_eq!(state.error, Some(VoiceError::NoStt));
    assert!(b.seen.capture.lock().unwrap().is_empty());
}
