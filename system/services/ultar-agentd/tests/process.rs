//! The real `ultar-agentd` on a bus with stand-ins for the services it uses: where it
//! thinks on a device without a model, with a lender in the mesh and with its own model,
//! and that nothing dangerous happens without the person's confirmation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::agent::{AgentState, Answer, Ask, Brain, Step, StepOutcome};
use ultar_bus::messages::audio::{AudioOutcome, AudioReply, SetMute};
use ultar_bus::messages::launcher::{InstanceState, Start, StartReply};
use ultar_bus::messages::llm::{Complete, Completion, LlmState, ModelClass};
use ultar_bus::messages::mesh_model::{Lender, ModelList, Models, Think, Thought};
use ultar_bus::messages::mesh_remote::{Act, Acted};
use ultar_bus::messages::power::{ActionOutcome, ActionReply, PowerOff};
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, AuthenticateReply};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.agentd"
register = ["agent"]
publish = ["agent.state"]
subscribe = ["llm.state", "mesh.state"]
call = ["launcher.list", "launcher.start", "audio.set_volume", "audio.set_mute", "net.set_wifi",
        "mesh.models", "mesh.think", "mesh.handoff_send", "mesh.send_file", "mesh.act",
        "llm.complete",
        "sysdialog.authenticate", "power.restart", "power.off"]

[[rule]]
identity = "system.stand_in"
register = ["launcher", "audio", "mesh", "llm", "sysdialog", "power"]
publish = ["llm.state"]

[[rule]]
identity = "system.shell"
call = ["agent.ask"]
subscribe = ["agent.state"]
"#;

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(30));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

/// What the stand-ins saw and how they answer.
#[derive(Default)]
struct Seen {
    muted: Mutex<Vec<bool>>,
    started: Mutex<Vec<String>>,
    powered_off: AtomicBool,
    /// What the model says next (the same script for a local and a lent model).
    script: Mutex<Vec<&'static str>>,
    asked_mesh: AtomicBool,
    /// What the helper of another device was asked to do.
    asked_there: Mutex<Vec<String>>,
    asked_local: AtomicBool,
    confirm: AtomicBool,
}

struct Bench {
    world: World,
    _daemon: Daemon,
    shell: Client,
    seen: Arc<Seen>,
    stand_in: Client,
}

fn next(seen: &Seen) -> String {
    let mut script = seen.script.lock().unwrap();
    if script.is_empty() {
        String::new()
    } else {
        script.remove(0).to_owned()
    }
}

fn serve_actions(world: &World, seen: &Arc<Seen>) {
    let s = seen.clone();
    world.serve("system.stand_in", "audio", move |request| {
        if let Ok(mute) = request.decode::<SetMute>() {
            s.muted.lock().unwrap().push(mute.on);
        }
        let outcome = AudioOutcome::Done;
        let _ = request.reply(&AudioReply { outcome });
    });
    let s = seen.clone();
    world.serve("system.stand_in", "launcher", move |request| {
        if let Ok(start) = request.decode::<Start>() {
            s.started.lock().unwrap().push(start.app);
            let state = InstanceState::Running;
            let _ = request.reply(&StartReply {
                state,
                pid: 1,
                already: false,
                opened: false,
            });
        }
    });
    let s = seen.clone();
    world.serve("system.stand_in", "power", move |request| {
        if request.decode::<PowerOff>().is_ok() {
            s.powered_off.store(true, Ordering::SeqCst);
        }
        let outcome = ActionOutcome::Done;
        let _ = request.reply(&ActionReply { outcome });
    });
    let s = seen.clone();
    world.serve("system.stand_in", "sysdialog", move |request| {
        if request.decode::<Authenticate>().is_ok() {
            let result = if s.confirm.load(Ordering::SeqCst) {
                AuthResult::Authorized
            } else {
                AuthResult::Cancelled
            };
            let _ = request.reply(&AuthenticateReply { result });
        }
    });
}

/// `lender`: the class of the model a paired device lends (`None`: nobody lends).
fn serve_models(world: &World, seen: &Arc<Seen>, lender: Option<ModelClass>) {
    let s = seen.clone();
    world.serve("system.stand_in", "mesh", move |request| {
        if request.method() == Models::METHOD {
            let lenders = lender
                .map(|class| Lender {
                    device: "desk".into(),
                    name: "Desk".into(),
                    class,
                })
                .into_iter()
                .collect();
            let _ = request.reply(&ModelList { lenders });
        } else if let Ok(act) = request.decode::<Act>() {
            // The other device's helper: it does what it was asked, when it is "desk".
            let step = Step {
                tool: "mute".into(),
                outcome: StepOutcome::Done,
            };
            s.asked_there.lock().unwrap().push(act.text);
            let _ = request.reply(&Acted {
                ok: act.device == "desk",
                steps: vec![step],
                by: act.device,
            });
        } else if request.decode::<Think>().is_ok() {
            s.asked_mesh.store(true, Ordering::SeqCst);
            let text = next(&s);
            let _ = request.reply(&Thought {
                ok: !text.is_empty(),
                text,
                by: "desk".into(),
            });
        }
    });
    let s = seen.clone();
    world.serve("system.stand_in", "llm", move |request| {
        if request.decode::<Complete>().is_ok() {
            s.asked_local.store(true, Ordering::SeqCst);
            let text = next(&s);
            let _ = request.reply(&Completion {
                ok: !text.is_empty(),
                text,
            });
        }
    });
}

fn bench(tag: &str, lender: Option<ModelClass>, local: Option<ModelClass>) -> Bench {
    let world = World::new(
        tag,
        POLICY,
        &["system.agentd", "system.stand_in", "system.shell"],
    );
    let seen = Arc::new(Seen::default());
    serve_actions(&world, &seen);
    serve_models(&world, &seen, lender);
    let stand_in = world.client("system.stand_in");
    if let Some(class) = local {
        let state = LlmState {
            ready: true,
            model: "own".into(),
            class,
        };
        stand_in.publish(&state).unwrap();
    }
    let daemon = world.spawn(env!("CARGO_BIN_EXE_ultar-agentd"), "system.agentd", &[]);
    let shell = world.client("system.shell");
    Bench {
        world,
        _daemon: daemon,
        shell,
        seen,
        stand_in,
    }
}

fn brain(b: &Bench, wanted: &Brain) -> AgentState {
    let states = b.shell.subscribe::<AgentState>().unwrap();
    let (tx, rx) = mpsc::channel();
    assert!(
        wait_until(WAIT, || {
            while let Ok(Some(state)) = states.try_recv() {
                if &state.brain == wanted {
                    let _ = tx.send(state);
                    return true;
                }
            }
            false
        }),
        "the helper never thought with {wanted:?}"
    );
    rx.recv().unwrap()
}

fn ask(b: &Bench, text: &str) -> Answer {
    call(
        &b.shell,
        &Ask {
            text: text.to_owned(),
        },
    )
    .unwrap()
}

#[test]
fn a_device_without_a_model_gets_the_smallest_help() {
    let b = bench("agent-min", None, None);
    brain(&b, &Brain::Minimal);
    let done = ask(&b, "mute");
    assert_eq!(
        (done.brain, done.text.key.as_str()),
        (Brain::Minimal, "agent.done")
    );
    assert_eq!(*b.seen.muted.lock().unwrap(), vec![true]);
    let help = ask(&b, "write me a letter");
    assert_eq!(help.text.key, "agent.minimal.help");
    assert!(help.steps.is_empty());
    assert!(
        call(&b.shell, &Ask { text: "  ".into() }).is_err(),
        "nothing was asked"
    );
    let _ = &b.world;
}

#[test]
fn a_weak_device_thinks_with_the_model_a_paired_device_lends() {
    let b = bench("agent-mesh", Some(ModelClass::Large), None);
    let state = brain(&b, &Brain::Mesh("desk".into()));
    assert_eq!(state.lender, "Desk");
    *b.seen.script.lock().unwrap() = vec![
        "{\"tool\":\"open_app\",\"args\":{\"app\":\"org.ultar.browser\"}}",
        "{\"say\":\"The browser is open.\"}",
    ];
    let answer = ask(&b, "I want to read the news");
    assert_eq!(answer.brain, Brain::Mesh("desk".into()));
    assert_eq!(answer.text.key, "agent.says");
    assert_eq!(answer.steps.len(), 1);
    assert_eq!(
        *b.seen.started.lock().unwrap(),
        // the launcher knows the apps of the system by their one-part id
        vec!["browser".to_owned()]
    );
    assert!(b.seen.asked_mesh.load(Ordering::SeqCst));
    assert!(!b.seen.asked_local.load(Ordering::SeqCst));
}

#[test]
fn the_helper_lets_the_helper_of_another_device_act_there() {
    let b = bench("agent-remote", None, Some(ModelClass::Medium));
    brain(&b, &Brain::Local);
    *b.seen.script.lock().unwrap() = vec![
        "{\"tool\":\"ask_device\",\"args\":{\"device\":\"desk\",\"request\":\"mute the sound\"}}",
        "{\"tool\":\"ask_device\",\"args\":{\"device\":\"tv\",\"request\":\"mute the sound\"}}",
        "{\"say\":\"The computer is muted, the television did not take it.\"}",
    ];
    let answer = ask(&b, "mute the computer and the television");
    let outcomes: Vec<StepOutcome> = answer.steps.iter().map(|s| s.outcome).collect();
    assert_eq!(outcomes, [StepOutcome::Done, StepOutcome::Failed]);
    assert_eq!(b.seen.asked_there.lock().unwrap().len(), 2);
    // Nothing was muted here.
    assert!(b.seen.muted.lock().unwrap().is_empty());
}

#[test]
fn an_own_model_is_used_unless_the_mesh_has_a_better_one() {
    let b = bench(
        "agent-local",
        Some(ModelClass::Small),
        Some(ModelClass::Medium),
    );
    brain(&b, &Brain::Local);
    *b.seen.script.lock().unwrap() = vec!["{\"say\":\"Hello.\"}"];
    assert_eq!(ask(&b, "hello").brain, Brain::Local);
    assert!(b.seen.asked_local.load(Ordering::SeqCst));
    // The own model becomes unusable (battery, heat): the lender takes over.
    let gone = LlmState {
        ready: false,
        model: "own".into(),
        class: ModelClass::Medium,
    };
    b.stand_in.publish(&gone).unwrap();
    brain(&b, &Brain::Mesh("desk".into()));
}

#[test]
fn nothing_dangerous_happens_without_the_persons_confirmation() {
    let b = bench("agent-danger", None, Some(ModelClass::Medium));
    brain(&b, &Brain::Local);
    let script = || vec!["{\"tool\":\"power_off\"}", "{\"say\":\"ok\"}"];
    *b.seen.script.lock().unwrap() = script();
    let refused = ask(&b, "switch it off");
    assert_eq!(refused.steps[0].outcome, StepOutcome::NotConfirmed);
    assert!(!b.seen.powered_off.load(Ordering::SeqCst));
    b.seen.confirm.store(true, Ordering::SeqCst);
    *b.seen.script.lock().unwrap() = script();
    let done = ask(&b, "switch it off");
    assert_eq!(done.steps[0].outcome, StepOutcome::Done);
    assert!(b.seen.powered_off.load(Ordering::SeqCst));
}

#[test]
fn every_text_the_helper_can_say_exists_in_all_three_languages() {
    let keys = [
        "agent.done",
        "agent.failed",
        "agent.not_confirmed",
        "agent.not_allowed",
        "agent.says",
        "agent.too_many_steps",
        "agent.minimal.help",
        "agent.action.restart",
        "agent.action.power_off",
    ];
    for (lang, catalogue) in ultar_bus::messages::agent::LOCALES {
        for key in keys {
            assert!(catalogue.contains(&format!("\n{key} = ")), "{lang}: {key}");
        }
    }
}
