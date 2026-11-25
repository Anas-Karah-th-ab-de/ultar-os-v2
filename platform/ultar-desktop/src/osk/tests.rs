use super::*;

fn text(s: &str) -> Act {
    Act::Text(s.to_owned())
}

#[test]
fn it_shows_by_mode_field_and_hardware_keyboard() {
    let mut k = Keyboard::default();
    assert!(!k.visible(), "no field, no keyboard");
    k.ime_state(true, Purpose::Text.code());
    assert!(k.visible(), "automatic without a hardware keyboard");
    k.hardware = true;
    assert!(!k.visible(), "automatic with a hardware keyboard");
    assert!(k.setting(MODE, &SettingValue::Text("always".into())));
    assert!(k.visible());
    assert!(k.setting(MODE, &SettingValue::Text("never".into())));
    assert!(!k.visible());
    // The accessibility switch wins over every mode.
    assert!(k.setting(ACCESSIBILITY, &SettingValue::Flag(true)));
    assert!(k.visible());
    k.ime_state(false, 0);
    assert!(!k.visible(), "the field lost focus");
    // A wrong type changes nothing.
    assert!(!k.setting(SUGGESTIONS, &SettingValue::Integer(1)));
    assert!(k.settings.suggestions);
}

#[test]
fn keys_commit_text_and_send_backspace_and_enter() {
    let mut k = Keyboard::default();
    k.ime_state(true, 0);
    assert_eq!(k.press(&text("س")), vec![Out::Commit("س".into())]);
    assert_eq!(k.word, "س");
    assert_eq!(k.press(&Act::Backspace), vec![Out::Key(Special::Backspace)]);
    assert!(k.word.is_empty());
    assert_eq!(k.press(&Act::Action), vec![Out::Key(Special::Enter)]);
    // Shift once, then off after one letter; twice locks.
    k.press(&Act::Lang);
    assert_eq!(k.lang, Lang::En);
    k.press(&Act::Shift);
    assert_eq!(k.shift, Shift::Once);
    k.press(&text("H"));
    assert_eq!(k.shift, Shift::Off);
    k.press(&Act::Shift);
    k.press(&Act::Shift);
    assert_eq!(k.shift, Shift::Lock);
    k.press(&text("I"));
    assert_eq!(k.shift, Shift::Lock);
    // Pages and the globe key.
    k.press(&Act::ToSymbols);
    assert_eq!(k.page, Page::Symbols);
    k.press(&Act::ToLetters);
    assert_eq!(k.page, Page::Letters);
    k.press(&Act::Lang);
    assert_eq!(k.lang, Lang::Ar, "back to the first language");
}

#[test]
fn suggestions_replace_the_word_and_stay_out_of_passwords() {
    let mut k = Keyboard::default();
    k.ime_state(true, 0);
    k.press(&Act::Lang);
    for c in ["t", "h", "a"] {
        k.press(&text(c));
    }
    assert_eq!(k.suggestions(), vec!["that", "than", "thanks"]);
    let out = k.choose("thanks");
    assert_eq!(
        out,
        vec![
            Out::Key(Special::Backspace),
            Out::Key(Special::Backspace),
            Out::Key(Special::Backspace),
            Out::Commit("thanks ".into()),
        ]
    );
    assert!(k.word.is_empty());
    k.ime_state(true, Purpose::Password.code());
    k.press(&text("t"));
    assert!(
        k.suggestions().is_empty(),
        "no suggestions in a password field"
    );
    k.setting(SUGGESTIONS, &SettingValue::Flag(false));
    k.ime_state(true, 0);
    k.press(&text("t"));
    assert!(k.suggestions().is_empty(), "switched off");
}

#[test]
fn autocorrect_fixes_a_typo_on_space_only_when_on() {
    let mut k = Keyboard::default();
    k.ime_state(true, 0);
    k.press(&Act::Lang);
    for c in ["t", "e", "h"] {
        k.press(&text(c));
    }
    assert_eq!(
        k.press(&Act::Space),
        vec![Out::Commit(" ".into())],
        "off by default"
    );
    k.setting(AUTOCORRECT, &SettingValue::Flag(true));
    for c in ["t", "e", "h"] {
        k.press(&text(c));
    }
    let out = k.press(&Act::Space);
    assert_eq!(out.len(), 5);
    assert_eq!(out[3], Out::Commit("the".into()));
    assert_eq!(out[4], Out::Commit(" ".into()));
    // Not in an e-mail field.
    k.ime_state(true, Purpose::Email.code());
    for c in ["t", "e", "h"] {
        k.press(&text(c));
    }
    assert_eq!(k.press(&Act::Space), vec![Out::Commit(" ".into())]);
}

#[test]
fn the_language_list_comes_from_the_input_languages() {
    let list = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(languages(&list(&["de", "en-us"])), vec![Lang::De, Lang::En]);
    assert_eq!(languages(&list(&["fr"])), vec![Lang::Ar, Lang::En]);
    let mut k = Keyboard::default();
    k.setting(LANGUAGES, &SettingValue::List(list(&["de"])));
    assert_eq!(k.lang, Lang::De, "the shown language left the list");
}
