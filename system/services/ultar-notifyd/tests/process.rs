//! The real `ultar-notifyd`: apps post with buttons, the shell presses them, only the
//! sender gets its presses back.

use std::time::Duration;
use ultar_bus::messages::notify::{
    Action, Body, Category, Dismiss, Dismissed, Invoke, InvokedHint, List, NoteOutcome, Post,
    Posted, TakeInvoked, Urgency,
};
use ultar_bus::{BusError, Call, Client, ErrorCode, Text};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.notifyd"
register = ["notify"]
publish = ["notify.posted", "notify.dismissed", "notify.invoked"]
subscribe = ["mesh.note", "session.state"]
call = ["mesh.note_mirror"]

[[rule]]
identity = "system.shell"
call = ["notify.invoke", "notify.dismiss", "notify.list", "notify.system"]
subscribe = ["notify.posted", "notify.dismissed"]

[[rule]]
identity = "app.*"
call = ["notify.post", "notify.dismiss", "notify.take_invoked", "notify.invoke", "notify.list"]
subscribe = ["notify.invoked"]
"#;

fn start(world: &World) -> Daemon {
    let policy = world.policy_file();
    world.spawn(
        env!("CARGO_BIN_EXE_ultar-notifyd"),
        "system.notifyd",
        &["--policy", policy.to_str().unwrap()],
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

fn note(app: &str, buttons: &[&str]) -> Post {
    Post {
        urgency: Urgency::Normal,
        category: Category::Message,
        app_id: app.into(),
        title: Text::key("chat.new_message"),
        body: Body::Verbatim("مرحبا".into()),
        actions: buttons
            .iter()
            .map(|b| Action {
                id: (*b).into(),
                label: Text::key("chat.action.reply"),
            })
            .collect(),
    }
}

#[test]
fn buttons_reach_only_their_sender() {
    let world = World::new(
        "notes",
        POLICY,
        &["system.notifyd", "system.shell", "app.chat", "app.spy"],
    );
    let mut daemon = start(&world);
    let shell = world.client("system.shell");
    let chat = world.client("app.chat");
    let spy = world.client("app.spy");
    let posted = shell.subscribe::<Posted>().unwrap();
    let dismissed = shell.subscribe::<Dismissed>().unwrap();
    let hints = chat.subscribe::<InvokedHint>().unwrap();
    let id = call(&chat, &note("tech.code-craft.chat", &["reply", "mute"]))
        .unwrap()
        .id;
    let seen = posted.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (seen.id, seen.sender.as_str(), seen.post.actions.len()),
        (id, "app.chat", 2)
    );
    spy_is_refused(&spy, id);
    assert_eq!(call(&shell, &List {}).unwrap().notes.len(), 1);
    assert_eq!(
        call(
            &shell,
            &Invoke {
                id,
                action: "nope".into()
            }
        )
        .unwrap()
        .outcome,
        NoteOutcome::Unknown
    );
    assert_eq!(
        call(
            &shell,
            &Invoke {
                id,
                action: "reply".into()
            }
        )
        .unwrap()
        .outcome,
        NoteOutcome::Done
    );
    assert_eq!(dismissed.recv_timeout(WAIT).unwrap().id, id);
    assert_eq!(hints.recv_timeout(WAIT).unwrap().sender, "app.chat");
    assert!(call(&spy, &TakeInvoked {}).unwrap().invoked.is_empty());
    let got = call(&chat, &TakeInvoked {}).unwrap().invoked;
    assert_eq!(
        (got.len(), got[0].id, got[0].action.as_str()),
        (1, id, "reply")
    );
    withdraw_and_close(&chat, &spy, &shell);
    assert_eq!(daemon.terminate(), Some(0));
}

fn withdraw_and_close(chat: &Client, spy: &Client, shell: &Client) {
    // The sender withdraws its own; the shell closes any.
    let a = call(chat, &note("x", &[])).unwrap().id;
    let b = call(spy, &note("y", &[])).unwrap().id;
    assert_eq!(
        call(chat, &Dismiss { id: a }).unwrap().outcome,
        NoteOutcome::Done
    );
    assert_eq!(
        call(shell, &Dismiss { id: b }).unwrap().outcome,
        NoteOutcome::Done
    );
    assert!(call(shell, &List {}).unwrap().notes.is_empty());
}

#[test]
fn bad_and_flooding_posts_are_refused() {
    let world = World::new(
        "flood",
        POLICY,
        &["system.notifyd", "system.shell", "app.flood"],
    );
    let mut daemon = start(&world);
    let app = world.client("app.flood");
    let bad = note("x", &["a", "b", "c", "d"]);
    assert!(matches!(
        call(&app, &bad),
        Err(BusError::Remote(ErrorCode::BadPayload))
    ));
    let mut ok = 0;
    let mut limited = false;
    for _ in 0..40 {
        match call(&app, &note("x", &[])) {
            Ok(_) => ok += 1,
            Err(BusError::Remote(ErrorCode::LimitExceeded)) => limited = true,
            Err(e) => panic!("{e}"),
        }
    }
    assert!(limited && ok == 30, "{ok}");
    // The service is still well and the shell sees the accepted ones.
    let shell = world.client("system.shell");
    assert_eq!(call(&shell, &List {}).unwrap().notes.len(), 30);
    // A crash empties the open list (documented); the restarted service answers.
    daemon.crash();
    let mut again = start(&world);
    assert!(call(&shell, &List {}).unwrap().notes.is_empty());
    assert_eq!(again.terminate(), Some(0));
}

fn spy_is_refused(spy: &Client, id: u64) {
    // The spy may call invoke and list (broker), the service refuses both.
    assert_eq!(
        call(
            spy,
            &Invoke {
                id,
                action: "reply".into()
            }
        )
        .unwrap()
        .outcome,
        NoteOutcome::Refused
    );
    assert!(matches!(
        spy.call(&List {}, WAIT),
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
    assert_eq!(
        call(spy, &Dismiss { id }).unwrap().outcome,
        NoteOutcome::Refused
    );
}
