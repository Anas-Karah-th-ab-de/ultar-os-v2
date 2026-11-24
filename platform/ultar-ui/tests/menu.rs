//! Menu and context menu: keyboard, pointer, submenu direction in both layouts, placement
//! near the surface edge, limits, accessibility nodes.

mod common;

use common::gallery::{file_menu, gallery, Gallery};
use common::{center, click, key, press, release};
use ultar_ui::event::{ActionKind, Event, Key, PointerButton, Response};
use ultar_ui::layout::{CrossAlign, Flex};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{NodeId, Role, Ui, UiError};
use ultar_ui::widgets::{Button, Label, Menu, MenuItem, MAX_MENU_ITEMS};
use ultar_ui::{Lang, Point, Size};

const SURFACE: Size = Size::new(880.0, 560.0);

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

fn secondary(ui: &mut Ui, pos: Point) -> Response {
    ui.handle(Event::PointerDown {
        pos,
        button: PointerButton::Secondary,
    })
}

// ---------------------------------------------------------------- menu

#[test]
fn a_menu_opens_under_its_button_and_is_operated_by_keyboard() {
    for lang in [Lang::En, Lang::Ar] {
        let (mut ui, g) = setup(lang);
        focus(&mut ui, g.menu_button);
        ui.open_menu(g.menu_button, file_menu()).unwrap();
        assert_eq!(ui.menu_owner(), Some(g.menu_button));
        assert_eq!(
            ui.menu_position(),
            Some((&[][..], Some(0))),
            "keyboard: first command ready"
        );
        let (button, panel) = (ui.rect(g.menu_button).unwrap(), ui.menu_rect(0).unwrap());
        assert!(panel.y >= button.bottom(), "{lang:?}: below the button");
        // The menu is modal for keys: Tab would close it, other keys never reach widgets.
        assert!(key(&mut ui, Key::Char('x')).consumed);
        key(&mut ui, Key::ArrowDown);
        key(&mut ui, Key::ArrowDown);
        assert_eq!(ui.menu_position().unwrap().1, Some(2));
        key(&mut ui, Key::ArrowDown);
        key(&mut ui, Key::ArrowDown);
        assert_eq!(
            ui.menu_position().unwrap().1,
            Some(5),
            "the disabled command is skipped"
        );
        key(&mut ui, Key::ArrowDown);
        assert_eq!(ui.menu_position().unwrap().1, Some(0), "wraps");
        key(&mut ui, Key::End);
        assert_eq!(ui.menu_position().unwrap().1, Some(5));
        key(&mut ui, Key::ArrowUp);
        assert_eq!(ui.menu_position().unwrap().1, Some(3));
        key(&mut ui, Key::Home);
        let chosen = key(&mut ui, Key::Enter);
        assert_eq!(
            acts(&chosen),
            vec![
                (g.menu_button, ActionKind::MenuChosen(1)),
                (g.menu_button, ActionKind::Closed)
            ]
        );
        assert_eq!(ui.menu_owner(), None);
        assert_eq!(
            ui.focused(),
            Some(g.menu_button),
            "focus never left the button"
        );
    }
}

/// A menu button in the middle of the surface: room for a submenu on both sides.
fn centred_menu(lang: Lang) -> (Ui, NodeId) {
    let mut ui = common::ui(lang, ThemeKind::Dark, SURFACE);
    let root = ui.root();
    ui.set_root_flex(Flex::column().cross(CrossAlign::Center));
    let button = ui
        .add(root, Button::new(Label::key("demo.menu.button")))
        .unwrap();
    focus(&mut ui, button);
    ui.open_menu(button, file_menu()).unwrap();
    (ui, button)
}

const BOTH_LAYOUTS: [(Lang, Key, Key); 2] = [
    (Lang::En, Key::ArrowRight, Key::ArrowLeft),
    (Lang::Ar, Key::ArrowLeft, Key::ArrowRight),
];

#[test]
fn submenus_open_towards_the_end_edge_and_close_towards_the_start() {
    for (lang, towards_end, towards_start) in BOTH_LAYOUTS {
        let (mut ui, _) = centred_menu(lang);
        key(&mut ui, towards_end);
        let top_first = Some((&[][..], Some(0)));
        assert_eq!(ui.menu_position(), top_first, "no submenu on this command");
        key(&mut ui, Key::ArrowDown);
        key(&mut ui, Key::ArrowDown);
        key(&mut ui, towards_start);
        let on_share = Some((&[][..], Some(2)));
        assert_eq!(ui.menu_position(), on_share, "nothing to close yet");
        key(&mut ui, towards_end);
        let in_submenu = Some((&[2][..], Some(0)));
        assert_eq!(ui.menu_position(), in_submenu, "{lang:?}: submenu open");
        let (parent, child) = (ui.menu_rect(0).unwrap(), ui.menu_rect(1).unwrap());
        if lang == Lang::Ar {
            assert!(child.center().x < parent.x, "Arabic: to the left");
        } else {
            assert!(child.center().x > parent.right(), "English: to the right");
        }
        key(&mut ui, towards_start);
        assert_eq!(ui.menu_position(), on_share, "back on the parent command");
        assert_eq!(ui.menu_rect(1), None);
    }
}

#[test]
fn enter_escape_and_tab_in_menus_and_submenus() {
    for (lang, _, _) in BOTH_LAYOUTS {
        let (mut ui, button) = centred_menu(lang);
        key(&mut ui, Key::ArrowDown);
        key(&mut ui, Key::ArrowDown);
        // Enter opens the submenu; Escape closes one level, then the menu.
        key(&mut ui, Key::Enter);
        key(&mut ui, Key::ArrowDown);
        assert_eq!(ui.menu_position(), Some((&[2][..], Some(1))));
        let escape = key(&mut ui, Key::Escape);
        assert!(escape.actions.is_empty());
        assert_eq!(ui.menu_position(), Some((&[][..], Some(2))));
        key(&mut ui, Key::Enter);
        let chosen = key(&mut ui, Key::Space);
        let closed = (button, ActionKind::Closed);
        assert_eq!(
            acts(&chosen),
            vec![(button, ActionKind::MenuChosen(31)), closed]
        );
        ui.open_menu(button, file_menu()).unwrap();
        assert_eq!(acts(&key(&mut ui, Key::Escape)), vec![closed]);
        ui.open_menu(button, file_menu()).unwrap();
        assert_eq!(acts(&key(&mut ui, Key::Tab)), vec![closed]);
    }
}

#[test]
fn a_submenu_flips_to_the_other_side_when_there_is_no_room() {
    // 480 points: the top menu (224) fits, a submenu beside it at the end edge does not.
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(480.0, 400.0));
    let root = ui.root();
    ui.set_root_flex(Flex::column().cross(CrossAlign::End));
    let button = ui
        .add(root, Button::new(Label::key("demo.menu.button")))
        .unwrap();
    focus(&mut ui, button);
    ui.open_menu(button, file_menu()).unwrap();
    for k in [Key::ArrowDown, Key::ArrowDown, Key::ArrowRight] {
        key(&mut ui, k);
    }
    let (parent, child) = (ui.menu_rect(0).unwrap(), ui.menu_rect(1).unwrap());
    assert!(
        child.x >= 0.0 && child.right() <= 480.0,
        "inside the surface: {child:?}"
    );
    assert!(
        child.center().x < parent.center().x,
        "flipped to the start side"
    );
    assert!(parent.x >= 0.0 && parent.right() <= 480.0 && parent.bottom() <= 400.0);
}

#[test]
fn pointer_operation_of_a_menu() {
    let (mut ui, g) = setup(Lang::En);
    click(&mut ui, g.menu_button);
    ui.open_menu(g.menu_button, file_menu()).unwrap();
    assert_eq!(
        ui.menu_position(),
        Some((&[][..], None)),
        "opened by pointer: nothing highlighted"
    );
    let panel = ui.menu_rect(0).unwrap();
    let row = |n: f32| Point::new(panel.x + 30.0, panel.y + 4.0 + 32.0 * n + 16.0);
    // Hover highlights; hovering the submenu's parent opens it without a click.
    ui.handle(Event::PointerMove { pos: row(1.0) });
    assert_eq!(ui.menu_position().unwrap().1, Some(1));
    assert_eq!(ui.hovered(), None, "nothing under the menu is hovered");
    ui.handle(Event::PointerMove { pos: row(2.0) });
    assert_eq!(ui.menu_position(), Some((&[2][..], None)));
    let sub = ui.menu_rect(1).unwrap();
    let sub_row = Point::new(sub.x + 30.0, sub.y + 4.0 + 32.0 + 16.0);
    ui.handle(Event::PointerMove { pos: sub_row });
    assert_eq!(ui.menu_position(), Some((&[2][..], Some(1))));
    // Back onto another top-level command: the submenu closes.
    ui.handle(Event::PointerMove { pos: row(0.0) });
    assert_eq!(ui.menu_position(), Some((&[][..], Some(0))));
    // A click on the disabled command does nothing and keeps the menu.
    let disabled = Point::new(panel.x + 30.0, panel.y + 4.0 + 32.0 * 4.0 + 8.0 + 16.0);
    press(&mut ui, disabled);
    assert!(release(&mut ui, disabled).actions.is_empty());
    assert!(ui.menu_owner().is_some());
    // A click on a command chooses it.
    ui.handle(Event::PointerMove { pos: row(2.0) });
    let sub = ui.menu_rect(1).unwrap();
    let first = Point::new(sub.x + 30.0, sub.y + 4.0 + 16.0);
    press(&mut ui, first);
    assert_eq!(
        acts(&release(&mut ui, first)),
        vec![
            (g.menu_button, ActionKind::MenuChosen(31)),
            (g.menu_button, ActionKind::Closed)
        ]
    );
    // A press beside the menu closes it and is not passed to what is under it.
    ui.open_menu(g.menu_button, file_menu()).unwrap();
    let away = center(&mut ui, g.checkbox);
    let down = press(&mut ui, away);
    assert_eq!(acts(&down), vec![(g.menu_button, ActionKind::Closed)]);
    assert!(release(&mut ui, away).actions.is_empty());
    assert!(!ui.is_checked(g.checkbox).unwrap());
    // Closing by the application emits nothing.
    ui.open_menu(g.menu_button, file_menu()).unwrap();
    ui.close_menu();
    assert_eq!(ui.menu_owner(), None);
}

#[test]
fn context_menus_open_by_secondary_click_and_by_the_menu_key() {
    for lang in [Lang::En, Lang::Ar] {
        let (mut ui, g) = setup(lang);
        let pos = center(&mut ui, g.items[2]);
        let opened = secondary(&mut ui, pos);
        assert!(opened.consumed);
        assert_eq!(
            acts(&opened),
            vec![(g.list, ActionKind::Opened)],
            "the list owns the menu"
        );
        let panel = ui.menu_rect(0).unwrap();
        if lang == Lang::Ar {
            assert!(
                (panel.right() - pos.x).abs() < 1.0,
                "Arabic: grows to the left of the pointer"
            );
        } else {
            assert!((panel.x - pos.x).abs() < 1.0);
        }
        assert!(panel.bottom() <= SURFACE.h && panel.y >= 0.0);
        // A second secondary click elsewhere closes it.
        assert_eq!(
            acts(&secondary(&mut ui, Point::new(5.0, 5.0))),
            vec![(g.list, ActionKind::Closed)]
        );
        // Where nothing has a context menu, the click is not used.
        let tabs = center(&mut ui, g.tabs);
        let nothing = secondary(&mut ui, tabs);
        assert!(!nothing.consumed && nothing.actions.is_empty());
        // Menu key: the nearest registered menu above the focused widget, under that node.
        focus(&mut ui, g.items[1]);
        let by_key = key(&mut ui, Key::Menu);
        assert_eq!(acts(&by_key), vec![(g.list, ActionKind::Opened)]);
        assert_eq!(
            ui.menu_position(),
            Some((&[][..], Some(0))),
            "keyboard: first command ready"
        );
        let chosen = key(&mut ui, Key::Enter);
        assert_eq!(chosen.actions[0].kind, ActionKind::MenuChosen(1));
        focus(&mut ui, g.tabs);
        assert!(!key(&mut ui, Key::Menu).consumed);
        // Removing the registration.
        ui.set_context_menu(g.list, None).unwrap();
        assert!(!secondary(&mut ui, pos).consumed);
    }
}

#[test]
fn a_context_menu_near_the_corner_stays_inside_the_surface() {
    let (mut ui, g) = setup(Lang::En);
    for pos in [
        Point::new(715.0, 555.0),
        Point::new(2.0, 555.0),
        Point::new(715.0, 2.0),
        Point::new(-50.0, 9000.0),
    ] {
        ui.open_context_menu(g.list, pos, file_menu()).unwrap();
        let panel = ui.menu_rect(0).unwrap();
        assert!(
            panel.x >= 0.0
                && panel.y >= 0.0
                && panel.right() <= SURFACE.w
                && panel.bottom() <= SURFACE.h,
            "{pos:?}: {panel:?}"
        );
        ui.close_menu();
    }
}

#[test]
fn menus_have_limits_and_accessibility_nodes() {
    let (mut ui, g) = setup(Lang::Ar);
    let item = |id| MenuItem::new(id, Label::key("demo.menu.open"));
    assert_eq!(
        ui.open_menu(g.menu_button, Menu::default()),
        Err(UiError::InvalidMenu)
    );
    let long = Menu::new((0..=MAX_MENU_ITEMS as u32).map(item).collect());
    assert_eq!(
        ui.open_menu(g.menu_button, long.clone()),
        Err(UiError::InvalidMenu)
    );
    assert_eq!(
        ui.set_context_menu(g.list, Some(long)),
        Err(UiError::InvalidMenu)
    );
    assert_eq!(ui.menu_owner(), None);
    focus(&mut ui, g.menu_button);
    ui.open_menu(g.menu_button, file_menu()).unwrap();
    for k in [Key::ArrowDown, Key::ArrowDown, Key::Enter] {
        key(&mut ui, k);
    }
    let tree = ui.a11y_tree();
    let menus: Vec<_> = tree.iter().filter(|n| n.role == Role::Menu).collect();
    let items: Vec<_> = tree.iter().filter(|n| n.role == Role::MenuItem).collect();
    assert_eq!((menus.len(), items.len()), (2, 8));
    assert!(items
        .iter()
        .all(|n| n.id == g.menu_button && n.name.is_some()));
    assert_eq!(items[0].value, Some(Label::data("Ctrl+O")));
    assert_eq!(items[2].state.expanded, Some(true));
    assert_eq!(items[3].state.checked, Some(true));
    assert!(items[4].state.disabled && !items[4].focusable);
    let focused: Vec<_> = items.iter().filter(|n| n.state.focused).collect();
    assert_eq!(focused.len(), 1);
    assert_eq!(focused[0].name, Some(Label::key("demo.menu.share.link")));
    let spoken = items[2].describe(ui.translator());
    assert!(
        spoken.contains("مشاركة") && spoken.contains("أمر") && spoken.contains("مفتوحة"),
        "{spoken}"
    );
    // Opening a dialog or disabling the owner removes the menu.
    ui.set_disabled(g.menu_button, true).unwrap();
    assert_eq!(ui.menu_owner(), None);
}
