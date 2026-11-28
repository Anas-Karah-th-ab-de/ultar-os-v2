//! The editing operations of the spreadsheet model: references that move, rows and columns,
//! sheets, fill, sort, tab-separated text, find and replace.

use ultar_office::sheet::{Value, Workbook};
use ultar_office::sheet_edit::{parse_tsv, NameError};
use ultar_office::sheet_find::{replace_text, text_matches, FindOpts};
use ultar_office::sheet_refs::{delete_refs, insert_refs, move_refs, relocate, Axis, MAX_ROWS};
use ultar_office::style::CellStyle;

fn wb(cells: &[(&str, &str)]) -> Workbook {
    let mut w = Workbook::new();
    for (c, v) in cells {
        w.set_ref(0, c, v).unwrap_or_default();
    }
    w
}

#[test]
fn relative_references_move_and_absolute_ones_stay() {
    assert_eq!(move_refs("A1+B2", 1, 0), "A2+B3");
    assert_eq!(move_refs("$A$1+B$2+$C3", 2, 1), "$A$1+C$2+$C5");
    assert_eq!(move_refs("SUM(A1:B2)", 0, 1), "SUM(B1:C2)");
    assert_eq!(move_refs("Sheet2!A1*Daten!$B2", 1, 1), "Sheet2!B2*Daten!$B3");
    assert_eq!(move_refs("'My Sheet'!A1", 1, 0), "'My Sheet'!A2");
    // text, functions that look like cells, and leaving the sheet
    assert_eq!(move_refs("\"A1\"&A1", 1, 0), "\"A1\"&A2");
    assert_eq!(move_refs("LOG10(A1)", 1, 0), "LOG10(A2)");
    assert_eq!(move_refs("A1", -1, 0), "#REF!");
    assert_eq!(move_refs("Z9", 0, -26), "#REF!");
    assert_eq!(relocate("=A1+1", 1, 0), "=A2+1");
    assert_eq!(relocate("A1", 1, 0), "A1");
}

#[test]
fn inserting_a_row_moves_the_references_below() {
    let f = |s: &str| insert_refs(s, "Sheet1", "Sheet1", Axis::Rows, 2, 1);
    assert_eq!(f("A1+A3+$B$5"), "A1+A4+$B$6");
    assert_eq!(f("SUM(A1:A5)"), "SUM(A1:A6)");
    assert_eq!(f("SUM(A3:A5)"), "SUM(A4:A6)");
    // another sheet is not touched, a named reference to this sheet is
    assert_eq!(insert_refs("A3+Sheet1!A3", "Daten", "Sheet1", Axis::Rows, 0, 2), "A3+Sheet1!A5");
    assert_eq!(insert_refs("B1", "S", "S", Axis::Cols, 0, 2), "D1");
}

#[test]
fn deleting_rows_gives_ref_errors_and_shrinks_ranges() {
    let f = |s: &str| delete_refs(s, "Sheet1", "Sheet1", Axis::Rows, 1, 1);
    assert_eq!(f("A1+A2+A3"), "A1+#REF!+A2");
    assert_eq!(f("SUM(A1:A5)"), "SUM(A1:A4)");
    assert_eq!(f("SUM(A2:A5)"), "SUM(A2:A4)");
    assert_eq!(f("SUM(A2:A2)"), "SUM(#REF!)");
    assert_eq!(f("SUM(A1:A2)"), "SUM(A1:A1)");
    assert_eq!(delete_refs("A1+B1", "S", "S", Axis::Cols, 0, 1), "#REF!+A1");
}

#[test]
fn rows_are_inserted_and_deleted_in_the_workbook() {
    let mut w = wb(&[("A1", "1"), ("A2", "2"), ("A3", "3"), ("B1", "=SUM(A1:A3)"), ("B2", "=A3*2")]);
    w.sheets[0].heights.insert(2, 40.0);
    assert!(w.insert_lines(0, Axis::Rows, 1, 2));
    assert_eq!(w.sheets[0].get(3, 0), "2");
    assert_eq!(w.sheets[0].get(0, 1), "=SUM(A1:A5)");
    assert_eq!(w.sheets[0].get(3, 1), "=A5*2");
    assert_eq!(w.value(0, 0, 1), Value::Num(6.0));
    assert_eq!(w.sheets[0].heights.get(&4), Some(&40.0));
    assert!(w.delete_lines(0, Axis::Rows, 1, 2));
    assert_eq!(w.sheets[0].get(0, 1), "=SUM(A1:A3)");
    assert!(w.delete_lines(0, Axis::Rows, 2, 1));
    assert_eq!(w.value(0, 1, 1), Value::Err("#REF!".into()));
    assert!(!w.insert_lines(0, Axis::Rows, 0, MAX_ROWS));
}

#[test]
fn columns_and_other_sheets_follow() {
    let mut w = wb(&[("A1", "5"), ("B1", "=A1+1")]);
    w.set_ref(0, "Zwei!A1", "=Sheet1!B1*2").unwrap_or_default();
    assert!(w.insert_lines(0, Axis::Cols, 0, 1));
    assert_eq!(w.sheets[0].get(0, 2), "=B1+1");
    assert_eq!(w.sheets[1].get(0, 0), "=Sheet1!C1*2");
    assert_eq!(w.value(1, 0, 0), Value::Num(12.0));
}

#[test]
fn renaming_and_deleting_sheets() {
    let mut w = wb(&[("A1", "=Zwei!A1+1")]);
    w.set_ref(0, "Zwei!A1", "4").unwrap_or_default();
    assert_eq!(w.rename_sheet(1, "Zwei"), Ok(()));
    assert_eq!(w.rename_sheet(1, "Mein Blatt"), Ok(()));
    assert_eq!(w.sheets[0].get(0, 0), "='Mein Blatt'!A1+1");
    assert_eq!(w.value(0, 0, 0), Value::Num(5.0));
    assert_eq!(w.rename_sheet(1, "sheet1"), Err(NameError::Taken));
    assert_eq!(w.rename_sheet(1, "a/b"), Err(NameError::Bad));
    assert_eq!(w.rename_sheet(1, ""), Err(NameError::Bad));
    assert_eq!(w.delete_sheet(1), Ok(()));
    assert_eq!(w.sheets[0].get(0, 0), "=#REF!+1");
    assert_eq!(w.value(0, 0, 0), Value::Err("#REF!".into()));
    assert_eq!(w.delete_sheet(0), Err(NameError::Last));
    assert_eq!(w.new_sheet_name("Sheet"), "Sheet2");
}

#[test]
fn fill_copies_with_moving_references() {
    let mut w = wb(&[("A1", "1"), ("B1", "=A1*2"), ("A2", "x"), ("B2", "y")]);
    w.sheets[0].set_style(0, 1, CellStyle { bold: true, ..CellStyle::default() });
    w.fill_down(0, 0, 0, 3, 1);
    assert_eq!(w.sheets[0].get(3, 0), "1");
    assert_eq!(w.sheets[0].get(3, 1), "=A4*2");
    assert!(w.sheets[0].style(2, 1).bold);
    w.fill_right(0, 0, 1, 0, 3);
    assert_eq!(w.sheets[0].get(0, 3), "=C1*2");
}

#[test]
fn sort_orders_numbers_text_and_keeps_empty_last() {
    let mut w = wb(&[
        ("A1", "b"), ("B1", "2"),
        ("A2", "a"), ("B2", "30"),
        ("A3", "c"), ("B3", "=B1+B2"),
        ("A4", "e"),
        ("A5", "d"), ("B5", "1"),
    ]);
    let cmp = |a: &str, b: &str| a.to_lowercase().cmp(&b.to_lowercase());
    w.sort_rows(0, (0, 0, 4, 1), 1, true, &cmp).unwrap_or_default();
    let col = |c: u32| (0..5).map(|r| w.sheets[0].get(r, c).to_owned()).collect::<Vec<_>>();
    assert_eq!(col(0), ["d", "b", "a", "c", "e"]);
    // the formula went with its row and now reads the row it sits in
    assert_eq!(w.sheets[0].get(3, 1), "=B2+B3");
    w.sort_rows(0, (0, 0, 4, 1), 0, false, &cmp).unwrap_or_default();
    assert_eq!((0..5).map(|r| w.sheets[0].get(r, 0).to_owned()).collect::<Vec<_>>(), ["e", "d", "c", "b", "a"]);
    assert!(w.sort_rows(0, (0, 0, 4, 1), 5, true, &cmp).is_err());
}

#[test]
fn tab_separated_text_round_trips() {
    let w = wb(&[("A1", "a"), ("B1", "b\tc"), ("A2", "say \"hi\""), ("B2", "=1+1")]);
    let t = w.to_tsv(0, (0, 0, 1, 1), false);
    assert_eq!(t, "a\t\"b\tc\"\n\"say \"\"hi\"\"\"\t2\n");
    assert_eq!(
        parse_tsv(&t),
        vec![vec!["a", "b\tc"], vec!["say \"hi\"", "2"]]
    );
    assert_eq!(w.to_tsv(0, (1, 1, 1, 1), true), "=1+1\n");
    assert_eq!(parse_tsv("1\t2\r\n3\t4"), vec![vec!["1", "2"], vec!["3", "4"]]);
    assert_eq!(parse_tsv("x"), vec![vec!["x"]]);
    assert_eq!(parse_tsv("a\t\n"), vec![vec!["a", ""]]);
    assert!(parse_tsv("").is_empty());
}

#[test]
fn find_and_replace() {
    let w = wb(&[("A1", "Hello World"), ("B2", "=CONCAT(\"hel\",\"lo\")"), ("C3", "x")]);
    let o = FindOpts::default();
    assert_eq!(w.find_all(None, "hello", &o), vec![(0, 0, 0)]);
    let v = FindOpts { in_values: true, ..o };
    assert_eq!(w.find_all(None, "hello", &v), vec![(0, 0, 0), (0, 1, 1)]);
    let c = FindOpts { match_case: true, ..o };
    assert!(w.find_all(None, "hello", &c).is_empty());
    let whole = FindOpts { whole_cell: true, ..o };
    assert_eq!(w.find_all(None, "X", &whole), vec![(0, 2, 2)]);
    assert_eq!(replace_text("Hello World", "o", "0", &c).as_deref(), Some("Hell0 W0rld"));
    assert_eq!(replace_text("HELLO", "l", "L", &o).as_deref(), Some("HELLO".replace('L', "L").as_str()));
    assert_eq!(replace_text("x", "y", "z", &o), None);
    assert_eq!(replace_text("X", "x", "yy", &whole).as_deref(), Some("yy"));
    assert_eq!(replace_text("a", "", "z", &o), None);
}

#[test]
fn arabic_search_uses_the_normaliser() {
    let o = FindOpts {
        normalize: Some(|s| s.replace('أ', "ا")),
        ..FindOpts::default()
    };
    assert!(text_matches("أحمد", "احمد", &o));
    assert!(!text_matches("أحمد", "احمد", &FindOpts::default()));
}
