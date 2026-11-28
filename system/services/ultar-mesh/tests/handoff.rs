//! Handing an activity over: asked on the receiver, chosen on the sender.
mod common;
use common::World;
use ultar_bus::messages::mesh::{
    Activity, DeviceKind, HandoffProgress, OfferState, Right, SendOutcome, Stage,
};
use ultar_mesh::node::Effect;

const PHONE: (DeviceKind, &[&str]) = (DeviceKind::Phone, &["editor", "browser"]);
const PC: (DeviceKind, &[&str]) = (DeviceKind::Desktop, &["editor", "browser"]);
const BARE: (DeviceKind, &[&str]) = (DeviceKind::Desktop, &[]);
const TV: (DeviceKind, &[&str]) = (DeviceKind::Tv, &[]);
const SPEAKER: (DeviceKind, &[&str]) = (DeviceKind::Speaker, &[]);

fn meshed(tag: &str, kinds: &[(DeviceKind, &[&str])]) -> World {
    let mut w = World::new(tag, kinds);
    for other in 1..kinds.len() {
        w.link(0, other);
        w.pair(0, other, None);
    }
    for i in 0..kinds.len() {
        w.take(i);
    }
    w
}

fn editing(w: &World, content: &[u8]) -> Activity {
    let file = w.file("notes.txt", content);
    Activity {
        app: "editor".into(),
        title: "notes.txt".into(),
        uri: Some(format!("file://{file}#line=12")),
        file: Some(file),
    }
}

fn states(effects: &[Effect]) -> Vec<OfferState> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Progress(HandoffProgress { state, .. }) => Some(*state),
            _ => None,
        })
        .collect()
}

fn incoming(effects: &[Effect]) -> Option<(u64, Stage, String)> {
    effects.iter().find_map(|e| match e {
        Effect::Incoming(i) => Some((i.offer, i.stage, i.from_name.clone())),
        _ => None,
    })
}

fn open(effects: &[Effect]) -> Option<(u64, String, Option<String>)> {
    effects.iter().find_map(|e| match e {
        Effect::Open { offer, app, uri } => Some((*offer, app.clone(), uri.clone())),
        _ => None,
    })
}

#[test]
fn an_activity_continues_on_the_other_device_only_after_it_was_accepted() {
    let mut w = meshed("ho-state", &[PHONE, PC]);
    let content: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    let activity = editing(&w, &content);
    w.nodes[0].set_activity(Some(activity));
    let (outcome, effects) = w.nodes[0].send(None, w.now);
    assert!(
        matches!(outcome, SendOutcome::Offered { .. }),
        "{outcome:?}"
    );
    w.run(0, effects);
    // Asked on the receiver; nothing was written, nothing was opened.
    let got = w.take(1);
    let (offer, stage, from) = incoming(&got).unwrap();
    assert_eq!((stage, from.as_str()), (Stage::State, "dev0"));
    assert!(open(&got).is_none());
    assert!(!w.dir.join("inbox1").exists());
    assert_eq!(states(&w.take(0)), vec![OfferState::Asked]);
    // Accepted: the file travels, the same app opens at the same place.
    let (ok, effects) = w.nodes[1].answer(offer, true, w.now);
    assert!(ok);
    w.run(1, effects);
    let got = w.take(1);
    let (offer, app, uri) = open(&got).unwrap();
    let arrived = w.dir.join("inbox1").join("notes.txt");
    assert_eq!(app, "editor");
    assert_eq!(
        uri.unwrap(),
        format!("file://{}#line=12", arrived.display())
    );
    assert_eq!(std::fs::read(&arrived).unwrap(), content);
    let effects = w.nodes[1].opened(offer, true);
    w.run(1, effects);
    assert_eq!(
        states(&w.take(0)),
        vec![OfferState::Accepted, OfferState::Opened]
    );
    assert_eq!(states(&w.take(1)), vec![OfferState::Opened]);
}

#[test]
fn with_several_devices_the_sender_is_asked_which_one() {
    let mut w = meshed("ho-choose", &[PHONE, PC, PC, SPEAKER]);
    w.nodes[0].set_activity(Some(Activity {
        app: "browser".into(),
        title: "page".into(),
        uri: Some("https://example.org/a".into()),
        file: None,
    }));
    let (outcome, effects) = w.nodes[0].send(None, w.now);
    let SendOutcome::Choose(targets) = outcome else {
        panic!("{outcome:?}");
    };
    // The two computers – the speaker cannot show a page and is not offered.
    assert_eq!(targets.len(), 2);
    assert!(effects.is_empty());
    assert!(w.take(1).is_empty() && w.take(2).is_empty());
    let chosen = w.id(2);
    let (outcome, effects) = w.nodes[0].send(Some(&chosen), w.now);
    assert!(matches!(outcome, SendOutcome::Offered { .. }));
    w.run(0, effects);
    assert!(w.take(1).is_empty());
    let (offer, _, _) = incoming(&w.take(2)).unwrap();
    let (_, effects) = w.nodes[2].answer(offer, true, w.now);
    w.run(2, effects);
    let (_, app, uri) = open(&w.take(2)).unwrap();
    assert_eq!(
        (app.as_str(), uri.as_deref()),
        ("browser", Some("https://example.org/a"))
    );
}

#[test]
fn declined_and_unanswered_offers_open_nothing() {
    let mut w = meshed("ho-no", &[PHONE, PC]);
    let activity = editing(&w, b"text");
    w.nodes[0].set_activity(Some(activity));
    let (_, effects) = w.nodes[0].send(None, w.now);
    w.run(0, effects);
    let (offer, _, _) = incoming(&w.take(1)).unwrap();
    let (_, effects) = w.nodes[1].answer(offer, false, w.now);
    w.run(1, effects);
    assert_eq!(
        states(&w.take(0)),
        vec![OfferState::Asked, OfferState::Declined]
    );
    assert!(!w.nodes[1].answer(offer, true, w.now).0);
    // Unanswered: both sides give up after a minute.
    let (_, effects) = w.nodes[0].send(None, w.now);
    w.run(0, effects);
    let (offer, _, _) = incoming(&w.take(1)).unwrap();
    w.now += std::time::Duration::from_secs(61);
    let effects = w.nodes[0].tick(w.now);
    w.run(0, effects);
    assert_eq!(
        states(&w.take(0)),
        vec![OfferState::Asked, OfferState::Expired]
    );
    assert_eq!(states(&w.take(1)), vec![OfferState::Expired]);
    assert!(!w.nodes[1].answer(offer, true, w.now).0);
    assert!(!w.dir.join("inbox1").exists());
}

#[test]
fn without_the_app_the_file_arrives_and_opens_with_the_default_app() {
    let mut w = meshed("ho-file", &[PHONE, BARE]);
    let activity = editing(&w, b"plain text");
    w.nodes[0].set_activity(Some(activity));
    let (outcome, effects) = w.nodes[0].send(None, w.now);
    let SendOutcome::Offered { target, .. } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(target.stage, Stage::File);
    w.run(0, effects);
    let (offer, _, _) = incoming(&w.take(1)).unwrap();
    let (_, effects) = w.nodes[1].answer(offer, true, w.now);
    w.run(1, effects);
    let (_, app, uri) = open(&w.take(1)).unwrap();
    assert_eq!(app, "");
    assert!(uri.unwrap().ends_with("/inbox1/notes.txt"));
}

#[test]
fn a_tv_and_a_speaker_are_offered_only_what_they_can_take() {
    let mut w = meshed("ho-tv", &[PHONE, TV, SPEAKER]);
    let activity = editing(&w, b"text");
    w.nodes[0].set_activity(Some(activity));
    assert!(w.nodes[0].targets().is_empty());
    assert_eq!(w.nodes[0].send(None, w.now).0, SendOutcome::NoTarget);
    // A plain file to a speaker is refused before anything is sent.
    let speaker = w.id(2);
    let path = w.file("song.txt", b"x");
    assert_eq!(
        w.nodes[0].send_file(&speaker, &path, w.now).0,
        SendOutcome::NoTarget
    );
    w.nodes[0].set_activity(None);
    assert_eq!(w.nodes[0].send(None, w.now).0, SendOutcome::NoActivity);
}

#[test]
fn a_device_whose_right_was_taken_away_cannot_even_ask() {
    let mut w = meshed("ho-right", &[PHONE, PC]);
    let phone = w.id(0);
    assert!(w.nodes[1].set_rights(&phone, &[Right::Clipboard]).0);
    w.take(1);
    let activity = editing(&w, b"text");
    w.nodes[0].set_activity(Some(activity));
    let (_, effects) = w.nodes[0].send(None, w.now);
    w.run(0, effects);
    assert!(incoming(&w.take(1)).is_none());
    assert_eq!(
        states(&w.take(0)),
        vec![OfferState::Asked, OfferState::Declined]
    );
}

#[test]
fn a_lost_connection_fails_the_transfer_and_leaves_no_half_file() {
    let mut w = meshed("ho-lost", &[PHONE, PC]);
    let activity = editing(&w, &vec![7u8; 200_000]);
    w.nodes[0].set_activity(Some(activity));
    let (_, effects) = w.nodes[0].send(None, w.now);
    w.run(0, effects);
    let (offer, _, _) = incoming(&w.take(1)).unwrap();
    // The pieces from the phone get lost after the first.
    w.cut = Some((1, 0));
    let (_, effects) = w.nodes[1].answer(offer, true, w.now);
    w.run(1, effects);
    w.cut = None;
    let phone = w.id(0);
    let effects = w.nodes[1].link_down(&phone);
    w.run(1, effects);
    assert!(states(&w.take(1)).contains(&OfferState::Failed));
    let left: Vec<_> = std::fs::read_dir(w.dir.join("inbox1")).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
    assert!(open(&w.take(1)).is_none());
}

#[test]
fn one_device_cannot_flood_another_with_questions() {
    let mut w = meshed("ho-flood", &[PHONE, PC]);
    w.nodes[0].set_activity(Some(Activity {
        app: "browser".into(),
        title: "page".into(),
        uri: Some("https://example.org".into()),
        file: None,
    }));
    for _ in 0..10 {
        let (_, effects) = w.nodes[0].send(None, w.now);
        w.run(0, effects);
    }
    let asked = w
        .take(1)
        .iter()
        .filter(|e| matches!(e, Effect::Incoming(_)))
        .count();
    assert_eq!(asked, ultar_mesh::offers::MAX_PENDING);
}

#[test]
fn copied_text_reaches_paired_devices_but_never_a_password() {
    let mut w = meshed("clip", &[PHONE, PC, SPEAKER]);
    // "Share the clipboard" is off for a newly paired device: nothing leaves, nothing arrives.
    assert!(w.nodes[0].clipboard_set("hello", false).is_empty());
    let (phone, pc, speaker) = (w.id(0), w.id(1), w.id(2));
    let all = [Right::Handoff, Right::Files, Right::Clipboard];
    assert!(w.nodes[0].set_rights(&pc, &all).0);
    assert!(w.nodes[0].set_rights(&speaker, &all).0);
    assert!(w.nodes[1].set_rights(&phone, &all).0);
    let effects = w.nodes[0].clipboard_set("hello", false);
    // The speaker has no clipboard: one frame only.
    assert_eq!(effects.len(), 1);
    w.run(0, effects);
    let got = w.take(1);
    assert!(matches!(&got[0], Effect::Clipboard(c) if c.text == "hello"));
    // The receiver's clipboard owner sets it – that must not bounce back.
    assert!(w.nodes[1].clipboard_set("hello", false).is_empty());
    assert!(w.nodes[0].clipboard_set("hunter2", true).is_empty());
    w.nodes[1].set_rights(&phone, &[Right::Handoff]);
    w.take(1);
    let effects = w.nodes[0].clipboard_set("again", false);
    w.run(0, effects);
    assert!(w.take(1).is_empty());
}

#[test]
fn the_app_in_front_is_offered_with_what_it_reported_itself() {
    let mut w = World::new("front", &[PHONE, PC]);
    // surfaces name an app org.ultar.<id>; the launcher and the app's bus name say <id>
    let place = |app: &str| Activity {
        app: app.into(),
        title: "Documents".into(),
        uri: Some("files://documents/".into()),
        file: None,
    };
    let bare = |app: &str| Activity {
        app: app.into(),
        title: "window".into(),
        uri: None,
        file: None,
    };
    let node = &mut w.nodes[0];
    // an app cannot report for another one
    assert!(!node.report("web", Some(place("org.ultar.files"))));
    assert_eq!(node.in_front(), None);
    assert!(node.report("files", Some(place("org.ultar.files"))));
    assert_eq!(node.in_front(), Some(&place("files")));
    // the shell names another app in front: that one reported nothing
    node.set_front(Some(bare("org.ultar.web")));
    assert_eq!(node.in_front(), Some(&bare("web")));
    // back to Files: its own report counts, not the bare window
    node.set_front(Some(bare("org.ultar.files")));
    assert_eq!(node.in_front(), Some(&place("files")));
    assert!(node.report("files", None));
    assert_eq!(node.in_front(), None);
    node.set_front(Some(bare("org.ultar.files")));
    assert_eq!(node.in_front(), Some(&bare("files")));
}
