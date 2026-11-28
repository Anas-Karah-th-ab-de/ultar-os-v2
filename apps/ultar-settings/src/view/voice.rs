//! The voice section on Settings > Assistant: the switch (off until the person turns it on),
//! the language and model of the recogniser, the training of the wake word, the secret word
//! for the locked screen and the person's voice, and a test button. It says plainly what is
//! missing (microphone, model, training). Model names are data.

use super::{k, section, Cx};
use crate::action::Act;
use crate::update::voice::{installed, KINDS, LANGUAGES};
use ultar_bus::messages::voice::{EnrollKind, Phase, TestResult, VoiceError, VoiceState, TAKES};
use ultar_ui::layout::Sizing;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Badge, Button, ButtonVariant, Label, Row, Select, Switch, Tone};

/// The words for what the service reports: its problem first, then off, then what it does.
pub fn status_key(voice: &VoiceState) -> &'static str {
    if let Some(error) = voice.error {
        return error_key(error);
    }
    if !voice.enabled {
        return "app.settings.voice.off";
    }
    match voice.phase {
        Phase::Idle => "app.settings.voice.status.idle",
        Phase::Listening => "app.settings.voice.status.listening",
        Phase::Recording => "app.settings.voice.status.recording",
        Phase::Thinking => "app.settings.voice.status.thinking",
        Phase::Answering => "app.settings.voice.status.answering",
        Phase::Enrolling => "app.settings.voice.status.enrolling",
    }
}

/// The words for a problem of the service.
pub fn error_key(error: VoiceError) -> &'static str {
    match error {
        VoiceError::NoAudio => "app.settings.voice.error.no_audio",
        VoiceError::NoStt => "app.settings.voice.error.no_stt",
        VoiceError::NoTts => "app.settings.voice.error.no_tts",
        VoiceError::NotEnrolled => "app.settings.voice.error.not_enrolled",
        VoiceError::TakeShort => "app.settings.voice.error.take_short",
        VoiceError::TakeDiffers => "app.settings.voice.error.take_differs",
        VoiceError::TakeMissed => "app.settings.voice.error.take_missed",
    }
}

fn name(kind: EnrollKind) -> &'static str {
    match kind {
        EnrollKind::Wake => "wake",
        EnrollKind::Secret => "secret",
        EnrollKind::Voice => "voice",
    }
}

fn progress(voice: &VoiceState, kind: EnrollKind) -> (bool, u8) {
    match kind {
        EnrollKind::Wake => (voice.wake.enrolled, voice.wake.clips),
        EnrollKind::Secret => (voice.secret.enrolled, voice.secret.clips),
        EnrollKind::Voice => (voice.voice.enrolled, voice.voice.clips),
    }
}

fn language(cx: &mut Cx<'_>, card: NodeId, voice: &VoiceState) -> Result<(), UiError> {
    let row =
        Row::new(k("app.settings.voice.language")).sublabel(k("app.settings.voice.language.sub"));
    let row = cx.ui.add(card, row)?;
    let options = LANGUAGES
        .iter()
        .map(|tag| k(&format!("app.settings.voice.lang.{tag}")))
        .collect();
    let chosen = LANGUAGES.iter().position(|t| *t == voice.language);
    let select =
        Select::new(k("app.settings.voice.language"), options).selected(chosen.unwrap_or(0));
    cx.act(row, select, Act::VoiceLanguage).map(|_| ())
}

fn model(cx: &mut Cx<'_>, card: NodeId, voice: &VoiceState) -> Result<(), UiError> {
    let names = installed(cx.state);
    if names.is_empty() {
        let none =
            Row::new(k("app.settings.voice.model")).sublabel(k("app.settings.voice.model.none"));
        return cx.ui.add(card, none).map(|_| ());
    }
    let row = Row::new(k("app.settings.voice.model")).sublabel(k("app.settings.voice.model.sub"));
    let row = cx.ui.add(card, row)?;
    let options = names.iter().map(|n| Label::data(n)).collect();
    let chosen = names.iter().position(|n| *n == voice.model);
    let select = Select::new(k("app.settings.voice.model"), options).selected(chosen.unwrap_or(0));
    cx.act(row, select, Act::VoiceModel).map(|_| ())
}

fn training(cx: &mut Cx<'_>, card: NodeId, voice: &VoiceState) -> Result<(), UiError> {
    for (i, kind) in KINDS.iter().enumerate() {
        let id = name(*kind);
        let (done, clips) = progress(voice, *kind);
        let sub = if done {
            k("app.settings.voice.train.done")
        } else {
            let (n, of) = (clips.to_string(), TAKES.to_string());
            let mut text = Label::key_with(
                "app.settings.voice.train.progress",
                &[("n", &n), ("of", &of)],
            );
            if clips == 0 {
                text = k(&format!("app.settings.voice.train.{id}.sub"));
            }
            text
        };
        let row = Row::new(k(&format!("app.settings.voice.train.{id}"))).sublabel(sub);
        let row = cx.ui.add(card, row)?;
        let button = if done || clips > 0 {
            "app.settings.voice.train.again"
        } else {
            "app.settings.voice.train.button"
        };
        cx.act(row, Button::new(k(button)), Act::VoiceTrain(i))?;
        if done || clips > 0 {
            let row = Row::new(k(&format!("app.settings.voice.forget.{id}")));
            let row = cx.ui.add(card, row)?;
            let forget = Button::new(k("app.settings.voice.forget")).variant(ButtonVariant::Danger);
            cx.act(row, forget, Act::VoiceForget(i))?;
        }
    }
    Ok(())
}

fn test(cx: &mut Cx<'_>, card: NodeId, voice: &VoiceState) -> Result<(), UiError> {
    let sub = match voice.test {
        TestResult::None => "app.settings.voice.test.sub",
        TestResult::Waiting => "app.settings.voice.test.waiting",
        TestResult::Heard => "app.settings.voice.test.heard",
        TestResult::Missed => "app.settings.voice.test.missed",
    };
    let row = Row::new(k("app.settings.voice.test")).sublabel(k(sub));
    let row = cx.ui.add(card, row)?;
    cx.act(
        row,
        Button::new(k("app.settings.voice.test.button")),
        Act::VoiceTest,
    )?;
    Ok(())
}

/// Adds the section; nothing on a device without the voice service.
pub(crate) fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let Some(voice) = cx.state.status.voice.clone() else {
        return Ok(());
    };
    let (card, _) = section(
        cx.ui,
        content,
        Sizing::AUTO,
        k("app.settings.voice.title"),
        Some(k("app.settings.voice.sub")),
    )?;
    let enable = Row::new(k("app.settings.voice.enable")).sublabel(k(status_key(&voice)));
    let row = cx.ui.add(card, enable)?;
    cx.act(
        row,
        Switch::new(k("app.settings.voice.enable"), voice.enabled),
        Act::VoiceToggle,
    )?;
    if voice.verify_needed {
        let row = cx.ui.add(card, Row::new(k("app.settings.voice.verify")))?;
        cx.ui.add(
            row,
            Badge::new(k("app.settings.voice.verify.badge")).tone(Tone::Warning),
        )?;
    }
    language(cx, card, &voice)?;
    model(cx, card, &voice)?;
    training(cx, card, &voice)?;
    test(cx, card, &voice)?;
    let privacy =
        Row::new(k("app.settings.voice.privacy")).sublabel(k("app.settings.voice.privacy.sub"));
    cx.ui.add(card, privacy.last())?;
    Ok(())
}
