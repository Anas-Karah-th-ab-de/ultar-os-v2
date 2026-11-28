//! Settings › Assistant, the helper's part, driven by clicks: the switches of what it
//! reaches are settings of the panel, and what it did is listed and forgotten on request
//! (docs/integration/ultar-agentd.md).

mod common;

use common::*;
use ultar_bus::messages::agent::{Activity, Area, Brain, Entry, Step, StepOutcome};
use ultar_bus::Text;
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::view::helper::{ago, verdict};
use ultar_settings::App;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::widgets::Tone;

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
    let node = app.view.node(act);
    click(app, node.unwrap_or_else(|| panic!("{act:?} not shown")));
}

fn entry(request: &str, outcome: StepOutcome) -> Entry {
    let mut step = Step::new("setting_set", outcome, Area::Settings);
    step.detail = "key=appearance.dark, value=true".to_owned();
    Entry {
        at: 0,
        request: request.to_owned(),
        by_voice: false,
        brain: Brain::Local,
        steps: vec![step],
        said: Text::default(),
    }
}

#[test]
fn what_the_helper_did_is_listed_with_its_outcome_and_forgotten_after_asking() {
    let world = board_world(1, true);
    let size = content_size(1440.0, 900.0);
    // a device without the helper shows no such section
    let plain = app(&world, Lang::En, ThemeKind::Dark, size, "assistant");
    assert!(!shown(&plain).contains("app.settings.helper.did"));

    world.borrow_mut().system.activity = Some(Activity::default());
    let empty = app(&world, Lang::Ar, ThemeKind::Dark, size, "assistant");
    assert!(shown(&empty).contains("app.settings.helper.none"));
    assert!(
        empty.view.node(Act::HelperForget).is_none(),
        "nothing to forget"
    );

    let entries = vec![
        entry("make it dark", StepOutcome::Done),
        entry("and again", StepOutcome::NotAllowed),
    ];
    world.borrow_mut().system.activity = Some(Activity { entries });
    let mut app = app(&world, Lang::Ar, ThemeKind::Dark, size, "assistant");
    let text = shown(&app);
    for wanted in [
        "make it dark",
        "and again",
        "appearance.dark",
        "app.settings.helper.done",
        "app.settings.helper.refused",
        "app.settings.helper.ago.days",
    ] {
        assert!(text.contains(wanted), "{wanted}: {text}");
    }

    press(&mut app, Act::HelperForget);
    assert!(log(&world).iter().all(|l| l != "agent.clear_activity"));
    press(&mut app, Act::DialogOk);
    assert_eq!(log(&world).last().unwrap(), "agent.clear_activity");
    assert!(shown(&app).contains("app.settings.helper.none"));
}

#[test]
fn a_request_is_judged_by_its_worst_step() {
    let mut e = entry("x", StepOutcome::Done);
    assert_eq!(verdict(&e).map(|v| v.1), Some(Tone::Success));
    e.steps
        .push(Step::new("wifi", StepOutcome::NotConfirmed, Area::Network));
    assert_eq!(verdict(&e).map(|v| v.1), Some(Tone::Warning));
    e.steps
        .push(Step::new("wifi", StepOutcome::NotAllowed, Area::Network));
    assert_eq!(verdict(&e).map(|v| v.1), Some(Tone::Danger));
    e.steps.clear();
    assert_eq!(verdict(&e), None, "it only answered");
    assert_eq!(ago(0, 59 * 60), ("app.settings.helper.ago.minutes", 59));
    assert_eq!(ago(0, 5 * 3600), ("app.settings.helper.ago.hours", 5));
    assert_eq!(ago(0, 3 * 86400), ("app.settings.helper.ago.days", 3));
}

#[test]
fn every_text_of_the_section_exists_in_all_three_languages() {
    let read = |lang: &str| {
        let file = format!("apps/ultar-settings/locales/{lang}.ftl");
        std::fs::read_to_string(repo().join(file)).unwrap()
    };
    let keys = |text: &str| -> Vec<String> {
        let lines = text.lines().filter_map(|l| l.split(" = ").next());
        let ours = lines.filter(|k| k.starts_with("app.settings.helper."));
        ours.map(str::to_owned).collect()
    };
    let english = keys(&read("en"));
    assert!(english.len() > 20);
    assert_eq!(keys(&read("ar")), english);
    assert_eq!(keys(&read("de")), english);
    for area in Area::ALL {
        let key = format!("app.settings.helper.area.{}", area.id());
        assert!(english.contains(&key), "{key}");
    }
}
