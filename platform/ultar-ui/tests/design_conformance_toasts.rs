//! Design conformance – the toolkit details of the boards "States", "Menus" and "Bricks" that
//! the colour samples of `design_conformance.rs` do not reach, and the toast timing decided
//! for DESKTOP-SPEC §3 / CLICKS.md (`toast.action`, `toast.close`):
//! a toast without action closes after motion.toast (4 s); a toast with an action stays
//! motion.toast_action (10 s), its time stands still while the pointer is over it or one of
//! its buttons has the keyboard focus (WCAG 2.2.1), and Esc or its close button closes it
//! at any time.

mod common;

use common::{click, key};
use ultar_ui::event::{ActionKind, Event, Key, Modifiers};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::snapshot::render;
use ultar_ui::tokens::{motion, ColorRole, Space, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::widgets::{
    Button, ButtonVariant, Label, List, ListItem, Menu, MenuItem, TextDirection, TextField, Toast,
    Tone,
};
use ultar_ui::{Lang, Point, Size};

const SHORT: u64 = motion::TOAST as u64;
const LONG: u64 = motion::TOAST_ACTION as u64;

fn surface(lang: Lang) -> Ui {
    let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(1280.0, 800.0));
    ui.set_background(ColorRole::Surface);
    ui.set_root_flex(
        Flex::column()
            .main(MainAlign::Start)
            .cross(CrossAlign::Start)
            .padding(Padding::all(Space::S32)),
    );
    ui
}

fn with_toast(action: bool) -> Ui {
    let mut ui = surface(Lang::En);
    let root = ui.root();
    ui.add(root, Button::new(Label::data("Open"))).unwrap();
    let toast = Toast::new(7, Tone::Success, Label::data("3 files moved to the Trash."));
    let toast = if action {
        toast.action(Label::data("Undo"))
    } else {
        toast
    };
    ui.show_toast(toast).unwrap();
    render(&mut ui).unwrap();
    ui
}

/// Centre of the toast (on its message, not on a button).
fn toast_message(ui: &mut Ui) -> Point {
    let tree = ui.a11y_tree();
    let status = tree
        .iter()
        .find(|n| {
            n.name
                .as_ref()
                .is_some_and(|l| l.resolve(ui.translator()).contains("Trash"))
        })
        .expect("toast in the accessibility tree");
    Point::new(status.rect.x + 80.0, status.rect.center().y)
}

fn closed(ui: &mut Ui) -> bool {
    ui.take_actions()
        .iter()
        .any(|a| a.kind == ActionKind::ToastClosed(7))
        || ui.toasts().is_empty()
}

#[test]
fn a_toast_without_action_closes_after_motion_toast_even_under_the_pointer() {
    let mut ui = with_toast(false);
    let pos = toast_message(&mut ui);
    ui.handle(Event::PointerMove { pos });
    ui.advance_to(SHORT - 1);
    assert_eq!(ui.toasts(), vec![7]);
    ui.advance_to(SHORT);
    assert!(closed(&mut ui), "closes after motion.toast");
}

#[test]
fn a_toast_with_action_stays_and_pauses_under_the_pointer() {
    let mut ui = with_toast(true);
    ui.advance_to(2_000);
    let pos = toast_message(&mut ui);
    ui.handle(Event::PointerMove { pos });
    // Under the pointer the time stands still, however long.
    ui.advance_to(60_000);
    assert_eq!(ui.toasts(), vec![7], "paused under the pointer");
    ui.handle(Event::PointerMove {
        pos: Point::new(5.0, 5.0),
    });
    // 2 s had run before; 8 s remain from 60 s on.
    ui.advance_to(60_000 + LONG - 2_000 - 1);
    assert_eq!(ui.toasts(), vec![7]);
    ui.advance_to(60_000 + LONG - 2_000);
    assert!(closed(&mut ui), "closes once its 10 s have run");
}

#[test]
fn keyboard_focus_on_a_toast_pauses_it_and_enter_runs_the_focused_button() {
    let mut ui = with_toast(true);
    let open = ui.focus_order()[0];
    // Tab: the button, then past the last control into the toast (its action first).
    key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(open));
    key(&mut ui, Key::Tab);
    assert_eq!(
        ui.toast_focus(),
        Some((7, true)),
        "Tab reaches the toast action"
    );
    let focused = ui
        .a11y_tree()
        .into_iter()
        .filter(|n| n.state.focused)
        .count();
    assert_eq!(focused, 1, "the focused toast button is announced");
    ui.advance_to(LONG * 5);
    assert_eq!(
        ui.toasts(),
        vec![7],
        "paused while it has the keyboard focus"
    );
    key(&mut ui, Key::Tab);
    assert_eq!(ui.toast_focus(), Some((7, false)), "then its close button");
    key(&mut ui, Key::Tab);
    assert_eq!(ui.toast_focus(), None, "Tab leaves the toast");
    assert_eq!(ui.focused(), Some(open), "and wraps to the first control");
    // Back into the toast from the start with Shift+Tab: the close button first.
    ui.handle(Event::Key {
        key: Key::Tab,
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
    });
    assert_eq!(ui.toast_focus(), Some((7, false)));
    ui.handle(Event::Key {
        key: Key::Tab,
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
    });
    assert_eq!(ui.toast_focus(), Some((7, true)));
    let r = key(&mut ui, Key::Enter);
    let kinds: Vec<_> = r.actions.iter().map(|a| a.kind).collect();
    assert!(kinds.contains(&ActionKind::ToastAction(7)), "{kinds:?}");
    assert!(ui.toasts().is_empty(), "closed after its action");
    assert_eq!(ui.focused(), Some(open), "focus returns where it was");
}

#[test]
fn escape_closes_a_focused_toast_and_its_time_runs_on_after_focus_leaves() {
    let mut ui = with_toast(true);
    key(&mut ui, Key::Tab);
    key(&mut ui, Key::Tab);
    assert!(ui.toast_focus().is_some());
    let r = key(&mut ui, Key::Escape);
    assert!(r
        .actions
        .iter()
        .any(|a| a.kind == ActionKind::ToastClosed(7)));
    // Focus leaving by a click elsewhere lets the time run again.
    let mut ui = with_toast(true);
    key(&mut ui, Key::Tab);
    key(&mut ui, Key::Tab);
    ui.advance_to(30_000);
    let open = ui.focus_order()[0];
    click(&mut ui, open);
    assert_eq!(ui.toast_focus(), None);
    ui.advance_to(30_000 + LONG - 1);
    assert_eq!(ui.toasts(), vec![7]);
    ui.advance_to(30_000 + LONG);
    assert!(closed(&mut ui));
}

/// T4: a left-to-right value (`dir="ltr"` on the board) starts at the left edge of the field
/// in an Arabic interface.
#[test]
fn a_left_to_right_value_starts_at_the_left_in_arabic() {
    let mut ui = surface(Lang::Ar);
    let root = ui.root();
    let field = ui
        .add_sized(
            root,
            TextField::new(Label::data("اسم الجهاز"))
                .text("ultar-pc")
                .direction(TextDirection::Ltr),
            Sizing::width(228.0),
        )
        .unwrap();
    let canvas = render(&mut ui).unwrap();
    let rect = ui.rect(field).unwrap();
    let ground = canvas
        .pixel((rect.x + 6.0) as i32, (rect.y + 16.0) as i32)
        .unwrap();
    let inked = |x0: f32, x1: f32| {
        (x0 as i32..x1 as i32).any(|x| {
            (rect.y as i32 + 8..rect.y as i32 + 24)
                .any(|y| canvas.pixel(x, y).is_some_and(|p| p != ground))
        })
    };
    assert!(
        inked(rect.x + 12.0, rect.x + 24.0),
        "text at the left padding"
    );
    assert!(
        !inked(rect.right() - 60.0, rect.right() - 4.0),
        "nothing at the right"
    );
}

/// T2: a list row that opens a page shows the forward arrow at its end (left in Arabic).
#[test]
fn a_page_row_shows_the_forward_arrow_at_its_end() {
    let shot = |chevron: bool| {
        let mut ui = surface(Lang::Ar);
        let root = ui.root();
        let list = ui
            .add_sized(root, List::navigation(), Sizing::width(228.0))
            .unwrap();
        let item = ListItem::new(Label::data("KM-Office"))
            .sublabel(Label::data("محفوظة"))
            .icon(IconId::Wifi);
        let item = if chevron { item.chevron() } else { item };
        let id = ui.add(list, item).unwrap();
        let canvas = render(&mut ui).unwrap();
        let rect = ui.rect(id).unwrap();
        assert!((rect.h - 56.0).abs() <= 1.0, "two lines: a 56-point row");
        // The end slot: 12 padding, 16 arrow.
        (rect.x as i32 + 12..rect.x as i32 + 28)
            .flat_map(|x| (rect.y as i32 + 20..rect.y as i32 + 36).map(move |y| (x, y)))
            .filter(|(x, y)| {
                canvas
                    .pixel(*x, *y)
                    .is_some_and(|p| p.r > 150 && p.g > 150 && p.b > 150)
            })
            .count()
    };
    assert!(shot(true) > 4, "arrow drawn");
    assert_eq!(shot(false), 0, "no arrow without .chevron()");
}

/// T13: the button that owns an open drop-down menu shows the pressed fill (`line`) and is
/// announced as expanded.
#[test]
fn the_menu_owner_shows_its_open_state() {
    let mut ui = surface(Lang::Ar);
    let root = ui.root();
    let owner = ui
        .add(
            root,
            Button::new(Label::data("ملف")).variant(ButtonVariant::Quiet),
        )
        .unwrap();
    let menu = Menu::new(vec![MenuItem::new(1, Label::data("جديد"))]);
    click(&mut ui, owner);
    ui.open_menu(owner, menu).unwrap();
    let canvas = render(&mut ui).unwrap();
    let rect = ui.rect(owner).unwrap();
    let line = ultar_ui::tokens::THEME
        .palette(ThemeKind::Dark)
        .get(ColorRole::Line);
    let got = canvas
        .pixel((rect.x + 5.0) as i32, (rect.y + 16.0) as i32)
        .unwrap();
    assert_eq!((got.r, got.g, got.b), (line.r, line.g, line.b));
    let node = ui.a11y_node(owner).unwrap();
    assert_eq!(node.state.expanded, Some(true));
    key(&mut ui, Key::Escape);
    let canvas = render(&mut ui).unwrap();
    let got = canvas
        .pixel((rect.x + 5.0) as i32, (rect.y + 16.0) as i32)
        .unwrap();
    assert_ne!(
        (got.r, got.g, got.b),
        (line.r, line.g, line.b),
        "closed: no fill"
    );
}
