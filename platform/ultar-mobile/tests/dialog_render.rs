//! Every system dialog of the phone (`ultar-mobile-dialog`) as a sheet on a 360 × 760 phone,
//! in Arabic and English, dark and light (German builds too): nothing outside the screen, no
//! key untranslated, touch-size buttons over the sheet's width with the primary one above the
//! safe one, the safe answer focused, and the dimmed part above the sheet. With
//! `ULTAR_MOBILE_OUT` set, each one is written there as `dialog-<name>-<lang>-<theme>.png`.

use ultar_bus::messages::auth::UserInfo;
use ultar_bus::messages::bluetooth::{DeviceKind as BtKind, PairConfirm, PairQuestion};
use ultar_bus::messages::mesh::{DeviceKind, Stage};
use ultar_bus::messages::mesh_dialog::AskHandoff;
use ultar_bus::messages::settings::SettingValue;
use ultar_bus::messages::sysdialog::{AskPermission, Authenticate, Permission};
use ultar_dialog_core::{Dialogs, Question};
use ultar_mobile::dialog::{build, DialogBuilt, DialogClick};
use ultar_ui::geom::Size;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{size, InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Lang;

const PHONE: Size = Size::new(360.0, 760.0);

fn offer(app: &str) -> Question {
    Question::Handoff(AskHandoff {
        from_name: "ultar-pc".into(),
        from_kind: DeviceKind::Desktop,
        title: "notes.txt".into(),
        app: app.into(),
        size: Some(1_258_291),
        stage: if app.is_empty() {
            Stage::File
        } else {
            Stage::State
        },
    })
}

fn pairing(question: PairQuestion) -> Question {
    Question::BtPair(PairConfirm {
        device_name: "Keyboard K380".into(),
        address: "AA:BB:CC:DD:EE:FF".into(),
        kind: BtKind::Keyboard,
        question,
    })
}

/// Every question, with a name.
fn questions() -> Vec<(&'static str, Question)> {
    vec![
        ("handoff", offer("editor")),
        ("handoff-file", offer("")),
        (
            "permission",
            Question::Permission(AskPermission {
                app_id: "org.ultar.files".into(),
                app_name: "Files".into(),
                permission: Permission::Camera,
                reason: None,
            }),
        ),
        (
            "auth",
            Question::Authenticate(Authenticate {
                app_id: "org.ultar.settings".into(),
                app_name: "Settings".into(),
                action: ultar_bus::Text::key("mobile.power.restart"),
            }),
        ),
        ("bt-consent", pairing(PairQuestion::Consent)),
        ("bt-compare", pairing(PairQuestion::Compare(42))),
        ("bt-show", pairing(PairQuestion::Show(7))),
        (
            "keep",
            Question::Keep {
                key: "display.mode".into(),
                value: SettingValue::Text("mirror".into()),
            },
        ),
    ]
}

fn dialogs(question: Question, wrong: bool) -> Dialogs {
    let mut d = Dialogs::default();
    d.ask(1, question, 0);
    d.admin = Some(UserInfo {
        name: "anas".into(),
        display_name: "Anas".into(),
        has_pin: false,
        admin: true,
    });
    d.wrong_until = wrong.then_some(5_000);
    d
}

fn built(lang: Lang, theme: ThemeKind, d: &Dialogs) -> (Ui, DialogBuilt) {
    let tr = ultar_mobile::i18n::translator(lang).unwrap();
    let mut ui = Ui::new(PaintEnv::new(lang, theme), InputMode::Touch, PHONE, tr).unwrap();
    let built = build(&mut ui, d, 0).unwrap().expect("a question is shown");
    ui.layout();
    (ui, built)
}

fn dump(ui: &mut Ui, name: &str) {
    if let Ok(dir) = std::env::var("ULTAR_MOBILE_OUT") {
        let canvas = ultar_ui::snapshot::render(ui).unwrap();
        let png = ultar_ui::snapshot::Image::from_canvas(&canvas)
            .encode_png()
            .unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), png).unwrap();
    }
}

fn check(ui: &mut Ui, built: &DialogBuilt, name: &str, problems: &mut Vec<String>) {
    let tree = ui.a11y_tree();
    for node in &tree {
        let text = node.describe(ui.translator());
        if text.contains("mobile.") || text.contains("dialog.") {
            problems.push(format!("{name}: untranslated {text}"));
        }
        let r = node.rect;
        let inside = r.x >= -0.5 && r.y >= -0.5 && r.right() <= PHONE.w + 0.5;
        if !inside || r.y + r.h > PHONE.h + 0.5 {
            problems.push(format!("{name}: {text:?} outside the screen: {r:?}"));
        }
    }
    let mut last_y = 0.0;
    for (node, click) in &built.clicks {
        let r = ui.rect(*node).unwrap();
        if r.h < size::TARGET_TOUCH_MIN - 0.5 || r.w < PHONE.w - 2.0 * 16.0 - 4.0 {
            problems.push(format!(
                "{name}: {click:?} not a full-width touch button: {r:?}"
            ));
        }
        if r.y < last_y {
            problems.push(format!("{name}: {click:?} above an earlier button"));
        }
        last_y = r.y;
    }
    // The dimmed part: the upper half of the screen has no operable button.
    if built
        .clicks
        .iter()
        .any(|(n, _)| ui.rect(*n).unwrap().y < PHONE.h / 3.0)
    {
        problems.push(format!("{name}: a button in the dimmed part"));
    }
    if built.password.is_none() && ui.focused() != built.node(DialogClick::Safe) {
        problems.push(format!("{name}: the safe answer has no focus"));
    }
}

#[test]
fn every_question_in_both_languages_and_themes() {
    let mut problems = Vec::new();
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            for (name, question) in questions() {
                let variants: &[bool] = if name == "auth" {
                    &[false, true]
                } else {
                    &[false]
                };
                for &wrong in variants {
                    let d = dialogs(question.clone(), wrong);
                    let (mut ui, built) = built(lang, theme, &d);
                    let tag = if wrong { "-wrong" } else { "" };
                    let name = format!("dialog-{name}{tag}-{lang:?}-{theme:?}").to_lowercase();
                    dump(&mut ui, &name);
                    check(&mut ui, &built, &name, &mut problems);
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn german_builds_too() {
    let mut problems = Vec::new();
    for (name, question) in questions() {
        let (mut ui, built) = built(Lang::De, ThemeKind::Dark, &dialogs(question, false));
        check(&mut ui, &built, &format!("{name}-de"), &mut problems);
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_offer_has_accept_above_decline_and_long_titles_are_cut() {
    let mut long = offer("editor");
    if let Question::Handoff(p) = &mut long {
        p.title = "x".repeat(500);
    }
    let (mut ui, built) = built(Lang::En, ThemeKind::Dark, &dialogs(long, false));
    let accept = ui.rect(built.node(DialogClick::Accept).unwrap()).unwrap();
    let decline = ui.rect(built.node(DialogClick::Safe).unwrap()).unwrap();
    assert!(accept.y + accept.h <= decline.y, "Accept above Decline");
    let shown: Vec<String> = ui
        .a11y_tree()
        .iter()
        .map(|n| n.describe(ui.translator()))
        .collect();
    assert!(shown.iter().any(|t| t.contains(&"x".repeat(96))));
    assert!(
        !shown.iter().any(|t| t.contains(&"x".repeat(97))),
        "at most 96 characters"
    );
    assert!(shown
        .iter()
        .any(|t| t.contains("Text Editor · 1.2 MB · at the same place")
            || t.contains("· 1.2 MB · at the same place")));
}

#[test]
fn nothing_asked_nothing_built() {
    let tr = ultar_mobile::i18n::translator(Lang::En).unwrap();
    let env = PaintEnv::new(Lang::En, ThemeKind::Dark);
    let mut ui = Ui::new(env, InputMode::Touch, PHONE, tr).unwrap();
    assert!(build(&mut ui, &Dialogs::default(), 0).unwrap().is_none());
}
