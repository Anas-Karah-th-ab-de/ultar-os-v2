//! Wave 4: the input primitives and widgets the click list needs (review round 3, M7):
//! double click, modifiers on pointer events, press-and-hold, dragging, long press, password
//! field, anchored panel, table, tree and toast – behaviour, keyboard and accessibility.

mod common;

use common::{center, click, key, press, release};
use ultar_ui::event::{Action, ActionKind, Event, Key, Modifiers, Response};
use ultar_ui::icon::IconId;
use ultar_ui::tokens::{motion, size, ThemeKind};
use ultar_ui::ui::{NodeId, Role, Ui};
use ultar_ui::widgets::{
    Button, Dialog, IconButton, Label, Menu, MenuItem, Switch, Table, TextField, Toast, Tone, Tree,
    TreeItem, MAX_TOASTS,
};
use ultar_ui::{Lang, Point, Size};

fn k(key: &str) -> Label {
    Label::key(key)
}

fn ui(lang: Lang) -> Ui {
    common::ui(lang, ThemeKind::Dark, Size::new(640.0, 480.0))
}

fn kinds(actions: &[Action]) -> Vec<ActionKind> {
    actions.iter().map(|a| a.kind).collect()
}

fn of(response: &Response) -> Vec<ActionKind> {
    kinds(&response.actions)
}

fn move_to(ui: &mut Ui, pos: Point) -> Response {
    ui.handle(Event::PointerMove { pos })
}

fn focus(ui: &mut Ui, id: NodeId) {
    for _ in 0..16 {
        if ui.focused() == Some(id) {
            return;
        }
        key(ui, Key::Tab);
    }
    panic!("node never got the focus");
}

#[test]
fn double_click_needs_the_same_node_within_the_window() {
    let mut ui = ui(Lang::En);
    let root = ui.root();
    let a = ui.add(root, Button::new(k("demo.save"))).expect("a");
    let b = ui.add(root, Button::new(k("demo.close"))).expect("b");
    assert_eq!(of(&click(&mut ui, a)), vec![ActionKind::Activated]);
    assert_eq!(
        of(&click(&mut ui, a)),
        vec![ActionKind::Activated, ActionKind::DoubleActivated]
    );
    // A third click starts a new pair.
    assert_eq!(of(&click(&mut ui, a)), vec![ActionKind::Activated]);
    // Too late: no double click.
    ui.advance_to(u64::from(motion::DOUBLE_CLICK) + 1);
    assert_eq!(of(&click(&mut ui, a)), vec![ActionKind::Activated]);
    // Another node in between: no double click either.
    ui.advance_to(10_000);
    click(&mut ui, a);
    assert_eq!(of(&click(&mut ui, b)), vec![ActionKind::Activated]);
}

#[test]
fn the_double_click_interval_is_a_setting_of_the_window() {
    let mut ui = ui(Lang::En);
    let root = ui.root();
    let a = ui.add(root, Button::new(k("demo.save"))).expect("a");
    assert_eq!(ui.double_click_ms(), motion::DOUBLE_CLICK as u16);
    // A longer interval: clicks 500 ms apart are still a double click.
    ui.set_double_click_ms(700);
    assert_eq!(of(&click(&mut ui, a)), vec![ActionKind::Activated]);
    ui.advance_to(500);
    assert_eq!(
        of(&click(&mut ui, a)),
        vec![ActionKind::Activated, ActionKind::DoubleActivated]
    );
    // A shorter one: 300 ms apart is too slow now.
    ui.set_double_click_ms(200);
    ui.advance_to(10_000);
    click(&mut ui, a);
    ui.advance_to(10_300);
    assert_eq!(of(&click(&mut ui, a)), vec![ActionKind::Activated]);
    // Out of range falls back to the default; a valid value survives a rebuild.
    for bad in [0, 199, 1001, u16::MAX] {
        ui.set_double_click_ms(bad);
        assert_eq!(ui.double_click_ms(), motion::DOUBLE_CLICK as u16, "{bad}");
    }
    ui.set_double_click_ms(600);
    ui.clear();
    assert_eq!(ui.double_click_ms(), 600, "clear keeps the setting");
}

#[test]
fn pointer_responses_carry_the_held_modifiers() {
    let mut ui = ui(Lang::En);
    let root = ui.root();
    let a = ui.add(root, Button::new(k("demo.save"))).expect("a");
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    assert!(!ui.handle(Event::Modifiers { modifiers: ctrl }).consumed);
    let response = click(&mut ui, a);
    assert_eq!(of(&response), vec![ActionKind::Activated]);
    assert!(response.modifiers.ctrl && !response.modifiers.shift);
    ui.handle(Event::Modifiers {
        modifiers: Modifiers::NONE,
    });
    ui.advance_to(10_000);
    assert!(!click(&mut ui, a).modifiers.ctrl);
}

#[test]
fn a_held_button_reports_press_repeats_and_release_instead_of_activation() {
    let mut ui = ui(Lang::Ar);
    let root = ui.root();
    let back = ui
        .add(root, Button::new(k("demo.back")).hold())
        .expect("button");
    let pos = center(&mut ui, back);
    move_to(&mut ui, pos);
    assert_eq!(of(&press(&mut ui, pos)), vec![ActionKind::Pressed]);
    let first = u64::from(motion::LONG_PRESS);
    assert_eq!(ui.next_wake(), Some(first));
    ui.advance_to(first - 1);
    assert!(ui.take_actions().is_empty());
    ui.advance_to(first);
    assert_eq!(kinds(&ui.take_actions()), vec![ActionKind::Repeated]);
    ui.advance_to(first + 3 * u64::from(motion::REPEAT));
    assert_eq!(kinds(&ui.take_actions()), vec![ActionKind::Repeated; 3]);
    // A stalled clock does not burst: at most eight repeats per step.
    ui.advance_to(first + 1000 * u64::from(motion::REPEAT));
    assert_eq!(ui.take_actions().len(), 8);
    assert_eq!(of(&release(&mut ui, pos)), vec![ActionKind::Released]);
    assert_eq!(ui.next_wake(), None);
}

#[test]
fn a_draggable_node_reports_the_drag_and_is_not_activated() {
    let mut ui = ui(Lang::En);
    let root = ui.root();
    let tile = ui.add(root, Button::new(k("demo.save"))).expect("tile");
    ui.set_draggable(tile, true).expect("draggable");
    let start = center(&mut ui, tile);
    move_to(&mut ui, start);
    assert!(of(&press(&mut ui, start)).is_empty());
    // Within the slop nothing happens.
    let near = Point::new(start.x + size::TOUCH_SLOP / 2.0, start.y);
    assert!(of(&move_to(&mut ui, near)).is_empty());
    let far = Point::new(start.x + 40.0, start.y + 10.0);
    assert_eq!(
        of(&move_to(&mut ui, far)),
        vec![
            ActionKind::DragStarted,
            ActionKind::Dragged(Point::new(40.0, 10.0))
        ]
    );
    assert_eq!(
        of(&release(&mut ui, far)),
        vec![ActionKind::DragEnded {
            delta: Point::new(40.0, 10.0),
            target: None
        }]
    );
    // Without `set_draggable` the same gesture is an ordinary press that left its node.
    ui.set_draggable(tile, false).expect("not draggable");
    ui.advance_to(10_000);
    press(&mut ui, start);
    assert!(of(&move_to(&mut ui, far)).is_empty());
    assert!(!of(&release(&mut ui, far))
        .iter()
        .any(|k| matches!(k, ActionKind::DragEnded { .. })));
}

#[test]
fn a_long_press_opens_the_context_menu_by_touch() {
    let mut ui = ui(Lang::Ar);
    let root = ui.root();
    let item = ui.add(root, Button::new(k("demo.save"))).expect("item");
    let menu = Menu::new(vec![MenuItem::new(1, k("demo.close"))]);
    ui.set_context_menu(item, Some(menu)).expect("menu");
    let pos = center(&mut ui, item);
    ui.handle(Event::TouchDown { id: 7, pos });
    let due = u64::from(motion::LONG_PRESS);
    assert_eq!(ui.next_wake(), Some(due));
    ui.advance_to(due);
    assert_eq!(ui.menu_owner(), Some(item));
    assert!(kinds(&ui.take_actions()).contains(&ActionKind::Opened));
    // Lifting the finger neither activates the item nor closes the menu.
    let up = ui.handle(Event::TouchUp { id: 7, pos });
    assert!(!of(&up).contains(&ActionKind::Activated));
    assert_eq!(ui.menu_owner(), Some(item));
    // A finger that moves first is a scroll or drag, not a long press.
    key(&mut ui, Key::Escape);
    ui.handle(Event::TouchDown { id: 8, pos });
    ui.handle(Event::TouchMove {
        id: 8,
        pos: Point::new(pos.x, pos.y + 30.0),
    });
    ui.advance_to(2 * due + 10);
    assert_eq!(ui.menu_owner(), None);
}

#[test]
fn a_password_is_masked_never_spoken_and_shown_only_while_held() {
    let mut ui = ui(Lang::En);
    let root = ui.root();
    let field = ui
        .add(
            root,
            TextField::password(k("demo.password")).text("sesame 42"),
        )
        .expect("field");
    assert_eq!(ui.text(field).expect("text"), "sesame 42");
    let tree = ui.a11y_tree();
    let node = tree
        .iter()
        .find(|n| n.id == field && n.part.is_none())
        .expect("node");
    assert_eq!(node.role, Role::PasswordField);
    assert!(node.value.is_none(), "a password value is never announced");
    let show = tree
        .iter()
        .find(|n| n.id == field && n.part == Some(1))
        .expect("button");
    assert_eq!(show.name, Some(k("ui.password.show")));
    assert!(!show.state.pressed);
    // Held: shown; released: masked again.
    let rect = show.rect;
    let pos = rect.center();
    move_to(&mut ui, pos);
    press(&mut ui, pos);
    let held = ui.a11y_tree();
    assert!(held
        .iter()
        .any(|n| n.id == field && n.part == Some(1) && n.state.pressed));
    release(&mut ui, pos);
    let after = ui.a11y_tree();
    assert!(after
        .iter()
        .any(|n| n.id == field && n.part == Some(1) && !n.state.pressed));
    // Typing still works and the content stays the application's.
    focus(&mut ui, field);
    key(&mut ui, Key::End);
    key(&mut ui, Key::Char('!'));
    assert_eq!(ui.text(field).expect("text"), "sesame 42!");
}

#[test]
fn an_anchored_panel_opens_beside_its_anchor_and_gives_the_focus_back() {
    let mut ui = ui(Lang::Ar);
    let root = ui.root();
    let anchor = ui
        .add(root, IconButton::new(IconId::Wifi, k("demo.panel")))
        .expect("anchor");
    let panel = ui.add(root, Dialog::panel(k("demo.panel"))).expect("panel");
    let night = ui
        .add(panel, Switch::new(k("demo.night"), true))
        .expect("switch");
    focus(&mut ui, anchor);
    ui.open_panel(panel, anchor).expect("open");
    assert!(ui.is_dialog_open(panel));
    ui.layout();
    let (a, p) = (
        ui.rect(anchor).expect("anchor"),
        ui.rect(panel).expect("panel"),
    );
    let surface = ui.size();
    assert!(p.x >= 0.0 && p.right() <= surface.w && p.y >= 0.0 && p.bottom() <= surface.h);
    assert!(
        p.y >= a.bottom() || p.bottom() <= a.y,
        "the panel does not cover its anchor"
    );
    // The focus is inside; nothing behind the panel is in the accessibility tree.
    key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(night));
    assert!(ui.a11y_tree().iter().all(|n| n.id != anchor));
    // A press beside the panel closes it (no scrim), the focus returns to the anchor.
    let beside = Point::new(surface.w - 4.0, surface.h - 4.0);
    press(&mut ui, beside);
    release(&mut ui, beside);
    assert!(!ui.is_dialog_open(panel));
    assert_eq!(ui.focused(), Some(anchor));
}

fn files_table(ui: &mut Ui) -> NodeId {
    let root = ui.root();
    let columns = vec![
        k("demo.table.name"),
        k("demo.table.size"),
        k("demo.table.kind"),
    ];
    let rows = ["a.odt", "b.jpg", "c.odt"]
        .iter()
        .map(|n| vec![Label::data(n), Label::data("1 KB"), k("demo.kind.doc")])
        .collect();
    ui.add(root, Table::new(k("demo.table"), columns).rows(rows))
        .expect("table")
}

#[test]
fn table_keys_choose_rows_activate_them_and_sort_from_the_headers() {
    let mut ui = ui(Lang::En);
    let table = files_table(&mut ui);
    focus(&mut ui, table);
    assert_eq!(
        of(&key(&mut ui, Key::ArrowDown)),
        vec![ActionKind::Selected(0)]
    );
    assert_eq!(of(&key(&mut ui, Key::End)), vec![ActionKind::Selected(2)]);
    assert!(
        of(&key(&mut ui, Key::ArrowDown)).is_empty(),
        "stays on the last row"
    );
    assert_eq!(
        of(&key(&mut ui, Key::Enter)),
        vec![ActionKind::RowActivated(2)]
    );
    assert_eq!(of(&key(&mut ui, Key::Home)), vec![ActionKind::Selected(0)]);
    // Up from the first row reaches the headers; Enter sorts, again reverses.
    assert!(of(&key(&mut ui, Key::ArrowUp)).is_empty());
    assert_eq!(
        of(&key(&mut ui, Key::Enter)),
        vec![ActionKind::Sorted(0, true)]
    );
    assert_eq!(
        of(&key(&mut ui, Key::Space)),
        vec![ActionKind::Sorted(0, false)]
    );
    key(&mut ui, Key::ArrowRight);
    assert_eq!(
        of(&key(&mut ui, Key::Enter)),
        vec![ActionKind::Sorted(1, true)]
    );
    // Accessibility: headers and rows are nodes with their position.
    let tree = ui.a11y_tree();
    let parts: Vec<_> = tree
        .iter()
        .filter(|n| n.id == table && n.part.is_some())
        .collect();
    assert_eq!(
        parts
            .iter()
            .filter(|n| n.role == Role::ColumnHeader)
            .count(),
        3
    );
    let rows: Vec<_> = parts.iter().filter(|n| n.role == Role::TableRow).collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].position, Some((2, 3)));
    assert!(rows.iter().all(|r| r.parent == Some(table)));
}

#[test]
fn table_rows_follow_a_click_and_tab_leaves_the_table() {
    let mut ui = ui(Lang::Ar);
    let table = files_table(&mut ui);
    let row = ui
        .a11y_tree()
        .into_iter()
        .find(|n| n.id == table && n.role == Role::TableRow && n.position == Some((3, 3)))
        .expect("third row");
    let pos = row.rect.center();
    move_to(&mut ui, pos);
    press(&mut ui, pos);
    assert_eq!(of(&release(&mut ui, pos)), vec![ActionKind::Selected(2)]);
    // A double click on a row activates it.
    press(&mut ui, pos);
    let second = release(&mut ui, pos);
    assert!(of(&second).contains(&ActionKind::DoubleActivated));
    // The table is one Tab stop: the next Tab reaches the button after it.
    let root = ui.root();
    let next = ui.add(root, Button::new(k("demo.save"))).expect("button");
    focus(&mut ui, table);
    key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(next));
}

#[test]
fn tree_arrows_open_enter_close_and_go_to_the_parent_in_reading_direction() {
    for (lang, end, start) in [
        (Lang::En, Key::ArrowRight, Key::ArrowLeft),
        (Lang::Ar, Key::ArrowLeft, Key::ArrowRight),
    ] {
        let mut ui = ui(lang);
        let root = ui.root();
        let items = vec![
            TreeItem::new(k("demo.tree.home"), 0),
            TreeItem::new(k("demo.tree.docs"), 1),
            TreeItem::new(k("demo.tree.work"), 2),
            TreeItem::new(k("demo.tree.pictures"), 1),
        ];
        let tree = ui
            .add(root, Tree::new(k("demo.tree"), items))
            .expect("tree");
        focus(&mut ui, tree);
        assert_eq!(
            of(&key(&mut ui, Key::ArrowDown)),
            vec![ActionKind::Selected(0)]
        );
        // Only the top level is visible: Down stays on it.
        assert!(of(&key(&mut ui, Key::ArrowDown)).is_empty());
        assert_eq!(of(&key(&mut ui, end)), vec![ActionKind::Expanded(0, true)]);
        assert_eq!(of(&key(&mut ui, end)), vec![ActionKind::Selected(1)]);
        assert_eq!(of(&key(&mut ui, Key::End)), vec![ActionKind::Selected(3)]);
        assert_eq!(of(&key(&mut ui, start)), vec![ActionKind::Selected(0)]);
        assert_eq!(
            of(&key(&mut ui, start)),
            vec![ActionKind::Expanded(0, false)]
        );
        assert_eq!(
            of(&key(&mut ui, Key::Enter)),
            vec![ActionKind::RowActivated(0)]
        );
        let nodes = ui.a11y_tree();
        let item = nodes
            .iter()
            .find(|n| n.id == tree && n.role == Role::TreeItem)
            .expect("item");
        assert_eq!(item.state.expanded, Some(false));
        assert_eq!(item.state.level, Some(1));
    }
}

#[test]
fn toasts_stack_expire_close_and_answer_keys() {
    let mut ui = ui(Lang::Ar);
    let root = ui.root();
    let save = ui.add(root, Button::new(k("demo.save"))).expect("button");
    for id in 1..=MAX_TOASTS as u32 {
        ui.show_toast(Toast::new(id, Tone::Success, k("demo.toast.saved")))
            .expect("toast");
    }
    // One more replaces the oldest.
    let undo = Toast::new(9, Tone::Info, k("demo.toast.deleted")).action(k("demo.toast.undo"));
    ui.show_toast(undo).expect("toast");
    assert_eq!(kinds(&ui.take_actions()), vec![ActionKind::ToastClosed(1)]);
    assert_eq!(ui.toasts(), vec![2, 3, 9]);
    // Status nodes, and the action and close buttons as nodes of their own.
    let tree = ui.a11y_tree();
    assert_eq!(tree.iter().filter(|n| n.role == Role::Status).count(), 3);
    assert!(tree.iter().all(|n| n.role != Role::Alert));
    let buttons: Vec<_> = tree
        .iter()
        .filter(|n| n.id == root && n.role == Role::Button)
        .collect();
    assert_eq!(buttons.len(), 4, "three close buttons and one action");
    assert!(buttons.iter().any(|b| b.name == Some(k("demo.toast.undo"))));
    assert!(buttons.iter().all(|b| !b.rect.is_empty()));
    // Toasts without an action time out; the one with an action stays.
    ui.advance_to(u64::from(motion::TOAST));
    let closed = kinds(&ui.take_actions());
    assert_eq!(
        closed,
        vec![ActionKind::ToastClosed(2), ActionKind::ToastClosed(3)]
    );
    assert_eq!(ui.toasts(), vec![9]);
    // Ctrl+Enter runs the action from anywhere and closes the toast.
    focus(&mut ui, save);
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let response = ui.handle(Event::Key {
        key: Key::Enter,
        modifiers: ctrl,
    });
    assert_eq!(
        of(&response),
        vec![ActionKind::ToastAction(9), ActionKind::ToastClosed(9)]
    );
    assert!(ui.toasts().is_empty());
    // Escape closes the newest toast; hide_toast closes without an action.
    ui.show_toast(Toast::new(20, Tone::Warning, k("demo.toast.saved")))
        .expect("toast");
    ui.show_toast(Toast::new(21, Tone::Info, k("demo.toast.saved")))
        .expect("toast");
    assert!(of(&key(&mut ui, Key::Escape)).contains(&ActionKind::ToastClosed(21)));
    ui.hide_toast(20);
    assert!(ui.toasts().is_empty());
}

#[test]
fn the_close_button_of_a_toast_closes_it_by_pointer() {
    let mut ui = ui(Lang::En);
    let root = ui.root();
    ui.add(root, Button::new(k("demo.save"))).expect("button");
    ui.show_toast(Toast::new(5, Tone::Info, k("demo.toast.saved")))
        .expect("toast");
    let close = ui
        .a11y_tree()
        .into_iter()
        .find(|n| n.name == Some(k("ui.toast.close")))
        .expect("close button");
    let pos = close.rect.center();
    move_to(&mut ui, pos);
    let down = press(&mut ui, pos);
    assert!(
        of(&down).is_empty(),
        "a toast button acts on release, like every button"
    );
    assert!(of(&release(&mut ui, pos)).contains(&ActionKind::ToastClosed(5)));
    assert!(ui.toasts().is_empty());
    // A press taken back by moving away does nothing.
    ui.show_toast(Toast::new(6, Tone::Info, k("demo.toast.saved")))
        .expect("toast");
    let close = ui
        .a11y_tree()
        .into_iter()
        .find(|n| n.name == Some(k("ui.toast.close")))
        .expect("close button");
    press(&mut ui, close.rect.center());
    assert!(of(&release(&mut ui, Point::new(1.0, 1.0))).is_empty());
    assert_eq!(ui.toasts(), vec![6]);
}
