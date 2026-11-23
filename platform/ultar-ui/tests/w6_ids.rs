//! Review round 6 (second reviewer): node handles with a generation (wave 6), keyed table
//! rows and the lazy accessibility rows of long tables.
//! Target path: platform/ultar-ui/tests/w6_ids.rs

mod common;

use common::key;
use ultar_ui::event::Key;
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{Role, Ui, UiError};
use ultar_ui::widgets::{Label, List, ListItem, Scroll, Table};
use ultar_ui::{Lang, Size};

fn ui() -> Ui {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(480.0, 400.0));
    ui.set_root_flex(Flex::column());
    ui
}

/// The generation is 16 bits and the most recently freed slot is reused first, so one
/// row replaced 65 536 times brings an old handle back to life: the app's stale handle
/// (kept in a map, captured by a closure) now changes an unrelated node without an error.
#[test]
fn id01_a_stale_handle_never_reaches_a_new_node() {
    let mut ui = ui();
    let root = ui.root();
    let list = ui.add(root, List::new()).unwrap();
    let old = ui.add(list, ListItem::new(Label::data("old"))).unwrap();
    ui.remove(old).unwrap();
    let mut current = ui.add(list, ListItem::new(Label::data("row"))).unwrap();
    for _ in 0..65_536u32 {
        ui.remove(current).unwrap();
        current = ui.add(list, ListItem::new(Label::data("row"))).unwrap();
        if current == old {
            break;
        }
    }
    assert!(
        matches!(
            ui.set_label(old, Label::data("x")),
            Err(UiError::UnknownNode(_))
        ),
        "the handle removed at the start addresses the live node {current:?} again"
    );
}

fn table(ui: &mut Ui, rows: usize) -> ultar_ui::ui::NodeId {
    let root = ui.root();
    let scroll = ui.add_sized(root, Scroll::new(), Sizing::grow(1)).unwrap();
    let data = (0..rows)
        .map(|i| vec![Label::data(&format!("file {i}"))])
        .collect();
    let t = Table::new(Label::data("Files"), vec![Label::data("Name")])
        .rows(data)
        .multi();
    ui.add(scroll, t).unwrap()
}

/// Keys are not checked for duplicates: two marked rows with the same key both follow it,
/// and the selection holds the same row twice.
#[test]
fn id02_duplicate_keys_do_not_duplicate_the_selection() {
    let mut ui = ui();
    let t = table(&mut ui, 2);
    let rows = |a: &str, b: &str| vec![vec![Label::data(a)], vec![Label::data(b)]];
    let keyed = ui.set_table_rows_keyed(t, rows("a", "b"), vec![5, 5]);
    if keyed.is_err() {
        return; // refusing duplicate keys is a correct answer too
    }
    ui.set_selected_rows(t, &[0, 1]).unwrap();
    ui.set_table_rows_keyed(t, rows("b", "a"), vec![5, 7])
        .unwrap();
    let marked = ui.selected_rows(t).unwrap();
    let mut unique = marked.clone();
    unique.dedup();
    assert_eq!(marked, unique, "selection holds a row twice: {marked:?}");
}

/// A screen-reader user reaches far rows by keyboard: the selected row always has a node,
/// with its position among all rows.
#[test]
fn id03_row_9000_of_10000_is_reachable_and_announced_by_keyboard() {
    let mut ui = ui();
    let t = table(&mut ui, 10_000);
    while ui.focused() != Some(t) {
        key(&mut ui, Key::Tab);
    }
    key(&mut ui, Key::End);
    for _ in 0..999 {
        key(&mut ui, Key::ArrowUp);
    }
    let tree = ui.a11y_tree();
    let row = tree
        .iter()
        .find(|n| n.id == t && n.role == Role::TableRow && n.state.focused)
        .expect("a focused row node");
    assert_eq!(row.position, Some((9001, 10_000)));
    let rows = tree.iter().filter(|n| n.role == Role::TableRow).count();
    assert!(rows < 200, "{rows} row nodes for a 10 000-row table");
}
