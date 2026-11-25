//! The helper's sheet (boards HelperSheet, HelperConfirm), driven by keys and clicks:
//! Super+H, typing a request, each step as it happens, a step that waits for the person,
//! stopping, and the way to what the helper may do.

mod common;

use common::*;
use ultar_bus::messages::agent::{Answer, Area, Brain, Progress, Step, StepOutcome};
use ultar_bus::{Text, TextArg};
use ultar_desktop::model::services::Call;
use ultar_desktop::model::Open;
use ultar_desktop::shell::Effect;
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_ui::event::Key;
use ultar_ui::tokens::ThemeKind;

const SUPER_H: u16 = 35;

fn calls(effects: &[Effect]) -> Vec<Call> {
    let service = |e: &Effect| match e {
        Effect::Service(call) => Some(call.clone()),
        _ => None,
    };
    effects.iter().filter_map(service).collect()
}

fn shown(h: &Harness) -> String {
    let view = &h.popup.as_ref().expect("the sheet is open").view;
    let mut out = String::new();
    let mut stack = vec![view.ui.root()];
    while let Some(n) = stack.pop() {
        if let Some(w) = view.ui.widget(n) {
            out.push_str(&format!("{w:?}\n"));
        }
        stack.extend(view.ui.children(n).iter().rev());
    }
    out
}

fn open(lang: Lang) -> Harness {
    let mut h = Harness::new(lang, ThemeKind::Dark);
    let out = h.shell.on_shell_key(SUPER_H);
    assert_eq!(
        out,
        ultar_desktop::shell::Outcome::none(),
        "no helper service"
    );
    h.shell.d.overview.sources.helper = true;
    h.shell.d.overview.helper.brain = Some(Brain::Local);
    let out = h.shell.on_shell_key(SUPER_H);
    h.apply(out);
    assert_eq!(h.shell.d.open, Open::Helper);
    h
}

fn step(tool: &str, area: Area, detail: &str, outcome: StepOutcome) -> Step {
    let mut step = Step::new(tool, outcome, area);
    step.detail = detail.to_owned();
    step
}

#[test]
fn a_request_is_typed_sent_and_shows_every_step_and_the_answer() {
    let mut h = open(Lang::Ar);
    let empty = shown(&h);
    assert!(empty.contains("desk.helper.empty") && empty.contains("desk.helper.brain.local"));
    assert!(h.popup_target(&Target::HelperSend) && !h.popup_target(&Target::HelperStop));

    h.type_popup("make it dark");
    assert_eq!(h.shell.d.overview.helper.query, "make it dark");
    h.key_popup(Key::Enter);
    assert_eq!(calls(&h.take()), [Call::AskHelper("make it dark".into())]);
    let busy = shown(&h);
    assert!(busy.contains("make it dark") && busy.contains("desk.helper.thinking"));
    assert!(h.popup_target(&Target::HelperStop) && !h.popup_target(&Target::HelperSend));
    assert!(
        h.shell.d.overview.helper.query.is_empty(),
        "the field is free again"
    );
    // while it runs, a second request is not sent
    h.type_popup("more");
    h.key_popup(Key::Enter);
    assert!(calls(&h.take()).is_empty());

    // a step arrives while the helper is still at it
    let done = step(
        "setting_set",
        Area::Settings,
        "appearance.dark=true",
        StepOutcome::Done,
    );
    assert!(h
        .shell
        .d
        .overview
        .helper
        .progress(Progress::Step(done.clone())));
    h.apply(ultar_desktop::shell::Outcome::surfaces());
    let live = shown(&h);
    for wanted in [
        "desk.helper.area.settings",
        "appearance.dark=true",
        "desk.helper.step.done",
    ] {
        assert!(live.contains(wanted), "{wanted}");
    }

    let answer = Answer {
        text: Text::key("agent.says").with(TextArg::Verbatim("It is dark now.".into())),
        brain: Brain::Local,
        steps: vec![done],
    };
    assert!(h.shell.d.overview.helper.answer("make it dark", answer));
    h.apply(ultar_desktop::shell::Outcome::surfaces());
    let text = shown(&h);
    assert!(text.contains("It is dark now.") && !text.contains("desk.helper.thinking"));
    assert!(h.popup_target(&Target::HelperSend));
}

#[test]
fn a_step_that_waits_for_the_person_says_so_and_waiting_can_be_stopped() {
    let mut h = open(Lang::En);
    h.type_popup("restart");
    h.click_popup(Target::HelperSend);
    assert_eq!(calls(&h.take()), [Call::AskHelper("restart".into())]);
    let refused = step(
        "setting_set",
        Area::Settings,
        "assistant.reach.power=true",
        StepOutcome::NotAllowed,
    );
    h.shell.d.overview.helper.progress(Progress::Step(refused));
    h.shell
        .d
        .overview
        .helper
        .progress(Progress::Waiting("restart".into()));
    h.apply(ultar_desktop::shell::Outcome::surfaces());
    let text = shown(&h);
    for wanted in [
        "desk.helper.step.refused",
        "desk.helper.waiting",
        "agent.action.restart",
    ] {
        assert!(text.contains(wanted), "{wanted}: {text}");
    }

    h.click_popup(Target::HelperStop);
    let text = shown(&h);
    assert!(text.contains("desk.helper.stopped") && !text.contains("desk.helper.waiting\""));
    assert!(
        text.contains("desk.helper.step.refused"),
        "what was done stays listed"
    );
    // the late answer is not shown as if it were waited for
    assert!(!h
        .shell
        .d
        .overview
        .helper
        .answer("restart", Answer::default()));

    h.click_popup(Target::HelperSettings);
    let effects = h.take();
    assert!(
        effects
            .iter()
            .any(|e| format!("{e:?}").contains("assistant")),
        "Settings opens on the assistant's page: {effects:?}"
    );
}

#[test]
fn asking_from_the_search_opens_the_sheet_and_the_quick_settings_have_its_button() {
    let mut h = Harness::new(Lang::En, ThemeKind::Dark);
    h.shell.d.overview.sources.helper = true;
    let out = h.shell.toggle(Open::Overview);
    h.apply(out);
    h.type_popup("mute the sound please");
    h.key_popup(Key::Enter);
    // nothing else matches: Enter asks the helper, and its sheet opens with the request
    assert_eq!(h.shell.d.open, Open::Helper);
    assert_eq!(h.shell.d.overview.helper.turns.len(), 1);
    assert!(calls(&h.take()).contains(&Call::AskHelper("mute the sound please".into())));

    let out = h
        .shell
        .toggle(Open::Quick(ultar_desktop::model::QuickPage::Tiles));
    h.apply(out);
    assert!(h.popup_target(&Target::Assistant));
    h.click_popup(Target::Assistant);
    assert_eq!(h.shell.d.open, Open::Helper);
    // while a request runs, another one from the search is answered as a notification
    let hit = ultar_desktop::model::search::Hit::Helper("and louder".into());
    let out = h.shell.on_popup(
        &Target::Result(hit),
        &ultar_ui::event::ActionKind::Activated,
    );
    h.apply(out);
    assert_eq!(h.shell.d.overview.helper.turns.len(), 1);
    assert!(calls(&h.take()).contains(&Call::AskHelper("and louder".into())));
}

#[test]
fn every_text_of_the_sheet_exists_in_all_three_languages() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let read =
        |lang: &str| std::fs::read_to_string(repo.join(format!("locales/{lang}.ftl"))).unwrap();
    let keys = |text: &str| -> Vec<String> {
        let lines = text.lines().filter_map(|l| l.split(" = ").next());
        let ours = lines.filter(|k| k.starts_with("desk.helper.") || *k == "desk.qs.assistant");
        ours.map(str::to_owned).collect()
    };
    let english = keys(&read("en"));
    assert!(english.len() > 30);
    assert_eq!(keys(&read("ar")), english);
    assert_eq!(keys(&read("de")), english);
    for area in Area::ALL {
        assert!(english.contains(&format!("desk.helper.area.{}", area.id())));
    }
}
