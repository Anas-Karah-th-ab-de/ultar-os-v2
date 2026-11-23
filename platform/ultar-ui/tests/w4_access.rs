//! Review round 4 (second reviewer): accessibility tree of the wave-4 widgets.
//! Target path: platform/ultar-ui/tests/w4_access.rs

mod common;

use common::gallery::file_menu;
use common::key;
use std::collections::HashSet;
use ultar_ui::event::{Event, Key, PointerButton};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{Role, Ui};
use ultar_ui::widgets::{Button, Label, Table, Tree, TreeItem, Widget};
use ultar_ui::{Lang, Size};

fn ui(lang: Lang) -> Ui {
    common::ui(lang, ThemeKind::Dark, Size::new(480.0, 400.0))
}

/// A screen-reader bridge keys its objects by (node, part). The commands of a submenu use
/// the same parts (their index) as the commands of the menu above, so two different
/// commands get the same identity while a submenu is open.
#[test]
fn ax01_every_accessibility_node_has_its_own_identity() {
    for lang in [Lang::Ar, Lang::En] {
        let mut ui = ui(lang);
        let root = ui.root();
        let button = ui
            .add(root, Button::new(Label::key("demo.menu.button")))
            .unwrap();
        key(&mut ui, Key::Tab);
        ui.open_menu(button, file_menu()).unwrap();
        key(&mut ui, Key::ArrowDown);
        key(&mut ui, Key::ArrowDown);
        key(&mut ui, Key::Enter);
        assert_eq!(ui.menu_position().unwrap().0, &[2][..], "submenu open");
        let mut seen = HashSet::new();
        let mut twice = Vec::new();
        for node in ui.a11y_tree() {
            if !seen.insert((node.id, node.part, node.role == Role::Menu)) {
                twice.push((node.id, node.part, node.role, node.name));
            }
        }
        assert!(
            twice.is_empty(),
            "{lang:?}: identities used twice: {twice:?}"
        );
    }
}

/// A row of a table is announced with its first cell as name and its second as value; the
/// third and every further column are never spoken.
#[test]
fn ax02_every_cell_of_a_table_row_is_announced() {
    let mut ui = ui(Lang::En);
    let root = ui.root();
    let columns = ["Name", "Size", "Kind", "Modified"]
        .map(Label::data)
        .to_vec();
    let row = ["report.odt", "24 KB", "Document", "Yesterday"]
        .map(Label::data)
        .to_vec();
    let table = Table::new(Label::data("Files"), columns).rows(vec![row]);
    let id = ui.add(root, table).unwrap();
    let tr = ui.translator().clone();
    let spoken: Vec<String> = ui
        .a11y_tree()
        .iter()
        .filter(|n| n.id == id && n.role == Role::TableRow)
        .map(|n| n.describe(&tr))
        .collect();
    assert_eq!(spoken.len(), 1);
    for cell in ["report.odt", "24 KB", "Document", "Yesterday"] {
        assert!(
            spoken[0].contains(cell),
            "{cell:?} missing in {:?}",
            spoken[0]
        );
    }
}

fn folders() -> Tree {
    let items = vec![
        TreeItem::new(Label::data("Home"), 0).expanded(),
        TreeItem::new(Label::data("Documents"), 1).expanded(),
        TreeItem::new(Label::data("Work"), 2),
        TreeItem::new(Label::data("Pictures"), 1),
    ];
    Tree::new(Label::data("Folders"), items)
}

fn tree_selected(ui: &Ui, id: ultar_ui::ui::NodeId) -> Option<usize> {
    match ui.widget(id) {
        Some(Widget::Tree(t)) => t.selected,
        _ => None,
    }
}

/// Collapsing (by its toggle) a folder whose child is selected hides the selected item. The
/// keyboard then has no current row: Up does nothing and the tree announces no selected
/// item. The selection (or at least the keyboard position) must move to the collapsed
/// folder, as in every desktop tree.
#[test]
fn ax03_collapsing_a_parent_moves_the_selection_out_of_the_hidden_branch() {
    for lang in [Lang::Ar, Lang::En] {
        let mut ui = ui(lang);
        let root = ui.root();
        let id = ui.add(root, folders()).unwrap();
        key(&mut ui, Key::Tab);
        for _ in 0..3 {
            key(&mut ui, Key::ArrowDown);
        }
        assert_eq!(tree_selected(&ui, id), Some(2), "Work selected");
        // Collapse "Documents" (item 1) with the pointer, on its toggle.
        let toggle = ui
            .a11y_tree()
            .into_iter()
            .find(|n| n.id == id && n.part == Some(1))
            .unwrap()
            .rect;
        let x = if lang == Lang::Ar {
            toggle.right() - 16.0 - 16.0
        } else {
            toggle.x + 16.0 + 16.0
        };
        let pos = ultar_ui::Point::new(x, toggle.y + toggle.h / 2.0);
        let button = PointerButton::Primary;
        ui.handle(Event::PointerDown { pos, button });
        ui.handle(Event::PointerUp { pos, button });
        let visible: Vec<_> = ui
            .a11y_tree()
            .into_iter()
            .filter(|n| n.id == id && n.role == Role::TreeItem)
            .collect();
        assert_eq!(visible.len(), 3, "{lang:?}: Documents collapsed");
        assert!(
            visible.iter().any(|n| n.state.selected == Some(true)),
            "{lang:?}: no visible item is selected (selection {:?} hidden)",
            tree_selected(&ui, id)
        );
    }
}
