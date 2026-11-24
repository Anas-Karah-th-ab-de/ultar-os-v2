//! The real `ultar-powerd` against an in-process broker, stand-ins for the service
//! manager, the lock screen and the system dialog, and a prepared kernel tree.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::init::{Power as InitPower, PowerAction, PowerReply};
use ultar_bus::messages::lock::{Lock, LockReply};
use ultar_bus::messages::power::{
    ActionOutcome, PowerActions, PowerOff, PowerSource, PowerState, Restart, Suspend,
};
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, AuthenticateReply};
use ultar_bus::{BusError, Call, Client, ErrorCode};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.powerd"
register = ["power"]
publish = ["power.state", "power.actions"]
subscribe = ["session.state"]
call = ["init.power", "lock.lock", "sysdialog.authenticate"]

[[rule]]
identity = "system.init"
register = ["init"]

[[rule]]
identity = "system.lock"
register = ["lock"]

[[rule]]
identity = "system.sysdialog"
register = ["sysdialog"]

[[rule]]
identity = "system.sessiond"
publish = ["session.state"]

[[rule]]
identity = "system.shell"
call = ["power.suspend", "power.restart", "power.off", "power.system"]
subscribe = ["power.state", "power.actions"]

[[rule]]
identity = "app.sneaky"
call = ["power.off"]
"#;

const IDS: [&str; 7] = [
    "system.powerd",
    "system.init",
    "system.lock",
    "system.sysdialog",
    "system.sessiond",
    "system.shell",
    "app.sneaky",
];

#[derive(Default)]
struct Seen {
    init: Vec<PowerAction>,
    locks: u32,
    dialogs: u32,
    lock_answer: bool,
    dialog_answer: Option<AuthResult>,
    held: Vec<ultar_bus::IncomingRequest>,
}

fn put(root: &Path, path: &str, value: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, value).unwrap();
}

struct Setup {
    world: World,
    seen: Arc<Mutex<Seen>>,
    root: std::path::PathBuf,
}

fn setup(tag: &str, sleep: bool, with_init: bool) -> Setup {
    let world = World::new(tag, POLICY, &IDS);
    let root = world.subdir("root");
    put(
        &root,
        "sys/power/state",
        if sleep { "freeze mem\n" } else { "freeze\n" },
    );
    put(&root, "sys/class/power_supply/BAT0/type", "Battery\n");
    put(&root, "sys/class/power_supply/BAT0/capacity", "64\n");
    put(&root, "sys/class/power_supply/BAT0/status", "Discharging\n");
    let seen = Arc::new(Mutex::new(Seen {
        lock_answer: true,
        dialog_answer: Some(AuthResult::Cancelled),
        ..Seen::default()
    }));
    if with_init {
        let s = seen.clone();
        world.serve("system.init", "init", move |r| {
            let _ = r.respond::<InitPower>(|p| {
                s.lock().unwrap().init.push(p.action);
                Ok(PowerReply {})
            });
        });
    }
    let s = seen.clone();
    world.serve("system.lock", "lock", move |r| {
        let _ = r.respond::<Lock>(|_| {
            let mut s = s.lock().unwrap();
            s.locks += 1;
            Ok(LockReply {
                locked: s.lock_answer,
            })
        });
    });
    let s = seen.clone();
    world.serve("system.sysdialog", "sysdialog", move |r| {
        let answer = {
            let mut s = s.lock().unwrap();
            s.dialogs += 1;
            s.dialog_answer
        };
        match answer {
            Some(result) => {
                let _ = r.respond::<Authenticate>(|_| Ok(AuthenticateReply { result }));
            }
            None => s.lock().unwrap().held.push(r),
        }
    });
    Setup { world, seen, root }
}

fn start(s: &Setup) -> Daemon {
    let policy = s.world.policy_file();
    s.world.spawn(
        env!("CARGO_BIN_EXE_ultar-powerd"),
        "system.powerd",
        &[
            "--policy",
            policy.to_str().unwrap(),
            "--root",
            s.root.to_str().unwrap(),
        ],
    )
}

fn call<C: Call>(client: &Client, c: &C) -> Result<C::Reply, BusError> {
    let mut last = Err(BusError::Timeout);
    wait_until(WAIT, || {
        last = client.call(c, Duration::from_secs(10));
        !matches!(last, Err(BusError::Remote(ErrorCode::UnknownService)))
    });
    last
}

fn outcome<C: Call<Reply = ultar_bus::messages::power::ActionReply>>(
    client: &Client,
    c: &C,
) -> ActionOutcome {
    call(client, c).unwrap().outcome
}

fn sessions(world: &World, names: &[&str]) {
    let s = world.client("system.sessiond");
    s.publish(&SessionState {
        user: names.first().map(|n| (*n).to_owned()),
        signed_in: names.iter().map(|n| (*n).to_owned()).collect(),
    })
    .unwrap();
}

#[test]
fn state_actions_and_sleep_after_locking() {
    let s = setup("sleep", true, true);
    let mut daemon = start(&s);
    let shell = s.world.client("system.shell");
    let state = shell.subscribe::<PowerState>().unwrap();
    let actions = shell.subscribe::<PowerActions>().unwrap();
    let st = state.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (st.source, st.battery_percent),
        (PowerSource::Battery, Some(64))
    );
    let a = actions.recv_timeout(WAIT).unwrap();
    assert!(a.suspend && a.restart && a.power_off && !a.needs_admin);
    // Nobody signed in: no lock needed.
    assert_eq!(outcome(&shell, &Suspend {}), ActionOutcome::Done);
    assert_eq!(s.seen.lock().unwrap().locks, 0);
    assert_eq!(
        std::fs::read_to_string(s.root.join("sys/power/state")).unwrap(),
        "mem"
    );
    // Someone signed in: the screen is locked first; a lock screen that refuses stops the sleep.
    put(&s.root, "sys/power/state", "freeze mem\n");
    sessions(&s.world, &["amal"]);
    std::thread::sleep(Duration::from_millis(300));
    s.seen.lock().unwrap().lock_answer = false;
    assert_eq!(outcome(&shell, &Suspend {}), ActionOutcome::Failed);
    assert_eq!(
        std::fs::read_to_string(s.root.join("sys/power/state")).unwrap(),
        "freeze mem\n"
    );
    s.seen.lock().unwrap().lock_answer = true;
    assert_eq!(outcome(&shell, &Suspend {}), ActionOutcome::Done);
    assert_eq!(s.seen.lock().unwrap().locks, 2);
    assert_eq!(daemon.terminate(), Some(0));
}

#[test]
fn restart_and_power_off_go_through_init_and_ask_an_administrator_when_others_are_signed_in() {
    let s = setup("end", false, true);
    let mut daemon = start(&s);
    let shell = s.world.client("system.shell");
    assert_eq!(outcome(&shell, &Suspend {}), ActionOutcome::Unsupported);
    assert_eq!(outcome(&shell, &Restart {}), ActionOutcome::Done);
    assert_eq!(s.seen.lock().unwrap().init, vec![PowerAction::Reboot]);
    assert_eq!(s.seen.lock().unwrap().dialogs, 0);
    let actions = shell.subscribe::<PowerActions>().unwrap();
    sessions(&s.world, &["amal", "badr"]);
    assert!(wait_until(WAIT, || actions
        .try_recv()
        .ok()
        .flatten()
        .is_some_and(|a| a.needs_admin && !a.suspend)));
    assert_eq!(outcome(&shell, &PowerOff {}), ActionOutcome::Cancelled);
    assert_eq!(
        s.seen.lock().unwrap().init.len(),
        1,
        "init not asked after a cancelled dialog"
    );
    s.seen.lock().unwrap().dialog_answer = Some(AuthResult::Authorized);
    assert_eq!(outcome(&shell, &PowerOff {}), ActionOutcome::Done);
    assert_eq!(
        s.seen.lock().unwrap().init,
        vec![PowerAction::Reboot, PowerAction::PowerOff]
    );
    assert_eq!(s.seen.lock().unwrap().dialogs, 2);
    assert_eq!(daemon.terminate(), Some(0));
}

#[test]
fn apps_are_refused_and_a_missing_service_manager_is_a_failure() {
    let s = setup("refuse", true, false);
    let mut daemon = start(&s);
    let shell = s.world.client("system.shell");
    assert_eq!(outcome(&shell, &PowerOff {}), ActionOutcome::Failed);
    let app = s.world.client("app.sneaky");
    // The broker lets the app call power.off, the service refuses it.
    assert_eq!(outcome(&app, &PowerOff {}), ActionOutcome::Refused);
    // power.suspend is not even routed for it.
    assert!(matches!(
        app.call(&Suspend {}, Duration::from_secs(5)),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    assert_eq!(
        std::fs::read_to_string(s.root.join("sys/power/state")).unwrap(),
        "freeze mem\n"
    );
    assert_eq!(daemon.terminate(), Some(0));
}

#[test]
fn a_hanging_dialog_answers_busy_to_others_and_a_crash_leaves_nothing_half_done() {
    let s = setup("hang", false, true);
    s.seen.lock().unwrap().dialog_answer = None;
    let mut daemon = start(&s);
    sessions(&s.world, &["amal", "badr"]);
    let shell = s.world.client("system.shell");
    let _ = call(&shell, &ultar_bus::messages::power::Suspend {});
    std::thread::sleep(Duration::from_millis(300));
    let socket = s.world.socket.clone();
    let asking = std::thread::spawn(move || {
        let c = ultar_svckit::testkit::connect(&socket, "system.shell");
        c.call(&Restart {}, Duration::from_secs(30))
    });
    assert!(wait_until(WAIT, || s.seen.lock().unwrap().dialogs == 1));
    assert_eq!(outcome(&shell, &PowerOff {}), ActionOutcome::Busy);
    daemon.crash();
    assert!(
        asking.join().unwrap().is_err(),
        "the caller learns that the service ended"
    );
    assert!(s.seen.lock().unwrap().init.is_empty());
    let mut again = start(&s);
    s.seen.lock().unwrap().dialog_answer = Some(AuthResult::Authorized);
    sessions(&s.world, &["amal", "badr"]);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(outcome(&shell, &Restart {}), ActionOutcome::Done);
    assert_eq!(again.terminate(), Some(0));
}
