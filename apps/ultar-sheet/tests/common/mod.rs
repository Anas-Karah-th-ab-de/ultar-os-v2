//! Helpers of the window tests: a window over a temporary set of places, keys, clicks and
//! pictures. Each test file uses some of them.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use ultar_i18n::Lang;
use ultar_office::sheet::{Sheet, Workbook};
use ultar_office::style::CellStyle;
use ultar_sheet::app::{Out, SheetApp};
use ultar_sheet::book::Book;
use ultar_sheet::cmd::Cmd;
use ultar_sheet::file::Format;
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::geom::Point;
use ultar_ui::paint::PaintEnv;
use ultar_ui::snapshot::{render, Image};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::Size;

pub const COMPACT: (f32, f32) = (360.0, 640.0);
pub const MEDIUM: (f32, f32) = (800.0, 600.0);
pub const WIDE: (f32, f32) = (1280.0, 800.0);

pub struct Dirs {
    pub areas: PathBuf,
    pub recover: PathBuf,
}

pub fn dirs(name: &str) -> Dirs {
    let d = std::env::temp_dir().join(format!("ultar-sheet-win-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("areas/documents")).unwrap();
    Dirs {
        areas: d.join("areas"),
        recover: d.join("recover"),
    }
}

pub fn ui(lang: Lang, theme: ThemeKind, mode: InputMode, size: (f32, f32)) -> Ui {
    Ui::new(
        PaintEnv::new(lang, theme),
        mode,
        Size::new(size.0, size.1),
        ultar_sheet::i18n::translator(lang),
    )
    .unwrap()
}

pub fn app_with(
    d: &Dirs,
    book: Book,
    lang: Lang,
    theme: ThemeKind,
    mode: InputMode,
    size: (f32, f32),
) -> SheetApp {
    SheetApp::new(
        ui(lang, theme, mode, size),
        book,
        d.areas.clone(),
        d.recover.clone(),
    )
    .unwrap()
}

pub fn app(d: &Dirs, lang: Lang, theme: ThemeKind, size: (f32, f32)) -> SheetApp {
    app_with(d, Book::new(), lang, theme, InputMode::Pointer, size)
}

/// A small workbook with headers, numbers, formulas, formats and Arabic.
pub fn sample(rtl: bool) -> Book {
    let mut sh = Sheet::new("Sheet1");
    sh.rtl = rtl;
    sh.frozen_rows = 1;
    let head = ["المنتج", "الكمية", "السعر", "المجموع"];
    for (c, h) in head.iter().enumerate() {
        sh.set(0, c as u32, h);
        sh.set_style(
            0,
            c as u32,
            CellStyle {
                bold: true,
                fill: Some(0xE0E0E0),
                ..CellStyle::default()
            },
        );
    }
    let rows = [
        ("تفاح", 12.0, 2.5),
        ("Bananas", 30.0, 0.75),
        ("برتقال", 7.0, 3.2),
        ("Mango", 5.0, 4.0),
    ];
    for (i, (n, q, p)) in rows.iter().enumerate() {
        let r = i as u32 + 1;
        sh.set(r, 0, n);
        sh.set(r, 1, &q.to_string());
        sh.set(r, 2, &p.to_string());
        sh.set(r, 3, &format!("=B{}*C{}", r + 1, r + 1));
        sh.set_style(
            r,
            2,
            CellStyle {
                num_fmt: "$#,##0.00".into(),
                ..CellStyle::default()
            },
        );
    }
    sh.set(6, 2, "=SUM(D2:D5)");
    sh.set(6, 3, "=1/0");
    sh.set_style(
        6,
        2,
        CellStyle {
            bold: true,
            color: Some(0xC62828),
            ..CellStyle::default()
        },
    );
    let mut b = Book::of(
        Workbook {
            sheets: vec![sh, Sheet::new("Sheet2")],
        },
        None,
        Format::Xlsx,
    );
    b.mark_saved();
    b
}

pub fn shot(app: &mut SheetApp, name: &str) -> Image {
    app.ui.layout();
    let canvas = render(&mut app.ui).unwrap();
    let img = Image::from_canvas(&canvas);
    let first = &img.rgba[..4];
    assert!(
        img.rgba.chunks(4).any(|p| p != first),
        "{name}: something is painted"
    );
    if let Ok(dir) = std::env::var("ULTAR_SHEET_SHOTS") {
        let png = img.encode_png().unwrap();
        ultar_ui::snapshot::write_atomic(&Path::new(&dir).join(format!("{name}.png")), &png)
            .unwrap();
    }
    img
}

pub fn key(app: &mut SheetApp, key: Key, m: Modifiers) {
    app.key(Event::Key { key, modifiers: m }).unwrap();
}

pub fn plain(app: &mut SheetApp, k: Key) {
    key(app, k, Modifiers::NONE);
}

pub fn ctrl(app: &mut SheetApp, c: char) {
    key(
        app,
        Key::Char(c),
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    );
}

pub fn type_text(app: &mut SheetApp, text: &str) {
    for c in text.chars() {
        plain(app, if c == ' ' { Key::Space } else { Key::Char(c) });
    }
}

pub fn click(app: &mut SheetApp, id: NodeId) {
    app.ui.layout();
    let r = app.ui.rect(id).expect("rect");
    let pos = Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0);
    let mut actions = Vec::new();
    actions.extend(app.ui.handle(Event::PointerMove { pos }).actions);
    actions.extend(
        app.ui
            .handle(Event::PointerDown {
                pos,
                button: PointerButton::Primary,
            })
            .actions,
    );
    actions.extend(
        app.ui
            .handle(Event::PointerUp {
                pos,
                button: PointerButton::Primary,
            })
            .actions,
    );
    actions.extend(app.ui.take_actions());
    app.actions(actions).unwrap();
}

pub fn button(app: &mut SheetApp, cmd: Cmd) {
    let n = app
        .node_of(cmd)
        .unwrap_or_else(|| panic!("no button for {cmd:?}"));
    click(app, n);
}

/// The middle of cell `(r, c)` in window coordinates.
pub fn cell_pos(app: &mut SheetApp, r: u32, c: u32) -> Point {
    app.ui.layout();
    let g = app.ui.rect(app.grid_node().unwrap()).unwrap();
    let (x, y, w, h) = app
        .core
        .frame()
        .cell(r, c)
        .unwrap_or_else(|| panic!("cell {r},{c} not visible"));
    Point::new(g.x + x + w / 2.0, g.y + y + h / 2.0)
}

pub fn ev_down(pos: Point) -> Event {
    Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    }
}

pub fn ev_up(pos: Point) -> Event {
    Event::PointerUp {
        pos,
        button: PointerButton::Primary,
    }
}

pub fn outs(app: &mut SheetApp) -> Vec<Out> {
    app.take_out()
}

pub fn put(app: &mut SheetApp, r: u32, c: u32, text: &str) {
    app.core.sel.select(r, c);
    type_text(app, text);
    plain(app, Key::Enter);
}
