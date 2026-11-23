//! Review round 3, "Prüfer Bedienung": behaviour the specification asks for (design spec §3,
//! §5.5, §13; docs/ui/CLICKS.md) driven by pointer, touch and keyboard.
//! Target path: platform/ultar-ui/tests/r3_behaviour.rs

mod common;

use common::gallery::{file_menu, gallery, Gallery};
use common::{center, click, key, press, release};
use ultar_ui::event::{ActionKind, Event, Key, PointerButton, Response};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{Button, Label, List, ListItem, Menu, MenuItem, Scroll, Select};
use ultar_ui::{Lang, Point, Rect, Size};

const SURFACE: Size = Size::new(720.0, 560.0);

fn setup(lang: Lang) -> (Ui, Gallery) {
    let mut ui = common::ui(lang, ThemeKind::Dark, SURFACE);
    let g = gallery(&mut ui).unwrap();
    ui.layout();
    (ui, g)
}

fn kinds(response: &Response) -> Vec<ActionKind> {
    response.actions.iter().map(|a| a.kind).collect()
}

fn focus(ui: &mut Ui, id: NodeId) {
    for _ in 0..4200 {
        if ui.focused() == Some(id) {
            return;
        }
        key(ui, Key::Tab);
    }
    panic!("{id:?} is not reachable by Tab");
}

fn wheel(ui: &mut Ui, pos: Point, lines: f32) -> Response {
    ui.handle(Event::Scroll { pos, lines })
}

fn secondary(ui: &mut Ui, pos: Point) -> Response {
    ui.handle(Event::PointerDown {
        pos,
        button: PointerButton::Secondary,
    })
}

fn inside(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x - 0.01
        && inner.y >= outer.y - 0.01
        && inner.right() <= outer.right() + 0.01
        && inner.bottom() <= outer.bottom() + 0.01
}

// ------------------------------------------------------------ modal surfaces block input

/// An open dialog blocks everything behind it (widgets/dialog.rs: "nothing behind it
/// reacts"): the wheel must not move the content behind the scrim.
#[test]
fn b01_the_wheel_does_not_scroll_content_behind_an_open_dialog() {
    let (mut ui, g) = setup(Lang::Ar);
    let pos = Point::new(60.0, 500.0);
    assert!(ui.rect(g.scroll).unwrap().contains(pos));
    ui.open_dialog(g.modal.dialog).unwrap();
    assert!(!ui.rect(g.modal.dialog).unwrap().contains(pos));
    let response = wheel(&mut ui, pos, 3.0);
    assert_eq!(
        ui.scroll_offset(g.scroll).unwrap(),
        0.0,
        "content behind the dialog moved"
    );
    assert!(response.actions.is_empty(), "{:?}", response.actions);
}

/// The same for a finger and for the scrollbar: neither drags content behind a dialog.
#[test]
fn b02_no_drag_scrolls_content_behind_an_open_dialog() {
    for lang in [Lang::Ar, Lang::En] {
        let (mut ui, g) = setup(lang);
        let view = ui.rect(g.scroll).unwrap();
        ui.open_dialog(g.modal.dialog).unwrap();
        let from = Point::new(view.x + 60.0, view.bottom() - 20.0);
        let to = Point::new(from.x, from.y - 120.0);
        assert!(!ui.rect(g.modal.dialog).unwrap().contains(from));
        ui.handle(Event::TouchDown { id: 1, pos: from });
        ui.handle(Event::TouchMove { id: 1, pos: to });
        ui.handle(Event::TouchUp { id: 1, pos: to });
        assert_eq!(ui.scroll_offset(g.scroll).unwrap(), 0.0, "{lang:?}: finger");
        let bar_x = if lang == Lang::Ar {
            view.x + 5.0
        } else {
            view.right() - 5.0
        };
        let grip = Point::new(bar_x, view.bottom() - 4.0);
        press(&mut ui, grip);
        release(&mut ui, grip);
        assert_eq!(ui.scroll_offset(g.scroll).unwrap(), 0.0, "{lang:?}: bar");
        assert!(ui.is_dialog_open(g.modal.dialog));
    }
}

/// Spec §13.5/§13.62: a sheet closes on a click outside and focus returns to its trigger;
/// the click that closes it must not operate what lies under it (no re-open).
#[test]
fn b03_a_click_on_the_trigger_behind_a_sheet_closes_it_and_returns_focus() {
    let (mut ui, g) = setup(Lang::Ar);
    click(&mut ui, g.search);
    assert_eq!(ui.focused(), Some(g.search));
    ui.open_dialog(g.modal.sheet).unwrap();
    assert_eq!(
        ui.focused(),
        Some(g.modal.close),
        "the sheet takes the focus"
    );
    let rect = ui.rect(g.search).unwrap();
    let trigger = Point::new(rect.right() - 40.0, rect.center().y);
    assert!(!ui.rect(g.modal.sheet).unwrap().contains(trigger));
    let mut actions = press(&mut ui, trigger).actions;
    actions.extend(release(&mut ui, trigger).actions);
    assert!(!ui.is_dialog_open(g.modal.sheet));
    assert_eq!(ui.focused(), Some(g.search), "focus back on the trigger");
    assert!(
        actions.iter().all(|a| a.node != g.search),
        "the closing click must not operate the trigger again: {actions:?}"
    );
}

/// Enter on a focused button presses THAT button, also inside a dialog whose default action
/// is another (destructive) one.
#[test]
fn b04_enter_on_the_focused_cancel_button_never_runs_the_default_action() {
    let (mut ui, g) = setup(Lang::En);
    focus(&mut ui, g.menu_button);
    ui.open_dialog(g.modal.dialog).unwrap();
    focus(&mut ui, g.modal.cancel);
    for k in [Key::Enter, Key::Space] {
        let response = key(&mut ui, k);
        let pressed: Vec<_> = response.actions.iter().map(|a| a.node).collect();
        assert_eq!(pressed, vec![g.modal.cancel], "{k:?}");
    }
}

// ------------------------------------------------------------ focus

/// A disabled widget cannot hold the keyboard focus: the next Tab continues from its place
/// and Enter/Space never reach it.
#[test]
fn b05_disabling_the_focused_widget_does_not_leave_focus_on_it() {
    let (mut ui, g) = setup(Lang::En);
    focus(&mut ui, g.menu_button);
    ui.set_disabled(g.menu_button, true).unwrap();
    if let Some(id) = ui.focused() {
        assert!(!ui.is_disabled(id), "focus sits on a disabled widget");
    }
    assert!(kinds(&key(&mut ui, Key::Enter)).is_empty());
    key(&mut ui, Key::Tab);
    assert_eq!(
        ui.focused(),
        Some(g.settings),
        "Tab continues after the place of the disabled widget, not from the start"
    );
}

/// Focus returns to where it was when a menu closes: by Escape, by choosing, by Tab.
#[test]
fn b06_focus_is_back_on_the_button_after_its_menu_closes() {
    for close in [Key::Escape, Key::Enter, Key::Tab] {
        let (mut ui, g) = setup(Lang::Ar);
        focus(&mut ui, g.menu_button);
        ui.open_menu(g.menu_button, file_menu()).unwrap();
        key(&mut ui, close);
        assert_eq!(ui.menu_owner(), None, "{close:?}");
        assert_eq!(ui.focused(), Some(g.menu_button), "{close:?}");
        assert!(ui.visual(g.menu_button).focused, "{close:?}: ring visible");
    }
}

/// Spec §13.13 (`files.search`): "Esc clears and leaves the field".
#[test]
fn b07_escape_clears_a_search_field_and_leaves_it() {
    let (mut ui, g) = setup(Lang::En);
    focus(&mut ui, g.search);
    for ch in "wifi".chars() {
        key(&mut ui, Key::Char(ch));
    }
    assert_eq!(ui.text(g.search).unwrap(), "wifi");
    let response = key(&mut ui, Key::Escape);
    assert_eq!(kinds(&response), vec![ActionKind::TextChanged]);
    assert_eq!(ui.text(g.search).unwrap(), "");
    assert_ne!(
        ui.focused(),
        Some(g.search),
        "the caret is still in the field after Esc"
    );
}

// ------------------------------------------------------------ hover, tooltip

/// After the wheel moved the content, the widget under the resting pointer is the hovered
/// one (and not the one that scrolled away).
#[test]
fn b08_hover_follows_the_content_under_a_resting_pointer() {
    let (mut ui, g) = setup(Lang::En);
    let pos = center(&mut ui, g.checkbox);
    ui.handle(Event::PointerMove { pos });
    assert_eq!(ui.hovered(), Some(g.checkbox));
    wheel(&mut ui, pos, 2.0);
    assert!(ui.scroll_offset(g.scroll).unwrap() > 0.0);
    let still_there = ui.rect(g.checkbox).is_some_and(|r| r.contains(pos));
    assert!(!still_there, "the checkbox scrolled away from the pointer");
    assert_ne!(ui.hovered(), Some(g.checkbox), "stale hover highlight");
}

/// A tooltip never stays on screen away from its widget: scrolling the widget away hides it.
#[test]
fn b09_a_tooltip_does_not_stay_behind_when_its_widget_scrolls_away() {
    let (mut ui, g) = setup(Lang::En);
    ui.set_tooltip(g.checkbox, Some(Label::key("demo.tip.settings")))
        .unwrap();
    ui.advance_to(1_000);
    let pos = center(&mut ui, g.checkbox);
    ui.handle(Event::PointerMove { pos });
    let due = ui.next_wake().expect("wake for the tooltip");
    ui.advance_to(due);
    assert_eq!(ui.tooltip_shown(), Some(g.checkbox));
    wheel(&mut ui, pos, 3.0);
    assert_eq!(ui.tooltip_shown(), None);
    ui.advance_to(due + 10_000);
    assert_ne!(ui.tooltip_shown(), Some(g.checkbox));
}

/// Tooltips are a pointer and keyboard aid: a finger tap never leaves one on screen.
#[test]
fn b10_a_touch_tap_shows_no_tooltip() {
    let (mut ui, g) = setup(Lang::Ar);
    ui.set_input_mode(InputMode::Touch);
    ui.advance_to(1_000);
    let pos = center(&mut ui, g.settings);
    ui.handle(Event::TouchDown { id: 7, pos });
    ui.handle(Event::TouchUp { id: 7, pos });
    assert_eq!(ui.next_wake(), None);
    ui.advance_to(60_000);
    assert_eq!(ui.tooltip_shown(), None);
}

// ------------------------------------------------------------ menus

fn tall_menu() -> Menu {
    let item = |i: u32| MenuItem::new(i, Label::data(&format!("Command {i}")));
    Menu::new((1..=16).map(item).collect())
}

/// A menu that is allowed by the limits (16 commands) must be usable on a small surface:
/// every command inside the surface or reachable by scrolling.
#[test]
fn b11_every_command_of_a_tall_menu_is_reachable_on_a_small_surface() {
    let size = Size::new(360.0, 320.0);
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, size);
    ui.set_input_mode(InputMode::Touch);
    let root = ui.root();
    let button = ui
        .add(root, Button::new(Label::key("demo.menu.button")))
        .unwrap();
    focus(&mut ui, button);
    ui.open_menu(button, tall_menu()).unwrap();
    let surface = Rect::new(0.0, 0.0, size.w, size.h);
    key(&mut ui, Key::End);
    assert_eq!(ui.menu_position().unwrap().1, Some(15));
    let tree = ui.a11y_tree();
    let last = tree
        .iter()
        .find(|n| n.state.focused && n.name == Some(Label::data("Command 16")))
        .expect("highlighted command");
    assert!(
        inside(surface, last.rect),
        "the highlighted command is outside the surface: {:?}",
        last.rect
    );
}

/// Type-ahead: a letter moves the highlight to the next command that starts with it
/// (parity list of spec §10; menus and lists of every desktop do this).
#[test]
fn b12_typing_a_letter_in_a_menu_jumps_to_the_command() {
    let (mut ui, g) = setup(Lang::En);
    focus(&mut ui, g.menu_button);
    ui.open_menu(g.menu_button, file_menu()).unwrap();
    key(&mut ui, Key::Char('s'));
    assert_eq!(ui.menu_position().unwrap().1, Some(2), "S -> Share");
    key(&mut ui, Key::Char('d'));
    assert_eq!(ui.menu_position().unwrap().1, Some(5), "D -> Delete");
    key(&mut ui, Key::Char('p'));
    assert_eq!(
        ui.menu_position().unwrap().1,
        Some(3),
        "P -> Pin to top (Properties is disabled)"
    );
}

/// Type-ahead in the open list of a select, Arabic included.
#[test]
fn b13_typing_a_letter_in_an_open_select_jumps_to_the_option() {
    for (lang, letter) in [(Lang::En, 'p'), (Lang::Ar, 'ع')] {
        let mut ui = common::ui(lang, ThemeKind::Dark, SURFACE);
        let root = ui.root();
        let select = ui.add(root, common::orientation_select()).unwrap();
        focus(&mut ui, select);
        key(&mut ui, Key::Enter);
        assert!(ui.is_open(select));
        key(&mut ui, Key::Char(letter));
        key(&mut ui, Key::Enter);
        assert_eq!(
            ui.selected(select).unwrap(),
            1,
            "{lang:?}: Portrait / عمودي"
        );
    }
}

/// A second right click outside the open context menu opens the menu of the new place at
/// once (one gesture, not two).
#[test]
fn b14_a_right_click_elsewhere_moves_the_context_menu() {
    let (mut ui, g) = setup(Lang::En);
    let first = center(&mut ui, g.items[2]);
    secondary(&mut ui, first);
    let panel = ui.menu_rect(0).unwrap();
    let second = Point::new(panel.x - 40.0, center(&mut ui, g.items[0]).y);
    assert!(ui.rect(g.items[0]).unwrap().contains(second));
    secondary(&mut ui, second);
    let moved = ui
        .menu_rect(0)
        .expect("a context menu is open at the new place");
    assert!((moved.x - second.x).abs() < 1.0 && (moved.y - second.y).abs() < 1.0);
}

/// Spec §5.5 (the same shell on phone and tablet) and §13.7/§13.15 (right click: context
/// menu): by touch the context menu opens on a long press.
#[test]
fn b15_a_long_press_opens_the_context_menu_by_touch() {
    let (mut ui, g) = setup(Lang::Ar);
    ui.set_input_mode(InputMode::Touch);
    ui.advance_to(1_000);
    let pos = center(&mut ui, g.items[2]);
    ui.handle(Event::TouchDown { id: 3, pos });
    ui.advance_to(2_500);
    assert_eq!(
        ui.menu_owner(),
        Some(g.list),
        "no menu after 1.5 s of holding"
    );
    let up = ui.handle(Event::TouchUp { id: 3, pos });
    assert!(
        !kinds(&up).contains(&ActionKind::Activated),
        "the long press must not also activate the item"
    );
}

/// A submenu never hides the command it belongs to (compact width: no room on either side).
#[test]
fn b16_a_submenu_on_a_narrow_surface_leaves_its_parent_command_visible() {
    for lang in [Lang::Ar, Lang::En] {
        let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(320.0, 480.0));
        let root = ui.root();
        let button = ui
            .add(root, Button::new(Label::key("demo.menu.button")))
            .unwrap();
        focus(&mut ui, button);
        ui.open_menu(button, file_menu()).unwrap();
        key(&mut ui, Key::ArrowDown);
        key(&mut ui, Key::ArrowDown);
        key(&mut ui, Key::Enter);
        assert_eq!(ui.menu_position().unwrap().0, &[2][..]);
        let (parent, child) = (ui.menu_rect(0).unwrap(), ui.menu_rect(1).unwrap());
        let row_y = ui
            .a11y_tree()
            .iter()
            .find(|n| n.state.expanded == Some(true))
            .map(|n| n.rect)
            .expect("the open command");
        let covered = child.x <= row_y.x && child.right() >= row_y.right();
        let over = child.y < row_y.bottom() && child.bottom() > row_y.y;
        assert!(
            !(covered && over),
            "{lang:?}: the submenu {child:?} hides its command {row_y:?} (panel {parent:?})"
        );
    }
}

// ------------------------------------------------------------ long lists

fn long_list(ui: &mut Ui, count: usize) -> (NodeId, Vec<NodeId>) {
    let root = ui.root();
    let scroll = ui
        .add_sized(root, Scroll::new(), ultar_ui::layout::Sizing::grow(1))
        .unwrap();
    let list = ui.add(scroll, List::new().selectable()).unwrap();
    let items = (0..count)
        .map(|i| {
            let item = ListItem::new(Label::data(&format!("ملف {i}")));
            ui.add(list, item).unwrap()
        })
        .collect();
    ui.layout();
    (scroll, items)
}

/// A long list (2000 rows): arrows walk it, the focused row is always fully visible, End
/// and Home reach both ends, and one step stays cheap.
#[test]
fn b17_a_long_list_is_walked_by_keyboard_with_the_focus_in_view() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let (scroll, items) = long_list(&mut ui, 2000);
    let view = ui.rect(scroll).unwrap();
    focus(&mut ui, items[0]);
    let start = std::time::Instant::now();
    for item in items.iter().take(400).skip(1) {
        key(&mut ui, Key::ArrowDown);
        assert_eq!(ui.focused(), Some(*item));
        let rect = ui.rect(*item).unwrap();
        assert!(inside(view, rect), "{item:?} at {rect:?} outside {view:?}");
    }
    let per_step = start.elapsed().as_secs_f64() * 1000.0 / 399.0;
    key(&mut ui, Key::End);
    let max = ui.scroll_offset(scroll).unwrap();
    assert!(max > 2000.0 * 40.0, "End scrolls to the end: {max}");
    key(&mut ui, Key::Home);
    assert_eq!(ui.scroll_offset(scroll).unwrap(), 0.0);
    println!("long list: {per_step:.3} ms per arrow step (debug build, 2000 rows)");
}

/// The wheel over a long list: a click after scrolling hits the row that is drawn there.
#[test]
fn b18_a_click_after_scrolling_selects_the_row_under_the_pointer() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, SURFACE);
    let (scroll, items) = long_list(&mut ui, 300);
    let view = ui.rect(scroll).unwrap();
    let pos = view.center();
    wheel(&mut ui, pos, 37.0);
    let offset = ui.scroll_offset(scroll).unwrap();
    assert!(offset > 0.0);
    let under: Vec<_> = items
        .iter()
        .filter(|id| ui.rect(**id).is_some_and(|r| r.contains(pos)))
        .collect();
    assert_eq!(under.len(), 1);
    let mut actions = press(&mut ui, pos).actions;
    actions.extend(release(&mut ui, pos).actions);
    assert!(actions.iter().any(|a| a.node == *under[0]), "{actions:?}");
    assert_eq!(ui.is_item_selected(*under[0]), Ok(true));
}

// ------------------------------------------------------------ select at the edge, disabled

/// A select opened by keyboard inside a dialog: Escape closes the list only.
#[test]
fn b19_escape_closes_the_open_select_before_the_dialog() {
    let (mut ui, g) = setup(Lang::Ar);
    let select = ui
        .add(
            g.modal.dialog,
            Select::new(
                Label::key("demo.orientation"),
                vec![
                    Label::key("demo.orientation.landscape"),
                    Label::key("demo.orientation.portrait"),
                ],
            ),
        )
        .unwrap();
    ui.open_dialog(g.modal.dialog).unwrap();
    focus(&mut ui, select);
    key(&mut ui, Key::Space);
    assert!(ui.is_open(select));
    key(&mut ui, Key::Escape);
    assert!(!ui.is_open(select));
    assert!(ui.is_dialog_open(g.modal.dialog), "one Escape, one layer");
    assert_eq!(ui.focused(), Some(select));
}

/// Disabled list items, tabs and radio groups react to nothing, by pointer or keyboard.
#[test]
fn b20_disabled_widgets_ignore_pointer_touch_and_keys() {
    let (mut ui, g) = setup(Lang::Ar);
    for id in [g.items[2], g.tabs, g.radio, g.checkbox, g.search] {
        ui.set_disabled(id, true).unwrap();
        let pos = center(&mut ui, id);
        assert!(kinds(&click(&mut ui, id)).is_empty(), "{id:?}");
        ui.handle(Event::TouchDown { id: 1, pos });
        assert!(kinds(&ui.handle(Event::TouchUp { id: 1, pos })).is_empty());
        assert!(!ui.focus_order().contains(&id), "{id:?} is a Tab stop");
        assert_ne!(ui.focused(), Some(id));
    }
}
