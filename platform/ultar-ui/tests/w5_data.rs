//! Review round 5 (second reviewer): the mutation, selection, drag and hold APIs of wave 5.
//! Target path: platform/ultar-ui/tests/w5_data.rs

mod common;

use common::{center, key, press, release};
use ultar_ui::event::{ActionKind, Event, Key, Modifiers};
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::tokens::{size, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{Button, Label, List, ListItem, Scroll, Table};
use ultar_ui::{Lang, Point, Size};

fn ui() -> Ui {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(480.0, 400.0));
    ui.set_root_flex(Flex::column());
    ui
}

fn button(ui: &mut Ui, parent: NodeId, text: &str) -> NodeId {
    ui.add(parent, Button::new(Label::data(text))).unwrap()
}

/// `Ui::remove` documents "focus moves on to the next stop (as when a focused node is
/// disabled)". `remove` disables first (which remembers the place), then forgets the removed
/// identifier – the remembered place with it – so the next Tab starts at the first widget.
#[test]
fn da01_removing_the_focused_node_moves_the_focus_to_the_next_stop() {
    let mut ui = ui();
    let root = ui.root();
    let ids: Vec<NodeId> = ["a", "b", "c", "d"]
        .iter()
        .map(|t| button(&mut ui, root, t))
        .collect();
    for _ in 0..3 {
        key(&mut ui, Key::Tab);
    }
    assert_eq!(ui.focused(), Some(ids[2]));
    ui.remove(ids[2]).unwrap();
    key(&mut ui, Key::Tab);
    assert_eq!(
        ui.focused(),
        Some(ids[3]),
        "Tab after removing c must reach d"
    );
}

/// Removed identifiers are never reused and `MAX_NODES` counts every node ever added. A
/// long-running app that replaces a few rows at a time (a chat, a log view, a settings page
/// that rebuilds a section) runs out of nodes although it never shows more than a handful.
#[test]
fn da02_a_long_running_app_can_replace_rows_forever() {
    let mut ui = ui();
    let root = ui.root();
    let list = ui.add(root, List::new()).unwrap();
    let mut rows: Vec<NodeId> = Vec::new();
    for round in 0..2000 {
        for id in rows.drain(..) {
            ui.remove(id).unwrap();
        }
        for i in 0..3 {
            let item = ListItem::new(Label::data(&format!("message {round}.{i}")));
            match ui.add(list, item) {
                Ok(id) => rows.push(id),
                Err(e) => panic!("round {round}: {e:?} with only 3 rows on screen"),
            }
        }
    }
}

fn files(ui: &mut Ui, names: &[&str]) -> NodeId {
    let root = ui.root();
    let rows = names.iter().map(|n| vec![Label::data(n)]).collect();
    let table = Table::new(Label::data("Files"), vec![Label::data("Name")])
        .rows(rows)
        .multi();
    ui.add(root, table).unwrap()
}

fn marked(ui: &Ui, table: NodeId, names: &[&str]) -> Vec<String> {
    ui.selected_rows(table)
        .unwrap()
        .into_iter()
        .map(|r| names[r].to_string())
        .collect()
}

/// The `Sorted` flow: the user marks files, sorts, the app replaces the rows in the new
/// order (`set_table_rows`, "selection kept where it still exists"). Selection is kept by
/// index, so after sorting other files are marked – a following "Delete" deletes the wrong
/// files.
#[test]
fn da03_sorting_by_replacing_rows_keeps_the_same_files_selected() {
    let mut ui = ui();
    let before = ["a.txt", "b.txt", "c.txt", "d.txt"];
    let table = files(&mut ui, &before);
    ui.set_selected_rows(table, &[0, 1]).unwrap();
    assert_eq!(marked(&ui, table, &before), ["a.txt", "b.txt"]);
    let after = ["d.txt", "c.txt", "b.txt", "a.txt"];
    let rows = after.iter().map(|n| vec![Label::data(n)]).collect();
    ui.set_table_rows(table, rows).unwrap();
    let mut now = marked(&ui, table, &after);
    now.sort();
    assert_eq!(
        now,
        ["a.txt", "b.txt"],
        "other files are selected after sorting"
    );
}

/// `List::multi` documents "Shift with a click or an arrow selects a range"; only the
/// table has the keyboard part.
#[test]
fn da04_shift_arrow_extends_the_selection_of_a_multi_list() {
    let mut ui = ui();
    let root = ui.root();
    let list = ui.add(root, List::new().multi()).unwrap();
    for t in ["one", "two", "three"] {
        ui.add(list, ListItem::new(Label::data(t))).unwrap();
    }
    key(&mut ui, Key::Tab);
    key(&mut ui, Key::Space);
    ui.handle(Event::Key {
        key: Key::ArrowDown,
        modifiers: Modifiers::SHIFT,
    });
    assert_eq!(ui.selected_rows(list).unwrap(), vec![0, 1]);
}

/// A held button keeps reporting `Repeated` after it was disabled or removed, and never
/// reports `Released` (a phone's backspace keeps deleting).
#[test]
fn da05_a_held_button_stops_when_it_is_disabled_or_removed() {
    for remove in [true, false] {
        let mut ui = ui();
        let root = ui.root();
        let b = ui.add(root, Button::new(Label::data("⌫")).hold()).unwrap();
        let pos = center(&mut ui, b);
        ui.advance_to(1000);
        press(&mut ui, pos);
        ui.advance_to(1000 + 2 * u64::from(ultar_ui::tokens::motion::LONG_PRESS));
        ui.take_actions();
        if remove {
            ui.remove(b).unwrap();
        } else {
            ui.set_disabled(b, true).unwrap();
        }
        ui.advance_to(10_000);
        let late: Vec<ActionKind> = ui.take_actions().into_iter().map(|a| a.kind).collect();
        assert!(
            !late.contains(&ActionKind::Repeated),
            "remove={remove}: still repeating: {late:?}"
        );
        release(&mut ui, pos);
    }
}

/// A drop target that the user scrolls into place during a drag (wheel while holding).
#[test]
fn da06_a_drop_target_scrolled_under_the_pointer_during_a_drag_is_the_target() {
    let mut ui = ui();
    let root = ui.root();
    let tile = button(&mut ui, root, "tile");
    let scroll = ui.add_sized(root, Scroll::new(), Sizing::grow(1)).unwrap();
    let col = ui.add(scroll, Flex::column()).unwrap();
    let mut targets = Vec::new();
    for i in 0..30 {
        let t = ui
            .add(col, Button::new(Label::data(&format!("folder {i}"))))
            .unwrap();
        ui.set_drop_target(t, true).unwrap();
        targets.push(t);
    }
    ui.set_draggable(tile, true).unwrap();
    let from = center(&mut ui, tile);
    let over = center(&mut ui, targets[2]);
    ui.handle(Event::PointerMove { pos: from });
    press(&mut ui, from);
    let step = Point::new(from.x, from.y + size::TOUCH_SLOP * 2.0);
    ui.handle(Event::PointerMove { pos: step });
    ui.handle(Event::PointerMove { pos: over });
    ui.handle(Event::Scroll {
        pos: over,
        lines: 6.0,
    });
    let under = ui.node_at(over);
    let end = release(&mut ui, over);
    let target = end.actions.iter().find_map(|a| match a.kind {
        ActionKind::DragEnded { target, .. } => Some(target),
        _ => None,
    });
    assert!(
        ui.scroll_offset(scroll).unwrap() > 0.0,
        "the wheel scrolls during a drag"
    );
    assert_eq!(
        target,
        Some(under),
        "dropped on what is under the pointer now"
    );
}

/// w-m2 residual: hover returns after an options list closes, not after a menu closes by
/// a press beside it – the select under the pointer stays without hover until the pointer
/// moves, while any unrelated layout shows it (two pictures for one state).
#[test]
fn da07_hover_returns_when_a_menu_closes_by_a_press_beside_it() {
    let mut ui = ui();
    let root = ui.root();
    let options = vec![Label::data("a"), Label::data("b")];
    let select = ui
        .add(
            root,
            ultar_ui::widgets::Select::new(Label::data("x"), options),
        )
        .unwrap();
    let owner = button(&mut ui, root, "menu");
    let pos = center(&mut ui, select);
    ui.open_menu(owner, common::gallery::file_menu()).unwrap();
    ui.handle(Event::PointerMove { pos });
    press(&mut ui, pos);
    release(&mut ui, pos);
    assert_eq!(ui.menu_owner(), None);
    assert_eq!(
        ui.hovered(),
        Some(select),
        "the pointer rests on the select"
    );
}

/// `open_menu` accepts an owner that is not shown (inside a closed dialog): the menu is
/// drawn and announced until the next layout silently closes it.
#[test]
fn da08_a_menu_cannot_open_on_a_node_that_is_not_shown() {
    let mut ui = ui();
    let root = ui.root();
    let dialog = ui
        .add(root, ultar_ui::widgets::Dialog::new(Label::data("d")))
        .unwrap();
    let hidden = button(&mut ui, dialog, "inside a closed dialog");
    ui.layout();
    let opened = ui.open_menu(hidden, common::gallery::file_menu());
    assert!(
        opened.is_err() || ui.menu_owner().is_none(),
        "a menu opened for a node that is not on the surface"
    );
}

/// w-m1 partly: the tab strip scrolls only to keep the chosen tab visible. Nothing scrolls
/// it by pointer or touch (no wheel, swipe or overflow button), so on a phone the tabs
/// past the edge cannot be reached at all: tapping the last visible tab does not reveal
/// the next one.
#[test]
fn da09_every_tab_can_be_reached_by_touch() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(200.0, 120.0));
    ui.set_input_mode(ultar_ui::tokens::InputMode::Touch);
    let root = ui.root();
    let names: Vec<Label> = (1..=8).map(|i| Label::data(&format!("Tab {i}"))).collect();
    let tabs = ui
        .add(
            root,
            ultar_ui::widgets::Tabs::new(Label::key("demo.tabs"), names),
        )
        .unwrap();
    for round in 0..10u64 {
        let tab_nodes: Vec<_> = ui
            .a11y_tree()
            .into_iter()
            .filter(|n| n.id == tabs && n.part.is_some())
            .filter(|n| n.rect.x >= 0.0 && n.rect.right() <= 200.0)
            .collect();
        let last = tab_nodes.iter().max_by_key(|n| n.part).unwrap();
        let pos = last.rect.center();
        ui.advance_to(1000 * (round + 1));
        ui.handle(Event::TouchDown { id: 1, pos });
        ui.handle(Event::TouchUp { id: 1, pos });
        ui.handle(Event::Scroll { pos, lines: 3.0 });
    }
    assert_eq!(
        ui.selected(tabs).unwrap(),
        7,
        "tab 8 cannot be reached by touch"
    );
}
