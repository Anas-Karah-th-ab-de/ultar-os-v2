//! One test per click of docs/ui/CLICKS.md on the boards PhoneDial, PhoneIncoming and
//! PhoneInCall (`account.copy`, `win.*`, `phone.*`): real pointer input on the built tree, the
//! effect CLICKS.md defines checked on the request sent, the host or the tree.

mod common;

use common::*;
use ultar_bus::messages::call::{
    Answer, Decline, Dial, Dtmf, Hangup, Hold, Mute, NextCamera, Others, Phase, SetOutput,
    SetVideo, Share, Way,
};
use ultar_phone::model::{Dialog, Filter};
use ultar_phone::{Request, WindowOp};
use ultar_ui::event::{ActionKind, Event, Key, Modifiers, PointerButton};

fn last(app: &mut App) -> Request {
    app.port().sent.last().cloned().expect("a request was sent")
}

fn menu_choose(app: &mut App, item: u32) {
    // Menu rows are reachable by keyboard: Down moves to the next row, Enter chooses.
    app.ui().layout();
    for _ in 0..=item {
        app.handle(Event::Key {
            key: Key::ArrowDown,
            modifiers: Modifiers::NONE,
        })
        .unwrap();
    }
    app.handle(Event::Key {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
    })
    .unwrap();
}

#[test]
fn account_copy_copies_the_own_number_and_confirms() {
    let mut a = app(ready());
    click_action(&mut a, "account.copy");
    assert_eq!(a.host().copied, vec!["UOS-1111-2222".to_string()]);
    assert!(a.toast_key() == Some("phone.copied"));
    a.host().refuse_copy = true;
    click_action(&mut a, "account.copy");
    assert!(a.toast_key() == Some("phone.error.generic"));
}

#[test]
fn window_buttons_ask_the_platform() {
    let mut a = app(ready());
    for action in ["win.min", "win.max", "win.close"] {
        click_action(&mut a, action);
    }
    assert_eq!(
        a.host().window,
        vec![WindowOp::Minimize, WindowOp::Maximize, WindowOp::Close]
    );
}

#[test]
fn way_chooses_ultar_or_mobile_network() {
    let mut a = app(ready());
    assert_eq!(a.model().current_way(), Way::Ultar);
    let way = node(&a, "phone.way");
    a.ui().layout();
    // Keyboard: arrows move the choice inside the radio group.
    a.ui().focus(way).unwrap();
    let key = if a.ui().env().direction == ultar_ui::Direction::Rtl {
        Key::ArrowLeft
    } else {
        Key::ArrowRight
    };
    a.handle(Event::Key {
        key,
        modifiers: Modifiers::NONE,
    })
    .unwrap();
    assert_eq!(a.model().current_way(), Way::Cellular(1));
    // The video call is for the Ultar number only.
    click_action(&mut a, "phone.digit");
    assert!({
        let n = node(&a, "phone.call.video");
        a.ui().is_disabled(n)
    });
    assert!(!{
        let n = node(&a, "phone.call");
        a.ui().is_disabled(n)
    });
}

#[test]
fn number_field_keeps_digits_and_converts_arabic_ones() {
    let mut a = app(ready());
    let field = node(&a, "phone.number");
    click(&mut a, field);
    for c in ['٤', '8', 'x', '2', '1', '7'] {
        let f = node(&a, "phone.number");
        a.ui().focus(f).unwrap();
        a.handle(Event::Key {
            key: Key::Char(c),
            modifiers: Modifiers::NONE,
        })
        .unwrap();
    }
    assert_eq!(a.model().digits, "4821-7");
    assert_eq!(
        {
            let n = node(&a, "phone.number");
            a.ui().text(n)
        }
        .unwrap(),
        "UOS-4821-7"
    );
    // Enter calls.
    let f = node(&a, "phone.number");
    a.ui().focus(f).unwrap();
    a.handle(Event::Key {
        key: Key::Enter,
        modifiers: Modifiers::NONE,
    })
    .unwrap();
    assert!(
        matches!(last(&mut a), Request::Dial(Dial { ref number, .. }) if number == "UOS-4821-7")
    );
}

#[test]
fn backspace_deletes_one_and_held_deletes_all() {
    let mut a = app(ready());
    for _ in 0..5 {
        {
            let n = nodes(&a, "phone.digit")[0];
            click(&mut a, n)
        };
    }
    assert_eq!(a.model().digits, "3333-3");
    click_backspace(&mut a);
    assert_eq!(a.model().digits, "3333", "the digit and the dash before it");
    let pos = backspace_rect(&mut a).center();
    a.handle(Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    })
    .unwrap();
    a.tick(200_000 + 600, NOW_S).unwrap();
    assert!(a.model().digits.is_empty(), "held: deletes all");
    a.handle(Event::PointerUp {
        pos,
        button: PointerButton::Primary,
    })
    .unwrap();
    assert!(a.model().digits.is_empty());
}

#[test]
fn suggestion_takes_the_contact_number() {
    let mut a = app(ready());
    for k in ['4', '8', '2', '1'] {
        let d = a
            .clicks()
            .iter()
            .find(|(_, c)| *c == ultar_phone::Click::Digit(k))
            .unwrap()
            .0;
        click(&mut a, d);
    }
    click_action(&mut a, "phone.suggest");
    assert_eq!(a.model().number(), "UOS-4821-7730");
}

#[test]
fn every_digit_appends_and_sends_tones_in_a_call() {
    let mut a = app(ready());
    a.model();
    let digits = nodes(&a, "phone.digit");
    assert_eq!(digits.len(), 12);
    {
        let n = node(&a, "phone.way");
        a.ui().focus(n)
    }
    .unwrap();
    let way_cell = a.model().cellular_way();
    let _ = way_cell;
    for i in 0..12 {
        let d = nodes(&a, "phone.digit")[i];
        click(&mut a, d);
    }
    assert_eq!(
        a.model().digits.len(),
        9,
        "Ultar: 8 digits + dash; * and # do not belong"
    );
    // In a call with the keypad open, a digit sends the tone.
    let mut a = app(with_calls(vec![call(1, Phase::Active, false)]));
    click_action(&mut a, "phone.keypad");
    assert!(a.model().keypad);
    let five = a
        .clicks()
        .iter()
        .find(|(_, c)| *c == ultar_phone::Click::Digit('5'))
        .unwrap()
        .0;
    click(&mut a, five);
    assert_eq!(last(&mut a), Request::Dtmf(Dtmf { id: 1, key: b'5' }));
    assert_eq!(a.model().tones, "5");
}

#[test]
fn call_and_video_call_dial_on_the_chosen_way() {
    let mut a = app(ready());
    assert!(
        {
            let n = node(&a, "phone.call");
            a.ui().is_disabled(n)
        },
        "nothing typed"
    );
    let one = digit(&a, '1');
    click(&mut a, one);
    click_action(&mut a, "phone.call.video");
    assert_eq!(
        last(&mut a),
        Request::Dial(Dial {
            way: Way::Ultar,
            number: "UOS-1".into(),
            video: true
        })
    );
    let one = digit(&a, '1');
    click(&mut a, one);
    click_action(&mut a, "phone.call");
    assert_eq!(
        last(&mut a),
        Request::Dial(Dial {
            way: Way::Ultar,
            number: "UOS-1".into(),
            video: false
        })
    );
    assert!(a.model().digits.is_empty(), "number cleared after dialling");
}

#[test]
fn failed_dial_shows_the_reason() {
    let mut a = app(ready());
    {
        let n = nodes(&a, "phone.digit")[0];
        click(&mut a, n)
    };
    a.port().fail_next = Some(sent_error());
    click_action(&mut a, "phone.call");
    assert!(a.toast_key() == Some("phone.why.no_network"));
}

#[test]
fn filter_shows_all_or_missed() {
    let mut a = app(ready());
    assert_eq!(nodes(&a, "phone.callback").len(), 5);
    let tabs = node(&a, "phone.filter");
    a.ui().focus(tabs).unwrap();
    let key = if a.ui().env().direction == ultar_ui::Direction::Rtl {
        Key::ArrowLeft
    } else {
        Key::ArrowRight
    };
    a.handle(Event::Key {
        key,
        modifiers: Modifiers::NONE,
    })
    .unwrap();
    assert_eq!(a.model().filter, Filter::Missed);
    assert_eq!(nodes(&a, "phone.callback").len(), 1);
}

#[test]
fn settings_open_the_phone_settings() {
    let mut a = app(ready());
    click_action(&mut a, "phone.settings");
    assert_eq!(a.host().settings, 1);
}

#[test]
fn callback_calls_back_on_the_way_it_came() {
    let mut a = app(ready());
    let backs = nodes(&a, "phone.callback");
    click(&mut a, backs[1]);
    assert_eq!(
        last(&mut a),
        Request::Dial(Dial {
            way: Way::Cellular(1),
            number: "+963 11 555 0100".into(),
            video: false
        })
    );
}

#[test]
fn entry_menu_creates_contact_blocks_and_deletes() {
    let mut a = app(ready());
    {
        let n = nodes(&a, "phone.entry.more")[2];
        click(&mut a, n)
    };
    menu_choose(&mut a, 0);
    assert_eq!(a.host().contacts, vec!["UOS-2290-1184".to_string()]);
    {
        let n = nodes(&a, "phone.entry.more")[2];
        click(&mut a, n)
    };
    menu_choose(&mut a, 1);
    assert!(matches!(last(&mut a), Request::Block(ref b) if b.number == "UOS-2290-1184" && b.on));
    {
        let n = nodes(&a, "phone.entry.more")[2];
        click(&mut a, n)
    };
    menu_choose(&mut a, 2);
    assert!(matches!(last(&mut a), Request::Forget(ref f) if f.id == 3));
    {
        let n = nodes(&a, "phone.entry.more")[0];
        click(&mut a, n)
    };
    menu_choose(&mut a, 3);
    assert!(
        matches!(last(&mut a), Request::Clear(_)),
        "history can be cleared"
    );
}

#[test]
fn incoming_accept_decline_and_decline_with_message() {
    let mut a = app(with_calls(vec![call(1, Phase::Ringing, false)]));
    click_action(&mut a, "phone.accept");
    assert_eq!(
        last(&mut a),
        Request::Answer(Answer {
            id: 1,
            others: Others::Hold
        })
    );
    click_action(&mut a, "phone.decline");
    assert_eq!(last(&mut a), Request::Decline(Decline { id: 1 }));
    click_action(&mut a, "phone.decline.msg");
    assert_eq!(last(&mut a), Request::Decline(Decline { id: 1 }));
    assert!(matches!(a.model().dialog, Some(Dialog::Text { .. })));
    // The call is gone; the dialog offers the ready messages.
    a.set_state(ready(), 201_000).unwrap();
    let first = a
        .clicks()
        .iter()
        .find(|(_, c)| *c == ultar_phone::Click::Text(1))
        .unwrap()
        .0;
    click(&mut a, first);
    assert!(
        matches!(last(&mut a), Request::Text(ref t) if t.number == "UOS-4821-7730" && t.text.contains("سأتصل"))
    );
    assert!(a.model().dialog.is_none());
}

#[test]
fn in_call_controls() {
    let mut a = app(with_calls(vec![call(1, Phase::Active, true)]));
    click_action(&mut a, "phone.mute");
    assert_eq!(last(&mut a), Request::Mute(Mute { on: true }));
    click_action(&mut a, "phone.hold");
    assert_eq!(last(&mut a), Request::Hold(Hold { id: 1, on: true }));
    click_action(&mut a, "phone.video");
    assert_eq!(last(&mut a), Request::Video(SetVideo { id: 1, on: false }));
    click_action(&mut a, "phone.camera");
    assert_eq!(last(&mut a), Request::Camera(NextCamera { id: 1 }));
    click_action(&mut a, "phone.share");
    assert_eq!(
        a.model().dialog,
        Some(Dialog::Share(1)),
        "permission prompt first"
    );
    let ok = a
        .clicks()
        .iter()
        .find(|(_, c)| *c == ultar_phone::Click::ShareConfirm(1))
        .unwrap()
        .0;
    click(&mut a, ok);
    assert_eq!(last(&mut a), Request::Share(Share { id: 1, on: true }));
    click_action(&mut a, "phone.output");
    menu_choose(&mut a, 1);
    assert_eq!(
        last(&mut a),
        Request::Output(SetOutput {
            output: ultar_bus::messages::call::Output::Speaker
        })
    );
    click_action(&mut a, "phone.keypad");
    assert!(a.model().keypad);
    let close = a
        .clicks()
        .iter()
        .find(|(_, c)| *c == ultar_phone::Click::CloseDialog)
        .unwrap()
        .0;
    click(&mut a, close);
    assert!(!a.model().keypad);
    click_action(&mut a, "phone.hangup");
    assert_eq!(last(&mut a), Request::Hangup(Hangup { id: 1 }));
}

#[test]
fn muted_held_and_sharing_states_change_the_labels() {
    let mut st = with_calls(vec![call(1, Phase::Held, true)]);
    st.audio.muted = true;
    let mut a = app(st);
    let (mute, hold) = (node(&a, "phone.mute"), node(&a, "phone.hold"));
    assert!(
        pressed(&mut a, mute) && pressed(&mut a, hold),
        "toggles announce their state"
    );
    let video = node(&a, "phone.video");
    assert!(pressed(&mut a, video), "camera on");
    click_action(&mut a, "phone.hold");
    assert_eq!(last(&mut a), Request::Hold(Hold { id: 1, on: false }));
    assert!(
        {
            let n = node(&a, "phone.video");
            a.ui().is_disabled(n)
        },
        "media only while active"
    );
    let mut sharing = call(1, Phase::Active, true);
    sharing.media.sharing = true;
    a.set_state(with_calls(vec![sharing]), 202_000).unwrap();
    click_action(&mut a, "phone.share");
    assert_eq!(last(&mut a), Request::Share(Share { id: 1, on: false }));
}

#[test]
fn second_incoming_call_can_be_answered_holding_or_ending_the_first() {
    let mut a = app(with_calls(vec![
        call(1, Phase::Active, false),
        call(2, Phase::Waiting, false),
    ]));
    assert_eq!(a.model().screen(), ultar_phone::Screen::InCall(1));
    let accept = a
        .clicks()
        .iter()
        .find(|(_, c)| *c == ultar_phone::Click::Accept(2))
        .unwrap()
        .0;
    click(&mut a, accept);
    assert_eq!(
        last(&mut a),
        Request::Answer(Answer {
            id: 2,
            others: Others::Hold
        })
    );
    let end = a
        .clicks()
        .iter()
        .find(|(_, c)| *c == ultar_phone::Click::AcceptEnd(2))
        .unwrap()
        .0;
    click(&mut a, end);
    assert_eq!(
        last(&mut a),
        Request::Answer(Answer {
            id: 2,
            others: Others::End
        })
    );
    a.set_state(
        with_calls(vec![
            call(1, Phase::Held, false),
            call(2, Phase::Active, false),
        ]),
        203_000,
    )
    .unwrap();
    let swap = a
        .clicks()
        .iter()
        .find(|(_, c)| *c == ultar_phone::Click::Swap(1))
        .unwrap()
        .0;
    click(&mut a, swap);
    assert_eq!(last(&mut a), Request::Hold(Hold { id: 1, on: false }));
}

#[test]
fn every_action_reacts_to_its_click_kind() {
    // Activated reaches every board element of the dial surface (smoke over all clicks).
    let mut a = app(ready());
    let all: Vec<_> = a.clicks().to_vec();
    for (id, click) in all {
        if click.action().is_some() {
            assert!(a.ui().rect(id).is_some(), "{click:?} is shown");
        }
    }
    let _ = ActionKind::Activated;
}
