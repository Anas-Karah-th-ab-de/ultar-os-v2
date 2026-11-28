//! The sheet window headless: it paints in Arabic and English, dark and light, at three sizes
//! and in every state; opening, typing, saving, closing with changes, crash recovery, the
//! mesh report, pointer and finger on the grid, and a sheet of 100 000 rows scrolling within
//! its budget. With `ULTAR_SHEET_SHOTS=<dir>` the pictures are written there for review.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use ultar_i18n::Lang;
use ultar_office::sheet::{Sheet, Workbook};
use ultar_office::style::CellStyle;
use ultar_sheet::agent::{Reply, Request};
use ultar_sheet::app::{Out, SheetApp, State};
use ultar_sheet::ask::Ask;
use ultar_sheet::book::Book;
use ultar_sheet::cmd::Cmd;
use ultar_sheet::file::{self, DocError, Format};
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::geom::Point;
use ultar_ui::paint::PaintEnv;
use ultar_ui::snapshot::{render, Image};
use ultar_ui::tokens::{InputMode, SizeClass, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::Size;

const COMPACT: (f32, f32) = (360.0, 640.0);
const MEDIUM: (f32, f32) = (800.0, 600.0);
const WIDE: (f32, f32) = (1280.0, 800.0);

struct Dirs {
    areas: PathBuf,
    recover: PathBuf,
}

fn dirs(name: &str) -> Dirs {
    let d = std::env::temp_dir().join(format!("ultar-sheet-win-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("areas/documents")).unwrap();
    Dirs {
        areas: d.join("areas"),
        recover: d.join("recover"),
    }
}

fn ui(lang: Lang, theme: ThemeKind, mode: InputMode, size: (f32, f32)) -> Ui {
    Ui::new(
        PaintEnv::new(lang, theme),
        mode,
        Size::new(size.0, size.1),
        ultar_sheet::i18n::translator(lang),
    )
    .unwrap()
}

fn app_with(
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

fn app(d: &Dirs, lang: Lang, theme: ThemeKind, size: (f32, f32)) -> SheetApp {
    app_with(d, Book::new(), lang, theme, InputMode::Pointer, size)
}

/// A small workbook with headers, numbers, formulas, formats and Arabic.
fn sample(rtl: bool) -> Book {
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

fn shot(app: &mut SheetApp, name: &str) -> Image {
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

fn key(app: &mut SheetApp, key: Key, m: Modifiers) {
    app.key(Event::Key { key, modifiers: m }).unwrap();
}

fn plain(app: &mut SheetApp, k: Key) {
    key(app, k, Modifiers::NONE);
}

fn ctrl(app: &mut SheetApp, c: char) {
    key(
        app,
        Key::Char(c),
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    );
}

fn type_text(app: &mut SheetApp, text: &str) {
    for c in text.chars() {
        plain(app, if c == ' ' { Key::Space } else { Key::Char(c) });
    }
}

fn click(app: &mut SheetApp, id: NodeId) {
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

fn button(app: &mut SheetApp, cmd: Cmd) {
    let n = app
        .node_of(cmd)
        .unwrap_or_else(|| panic!("no button for {cmd:?}"));
    click(app, n);
}

/// The middle of cell `(r, c)` in window coordinates.
fn cell_pos(app: &mut SheetApp, r: u32, c: u32) -> Point {
    app.ui.layout();
    let g = app.ui.rect(app.grid_node().unwrap()).unwrap();
    let (x, y, w, h) = app
        .core
        .frame()
        .cell(r, c)
        .unwrap_or_else(|| panic!("cell {r},{c} not visible"));
    Point::new(g.x + x + w / 2.0, g.y + y + h / 2.0)
}

fn ev_down(pos: Point) -> Event {
    Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    }
}

fn ev_up(pos: Point) -> Event {
    Event::PointerUp {
        pos,
        button: PointerButton::Primary,
    }
}

fn outs(app: &mut SheetApp) -> Vec<Out> {
    app.take_out()
}

#[test]
fn paints_in_four_variants_at_three_sizes() {
    let d = dirs("variants");
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            for (size_name, size) in [("compact", COMPACT), ("medium", MEDIUM), ("wide", WIDE)] {
                let mut a = app_with(
                    &d,
                    sample(lang == Lang::Ar),
                    lang,
                    theme,
                    InputMode::Pointer,
                    size,
                );
                let tag = format!("{lang:?}-{}-{size_name}", theme.name()).to_lowercase();
                let class = a.class();
                assert_eq!(
                    class,
                    match size_name {
                        "compact" => SizeClass::Compact,
                        "medium" => SizeClass::Medium,
                        _ => SizeClass::Wide,
                    }
                );
                a.core.sel.select(1, 1);
                a.refresh().unwrap();
                shot(&mut a, &format!("sheet-{tag}"));
                // a range selected and the second tool bar open on a phone
                a.core.sel.extend_to(3, 2);
                if class == SizeClass::Compact {
                    button(&mut a, Cmd::Tools);
                }
                shot(&mut a, &format!("sheet-range-{tag}"));
            }
        }
    }
}

#[test]
fn an_arabic_sheet_runs_from_the_right_and_an_english_one_from_the_left() {
    let d = dirs("rtl");
    for rtl in [true, false] {
        let mut a = app_with(
            &d,
            sample(rtl),
            Lang::Ar,
            ThemeKind::Dark,
            InputMode::Pointer,
            WIDE,
        );
        let a1 = cell_pos(&mut a, 1, 0);
        let b1 = cell_pos(&mut a, 1, 1);
        assert_eq!(
            a1.x > b1.x,
            rtl,
            "column A is at the {} edge",
            if rtl { "right" } else { "left" }
        );
        // a click on column A selects it wherever it is
        let mut a2 = app_with(
            &d,
            sample(rtl),
            Lang::En,
            ThemeKind::Light,
            InputMode::Pointer,
            WIDE,
        );
        let p = cell_pos(&mut a2, 2, 0);
        a2.pointer(ev_down(p)).unwrap();
        a2.pointer(ev_up(p)).unwrap();
        assert_eq!(a2.core.sel.active, (2, 0));
    }
}

#[test]
fn the_states_are_shown_and_have_a_way_out() {
    let d = dirs("states");
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            let tag = format!("{lang:?}-{}", theme.name()).to_lowercase();
            // empty: a new workbook says what to do
            let mut a = app(&d, lang, theme, WIDE);
            assert!(a.core.book.sheet().cells.is_empty());
            shot(&mut a, &format!("state-empty-{tag}"));
            // loading
            a.loading("حسابات.xlsx").unwrap();
            assert!(matches!(a.state(), State::Loading(_)));
            shot(&mut a, &format!("state-loading-{tag}"));
            // damaged
            a.loaded(PathBuf::from("x.xlsx"), Err(DocError::Damaged))
                .unwrap();
            assert_eq!(a.state(), &State::Damaged(DocError::Damaged));
            shot(&mut a, &format!("state-damaged-{tag}"));
            assert!(a.node_of(Cmd::New).is_some(), "a way out");
            // too large
            a.loaded(PathBuf::from("x.xlsx"), Err(DocError::TooBig))
                .unwrap();
            assert_eq!(a.state(), &State::TooBig);
            shot(&mut a, &format!("state-toobig-{tag}"));
            // "New" leaves the state
            button(&mut a, Cmd::New);
            assert_eq!(a.state(), &State::Ready);
        }
    }
    // compact too
    let mut a = app(&d, Lang::Ar, ThemeKind::Dark, COMPACT);
    a.loaded(PathBuf::from("x.xlsx"), Err(DocError::Damaged))
        .unwrap();
    shot(&mut a, "state-damaged-compact");
}

#[test]
fn a_real_damaged_file_and_a_real_workbook_open_from_a_uri() {
    let d = dirs("open");
    std::fs::write(d.areas.join("documents/broken.xlsx"), b"PK not really").unwrap();
    let book = sample(true);
    file::save(
        &d.areas.join("documents/حسابات.xlsx"),
        &book.wb,
        Format::Xlsx,
        0,
    )
    .unwrap();
    let mut a = app(&d, Lang::Ar, ThemeKind::Dark, WIDE);
    outs(&mut a);
    // the workbook, at a sheet and a cell
    a.open_uri("files://documents/%D8%AD%D8%B3%D8%A7%D8%A8%D8%A7%D8%AA.xlsx#sheet=Sheet2&cell=C5")
        .unwrap();
    let load = outs(&mut a)
        .into_iter()
        .find_map(|o| if let Out::Load(p) = o { Some(p) } else { None })
        .unwrap();
    assert!(matches!(a.state(), State::Loading(_)));
    a.loaded(load.clone(), file::open(&load)).unwrap();
    assert_eq!(a.state(), &State::Ready);
    assert_eq!(a.core.book.wb.sheets.len(), 2);
    assert_eq!((a.core.book.active, a.core.sel.active), (1, (4, 2)));
    let rep = a.report();
    assert_eq!(rep.sheet, "Sheet2");
    assert_eq!(rep.cell, "C5");
    assert_eq!(
        rep.uri.as_deref(),
        Some("files://documents/%D8%AD%D8%B3%D8%A7%D8%A8%D8%A7%D8%AA.xlsx#sheet=Sheet2&cell=C5")
    );
    // formulas of the file are calculated
    a.core.show_sheet(0);
    assert_eq!(a.core.book.shown(1, 3).text, "30");
    assert_eq!(a.core.book.shown(6, 3).text, "#DIV/0!");
    // the broken one
    let broken = d.areas.join("documents/broken.xlsx");
    a.loaded(broken.clone(), file::open(&broken)).unwrap();
    assert_eq!(a.state(), &State::Damaged(DocError::Damaged));
    // a place outside the areas is refused with a message, not opened
    let mut b = app(&d, Lang::En, ThemeKind::Dark, WIDE);
    outs(&mut b);
    b.open_uri("files://documents/../../etc/passwd").unwrap();
    assert!(!outs(&mut b).iter().any(|o| matches!(o, Out::Load(_))));
}

#[test]
fn typing_saving_and_closing_with_changes() {
    let d = dirs("save");
    let mut a = app(&d, Lang::En, ThemeKind::Dark, WIDE);
    outs(&mut a);
    assert!(a.grid_focused());
    type_text(&mut a, "42");
    plain(&mut a, Key::Enter);
    type_text(&mut a, "=A1+8");
    plain(&mut a, Key::Enter);
    assert_eq!(a.core.book.shown(1, 0).text, "50");
    assert!(a.title().starts_with("• "));
    // Ctrl+S on a new workbook asks for a name in Documents
    ctrl(&mut a, 's');
    assert!(matches!(a.question(), Some(Ask::Name(_))));
    let [ok, _, _, field] = a.dialog_nodes();
    a.ui.set_text(field.unwrap(), "totals").unwrap();
    click(&mut a, ok.unwrap());
    assert!(a.question().is_none());
    let saved = d.areas.join("documents/totals.xlsx");
    assert!(saved.exists());
    assert!(!a.core.book.dirty());
    let back = file::open(&saved).unwrap();
    assert_eq!(back.book.sheets[0].get(1, 0), "=A1+8");
    // closing without changes closes at once
    a.close_requested().unwrap();
    assert!(outs(&mut a).contains(&Out::Close));
    // with changes it asks; "don't save" closes, cancel stays
    put(&mut a, 5, 5, "x");
    a.close_requested().unwrap();
    assert!(!outs(&mut a).contains(&Out::Close));
    assert!(matches!(a.question(), Some(Ask::Unsaved(_))));
    let [_, cancel, discard, _] = a.dialog_nodes();
    click(&mut a, cancel.unwrap());
    assert!(a.question().is_none() && a.core.book.dirty());
    a.close_requested().unwrap();
    let [ok, _, discard2, _] = a.dialog_nodes();
    let _ = (discard, discard2);
    click(&mut a, discard2.unwrap());
    assert!(outs(&mut a).contains(&Out::Close));
    // "save" in the question saves and closes
    let mut b = app(&d, Lang::Ar, ThemeKind::Light, WIDE);
    outs(&mut b);
    b.core.book.path = Some(saved.clone());
    put(&mut b, 0, 0, "7");
    b.close_requested().unwrap();
    let [ok2, ..] = b.dialog_nodes();
    let _ = ok;
    click(&mut b, ok2.unwrap());
    assert!(outs(&mut b).contains(&Out::Close));
    assert_eq!(file::open(&saved).unwrap().book.sheets[0].get(0, 0), "7");
}

fn put(app: &mut SheetApp, r: u32, c: u32, text: &str) {
    app.core.sel.select(r, c);
    type_text(app, text);
    plain(app, Key::Enter);
}

#[test]
fn csv_in_windows_1256_opens_and_saves_back_in_it() {
    let d = dirs("csv");
    let (bytes, _) = file::write_bytes(
        &sample(false).wb,
        Format::Csv {
            enc: ultar_office::code::Enc::Win1256,
            sep: ';',
            eol: ultar_office::code::Eol::CrLf,
        },
        0,
    )
    .unwrap();
    let p = d.areas.join("documents/قائمة.csv");
    std::fs::write(&p, &bytes).unwrap();
    let mut a = app(&d, Lang::Ar, ThemeKind::Dark, MEDIUM);
    a.loaded(p.clone(), file::open(&p)).unwrap();
    assert_eq!(a.core.book.sheet().get(1, 0), "تفاح");
    put(&mut a, 8, 0, "جديد");
    ctrl(&mut a, 's');
    let again = std::fs::read(&p).unwrap();
    assert!(
        !again.starts_with(&[0xEF, 0xBB, 0xBF]),
        "still Windows-1256"
    );
    assert_eq!(file::open(&p).unwrap().book.sheets[0].get(8, 0), "جديد");
    // the message says a CSV keeps only values
    assert!(a.core.message.is_some());
}

#[test]
fn the_formula_bar_edits_the_active_cell() {
    let d = dirs("bar");
    let mut a = app_with(
        &d,
        sample(false),
        Lang::En,
        ThemeKind::Dark,
        InputMode::Pointer,
        WIDE,
    );
    a.core.sel.select(1, 1);
    a.refresh().unwrap();
    let f = a.formula_node().unwrap();
    assert_eq!(a.ui.text(f).unwrap(), "12");
    // the bar follows the selection
    a.core.sel.select(1, 3);
    a.refresh().unwrap();
    assert_eq!(a.ui.text(f).unwrap(), "=B2*C2");
}

#[test]
fn mouse_selects_cells_ranges_rows_columns_and_resizes() {
    let d = dirs("mouse");
    let mut a = app_with(
        &d,
        sample(false),
        Lang::En,
        ThemeKind::Dark,
        InputMode::Pointer,
        WIDE,
    );
    let p = cell_pos(&mut a, 2, 1);
    a.pointer(ev_down(p)).unwrap();
    let q = cell_pos(&mut a, 4, 2);
    a.pointer(Event::PointerMove { pos: q }).unwrap();
    a.pointer(ev_up(q)).unwrap();
    assert_eq!(a.core.sel.rect(), (2, 1, 4, 2));
    // a drag from a row number selects rows
    let g = a.ui.rect(a.grid_node().unwrap()).unwrap();
    let fr = a.core.frame();
    let row = fr.row(2).unwrap();
    let hp = Point::new(g.x + 10.0, g.y + row.pos + row.size / 2.0);
    a.pointer(ev_down(hp)).unwrap();
    a.pointer(ev_up(hp)).unwrap();
    assert_eq!(a.core.sel.rect().0, 2);
    assert_eq!(a.core.sel.span, ultar_sheet::sel::Span::Rows);
    // and from a column letter
    let col = fr.col(1).unwrap();
    let cp = Point::new(g.x + col.pos + col.size / 2.0, g.y + 8.0);
    a.pointer(ev_down(cp)).unwrap();
    a.pointer(ev_up(cp)).unwrap();
    assert_eq!(a.core.sel.span, ultar_sheet::sel::Span::Cols);
    // Shift extends
    a.pointer(Event::Modifiers {
        modifiers: Modifiers::SHIFT,
    })
    .unwrap();
    let cp2 = Point::new(g.x + fr.col(3).unwrap().pos + 5.0, g.y + 8.0);
    a.pointer(ev_down(cp2)).unwrap();
    a.pointer(ev_up(cp2)).unwrap();
    assert_eq!(a.core.sel.rect().1, 1);
    assert_eq!(a.core.sel.rect().3, 3);
    a.pointer(Event::Modifiers {
        modifiers: Modifiers::NONE,
    })
    .unwrap();
    // drag the border of column B wider
    let before = a.core.book.sheet().width(1);
    let edge = Point::new(g.x + col.pos + col.size - 1.0, g.y + 8.0);
    a.pointer(ev_down(edge)).unwrap();
    a.pointer(Event::PointerMove {
        pos: Point::new(edge.x + 40.0, edge.y),
    })
    .unwrap();
    a.pointer(ev_up(Point::new(edge.x + 40.0, edge.y))).unwrap();
    assert!(
        a.core.book.sheet().width(1) > before + 4.0,
        "{} -> {}",
        before,
        a.core.book.sheet().width(1)
    );
    // and the bottom border of a row taller
    let fr = a.core.frame();
    let r3 = fr.row(3).unwrap();
    let h0 = a.core.book.sheet().heights.get(&3).copied().unwrap_or(0.0);
    let edge = Point::new(g.x + 10.0, g.y + r3.pos + r3.size - 1.0);
    a.pointer(ev_down(edge)).unwrap();
    a.pointer(Event::PointerMove {
        pos: Point::new(edge.x, edge.y + 18.0),
    })
    .unwrap();
    a.pointer(ev_up(Point::new(edge.x, edge.y + 18.0))).unwrap();
    assert!(a.core.book.sheet().heights.get(&3).copied().unwrap_or(0.0) > h0 + 10.0);
    // the wheel scrolls
    let before = a.core.first.0;
    a.pointer(Event::Scroll { pos: p, lines: 2.0 }).unwrap();
    assert!(a.core.first.0 > before);
}

#[test]
fn a_double_click_edits_in_the_cell() {
    let d = dirs("double");
    let mut a = app_with(
        &d,
        sample(false),
        Lang::En,
        ThemeKind::Dark,
        InputMode::Pointer,
        WIDE,
    );
    let p = cell_pos(&mut a, 1, 0);
    for _ in 0..2 {
        a.pointer(ev_down(p)).unwrap();
        a.pointer(ev_up(p)).unwrap();
    }
    assert!(a.core.edit.is_some());
    type_text(&mut a, "!");
    plain(&mut a, Key::Enter);
    assert_eq!(a.core.book.sheet().get(1, 0), "تفاح!");
}

#[test]
fn touch_taps_select_drags_scroll_and_extend_widens() {
    let d = dirs("touch");
    let mut book = sample(false);
    for r in 10..200 {
        book.wb.sheets[0].set(r, 0, &r.to_string());
    }
    let mut a = app_with(
        &d,
        book,
        Lang::Ar,
        ThemeKind::Dark,
        InputMode::Touch,
        COMPACT,
    );
    let p = cell_pos(&mut a, 2, 0);
    a.pointer(Event::TouchDown { id: 1, pos: p }).unwrap();
    a.pointer(Event::TouchUp { id: 1, pos: p }).unwrap();
    assert_eq!(a.core.sel.active, (2, 0));
    // a drag scrolls the rows (and does not select)
    let from = cell_pos(&mut a, 8, 0);
    let to = Point::new(from.x, from.y - 120.0);
    a.pointer(Event::TouchDown { id: 1, pos: from }).unwrap();
    a.pointer(Event::TouchMove {
        id: 1,
        pos: Point::new(from.x, from.y - 40.0),
    })
    .unwrap();
    a.pointer(Event::TouchMove { id: 1, pos: to }).unwrap();
    a.pointer(Event::TouchUp { id: 1, pos: to }).unwrap();
    assert!(a.core.first.0 > 0, "scrolled");
    assert_eq!(a.core.sel.active, (2, 0), "a drag does not select");
    // the Extend button makes the next tap widen the selection
    button(&mut a, Cmd::Extend);
    let first = a.core.first;
    let q = cell_pos(&mut a, first.0 + 1, 1);
    a.pointer(Event::TouchDown { id: 1, pos: q }).unwrap();
    a.pointer(Event::TouchUp { id: 1, pos: q }).unwrap();
    assert!(!a.core.sel.single());
    assert_eq!(a.core.sel.anchor, (2, 0));
    // the finger sizes: bigger rows
    assert!(a.core.frame().rows[1].size >= 32.0);
}

#[test]
fn unsaved_changes_survive_a_crash_and_are_offered_back() {
    let d = dirs("crash");
    let p = d.areas.join("documents/a.xlsx");
    file::save(&p, &sample(false).wb, Format::Xlsx, 0).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    let mut a = app(&d, Lang::En, ThemeKind::Dark, WIDE);
    a.loaded(p.clone(), file::open(&p)).unwrap();
    put(&mut a, 20, 0, "never saved");
    // twenty seconds later the copy is written
    a.tick(Instant::now() + Duration::from_secs(30));
    drop(a);
    // the next start finds it
    let mut b = app(&d, Lang::En, ThemeKind::Dark, WIDE);
    b.loaded(p.clone(), file::open(&p)).unwrap();
    assert!(matches!(b.question(), Some(Ask::Recover(_))), "offered");
    assert_eq!(b.core.book.sheet().get(20, 0), "");
    let [ok, ..] = b.dialog_nodes();
    click(&mut b, ok.unwrap());
    assert_eq!(b.core.book.sheet().get(20, 0), "never saved");
    assert!(b.core.book.dirty(), "recovered changes are still unsaved");
    // saving removes the copy: the next start is quiet
    ctrl(&mut b, 's');
    let mut c = app(&d, Lang::En, ThemeKind::Dark, WIDE);
    c.loaded(p.clone(), file::open(&p)).unwrap();
    assert!(c.question().is_none());
    // "discard" in the offer removes the copy too
    put(&mut c, 21, 0, "again");
    c.tick(Instant::now() + Duration::from_secs(30));
    let mut e = app(&d, Lang::En, ThemeKind::Dark, WIDE);
    e.loaded(p.clone(), file::open(&p)).unwrap();
    let [_, cancel, ..] = e.dialog_nodes();
    click(&mut e, cancel.unwrap());
    let mut f = app(&d, Lang::En, ThemeKind::Dark, WIDE);
    f.loaded(p.clone(), file::open(&p)).unwrap();
    assert!(f.question().is_none());
}

#[test]
fn a_new_workbook_is_recovered_too() {
    let d = dirs("crash-new");
    let mut a = app(&d, Lang::Ar, ThemeKind::Dark, WIDE);
    put(&mut a, 0, 0, "غير محفوظ");
    a.tick(Instant::now() + Duration::from_secs(30));
    drop(a);
    let mut b = app(&d, Lang::Ar, ThemeKind::Dark, WIDE);
    b.offer_recovery_new().unwrap();
    assert!(matches!(b.question(), Some(Ask::Recover(_))));
    let [ok, ..] = b.dialog_nodes();
    click(&mut b, ok.unwrap());
    assert_eq!(b.core.book.sheet().get(0, 0), "غير محفوظ");
}

#[test]
fn the_assistant_asks_before_it_writes() {
    let d = dirs("agent");
    let mut a = app_with(
        &d,
        sample(false),
        Lang::En,
        ThemeKind::Dark,
        InputMode::Pointer,
        WIDE,
    );
    let r = a
        .agent(Request::ReadRange {
            range: "A2:B3".into(),
        })
        .unwrap();
    assert_eq!(r, Reply::Done("تفاح\t12\nBananas\t30\n".into()));
    let w = Request::WriteRange {
        range: "F1".into(),
        values: vec![vec!["hello".into()]],
    };
    assert_eq!(a.agent(w.clone()).unwrap(), Reply::Confirm);
    assert!(matches!(a.question(), Some(Ask::Agent(_))));
    assert_eq!(a.core.book.sheet().get(0, 5), "");
    shot(&mut a, "agent-ask");
    let [_, no, ..] = a.dialog_nodes();
    click(&mut a, no.unwrap());
    assert!(matches!(a.take_agent_reply(), Some(Reply::Refused(_))));
    assert_eq!(a.core.book.sheet().get(0, 5), "");
    assert_eq!(a.agent(w).unwrap(), Reply::Confirm);
    let [yes, ..] = a.dialog_nodes();
    click(&mut a, yes.unwrap());
    assert!(matches!(a.take_agent_reply(), Some(Reply::Done(_))));
    assert_eq!(a.core.book.sheet().get(0, 5), "hello");
    assert_eq!(
        a.agent(Request::AddSheet {
            name: "Summary".into()
        })
        .unwrap(),
        Reply::Confirm
    );
    let [yes, ..] = a.dialog_nodes();
    click(&mut a, yes.unwrap());
    assert_eq!(a.core.book.wb.sheets.len(), 3);
}

#[test]
fn find_replace_bar_and_sheet_tabs_work_through_the_buttons() {
    let d = dirs("bars");
    let mut a = app_with(
        &d,
        sample(false),
        Lang::En,
        ThemeKind::Light,
        InputMode::Pointer,
        WIDE,
    );
    ctrl(&mut a, 'h');
    assert!(a.core.find.open && a.core.find.replace);
    shot(&mut a, "find-bar");
    a.core.find.needle = "mango".into();
    a.core.find.with = "Papaya".into();
    button(&mut a, Cmd::FindNext);
    assert_eq!(a.core.sel.active, (4, 0));
    button(&mut a, Cmd::ReplaceOne);
    assert_eq!(a.core.book.sheet().get(4, 0), "Papaya");
    button(&mut a, Cmd::CloseFind);
    assert!(!a.core.find.open);
    // tabs: add, switch, rename through the dialog, delete, undo
    button(&mut a, Cmd::AddSheet);
    assert_eq!(a.core.book.wb.sheets.len(), 3);
    button(&mut a, Cmd::RenameSheet);
    let [ok, _, _, field] = a.dialog_nodes();
    a.ui.set_text(field.unwrap(), "Totals").unwrap();
    click(&mut a, ok.unwrap());
    assert_eq!(a.core.book.sheet().name, "Totals");
    button(&mut a, Cmd::Tab(0));
    assert_eq!(a.core.book.active, 0);
    button(&mut a, Cmd::Tab(2));
    button(&mut a, Cmd::DeleteSheet);
    assert_eq!(a.core.book.wb.sheets.len(), 2);
    button(&mut a, Cmd::Undo);
    assert_eq!(a.core.book.wb.sheets.len(), 3);
    shot(&mut a, "tabs");
}

#[test]
fn a_sheet_of_100_000_rows_scrolls_within_its_budget() {
    let d = dirs("perf");
    let mut sh = Sheet::new("Big");
    for r in 0..100_000u32 {
        sh.set(r, 0, &format!("item {r}"));
        sh.set(r, 1, &(r % 977).to_string());
        sh.set(r, 2, &format!("=B{}*2", r + 1));
        sh.set(r, 3, &format!("=IF(B{}>500,\"high\",\"low\")", r + 1));
    }
    sh.frozen_rows = 1;
    let book = Book::of(Workbook { sheets: vec![sh] }, None, Format::Xlsx);
    let mut a = app_with(
        &d,
        book,
        Lang::En,
        ThemeKind::Dark,
        InputMode::Pointer,
        WIDE,
    );
    // best of three sweeps: a busy machine only slows a sweep down, it never speeds one up
    let mut per_frame = Duration::MAX;
    let mut frames = 0u32;
    for _ in 0..3 {
        let start = Instant::now();
        frames = 0;
        for top in [0u32, 25_000, 50_000, 75_000, 99_950] {
            a.core.first.0 = top;
            for step in 0..20u32 {
                a.core.first.0 = top + step * 3;
                a.draw_grid().unwrap();
                frames += 1;
            }
        }
        per_frame = per_frame.min(start.elapsed() / frames);
    }
    println!("100k rows: {frames} frames, {per_frame:?} per frame (debug build, best of 3)");
    // 100 ms per frame in a debug build; the release build is several times faster
    assert!(
        per_frame < Duration::from_millis(100),
        "{per_frame:?} per frame"
    );
    // jumping with the keyboard to the last row shows it
    a.core.sel.select(0, 0);
    key(
        &mut a,
        Key::Home,
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    );
    key(
        &mut a,
        Key::ArrowDown,
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    );
    assert_eq!(a.core.sel.active.0, 99_999);
    assert!(a.core.frame().shows(99_999, 0));
    assert_eq!(
        a.core.book.shown(99_999, 2).text,
        ((99_999 % 977) * 2).to_string()
    );
}
