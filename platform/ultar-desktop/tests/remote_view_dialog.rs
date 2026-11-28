//! The system dialog of the remote view (`sysdialog.handoff` with the stage `RemoteView`):
//! its own words in Arabic, English and German ("<device> wants to see and control this
//! screen", Allow / Deny), its own buttons, and the answer of the person.

use ultar_bus::messages::mesh::{DeviceKind, Stage};
use ultar_bus::messages::mesh_dialog::AskHandoff;
use ultar_desktop::dialog::{self, Answer, DialogEffect, Dialogs, Question};
use ultar_desktop::view::Look;
use ultar_i18n::{Arg, Lang};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::widgets::Label;

fn question() -> Question {
    Question::Handoff(AskHandoff {
        from_name: "Laptop".into(),
        from_kind: DeviceKind::Desktop,
        title: String::new(),
        app: String::new(),
        size: None,
        stage: Stage::RemoteView,
    })
}

#[test]
fn the_dialog_names_the_device_and_has_allow_and_deny_in_every_language() {
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        let tr = ultar_desktop::i18n::translator(lang).unwrap();
        let mut d = Dialogs::default();
        d.ask(1, question(), 0);
        let look = Look::new(lang, ThemeKind::Dark);
        let mut v = dialog::view::build(&d, 0, look, &tr).unwrap().unwrap();
        let names: Vec<Label> =
            v.ui.a11y_tree()
                .into_iter()
                .filter(|n| n.focusable)
                .filter_map(|n| n.name)
                .collect();
        assert!(
            names.contains(&Label::key("dialog.view.accept")),
            "{lang:?}"
        );
        assert!(
            names.contains(&Label::key("dialog.view.decline")),
            "{lang:?}"
        );
        assert!(
            !names.contains(&Label::key("dialog.handoff.accept")),
            "{lang:?}"
        );
        let title = tr.t_args("dialog.view.title", &[("name", Arg::Str("Laptop"))]);
        assert!(
            title.contains("Laptop") && !title.contains("dialog.view"),
            "{lang:?}: {title}"
        );
    }
    let tr = ultar_desktop::i18n::translator(Lang::De).unwrap();
    let de = tr.t_args("dialog.view.title", &[("name", Arg::Str("Laptop"))]);
    assert_eq!(de, "Laptop möchte diesen Bildschirm sehen und steuern");
    let tr = ultar_desktop::i18n::translator(Lang::En).unwrap();
    assert_eq!(tr.t("dialog.view.accept"), "Allow");
    assert_eq!(tr.t("dialog.view.decline"), "Deny");
}

#[test]
fn only_a_yes_of_the_person_allows_it() {
    let mut d = Dialogs::default();
    d.ask(1, question(), 0);
    assert_eq!(
        d.answer(Answer::Handoff(true)),
        vec![DialogEffect::Reply(1, Answer::Handoff(true))]
    );
    d.ask(2, question(), 0);
    assert_eq!(
        d.dismiss(),
        vec![DialogEffect::Reply(2, Answer::Handoff(false))]
    );
}
