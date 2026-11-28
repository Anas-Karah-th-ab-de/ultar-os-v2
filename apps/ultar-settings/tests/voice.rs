//! Settings > Assistant > voice control, driven by clicks: off until switched on, language and
//! model, the training of the wake word, the secret word and the person's voice, the test,
//! and what the page says when something is missing. Arabic first.

mod common;

use common::*;
use ultar_bus::messages::voice::{ModelInfo, Ready, VoiceError, VoiceState};
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::service::fake::Shared;
use ultar_settings::App;
use ultar_ui::tokens::ThemeKind;

fn voice() -> VoiceState {
    let model = |name: &str, size_mb, installed| ModelInfo {
        name: name.into(),
        size_mb,
        installed,
    };
    VoiceState {
        wake_word: "Hey KM".into(),
        language: "ar".into(),
        models: vec![model("tiny", 75, false), model("small", 466, true)],
        model: "small".into(),
        ready: Ready {
            stt: true,
            tts: true,
            mic: true,
        },
        ..VoiceState::default()
    }
}

fn open(lang: Lang, state: Option<VoiceState>) -> (Shared, App) {
    let world = board_world(1, true);
    world.borrow_mut().system.voice = state;
    let app = app(
        &world,
        lang,
        ThemeKind::Dark,
        content_size(1440.0, 900.0),
        "assistant",
    );
    (world, app)
}

fn shown(app: &App) -> String {
    let mut out = String::new();
    let mut stack = vec![app.ui.root()];
    stack.extend(app.view.dialog);
    while let Some(n) = stack.pop() {
        if let Some(w) = app.ui.widget(n) {
            out.push_str(&format!("{w:?}\n"));
        }
        stack.extend(app.ui.children(n).iter().rev());
    }
    out
}

fn press(app: &mut App, act: Act) {
    let node = app
        .view
        .node(act)
        .unwrap_or_else(|| panic!("{act:?} not shown"));
    click(app, node);
}

fn last(world: &Shared) -> String {
    log(world).last().cloned().unwrap_or_default()
}

fn choose(app: &mut App, act: Act, index: usize) {
    let node = app
        .view
        .node(act)
        .unwrap_or_else(|| panic!("{act:?} not shown"));
    let kind = ultar_ui::event::ActionKind::Selected(index);
    app.actions(vec![ultar_ui::event::Action { node, kind }])
        .unwrap();
}

fn state(world: &Shared) -> VoiceState {
    world.borrow().system.voice.clone().unwrap()
}

#[test]
fn a_device_without_the_voice_service_shows_no_such_section() {
    let (_, app) = open(Lang::Ar, None);
    assert!(app.view.node(Act::VoiceToggle).is_none());
    assert!(!shown(&app).contains("app.settings.voice."));
}

#[test]
fn it_is_off_until_switched_on_and_says_what_is_missing() {
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        let (world, mut app) = open(lang, Some(voice()));
        let text = shown(&app);
        assert!(text.contains("app.settings.voice.title"));
        assert!(text.contains("app.settings.voice.off"), "off at first");
        assert!(text.contains("app.settings.voice.privacy.sub"));
        assert!(!state(&world).enabled);
        press(&mut app, Act::VoiceToggle);
        assert_eq!(last(&world), "voice.set_enabled true");
        assert!(state(&world).enabled);
        assert!(shown(&app).contains("app.settings.voice.status.listening"));
        press(&mut app, Act::VoiceToggle);
        assert_eq!(last(&world), "voice.set_enabled false");
        assert!(shown(&app).contains("app.settings.voice.off"));
    }
    // the service's own reasons are shown in words
    let mut missing = voice();
    missing.enabled = true;
    missing.error = Some(VoiceError::NoStt);
    let (_, app) = open(Lang::Ar, Some(missing));
    assert!(shown(&app).contains("app.settings.voice.error.no_stt"));
}

#[test]
fn language_and_model_are_chosen_and_only_installed_models_are_offered() {
    let (world, mut app) = open(Lang::Ar, Some(voice()));
    choose(&mut app, Act::VoiceLanguage, 1);
    assert_eq!(last(&world), "voice.set_language en");
    assert_eq!(state(&world).language, "en");
    choose(&mut app, Act::VoiceLanguage, 0);
    assert_eq!(state(&world).language, "ar");
    // "tiny" is not installed: the only choice is "small"
    choose(&mut app, Act::VoiceModel, 0);
    assert_eq!(last(&world), "voice.set_model small");
    // no model installed at all: said, nothing to choose
    let mut none = voice();
    none.models = vec![ModelInfo {
        name: "tiny".into(),
        size_mb: 75,
        installed: false,
    }];
    none.model = String::new();
    let (_, app) = open(Lang::En, Some(none));
    assert!(app.view.node(Act::VoiceModel).is_none());
    assert!(shown(&app).contains("app.settings.voice.model.none"));
}

#[test]
fn the_words_and_the_voice_are_trained_in_three_takes_and_can_be_forgotten() {
    let (world, mut app) = open(Lang::Ar, Some(voice()));
    assert!(
        app.view.node(Act::VoiceForget(0)).is_none(),
        "nothing to forget yet"
    );
    for take in 1..=3u8 {
        press(&mut app, Act::VoiceTrain(0));
        assert_eq!(last(&world), "voice.enroll Wake");
        assert_eq!(state(&world).wake.clips, take);
    }
    assert!(state(&world).wake.enrolled);
    assert!(shown(&app).contains("app.settings.voice.train.done"));
    press(&mut app, Act::VoiceTrain(2));
    assert_eq!(state(&world).voice.clips, 1);
    assert!(!state(&world).voice.enrolled);
    assert!(shown(&app).contains("app.settings.voice.train.progress"));
    press(&mut app, Act::VoiceForget(0));
    assert_eq!(last(&world), "voice.forget Wake");
    assert!(!state(&world).wake.enrolled);
    assert!(app.view.node(Act::VoiceForget(0)).is_none());
    press(&mut app, Act::VoiceForget(2));
    assert_eq!(state(&world).voice.clips, 0);
}

#[test]
fn the_test_needs_a_trained_word_and_then_hears_it() {
    let (world, mut app) = open(Lang::En, Some(voice()));
    let toasts = app.toasts_shown();
    press(&mut app, Act::VoiceTest);
    assert_eq!(
        app.toasts_shown(),
        toasts + 1,
        "an untrained word is said, not silently ignored"
    );
    assert!(!log(&world).iter().any(|l| l == "voice.test"));
    for _ in 0..3 {
        press(&mut app, Act::VoiceTrain(0));
    }
    press(&mut app, Act::VoiceTest);
    assert_eq!(last(&world), "voice.test");
    assert!(shown(&app).contains("app.settings.voice.test.heard"));
}

#[test]
fn a_refused_voice_asks_for_the_fingerprint_or_face() {
    let mut refused = voice();
    refused.enabled = true;
    refused.verify_needed = true;
    let (_, app) = open(Lang::Ar, Some(refused));
    assert!(shown(&app).contains("app.settings.voice.verify.badge"));
}

#[test]
fn every_text_of_the_section_exists_in_all_three_languages() {
    let read = |lang: &str| {
        let file = format!("apps/ultar-settings/locales/{lang}.ftl");
        std::fs::read_to_string(repo().join(file)).unwrap()
    };
    let keys = |text: &str| -> Vec<String> {
        let lines = text.lines().filter_map(|l| l.split(" = ").next());
        lines
            .filter(|k| k.starts_with("app.settings.voice."))
            .map(str::to_owned)
            .collect()
    };
    let english = keys(&read("en"));
    assert!(english.len() > 40, "{}", english.len());
    assert_eq!(keys(&read("ar")), english);
    assert_eq!(keys(&read("de")), english);
    // every key the page uses is in the files
    let source =
        std::fs::read_to_string(repo().join("apps/ultar-settings/src/view/voice.rs")).unwrap();
    let have = |key: &str| english.iter().any(|k| k == key);
    for key in [
        "title",
        "sub",
        "enable",
        "off",
        "language",
        "model",
        "model.none",
        "train.done",
        "train.progress",
        "test.heard",
        "verify.badge",
        "privacy.sub",
        "error.no_stt",
    ] {
        assert!(have(&format!("app.settings.voice.{key}")), "{key}");
        assert!(
            source.contains(&format!("app.settings.voice.{key}")),
            "{key} is not used by the page"
        );
    }
}
