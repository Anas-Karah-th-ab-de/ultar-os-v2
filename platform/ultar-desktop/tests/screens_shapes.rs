//! Snapshot tests of the shell's other shape in both themes and both languages
//! (DESKTOP-SPEC §2.2c): the desktop with the top bar and the dock (board DesktopTop) at
//! 1440 × 900 – composed like the compositor does.

mod common;

use common::shots::*;
use common::*;
use ultar_desktop::model::{Open, QuickPage};
use ultar_desktop::shell::{bar_panel, second_panel, Outcome};
use ultar_surface::proto::Edge;
use ultar_ui::geom::Size;
use ultar_ui::snapshot::Image;

/// Draws the whole screen of a harness: panels, popup.
fn compose(h: &mut Harness) -> Image {
    let screen = h.screen;
    let mut img = blank_of(h.look.theme, screen);
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
fn the_top_bar_looks_as_recorded() {
    let mut problems = Vec::new();
    for (lang, theme) in VARIANTS {
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
