//! Review round 3, "Prüfer Zugang": accessibility tree (roles, names, states, relations),
//! minimum target sizes, spoken sentences in both languages.
//! Target path: platform/ultar-ui/tests/r3_access.rs

mod common;

use common::gallery::{file_menu, gallery, Gallery};
use common::{center, click, key};
use ultar_ui::event::{Event, Key};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{A11yNode, NodeId, Role, Ui};
use ultar_ui::widgets::{InfoBar, Label, SearchField, Tabs, Tone};
use ultar_ui::{Lang, Point, Size};

const SURFACE: Size = Size::new(720.0, 560.0);

fn setup(lang: Lang) -> (Ui, Gallery) {
    let mut ui = common::ui(lang, ThemeKind::Dark, SURFACE);
    let g = gallery(&mut ui).unwrap();
    ui.layout();
    (ui, g)
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

fn named(tree: &[A11yNode], key: &str) -> Vec<A11yNode> {
    let label = Some(Label::key(key));
    tree.iter().filter(|n| n.name == label).cloned().collect()
}

// ------------------------------------------------------------ what is missing in the tree

/// Every option of a radio group is a node of its own with a checked state: a screen
/// reader must be able to say "Light, 2 of 3, not chosen", not only the chosen value.
#[test]
fn z01_radio_options_are_nodes_with_a_checked_state() {
    let (mut ui, _g) = setup(Lang::Ar);
    let tree = ui.a11y_tree();
    let mut checked = Vec::new();
    for option in ["demo.radio.dark", "demo.radio.light", "demo.radio.auto"] {
        let nodes = named(&tree, option);
        assert_eq!(nodes.len(), 1, "{option}: no node of its own");
        checked.push(nodes[0].state.checked);
    }
    assert_eq!(checked, vec![Some(true), Some(false), Some(false)]);
}

/// Every tab is a node with a selected state.
#[test]
fn z02_tabs_are_nodes_with_a_selected_state() {
    let (mut ui, _g) = setup(Lang::En);
    let tree = ui.a11y_tree();
    let mut selected = Vec::new();
    for tab in ["demo.tabs.general", "demo.tabs.network", "demo.tabs.about"] {
        let nodes = named(&tree, tab);
        assert_eq!(nodes.len(), 1, "{tab}: no node of its own");
        selected.push(nodes[0].state.selected);
    }
    assert_eq!(selected, vec![Some(true), Some(false), Some(false)]);
}

/// While the list of a select is open its options are in the tree and the highlighted one
/// is the focused node (as the open menu already does).
#[test]
fn z03_the_options_of_an_open_select_are_in_the_tree() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let root = ui.root();
    let select = ui.add(root, common::orientation_select()).unwrap();
    focus(&mut ui, select);
    key(&mut ui, Key::Enter);
    key(&mut ui, Key::ArrowDown);
    let tree = ui.a11y_tree();
    let portrait = named(&tree, "demo.orientation.portrait");
    assert_eq!(portrait.len(), 1, "the highlighted option is not announced");
    assert!(portrait[0].state.focused);
    assert_eq!(named(&tree, "demo.orientation.flipped").len(), 1);
}

/// An informative or successful message is a polite status; only warnings and errors
/// interrupt (boards: `role="status"` for info/ok, `role="alert"` for warning/danger).
#[test]
fn z04_only_warnings_and_errors_are_alerts() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, SURFACE);
    let root = ui.root();
    let mut roles = Vec::new();
    for tone in [Tone::Info, Tone::Success, Tone::Warning, Tone::Danger] {
        let bar = ui
            .add(root, InfoBar::new(tone, Label::key("demo.bar.warning")))
            .unwrap();
        roles.push(ui.a11y_node(bar).unwrap().role);
    }
    assert_eq!(roles[2..], [Role::Alert, Role::Alert]);
    assert_eq!(
        roles[..2],
        [Role::Status, Role::Status],
        "info/success interrupt"
    );
}

/// The clear button of a filled search field is announced (boards: `store.search.clear`,
/// accessible name "مسح البحث").
#[test]
fn z05_the_clear_button_of_a_search_field_has_a_node() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let root = ui.root();
    let field = SearchField::new(Label::key("demo.search")).text("شاشة");
    let search = ui.add(root, field).unwrap();
    ui.layout();
    let area = ui.rect(search).unwrap();
    assert!(area.w > 0.0);
    let tree = ui.a11y_tree();
    let buttons = tree
        .iter()
        .filter(|n| n.role == Role::Button && area.contains(n.rect.center()))
        .count();
    assert_eq!(
        buttons, 1,
        "no accessible clear button inside the search field"
    );
}

// ------------------------------------------------------------ names and sentences

fn sentences(lang: Lang) -> Vec<(Role, String)> {
    let (mut ui, g) = setup(lang);
    ui.open_menu(g.menu_button, file_menu()).unwrap();
    let mut out: Vec<_> = ui
        .a11y_tree()
        .iter()
        .map(|n| (n.role, n.describe(ui.translator())))
        .collect();
    ui.close_menu();
    ui.open_dialog(g.modal.dialog).unwrap();
    out.extend(
        ui.a11y_tree()
            .iter()
            .map(|n| (n.role, n.describe(ui.translator()))),
    );
    out
}

/// Every node speaks a sentence in Arabic and in English: never empty, never a raw key,
/// Arabic sentences contain Arabic letters, and both languages have the same nodes.
#[test]
fn z06_every_node_speaks_in_both_languages() {
    let (ar, en) = (sentences(Lang::Ar), sentences(Lang::En));
    assert_eq!(ar.len(), en.len());
    assert!(ar.len() > 25, "gallery, menu and dialog: {}", ar.len());
    for ((role_ar, text_ar), (role_en, text_en)) in ar.iter().zip(&en) {
        assert_eq!(role_ar, role_en, "same order in both directions");
        for text in [text_ar, text_en] {
            assert!(!text.trim().is_empty(), "{role_ar:?} is silent");
            assert!(
                !text.contains("demo.") && !text.contains("ui."),
                "raw key: {text}"
            );
        }
        let arabic = text_ar
            .chars()
            .any(|c| ('\u{0600}'..='\u{06FF}').contains(&c));
        assert!(arabic, "{role_ar:?}: no Arabic in {text_ar:?}");
        let foreign = |c: char| !c.is_ascii() && !('\u{2066}'..='\u{2069}').contains(&c);
        assert!(!text_en.chars().any(foreign), "English: {text_en:?}");
    }
}

/// States are mirrored after operating the widgets: checked, selected, expanded, disabled,
/// focused, busy.
#[test]
fn z07_states_follow_the_wave_three_widgets() {
    let (mut ui, g) = setup(Lang::Ar);
    click(&mut ui, g.checkbox);
    let node = ui.a11y_node(g.checkbox).unwrap();
    assert_eq!(
        (node.role, node.state.checked),
        (Role::Checkbox, Some(true))
    );
    assert!(node.state.focused && node.focusable);
    click(&mut ui, g.items[2]);
    assert_eq!(ui.a11y_node(g.items[2]).unwrap().state.selected, Some(true));
    assert_eq!(
        ui.a11y_node(g.items[0]).unwrap().state.selected,
        Some(false)
    );
    ui.set_disabled(g.list, true).unwrap();
    let item = ui.a11y_node(g.items[1]).unwrap();
    assert!(item.state.disabled && !item.focusable);
    ui.set_progress(g.progress, None).unwrap();
    let bar = ui.a11y_node(g.progress).unwrap();
    assert!(bar.state.busy && bar.value.is_none());
    ui.set_progress(g.progress, Some(0.4)).unwrap();
    let spoken = ui.a11y_node(g.progress).unwrap().describe(ui.translator());
    assert!(
        spoken.contains("40%"),
        "digits stay 0-9 in Arabic: {spoken}"
    );
    let dialog = g.modal.dialog;
    ui.open_dialog(dialog).unwrap();
    let tree = ui.a11y_tree();
    assert_eq!(tree[0].role, Role::Dialog, "the dialog itself comes first");
    assert_eq!(tree[0].name, Some(Label::key("demo.dialog.title")));
    assert!(tree
        .iter()
        .all(|n| n.id == dialog || ui.parent(n.id).is_some()));
    assert_eq!(tree.iter().filter(|n| n.state.focused).count(), 1);
}

// ------------------------------------------------------------ targets

/// Every operable node is at least as large as the minimum target of the input mode
/// (spec §3 "Ziele": pointer 32, touch 44) in both directions.
#[test]
fn z08_every_operable_node_reaches_the_minimum_target() {
    for (lang, mode) in [
        (Lang::Ar, InputMode::Pointer),
        (Lang::En, InputMode::Touch),
        (Lang::Ar, InputMode::Touch),
    ] {
        let (mut ui, g) = setup(lang);
        ui.set_input_mode(mode);
        ui.open_dialog(g.modal.sheet).unwrap();
        let mut nodes = ui.a11y_tree();
        ui.close_dialog(g.modal.sheet).unwrap();
        ui.open_menu(g.menu_button, file_menu()).unwrap();
        nodes.extend(ui.a11y_tree());
        let min = mode.min_target();
        let operable = nodes.iter().filter(|n| n.focusable);
        for node in operable.filter(|n| n.role != Role::ScrollArea) {
            let r = node.rect;
            assert!(
                r.w >= min && r.h >= min,
                "{lang:?} {mode:?}: {:?} is {} x {}",
                node.role,
                r.w,
                r.h
            );
        }
    }
}

/// A tab must stay a full target when the bar is squeezed (five tabs on a phone).
#[test]
fn z09_tabs_keep_the_minimum_target_width_on_a_phone() {
    let size = Size::new(200.0, 400.0);
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, size);
    ui.set_input_mode(InputMode::Touch);
    let root = ui.root();
    let labels = (1..=5).map(|i| Label::data(&format!("قسم {i}"))).collect();
    let tabs = ui
        .add(root, Tabs::new(Label::key("demo.tabs"), labels))
        .unwrap();
    // The bar scrolls by whole tabs (round 4, w-m1): every tab keeps the target width.
    let parts: Vec<_> = ui
        .a11y_tree()
        .into_iter()
        .filter(|n| n.id == tabs && n.part.is_some())
        .collect();
    assert_eq!(parts.len(), 5);
    for tab in parts {
        assert!(
            tab.rect.w >= InputMode::Touch.min_target(),
            "a tab is {} points wide",
            tab.rect.w
        );
    }
}

/// The clear cross of a search field is a touch target of 44 x 44.
#[test]
fn z10_the_clear_button_is_a_full_touch_target() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, SURFACE);
    ui.set_input_mode(InputMode::Touch);
    let root = ui.root();
    let field = SearchField::new(Label::key("demo.search")).text("display");
    let search = ui.add(root, field).unwrap();
    ui.layout();
    let rect = ui.rect(search).unwrap();
    let min = InputMode::Touch.min_target();
    // A finger 2 points inside the 44-point square at the end of the field.
    let pos = Point::new(rect.right() - min + 2.0, center(&mut ui, search).y);
    ui.handle(Event::TouchDown { id: 1, pos });
    ui.handle(Event::TouchUp { id: 1, pos });
    assert_eq!(
        ui.text(search).unwrap(),
        "",
        "the tap missed the clear button"
    );
}
