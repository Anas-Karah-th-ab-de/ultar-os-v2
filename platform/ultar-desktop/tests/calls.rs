//! Calls of the person's other devices on the desktop (mesh item C2): the banner of an
//! incoming call, its answer and decline, the controls of the call in progress, and the
//! mirrored notifications naming their device - in Arabic, English and German, dark and
//! light.

mod common;

use common::shots::*;
use common::*;
use ultar_bus::messages::mesh_calls::{CallIncoming, CallPhase, CallProgress, MeshCallState};
use ultar_bus::messages::notify::{Body, Category, Post, Posted, Urgency};
use ultar_bus::{Text, TextArg};
use ultar_desktop::model::calls;
use ultar_desktop::model::services::Call;
use ultar_desktop::shell::Effect;
use ultar_i18n::Lang;
use ultar_ui::tokens::ThemeKind;

fn offer(id: u64) -> CallIncoming {
    CallIncoming {
        id,
        from: "phone".into(),
        from_name: "KM One".into(),
        caller: "Samer".into(),
        video: false,
    }
}

fn led(id: u64, muted: bool) -> MeshCallState {
    MeshCallState {
        id: Some(id),
        device: "phone".into(),
        muted,
    }
}

fn services(effects: &[Effect]) -> Vec<Call> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Service(call) => Some(call.clone()),
            _ => None,
        })
        .collect()
}

/// The button `action` of the notice `id`.
fn index(h: &Harness, id: u64, action: &str) -> u8 {
    let note = h.shell.d.notes.list().iter().find(|n| n.id == id).unwrap();
    note.actions.iter().position(|a| a.id == action).unwrap() as u8
}

fn ring_id(h: &Harness) -> u64 {
    h.shell
        .d
        .notes
        .list()
        .iter()
        .find(|n| n.title.key == "desk.call.incoming")
        .unwrap()
        .id
}

#[test]
fn a_call_rings_is_answered_led_muted_and_hung_up_from_the_desktop() {
    let mut h = Harness::new(Lang::En, ThemeKind::Dark);
    calls::incoming(&mut h.shell.d, offer(5), BOARD_MS);
    assert_eq!(h.shell.d.notes.banners(BOARD_MS).len(), 1, "a banner rings");
    let id = ring_id(&h);
    let answer = h.shell.note_action(id, index(&h, id, "answer"));
    assert_eq!(services(&answer.effects), [Call::CallAnswer(5, true)]);
    assert!(
        h.shell.d.notes.banners(BOARD_MS).is_empty(),
        "answered: no ring"
    );

    // The call service says it leads the call: the controls appear.
    calls::led(&mut h.shell.d, &led(5, false), BOARD_MS);
    let controls = h.shell.d.notes.banners(BOARD_MS);
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].urgency, Urgency::High, "it stays");
    let mute = h
        .shell
        .note_action(calls::ACTIVE, index(&h, calls::ACTIVE, "mute"));
    assert_eq!(services(&mute.effects), [Call::CallMute(true)]);
    calls::led(&mut h.shell.d, &led(5, true), BOARD_MS);
    let unmute = h
        .shell
        .note_action(calls::ACTIVE, index(&h, calls::ACTIVE, "unmute"));
    assert_eq!(services(&unmute.effects), [Call::CallMute(false)]);
    let hangup = h
        .shell
        .note_action(calls::ACTIVE, index(&h, calls::ACTIVE, "hangup"));
    assert_eq!(services(&hangup.effects), [Call::CallEnd(5)]);

    // The other end hangs up: the controls go.
    let ended = CallProgress {
        id: 5,
        device: "phone".into(),
        phase: CallPhase::Ended,
    };
    calls::progress(&mut h.shell.d, &ended);
    assert!(h.shell.d.notes.banners(BOARD_MS).is_empty());
}

#[test]
fn declining_and_a_call_taken_elsewhere_end_the_ring() {
    let mut h = Harness::new(Lang::Ar, ThemeKind::Light);
    calls::incoming(&mut h.shell.d, offer(1), BOARD_MS);
    let id = ring_id(&h);
    let decline = h.shell.note_action(id, index(&h, id, "decline"));
    assert_eq!(services(&decline.effects), [Call::CallAnswer(1, false)]);
    assert!(h.shell.d.notes.banners(BOARD_MS).is_empty());
    calls::incoming(&mut h.shell.d, offer(2), BOARD_MS);
    let taken = CallProgress {
        id: 2,
        device: "phone".into(),
        phase: CallPhase::Ended,
    };
    calls::progress(&mut h.shell.d, &taken);
    assert!(h.shell.d.notes.banners(BOARD_MS).is_empty());
}

#[test]
fn every_text_of_the_call_banners_exists_in_every_language() {
    for lang in Lang::ALL {
        let tr = ultar_desktop::i18n::translator(lang).unwrap();
        for key in [
            "desk.call.incoming",
            "desk.call.incoming.body",
            "desk.call.answer",
            "desk.call.decline",
            "desk.call.active",
            "desk.call.active.body",
            "desk.call.active.muted",
            "desk.call.mute",
            "desk.call.unmute",
            "desk.call.hangup",
            "desk.note.mirror.title",
            "desk.note.mirror.locked",
        ] {
            assert!(tr.catalog(lang).get(key).is_some(), "{key} in {lang:?}");
        }
    }
}

/// The banners over the desktop's ground, as the compositor would draw them.
fn shot(
    name: &str,
    lang: Lang,
    theme: ThemeKind,
    fill: impl Fn(&mut Harness),
    p: &mut Vec<String>,
) {
    let mut h = Harness::new(lang, theme);
    fill(&mut h);
    let mut img = blank_of(theme, SCREEN);
    let tr = ultar_desktop::i18n::translator(lang).unwrap();
    let (mut v, place) =
        ultar_desktop::view::banners::build(&h.shell.d, (BOARD_MS, 0), h.look, SCREEN, &tr)
            .unwrap()
            .unwrap();
    // The banners are a popup of the bar: their place is counted from the bar's top.
    let bar_top = (SCREEN.h - 56.0) as i32;
    over(
        &mut img,
        &mut v,
        place.surface,
        (place.offset.0, bar_top + place.offset.1),
    );
    check(name, lang, theme, &img, p);
}

fn mirrored(h: &mut Harness) {
    let post = |title: Text, body: Body| Post {
        urgency: Urgency::Normal,
        category: Category::App,
        app_id: "mesh.tech.code-craft.phone".into(),
        title,
        body,
        actions: Vec::new(),
    };
    let shown = post(
        Text::key("desk.note.mirror.title")
            .with(TextArg::Verbatim("KM One".into()))
            .with(TextArg::Verbatim("Samer".into())),
        Body::Verbatim("hello".into()),
    );
    let hidden = post(
        Text::key("desk.note.mirror.locked").with(TextArg::Verbatim("KM One".into())),
        Body::Empty,
    );
    for (id, post) in [(1, shown), (2, hidden)] {
        let posted = Posted {
            id,
            timestamp_ms: BOARD_MS,
            sender: "mesh".into(),
            post,
        };
        h.shell.d.notes.add(posted);
        h.shell.d.notes.show_banner(id, BOARD_MS);
    }
}

#[test]
fn the_call_banners_look_as_designed_in_both_languages_and_themes() {
    let mut problems = Vec::new();
    for (lang, theme) in VARIANTS {
        shot(
            "call-incoming",
            lang,
            theme,
            |h| calls::incoming(&mut h.shell.d, offer(5), BOARD_MS),
            &mut problems,
        );
        shot(
            "call-active",
            lang,
            theme,
            |h| {
                calls::incoming(&mut h.shell.d, offer(5), BOARD_MS);
                calls::led(&mut h.shell.d, &led(5, true), BOARD_MS);
            },
            &mut problems,
        );
        shot("note-mirrored", lang, theme, mirrored, &mut problems);
    }
    assert!(problems.is_empty(), "{problems:#?}");
}
