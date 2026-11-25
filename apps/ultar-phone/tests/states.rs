//! Every state of the phone (DESKTOP-SPEC §12.2 "Zustände" and the task list): the ways'
//! availability, failed / busy / ended / missed calls, mute, speaker, hold, keypad, second call.

mod common;

use common::*;
use ultar_bus::messages::call::{Cellular, End, Failure, Output, Phase, Ultar, Way};
use ultar_phone::Screen;

fn shown(a: &mut App, needle: &str) -> bool {
    names(a).iter().any(|n| n.contains(needle))
}

#[test]
fn mobile_network_states_show_a_notice_and_allow_emergency_calls_only() {
    for (state, text) in [
        (Cellular::NoSim, "لا توجد شريحة SIM"),
        (Cellular::NoNetwork, "لا توجد شبكة"),
        (Cellular::Airplane, "وضع الطيران مفعّل"),
        (Cellular::EmergencyOnly, "مكالمات الطوارئ فقط"),
    ] {
        let mut st = ready();
        st.lines.cellular = state;
        st.lines.ultar = Ultar::SignedOut;
        let mut a = app(st);
        assert_eq!(a.model().current_way(), Way::Cellular(1));
        assert!(shown(&mut a, text), "{state:?}: {:?}", names(&mut a));
        for k in ['1', '1', '2'] {
            let d = digit(&a, k);
            click(&mut a, d);
        }
        assert!(
            !{
                let n = node(&a, "phone.call");
                a.ui().is_disabled(n)
            },
            "112 works in {state:?}"
        );
        click_backspace(&mut a);
        let d = digit(&a, '5');
        click(&mut a, d);
        assert!(
            {
                let n = node(&a, "phone.call");
                a.ui().is_disabled(n)
            },
            "115 does not in {state:?}"
        );
    }
}

#[test]
fn no_modem_makes_the_mobile_network_unavailable() {
    let mut st = ready();
    st.lines.cellular = Cellular::NoModem;
    st.lines.sims.clear();
    let mut a = app(st);
    assert!(shown(&mut a, "هذا الجهاز بلا مودم"), "{:?}", names(&mut a));
}

#[test]
fn ultar_signed_out_and_offline() {
    for (state, text) in [
        (Ultar::Offline, "خادم Ultar غير متاح"),
        (Ultar::SignedOut, "سجّل الدخول"),
    ] {
        let mut st = ready();
        st.lines.ultar = state;
        st.lines.own_number = None;
        st.lines.sims.clear();
        st.lines.cellular = Cellular::NoSim;
        let mut a = app(st);
        assert_eq!(a.model().current_way(), Way::Ultar);
        assert!(shown(&mut a, text), "{state:?}");
        assert!(
            super_none(&a, "account.copy"),
            "no own number, no copy button"
        );
    }
}

fn super_none(a: &App, action: &str) -> bool {
    nodes(a, action).is_empty()
}

#[test]
fn ended_busy_failed_and_missed_calls_are_announced_with_call_again() {
    for (end, text) in [
        (End::Local, "انتهت المكالمة"),
        (End::Remote, "أنهى الطرف الآخر"),
        (End::Busy, "الخط مشغول"),
        (End::Missed, "مكالمة فائتة"),
        (End::Failed(Failure::NoNetwork), "تعذّر الاتصال: لا توجد شبكة"),
        (End::Failed(Failure::PeerOffline), "غير متصل"),
    ] {
        let mut st = ready();
        st.ended = Some(call(9, Phase::Ended(end), false));
        let mut a = app(st);
        assert!(shown(&mut a, text), "{end:?}: {:?}", names(&mut a));
        let again = a
            .clicks()
            .iter()
            .find(|(_, c)| *c == ultar_phone::Click::Again)
            .unwrap()
            .0;
        click(&mut a, again);
        assert!(
            matches!(a.port().sent.last(), Some(ultar_phone::Request::Dial(d)) if d.number == "UOS-4821-7730")
        );
    }
}

#[test]
fn dialing_alerting_held_show_their_phase() {
    for (phase, text) in [
        (Phase::Dialing, "جارٍ الاتصال"),
        (Phase::Alerting, "يرنّ"),
        (Phase::Held, "المكالمة معلّقة"),
    ] {
        let mut c = call(1, phase, false);
        c.active_since_ms = None;
        let mut a = app(with_calls(vec![c]));
        assert_eq!(a.model().screen(), Screen::InCall(1));
        assert!(shown(&mut a, text), "{phase:?}");
    }
}

#[test]
fn timer_counts_while_active() {
    let mut a = app(with_calls(vec![call(1, Phase::Active, false)]));
    assert!(shown(&mut a, "02:14"), "{:?}", names(&mut a));
    a.tick(203_000, NOW_S).unwrap();
    assert!(shown(&mut a, "02:17"));
}

#[test]
fn speaker_choice_marks_the_current_output() {
    let mut st = with_calls(vec![call(1, Phase::Active, false)]);
    st.audio.output = Output::Speaker;
    st.audio.muted = true;
    let mut a = app(st);
    click_action(&mut a, "phone.output");
    let tree = names(&mut a).join("|");
    assert!(tree.contains("مكبّر الصوت"), "{tree}");
    let mute = node(&a, "phone.mute");
    assert!(pressed(&mut a, mute));
}

#[test]
fn single_output_disables_the_output_button() {
    let mut st = with_calls(vec![call(1, Phase::Active, false)]);
    st.audio.outputs = vec![Output::Earpiece];
    let a = app(st);
    assert!(a.ui_ref_disabled(node(&a, "phone.output")));
}

trait Disabled {
    fn ui_ref_disabled(&self, id: ultar_ui::ui::NodeId) -> bool;
}
impl Disabled for App {
    fn ui_ref_disabled(&self, id: ultar_ui::ui::NodeId) -> bool {
        self.ui_ref().is_disabled(id)
    }
}

#[test]
fn second_incoming_call_and_held_call_banners() {
    let mut a = app(with_calls(vec![
        call(1, Phase::Active, false),
        call(2, Phase::Waiting, false),
    ]));
    assert!(shown(&mut a, "مكالمة واردة أخرى"));
    let mut a = app(with_calls(vec![
        call(1, Phase::Held, false),
        call(2, Phase::Active, false),
    ]));
    assert!(shown(&mut a, "معلّقة:"));
}

#[test]
fn incoming_cellular_call_is_not_called_encrypted() {
    let mut c = call(1, Phase::Ringing, false);
    c.way = Way::Cellular(2);
    c.encrypted = false;
    let mut a = app(with_calls(vec![c]));
    assert!(shown(&mut a, "عبر SIM"));
    assert!(!shown(&mut a, "مشفّرة من طرف إلى طرف"));
}

#[test]
fn ringing_asks_for_the_call_screen() {
    let a = app(with_calls(vec![call(1, Phase::Ringing, false)]));
    assert!(a.wants_call_screen());
    let a = app(with_calls(vec![call(1, Phase::Active, false)]));
    assert!(!a.wants_call_screen());
}

/// Regression (real-input gate, "phone: no answer to Tab 1 … window frozen"): a rebuild of the
/// tree (a new `call.state`, the minute turning, the bus thread's first answer) right after a
/// Tab hid the focus ring (the toolkit forgot that the keyboard was in use when the tree was
/// built again) and took the focus away altogether when it stood on the scrolling dial pad;
/// the ring vanished before a frame showed it and the window looked as if the key had not
/// arrived. Wherever Tab puts the focus, it is still there, with its ring, after a rebuild.
#[test]
fn the_focus_survives_a_rebuild_wherever_tab_put_it() {
    use ultar_ui::event::{Event, Key, Modifiers};
    let state = ultar_bus::messages::call::CallState::default();
    for (w, h) in [
        (1280.0, 800.0),
        (800.0, 660.0),
        (800.0, 420.0),
        (480.0, 320.0),
    ] {
        for with_log in [true, false] {
            let size = ultar_ui::Size { w, h };
            let mut a = app_with(
                ultar_ui::Lang::Ar,
                ultar_ui::tokens::ThemeKind::Dark,
                size,
                state.clone(),
            );
            if !with_log {
                a.set_log(Vec::new()).unwrap();
            }
            for step in 0..60 {
                a.handle(Event::Key {
                    key: Key::Tab,
                    modifiers: Modifiers::default(),
                })
                .unwrap();
                let Some(before) = a.ui_ref().focused() else {
                    panic!("{w}x{h}: Tab {step} left nothing focused");
                };
                let at = a.ui_ref().rect(before).unwrap();
                assert!(
                    a.ui_ref().visual(before).focused,
                    "{w}x{h}: Tab {step}: no ring"
                );
                a.set_state(state.clone(), 1_000 + step).unwrap();
                let after = a.ui_ref().focused();
                let ring = after.is_some_and(|id| a.ui_ref().visual(id).focused);
                assert!(
                    ring,
                    "{w}x{h} log={with_log}: Tab {step}: the rebuild hid the focus ring"
                );
                let kept = after.and_then(|id| a.ui_ref().rect(id));
                assert_eq!(
                    kept,
                    Some(at),
                    "{w}x{h} log={with_log}: Tab {step}: focus at {at:?} lost by a rebuild"
                );
            }
        }
    }
}
