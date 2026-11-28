//! Real speech to text with the real programs: eSpeak NG says typical requests (Arabic first,
//! then English and German) and `whisper-server` with a real model writes them down; the
//! test prints word accuracy and time per clip for every model asked for.
//!
//! It needs the packages and models, so it does nothing unless `ULTAR_WHISPER` points at the
//! `whisper-server` program (it then reports what it measured and fails only when the
//! numbers are absurd):
//!
//! ```text
//! ULTAR_WHISPER=/pkg/whisper/bin/whisper-server \
//! ULTAR_WHISPER_MODELS=/path/to/models        (ggml-small.bin, ggml-base.bin, ...)
//! ULTAR_ESPEAK=/pkg/espeak-ng/bin/espeak-ng \
//! ULTAR_WHISPER_MODEL_NAMES=small,base        (optional, this is the default)
//! cargo test -p ultar-voiced --test real_speech -- --nocapture
//! ```
//!
//! eSpeak is a synthetic voice, not a person: the numbers say how well the model reads clean,
//! clearly spoken requests, which is the best case, not what a room and a microphone give.

use std::path::{Path, PathBuf};
use std::time::Instant;
use ultar_voiced::audio::{f32_to_pcm16, pcm16_to_f32};
use ultar_voiced::engine::{Speaker, Transcriber};
use ultar_voiced::espeak::EspeakSpeaker;
use ultar_voiced::models::ModelCatalog;
use ultar_voiced::whisper::WhisperServer;

/// Group, request. The group is the language; `ar-marks` is Arabic written with its vowel
/// marks, which eSpeak needs to pronounce Arabic well (people do not write them, so only the
/// plain group says how the helper is spoken to; the marked one shows how much of a poor
/// result is the synthetic voice).
const REQUESTS: [(&str, &str); 14] = [
    ("ar", "افتح الإعدادات"),
    ("ar", "أطفئ الواي فاي"),
    ("ar", "كم الساعة"),
    ("ar", "ارفع الصوت"),
    ("ar", "ما حالة الطقس اليوم"),
    ("ar-marks", "اِفْتَحِ الْإِعْدَادَاتِ"),
    ("ar-marks", "أَطْفِئِ الْواي فاي"),
    ("ar-marks", "كَمِ السَّاعَةُ"),
    ("ar-marks", "اِرْفَعِ الصَّوْتَ"),
    ("ar-marks", "مَا حَالَةُ الطَّقْسِ الْيَوْمَ"),
    ("en", "Turn the wifi off"),
    ("en", "What time is it"),
    ("de", "Schalte das WLAN aus"),
    ("de", "Wie spät ist es"),
];

const GROUPS: [&str; 4] = ["ar", "ar-marks", "en", "de"];

/// The language of a group.
fn language_of(group: &str) -> &str {
    group.split('-').next().unwrap_or(group)
}

struct Measure {
    model: String,
    language: &'static str,
    request: &'static str,
    heard: String,
    accuracy: f32,
    seconds: f32,
}

/// Lower case, no vowel marks, one form of the letters Arabic writers mix, words only.
fn normalise(text: &str) -> Vec<String> {
    let mut clean = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            '\u{064B}'..='\u{065F}' | '\u{0640}' | '-' => {}
            'أ' | 'إ' | 'آ' | 'ٱ' => clean.push('ا'),
            'ى' => clean.push('ي'),
            'ة' => clean.push('ه'),
            c if c.is_alphanumeric() => clean.push(c),
            _ => clean.push(' '),
        }
    }
    clean.split_whitespace().map(str::to_owned).collect()
}

/// Edit distance between two word lists.
fn distance(a: &[String], b: &[String]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(x != y))
                .min(row[j] + 1)
                .min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

/// The share of the request's words that came out right (1.0: all, in order).
fn accuracy(request: &str, heard: &str) -> f32 {
    let want = normalise(request);
    let got = normalise(heard);
    if want.is_empty() {
        return 0.0;
    }
    (1.0 - distance(&want, &got) as f32 / want.len() as f32).max(0.0)
}

/// Linear resampling to 16 kHz (what the microphone stream delivers).
fn to_16k(samples: &[i16], rate: u32) -> Vec<i16> {
    if rate == 16_000 {
        return samples.to_vec();
    }
    let input = pcm16_to_f32(samples);
    let step = rate as f32 / 16_000.0;
    let count = (input.len() as f32 / step) as usize;
    let out: Vec<f32> = (0..count)
        .map(|n| {
            let at = n as f32 * step;
            let i = at as usize;
            let frac = at - i as f32;
            let next = input.get(i + 1).copied().unwrap_or(input[i]);
            input[i] * (1.0 - frac) + next * frac
        })
        .collect();
    f32_to_pcm16(&out)
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

fn measure_model(
    model: &str,
    clips: &[(&'static str, &'static str, Vec<i16>)],
    server: &Path,
    models: &Path,
) -> Vec<Measure> {
    let mut out = Vec::new();
    // one server per language: the server is started for one language and kept
    for (n, language) in GROUPS.into_iter().enumerate() {
        let port = 19_300 + (std::process::id() % 400) as u16 * 4 + n as u16;
        let whisper = WhisperServer::new(server.to_path_buf(), ModelCatalog::new(models), port, 4);
        let mut first = true;
        for (lang, request, pcm) in clips.iter().filter(|c| c.0 == language) {
            let started = Instant::now();
            let heard = whisper
                .transcribe(pcm, 16_000, language_of(lang), model)
                .unwrap_or_else(|e| panic!("{model} {lang}: {e}"));
            let seconds = started.elapsed().as_secs_f32();
            if first {
                println!("  ({model}/{lang}: first clip includes starting the server)");
                first = false;
            }
            out.push(Measure {
                model: model.to_owned(),
                language: lang,
                request,
                accuracy: accuracy(request, &heard),
                heard,
                seconds,
            });
        }
    }
    out
}

#[test]
fn whisper_reads_espeak_requests_in_arabic_english_and_german() {
    let Some(server) = env_path("ULTAR_WHISPER") else {
        println!("ULTAR_WHISPER is not set: real speech test skipped");
        return;
    };
    let models = env_path("ULTAR_WHISPER_MODELS").expect("ULTAR_WHISPER_MODELS");
    let espeak = env_path("ULTAR_ESPEAK").expect("ULTAR_ESPEAK");
    let names = std::env::var("ULTAR_WHISPER_MODEL_NAMES").unwrap_or_else(|_| "small,base".into());

    let clips = speak_requests(espeak);
    let catalog = ModelCatalog::new(&models);
    let mut all = Vec::new();
    for model in names.split(',').map(str::trim).filter(|m| !m.is_empty()) {
        if !catalog.installed(model) {
            println!("model {model}: not in {}, skipped", models.display());
            continue;
        }
        all.extend(measure_model(model, &clips, &server, &models));
    }
    assert!(!all.is_empty(), "no requested model is installed");
    let best_english = report(&all);
    // English is the sanity check of the whole chain (the best model must read it);
    // Arabic and German are only reported
    assert!(
        best_english >= 0.5,
        "English at {best_english}: chain broken?"
    );
}

/// Every request spoken by eSpeak NG, at 16 kHz.
fn speak_requests(espeak: PathBuf) -> Vec<(&'static str, &'static str, Vec<i16>)> {
    let speaker = EspeakSpeaker::new(espeak, env_path("ULTAR_ESPEAK_DATA"));
    REQUESTS
        .iter()
        .map(|(language, request)| {
            let said = speaker
                .synthesize(request, language_of(language))
                .unwrap_or_else(|e| panic!("espeak {language}: {e}"));
            (*language, *request, to_16k(&said.samples, said.rate))
        })
        .collect()
}

/// Prints every clip and the means per model and group; gives the best English accuracy.
fn report(all: &[Measure]) -> f32 {
    println!("\nmodel  lang  accuracy  seconds  request -> heard");
    for m in all {
        println!(
            "{:<6} {:<4}  {:>6.0}%  {:>7.1}  {} -> {}",
            m.model,
            m.language,
            m.accuracy * 100.0,
            m.seconds,
            m.request,
            m.heard
        );
    }
    println!("\nmodel  group     mean accuracy  mean seconds");
    let mut best_english = 0.0f32;
    for model in all
        .iter()
        .map(|m| m.model.clone())
        .collect::<std::collections::BTreeSet<_>>()
    {
        for group in GROUPS {
            let (acc, secs) = mean(all, &model, group);
            println!(
                "{model:<6} {group:<8}  {:>12.0}%  {secs:>10.1}",
                acc * 100.0
            );
            if group == "en" {
                best_english = best_english.max(acc);
            }
        }
    }
    best_english
}

/// Mean accuracy and seconds of a model on a group.
fn mean(all: &[Measure], model: &str, group: &str) -> (f32, f32) {
    let rows: Vec<&Measure> = all
        .iter()
        .filter(|m| m.model == model && m.language == group)
        .collect();
    let n = rows.len().max(1) as f32;
    (
        rows.iter().map(|m| m.accuracy).sum::<f32>() / n,
        rows.iter().map(|m| m.seconds).sum::<f32>() / n,
    )
}

#[test]
fn accuracy_counts_missing_wrong_and_extra_words() {
    assert_eq!(accuracy("Turn the wifi off", "Turn the Wi-Fi off."), 1.0);
    assert_eq!(accuracy("one two three four", "one two four"), 0.75);
    assert_eq!(accuracy("one two", "three"), 0.0);
    assert_eq!(accuracy("افتح الإعدادات", "افتح الاعدادات."), 1.0);
    assert_eq!(accuracy("", "anything"), 0.0);
}
