//! Clicks of the boards Lock, Login and SettingsDisplay (the keep dialog) that belong to
//! the lock screen and the system dialogs (docs/ui/CLICKS.md): every one has its effect or
//! is disabled with a reason; never a dead click.

mod common;

use common::*;
use std::collections::BTreeSet;
use ultar_bus::messages::auth::{Outcome as AuthOutcome, Secret, UserInfo};
use ultar_bus::messages::settings::SettingValue;
use ultar_bus::messages::sysdialog::{AskPermission, Decision, Permission};
use ultar_desktop::dialog::{Answer, DialogEffect, Dialogs, Question};
use ultar_desktop::lock::{LockEffect, LockState, Stage};
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_ui::tokens::ThemeKind;

/// Actions of these boards that belong to the Settings app or the compositor.
const ELSEWHERE: [&str; 3] = ["win.", "settings.", "display."];
/// The keep dialog's buttons on board SettingsDisplay are the system dialog's.
const KEEP: [&str; 3] = ["display.keep", "display.revert", "display.keep.more"];

fn actions() -> BTreeSet<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/ui/CLICKS.md");
    let text = std::fs::read_to_string(path).unwrap();
    let mut board = String::new();
    let mut out = BTreeSet::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            board = rest.split(' ').next().unwrap_or("").to_owned();
        } else if line.starts_with("| ")
            && ["Lock", "Login", "SettingsDisplay"].contains(&board.as_str())
        {
            if let Some(a) = line.split('|').nth(3).map(|c| c.trim().trim_matches('`')) {
                if a.contains('.') && !a.contains(' ') {
                    out.insert(a.to_owned());
                }
            }
        }
    }
    out
}

fn user() -> UserInfo {
    UserInfo {
        name: "anas".into(),
        display_name: "أنس".into(),
        has_pin: true,
        admin: true,
    }
}

fn lock(a: &str) -> bool {
    let mut s = LockState {
        user: Some(user()),
        ..LockState::default()
    };
    s.locked();
    match a {
        "lock.unlock" => {
            s.show_sign_in();
            s.stage == Stage::SignIn
        }
        "lock.media" => media_card(&mut s),
        // Power off when the power service offers it (power.actions).
        "lock.power"
            if {
                s.svc.power_off = true;
                true
            } =>
        {
            let v = lock_view(&s);
            let n = v.node(&Target::EmergencyOff).unwrap();
            let on = !v.ui.is_disabled(n);
            s.svc.power_off = false;
            let v = lock_view(&s);
            let n = v.node(&Target::EmergencyOff).unwrap();
            on && v.ui.is_disabled(n) && v.ui.tooltip(n).is_some()
        }
        "login.access" => access(&mut s),
        "lock.sos" | "qs.power" => {
            // Disabled with the reason until the emergency / accessibility parts exist.
            let tr = ultar_desktop::i18n::translator(Lang::Ar).unwrap();
            let look = ultar_desktop::view::Look::new(Lang::Ar, ThemeKind::Dark);
            if a != "lock.sos" {
                s.show_sign_in();
            }
            let mut v = ultar_desktop::lock::view::build(&s, (0, None), look, SCREEN, &tr)
                .unwrap()
                .unwrap();
            let _ = v.ui.content_size();
            let t = match a {
                "lock.sos" => Target::Emergency,
                _ => Target::EmergencyOff,
            };
            let n = v.node(&t).unwrap();
            v.ui.is_disabled(n) && v.ui.tooltip(n).is_some()
        }
        _ => login(a, &mut s),
    }
}

/// login.access: disabled with the reason without the settings service; with it the menu
/// opens with its four switches, a switch changes its setting at once (the narrow right
/// `settings.set_accessibility`), Esc closes the menu before it leaves the sign-in.
fn access(s: &mut LockState) -> bool {
    s.show_sign_in();
    let v = lock_view(s);
    let n = v.node(&Target::Access).unwrap();
    let off = v.ui.is_disabled(n) && v.ui.tooltip(n).is_some();
    s.access
        .reported(&[("accessibility.large_text".into(), true)]);
    s.access.flip();
    let v = lock_view(s);
    let n = v.node(&Target::Access).unwrap();
    let switches = (0..4u8).all(|i| v.node(&Target::AccessSwitch(i)).is_some());
    let call = s.access.toggle(1, true) == Some(("accessibility.high_contrast", true));
    s.back();
    let closed = !s.access.open
        && s.stage == Stage::SignIn
        && lock_view(s).node(&Target::AccessSwitch(0)).is_none();
    off && !v.ui.is_disabled(n) && switches && call && closed
}

/// What plays: the card with its three buttons (media.state); none while nothing plays.
fn media_card(s: &mut LockState) -> bool {
    s.svc.media = Some(("نشرة".into(), "إذاعة".into(), true, false, true));
    let v = lock_view(s);
    let buttons: Vec<bool> = (0..3)
        .map(|i| {
            v.node(&Target::Media(i))
                .is_some_and(|n| !v.ui.is_disabled(n))
        })
        .collect();
    s.svc.media = None;
    let gone = lock_view(s).node(&Target::Media(1)).is_none();
    buttons == [false, true, true] && gone
}

fn lock_view(s: &LockState) -> ultar_desktop::view::View {
    let tr = ultar_desktop::i18n::translator(Lang::Ar).unwrap();
    let look = ultar_desktop::view::Look::new(Lang::Ar, ThemeKind::Dark);
    let mut v = ultar_desktop::lock::view::build(s, (0, None), look, SCREEN, &tr)
        .unwrap()
        .unwrap();
    let _ = v.ui.content_size();
    v
}

/// The sign-in part of the lock screen (board Login).
fn login(a: &str, s: &mut LockState) -> bool {
    match a {
        "login.user" => {
            // The session's user is chosen; another account is shown, disabled with the reason.
            s.users = vec![
                s.user.clone().unwrap(),
                UserInfo {
                    name: "layla".into(),
                    display_name: "ليلى".into(),
                    has_pin: false,
                    admin: false,
                },
            ];
            s.show_sign_in();
            let tr = ultar_desktop::i18n::translator(Lang::Ar).unwrap();
            let look = ultar_desktop::view::Look::new(Lang::Ar, ThemeKind::Dark);
            let mut v = ultar_desktop::lock::view::build(s, (0, None), look, SCREEN, &tr)
                .unwrap()
                .unwrap();
            let _ = v.ui.content_size();
            let (own, other) = (
                v.node(&Target::User(0)).unwrap(),
                v.node(&Target::User(1)).unwrap(),
            );
            let both = !v.ui.is_disabled(own) && !v.ui.is_disabled(other);
            // A click on the other account: that person signs in.
            both && s.choose_user(1)
                && s.user.as_ref().map(|u| u.name.as_str()) == Some("layla")
                && !s.choose_user(5)
        }
        "login.password" | "login.submit" => {
            s.show_sign_in();
            let e = s.submit(Secret::new("pw".into()), 0);
            let sent = matches!(e, Some(LockEffect::Verify { .. }));
            let back = s.verified(
                Some(AuthOutcome::Denied {
                    retry_after_ms: 1000,
                }),
                0,
            );
            sent && back.is_none() && s.wait_ms(0) == 1000
        }
        "login.show" => {
            // The toolkit's password field: shown while its eye is held (ultar-ui tests).
            let f = ultar_ui::widgets::TextField::password(ultar_ui::widgets::Label::key(
                "lock.password",
            ));
            f.is_password() && !f.is_revealed()
        }
        "login.method" => {
            s.show_sign_in();
            s.switch_method();
            s.method == ultar_bus::messages::auth::Method::Pin
        }
        "shell.kbd" => true,
        _ => false,
    }
}

fn keep(a: &str) -> bool {
    let mut d = Dialogs::default();
    let q = Question::Keep {
        key: "display.mode".into(),
        value: SettingValue::Text("extend".into()),
    };
    d.ask(1, q, 0);
    match a {
        "display.keep" => {
            d.answer(Answer::Keep(true)) == vec![DialogEffect::Confirm("display.mode".into())]
        }
        "display.revert" => {
            d.dismiss() == vec![DialogEffect::Revert("display.mode".into())] && {
                let mut e = Dialogs::default();
                e.ask(
                    1,
                    Question::Keep {
                        key: "k".into(),
                        value: SettingValue::Flag(true),
                    },
                    0,
                );
                e.tick(15_000) == vec![DialogEffect::Revert("k".into())]
            }
        }
        "display.keep.more" => matches!(d.more_time(1000).as_slice(), [DialogEffect::Again(..)]),
        _ => false,
    }
}

#[test]
fn every_click_of_the_lock_login_and_keep_boards_has_its_effect() {
    let mut missing = Vec::new();
    for a in actions() {
        let keep_dialog = KEEP.contains(&a.as_str());
        if ELSEWHERE.iter().any(|p| a.starts_with(p)) && !keep_dialog {
            continue;
        }
        let ok = if keep_dialog { keep(&a) } else { lock(&a) };
        if !ok {
            missing.push(a);
        }
    }
    assert!(missing.is_empty(), "clicks without effect: {missing:?}");
    let _ = (Decision::Deny, Permission::Camera, AskPermission::clone);
}
