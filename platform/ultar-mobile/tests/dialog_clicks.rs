//! The phone's system dialogs driven by taps on the built sheet: Accept and Decline answer
//! the offer of another device, the other buttons answer their questions, a question nobody
//! answers gets the safe answer – and a tap on the dimmed part answers nothing.

use ultar_bus::messages::mesh::{DeviceKind, Stage};
use ultar_bus::messages::mesh_dialog::AskHandoff;
use ultar_bus::messages::settings::SettingValue;
use ultar_bus::messages::sysdialog::{AskPermission, Decision, Permission, ANSWER_WITHIN_MS};
use ultar_dialog_core::{Answer, DialogEffect, Dialogs, Question};
use ultar_mobile::dialog::{build, choose, Choice, DialogClick};
use ultar_ui::event::{ActionKind, Event, PointerButton};
use ultar_ui::geom::{Point, Size};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Lang;

fn offer() -> Question {
    Question::Handoff(AskHandoff {
        from_name: "ultar-pc".into(),
        from_kind: DeviceKind::Desktop,
        title: "notes.txt".into(),
        app: "editor".into(),
        size: Some(1_200_000),
        stage: Stage::State,
    })
}

fn permission() -> Question {
    Question::Permission(AskPermission {
        app_id: "org.ultar.files".into(),
        app_name: "Files".into(),
        permission: Permission::Camera,
        reason: None,
    })
}

/// Taps at `pos` on the built sheet of `d` and runs what the tap means.
fn tap_at(
    d: &mut Dialogs,
    pos: impl Fn(&Ui, &ultar_mobile::dialog::DialogBuilt) -> Point,
) -> Vec<DialogEffect> {
    let tr = ultar_mobile::i18n::translator(Lang::En).unwrap();
    let env = PaintEnv::new(Lang::En, ThemeKind::Dark);
    let mut ui = Ui::new(env, InputMode::Touch, Size::new(360.0, 760.0), tr).unwrap();
    let built = build(&mut ui, d, 0).unwrap().unwrap();
    ui.layout();
    let pos = pos(&ui, &built);
    let button = PointerButton::Primary;
    let mut actions = ui.handle(Event::PointerMove { pos }).actions;
    actions.extend(ui.handle(Event::PointerDown { pos, button }).actions);
    actions.extend(ui.handle(Event::PointerUp { pos, button }).actions);
    let mut effects = Vec::new();
    for a in actions.iter().filter(|a| a.kind == ActionKind::Activated) {
        if let Some(click) = built.click(a.node) {
            match choose(d, click, 0) {
                Choice::Effects(e) => effects.extend(e),
                Choice::Submit => panic!("no password asked"),
            }
        }
    }
    effects
}

fn tap(d: &mut Dialogs, click: DialogClick) -> Vec<DialogEffect> {
    tap_at(d, |ui, built| {
        let node = built.node(click).unwrap_or_else(|| panic!("no {click:?}"));
        ui.rect(node).unwrap().center()
    })
}

#[test]
fn accept_and_decline_answer_the_offer() {
    let mut d = Dialogs::default();
    d.ask(1, offer(), 0);
    assert_eq!(
        tap(&mut d, DialogClick::Accept),
        vec![DialogEffect::Reply(1, Answer::Handoff(true))]
    );
    d.ask(2, offer(), 0);
    assert_eq!(
        tap(&mut d, DialogClick::Safe),
        vec![DialogEffect::Reply(2, Answer::Handoff(false))]
    );
    assert!(d.current().is_none());
}

#[test]
fn an_unanswered_offer_is_declined_and_the_dimmed_part_answers_nothing() {
    let mut d = Dialogs::default();
    d.ask(3, offer(), 0);
    let effects = tap_at(&mut d, |_, _| Point::new(180.0, 100.0));
    assert!(effects.is_empty(), "a tap above the sheet: {effects:?}");
    assert_eq!(d.current().map(|e| e.id), Some(3));
    assert!(d.tick(ANSWER_WITHIN_MS - 1).is_empty());
    assert_eq!(
        d.tick(ANSWER_WITHIN_MS),
        vec![DialogEffect::Reply(3, Answer::Handoff(false))],
        "nobody answered: nothing is taken"
    );
}

#[test]
fn the_permission_buttons_and_its_safe_answer() {
    let mut d = Dialogs::default();
    for (id, click, decision) in [
        (1, DialogClick::AllowAlways, Decision::AllowAlways),
        (2, DialogClick::AllowOnce, Decision::AllowOnce),
        (3, DialogClick::Safe, Decision::Deny),
    ] {
        d.ask(id, permission(), 0);
        assert_eq!(
            tap(&mut d, click),
            vec![DialogEffect::Reply(id, Answer::Permission(decision))]
        );
    }
    d.ask(4, permission(), 0);
    assert_eq!(
        d.tick(ANSWER_WITHIN_MS),
        vec![DialogEffect::Reply(4, Answer::Permission(Decision::Deny))]
    );
}

#[test]
fn keep_more_time_and_revert() {
    let mut d = Dialogs::default();
    let keep = Question::Keep {
        key: "display.mode".into(),
        value: SettingValue::Text("mirror".into()),
    };
    d.ask(1, keep.clone(), 0);
    assert_eq!(
        tap(&mut d, DialogClick::MoreTime),
        vec![DialogEffect::Again(
            "display.mode".into(),
            SettingValue::Text("mirror".into())
        )]
    );
    assert_eq!(
        tap(&mut d, DialogClick::Keep),
        vec![DialogEffect::Confirm("display.mode".into())]
    );
    d.ask(2, keep, 0);
    assert_eq!(
        tap(&mut d, DialogClick::Safe),
        vec![DialogEffect::Revert("display.mode".into())]
    );
}

#[test]
fn a_button_of_another_question_does_nothing() {
    let mut d = Dialogs::default();
    d.ask(1, offer(), 0);
    assert_eq!(
        choose(&mut d, DialogClick::Keep, 0),
        Choice::Effects(Vec::new())
    );
    assert_eq!(
        choose(&mut d, DialogClick::AllowAlways, 0),
        Choice::Effects(Vec::new())
    );
    assert_eq!(
        choose(&mut d, DialogClick::Authenticate, 0),
        Choice::Effects(Vec::new())
    );
    assert_eq!(d.current().map(|e| e.id), Some(1));
}
