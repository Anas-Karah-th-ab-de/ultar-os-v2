//! What the service says aloud: the helper's answers (keys of its catalogue) and the service's
//! own sentences (`voice.say.*`), rendered in the speech language, and the short tone that
//! tells the person the helper listens.

use std::f32::consts::PI;
use ultar_bus::messages::agent::LOCALES;
use ultar_bus::messages::voice::{Play, SAMPLE_RATE};
use ultar_bus::{Text, TextArg};
use ultar_i18n::{Arg, Lang, Translator};

/// The service's own texts by language, Arabic first.
const OWN: [(&str, &str); 3] = [
    ("ar", include_str!("../locales/ar.ftl")),
    ("en", include_str!("../locales/en.ftl")),
    ("de", include_str!("../locales/de.ftl")),
];

/// Longest sentence of the tone (milliseconds).
const TONE_MS: usize = 140;

/// The translator of the speech language with the helper's catalogue and the service's own.
pub fn translator(language: &str) -> Translator {
    let lang = Lang::parse(language).unwrap_or(Lang::Ar);
    let mut tr = Translator::new(lang);
    for (tag, source) in OWN.iter().chain(LOCALES.iter()) {
        if let Some(each) = Lang::parse(tag) {
            // a catalogue that cannot be read leaves its keys unspoken, never a crash
            let _ = tr.add_source(each, source);
        }
    }
    tr
}

/// The words of a text in the speech language (arguments as the catalogue expects them:
/// `arg0`, `arg1`, ...; key arguments are translated first).
pub fn say(tr: &Translator, text: &Text) -> String {
    let values: Vec<(String, String)> = text
        .args
        .iter()
        .enumerate()
        .map(|(i, arg)| {
            let value = match arg {
                TextArg::Verbatim(s) => s.clone(),
                TextArg::Number(n) => n.to_string(),
                TextArg::Key(k) => tr.t(k).into_owned(),
            };
            (format!("arg{i}"), value)
        })
        .collect();
    let args: Vec<(&str, Arg<'_>)> = values
        .iter()
        .map(|(k, v)| (k.as_str(), Arg::Str(v.as_str())))
        .collect();
    tr.t_args(&text.key, &args)
}

/// The short rising tone played when the word was heard (a soft 880 Hz beep).
pub fn tone() -> Play {
    let n = SAMPLE_RATE as usize * TONE_MS / 1000;
    let samples = (0..n)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let edge = (i.min(n - i) as f32 / (n as f32 / 4.0)).min(1.0);
            ((2.0 * PI * 880.0 * t).sin() * 0.35 * edge * 32767.0) as i16
        })
        .collect();
    Play {
        rate: SAMPLE_RATE,
        samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_own_sentences_exist_in_every_language() {
        let keys = |source: &str| -> Vec<String> {
            source
                .lines()
                .filter_map(|l| l.split(" = ").next())
                .filter(|k| k.starts_with("voice.say."))
                .map(str::to_owned)
                .collect()
        };
        let ar = keys(OWN[0].1);
        assert!(ar.len() >= 4);
        for (_, source) in OWN {
            assert_eq!(keys(source), ar);
        }
        for tag in ["ar", "en", "de"] {
            let tr = translator(tag);
            for key in &ar {
                let said = say(&tr, &Text::key(key));
                assert_ne!(&said, key, "{tag} {key}");
                assert!(!said.is_empty());
            }
        }
    }

    #[test]
    fn the_refusal_is_the_owners_sentence() {
        let de = say(&translator("de"), &Text::key("voice.say.not_standard_user"));
        assert_eq!(de, "Du bist nicht der Standardnutzer.");
        let ar = say(&translator("ar"), &Text::key("voice.say.not_standard_user"));
        assert!(ar.contains("المستخدم الأساسي"));
    }

    #[test]
    fn the_helpers_answers_are_spoken_with_their_arguments() {
        let tr = translator("en");
        let answer = Text::key("agent.says").with(TextArg::Verbatim("Volume is 40.".into()));
        assert_eq!(say(&tr, &answer), "Volume is 40.");
        assert_eq!(say(&tr, &Text::key("agent.done")), "Done.");
    }

    #[test]
    fn the_tone_is_short_and_not_clipped() {
        let tone = tone();
        assert_eq!(tone.rate, 16_000);
        assert_eq!(tone.samples.len(), 2_240);
        assert!(tone.samples.iter().all(|s| s.abs() < 14_000));
        assert!(tone.samples.iter().any(|s| s.abs() > 8_000));
    }
}
