//! Keyboard operation: every widget is reachable and operable without a pointer, and focus
//! traversal follows the logical order in both directions.

mod common;

use common::{display_settings, key, shift_tab};
use ultar_ui::event::{Action, ActionKind, Event, Key, Modifiers};
use ultar_ui::icon::IconId;
use ultar_ui::layout::Flex;
use ultar_ui::tokens::{Space, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{Button, IconButton, Label, Slider, Switch, TextField};
use ultar_ui::{Direction, Lang, Size};

const SURFACE: Size = Size::new(720.0, 760.0);

fn kinds(actions: &[Action]) -> Vec<ActionKind> {
    actions.iter().map(|a| a.kind).collect()
}

/// Presses Tab until `target` has focus; fails if it is never reached.
fn tab_to(ui: &mut Ui, target: NodeId) {
    for _ in 0..200 {
        if ui.focused() == Some(target) {
            return;
        }
        assert!(key(ui, Key::Tab).consumed);
    }
    panic!("{target:?} cannot be reached with Tab");
}

#[test]
fn every_interactive_widget_is_reachable_by_tab_in_logical_order() {
    for lang in common::LANGS {
        let mut ui = common::ui(lang, ThemeKind::Dark, SURFACE);
        let s = display_settings(&mut ui).unwrap();
        let expected = vec![
            s.primary,
            s.resolution,
            s.scale,
            s.refresh,
            s.orientation,
            s.night,
            s.brightness,
            s.name,
            s.detect,
            s.revert,
            s.apply,
        ];
        assert_eq!(
            ui.focus_order(),
            expected,
            "{lang:?}: logical order is the same in both directions"
        );
        let mut visited = Vec::new();
        for _ in 0..expected.len() {
            assert!(key(&mut ui, Key::Tab).consumed);
            let focused = ui.focused().unwrap();
            assert!(ui.visual(focused).focused, "keyboard focus shows the ring");
            assert!(ui.a11y_node(focused).unwrap().state.focused);
            visited.push(focused);
        }
        assert_eq!(visited, expected);
        key(&mut ui, Key::Tab);
        assert_eq!(
            ui.focused(),
            Some(s.primary),
            "Tab wraps to the first widget"
        );
        shift_tab(&mut ui);
        assert_eq!(ui.focused(), Some(s.apply), "Shift+Tab wraps backwards");
        shift_tab(&mut ui);
        assert_eq!(ui.focused(), Some(s.revert));
    }
}

#[test]
fn horizontal_arrows_follow_the_direction() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(400.0, 100.0));
    ui.set_root_flex(Flex::row().gap(Space::S8));
    let root = ui.root();
    let ids: Vec<NodeId> = ["demo.save", "demo.revert", "demo.delete"]
        .iter()
        .map(|k| ui.add(root, Button::new(Label::key(*k))).unwrap())
        .collect();
    ui.layout();
    // Logical order is start to end; in RTL that is right to left on screen.
    let xs: Vec<f32> = ids.iter().map(|id| ui.rect(*id).unwrap().x).collect();
    assert!(xs[0] > xs[1] && xs[1] > xs[2], "{xs:?}");
    key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(ids[0]));
    key(&mut ui, Key::ArrowLeft);
    assert_eq!(ui.focused(), Some(ids[1]), "left is towards the end in RTL");
    key(&mut ui, Key::ArrowLeft);
    assert_eq!(ui.focused(), Some(ids[2]));
    assert!(
        !key(&mut ui, Key::ArrowLeft).consumed,
        "arrows stop at the last widget"
    );
    key(&mut ui, Key::ArrowRight);
    assert_eq!(ui.focused(), Some(ids[1]));
    key(&mut ui, Key::ArrowUp);
    assert_eq!(ui.focused(), Some(ids[0]), "up is always towards the start");
    key(&mut ui, Key::ArrowDown);
    assert_eq!(ui.focused(), Some(ids[1]));
    // The same tree left-to-right: the same keys move the other way.
    ui.set_direction(Direction::Ltr);
    ui.layout();
    let xs: Vec<f32> = ids.iter().map(|id| ui.rect(*id).unwrap().x).collect();
    assert!(xs[0] < xs[1] && xs[1] < xs[2], "{xs:?}");
    key(&mut ui, Key::ArrowRight);
    assert_eq!(ui.focused(), Some(ids[2]));
    key(&mut ui, Key::ArrowLeft);
    assert_eq!(ui.focused(), Some(ids[1]));
}

#[test]
fn buttons_and_switches_activate_with_enter_and_space() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let root = ui.root();
    let button = ui.add(root, Button::new(Label::key("demo.save"))).unwrap();
    let icon = ui
        .add(
            root,
            IconButton::new(IconId::Close, Label::key("demo.delete")),
        )
        .unwrap();
    let switch = ui
        .add(root, Switch::new(Label::key("demo.night"), false))
        .unwrap();
    tab_to(&mut ui, button);
    for k in [Key::Enter, Key::Space] {
        let response = key(&mut ui, k);
        assert!(response.consumed);
        assert_eq!(
            response.actions,
            vec![Action {
                node: button,
                kind: ActionKind::Activated
            }]
        );
    }
    tab_to(&mut ui, icon);
    assert_eq!(
        kinds(&key(&mut ui, Key::Enter).actions),
        vec![ActionKind::Activated]
    );
    tab_to(&mut ui, switch);
    assert_eq!(
        kinds(&key(&mut ui, Key::Space).actions),
        vec![ActionKind::Toggled(true)]
    );
    assert!(ui.is_on(switch).unwrap());
    assert_eq!(
        kinds(&key(&mut ui, Key::Enter).actions),
        vec![ActionKind::Toggled(false)]
    );
    assert!(!ui.is_on(switch).unwrap());
    // Shortcut modifiers are not an activation; unrelated keys are not consumed.
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let response = ui.handle(Event::Key {
        key: Key::Enter,
        modifiers: ctrl,
    });
    assert!(!response.consumed && response.actions.is_empty());
    assert!(!key(&mut ui, Key::Char('x')).consumed);
    assert!(!key(&mut ui, Key::Escape).consumed);
}

#[test]
fn disabled_widgets_are_skipped_and_loading_widgets_do_not_act() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, SURFACE);
    let root = ui.root();
    let first = ui.add(root, Button::new(Label::key("demo.save"))).unwrap();
    let disabled = ui
        .add(root, Button::new(Label::key("demo.delete")))
        .unwrap();
    let loading = ui
        .add(root, Switch::new(Label::key("demo.night"), false))
        .unwrap();
    ui.set_disabled(disabled, true).unwrap();
    ui.set_loading(loading, true).unwrap();
    assert_eq!(ui.focus_order(), vec![first, loading]);
    tab_to(&mut ui, loading);
    let response = key(&mut ui, Key::Space);
    assert!(
        response.consumed && response.actions.is_empty(),
        "a loading switch swallows the key but does nothing"
    );
    assert!(!ui.is_on(loading).unwrap());
    assert!(ui.a11y_node(loading).unwrap().state.busy);
    // Disabling the focused widget drops the focus.
    ui.set_disabled(loading, true).unwrap();
    assert_eq!(ui.focused(), None);
    key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(first));
    assert!(
        !key(&mut ui, Key::Tab).consumed,
        "with one widget Tab has nowhere to go"
    );
    // An empty tree has nothing to focus.
    ui.clear();
    assert!(!key(&mut ui, Key::Tab).consumed && !key(&mut ui, Key::ArrowDown).consumed);
}

#[test]
fn select_is_operated_by_keyboard() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let s = display_settings(&mut ui).unwrap();
    tab_to(&mut ui, s.orientation);
    assert_eq!(
        kinds(&key(&mut ui, Key::Enter).actions),
        vec![ActionKind::Opened]
    );
    assert!(ui.is_open(s.orientation));
    assert_eq!(
        ui.a11y_node(s.orientation).unwrap().state.expanded,
        Some(true)
    );
    for step in [Key::ArrowDown, Key::ArrowDown, Key::ArrowDown, Key::ArrowUp] {
        key(&mut ui, step);
    }
    let response = key(&mut ui, Key::Enter);
    assert_eq!(
        kinds(&response.actions),
        vec![ActionKind::Selected(1), ActionKind::Closed]
    );
    assert_eq!(ui.selected(s.orientation).unwrap(), 1);
    assert!(!ui.is_open(s.orientation));
    assert_eq!(ui.focused(), Some(s.orientation), "focus stays");
    // Home/End, choosing the current option again, and Escape.
    key(&mut ui, Key::Space);
    key(&mut ui, Key::End);
    assert_eq!(
        kinds(&key(&mut ui, Key::Space).actions),
        vec![ActionKind::Selected(2), ActionKind::Closed]
    );
    key(&mut ui, Key::Enter);
    key(&mut ui, Key::Home);
    assert_eq!(
        kinds(&key(&mut ui, Key::Escape).actions),
        vec![ActionKind::Closed]
    );
    assert_eq!(
        ui.selected(s.orientation).unwrap(),
        2,
        "Escape keeps the old choice"
    );
    key(&mut ui, Key::Enter);
    assert_eq!(
        kinds(&key(&mut ui, Key::Enter).actions),
        vec![ActionKind::Closed],
        "same option: no Selected"
    );
    // Tab closes the list and moves on; a letter jumps to an option (type-ahead) and keys
    // the list does not know are left alone.
    key(&mut ui, Key::Enter);
    assert!(key(&mut ui, Key::Char('a')).consumed);
    assert!(!key(&mut ui, Key::Backspace).consumed);
    let response = key(&mut ui, Key::Tab);
    assert_eq!(kinds(&response.actions), vec![ActionKind::Closed]);
    assert_eq!(ui.focused(), Some(s.night));
}

#[test]
fn slider_is_operated_by_keyboard_in_both_directions() {
    for (lang, towards_end) in [(Lang::En, Key::ArrowRight), (Lang::Ar, Key::ArrowLeft)] {
        let mut ui = common::ui(lang, ThemeKind::Dark, SURFACE);
        let root = ui.root();
        let slider = ui
            .add(
                root,
                Slider::new(Label::key("demo.brightness"), 0.0, 100.0, 10.0).value(50.0),
            )
            .unwrap();
        tab_to(&mut ui, slider);
        assert_eq!(
            kinds(&key(&mut ui, towards_end).actions),
            vec![ActionKind::ValueChanged(60.0)],
            "{lang:?}"
        );
        let back = if towards_end == Key::ArrowRight {
            Key::ArrowLeft
        } else {
            Key::ArrowRight
        };
        key(&mut ui, back);
        key(&mut ui, back);
        assert_eq!(ui.value(slider).unwrap(), 40.0);
        key(&mut ui, Key::ArrowUp);
        assert_eq!(ui.value(slider).unwrap(), 50.0);
        key(&mut ui, Key::ArrowDown);
        assert_eq!(ui.value(slider).unwrap(), 40.0);
        assert_eq!(
            kinds(&key(&mut ui, Key::End).actions),
            vec![ActionKind::ValueChanged(100.0)]
        );
        let at_max = key(&mut ui, towards_end);
        assert!(
            at_max.consumed && at_max.actions.is_empty(),
            "no action when the value cannot change"
        );
        assert_eq!(
            kinds(&key(&mut ui, Key::Home).actions),
            vec![ActionKind::ValueChanged(0.0)]
        );
        assert_eq!(
            ui.focused(),
            Some(slider),
            "arrows operate the slider instead of moving focus"
        );
    }
}

fn field_and_button(ui: &mut Ui) -> (NodeId, NodeId) {
    let root = ui.root();
    let field = TextField::new(Label::key("demo.device")).max_len(16);
    let field = ui.add(root, field).unwrap();
    let after = ui.add(root, Button::new(Label::key("demo.save"))).unwrap();
    tab_to(ui, field);
    (field, after)
}

#[test]
fn text_field_typing_and_editing_by_keyboard() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let (field, _) = field_and_button(&mut ui);
    let changed = vec![ActionKind::TextChanged];
    for ch in "سلا".chars() {
        assert_eq!(kinds(&key(&mut ui, Key::Char(ch)).actions), changed);
    }
    assert_eq!(kinds(&key(&mut ui, Key::Space).actions), changed);
    key(&mut ui, Key::Char('m'));
    assert_eq!(ui.text(field).unwrap(), "سلا m");
    assert_eq!(kinds(&key(&mut ui, Key::Backspace).actions), changed);
    // In RTL the right arrow moves towards the start of the text.
    key(&mut ui, Key::ArrowRight);
    key(&mut ui, Key::ArrowRight);
    key(&mut ui, Key::Char('ـ'));
    assert_eq!(ui.text(field).unwrap(), "سلـا ");
    key(&mut ui, Key::ArrowLeft);
    assert_eq!(kinds(&key(&mut ui, Key::Delete).actions), changed);
    assert_eq!(ui.text(field).unwrap(), "سلـا");
    key(&mut ui, Key::Home);
    assert!(
        key(&mut ui, Key::Backspace).actions.is_empty(),
        "nothing before the caret"
    );
    key(&mut ui, Key::Char('ا'));
    key(&mut ui, Key::End);
    assert!(key(&mut ui, Key::Delete).actions.is_empty());
    assert_eq!(ui.text(field).unwrap(), "اسلـا");
    let value = ui.a11y_node(field).unwrap().value;
    assert_eq!(value, Some(Label::data(ui.text(field).unwrap())));
}

#[test]
fn text_field_limits_shortcuts_submit_and_leaving() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let (field, after) = field_and_button(&mut ui);
    // The length limit, control characters and shortcuts.
    for _ in 0..20 {
        key(&mut ui, Key::Char('x'));
    }
    assert_eq!(ui.text(field).unwrap().len(), 16);
    assert!(key(&mut ui, Key::Char('\u{8}')).actions.is_empty());
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let shortcut = Event::Key {
        key: Key::Char('a'),
        modifiers: ctrl,
    };
    assert!(!ui.handle(shortcut).consumed);
    assert_eq!(
        kinds(&key(&mut ui, Key::Enter).actions),
        vec![ActionKind::Submitted]
    );
    // Up/Down leave the field; Tab too.
    key(&mut ui, Key::ArrowDown);
    assert_eq!(ui.focused(), Some(after));
    key(&mut ui, Key::ArrowUp);
    assert_eq!(ui.focused(), Some(field));
    key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(after));
}

#[test]
fn the_whole_display_screen_can_be_operated_without_a_pointer() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let s = display_settings(&mut ui).unwrap();
    let mut log = Vec::new();
    tab_to(&mut ui, s.primary);
    log.extend(key(&mut ui, Key::Space).actions);
    tab_to(&mut ui, s.resolution);
    log.extend(key(&mut ui, Key::Enter).actions);
    log.extend(key(&mut ui, Key::ArrowDown).actions);
    log.extend(key(&mut ui, Key::Enter).actions);
    tab_to(&mut ui, s.brightness);
    log.extend(key(&mut ui, Key::Home).actions);
    tab_to(&mut ui, s.name);
    log.extend(key(&mut ui, Key::Char('A')).actions);
    tab_to(&mut ui, s.apply);
    log.extend(key(&mut ui, Key::Enter).actions);
    let expected = vec![
        Action {
            node: s.primary,
            kind: ActionKind::Toggled(false),
        },
        Action {
            node: s.resolution,
            kind: ActionKind::Opened,
        },
        Action {
            node: s.resolution,
            kind: ActionKind::Selected(1),
        },
        Action {
            node: s.resolution,
            kind: ActionKind::Closed,
        },
        Action {
            node: s.brightness,
            kind: ActionKind::ValueChanged(0.0),
        },
        Action {
            node: s.name,
            kind: ActionKind::TextChanged,
        },
        Action {
            node: s.apply,
            kind: ActionKind::Activated,
        },
    ];
    assert_eq!(log, expected);
}
