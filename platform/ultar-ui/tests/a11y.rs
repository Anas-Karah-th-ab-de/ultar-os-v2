//! Accessibility nodes: role, name key, value and state of every widget; spoken sentences;
//! completeness of the toolkit's and the demo catalogs.

mod common;

use common::{click, display_settings, key};
use ultar_i18n::Translator;
use ultar_ui::event::Key;
use ultar_ui::icon::IconId;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{Role, UiError};
use ultar_ui::widgets::{Button, Icon, IconButton, Label, TextField};
use ultar_ui::{Lang, Size};

const SURFACE: Size = Size::new(720.0, 760.0);

#[test]
fn every_widget_has_a_role_a_name_key_and_a_state() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let s = display_settings(&mut ui).unwrap();
    let tree = ui.a11y_tree();
    let roles: Vec<Role> = tree.iter().map(|n| n.role).collect();
    let expected = [
        Role::Group,
        Role::Text,
        Role::Row,
        Role::Switch,
        Role::Select,
        Role::Slider,
        Role::TextField,
        Role::Button,
    ];
    for role in expected {
        assert!(roles.contains(&role), "{role:?} missing");
    }
    let focusable = ui.focus_order();
    for node in &tree {
        if node.role != Role::Group {
            let name = node
                .name
                .as_ref()
                .unwrap_or_else(|| panic!("{:?} has no name", node.id));
            let is_key = name.key_name().is_some_and(|k| k.starts_with("demo."));
            assert!(is_key, "{name:?} is not a catalog key");
        }
        assert!(!node.rect.is_empty());
        assert_eq!(node.focusable, focusable.contains(&node.id));
    }
    // Reading order is the logical order: the title first, the main action last.
    assert_eq!(
        tree.first().unwrap().name,
        Some(Label::key("demo.display.title"))
    );
    assert_eq!(tree.last().unwrap().id, s.apply);
    assert!(
        ui.a11y_node(ui.root()).is_none(),
        "layout containers are not announced"
    );
}

#[test]
fn values_and_checked_states_are_exposed() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let s = display_settings(&mut ui).unwrap();
    ui.layout();
    let switch = ui.a11y_node(s.primary).unwrap();
    assert_eq!(switch.role, Role::Switch);
    assert_eq!(
        (switch.state.checked, switch.state.expanded),
        (Some(true), None)
    );
    let select = ui.a11y_node(s.scale).unwrap();
    assert_eq!(select.role, Role::Select);
    assert_eq!(
        (select.state.expanded, select.state.checked),
        (Some(false), None)
    );
    let chosen = Label::key_with("demo.display.recommended", &[("value", "200%")]);
    assert_eq!(select.value, Some(chosen));
    let slider = ui.a11y_node(s.brightness).unwrap();
    assert_eq!(slider.value, Some(Label::data("70")));
}

#[test]
fn states_follow_interaction() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, SURFACE);
    let s = display_settings(&mut ui).unwrap();
    click(&mut ui, s.primary);
    assert_eq!(ui.a11y_node(s.primary).unwrap().state.checked, Some(false));
    click(&mut ui, s.orientation);
    let open = ui.a11y_node(s.orientation).unwrap();
    assert_eq!(open.state.expanded, Some(true));
    assert!(open.state.pressed && open.state.focused);
    key(&mut ui, Key::Escape);
    ui.set_disabled(s.revert, true).unwrap();
    let disabled = ui.a11y_node(s.revert).unwrap();
    assert!(disabled.state.disabled && !disabled.focusable);
    ui.set_loading(s.apply, true).unwrap();
    assert!(ui.a11y_node(s.apply).unwrap().state.busy);
    ui.set_error(s.name, Some(Label::key("demo.device.error")))
        .unwrap();
    let invalid = ui.a11y_node(s.name).unwrap();
    assert!(invalid.state.invalid);
    assert_eq!(invalid.error, Some(Label::key("demo.device.error")));
    assert_eq!(ui.error(s.name), Some(&Label::key("demo.device.error")));
    ui.set_error(s.name, None).unwrap();
    assert!(!ui.a11y_node(s.name).unwrap().state.invalid);
    // States that a widget does not have are refused.
    assert_eq!(
        ui.set_loading(s.card, true),
        Err(UiError::NotApplicable(s.card))
    );
    assert_eq!(
        ui.set_error(s.apply, Some(Label::key("demo.device.error"))),
        Err(UiError::NotApplicable(s.apply))
    );
    assert!(matches!(
        ui.set_on(s.apply, true),
        Err(UiError::WrongWidget {
            expected: "switch",
            ..
        })
    ));
    assert!(matches!(
        ui.set_value(s.apply, 1.0),
        Err(UiError::WrongWidget {
            expected: "slider",
            ..
        })
    ));
    assert!(ui.set_label(s.apply, Label::key("demo.save")).is_ok());
    assert!(ui.set_label(s.primary, Label::key("demo.save")).is_err());
    ui.set_on(s.night, true).unwrap();
    ui.set_value(s.brightness, 33.0).unwrap();
    assert!(ui.is_on(s.night).unwrap());
    assert_eq!(ui.value(s.brightness).unwrap(), 35.0, "snapped to the step");
}

#[test]
fn spoken_sentences_in_all_three_languages() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let s = display_settings(&mut ui).unwrap();
    ui.set_error(s.name, Some(Label::key("demo.device.error")))
        .unwrap();
    ui.set_loading(s.apply, true).unwrap();
    ui.set_disabled(s.revert, true).unwrap();
    let say = |ui: &ultar_ui::ui::Ui, id| ui.a11y_node(id).unwrap().describe(ui.translator());
    assert_eq!(say(&ui, s.primary), "الشاشة الرئيسية، مفتاح تبديل، مفعّل");
    assert_eq!(say(&ui, s.orientation), "الاتجاه، قائمة اختيار، أفقي، مغلقة");
    assert_eq!(
        say(&ui, s.brightness),
        "السطوع، شريط تمرير، \u{2068}70\u{2069}"
    );
    assert_eq!(
        say(&ui, s.name),
        "اسم الجهاز، حقل نص، قيمة غير صالحة، الاسم مطلوب"
    );
    assert_eq!(say(&ui, s.apply), "تطبيق، زر، جارٍ التحميل");
    assert_eq!(say(&ui, s.revert), "تراجع، زر، غير متاح");
    ui.set_lang(Lang::En);
    assert_eq!(say(&ui, s.primary), "Main display, switch, on");
    assert_eq!(say(&ui, s.revert), "Revert, button, unavailable");
    ui.set_lang(Lang::De);
    assert_eq!(say(&ui, s.primary), "Hauptbildschirm, Schalter, ein");
    assert_eq!(
        say(&ui, s.orientation),
        "Ausrichtung, Auswahlliste, Querformat, geschlossen"
    );
}

#[test]
fn icons_are_announced_only_when_named() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, SURFACE);
    let root = ui.root();
    let decorative = ui.add(root, Icon::new(IconId::Gear)).unwrap();
    let named = ui
        .add(
            root,
            Icon::new(IconId::Shield)
                .small()
                .name(Label::key("demo.settings")),
        )
        .unwrap();
    let button = ui
        .add(
            root,
            IconButton::new(IconId::Close, Label::key("demo.delete")),
        )
        .unwrap();
    let plain = ui
        .add(
            root,
            Button::new(Label::key("demo.save")).icon(IconId::Check),
        )
        .unwrap();
    let field = ui
        .add(root, TextField::new(Label::key("demo.device")).text("abc"))
        .unwrap();
    ui.layout();
    assert!(ui.a11y_node(decorative).is_none());
    assert_eq!(ui.a11y_node(named).unwrap().role, Role::Image);
    assert_eq!(
        ui.a11y_node(button).unwrap().name,
        Some(Label::key("demo.delete"))
    );
    assert_eq!(
        ui.a11y_node(plain).unwrap().describe(ui.translator()),
        "Save, button"
    );
    assert_eq!(
        ui.a11y_node(field).unwrap().describe(ui.translator()),
        "Device name, text field, \u{2068}abc\u{2069}"
    );
    assert_eq!(ui.a11y_tree().len(), 4);
}

fn keys(source: &str) -> Vec<&str> {
    source
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with(' ') && l.contains('='))
        .filter_map(|l| l.split('=').next())
        .map(str::trim)
        .collect()
}

#[test]
fn catalogs_are_complete_in_arabic_english_and_german() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for dir in ["locales", "tests/locales"] {
        let read = |lang: &str| {
            std::fs::read_to_string(root.join(dir).join(format!("{lang}.ftl"))).unwrap()
        };
        let (ar, en, de) = (read("ar"), read("en"), read("de"));
        assert!(!keys(&ar).is_empty());
        assert_eq!(keys(&ar), keys(&en), "{dir}: en differs from ar");
        assert_eq!(keys(&ar), keys(&de), "{dir}: de differs from ar");
        for (lang, source) in [(Lang::Ar, &ar), (Lang::En, &en), (Lang::De, &de)] {
            let mut tr = Translator::empty(lang);
            tr.add_source(lang, source).unwrap();
            for key in keys(source) {
                assert!(tr.has(key), "{dir} {lang:?} {key}");
            }
        }
        let arabic_letters = ar
            .chars()
            .filter(|c| ('\u{0600}'..='\u{06FF}').contains(c))
            .count();
        assert!(arabic_letters > 100, "{dir}/ar.ftl is written in Arabic");
    }
    // Every role and state key the toolkit can speak exists.
    let ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let roles = Role::ALL;
    for role in roles {
        for lang in [Lang::Ar, Lang::En, Lang::De] {
            assert!(
                ui.translator().catalog(lang).contains(role.name_key()),
                "{role:?} {lang:?}"
            );
        }
    }
    let spoken = ["ui.state.selected", "ui.a11y.joiner"];
    let tones = ultar_ui::widgets::Tone::ALL;
    let tone_keys = tones.iter().filter_map(|tone| tone.name_key());
    for key in spoken.into_iter().chain(tone_keys) {
        for lang in [Lang::Ar, Lang::En, Lang::De] {
            assert!(
                ui.translator().catalog(lang).contains(key),
                "{key} {lang:?}"
            );
        }
    }
    assert_eq!(roles.len(), 35);
}
