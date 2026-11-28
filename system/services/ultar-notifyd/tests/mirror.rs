//! Notifications between the person's devices with the real `ultar-notifyd`: what leaves
//! this device (never a private one), and what arrives from another device (named by its
//! device, nothing on a locked one that asked for that).

use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::mesh::Done;
use ultar_bus::messages::mesh_calls::{NoteMirror, NoteRemote};
use ultar_bus::messages::notify::{Body, Category, Post, PostReply, Posted, Urgency};
use ultar_bus::messages::notify_mirror::{PostEx, Privacy};
use ultar_bus::messages::session::SessionState;
use ultar_bus::{BusError, Call, Client, ErrorCode, Text, TextArg};
use ultar_svckit::testkit::{wait_until, Daemon, World, WAIT};

const POLICY: &str = r#"
[[rule]]
identity = "system.notifyd"
register = ["notify"]
publish = ["notify.posted", "notify.dismissed", "notify.invoked"]
call = ["mesh.note_mirror"]
subscribe = ["mesh.note", "session.state"]

[[rule]]
identity = "system.mesh"
register = ["mesh"]
publish = ["mesh.note"]

[[rule]]
identity = "system.session"
publish = ["session.state"]

[[rule]]
identity = "system.shell"
subscribe = ["notify.posted"]

[[rule]]
identity = "app.*"
call = ["notify.post", "notify.post_ex"]
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

fn sms(who: &str, text: &str) -> Post {
    Post {
        urgency: Urgency::Normal,
        category: Category::Sms,
        app_id: "tech.code-craft.phone".into(),
        title: Text::key("notify.sms.title").with(TextArg::Verbatim(who.into())),
        body: Body::Verbatim(text.into()),
        actions: Vec::new(),
    }
}

/// The stand-in mesh service: records what notifyd asks it to mirror.
fn mesh(world: &World) -> Arc<Mutex<Vec<NoteMirror>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    world.serve("system.mesh", "mesh", move |request| {
        let record = record.clone();
        let _ = request.respond::<NoteMirror>(|c| {
            record.lock().unwrap().push(c);
            Ok(Done { ok: true })
        });
    });
    seen
}

fn remote(hide: bool, title: &str, text: &str) -> NoteRemote {
    NoteRemote {
        from: "phone".into(),
        from_name: "Anas phone".into(),
        title: title.into(),
        text: text.into(),
        app: "tech.code-craft.phone".into(),
        urgency: Urgency::High,
        hide_on_locked: hide,
    }
}

#[test]
fn what_leaves_is_what_may_leave() {
    let world = World::new(
        "mirror-out",
        POLICY,
        &["system.notifyd", "system.mesh", "app.phone"],
    );
    let seen = mesh(&world);
    let mut daemon = start(&world);
    let phone = world.client("app.phone");
    call(&phone, &sms("Samer", "first")).unwrap();
    // A private one is posted (shown here) and never reported to the mesh.
    let private = PostEx {
        post: sms("Bank", "code 123456"),
        privacy: Privacy::Private,
    };
    assert!(call(&phone, &private).is_ok(), "it is still shown here");
    let hidden = PostEx {
        post: sms("Layla", "second"),
        privacy: Privacy::HideOnLocked,
    };
    call(&phone, &hidden).unwrap();
    // A system notice stays home as well.
    let mut battery = sms("", "battery low");
    battery.category = Category::System;
    call(&phone, &battery).unwrap();
    let last = sms("Nour", "third");
    call(&phone, &last).unwrap();
    assert!(wait_until(WAIT, || seen.lock().unwrap().len() >= 3));
    let mut got = seen.lock().unwrap().clone();
    got.sort_by(|a, b| a.text.cmp(&b.text));
    let texts: Vec<_> = got.iter().map(|n| n.text.as_str()).collect();
    assert_eq!(texts, ["first", "second", "third"]);
    let layla = got.iter().find(|n| n.title == "Layla").unwrap();
    assert!(layla.hide_on_locked && !layla.confidential);
    assert!(!format!("{got:?}").contains("123456"), "no private content");
    assert_eq!(daemon.terminate(), Some(0));
}

#[test]
fn what_arrives_names_its_device_and_respects_a_locked_screen() {
    let world = World::new(
        "mirror-in",
        POLICY,
        &["system.notifyd", "system.mesh", "system.session", "system.shell"],
    );
    let mut daemon = start(&world);
    let shell = world.client("system.shell");
    let posted = shell.subscribe::<Posted>().unwrap();
    let from_mesh = world.client("system.mesh");
    // Nobody is signed in yet: the device is locked. The first note asks to be hidden, the
    // second only to be shown without content.
    let publish_until_posted = |notes: &[NoteRemote]| {
        let mut seen = None;
        wait_until(WAIT, || {
            for n in notes {
                from_mesh.publish(n).unwrap();
            }
            seen = posted.recv_timeout(Duration::from_millis(300)).ok();
            seen.is_some()
        });
        seen.unwrap()
    };
    let locked = publish_until_posted(&[
        remote(true, "Samer", "secret"),
        remote(false, "Samer", "secret"),
    ]);
    assert_eq!(locked.sender, "mesh");
    assert_eq!(locked.post.title.key, "desk.note.mirror.locked");
    assert_eq!(locked.post.title.args, [TextArg::Verbatim("Anas phone".into())]);
    assert_eq!(locked.post.body, Body::Empty);
    assert!(!format!("{locked:?}").contains("secret"));
    assert_eq!(locked.post.urgency, Urgency::Normal, "it cannot alert");
    // The hidden one never shows up, even later.
    while let Ok(p) = posted.recv_timeout(Duration::from_millis(300)) {
        assert_eq!(p.post.title.key, "desk.note.mirror.locked");
    }

    // Signed in: the content appears, with the device named.
    let session = world.client("system.session");
    session
        .publish(&SessionState {
            user: Some("anas".into()),
            signed_in: vec!["anas".into()],
        })
        .unwrap();
    let mut shown = None;
    wait_until(WAIT, || {
        from_mesh.publish(&remote(true, "Samer", "hello")).unwrap();
        while let Ok(p) = posted.recv_timeout(Duration::from_millis(300)) {
            if p.post.title.key == "desk.note.mirror.title" {
                shown = Some(p);
            }
        }
        shown.is_some()
    });
    let shown = shown.unwrap();
    assert_eq!(
        shown.post.title.args,
        [
            TextArg::Verbatim("Anas phone".into()),
            TextArg::Verbatim("Samer".into())
        ]
    );
    assert_eq!(shown.post.body, Body::Verbatim("hello".into()));
    let _ = PostReply { id: shown.id };
    assert_eq!(daemon.terminate(), Some(0));
}
