//! Every `mesh.*` entry of docs/ui/CLICKS.md on the desktop: the choice dialog, the toasts'
//! buttons, the list of the Mesh tile and the question on the receiving side.

mod common;

use common::*;
use ultar_bus::messages::mesh::{
    DeviceKind, HandoffProgress, MeshState, OfferState, Peer, SendOutcome, Stage, Target as Device,
};
use ultar_bus::messages::mesh_dialog::AskHandoff;
use ultar_desktop::dialog::{Answer, DialogEffect, Dialogs, Question};
use ultar_desktop::model::mesh::{self, Act, Kind};
use ultar_desktop::model::services::Call;
use ultar_desktop::model::Open;
use ultar_desktop::shell::Effect;
use ultar_desktop::view::{Look, Target};
use ultar_ui::event::{ActionKind, Event, PointerButton};

fn peer(id: &str, name: &str, reachable: bool) -> Peer {
    Peer {
        id: id.into(),
        name: name.into(),
        kind: DeviceKind::Phone,
        capabilities: Vec::new(),
        paired: true,
        reachable,
        rights: Vec::new(),
    }
}

fn harness() -> Harness {
    let mut h = Harness::new(ultar_i18n::Lang::En, ultar_ui::tokens::ThemeKind::Dark);
    h.shell.d.svc.mesh.state = Some(MeshState {
        enabled: true,
        id: "me".into(),
        name: "pc".into(),
        peers: vec![peer("km-one", "KM One", true), peer("far", "Far", false)],
    });
    h
}

fn device(id: &str, name: &str) -> Device {
    Device {
        device: id.into(),
        name: name.into(),
        kind: DeviceKind::Phone,
        stage: Stage::State,
    }
}

fn calls(effects: &[Effect]) -> Vec<Call> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Service(c) => Some(c.clone()),
            _ => None,
        })
        .collect()
}

/// mesh.send: the shortcut and the menu entry hand over what is in front.
#[test]
fn mesh_send_hands_over_what_is_in_front() {
    let mut h = harness();
    let out = h.shell.on_shell_key_with(50, true);
    assert!(matches!(
        calls(&out.effects).as_slice(),
        [Call::MeshSend(None, Some(_))]
    ));
}

/// mesh.send.target and mesh.send.cancel: the choice dialog.
#[test]
fn the_choice_dialog_sends_to_the_chosen_device_or_cancels() {
    let mut h = harness();
    let s = &mut h.shell;
    let targets = vec![device("km-one", "KM One"), device("beelink", "beelink")];
    mesh::sent(&mut s.d, SendOutcome::Choose(targets.clone()));
    assert_eq!(s.d.open, Open::MeshSend);
    // the second device, then Send
    let chosen = s.on_popup(&Target::MeshTargets, &ActionKind::Selected(1));
    assert!(chosen.popup, "the dialog is built again with the choice");
    let out = s.on_popup(&Target::MeshSend, &ActionKind::Activated);
    match calls(&out.effects).as_slice() {
        [Call::MeshSend(Some(id), _)] => assert_eq!(id, "beelink"),
        other => panic!("{other:?}"),
    }
    assert!(s.d.svc.mesh.targets.is_empty());
    // Cancel: nothing is sent and the dialog is gone
    mesh::sent(&mut s.d, SendOutcome::Choose(targets));
    let out = s.on_popup(&Target::MeshSendCancel, &ActionKind::Activated);
    assert!(calls(&out.effects).is_empty());
    assert_ne!(s.d.open, Open::MeshSend);
}

/// mesh.offer.cancel: the button of the waiting toast takes the offer back.
#[test]
fn the_waiting_toast_takes_the_offer_back() {
    let mut h = harness();
    let s = &mut h.shell;
    mesh::sent(
        &mut s.d,
        SendOutcome::Offered {
            offer: 7,
            target: device("km-one", "KM One"),
        },
    );
    let toast = s.d.svc.mesh.toasts[0].clone();
    assert_eq!(toast.kind, Kind::Waiting);
    let act = s.d.svc.mesh.take_act(toast.id).expect("a button");
    assert_eq!(act, Act::Cancel(7));
    let out = s.on_mesh_toast(act);
    assert_eq!(calls(&out.effects), vec![Call::OfferCancel(7)]);
    assert!(s.d.svc.mesh.toasts.is_empty());
}

/// mesh.retry: an offer nobody answered is sent again with one click.
#[test]
fn an_expired_offer_is_sent_again() {
    let mut h = harness();
    let s = &mut h.shell;
    for state in [OfferState::Expired, OfferState::Failed] {
        mesh::report(
            &mut s.d,
            &[HandoffProgress {
                offer: 9,
                device: "km-one".into(),
                outgoing: true,
                state,
            }],
        );
        let toast = s.d.svc.mesh.toasts.last().cloned().expect("a toast");
        let act = s.d.svc.mesh.take_act(toast.id).expect("Retry");
        assert_eq!(act, Act::Retry("km-one".into()));
        let out = s.on_mesh_toast(act);
        assert!(matches!(
            calls(&out.effects).as_slice(),
            [Call::MeshSend(Some(id), _)] if id == "km-one"
        ));
    }
}

/// mesh.received.open: what arrived opens in Files.
#[test]
fn what_arrived_opens_in_files() {
    let mut h = harness();
    let s = &mut h.shell;
    s.d.svc
        .mesh
        .received("KM One", "plan.txt", Some("file:///inbox/plan.txt".into()));
    let toast = s.d.svc.mesh.toasts[0].clone();
    assert_eq!(toast.kind, Kind::Received);
    let act = s.d.svc.mesh.take_act(toast.id).expect("Open");
    let out = s.on_mesh_toast(act);
    match out.effects.as_slice() {
        [Effect::Launch { app, open }] => {
            assert_eq!(app, "org.ultar.files");
            assert_eq!(open.uri.as_deref(), Some("file:///inbox/plan.txt"));
        }
        other => panic!("{other:?}"),
    }
}

/// qs.mesh.list: a connected device gets what is in front, one that is away opens Settings.
#[test]
fn the_tile_list_sends_or_opens_settings() {
    let mut h = harness();
    let s = &mut h.shell;
    let out = s.on_surface(&Target::MeshDevice("km-one".into()), &ActionKind::Activated);
    assert!(matches!(
        calls(&out.effects).as_slice(),
        [Call::MeshSend(Some(id), _)] if id == "km-one"
    ));
    let out = s.on_surface(&Target::MeshDevice("far".into()), &ActionKind::Activated);
    assert!(calls(&out.effects).is_empty());
    assert!(
        out.effects
            .iter()
            .any(|e| matches!(e, Effect::Launch { open, .. } if open.uri.as_deref() == Some("settings://mesh"))),
        "{:?}",
        out.effects
    );
}

/// mesh.accept and mesh.decline: the two buttons of the question on the receiving device.
#[test]
fn the_question_is_answered_with_accept_or_decline() {
    for (target, accepted) in [(Target::AllowOnce, true), (Target::Deny, false)] {
        let mut dialogs = Dialogs::default();
        dialogs.ask(
            1,
            Question::Handoff(AskHandoff {
                from_name: "KM One".into(),
                from_kind: DeviceKind::Phone,
                title: "notes.txt".into(),
                app: "org.ultar.editor".into(),
                size: None,
                stage: Stage::State,
            }),
            0,
        );
        let tr = ultar_desktop::i18n::translator(ultar_i18n::Lang::Ar).expect("catalogs");
        let look = Look::new(ultar_i18n::Lang::Ar, ultar_ui::tokens::ThemeKind::Dark);
        let mut view = ultar_desktop::dialog::view::build(&dialogs, 0, look, &tr)
            .expect("builds")
            .expect("a question");
        view.ui.layout();
        let node = view.node(&target).expect("button");
        let pos = view.ui.rect(node).expect("laid out").center();
        let mut actions = Vec::new();
        for event in [
            Event::PointerMove { pos },
            Event::PointerDown {
                pos,
                button: PointerButton::Primary,
            },
            Event::PointerUp {
                pos,
                button: PointerButton::Primary,
            },
        ] {
            actions.extend(view.ui.handle(event).actions);
        }
        assert!(
            actions
                .iter()
                .any(|a| view.target(a.node) == Some(&target) && a.kind == ActionKind::Activated),
            "{target:?} was not pressed"
        );
        // what the compositor does with the two buttons (app/sysdialog.rs)
        let effects = if accepted {
            dialogs.answer(Answer::Handoff(true))
        } else {
            dialogs.dismiss()
        };
        assert_eq!(
            effects,
            vec![DialogEffect::Reply(1, Answer::Handoff(accepted))]
        );
    }
}
