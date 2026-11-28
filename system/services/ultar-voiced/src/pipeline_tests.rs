use super::*;
use std::f32::consts::PI;

const CHUNK: usize = 1600;

/// A synthetic word: bursts around `base` Hz, `speed` stretches it in time.
fn word(base: f32, speed: f32, secs: f32) -> Vec<f32> {
    let len = (secs * 16_000.0 * speed) as usize;
    (0..len)
        .map(|i| {
            let t = i as f32 / 16_000.0 / speed;
            let burst = 0.55 + 0.45 * (2.0 * PI * 3.0 * t).sin();
            let glide = base * (1.0 + 0.3 * (2.0 * PI * 1.5 * t).sin());
            (2.0 * PI * glide * t).sin() * 0.25 * burst
        })
        .collect()
}

fn quiet(secs: f32) -> Vec<f32> {
    vec![0.0; (secs * 16_000.0) as usize]
}

fn feed_all(pipe: &mut Pipeline, samples: &[f32]) -> Vec<Effect> {
    let mut out = Vec::new();
    for chunk in samples.chunks(CHUNK) {
        out.extend(pipe.feed(chunk));
    }
    out
}

fn enrolled(base: f32) -> WakeWord {
    let mfcc = Mfcc::new();
    let takes: Vec<Vec<f32>> = [1.0f32, 1.08, 0.94]
        .iter()
        .map(|s| word(base, *s, 0.75))
        .collect();
    WakeWord::from_takes(&mfcc, &takes).unwrap()
}

fn word_then_request(base: f32, request: &[f32]) -> Vec<f32> {
    let mut s = quiet(0.5);
    s.extend(word(base, 1.02, 0.75));
    s.extend(quiet(0.6));
    s.extend(quiet(0.4));
    s.extend(request);
    s.extend(quiet(1.6));
    s
}

#[test]
fn a_request_flows_from_the_word_to_the_text() {
    let mut pipe = Pipeline::new();
    pipe.enrol(Some(enrolled(400.0)), None, None);
    pipe.start();
    assert_eq!(pipe.phase(), Phase::Listening);
    let effects = feed_all(&mut pipe, &word_then_request(400.0, &word(300.0, 1.0, 2.0)));
    assert_eq!(effects[0], Effect::BeginRecording);
    let Effect::Transcribe(request) = &effects[1] else {
        panic!("{effects:?}");
    };
    assert!(request.len() > 16_000 * 2, "{}", request.len());
    assert_eq!(pipe.phase(), Phase::Thinking);
    assert!(pipe.on_text(false));
    pipe.answering();
    assert_eq!(pipe.phase(), Phase::Answering);
    // while it speaks, what it hears is ignored
    assert!(feed_all(&mut pipe, &word(400.0, 1.0, 0.75)).is_empty());
    pipe.resume();
    assert_eq!(pipe.phase(), Phase::Listening);
}

#[test]
fn another_word_or_silence_wakes_nothing_and_nothing_is_kept() {
    let mut pipe = Pipeline::new();
    pipe.enrol(Some(enrolled(400.0)), None, None);
    pipe.start();
    let mut stream = quiet(1.0);
    stream.extend(word(1_900.0, 1.0, 0.75));
    stream.extend(quiet(2.0));
    stream.extend(word(400.0, 1.0, 5.0)); // a long talk is never looked at as the word
    stream.extend(quiet(2.0));
    assert!(feed_all(&mut pipe, &stream).is_empty());
    assert_eq!(pipe.phase(), Phase::Listening);
    // off: nothing is heard at all
    pipe.stop();
    assert_eq!(pipe.phase(), Phase::Idle);
    assert!(feed_all(&mut pipe, &word_then_request(400.0, &word(300.0, 1.0, 2.0))).is_empty());
}

#[test]
fn locked_means_only_the_secret_word_counts() {
    let mut pipe = Pipeline::new();
    pipe.enrol(Some(enrolled(400.0)), Some(enrolled(1_500.0)), None);
    pipe.set_locked(true);
    pipe.start();
    assert!(pipe.can_wake());
    let stream = word_then_request(400.0, &word(300.0, 1.0, 1.0));
    assert!(
        feed_all(&mut pipe, &stream).is_empty(),
        "the wake word is deaf while locked"
    );
    let stream = word_then_request(1_500.0, &word(300.0, 1.0, 1.0));
    assert_eq!(feed_all(&mut pipe, &stream)[0], Effect::BeginRecording);
    // locked without a secret word: nothing can wake it
    let mut none = Pipeline::new();
    none.enrol(Some(enrolled(400.0)), None, None);
    none.set_locked(true);
    none.start();
    assert!(!none.can_wake());
    none.set_locked(false);
    assert!(none.can_wake());
}

#[test]
fn silence_after_the_word_gives_up() {
    let mut pipe = Pipeline::new();
    pipe.enrol(Some(enrolled(400.0)), None, None);
    pipe.start();
    let mut stream = quiet(0.5);
    stream.extend(word(400.0, 1.02, 0.75));
    stream.extend(quiet(0.6));
    stream.extend(quiet(8.0));
    let effects = feed_all(&mut pipe, &stream);
    assert_eq!(effects, vec![Effect::BeginRecording, Effect::GaveUp]);
    assert_eq!(pipe.phase(), Phase::Listening);
}

#[test]
fn a_stranger_is_refused_once_the_person_confirmed_the_next_is_accepted() {
    let mfcc = Mfcc::new();
    let sentences: Vec<Vec<f32>> = [1.0f32, 1.1, 0.9]
        .iter()
        .map(|s| word(500.0, *s, 2.0))
        .collect();
    let print = VoicePrint::from_takes(&mfcc, &sentences).unwrap();
    let mut pipe = Pipeline::new();
    pipe.enrol(Some(enrolled(400.0)), None, Some(print));
    pipe.start();
    let person = feed_all(
        &mut pipe,
        &word_then_request(400.0, &word(500.0, 1.05, 2.0)),
    );
    assert!(matches!(person[1], Effect::Transcribe(_)), "{person:?}");
    pipe.resume();
    let stranger = feed_all(
        &mut pipe,
        &word_then_request(400.0, &word(2_600.0, 1.0, 2.0)),
    );
    assert_eq!(
        stranger,
        vec![Effect::BeginRecording, Effect::RefuseSpeaker]
    );
    assert_eq!(pipe.phase(), Phase::Listening);
    // fingerprint or face: the next request passes once
    pipe.accept_next_speaker();
    let again = feed_all(
        &mut pipe,
        &word_then_request(400.0, &word(2_600.0, 1.0, 2.0)),
    );
    assert!(matches!(again[1], Effect::Transcribe(_)));
    pipe.resume();
    let after = feed_all(
        &mut pipe,
        &word_then_request(400.0, &word(2_600.0, 1.0, 2.0)),
    );
    assert_eq!(after[1], Effect::RefuseSpeaker);
}

#[test]
fn enrolment_takes_come_out_and_time_out() {
    let mut pipe = Pipeline::new();
    pipe.begin_enrol(EnrollKind::Wake);
    assert_eq!(pipe.phase(), Phase::Enrolling);
    let mut stream = quiet(0.4);
    stream.extend(word(400.0, 1.0, 0.75));
    stream.extend(quiet(0.6));
    let effects = feed_all(&mut pipe, &stream);
    assert!(matches!(
        effects.as_slice(),
        [Effect::Take {
            kind: EnrollKind::Wake,
            ..
        }]
    ));
    assert_eq!(pipe.phase(), Phase::Idle);
    pipe.begin_enrol(EnrollKind::Voice);
    // a word is too short for the sentence; ten seconds of nothing time out
    let mut short = word(400.0, 1.0, 0.75);
    short.extend(quiet(0.6));
    let mut effects = feed_all(&mut pipe, &short);
    effects.extend(feed_all(&mut pipe, &quiet(10.5)));
    assert_eq!(effects, vec![Effect::TakeMissed(EnrollKind::Voice)]);
}

#[test]
fn the_test_hears_the_word_or_misses_it() {
    let mut pipe = Pipeline::new();
    pipe.enrol(Some(enrolled(400.0)), None, None);
    pipe.begin_test();
    let mut stream = quiet(0.4);
    stream.extend(word(400.0, 1.0, 0.75));
    stream.extend(quiet(0.6));
    assert_eq!(feed_all(&mut pipe, &stream), vec![Effect::Heard]);
    assert_eq!(pipe.phase(), Phase::Idle);
    pipe.begin_test();
    assert_eq!(feed_all(&mut pipe, &quiet(10.5)), vec![Effect::Missed]);
}
