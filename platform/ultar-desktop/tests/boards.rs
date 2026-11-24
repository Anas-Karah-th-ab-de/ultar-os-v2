//! The implementation against the boards' key values (DESKTOP-SPEC §2.2c no. 3): sizes read
//! from the board files (`design/boards/project/*.dc.html`, the `style` of the element with
//! the click's `data-act`), positions from the boards' layout rules, colours from the
//! tokens the boards use. Tolerance: 1 point (sub-pixel rounding of the toolkit).

mod common;

use common::*;
use ultar_desktop::model::{Open, QuickPage};
use ultar_desktop::shell::Outcome;
use ultar_desktop::view::{Target, View};
use ultar_i18n::Lang;
use ultar_ui::geom::Rect;
use ultar_ui::tokens::{ColorRole, ThemeKind, DARK, LIGHT};

fn board(name: &str) -> String {
    let path = format!(
        "{}/../../design/boards/project/{name}.dc.html",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap()
}

/// `(width, height)` in px of the first element with `data-act="act"` whose
/// `aria-label` contains `label`.
fn size_of(html: &str, act: &str, label: &str) -> (f32, f32) {
    let needle = format!("data-act=\"{act}\"");
    for (at, _) in html.match_indices(&needle) {
        let start = html[..at].rfind('<').unwrap();
        let end = at + html[at..].find('>').unwrap();
        let tag = &html[start..end];
        if !tag.contains(label) {
            continue;
        }
        let style = tag
            .split("style=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let px = |prop: &str| {
            style
                .split(';')
                .map(str::trim)
                .find_map(|p| {
                    p.strip_prefix(prop)
                        .and_then(|v| v.trim().strip_suffix("px"))
                })
                .map(|v| v.parse::<f32>().unwrap())
                .unwrap_or(f32::NAN)
        };
        return (px("width:"), px("height:"));
    }
    panic!("{act} {label} not on the board");
}

fn rect(view: &View, t: &Target) -> Rect {
    view.ui.rect(view.node(t).unwrap()).unwrap()
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1.0
}

#[test]
fn bar_items_have_the_boards_sizes_and_places() {
    for (lang, theme, name) in [
        (Lang::Ar, ThemeKind::Dark, "Main"),
        (Lang::En, ThemeKind::Light, "DesktopBottom"),
    ] {
        let html = board(name);
        let mut h = Harness::new(lang, theme);
        let _ = h.bar.ui.content_size();
        let v = &h.bar;
        for (t, act, label) in [
            (Target::Trash, "shell.trash", ""),
            (Target::Overview, "shell.overview", ""),
            (Target::Search, "shell.search", ""),
            (Target::App(0), "shell.app", ""),
        ] {
            let (w, hh) = size_of(&html, act, label);
            let r = rect(v, &t);
            assert!(
                close(r.w, w) && close(r.h, hh),
                "{name} {act}: {r:?} vs {w}×{hh}"
            );
        }
        let (_, kbd_h) = size_of(&html, "shell.kbd", "");
        assert!(close(rect(v, &Target::Keyboard).h, kbd_h));
        assert!(close(rect(v, &Target::Status).h, 32.0));
        assert!(close(rect(v, &Target::Clock).h, 44.0));
        // The bar: 56 high, a hairline on top, items centred in the 55 below it.
        assert_eq!(v.ui.size().h, 56.0);
        let trash = rect(v, &Target::Trash);
        assert!(close(trash.y, 1.0 + (55.0 - 44.0) / 2.0), "{trash:?}");
        // Start group at the start edge (12 in), end group at the end edge.
        let rtl = lang == Lang::Ar;
        let start_edge = if rtl {
            SCREEN.w - trash.right()
        } else {
            trash.x
        };
        assert!(close(start_edge, 12.0), "{start_edge}");
        let clock = rect(v, &Target::Clock);
        let end_edge = if rtl {
            clock.x
        } else {
            SCREEN.w - clock.right()
        };
        assert!(close(end_edge, 12.0), "{end_edge}");
        // The centre group is centred.
        let first = rect(v, &Target::Overview);
        let last = rect(v, &Target::App(h.shell.d.bar_apps().len() - 1));
        let middle = (first.x.min(last.x) + first.right().max(last.right())) / 2.0;
        assert!(close(middle, SCREEN.w / 2.0), "centre {middle}");
        // Overview first in reading order.
        assert_eq!(first.x < last.x, !rtl);
        h.take();
    }
}

fn pixel(c: &ultar_ui::canvas::Canvas, x: f32, y: f32) -> [u8; 3] {
    let p = c.pixel(x as i32, y as i32).unwrap();
    [p.r, p.g, p.b]
}

fn role(theme: ThemeKind, r: ColorRole) -> [u8; 3] {
    let c = if theme == ThemeKind::Dark {
        DARK
    } else {
        LIGHT
    }
    .get(r);
    [c.r, c.g, c.b]
}

#[test]
fn colours_are_the_boards_token_colours() {
    for theme in [ThemeKind::Dark, ThemeKind::Light] {
        let mut h = Harness::new(Lang::Ar, theme);
        let size = h.bar.ui.size();
        let c = render(&mut h.bar, size);
        // Bar background `bar`, hairline `line` on top, idle tile `raised`.
        assert_eq!(pixel(&c, 600.0, 3.0), role(theme, ColorRole::Bar));
        assert_eq!(pixel(&c, 600.0, 0.0), role(theme, ColorRole::Line));
        let t = rect(&h.bar, &Target::App(1));
        assert_eq!(
            pixel(&c, t.x + 6.0, t.y + 6.0),
            role(theme, ColorRole::Raised)
        );
        // Focused app's tile `hover`.
        let f = rect(&h.bar, &Target::App(0));
        assert_eq!(
            pixel(&c, f.x + 6.0, f.y + 6.0),
            role(theme, ColorRole::Hover)
        );
        // The quick settings card is `surface`; a tile that is on is `accent`.
        h.shell.d.open = Open::Quick(QuickPage::Tiles);
        h.apply(Outcome::surfaces());
        let p = h.popup.as_mut().unwrap();
        let size = p.size;
        let dark_tile = rect(
            &p.view,
            &Target::Tile(ultar_desktop::model::quick::TileId::Dark),
        );
        let c = render(&mut p.view, size);
        let lit = if theme == ThemeKind::Dark {
            ColorRole::Accent
        } else {
            ColorRole::Raised
        };
        assert_eq!(
            pixel(&c, dark_tile.x + 30.0, dark_tile.y + 4.0),
            role(theme, lit)
        );
        let card_inside = pixel(&c, dark_tile.x + 2.0, dark_tile.bottom() + 4.0);
        assert_eq!(card_inside, role(theme, ColorRole::Surface));
    }
}

#[test]
fn sheets_are_where_the_boards_put_them() {
    let mut h = Harness::new(Lang::Ar, ThemeKind::Dark);
    let html = board("QuickSettings");
    assert!(html.contains("width: 448px") && html.contains("bottom: 64px"));
    for (o, width) in [
        (Open::Quick(QuickPage::Tiles), 448.0),
        (Open::Calendar, 736.0),
    ] {
        h.shell.d.open = o;
        h.apply(Outcome::surfaces());
        let p = h.popup.as_ref().unwrap();
        // Arabic: the end side is the left; the card is 8 from it and 64 above the bottom.
        let card_left = p.offset.0 as f32 + 8.0;
        assert!(close(card_left, 8.0), "{o:?} {card_left}");
        let card_right = card_left + width;
        assert!(
            close(p.size.w, card_right - p.offset.0 as f32 + 32.0),
            "{o:?} {:?}",
            p.size
        );
        let card_bottom = SCREEN.h - 56.0 + p.offset.1 as f32 + 32.0 + (p.size.h - 32.0 - 48.0);
        assert!(close(card_bottom, SCREEN.h - 64.0), "{o:?} {card_bottom}");
        // Quick tiles are 56 high (board `qt`: height bar_h), two per row.
        if let Open::Quick(_) = o {
            let wifi = rect(
                &p.view,
                &Target::Tile(ultar_desktop::model::quick::TileId::Wifi),
            );
            let more = rect(
                &p.view,
                &Target::TileMore(ultar_desktop::model::quick::TileId::Wifi),
            );
            assert!(close(wifi.h, 56.0) && close(more.w, 32.0));
            assert!(
                close(wifi.w + more.w, (448.0 - 32.0 - 8.0) / 2.0),
                "{wifi:?}"
            );
        }
    }
}
