//! Round 4 of the UI review (`docs/review/ui-runde-4.md`): middle click, multi-selection,
//! drop targets, changing table rows / tree items / select options, removing and
//! reordering nodes (W3), unique accessibility identities in every gallery state (W5), the
//! show-password button by keyboard (W6).

mod common;

use common::gallery::{file_menu, gallery};
use common::{center, click, key, press, release};
use std::collections::HashSet;
use ultar_ui::event::{ActionKind, Event, Key, Modifiers, PointerButton, Response};
use ultar_ui::tokens::{size, ThemeKind};
use ultar_ui::ui::{NodeId, Role, Ui, UiError};
use ultar_ui::widgets::{
    Button, Label, List, ListItem, Select, Table, TextField, Toast, Tone, Tree, TreeItem, Widget,
};
use ultar_ui::{Lang, Point, Size};

fn ui(lang: Lang) -> Ui {
    common::ui(lang, ThemeKind::Dark, Size::new(640.0, 480.0))
}

fn kinds(r: &Response) -> Vec<ActionKind> {
    r.actions.iter().map(|a| a.kind).collect()
}

fn with(ui: &mut Ui, modifiers: Modifiers, id: NodeId) -> Response {
    ui.handle(Event::Modifiers { modifiers });
    let response = click(ui, id);
    ui.handle(Event::Modifiers {
        modifiers: Modifiers::NONE,
    });
    response
}

const CTRL: Modifiers = Modifiers {
    ctrl: true,
    ..Modifiers::NONE
};
const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..Modifiers::NONE
};

#[test]
fn a_middle_click_is_reported_on_release_over_the_same_node() {
    let mut ui = ui(Lang::Ar);
    let root = ui.root();
    let a = ui
        .add(root, Button::new(Label::key("demo.save")))
        .expect("a");
    let b = ui
        .add(root, Button::new(Label::key("demo.close")))
        .expect("b");
    let (pa, pb) = (center(&mut ui, a), center(&mut ui, b));
    let button = PointerButton::Middle;
    ui.handle(Event::PointerDown { pos: pa, button });
    let up = ui.handle(Event::PointerUp { pos: pa, button });
    assert_eq!(up.actions.len(), 1);
    assert_eq!(
        (up.actions[0].node, up.actions[0].kind),
        (a, ActionKind::MiddleClicked)
    );
    // Released over another node: nothing.
    ui.handle(Event::PointerDown { pos: pa, button });
    assert!(ui
        .handle(Event::PointerUp { pos: pb, button })
        .actions
        .is_empty());
    // A middle click never activates.
    assert!(!kinds(&up).contains(&ActionKind::Activated));
}

fn files(ui: &mut Ui, multi: bool) -> NodeId {
    let root = ui.root();
    let rows = (0..6)
        .map(|i| vec![Label::data(&format!("file {i}"))])
        .collect();
    let table = Table::new(Label::data("Files"), vec![Label::data("Name")]).rows(rows);
    let table = if multi { table.multi() } else { table };
    ui.add(root, table).expect("table")
}

fn row_pos(ui: &mut Ui, table: NodeId, row: usize) -> Point {
    ui.a11y_tree()
        .into_iter()
        .find(|n| {
            n.id == table && n.role == Role::TableRow && n.position.map(|p| p.0) == Some(row + 1)
        })
        .expect("row")
        .rect
        .center()
}

fn click_at(ui: &mut Ui, pos: Point, modifiers: Modifiers) -> Response {
    ui.handle(Event::Modifiers { modifiers });
    ui.handle(Event::PointerMove { pos });
    let mut r = press(ui, pos);
    r.actions.extend(release(ui, pos).actions);
    ui.handle(Event::Modifiers {
        modifiers: Modifiers::NONE,
    });
    ui.advance_to(ui_clock_step());
    r
}

/// Clicks are spaced beyond the double-click time.
fn ui_clock_step() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NOW: AtomicU64 = AtomicU64::new(0);
    NOW.fetch_add(1000, Ordering::Relaxed) + 1000
}

#[test]
fn a_multi_selection_table_marks_by_ctrl_and_shift_and_announces_every_marked_row() {
    let mut ui = ui(Lang::En);
    let table = files(&mut ui, true);
    let p1 = row_pos(&mut ui, table, 1);
    assert_eq!(
        kinds(&click_at(&mut ui, p1, Modifiers::NONE)),
        vec![ActionKind::Selected(1)]
    );
    assert_eq!(ui.selected_rows(table).expect("rows"), vec![1]);
    let p4 = row_pos(&mut ui, table, 4);
    assert_eq!(
        kinds(&click_at(&mut ui, p4, SHIFT)),
        vec![ActionKind::SelectionChanged]
    );
    assert_eq!(ui.selected_rows(table).expect("rows"), vec![1, 2, 3, 4]);
    let p2 = row_pos(&mut ui, table, 2);
    click_at(&mut ui, p2, CTRL);
    assert_eq!(ui.selected_rows(table).expect("rows"), vec![1, 3, 4]);
    let marked: Vec<_> = ui
        .a11y_tree()
        .into_iter()
        .filter(|n| n.id == table && n.role == Role::TableRow && n.state.selected == Some(true))
        .map(|n| n.position.map(|p| p.0 - 1))
        .collect();
    assert_eq!(marked, vec![Some(1), Some(3), Some(4)]);
    // Keyboard: Shift+Down extends from the anchor (row 2), Ctrl+Space toggles.
    common::key(&mut ui, Key::Tab);
    let shift_down = ui.handle(Event::Key {
        key: Key::ArrowDown,
        modifiers: SHIFT,
    });
    assert_eq!(kinds(&shift_down), vec![ActionKind::SelectionChanged]);
    assert_eq!(ui.selected_rows(table).expect("rows"), vec![2, 3]);
    ui.handle(Event::Key {
        key: Key::Space,
        modifiers: CTRL,
    });
    assert_eq!(ui.selected_rows(table).expect("rows"), vec![2]);
    // A plain click keeps one row; a single-selection table ignores the modifiers.
    click_at(&mut ui, p4, Modifiers::NONE);
    assert_eq!(ui.selected_rows(table).expect("rows"), vec![4]);
    let mut single = self::ui(Lang::En);
    let plain = files(&mut single, false);
    let q1 = row_pos(&mut single, plain, 1);
    click_at(&mut single, q1, Modifiers::NONE);
    let q3 = row_pos(&mut single, plain, 3);
    click_at(&mut single, q3, SHIFT);
    assert_eq!(single.selected_rows(plain).expect("rows"), vec![3]);
}

#[test]
fn a_multi_selection_list_marks_by_ctrl_and_shift() {
    let mut ui = ui(Lang::Ar);
    let root = ui.root();
    let list = ui.add(root, List::new().multi()).expect("list");
    let items: Vec<NodeId> = (0..5)
        .map(|i| {
            ui.add(list, ListItem::new(Label::data(&format!("item {i}"))))
                .expect("item")
        })
        .collect();
    click(&mut ui, items[0]);
    ui.advance_to(1000);
    assert_eq!(
        kinds(&with(&mut ui, SHIFT, items[2])),
        vec![ActionKind::SelectionChanged]
    );
    assert_eq!(ui.selected_rows(list).expect("rows"), vec![0, 1, 2]);
    ui.advance_to(2000);
    with(&mut ui, CTRL, items[4]);
    assert_eq!(ui.selected_rows(list).expect("rows"), vec![0, 1, 2, 4]);
    assert!(
        ui.a11y_tree()
            .iter()
            .filter(|n| n.role == Role::ListItem)
            .filter(|n| n.state.selected == Some(true))
            .count()
            == 4
    );
    // Ctrl+Space on the focused item toggles it; Shift+Down extends from the anchor.
    ui.advance_to(3000);
    click(&mut ui, items[1]);
    assert_eq!(ui.selected_rows(list).expect("rows"), vec![1]);
    ui.handle(Event::Key {
        key: Key::ArrowDown,
        modifiers: SHIFT,
    });
    assert_eq!(ui.selected_rows(list).expect("rows"), vec![1, 2]);
    ui.handle(Event::Key {
        key: Key::Space,
        modifiers: CTRL,
    });
    assert_eq!(ui.selected_rows(list).expect("rows"), vec![1]);
}

#[test]
fn a_drag_reports_the_drop_target_under_the_pointer() {
    let mut ui = ui(Lang::En);
    let root = ui.root();
    let tile = ui
        .add(root, Button::new(Label::key("demo.save")))
        .expect("tile");
    let bin = ui
        .add(root, Button::new(Label::key("demo.close")))
        .expect("bin");
    ui.set_draggable(tile, true).expect("drag");
    ui.set_drop_target(bin, true).expect("drop");
    let (from, to) = (center(&mut ui, tile), center(&mut ui, bin));
    assert_eq!(ui.node_at(to), Some(bin));
    ui.handle(Event::PointerMove { pos: from });
    press(&mut ui, from);
    let step = Point::new(from.x, from.y + size::TOUCH_SLOP * 2.0);
    ui.handle(Event::PointerMove { pos: step });
    ui.handle(Event::PointerMove { pos: to });
    let end = release(&mut ui, to);
    let delta = Point::new(to.x - from.x, to.y - from.y);
    assert!(kinds(&end).contains(&ActionKind::DragEnded {
        delta,
        target: Some(bin)
    }));
    // Dropped beside every target: no target.
    ui.advance_to(5000);
    press(&mut ui, from);
    ui.handle(Event::PointerMove { pos: step });
    let away = Point::new(600.0, 470.0);
    ui.handle(Event::PointerMove { pos: away });
    let end = release(&mut ui, away);
    assert!(end
        .actions
        .iter()
        .any(|a| matches!(a.kind, ActionKind::DragEnded { target: None, .. })));
}

#[test]
fn keyed_rows_keep_the_selection_on_their_key() {
    let mut ui = ui(Lang::En);
    let table = files(&mut ui, false);
    common::key(&mut ui, Key::Tab);
    // With keys the selection follows the key even when the cells change.
    let keyed: Vec<Vec<Label>> = (0..3)
        .map(|i| vec![Label::data(&format!("k{i}"))])
        .collect();
    ui.set_table_rows_keyed(table, keyed, vec![10, 20, 30])
        .expect("keyed");
    common::key(&mut ui, Key::Home);
    let renamed: Vec<Vec<Label>> = (0..3)
        .map(|i| vec![Label::data(&format!("r{i}"))])
        .collect();
    ui.set_table_rows_keyed(table, renamed, vec![20, 30, 10])
        .expect("keyed");
    assert_eq!(
        ui.selected_rows(table).expect("rows"),
        vec![2],
        "key 10 moved to 2"
    );
    assert!(ui.set_table_rows_keyed(table, Vec::new(), vec![1]).is_err());
}

#[test]
fn table_rows_tree_items_and_select_options_change_in_place() {
    let mut ui = ui(Lang::En);
    let table = files(&mut ui, false);
    common::key(&mut ui, Key::Tab);
    common::key(&mut ui, Key::End);
    // "file 5" (selected) moves to place 2 among new rows: the selection follows it.
    let mut rows: Vec<Vec<Label>> = (0..9)
        .map(|i| vec![Label::data(&format!("new {i}"))])
        .collect();
    rows.insert(2, vec![Label::data("file 5")]);
    ui.set_table_rows(table, rows).expect("rows");
    assert_eq!(ui.focused(), Some(table), "focus stays");
    assert_eq!(
        ui.selected_rows(table).expect("rows"),
        vec![2],
        "selection follows its row"
    );
    ui.set_table_rows(table, vec![vec![Label::data("only")]])
        .expect("rows");
    assert!(
        ui.selected_rows(table).expect("rows").is_empty(),
        "row 5 is gone"
    );
    let root = ui.root();
    let items = vec![
        TreeItem::new(Label::data("a"), 0),
        TreeItem::new(Label::data("b"), 1),
    ];
    let tree = ui
        .add(root, Tree::new(Label::data("t"), items))
        .expect("tree");
    ui.set_tree_items(
        tree,
        vec![
            TreeItem::new(Label::data("x"), 0),
            TreeItem::new(Label::data("y"), 5),
        ],
    )
    .expect("items");
    match ui.widget(tree) {
        Some(Widget::Tree(t)) => assert_eq!(t.items[1].depth, 1, "normalised like Tree::new"),
        _ => panic!("tree"),
    }
    let options = ["a", "b", "c"].map(Label::data).to_vec();
    let select = ui
        .add(root, Select::new(Label::data("s"), options).selected(2))
        .expect("select");
    ui.set_select_options(select, ["x", "y"].map(Label::data).to_vec())
        .expect("options");
    assert_eq!(ui.selected(select).expect("selected"), 1);
    assert!(ui.set_select_options(select, Vec::new()).is_err());
    assert!(ui.set_table_rows(tree, Vec::new()).is_err());
}

#[test]
fn removing_a_focused_branch_moves_the_focus_on_and_forgets_the_nodes() {
    let mut ui = ui(Lang::Ar);
    let root = ui.root();
    let list = ui.add(root, List::new()).expect("list");
    let first = ui
        .add(list, ListItem::new(Label::data("first")))
        .expect("item");
    let after = ui
        .add(root, Button::new(Label::key("demo.save")))
        .expect("button");
    common::key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(first));
    ui.remove(list).expect("remove");
    assert!(ui.children(root).iter().all(|c| *c != list));
    assert!(matches!(
        ui.set_label(first, Label::data("x")),
        Err(UiError::UnknownNode(_))
    ));
    assert!(ui.a11y_tree().iter().all(|n| n.id != first && n.id != list));
    common::key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(after));
    assert!(ui.remove(root).is_err());
    // Reordering keeps the nodes and changes the reading order.
    let b = ui
        .add(root, Button::new(Label::key("demo.close")))
        .expect("b");
    ui.move_child(root, 1, 0).expect("move");
    assert_eq!(ui.children(root), &[b, after]);
    assert!(ui.move_child(root, 0, 5).is_err());
}

#[test]
fn the_show_password_button_is_a_tab_stop_and_space_toggles_it() {
    let mut ui = ui(Lang::En);
    let root = ui.root();
    let field = ui
        .add(
            root,
            TextField::password(Label::key("demo.password")).text("sesame"),
        )
        .expect("f");
    let next = ui
        .add(root, Button::new(Label::key("demo.save")))
        .expect("b");
    key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(field));
    key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(field), "the eye button is the next stop");
    let eye = |ui: &mut Ui| {
        ui.a11y_tree()
            .into_iter()
            .find(|n| n.id == field && n.part == Some(1))
            .expect("eye")
    };
    assert!(eye(&mut ui).state.focused);
    key(&mut ui, Key::Space);
    assert!(eye(&mut ui).state.pressed, "shown");
    key(&mut ui, Key::Space);
    assert!(!eye(&mut ui).state.pressed, "hidden again");
    key(&mut ui, Key::Space);
    key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(next));
    assert!(
        !eye(&mut ui).state.pressed,
        "leaving the field hides the password"
    );
    // Shift+Tab comes back to the eye button first.
    ui.handle(Event::Key {
        key: Key::Tab,
        modifiers: Modifiers::SHIFT,
    });
    assert!(eye(&mut ui).state.focused);
    assert!(!format!("{:?}", ui.widget(field)).contains("sesame"));
}

fn assert_unique(ui: &mut Ui, state: &str) {
    let mut seen = HashSet::new();
    for node in ui.a11y_tree() {
        assert!(
            seen.insert((node.id, node.part)),
            "{state}: ({:?}, {:?}) twice ({:?})",
            node.id,
            node.part,
            node.role
        );
    }
}

#[test]
fn accessibility_identities_are_unique_in_every_gallery_state() {
    for lang in [Lang::Ar, Lang::En] {
        let mut ui = ui(lang);
        let g = gallery(&mut ui).expect("gallery");
        assert_unique(&mut ui, "start");
        let stops = ui.focus_order().len();
        for step in 0..stops {
            key(&mut ui, Key::Tab);
            assert_unique(&mut ui, &format!("focus {step}"));
        }
        ui.open_menu(g.menu_button, file_menu()).expect("menu");
        for (n, k) in [Key::ArrowDown, Key::ArrowDown, Key::Enter]
            .into_iter()
            .enumerate()
        {
            key(&mut ui, k);
            assert_unique(&mut ui, &format!("menu {n}"));
        }
        key(&mut ui, Key::Escape);
        key(&mut ui, Key::Escape);
        let at = center(&mut ui, g.items[0]);
        ui.open_context_menu(g.items[0], at, file_menu())
            .expect("open");
        key(&mut ui, Key::ArrowDown);
        key(&mut ui, Key::ArrowDown);
        key(&mut ui, Key::Enter);
        assert_unique(&mut ui, "context submenu");
        key(&mut ui, Key::Escape);
        key(&mut ui, Key::Escape);
        for id in 1..=3 {
            let toast = Toast::new(id, Tone::Info, Label::key("demo.toast.saved"));
            ui.show_toast(toast.action(Label::key("demo.toast.undo")))
                .expect("toast");
        }
        assert_unique(&mut ui, "toasts");
        for dialog in [g.modal.dialog, g.modal.sheet] {
            ui.open_dialog(dialog).expect("dialog");
            assert_unique(&mut ui, "dialog");
            ui.close_dialog(dialog).expect("close");
        }
    }
}
