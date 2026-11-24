//! Keyboard only and the screen reader (DESKTOP-SPEC §2.2b "Prüfer Zugang", §13 no. 62):
//! every operable element of every surface is reachable with Tab, has a role and a spoken
//! name; a disabled one says why; sheets take the focus and keep it inside.

mod common;

use common::*;
use ultar_desktop::model::{Open, QuickPage};
use ultar_desktop::shell::Outcome;
use ultar_desktop::view::View;
use ultar_i18n::Lang;
use ultar_ui::event::{Event, Key, Modifiers};
use ultar_ui::tokens::ThemeKind;

fn check_view(name: &str, view: &mut View, tr: &ultar_i18n::Translator) {
    let tree = view.ui.a11y_tree();
    for (node, target) in &view.targets {
        let a = tree
            .iter()
            .find(|n| n.id == *node && n.part.is_none())
            .unwrap_or_else(|| panic!("{name}: {target:?} has no accessibility node"));
        let spoken = a
            .name
            .as_ref()
            .map(|l| l.resolve(tr).trim().to_owned())
            .unwrap_or_default();
        assert!(!spoken.is_empty(), "{name}: {target:?} has no name");
        assert!(
            !spoken.contains("desk.") && !spoken.contains("lock."),
            "{name}: {target:?} untranslated: {spoken}"
        );
        if a.state.disabled {
            assert!(
                a.description.is_some() || view.ui.tooltip(*node).is_some(),
                "{name}: disabled {target:?} without a reason"
            );
        } else {
            assert!(a.focusable, "{name}: {target:?} not focusable");
        }
    }
    // Tab reaches every enabled operable node.
    let order = view.ui.focus_order();
    for (node, target) in &view.targets {
        if !view.ui.is_disabled(*node) && view.ui.rect(*node).is_some_and(|r| !r.is_empty()) {
            assert!(
                order.contains(node),
                "{name}: {target:?} not in the Tab order"
            );
        }
    }
}

#[test]
fn every_surface_is_named_and_reachable_by_keyboard() {
    for lang in [Lang::Ar, Lang::En] {
        let tr = ultar_desktop::i18n::translator(lang).unwrap();
        let mut h = Harness::new(lang, ThemeKind::Dark);
        check_view("bar", &mut h.bar, &tr);
        for open in [
            Open::Quick(QuickPage::Tiles),
            Open::Quick(QuickPage::PowerModes),
            Open::Calendar,
            Open::Overview,
            Open::Apps(None),
            Open::Projection(0),
        ] {
            h.shell.d.open = open;
            h.apply(Outcome::surfaces());
            let p = h.popup.as_mut().unwrap();
            check_view(&format!("{open:?}"), &mut p.view, &tr);
            assert!(
                p.view.ui.focused().is_some(),
                "{open:?} did not take the focus"
            );
        }
    }
}

#[test]
fn tab_stays_inside_an_open_sheet_and_esc_returns() {
    let mut h = Harness::new(Lang::Ar, ThemeKind::Dark);
    h.click_bar(ultar_desktop::view::Target::Status);
    let p = h.popup.as_mut().unwrap();
    let first = p.view.ui.focused();
    let count = p.view.ui.focus_order().len();
    for _ in 0..count {
        p.view.ui.handle(Event::Key {
            key: Key::Tab,
            modifiers: Modifiers::default(),
        });
        assert!(p.view.ui.focused().is_some());
    }
    assert_eq!(p.view.ui.focused(), first, "Tab cycles inside the sheet");
    // The focused tile shows its reason as a tooltip: the first Esc dismisses it
    // (WCAG 1.4.13), the next one closes the sheet.
    let tip = p
        .view
        .ui
        .focused()
        .is_some_and(|n| p.view.ui.tooltip(n).is_some());
    h.key_popup(Key::Escape);
    if tip && h.popup.is_some() {
        h.key_popup(Key::Escape);
    }
    assert!(h.popup.is_none() && h.shell.d.open == Open::None);
}

#[test]
fn lock_screen_and_dialogs_are_named() {
    use ultar_bus::messages::auth::UserInfo;
    use ultar_desktop::lock::{view, LockState};
    for lang in [Lang::Ar, Lang::En] {
        let tr = ultar_desktop::i18n::translator(lang).unwrap();
        let look = ultar_desktop::view::Look::new(lang, ThemeKind::Dark);
        let mut s = LockState {
            user: Some(UserInfo {
                name: "a".into(),
                display_name: "A".into(),
                has_pin: true,
                admin: true,
            }),
            layouts: (vec!["ar".into(), "en".into()], 0),
            ..LockState::default()
        };
        s.locked();
        let mut v = view::build(&s, (0, None), look, SCREEN, &tr)
            .unwrap()
            .unwrap();
        check_view("lock", &mut v, &tr);
        s.show_sign_in();
        let mut v = view::build(&s, (0, None), look, SCREEN, &tr)
            .unwrap()
            .unwrap();
        check_view("sign-in", &mut v, &tr);
        let field = v.node(&ultar_desktop::view::Target::Password).unwrap();
        assert_eq!(
            v.ui.focused(),
            Some(field),
            "the password field has the focus"
        );
    }
}
