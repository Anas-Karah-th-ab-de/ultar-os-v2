//! Snapshot tests of the shell's other shapes in both themes and both languages
//! (DESKTOP-SPEC §2.2c): the phone (boards CompactHome, CompactApps, CompactQuick and the
//! search) at 360 × 800 and the desktop with the top bar and the dock (board DesktopTop)
//! at 1440 × 900 – each composed like the compositor does.

mod common;

use common::shots::*;
use common::*;
use ultar_bus::messages::auth::UserInfo;
use ultar_desktop::lock::{self, LockState};
use ultar_desktop::model::time::LocalTime;
use ultar_desktop::model::{Open, QuickPage};
use ultar_desktop::shell::{bar_panel, second_panel, Outcome};
use ultar_desktop::view::{Look, Target};
use ultar_surface::proto::Edge;
use ultar_ui::geom::Size;
use ultar_ui::snapshot::Image;

/// Draws the whole screen of a harness: home screen, panels, popup.
fn compose(h: &mut Harness) -> Image {
    let screen = h.screen;
    let mut img = blank_of(h.look.theme, screen);
    if let Some(home) = &mut h.home {
        over(&mut img, home, screen, (0, 0));
    }
    let (edge, thick) = bar_panel(h.shell.shape);
    let bar_top = if edge == Edge::Top {
        0
    } else {
        (screen.h - thick) as i32
    };
    over(
        &mut img,
        &mut h.bar,
        Size::new(screen.w, thick),
        (0, bar_top),
    );
    if let (Some(v), Some((_, thick, _))) = (&mut h.second, second_panel(h.shell.shape)) {
        let top = (screen.h - thick) as i32;
        over(&mut img, v, Size::new(screen.w, thick), (0, top));
    }
    if let Some(p) = &mut h.popup {
        let size = p.size;
        let off = (p.offset.0, bar_top + p.offset.1);
        over(&mut img, &mut p.view, size, off);
    }
    img
}

fn open(h: &mut Harness, open: Open) {
    h.shell.d.open = open;
    h.apply(Outcome::surfaces());
}

#[test]
fn the_phone_and_the_top_bar_look_as_recorded() {
    let mut problems = Vec::new();
    for (lang, theme) in VARIANTS {
        let phone: [(&str, Open); 4] = [
            ("phone-home", Open::None),
            ("phone-apps", Open::Apps(None)),
            ("phone-quick", Open::Quick(QuickPage::Tiles)),
            ("phone-search", Open::Overview),
        ];
        for (name, o) in phone {
            let mut h = Harness::phone(lang, theme);
            phone_note(&mut h.shell.d, lang);
            open(&mut h, o);
            let img = compose(&mut h);
            check(name, lang, theme, &img, &mut problems);
        }
        let top: [(&str, Open); 5] = [
            ("top", Open::None),
            ("top-appmenu", Open::AppMenu),
            ("top-quick", Open::Quick(QuickPage::Tiles)),
            ("top-overview", Open::Overview),
            ("top-menu-tile", Open::TileMenu(0)),
        ];
        for (name, o) in top {
            let mut h = Harness::top(lang, theme);
            open(&mut h, o);
            let img = compose(&mut h);
            check(name, lang, theme, &img, &mut problems);
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

fn locked() -> LockState {
    let mut s = LockState {
        user: Some(UserInfo {
            name: "anas".into(),
            display_name: "أنس".into(),
            has_pin: false,
            admin: true,
        }),
        notes: vec![("org.ultar.phone".into(), 1), ("org.ultar.store".into(), 1)],
        ..LockState::default()
    };
    s.locked();
    s
}

/// Board CompactLock: the cover and the sign-in at phone size; `lock.unlock` is the
/// primary control, `lock.sos` and `lock.power` are disabled with their reason, and
/// `lock.media` is absent (no media service).
#[test]
fn the_phone_lock_screen_looks_as_recorded() {
    let mut problems = Vec::new();
    let t = LocalTime::at(BOARD_MS, 0);
    for (lang, theme) in VARIANTS {
        let tr = ultar_desktop::i18n::translator(lang).unwrap();
        let look = Look::new(lang, theme);
        let mut s = locked();
        for name in ["phone-lock", "phone-lock-signin"] {
            let mut v = lock::view::build(&s, (BOARD_MS, t), look, PHONE, &tr)
                .unwrap()
                .unwrap();
            let mut img = blank_of(theme, PHONE);
            over(&mut img, &mut v, PHONE, (0, 0));
            check(name, lang, theme, &img, &mut problems);
            if name == "phone-lock" {
                let unlock = v.ui.rect(v.node(&Target::Unlock).unwrap()).unwrap();
                assert_eq!(unlock.h, 32.0);
                assert!((unlock.x + unlock.w / 2.0 - 180.0).abs() <= 1.0, "centred");
                assert_eq!(v.ui.focused(), v.node(&Target::Unlock));
                for t in [Target::Emergency, Target::EmergencyOff] {
                    let node = v.node(&t).unwrap();
                    assert!(v.ui.is_disabled(node) && v.ui.tooltip(node).is_some());
                    assert_eq!(v.ui.rect(node).unwrap().bottom(), 800.0 - 12.0);
                }
                assert!(v.node(&Target::Media(1)).is_none());
            } else {
                assert!(v.node(&Target::Password).is_some());
                let field = v.ui.rect(v.node(&Target::Password).unwrap()).unwrap();
                assert!(field.x >= 24.0 && field.right() <= 336.0, "fits the phone");
            }
            s.show_sign_in();
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// A synthetic window copy: `rgb` with a darker band at the top (a toolbar).
fn copy_of(rgb: [u8; 3], w: u32, h: u32) -> std::rc::Rc<ultar_ui::widgets::Thumbnail> {
    let mut xrgb = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let k = if y < h / 8 { 2 } else { 1 };
        for _ in 0..w {
            xrgb.extend_from_slice(&[rgb[2] / k, rgb[1] / k, rgb[0] / k, 0xff]);
        }
    }
    ultar_ui::widgets::Thumbnail::new(w, h, xrgb).unwrap()
}

/// Board Overview with the compositor's copies: each window card shows its thumbnail
/// (overview.window), the workspace strip has two workspaces and "new" (overview.workspace).
#[test]
fn the_overview_shows_the_copies_of_the_windows() {
    let mut problems = Vec::new();
    let green = [0x2e, 0x7d, 0x32];
    for (lang, theme) in VARIANTS {
        let mut h = Harness::with(board_desktop(lang, theme), lang, theme);
        h.shell.d.overview.keep_thumb(1, copy_of(green, 320, 200));
        h.shell
            .d
            .overview
            .keep_thumb(2, copy_of([0xc8, 0xd2, 0xe6], 448, 280));
        open(&mut h, Open::Overview);
        let img = compose(&mut h);
        let found = img.rgba.chunks_exact(4).any(|p| p[..3] == green);
        assert!(found, "the terminal's copy is drawn ({lang:?} {theme:?})");
        check("overview-thumbs", lang, theme, &img, &mut problems);
    }
    assert!(problems.is_empty(), "{problems:?}");
}
