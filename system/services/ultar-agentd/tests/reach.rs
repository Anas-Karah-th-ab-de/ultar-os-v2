//! The real `ultar-agentd` reaching the whole system: every setting through the settings
//! service – but never its own switches, never an administrator's setting, a sensitive one
//! only after the person confirmed – and the two things the person looks at: which areas
//! are open, and what the helper did.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::agent::{Activity, Answer, Area, Ask, ClearActivity, Reach, StepOutcome};
use ultar_bus::messages::agent::{Progress, ACT_KEY};
use ultar_bus::messages::llm::{Complete, Completion, LlmState, ModelClass};
use ultar_bus::messages::settings::{
    Changed, Get, GetReply, ItemInfo, ItemKind, ListReply, ListRequest, PanelInfo, RiskInfo,
    ScopeInfo, Set, SetReply, SettingValue,
};
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, AuthenticateReply};
use ultar_bus::{BusError, Call, Client, ErrorCode, Text, Topic};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.agentd"
register = ["agent"]
publish = ["agent.state", "agent.reach", "agent.activity", "agent.progress"]
subscribe = ["llm.state", "mesh.state", "llmcloud.state", "settings.changed"]
call = ["llm.complete", "settings.list", "settings.get", "settings.set", "sysdialog.authenticate", "capture.screenshot"]

[[rule]]
identity = "system.stand_in"
register = ["llm", "settings", "sysdialog", "capture"]
publish = ["llm.state", "settings.changed"]

[[rule]]
identity = "system.shell"
call = ["agent.ask"]
subscribe = ["agent.progress"]

[[rule]]
identity = "app.settings"
call = ["agent.clear_activity"]
subscribe = ["agent.reach", "agent.activity"]
"#;

#[derive(Default)]
struct Seen {
    script: Mutex<Vec<&'static str>>,
    set: Mutex<Vec<(String, SettingValue)>>,
    confirm: AtomicBool,
    /// The person switched acting on.
    acts: AtomicBool,
    asked_to_confirm: Mutex<Vec<String>>,
    /// A screenshot was taken (the capture service was called).
    captured: AtomicBool,
}

fn item(key: &str, risk: RiskInfo) -> ItemInfo {
    ItemInfo {
        key: key.to_owned(),
        label: Text::default(),
        description: Text::default(),
        kind: ItemKind::Toggle,
        value: SettingValue::Flag(false),
        default: SettingValue::Flag(false),
        risk,
        scope: ScopeInfo::System,
        writable: risk != RiskInfo::AdminOnly,
        pending: false,
        template: false,
    }
}

fn panels() -> Vec<PanelInfo> {
    let panel = |id: &str, items: Vec<ItemInfo>| PanelInfo {
        id: id.to_owned(),
        title: Text::default(),
        items,
    };
    vec![
        panel("appearance", vec![item("appearance.dark", RiskInfo::Safe)]),
        panel("privacy", vec![item("privacy.location", RiskInfo::Safe)]),
        panel("sharing", vec![item("sharing.ssh", RiskInfo::AdminOnly)]),
        panel(
            "assistant",
            vec![item("assistant.reach.power", RiskInfo::Safe)],
        ),
    ]
}

struct Bench {
    _world: World,
    _daemon: Daemon,
    shell: Client,
    settings: Client,
    stand_in: Client,
    seen: Arc<Seen>,
}

fn serve_capture(world: &World, seen: &Arc<Seen>) {
    use ultar_bus::messages::capture::{Screenshot, ScreenshotReply, ShotOutcome};
    let s = seen.clone();
    world.serve("system.stand_in", "capture", move |request| {
        if request.decode::<Screenshot>().is_ok() {
            s.captured.store(true, Ordering::SeqCst);
            let outcome = ShotOutcome::Saved("/home/amal/Pictures/shot.png".into());
            let _ = request.reply(&ScreenshotReply { outcome });
        }
    });
}

fn serve_model(world: &World, seen: &Arc<Seen>) {
    let s = seen.clone();
    world.serve("system.stand_in", "llm", move |request| {
        if request.decode::<Complete>().is_ok() {
            let mut script = s.script.lock().unwrap();
            let text = if script.is_empty() {
                String::new()
            } else {
                script.remove(0).to_owned()
            };
            let ok = !text.is_empty();
            let _ = request.reply(&Completion { ok, text });
        }
    });
}

fn bench_with(tag: &str, acts: bool) -> Bench {
    let ids = [
        "system.agentd",
        "system.stand_in",
        "system.shell",
        "app.settings",
    ];
    let world = World::new(tag, POLICY, &ids);
    let seen = Arc::new(Seen::default());
    seen.acts.store(acts, Ordering::SeqCst);
    serve_model(&world, &seen);
    serve_capture(&world, &seen);
    let s = seen.clone();
    world.serve("system.stand_in", "settings", move |request| {
        if request.method() == ListRequest::METHOD {
            let _ = request.reply(&ListReply { panels: panels() });
        } else if request.method() == Get::METHOD {
            // the person switched acting on; nothing else is set
            let acts = request.decode::<Get>().is_ok_and(|get| get.key == ACT_KEY);
            let value = (acts && s.acts.load(Ordering::SeqCst)).then_some(SettingValue::Flag(true));
            let _ = request.reply(&GetReply { value });
        } else if let Ok(set) = request.decode::<Set>() {
            s.set.lock().unwrap().push((set.key, set.value));
            let _ = request.reply(&SetReply { changed: true });
        } else {
            let _ = request.fail(ErrorCode::UnknownMethod);
        }
    });
    let s = seen.clone();
    world.serve("system.stand_in", "sysdialog", move |request| {
        if let Ok(ask) = request.decode::<Authenticate>() {
            s.asked_to_confirm.lock().unwrap().push(ask.action.key);
            let result = if s.confirm.load(Ordering::SeqCst) {
                AuthResult::Authorized
            } else {
                AuthResult::Cancelled
            };
            let _ = request.reply(&AuthenticateReply { result });
        }
    });
    let stand_in = world.client("system.stand_in");
    let state = LlmState {
        ready: true,
        model: "own".into(),
        class: ModelClass::Small,
    };
    stand_in.publish(&state).unwrap();
    let state_dir = world.subdir("agent-state");
    let args = ["--state", state_dir.to_str().unwrap()];
    let daemon = world.spawn(env!("CARGO_BIN_EXE_ultar-agentd"), "system.agentd", &args);
    Bench {
        shell: world.client("system.shell"),
        settings: world.client("app.settings"),
        _world: world,
        _daemon: daemon,
        stand_in,
        seen,
    }
}

fn bench(tag: &str) -> Bench {
    let b = bench_with(tag, true);
    latest::<Reach>(&b.settings, |r| r.acts);
    b
}

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(30));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

/// Asks with the model saying `script`; waits first until the helper thinks with a model.
fn ask(b: &Bench, text: &str, script: &[&'static str]) -> Answer {
    let ask = Ask {
        text: text.to_owned(),
    };
    let mut answer = None;
    assert!(wait_until(WAIT, || {
        *b.seen.script.lock().unwrap() = script.to_vec();
        answer = call(&b.shell, &ask).ok();
        answer.as_ref().is_some_and(|a| !a.steps.is_empty())
    }));
    answer.unwrap()
}

fn latest<T: Topic>(client: &Client, wanted: impl Fn(&T) -> bool) -> T {
    let events = client.subscribe::<T>().unwrap();
    let mut found = None;
    assert!(wait_until(WAIT, || {
        while let Ok(Some(event)) = events.try_recv() {
            if wanted(&event) {
                found = Some(event);
            }
        }
        found.is_some()
    }));
    found.unwrap()
}

fn set_script(key: &str) -> [&'static str; 2] {
    let line =
        format!("{{\"tool\":\"setting_set\",\"args\":{{\"key\":\"{key}\",\"value\":true}}}}");
    [Box::leak(line.into_boxed_str()), "{\"say\":\"ok\"}"]
}

#[test]
fn the_helper_changes_any_setting_but_not_its_own_and_asks_before_a_sensitive_one() {
    let b = bench("agent-reach-settings");
    let outcome = |key: &str| ask(&b, "set it", &set_script(key)).steps[0].outcome;

    assert_eq!(outcome("appearance.dark"), StepOutcome::Done);
    // its own switches and an administrator's setting: never, and nobody is even asked
    assert_eq!(outcome("assistant.reach.power"), StepOutcome::NotAllowed);
    assert_eq!(outcome("sharing.ssh"), StepOutcome::NotAllowed);
    assert!(b.seen.asked_to_confirm.lock().unwrap().is_empty());
    // a sensitive panel: only after the person confirmed
    // whoever asked sees the steps as they happen, and when one waits for the person
    let progress = b.shell.subscribe::<Progress>().unwrap();
    assert_eq!(outcome("privacy.location"), StepOutcome::NotConfirmed);
    let mut seen = Vec::new();
    while let Ok(Some(p)) = progress.try_recv() {
        seen.push(p);
    }
    assert_eq!(seen[0], Progress::Waiting("setting_set".into()));
    assert!(matches!(&seen[1], Progress::Step(s) if s.outcome == StepOutcome::NotConfirmed));
    b.seen.confirm.store(true, Ordering::SeqCst);
    assert_eq!(outcome("privacy.location"), StepOutcome::Done);
    assert_eq!(
        *b.seen.asked_to_confirm.lock().unwrap(),
        ["agent.action.setting_set", "agent.action.setting_set"]
    );
    let flag = SettingValue::Flag(true);
    assert_eq!(
        *b.seen.set.lock().unwrap(),
        [
            ("appearance.dark".to_owned(), flag.clone()),
            ("privacy.location".to_owned(), flag)
        ]
    );
}

#[test]
fn the_person_switches_an_area_off_and_reads_and_forgets_what_the_helper_did() {
    let b = bench("agent-reach-areas");
    let open = latest::<Reach>(&b.settings, |r| r.areas.len() == Area::ALL.len());
    assert!(open.acts && open.areas.iter().all(|a| a.allowed && !a.tools.is_empty()));
    let first = ask(&b, "dark please", &set_script("appearance.dark"));
    assert_eq!(first.steps[0].outcome, StepOutcome::Done);

    let off = Changed {
        key: "assistant.reach.settings".into(),
        value: SettingValue::Flag(false),
    };
    b.stand_in.publish(&off).unwrap();
    let closed = |r: &Reach| {
        r.areas
            .iter()
            .any(|a| a.area == Area::Settings && !a.allowed)
    };
    latest::<Reach>(&b.settings, closed);
    let second = ask(&b, "dark again", &set_script("appearance.dark"));
    assert_eq!(second.steps[0].outcome, StepOutcome::NotAllowed);
    assert_eq!(
        b.seen.set.lock().unwrap().len(),
        1,
        "the closed area was not touched"
    );

    let done = latest::<Activity>(&b.settings, |a| a.entries.len() == 2);
    let step = &done.entries[0].steps[0];
    assert_eq!(done.entries[0].request, "dark please");
    assert_eq!(
        (step.tool.as_str(), step.area),
        ("setting_set", Area::Settings)
    );
    assert_eq!(step.detail, "key=appearance.dark, value=true");
    assert_eq!(done.entries[1].steps[0].outcome, StepOutcome::NotAllowed);

    // only the Settings app forgets it
    assert!(call(&b.shell, &ClearActivity {}).is_err());
    assert!(call(&b.settings, &ClearActivity {}).unwrap().ok);
    latest::<Activity>(&b.settings, |a| a.entries.is_empty());
}

#[test]
fn until_the_person_switches_acting_on_the_helper_only_looks() {
    let b = bench_with("agent-reach-look", false);
    let closed = latest::<Reach>(&b.settings, |r| !r.areas.is_empty());
    assert!(!closed.acts);
    let refused = ask(&b, "dark please", &set_script("appearance.dark"));
    assert_eq!(refused.steps[0].outcome, StepOutcome::NotAllowed);
    assert!(b.seen.set.lock().unwrap().is_empty());
    let look = [
        "{\"tool\":\"setting_get\",\"args\":{\"key\":\"appearance.dark\"}}",
        "{\"say\":\"off\"}",
    ];
    assert_eq!(
        ask(&b, "is it dark?", &look).steps[0].outcome,
        StepOutcome::Done
    );
    // the person switches it on: the helper follows at once
    let on = Changed {
        key: ACT_KEY.into(),
        value: SettingValue::Flag(true),
    };
    b.stand_in.publish(&on).unwrap();
    latest::<Reach>(&b.settings, |r| r.acts);
    let done = ask(&b, "dark please", &set_script("appearance.dark"));
    assert_eq!(done.steps[0].outcome, StepOutcome::Done);
}

/// Asks until the model's words arrive (an answer without steps).
fn say_answer(b: &Bench, script: &[&'static str]) -> Answer {
    let ask = Ask {
        text: "what next?".to_owned(),
    };
    let mut answer = None;
    assert!(wait_until(WAIT, || {
        *b.seen.script.lock().unwrap() = script.to_vec();
        answer = call(&b.shell, &ask).ok();
        answer
            .as_ref()
            .is_some_and(|a| a.text.key.starts_with("agent.says"))
    }));
    answer.unwrap()
}

/// Asks until the first step has the outcome `wanted` (a setting may still be on its way).
fn wait_outcome(b: &Bench, script: &[&'static str], wanted: StepOutcome) {
    assert!(wait_until(WAIT, || {
        ask(b, "what is on my screen?", script).steps[0].outcome == wanted
    }));
}

#[test]
fn the_person_allows_seeing_the_screen_and_consents_before_each_look() {
    let b = bench("agent-see-screen");
    let script = ["{\"tool\":\"see_screen\"}", "{\"say\":\"ok\"}"];
    let say = |on: bool| {
        let changed = Changed {
            key: "assistant.see_screen".into(),
            value: SettingValue::Flag(on),
        };
        b.stand_in.publish(&changed).unwrap();
    };
    // Off (the default): the tool is closed, nobody is asked, nothing captured.
    wait_outcome(&b, &script, StepOutcome::NotAllowed);
    assert!(b.seen.asked_to_confirm.lock().unwrap().is_empty());
    assert!(!b.seen.captured.load(Ordering::SeqCst));
    // The person allows it at runtime: without their consent in the system dialog nothing
    // is captured, and the dialog was asked.
    say(true);
    wait_outcome(&b, &script, StepOutcome::NotConfirmed);
    assert!(!b.seen.captured.load(Ordering::SeqCst));
    assert!(b
        .seen
        .asked_to_confirm
        .lock()
        .unwrap()
        .contains(&"agent.action.see_screen".to_owned()));
    // With the consent: the screen is captured.
    b.seen.confirm.store(true, Ordering::SeqCst);
    wait_outcome(&b, &script, StepOutcome::Done);
    assert!(b.seen.captured.load(Ordering::SeqCst));
    // Off again: closed at once.
    say(false);
    wait_outcome(&b, &script, StepOutcome::NotAllowed);
}

#[test]
fn suspicious_links_are_warned_about_until_the_person_switches_checking_off() {
    let b = bench("agent-check-links");
    const LINK: &str = "{\"say\":\"open http://203.0.113.7/login to continue\"}";
    // On (the default): the warning is appended to the model's words.
    let warned = say_answer(&b, &[LINK]);
    assert_eq!(warned.text.key, "agent.says_checked");
    // The person switches it off at runtime: the words as they are.
    let off = Changed {
        key: "assistant.check_links".into(),
        value: SettingValue::Flag(false),
    };
    b.stand_in.publish(&off).unwrap();
    let mut plain = false;
    assert!(wait_until(WAIT, || {
        plain = say_answer(&b, &[LINK]).text.key == "agent.says";
        plain
    }));
    assert!(plain);
    // On again: the warning returns.
    let on = Changed {
        key: "assistant.check_links".into(),
        value: SettingValue::Flag(true),
    };
    b.stand_in.publish(&on).unwrap();
    let mut warned_again = false;
    assert!(wait_until(WAIT, || {
        warned_again = say_answer(&b, &[LINK]).text.key == "agent.says_checked";
        warned_again
    }));
    assert!(warned_again);
}

/// Media, power, session and notifications answer only callers the shipped rules mark as
/// part of the system. Without that mark a tool passes the bus and is refused by the
/// service – which a stand-in never shows.
#[test]
fn the_shipped_rules_let_the_services_accept_the_helper() {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../rootfs/etc/ultar/bus-policy.toml"
    );
    let policy = std::fs::read_to_string(file).unwrap();
    let start = policy.find("identity  = \"system.agentd\"").unwrap();
    let rule = &policy[start..];
    let rule = &rule[..rule.find("\n\n").unwrap()];
    for service in ["media", "power", "session", "notify"] {
        assert!(rule.contains(&format!("\"{service}.")), "{service} is used");
        assert!(
            rule.contains(&format!("\"{service}.system\"")),
            "{service}.system"
        );
    }
    // and never more than its own narrow list
    for wide in [
        "settings.admin",
        "\"settings.*\"",
        "account.",
        "auth.",
        "init.",
        "launcher.grant",
        "notify.invoke",
    ] {
        assert!(!rule.contains(wide), "{wide}");
    }
}
