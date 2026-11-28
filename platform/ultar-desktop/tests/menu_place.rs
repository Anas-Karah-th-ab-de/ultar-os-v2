//! Where the bar's menus open (DESKTOP-SPEC §5): at the control that was clicked – the
//! input language above its button, an app tile's menu above the tile, the power menu at
//! the power button of the quick settings – in both directions and both shapes; opened by
//! a key, in the middle.

mod common;

use common::*;
use ultar_desktop::model::{Open, Shape};
use ultar_desktop::view::{Target, View};
use ultar_i18n::Lang;
use ultar_ui::geom::Rect;
use ultar_ui::tokens::ThemeKind;

/// The middle of a node of `view` lying at `origin` (screen points from the left).
fn middle(view: &View, origin: f32, t: &Target) -> f32 {
    let r = view.ui.rect(view.node(t).unwrap()).unwrap();
    origin + r.x + r.w / 2.0
}

/// The open menu's card in screen points (horizontally).
fn card(h: &Harness) -> Rect {
    let p = h.popup.as_ref().expect("a menu is open");
    let r = p.view.ui.menu_rect(0).expect("the menu is open");
    Rect::new(p.offset.0 as f32 + r.x, r.y, r.w, r.h)
}

/// The card is centred on `x`, or pushed in to 8 points from the screen's edge as little as
/// needed.
fn at(h: &Harness, x: f32, what: &str) {
    let c = card(h);
    let centred = (c.x + c.w / 2.0 - x).abs() <= 1.0;
    let at_edge = (c.x - 8.0).abs() <= 1.0 || (SCREEN.w - c.right() - 8.0).abs() <= 1.0;
    assert!(
        centred || (at_edge && c.x <= x && x <= c.right()),
        "{what}: card {c:?}, control at {x}"
    );
}

#[test]
fn the_bar_menus_open_at_their_button() {
    for lang in [Lang::En, Lang::Ar] {
        let mut h = Harness::new(lang, ThemeKind::Dark);
        let kbd = middle(&h.bar, 0.0, &Target::Keyboard);
        assert!(
            (kbd - SCREEN.w / 2.0).abs() > 300.0,
            "the button sits at a side"
        );
        h.click_bar(Target::Keyboard);
        assert_eq!(h.shell.d.open, Open::KeyboardMenu);
        at(&h, kbd, "input language");
        // An app tile's menu (right click) above the tile.
        h.click_bar(Target::Keyboard);
        let tile = middle(&h.bar, 0.0, &Target::App(1));
        h.right_bar(Target::App(1));
        assert_eq!(h.shell.d.open, Open::TileMenu(1));
        at(&h, tile, "tile");
        // The power button of the quick settings.
        h.click_bar(Target::Status);
        let p = h.popup.as_ref().unwrap();
        let power = middle(&p.view, p.offset.0 as f32, &Target::Power);
        h.click_popup(Target::Power);
        assert_eq!(h.shell.d.open, Open::PowerMenu);
        at(&h, power, "power");
    }
}

#[test]
fn with_the_top_bar_they_open_below_their_button_and_above_the_dock_tile() {
    for lang in [Lang::En, Lang::Ar] {
        let d = board_desktop(lang, ThemeKind::Light);
        let mut h = Harness::shaped(d, lang, ThemeKind::Light, Shape::TOP, SCREEN);
        let kbd = middle(&h.bar, 0.0, &Target::Keyboard);
        h.click_bar(Target::Keyboard);
        at(&h, kbd, "input language");
        assert!(card(&h).y < 100.0, "below the top bar");
        h.click_bar(Target::Keyboard);
        let dock = h.second.as_ref().unwrap();
        let tile = middle(dock, 0.0, &Target::App(0));
        h.right_side(Target::App(0));
        assert_eq!(h.shell.d.open, Open::TileMenu(0));
        at(&h, tile, "dock tile");
    }
}

#[test]
fn a_menu_opened_without_a_click_opens_in_the_middle() {
    let mut h = Harness::new(Lang::En, ThemeKind::Dark);
    h.click_bar(Target::Keyboard);
    h.click_bar(Target::Keyboard);
    // The same menu, opened by the shell itself (a key): no control, the middle.
    h.shell.d.open = Open::PowerMenu;
    h.apply(ultar_desktop::shell::Outcome::surfaces());
    at(&h, SCREEN.w / 2.0, "power by key");
}
