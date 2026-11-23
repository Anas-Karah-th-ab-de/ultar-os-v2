//! Dialog and sheet: focus trap, default and cancel action, Escape, stacking; and the
//! tooltip.

mod common;

use common::gallery::{gallery, Gallery};
use common::{center, click, key, press, release, shift_tab};
use ultar_ui::event::{ActionKind, Event, Key, Response};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{NodeId, Role, Ui, UiError};
use ultar_ui::widgets::{Button, Dialog, Label, TextField};
use ultar_ui::{Lang, Point, Size};

const SURFACE: Size = Size::new(720.0, 560.0);

fn setup(lang: Lang) -> (Ui, Gallery) {
    let mut ui = common::ui(lang, ThemeKind::Dark, SURFACE);
    let g = gallery(&mut ui).unwrap();
    ui.layout();
    (ui, g)
}

fn acts(response: &Response) -> Vec<(NodeId, ActionKind)> {
    response.actions.iter().map(|a| (a.node, a.kind)).collect()
}

fn focus(ui: &mut Ui, id: NodeId) {
    for _ in 0..64 {
        if ui.focused() == Some(id) {
            return;
        }
        key(ui, Key::Tab);
    }
    panic!("{id:?} is not reachable by Tab");
}

// ---------------------------------------------------------------- dialog and sheet

#[test]
fn a_closed_dialog_does_not_exist_for_the_user() {
    let (mut ui, g) = setup(Lang::Ar);
    assert!(!ui.is_dialog_open(g.modal.dialog) && ui.open_dialog_node().is_none());
    assert!(!ui.focus_order().contains(&g.modal.delete));
    assert!(ui
        .a11y_tree()
        .iter()
        .all(|n| n.role != Role::Dialog && n.id != g.modal.cancel));
    assert!(
        click(&mut ui, g.modal.delete).actions.is_empty(),
        "not laid out, not hit"
    );
}

#[test]
fn an_open_dialog_traps_focus() {
    let (mut ui, g) = setup(Lang::Ar);
    focus(&mut ui, g.menu_button);
    ui.open_dialog(g.modal.dialog).unwrap();
    assert!(ui.is_dialog_open(g.modal.dialog));
    assert_eq!(ui.open_dialog_node(), Some(g.modal.dialog));
    assert_eq!(
        ui.focused(),
        Some(g.modal.delete),
        "focus starts on the default action"
    );
    assert_eq!(ui.focus_order(), vec![g.modal.cancel, g.modal.delete]);
    // Tab and Shift+Tab cycle inside; arrows do not leave either.
    key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(g.modal.cancel));
    shift_tab(&mut ui);
    assert_eq!(ui.focused(), Some(g.modal.delete));
    shift_tab(&mut ui);
    assert_eq!(ui.focused(), Some(g.modal.cancel));
    for arrow in [
        Key::ArrowUp,
        Key::ArrowDown,
        Key::ArrowLeft,
        Key::ArrowRight,
    ] {
        key(&mut ui, arrow);
        assert!(ui.focus_order().contains(&ui.focused().unwrap()));
    }
    // The screen reader sees only the dialog.
    let tree = ui.a11y_tree();
    assert_eq!(tree[0].role, Role::Dialog);
    assert_eq!(tree[0].name, Some(Label::key("demo.dialog.title")));
    let inside = |mut id: NodeId| loop {
        if id == g.modal.dialog {
            break true;
        }
        match ui.parent(id) {
            Some(parent) => id = parent,
            None => break false,
        }
    };
    assert!(tree.iter().all(|n| inside(n.id)));
}

#[test]
fn an_open_dialog_blocks_what_is_behind() {
    let (mut ui, g) = setup(Lang::Ar);
    ui.open_dialog(g.modal.dialog).unwrap();
    // The panel is centred and inside the surface.
    let panel = ui.rect(g.modal.dialog).unwrap();
    let middle = Point::new(SURFACE.w / 2.0, SURFACE.h / 2.0);
    assert!((panel.center().x - middle.x).abs() < 1.0 && (panel.center().y - middle.y).abs() < 1.0);
    // A click on a widget behind the scrim is swallowed and does not close the dialog.
    let behind = center(&mut ui, g.tabs);
    assert!(!panel.contains(behind));
    let down = press(&mut ui, behind);
    let up = release(&mut ui, behind);
    assert!(down.consumed && up.consumed && down.actions.is_empty() && up.actions.is_empty());
    ui.handle(Event::PointerMove { pos: behind });
    assert_eq!(ui.hovered(), None, "nothing behind is hovered");
    assert!(ui.is_dialog_open(g.modal.dialog));
    let over_list = center(&mut ui, g.scroll);
    let wheel = Event::Scroll {
        pos: over_list,
        lines: 1.0,
    };
    assert!(!ui.handle(wheel).consumed);
    assert_eq!(
        ui.scroll_offset(g.scroll).unwrap(),
        0.0,
        "nothing behind scrolls"
    );
    // Keys do not reach what is behind either: the search field stays empty.
    key(&mut ui, Key::Char('x'));
    assert_eq!(ui.text(g.search).unwrap(), "");
}

#[test]
fn enter_runs_the_default_action_and_escape_the_cancel_action() {
    let (mut ui, g) = setup(Lang::En);
    focus(&mut ui, g.menu_button);
    ui.open_dialog(g.modal.dialog).unwrap();
    // Enter on a focused button presses that button, not the default.
    key(&mut ui, Key::Tab);
    assert_eq!(
        acts(&key(&mut ui, Key::Enter)),
        vec![(g.modal.cancel, ActionKind::Activated)]
    );
    assert!(
        ui.is_dialog_open(g.modal.dialog),
        "the application decides to close"
    );
    // Escape: cancel action, then the dialog closes and says so; focus returns.
    let escape = key(&mut ui, Key::Escape);
    assert!(escape.consumed);
    assert_eq!(
        acts(&escape),
        vec![
            (g.modal.cancel, ActionKind::Activated),
            (g.modal.dialog, ActionKind::Closed)
        ]
    );
    assert!(!ui.is_dialog_open(g.modal.dialog));
    assert_eq!(
        ui.focused(),
        Some(g.menu_button),
        "focus returns to where it was"
    );
    assert!(!key(&mut ui, Key::Escape).consumed, "nothing left to close");
    // Closing by the application: no action, focus returns, closing twice is harmless.
    ui.open_dialog(g.modal.dialog).unwrap();
    ui.open_dialog(g.modal.dialog).unwrap();
    ui.close_dialog(g.modal.dialog).unwrap();
    ui.close_dialog(g.modal.dialog).unwrap();
    assert_eq!(ui.focused(), Some(g.menu_button));
    assert!(ui.open_dialog(g.tabs).is_err() && ui.close_dialog(g.tabs).is_err());
}

#[test]
fn enter_in_a_field_of_a_dialog_submits_and_runs_the_default_action() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let root = ui.root();
    let dialog = ui
        .add(root, Dialog::new(Label::key("demo.dialog.title")))
        .unwrap();
    let field = ui
        .add(dialog, TextField::new(Label::key("demo.device")))
        .unwrap();
    let ok = ui
        .add(dialog, Button::new(Label::key("demo.save")))
        .unwrap();
    let outside = ui.add(root, Button::new(Label::key("demo.apply"))).unwrap();
    ui.set_default_action(dialog, Some(ok)).unwrap();
    // Default and cancel must be buttons of this dialog.
    let wrong = |node| UiError::WrongWidget {
        node,
        expected: "button inside the dialog",
    };
    assert_eq!(
        ui.set_default_action(dialog, Some(outside)),
        Err(wrong(outside))
    );
    assert_eq!(ui.set_cancel_action(dialog, Some(field)), Err(wrong(field)));
    assert!(ui.set_cancel_action(outside, None).is_err());
    ui.set_default_action(dialog, None).unwrap();
    ui.open_dialog(dialog).unwrap();
    assert_eq!(
        ui.focused(),
        Some(field),
        "without default action: the first widget"
    );
    ui.set_default_action(dialog, Some(ok)).unwrap();
    key(&mut ui, Key::Char('x'));
    let enter = key(&mut ui, Key::Enter);
    assert_eq!(
        acts(&enter),
        vec![(field, ActionKind::Submitted), (ok, ActionKind::Activated)]
    );
    // Without cancel action Escape still closes and reports it.
    assert_eq!(
        acts(&key(&mut ui, Key::Escape)),
        vec![(dialog, ActionKind::Closed)]
    );
    // A disabled default action is not run, and focus does not start on it.
    ui.set_disabled(ok, true).unwrap();
    ui.open_dialog(dialog).unwrap();
    assert_eq!(ui.focused(), Some(field));
    assert_eq!(
        acts(&key(&mut ui, Key::Enter)),
        vec![(field, ActionKind::Submitted)]
    );
    // The title is the first child and uses the dialog's label.
    assert_eq!(ui.children(dialog).len(), 3);
    let title = ui.a11y_node(ui.children(dialog)[0]).unwrap();
    assert_eq!(
        (title.role, title.name),
        (Role::Text, Some(Label::key("demo.dialog.title")))
    );
}

#[test]
fn a_sheet_hugs_an_edge_and_a_press_beside_it_dismisses_it() {
    for (lang, size) in [
        (Lang::Ar, Size::new(720.0, 560.0)),
        (Lang::En, Size::new(720.0, 560.0)),
        (Lang::Ar, Size::new(360.0, 640.0)),
    ] {
        let mut ui = common::ui(lang, ThemeKind::Dark, size);
        let g = gallery(&mut ui).unwrap();
        ui.open_dialog(g.modal.sheet).unwrap();
        let panel = ui.rect(g.modal.sheet).unwrap();
        if size.w < 600.0 {
            assert_eq!(
                (panel.x, panel.w, panel.bottom()),
                (0.0, size.w, size.h),
                "compact: from the bottom"
            );
        } else if lang == Lang::Ar {
            assert_eq!(
                (panel.x, panel.h),
                (0.0, size.h),
                "end edge is the left one in Arabic"
            );
        } else {
            assert_eq!((panel.right(), panel.h), (size.w, size.h));
        }
        assert_eq!(ui.focused(), Some(g.modal.close));
        // A press on the sheet itself (not on a widget) does nothing.
        let on_panel = Point::new(panel.center().x, panel.bottom() - 6.0);
        press(&mut ui, on_panel);
        assert!(release(&mut ui, on_panel).actions.is_empty() && ui.is_dialog_open(g.modal.sheet));
        // Beside it: dismissed, with the cancel action.
        let beside = if size.w < 600.0 {
            Point::new(10.0, 10.0)
        } else if lang == Lang::Ar {
            Point::new(size.w - 10.0, 10.0)
        } else {
            Point::new(10.0, 10.0)
        };
        let down = press(&mut ui, beside);
        assert_eq!(
            acts(&down),
            vec![
                (g.modal.close, ActionKind::Activated),
                (g.modal.sheet, ActionKind::Closed)
            ],
            "{lang:?} {size:?}"
        );
        assert!(!ui.is_dialog_open(g.modal.sheet));
        assert!(
            release(&mut ui, beside).actions.is_empty(),
            "the release does not click what was behind"
        );
    }
}

#[test]
fn dialogs_stack_and_the_topmost_has_the_focus() {
    let (mut ui, g) = setup(Lang::En);
    ui.open_dialog(g.modal.sheet).unwrap();
    ui.open_dialog(g.modal.dialog).unwrap();
    assert_eq!(ui.open_dialog_node(), Some(g.modal.dialog));
    assert_eq!(ui.focus_order(), vec![g.modal.cancel, g.modal.delete]);
    assert!(
        click(&mut ui, g.modal.close).actions.is_empty(),
        "the lower one is blocked"
    );
    key(&mut ui, Key::Escape);
    assert_eq!(ui.open_dialog_node(), Some(g.modal.sheet));
    assert_eq!(
        ui.focused(),
        Some(g.modal.close),
        "focus returns into the sheet"
    );
    key(&mut ui, Key::Escape);
    assert_eq!(ui.open_dialog_node(), None);
}

// ---------------------------------------------------------------- tooltip

#[test]
fn a_tooltip_shows_after_a_rest_and_at_once_for_keyboard_focus() {
    let (mut ui, g) = setup(Lang::Ar);
    ui.advance_to(1_000);
    assert_eq!(ui.next_wake(), None);
    let pos = center(&mut ui, g.settings);
    ui.handle(Event::PointerMove { pos });
    assert_eq!(ui.tooltip_shown(), None, "not at once");
    let due = ui.next_wake().expect("the toolkit asks to be woken");
    assert!(due > 1_000);
    ui.advance_to(due - 1);
    assert_eq!(ui.tooltip_shown(), None);
    ui.advance_to(due);
    assert_eq!(ui.tooltip_shown(), Some(g.settings));
    assert_eq!(ui.next_wake(), None, "nothing more to wait for");
    // Moving within the widget keeps it; leaving hides it; pressing hides it.
    ui.handle(Event::PointerMove {
        pos: Point::new(pos.x + 2.0, pos.y),
    });
    assert_eq!(ui.tooltip_shown(), Some(g.settings));
    let tabs = center(&mut ui, g.tabs);
    ui.handle(Event::PointerMove { pos: tabs });
    assert_eq!(ui.tooltip_shown(), None);
    assert_eq!(ui.next_wake(), None, "the tab bar has no tooltip");
    ui.handle(Event::PointerMove { pos });
    ui.advance_to(due + 10_000);
    assert_eq!(ui.tooltip_shown(), Some(g.settings));
    press(&mut ui, pos);
    assert_eq!(ui.tooltip_shown(), None);
    release(&mut ui, pos);
    ui.advance_to(due + 60_000);
    assert_eq!(
        ui.tooltip_shown(),
        None,
        "not again while the pointer stays after a click"
    );
    // Keyboard focus: immediately; Escape hides it without doing anything else.
    ui.handle(Event::PointerLeave);
    key(&mut ui, Key::Tab);
    shift_tab(&mut ui);
    assert_eq!(ui.focused(), Some(g.settings));
    assert_eq!(ui.tooltip_shown(), Some(g.settings));
    let escape = key(&mut ui, Key::Escape);
    assert!(escape.consumed && escape.actions.is_empty());
    assert_eq!(ui.tooltip_shown(), None);
    key(&mut ui, Key::Tab);
    assert_eq!(ui.tooltip_shown(), None);
    // The tooltip is the accessible description, shown or not.
    let node = ui.a11y_node(g.settings).unwrap();
    assert_eq!(node.description, Some(Label::key("demo.tip.settings")));
    assert!(node.describe(ui.translator()).contains("فتح الإعدادات"));
    assert_eq!(
        ui.tooltip(g.settings),
        Some(&Label::key("demo.tip.settings"))
    );
    ui.set_tooltip(g.settings, None).unwrap();
    assert_eq!(ui.tooltip(g.settings), None);
}

#[test]
fn a_tooltip_stays_inside_the_surface_and_disappears_with_its_widget() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(200.0, 60.0));
    let root = ui.root();
    let button = ui.add(root, Button::new(Label::key("demo.save"))).unwrap();
    ui.set_tooltip(button, Some(Label::key("demo.card.text")))
        .unwrap();
    focus(&mut ui, button);
    assert_eq!(ui.tooltip_shown(), Some(button));
    ui.set_disabled(button, true).unwrap();
    assert_eq!(ui.tooltip_shown(), None);
    ui.set_disabled(button, false).unwrap();
    focus(&mut ui, button);
    ui.set_tooltip(button, Some(Label::key("demo.save")))
        .unwrap();
    assert_eq!(
        ui.tooltip_shown(),
        None,
        "a changed text is not shown stale"
    );
}
