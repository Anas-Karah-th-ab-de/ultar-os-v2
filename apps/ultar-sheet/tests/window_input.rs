//! The sheet window headless, part two: pointer and finger on the grid, crash recovery, the
//! assistant, find and replace, the sheet tabs, and a sheet of 100 000 rows scrolling within
//! its budget. With `ULTAR_SHEET_SHOTS=<dir>` the pictures are written for review.

mod common;

use common::*;
use std::time::{Duration, Instant};
use ultar_i18n::Lang;
use ultar_office::sheet::{Sheet, Workbook};
use ultar_sheet::agent::{Reply, Request};
use ultar_sheet::ask::Ask;
use ultar_sheet::book::Book;
use ultar_sheet::cmd::Cmd;
use ultar_sheet::file::{self, Format};
use ultar_ui::event::{Event, Key, Modifiers};
use ultar_ui::geom::Point;
use ultar_ui::tokens::{InputMode, ThemeKind};

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
}

#[test]
fn borders_are_dragged_and_the_wheel_scrolls() {
    let d = dirs("borders");
    let mut a = app_with(
        &d,
        sample(false),
        Lang::En,
        ThemeKind::Dark,
        InputMode::Pointer,
        WIDE,
    );
    let p = cell_pos(&mut a, 2, 1);
    let g = a.ui.rect(a.grid_node().unwrap()).unwrap();
    let fr = a.core.frame();
    let col = fr.col(1).unwrap();
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

/// 100 000 rows of text, numbers and formulas, the first row frozen.
fn big_book() -> Book {
    let mut sh = Sheet::new("Big");
    for r in 0..100_000u32 {
        sh.set(r, 0, &format!("item {r}"));
        sh.set(r, 1, &(r % 977).to_string());
        sh.set(r, 2, &format!("=B{}*2", r + 1));
        sh.set(r, 3, &format!("=IF(B{}>500,\"high\",\"low\")", r + 1));
    }
    sh.frozen_rows = 1;
    Book::of(Workbook { sheets: vec![sh] }, None, Format::Xlsx)
}

#[test]
fn a_sheet_of_100_000_rows_scrolls_within_its_budget() {
    let d = dirs("perf");
    let mut a = app_with(
        &d,
        big_book(),
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
}

#[test]
fn ctrl_arrows_reach_the_last_row_of_a_big_sheet() {
    let d = dirs("jump");
    let mut a = app_with(
        &d,
        big_book(),
        Lang::En,
        ThemeKind::Dark,
        InputMode::Pointer,
        WIDE,
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
    assert!(
        a.core.frame().shows(99_999, 0),
        "first {:?} row {:?} view {:?}",
        a.core.first,
        a.core.frame().row(99_999),
        a.core.view
    );
    assert_eq!(
        a.core.book.shown(99_999, 2).text,
        ((99_999 % 977) * 2).to_string()
    );
}
