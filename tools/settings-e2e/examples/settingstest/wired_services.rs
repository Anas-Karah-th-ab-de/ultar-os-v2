//! The keyboard page's layout settings and the assistant page: the switch combination and
//! the per-window languages reach the compositor through the display service, the input
//! languages become the shell's layout list, the model size chooses among the installed
//! models, and the helper's seeing-the-screen and link-warning follow their switches. Each
//! check uses the code of the service that follows the setting (as the other checks do).

use super::super::setup::Run;
use super::{change, choose, edit, page, Display, Follower};
use ultar_agentd::model::Thinker;
use ultar_agentd::tools::{Call, Limits, Outcome, Toolbox};
use ultar_agentd::{links, session, tools};
use ultar_bus::messages::agent::{Brain, StepOutcome};
use ultar_bus::messages::llm::ModelClass;
use ultar_bus::messages::settings::{Set, SettingValue};
use ultar_bus::Client;
use ultar_desktop::app::keyboard;
use ultar_displayd::input::InputSettings;
use ultar_llm_bus::{backend, choice, place};
use ultar_surface::proto::{Request, SwitchShortcut};

/// Keyboard page: the switch combination and the per-window languages reach the compositor
/// through the display service; the input languages become the shell's layout list.
pub(super) fn keyboard_layouts(
    bus: &Client,
    admin: &Client,
    run: &Run,
    displayd: &Follower,
    desktop: &Follower,
) -> Result<(), String> {
    let mut d = Display {
        told: displayd,
        service: InputSettings::default(),
    };
    let mut app = page(bus, run, "keyboard")?;
    // super_space -> alt_shift -> ctrl_shift (the list starts at the selected option).
    let shortcut = choose(
        (&mut app, admin),
        ("keyboard", "keyboard.switch_shortcut"),
        2,
    )?;
    d.follow("keyboard.switch_shortcut", shortcut)?;
    let per = change((&mut app, admin), ("keyboard", "keyboard.per_window"), None)?;
    d.follow("keyboard.per_window", per)?;
    let Request::ConfigureKeyboard {
        switch_shortcut,
        per_window,
    } = d.service.requests()[2].clone()
    else {
        return Err("no keyboard configuration".into());
    };
    if switch_shortcut != SwitchShortcut::CtrlShift || !per_window {
        return Err(format!(
            "keyboard configuration {switch_shortcut:?}, {per_window}"
        ));
    }
    // The input languages: the shell's own list code follows the page's list.
    let langs = edit(
        &mut app,
        admin,
        "keyboard",
        "keyboard.input_languages",
        "de, en",
    )?;
    desktop.told("keyboard.input_languages", &langs)?;
    let SettingValue::List(l) = langs else {
        return Err(format!("input_languages is {langs:?}"));
    };
    let list = keyboard::combined(&l, "ar_en");
    let got = keyboard::from_values(list.join("_"), Some("ar".into()));
    let wanted = (vec!["de".to_owned(), "en".to_owned()], 0);
    if got != Some(wanted) {
        return Err(format!("the shell's layouts are {got:?}"));
    }
    println!(
        "SETTINGS: keyboard page -> compositor and shell: Ctrl+Shift switches, every window \
         keeps its language, input languages de, en"
    );
    Ok(())
}

/// A model that always says the same words; a toolbox that does nothing (the helper's own
/// session code decides what the answer looks like).
struct Say(&'static str);
impl Thinker for Say {
    fn think(&self, _: &str, _: &str) -> Option<String> {
        Some(self.0.to_owned())
    }
}
struct NoTools;
impl Toolbox for NoTools {
    fn run(&self, _: &Call) -> Outcome {
        (StepOutcome::Done, String::new())
    }
}

/// Assistant page: the model size chooses among the installed models (the bridge's own
/// code); seeing the screen opens only with the person's switch, and the link warning
/// follows its switch (the helper's own code).
pub(super) fn assistant(bus: &Client, admin: &Client, run: &Run) -> Result<(), String> {
    let llmbus = Follower::connect(run, "the model bridge", "llmbus")?;
    let agentd = Follower::connect(run, "the helper", "agentd")?;
    let mut app = page(bus, run, "assistant")?;
    // small -> medium: the bridge's parsing and pick choose the medium model.
    let model = choose((&mut app, admin), ("assistant", "assistant.model"), 1)?;
    llmbus.told("assistant.model", &model)?;
    let class = choice::of(&model).ok_or(format!("model value {model:?}"))?;
    if class != ModelClass::Medium {
        return Err(format!("model class {class:?}"));
    }
    let installed = vec![
        ("tiny".to_owned(), 400_000_000u64),
        ("qwen-4b".to_owned(), 2_500_000_000),
        ("big".to_owned(), 20_000_000_000),
    ];
    let picked = backend::pick_class(&installed, class).ok_or("no medium model picked")?;
    if picked.0 != "qwen-4b" {
        return Err(format!("picked {}", picked.0));
    }
    // Seeing the screen: off (the default) closes the tool, on opens it.
    let see = change(
        (&mut app, admin),
        ("assistant", "assistant.see_screen"),
        None,
    )?;
    agentd.told("assistant.see_screen", &see)?;
    let tool = tools::find("see_screen").ok_or("no see_screen tool")?;
    let on = Limits {
        see_screen: true,
        ..Limits::default()
    };
    if Limits::default().closes(tool).is_none() || on.closes(tool).is_some() {
        return Err("seeing the screen does not follow its switch".into());
    }
    check_links(&mut app, admin, &agentd)?;
    model_place(admin, &llmbus)?;
    println!(
        "SETTINGS: assistant page -> model bridge and helper: medium model chosen, seeing the \
         screen opened by its switch, link warning follows its switch, model place passed on \
         only when checked"
    );
    Ok(())
}

/// The model place: the bridge is told and passes only a plain folder below the model
/// folders or removable media on to the model service (the bridge's own code).
fn model_place(admin: &Client, llmbus: &Follower) -> Result<(), String> {
    const MINE: &str = "/data/ultar/models/mine";
    let value = SettingValue::Text(MINE.into());
    let set = Set {
        key: "assistant.model_place".into(),
        value: value.clone(),
    };
    admin
        .call(&set, std::time::Duration::from_secs(20))
        .map_err(|e| format!("set assistant.model_place: {e}"))?;
    llmbus.told("assistant.model_place", &value)?;
    let outside = SettingValue::Text("/data/ultar/models/../../etc".into());
    if place::of(&value).as_deref() != Some(MINE) || place::of(&outside).as_deref() != Some("") {
        return Err("the model place is not checked".into());
    }
    Ok(())
}

/// Checking links: the warning is appended while it is on, gone when off (the helper's own
/// code).
fn check_links(
    app: &mut ultar_settings::App,
    admin: &Client,
    agentd: &Follower,
) -> Result<(), String> {
    let checking = change((app, admin), ("assistant", "assistant.check_links"), None)?;
    agentd.told("assistant.check_links", &checking)?;
    if links::first_suspicious("open http://203.0.113.7/login").is_none() {
        return Err("the link checker does not see the suspicious link".into());
    }
    const WORDS: &str = "{\"say\":\"open http://203.0.113.7/login\"}";
    let warned = session::answer("x", Brain::Local, &Say(WORDS), &NoTools, &Limits::default());
    let off = Limits {
        check_links: false,
        ..Limits::default()
    };
    let plain = session::answer("x", Brain::Local, &Say(WORDS), &NoTools, &off);
    if warned.text.key != "agent.says_checked" || plain.text.key != "agent.says" {
        return Err(format!(
            "link warning: {} / {}",
            warned.text.key, plain.text.key
        ));
    }
    Ok(())
}
