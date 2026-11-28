//! The detectors against real eSpeak NG speech (fixtures made with the `espeak-ng` package,
//! resampled to 16 kHz): the same word again, other words, another voice. Synthetic speech is
//! not a person: these numbers show that the matchers separate words and voices that differ
//! clearly, not how they behave with every real voice and room.

use std::path::PathBuf;
use ultar_voiced::audio::{decode_wav, pcm16_to_f32};
use ultar_voiced::detect::{VoicePrint, WakeWord};
use ultar_voiced::mfcc::Mfcc;

fn clip(name: &str) -> Vec<f32> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.wav"));
    let bytes = std::fs::read(&path).unwrap_or_default();
    let wav = decode_wav(&bytes).unwrap_or_else(|| panic!("{} is not a WAV", path.display()));
    assert_eq!(wav.rate, 16_000, "{name}");
    pcm16_to_f32(&wav.samples)
}

fn word(takes: &[&str]) -> WakeWord {
    let mfcc = Mfcc::new();
    let takes: Vec<Vec<f32>> = takes.iter().map(|n| clip(n)).collect();
    WakeWord::from_takes(&mfcc, &takes).unwrap()
}

#[test]
fn the_wake_word_is_the_same_word_again_and_not_another_word_or_voice() {
    let mfcc = Mfcc::new();
    let wake = word(&["wake1", "wake2", "wake3"]);
    for same in ["wake4", "wake5"] {
        assert!(
            wake.matches(&mfcc, &clip(same)),
            "{same}: {}",
            wake.distance(&mfcc, &clip(same))
        );
    }
    // other words, one that sounds alike ("Hey Kim"), a long request, Arabic, other voices
    for other in [
        "other1", "other2", "other3", "other4", "req", "arab", "wakeF", "wakeC",
    ] {
        assert!(
            !wake.matches(&mfcc, &clip(other)),
            "{other}: {}",
            wake.distance(&mfcc, &clip(other))
        );
    }
    // the closest wrong word is still clearly beyond the threshold
    let closest = ["other1", "other2", "other3", "other4", "wakeF"]
        .iter()
        .map(|n| wake.distance(&mfcc, &clip(n)))
        .fold(f32::MAX, f32::min);
    assert!(
        closest > wake.threshold() * 1.8,
        "{closest} vs {}",
        wake.threshold()
    );
}

#[test]
fn the_voice_print_takes_the_person_and_refuses_clearly_different_voices() {
    let mfcc = Mfcc::new();
    let takes: Vec<Vec<f32>> = ["s1", "s2", "s3"].iter().map(|n| clip(n)).collect();
    let print = VoicePrint::from_takes(&mfcc, &takes).unwrap();
    assert!(
        print.accepts(&mfcc, &clip("req")),
        "{}",
        print.distance(&mfcc, &clip("req"))
    );
    for stranger in ["reqF", "reqC"] {
        assert!(
            !print.accepts(&mfcc, &clip(stranger)),
            "{stranger}: {} vs {}",
            print.distance(&mfcc, &clip(stranger)),
            print.threshold()
        );
    }
    // a second male voice of the same synthesiser lies between the person and the strangers:
    // the print is a weak check and must not be taken for a lock (docs/integration/ultar-voiced.md)
    let near = print.distance(&mfcc, &clip("reqM2"));
    assert!(near > print.distance(&mfcc, &clip("req")), "{near}");
}
