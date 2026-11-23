//! Behaviour of the wave-3 base widgets: pointer, keyboard (both directions), actions,
//! state setters, accessibility nodes.

mod common;

use common::gallery::{gallery, Gallery};
use common::{center, click, key, press, release};
use ultar_ui::event::{ActionKind, Event, Key, Modifiers};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{Role, Ui, UiError};
use ultar_ui::widgets::{Label, Widget};
use ultar_ui::{Direction, Lang, Point, Size};

const SURFACE: Size = Size::new(720.0, 1400.0);

fn setup(lang: Lang) -> (Ui, Gallery) {
    let mut ui = common::ui(lang, ThemeKind::Dark, SURFACE);
    let g = gallery(&mut ui).unwrap();
    ui.layout();
    (ui, g)
}

fn kinds(response: &ultar_ui::event::Response) -> Vec<ActionKind> {
    response.actions.iter().map(|a| a.kind).collect()
}

fn focus(ui: &mut Ui, id: ultar_ui::ui::NodeId) {
    for _ in 0..64 {
        if ui.focused() == Some(id) {
            return;
        }
        key(ui, Key::Tab);
    }
    panic!("{id:?} is not reachable by Tab");
}

#[test]
fn checkbox_toggles_by_click_and_space_but_not_enter() {
    let (mut ui, g) = setup(Lang::Ar);
    assert_eq!(
        kinds(&click(&mut ui, g.checkbox)),
        vec![ActionKind::Toggled(true)]
    );
    assert!(ui.is_checked(g.checkbox).unwrap());
    focus(&mut ui, g.checkbox);
    assert_eq!(
        kinds(&key(&mut ui, Key::Space)),
        vec![ActionKind::Toggled(false)]
    );
    let enter = key(&mut ui, Key::Enter);
    assert!(
        enter.actions.is_empty() && !enter.consumed,
        "Enter belongs to the form"
    );
    ui.set_checked(g.checkbox, true).unwrap();
    assert!(ui.is_checked(g.checkbox).unwrap());
    ui.set_disabled(g.checkbox, true).unwrap();
    assert!(click(&mut ui, g.checkbox).actions.is_empty());
    assert_eq!(
        ui.is_checked(g.radio),
        Err(UiError::WrongWidget {
            node: g.radio,
            expected: "checkbox"
        })
    );
    let node = ui.a11y_node(g.checkbox).unwrap();
    assert_eq!(
        (node.role, node.state.checked, node.state.disabled),
        (Role::Checkbox, Some(true), true)
    );
}

#[test]
fn radio_group_is_one_focus_stop_and_arrows_choose() {
    let (mut ui, g) = setup(Lang::En);
    focus(&mut ui, g.radio);
    assert_eq!(
        kinds(&key(&mut ui, Key::ArrowDown)),
        vec![ActionKind::Selected(1)]
    );
    assert_eq!(
        kinds(&key(&mut ui, Key::ArrowDown)),
        vec![ActionKind::Selected(2)]
    );
    assert_eq!(
        kinds(&key(&mut ui, Key::ArrowDown)),
        vec![ActionKind::Selected(0)],
        "wraps"
    );
    assert_eq!(
        kinds(&key(&mut ui, Key::ArrowUp)),
        vec![ActionKind::Selected(2)]
    );
    assert_eq!(
        kinds(&key(&mut ui, Key::Home)),
        vec![ActionKind::Selected(0)]
    );
    assert_eq!(
        kinds(&key(&mut ui, Key::End)),
        vec![ActionKind::Selected(2)]
    );
    assert!(key(&mut ui, Key::End).actions.is_empty(), "already chosen");
    assert_eq!(ui.focused(), Some(g.radio), "arrows never leave the group");
    // Pointer: the row under the pointer is chosen; leaving the row before release is not.
    let rect = ui.rect(g.radio).unwrap();
    let row = rect.h / 3.0;
    let second = Point::new(rect.x + 10.0, rect.y + 1.5 * row);
    press(&mut ui, second);
    assert_eq!(
        kinds(&release(&mut ui, second)),
        vec![ActionKind::Selected(1)]
    );
    press(&mut ui, second);
    let first = Point::new(rect.x + 10.0, rect.y + 0.5 * row);
    assert!(
        release(&mut ui, first).actions.is_empty(),
        "released on another option"
    );
    assert_eq!(ui.radio_selected(g.radio).unwrap(), Some(1));
    assert_eq!(ui.selected(g.radio).unwrap(), 1);
    ui.set_selected(g.radio, 2).unwrap();
    assert_eq!(
        ui.set_selected(g.radio, 3),
        Err(UiError::OptionOutOfRange {
            node: g.radio,
            index: 3
        })
    );
    let node = ui.a11y_node(g.radio).unwrap();
    assert_eq!(node.role, Role::RadioGroup);
    assert_eq!(node.value, Some(Label::key("demo.radio.auto")));
}

#[test]
fn radio_arrows_follow_the_direction() {
    for (lang, towards_end) in [(Lang::En, Key::ArrowRight), (Lang::Ar, Key::ArrowLeft)] {
        let (mut ui, g) = setup(lang);
        focus(&mut ui, g.radio);
        assert_eq!(
            kinds(&key(&mut ui, towards_end)),
            vec![ActionKind::Selected(1)],
            "{lang:?}"
        );
    }
}

#[test]
fn tabs_follow_the_direction_and_stop_at_the_ends() {
    for (lang, next, previous) in [
        (Lang::En, Key::ArrowRight, Key::ArrowLeft),
        (Lang::Ar, Key::ArrowLeft, Key::ArrowRight),
    ] {
        let (mut ui, g) = setup(lang);
        focus(&mut ui, g.tabs);
        assert!(
            key(&mut ui, previous).actions.is_empty(),
            "{lang:?}: no tab before the first"
        );
        assert_eq!(kinds(&key(&mut ui, next)), vec![ActionKind::Selected(1)]);
        assert_eq!(
            kinds(&key(&mut ui, Key::End)),
            vec![ActionKind::Selected(2)]
        );
        assert!(
            key(&mut ui, next).actions.is_empty(),
            "no tab after the last"
        );
        assert_eq!(
            kinds(&key(&mut ui, Key::Home)),
            vec![ActionKind::Selected(0)]
        );
        // The first tab is at the start edge: right in Arabic, left in English.
        let rect = ui.rect(g.tabs).unwrap();
        let near_left = Point::new(rect.x + 4.0, rect.center().y);
        press(&mut ui, near_left);
        let chosen = kinds(&release(&mut ui, near_left));
        let expected = if lang == Lang::Ar {
            vec![ActionKind::Selected(2)]
        } else {
            vec![]
        };
        assert_eq!(chosen, expected, "{lang:?}");
        // Up and Down are not the tab bar's: they move focus.
        focus(&mut ui, g.tabs);
        assert!(key(&mut ui, Key::ArrowDown).consumed);
        assert_ne!(ui.focused(), Some(g.tabs));
    }
    let (mut ui, g) = setup(Lang::Ar);
    ui.set_selected(g.tabs, 1).unwrap();
    assert_eq!(ui.selected(g.tabs).unwrap(), 1);
    let node = ui.a11y_node(g.tabs).unwrap();
    assert_eq!(
        (node.role, node.value),
        (Role::TabList, Some(Label::key("demo.tabs.network")))
    );
    ui.set_direction(Direction::Ltr);
    focus(&mut ui, g.tabs);
    assert_eq!(
        kinds(&key(&mut ui, Key::ArrowRight)),
        vec![ActionKind::Selected(2)]
    );
}

#[test]
fn list_items_activate_select_and_keep_their_trailing_control() {
    let (mut ui, g) = setup(Lang::Ar);
    assert_eq!(ui.selected_item(g.list).unwrap(), Some(0));
    let response = click(&mut ui, g.items[2]);
    assert_eq!(response.actions.len(), 2);
    assert_eq!(
        (response.actions[0].node, response.actions[0].kind),
        (g.items[2], ActionKind::Activated)
    );
    assert_eq!(
        (response.actions[1].node, response.actions[1].kind),
        (g.list, ActionKind::Selected(2))
    );
    assert_eq!(ui.selected_item(g.list).unwrap(), Some(2));
    assert!(
        !ui.is_item_selected(g.items[0]).unwrap(),
        "single selection"
    );
    // Activating the selected item again reports only the activation.
    assert_eq!(
        kinds(&click(&mut ui, g.items[2])),
        vec![ActionKind::Activated]
    );
    // The trailing switch is its own target and does not activate the item.
    let toggled = click(&mut ui, g.item_switch);
    assert_eq!(kinds(&toggled), vec![ActionKind::Toggled(false)]);
    assert_eq!(ui.selected_item(g.list).unwrap(), Some(2));
    // Keyboard: Enter and Space activate; arrows walk item, control, item.
    focus(&mut ui, g.items[0]);
    assert_eq!(
        kinds(&key(&mut ui, Key::Enter)),
        vec![ActionKind::Activated, ActionKind::Selected(0)]
    );
    key(&mut ui, Key::ArrowDown);
    assert_eq!(ui.focused(), Some(g.items[1]));
    key(&mut ui, Key::ArrowDown);
    assert_eq!(ui.focused(), Some(g.item_switch));
    key(&mut ui, Key::ArrowDown);
    assert_eq!(ui.focused(), Some(g.items[2]));
    assert_eq!(
        kinds(&key(&mut ui, Key::Space)),
        vec![ActionKind::Activated, ActionKind::Selected(2)]
    );
    ui.set_item_selected(g.items[1], true).unwrap();
    assert_eq!(ui.selected_item(g.list).unwrap(), Some(1));
    ui.set_item_selected(g.items[1], false).unwrap();
    assert_eq!(ui.selected_item(g.list).unwrap(), None);
    assert!(ui.selected_item(g.items[0]).is_err() && ui.set_item_selected(g.list, true).is_err());
    let item = ui.a11y_node(g.items[0]).unwrap();
    assert_eq!(
        (item.role, item.state.selected, item.focusable),
        (Role::ListItem, Some(false), true)
    );
    assert_eq!(item.value, Some(Label::key("demo.list.home.sub")));
    let list = ui.a11y_node(g.list).unwrap();
    assert_eq!(
        (list.role, list.name),
        (Role::List, Some(Label::key("demo.list")))
    );
}

#[test]
fn a_disabled_list_disables_its_items() {
    let (mut ui, g) = setup(Lang::En);
    ui.set_disabled(g.list, true).unwrap();
    assert!(click(&mut ui, g.items[2]).actions.is_empty());
    assert!(!ui.focus_order().contains(&g.items[0]));
    assert!(!ui.focus_order().contains(&g.item_switch));
}

#[test]
fn search_field_clears_by_button_and_escape_and_submits() {
    for lang in [Lang::Ar, Lang::En] {
        let (mut ui, g) = setup(lang);
        focus(&mut ui, g.search);
        for ch in "wifi".chars() {
            assert_eq!(
                kinds(&key(&mut ui, Key::Char(ch))),
                vec![ActionKind::TextChanged]
            );
        }
        assert_eq!(
            kinds(&key(&mut ui, Key::Enter)),
            vec![ActionKind::Submitted]
        );
        assert_eq!(
            kinds(&key(&mut ui, Key::Escape)),
            vec![ActionKind::TextChanged]
        );
        assert_eq!(ui.text(g.search).unwrap(), "");
        let again = key(&mut ui, Key::Escape);
        assert!(
            again.actions.is_empty() && !again.consumed,
            "nothing left to clear"
        );
        // The clear button is at the end edge: left in Arabic, right in English.
        ui.set_text(g.search, "bluetooth").unwrap();
        let rect = ui.rect(g.search).unwrap();
        let at_end = if lang == Lang::Ar {
            rect.x + 14.0
        } else {
            rect.right() - 14.0
        };
        let at_start = if lang == Lang::Ar {
            rect.right() - 14.0
        } else {
            rect.x + 14.0
        };
        let pos = Point::new(at_start, rect.center().y);
        press(&mut ui, pos);
        assert!(
            release(&mut ui, pos).actions.is_empty(),
            "{lang:?}: the search icon does nothing"
        );
        let pos = Point::new(at_end, rect.center().y);
        press(&mut ui, pos);
        assert_eq!(
            kinds(&release(&mut ui, pos)),
            vec![ActionKind::TextChanged],
            "{lang:?}"
        );
        assert_eq!(ui.text(g.search).unwrap(), "");
        assert_eq!(ui.focused(), Some(g.search), "focus stays in the field");
        let node = ui.a11y_node(g.search).unwrap();
        assert_eq!(
            (node.role, node.name),
            (Role::SearchField, Some(Label::key("demo.search")))
        );
    }
}

#[test]
fn progress_values_and_animation() {
    let (mut ui, g) = setup(Lang::Ar);
    assert_eq!(ui.progress(g.progress).unwrap(), Some(0.4));
    assert!(!ui.animating(), "a determinate bar stands still");
    let node = ui.a11y_node(g.progress).unwrap();
    assert_eq!(
        (node.role, node.value, node.state.busy),
        (Role::Progress, Some(Label::data("40%")), false)
    );
    ui.set_progress(g.progress, Some(7.0)).unwrap();
    assert_eq!(ui.progress(g.progress).unwrap(), Some(1.0));
    ui.set_progress(g.progress, Some(f32::NAN)).unwrap();
    assert_eq!(ui.progress(g.progress).unwrap(), Some(0.0));
    ui.set_progress(g.progress, None).unwrap();
    assert!(ui.animating(), "an indeterminate bar moves");
    let node = ui.a11y_node(g.progress).unwrap();
    assert_eq!((node.value, node.state.busy), (None, true));
    ui.set_reduce_motion(true);
    assert!(!ui.animating());
    assert!(ui.set_progress(g.checkbox, None).is_err());
    assert!(!ui.focus_order().contains(&g.progress), "not operable");
}

#[test]
fn status_widgets_announce_their_tone_in_words() {
    let (mut ui, g) = setup(Lang::Ar);
    let bar = ui.a11y_node(g.status.bar).unwrap();
    assert_eq!(bar.role, Role::Alert);
    assert_eq!(bar.name, Some(Label::key("demo.bar.warning")));
    assert_eq!(bar.value, Some(Label::key("demo.bar.warning.msg")));
    assert_eq!(bar.description, Some(Label::key("ui.tone.warning")));
    let spoken = bar.describe(ui.translator());
    assert!(
        spoken.contains("تحذير") && spoken.contains("تنبيه"),
        "{spoken}"
    );
    // The action of the bar is a normal button inside it.
    assert_eq!(
        kinds(&click(&mut ui, g.status.bar_action)),
        vec![ActionKind::Activated]
    );
    let empty = ui.a11y_node(g.status.empty).unwrap();
    assert_eq!(
        (empty.role, empty.name),
        (Role::Status, Some(Label::key("demo.empty.title")))
    );
    assert_eq!(
        kinds(&click(&mut ui, g.status.empty_action)),
        vec![ActionKind::Activated]
    );
    let tree = ui.a11y_tree();
    let badge = tree
        .iter()
        .find(|n| n.role == Role::Badge && n.description.is_some())
        .unwrap();
    assert!(!badge.focusable);
    assert!(badge.describe(ui.translator()).contains("شارة"));
    // Labels can be replaced.
    ui.set_label(g.status.bar, Label::key("demo.bar.danger"))
        .unwrap();
    ui.set_label(g.checkbox, Label::key("demo.night")).unwrap();
    ui.set_label(g.items[0], Label::key("demo.night")).unwrap();
    ui.set_label(g.status.empty, Label::key("demo.night"))
        .unwrap();
    assert!(ui.set_label(g.tabs, Label::key("demo.night")).is_err());
}

#[test]
fn hosts_take_exactly_one_child_and_leaves_none() {
    let (mut ui, g) = setup(Lang::En);
    let extra = || ultar_ui::widgets::Button::new(Label::key("demo.save"));
    for host in [g.status.bar, g.status.empty, g.items[1]] {
        assert_eq!(
            ui.add(host, extra()),
            Err(UiError::NotAContainer(host)),
            "{:?}",
            ui.widget(host)
        );
    }
    for leaf in [g.checkbox, g.radio, g.tabs, g.progress, g.search] {
        assert_eq!(ui.add(leaf, extra()), Err(UiError::NotAContainer(leaf)));
    }
    assert!(
        ui.add(g.items[0], extra()).is_ok(),
        "an item without control takes one"
    );
    assert!(ui.add(g.list, extra()).is_ok() && ui.add(g.scroll, extra()).is_ok());
    assert!(matches!(ui.widget(g.search), Some(Widget::TextField(f)) if f.is_search()));
}

#[test]
fn targets_reach_the_minimum_size_in_both_input_modes() {
    for mode in [InputMode::Pointer, InputMode::Touch] {
        let (mut ui, g) = setup(Lang::Ar);
        ui.set_input_mode(mode);
        ui.layout();
        let min = mode.min_target();
        for id in [g.checkbox, g.tabs, g.search, g.items[0], g.items[2]] {
            let rect = ui.rect(id).unwrap();
            assert!(
                rect.h >= min,
                "{mode:?} {:?}: {} high",
                ui.widget(id),
                rect.h
            );
        }
        let radio = ui.rect(g.radio).unwrap();
        assert!(radio.h >= 3.0 * min, "each radio row is a target");
    }
}

#[test]
fn modified_keys_are_left_to_shortcuts() {
    let (mut ui, g) = setup(Lang::En);
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    for (id, k) in [
        (g.checkbox, Key::Space),
        (g.radio, Key::ArrowDown),
        (g.tabs, Key::End),
        (g.items[0], Key::Enter),
    ] {
        focus(&mut ui, id);
        let response = ui.handle(Event::Key {
            key: k,
            modifiers: ctrl,
        });
        assert!(response.actions.is_empty(), "{:?}", ui.widget(id));
    }
    let _ = center(&mut ui, g.tabs);
}
