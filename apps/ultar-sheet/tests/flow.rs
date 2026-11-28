//! What a user does to a workbook, without a window: typing, formulas, undo, copy and paste,
//! fill, rows and columns, sort, find and replace, sheets, formats, the assistant.

use ultar_office::sheet::Value;
use ultar_office::sheet_refs::Axis;
use ultar_office::style::Align;
use ultar_sheet::agent::{Reply, Request};
use ultar_sheet::book::Book;
use ultar_sheet::core::{Core, Fx};
use ultar_sheet::ops::NumFmt;
use ultar_ui::event::{Key, Modifiers};

fn core() -> Core {
    Core::new(Book::new())
}

fn press(c: &mut Core, key: Key, m: Modifiers) -> Fx {
    c.key(key, m)
}

fn plain(c: &mut Core, key: Key) {
    press(c, key, Modifiers::NONE);
}

fn ctrl(c: &mut Core, ch: char) -> Fx {
    press(
        c,
        Key::Char(ch),
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    )
}

fn type_text(c: &mut Core, text: &str) {
    for ch in text.chars() {
        plain(c, if ch == ' ' { Key::Space } else { Key::Char(ch) });
    }
}

/// Selects `(r, c)`, types `text` and presses Enter.
fn put(c: &mut Core, r: u32, col: u32, text: &str) {
    c.sel.select(r, col);
    type_text(c, text);
    plain(c, Key::Enter);
}

fn num(c: &mut Core, r: u32, col: u32) -> f64 {
    match c.book.value(r, col) {
        Value::Num(n) => n,
        other => panic!("({r},{col}) is {other:?}"),
    }
}

#[test]
fn typing_enter_and_tab_move_on_and_formulas_follow() {
    let mut c = core();
    type_text(&mut c, "5");
    plain(&mut c, Key::Enter);
    assert_eq!(c.sel.active, (1, 0), "Enter goes down");
    type_text(&mut c, "=A1*2");
    plain(&mut c, Key::Tab);
    assert_eq!(c.sel.active, (1, 1), "Tab goes right");
    assert_eq!(num(&mut c, 1, 0), 10.0);
    // changing A1 recalculates A2 without anyone asking
    put(&mut c, 0, 0, "7");
    assert_eq!(num(&mut c, 1, 0), 14.0);
    // Escape gives the edit up
    c.sel.select(3, 3);
    type_text(&mut c, "junk");
    plain(&mut c, Key::Escape);
    assert_eq!(c.book.sheet().get(3, 3), "");
    // F2 edits what is there, the caret at the end
    c.sel.select(1, 0);
    plain(&mut c, Key::F(2));
    plain(&mut c, Key::Backspace);
    type_text(&mut c, "3");
    plain(&mut c, Key::Enter);
    assert_eq!(num(&mut c, 1, 0), 21.0);
}

#[test]
fn a_formula_that_does_not_parse_stays_in_the_edit() {
    let mut c = core();
    type_text(&mut c, "=SUM(");
    plain(&mut c, Key::Enter);
    assert!(c.edit.is_some(), "the edit stays open");
    assert!(c.message.is_some());
    assert_eq!(c.book.sheet().get(0, 0), "");
    for _ in 0..5 {
        plain(&mut c, Key::Backspace);
    }
    type_text(&mut c, "=1/0");
    plain(&mut c, Key::Enter);
    assert_eq!(c.book.shown(0, 0).text, "#DIV/0!");
    assert!(c.book.shown(0, 0).error);
}

#[test]
fn undo_and_redo_with_the_keyboard() {
    let mut c = core();
    put(&mut c, 0, 0, "1");
    put(&mut c, 0, 0, "2");
    ctrl(&mut c, 'z');
    assert_eq!(c.book.sheet().get(0, 0), "1");
    ctrl(&mut c, 'z');
    assert_eq!(c.book.sheet().get(0, 0), "");
    ctrl(&mut c, 'y');
    ctrl(&mut c, 'y');
    assert_eq!(c.book.sheet().get(0, 0), "2");
    // Delete clears and can be undone
    c.sel.select(0, 0);
    plain(&mut c, Key::Delete);
    assert_eq!(c.book.sheet().get(0, 0), "");
    ctrl(&mut c, 'z');
    assert_eq!(c.book.sheet().get(0, 0), "2");
}

#[test]
fn shift_arrows_select_ranges_and_the_status_adds_up() {
    let mut c = core();
    for (i, v) in ["1", "2", "3", "4"].iter().enumerate() {
        put(&mut c, i as u32, 0, v);
    }
    c.sel.select(0, 0);
    let shift = Modifiers::SHIFT;
    press(&mut c, Key::ArrowDown, shift);
    press(&mut c, Key::ArrowDown, shift);
    press(&mut c, Key::ArrowRight, shift);
    assert_eq!(c.sel.rect(), (0, 0, 2, 1));
    assert_eq!(c.sel.active, (0, 0));
    let (sum, avg, n) = c.stats().unwrap();
    assert_eq!((sum, avg, n), (6.0, 2.0, 3));
    // Ctrl+A takes everything used
    ctrl(&mut c, 'a');
    assert_eq!(c.sel.rect(), (0, 0, 3, 0));
    // Ctrl+arrow jumps to the end of the data
    c.sel.select(0, 0);
    press(
        &mut c,
        Key::ArrowDown,
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    );
    assert_eq!(c.sel.active, (3, 0));
}

#[test]
fn rows_and_columns_are_selected_whole() {
    let mut c = core();
    put(&mut c, 2, 3, "x");
    c.sel.row(2, false);
    c.sel.row(4, true);
    assert_eq!(c.used_rect(), (2, 0, 4, 3));
    c.sel.col(3, false);
    assert_eq!(c.used_rect(), (0, 3, 2, 3));
}

#[test]
fn copy_paste_moves_formulas_and_formats_and_takes_text_from_other_apps() {
    let mut c = core();
    put(&mut c, 0, 0, "10");
    put(&mut c, 0, 1, "=A1+1");
    c.sel.select(0, 0);
    c.sel.extend_to(0, 1);
    ctrl(&mut c, 'b');
    let Fx::Copy(text) = ctrl(&mut c, 'c') else {
        panic!("copy");
    };
    assert_eq!(text, "10\t11\n");
    // paste two rows below: =A3+1 follows the block, bold comes along
    c.sel.select(2, 0);
    c.paste(&text);
    assert_eq!(c.book.sheet().get(2, 1), "=A3+1");
    assert!(c.book.sheet().style(2, 1).bold);
    assert_eq!(num(&mut c, 2, 1), 11.0);
    // text from another app: tab separated, quoted fields, Arabic
    c.sel.select(5, 0);
    c.paste("اسم\tقيمة\n\"a\tb\"\t3\n");
    assert_eq!(c.book.sheet().get(5, 0), "اسم");
    assert_eq!(c.book.sheet().get(6, 0), "a\tb");
    assert_eq!(c.book.sheet().get(6, 1), "3");
    assert_eq!(c.sel.rect(), (5, 0, 6, 1));
    // one undo step takes the whole paste back
    ctrl(&mut c, 'z');
    assert_eq!(c.book.sheet().get(6, 1), "");
    // cut clears the source
    c.sel.select(0, 0);
    let Fx::Copy(_) = ctrl(&mut c, 'x') else {
        panic!("cut");
    };
    assert_eq!(c.book.sheet().get(0, 0), "");
}

#[test]
fn pasting_into_an_edit_goes_into_the_edit() {
    let mut c = core();
    type_text(&mut c, "ab");
    c.paste("cd\nef");
    plain(&mut c, Key::Enter);
    assert_eq!(c.book.sheet().get(0, 0), "abcd ef");
}

#[test]
fn fill_down_and_right_move_relative_references() {
    let mut c = core();
    put(&mut c, 0, 0, "1");
    put(&mut c, 0, 1, "=A1*2");
    c.sel.select(0, 1);
    c.sel.extend_to(3, 1);
    ctrl(&mut c, 'd');
    assert_eq!(c.book.sheet().get(3, 1), "=A4*2");
    put(&mut c, 5, 0, "=A1+B1");
    c.sel.select(5, 0);
    c.sel.extend_to(5, 2);
    ctrl(&mut c, 'r');
    assert_eq!(c.book.sheet().get(5, 1), "=B1+C1");
    assert_eq!(c.book.sheet().get(5, 2), "=C1+D1");
}

#[test]
fn inserting_and_deleting_rows_keeps_formulas_pointing_at_their_data() {
    let mut c = core();
    put(&mut c, 0, 0, "1");
    put(&mut c, 1, 0, "2");
    put(&mut c, 2, 0, "=SUM(A1:A2)");
    c.sel.select(1, 0);
    c.insert_lines(Axis::Rows);
    assert_eq!(c.book.sheet().get(3, 0), "=SUM(A1:A3)");
    assert_eq!(c.book.sheet().get(2, 0), "2");
    c.sel.select(0, 0);
    c.delete_lines(Axis::Rows);
    assert_eq!(c.book.sheet().get(2, 0), "=SUM(A1:A2)");
    assert_eq!(num(&mut c, 2, 0), 2.0);
    // columns too
    c.sel.select(0, 0);
    c.insert_lines(Axis::Cols);
    assert_eq!(c.book.sheet().get(0, 1), "");
    assert_eq!(c.book.sheet().get(2, 1), "=SUM(B1:B2)");
    // every structural change is one undo step
    for _ in 0..3 {
        ctrl(&mut c, 'z');
    }
    assert_eq!(c.book.sheet().get(2, 0), "=SUM(A1:A2)");
    assert_eq!(c.book.sheet().get(1, 0), "2");
}

#[test]
fn sort_keeps_the_frozen_header_and_moves_whole_rows() {
    let mut c = core();
    for (r, (a, b)) in [("n", "name"), ("3", "c"), ("1", "a"), ("2", "b")]
        .iter()
        .enumerate()
    {
        put(&mut c, r as u32, 0, a);
        put(&mut c, r as u32, 1, b);
    }
    c.toggle_freeze_row();
    c.sel.select(1, 0);
    c.sort(true);
    let col = |c: &Core, k: u32| {
        (0..4)
            .map(|r| c.book.sheet().get(r, k).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(col(&c, 0), ["n", "1", "2", "3"]);
    assert_eq!(col(&c, 1), ["name", "a", "b", "c"]);
    c.sort(false);
    assert_eq!(col(&c, 1), ["name", "c", "b", "a"]);
    ctrl(&mut c, 'z');
    assert_eq!(col(&c, 1), ["name", "a", "b", "c"]);
}

#[test]
fn arabic_text_sorts_by_the_alphabet() {
    let mut c = core();
    for (i, w) in ["ياسمين", "أحمد", "بسمة", "ليلى"].iter().enumerate() {
        put(&mut c, i as u32, 0, w);
    }
    c.sel.select(0, 0);
    c.sort(true);
    let got: Vec<_> = (0..4)
        .map(|r| c.book.sheet().get(r, 0).to_owned())
        .collect();
    assert_eq!(got, ["أحمد", "بسمة", "ليلى", "ياسمين"]);
}

#[test]
fn find_and_replace_across_sheets_ignoring_arabic_marks() {
    let mut c = core();
    put(&mut c, 0, 0, "مُحَمَّد");
    put(&mut c, 3, 1, "الاسم محمد");
    c.add_sheet();
    put(&mut c, 0, 0, "محمد");
    c.show_sheet(0);
    c.sel.select(0, 0);
    c.find.needle = "محمد".into();
    // the vowel marks do not matter; hits come in order and wrap around, over the sheets
    assert!(c.find_next(true));
    assert_eq!((c.book.active, c.sel.active), (0, (3, 1)));
    assert!(c.find_next(true));
    assert_eq!((c.book.active, c.sel.active), (1, (0, 0)));
    assert!(c.find_next(true));
    assert_eq!((c.book.active, c.sel.active), (0, (0, 0)));
    assert_eq!(c.find.hits, 3);
    c.find.needle = "nothing".into();
    assert!(!c.find_next(true));
    // replace all, one undo step
    c.find.needle = "الاسم".into();
    c.find.with = "الكنية".into();
    assert_eq!(c.replace_all(), 1);
    assert_eq!(c.book.sheet().get(3, 1), "الكنية محمد");
    ctrl(&mut c, 'z');
    assert_eq!(c.book.sheet().get(3, 1), "الاسم محمد");
}

#[test]
fn sheets_can_be_added_renamed_deleted_and_the_delete_undone() {
    let mut c = core();
    put(&mut c, 0, 0, "1");
    c.add_sheet();
    assert_eq!(c.book.wb.sheets.len(), 2);
    assert_eq!(c.book.active, 1);
    put(&mut c, 0, 0, "=Sheet1!A1+1");
    assert_eq!(num(&mut c, 0, 0), 2.0);
    // rename: the formula follows
    let name = c.book.wb.sheets[0].name.clone();
    c.rename_sheet(0, "Data").unwrap();
    assert_eq!(c.book.sheet().get(0, 0), "=Data!A1+1");
    assert!(c.rename_sheet(1, "Data").is_err());
    assert!(c.rename_sheet(1, "").is_err());
    assert_eq!(name, "Sheet1");
    // delete sheet 1, then bring it back
    c.delete_sheet(1).unwrap();
    assert_eq!(c.book.wb.sheets.len(), 1);
    assert!(c.delete_sheet(0).is_err(), "the last sheet stays");
    c.undo();
    assert_eq!(c.book.wb.sheets.len(), 2);
    assert_eq!(c.book.wb.sheets[1].get(0, 0), "=Data!A1+1");
    // every sheet keeps its own selection
    c.show_sheet(0);
    c.sel.select(4, 4);
    c.show_sheet(1);
    c.show_sheet(0);
    assert_eq!(c.sel.active, (4, 4));
}

#[test]
fn formats_show_in_the_cell_and_toggle() {
    let mut c = core();
    put(&mut c, 0, 0, "0.256");
    put(&mut c, 1, 0, "1234.5");
    c.sel.select(0, 0);
    c.set_num_fmt(NumFmt::Percent);
    assert_eq!(c.book.shown(0, 0).text, "25.60%");
    c.sel.select(1, 0);
    c.set_num_fmt(NumFmt::Number);
    assert_eq!(c.book.shown(1, 0).text, "1,234.50");
    c.set_num_fmt(NumFmt::Currency);
    assert_eq!(c.book.shown(1, 0).text, "$1,234.50");
    c.set_color(Some(0xC62828));
    c.set_fill(Some(0xFFF59D));
    c.set_align(Align::Center);
    c.toggle_wrap();
    c.toggle_bold();
    c.toggle_italic();
    let st = c.book.sheet().style(1, 0);
    assert!(st.bold && st.italic && st.wrap);
    assert_eq!(
        (st.color, st.fill, st.align),
        (Some(0xC62828), Some(0xFFF59D), Align::Center)
    );
    c.toggle_bold();
    assert!(!c.book.sheet().style(1, 0).bold);
    // dates
    put(&mut c, 2, 0, "=DATE(2026,3,9)");
    c.sel.select(2, 0);
    c.set_num_fmt(NumFmt::Date);
    assert_eq!(c.book.shown(2, 0).text, "2026-03-09");
}

#[test]
fn an_rtl_sheet_moves_the_arrows_the_other_way() {
    let mut c = core();
    c.toggle_rtl();
    c.sel.select(0, 3);
    plain(&mut c, Key::ArrowLeft);
    assert_eq!(
        c.sel.active,
        (0, 4),
        "left in a right-to-left sheet is the next column"
    );
    plain(&mut c, Key::ArrowRight);
    assert_eq!(c.sel.active, (0, 3));
}

#[test]
fn the_assistant_reads_freely_and_writes_only_after_the_users_yes() {
    let mut c = core();
    put(&mut c, 0, 0, "4");
    put(&mut c, 0, 1, "=A1*2");
    let Reply::Done(text) = c.agent(Request::ReadRange {
        range: "A1:B1".into(),
    }) else {
        panic!("read");
    };
    assert_eq!(text, "4\t8 [=A1*2]\n");
    assert!(matches!(
        c.agent(Request::ReadRange {
            range: "Nope!A1".into()
        }),
        Reply::Refused(_)
    ));
    // a write waits
    let req = Request::WriteRange {
        range: "A3".into(),
        values: vec![vec!["x".into(), "=A1+1".into()]],
    };
    assert_eq!(c.agent(req.clone()), Reply::Confirm);
    assert_eq!(
        c.book.sheet().get(2, 0),
        "",
        "nothing written before the answer"
    );
    assert!(matches!(c.agent_answer(false), Reply::Refused(_)));
    assert_eq!(c.book.sheet().get(2, 0), "");
    assert_eq!(c.agent(req), Reply::Confirm);
    assert!(matches!(c.agent_answer(true), Reply::Done(_)));
    assert_eq!(c.book.sheet().get(2, 1), "=A1+1");
    // one undo step
    c.undo();
    assert_eq!(c.book.sheet().get(2, 1), "");
    // a bad formula is refused before the user is asked
    let bad = Request::WriteRange {
        range: "A1".into(),
        values: vec![vec!["=SUM(".into()]],
    };
    assert!(matches!(c.agent(bad), Reply::Refused(_)));
    // a sheet is added after the yes, and a taken name is refused
    assert_eq!(
        c.agent(Request::AddSheet {
            name: "Totals".into()
        }),
        Reply::Confirm
    );
    assert!(matches!(c.agent_answer(true), Reply::Done(_)));
    assert_eq!(c.book.wb.sheets[1].name, "Totals");
    assert!(matches!(
        c.agent(Request::AddSheet {
            name: "Totals".into()
        }),
        Reply::Refused(_)
    ));
    // open is handed to the window
    let uri = "files://documents/a.xlsx".to_owned();
    assert_eq!(
        c.agent(Request::Open { uri: uri.clone() }),
        Reply::Open(uri)
    );
}

#[test]
fn a_big_paste_is_cut_at_the_limits_and_says_so() {
    let mut c = core();
    c.sel.select(1_048_574, 16_383);
    c.paste("a\tb\nc\td\n");
    assert_eq!(c.book.sheet().get(1_048_574, 16_383), "a");
    assert_eq!(c.book.sheet().get(1_048_575, 16_383), "c");
    assert_eq!(c.book.sheet().cells.len(), 2);
}
