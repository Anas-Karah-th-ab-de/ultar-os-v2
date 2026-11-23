//! Review round 4 (second reviewer): behaviour of the wave-4 widgets and input primitives
//! (password field, hover after interruptions, toasts).
//! Target path: platform/ultar-ui/tests/w4_widgets.rs

mod common;

use ultar_ui::event::{Event, Key, PointerButton};
use ultar_ui::layout::Flex;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::{NodeId, Role, Ui};
use ultar_ui::widgets::{Button, Dialog, Label, Tabs, TextField, Widget};
use ultar_ui::{Direction, Lang, Point, Size};

const SECRET: &str = "sesame-42";

fn revealed(ui: &Ui, id: NodeId) -> bool {
    matches!(ui.widget(id), Some(Widget::TextField(f)) if f.is_revealed())
}

/// A field with its eye button held down; returns (ui, field, dialog).
fn held(lang: Lang) -> (Ui, NodeId, NodeId) {
    let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(400.0, 300.0));
    ui.set_root_flex(Flex::column());
    let root = ui.root();
    let field = TextField::password(Label::key("demo.device")).text(SECRET);
    let id = ui.add(root, field).unwrap();
    let dialog = ui
        .add(root, Dialog::new(Label::key("demo.dialog.title")))
        .unwrap();
    ui.add(dialog, Button::new(Label::key("demo.cancel")))
        .unwrap();
    let eye = ui
        .a11y_tree()
        .into_iter()
        .find(|n| n.id == id && n.part == Some(1))
        .expect("eye button node")
        .rect;
    let pos = eye.center();
    ui.handle(Event::PointerMove { pos });
    ui.handle(Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    });
    assert!(revealed(&ui, id), "{lang:?}: held eye shows the password");
    (ui, id, dialog)
}

/// The password is shown only while the eye button is held. Interruptions that end the
/// hold without a release on the window must hide it again.
#[test]
fn pw01_the_password_is_hidden_again_when_the_hold_is_interrupted() {
    type Cut = fn(&mut Ui, NodeId, NodeId);
    let cuts: [(&str, Cut); 3] = [
        ("pointer leaves the window", |ui, _, _| {
            ui.handle(Event::PointerLeave);
        }),
        ("field disabled", |ui, f, _| {
            ui.set_disabled(f, true).unwrap()
        }),
        ("dialog opens", |ui, _, d| ui.open_dialog(d).unwrap()),
    ];
    let mut still = Vec::new();
    for lang in [Lang::Ar, Lang::En] {
        for (name, cut) in cuts {
            let (mut ui, id, dialog) = held(lang);
            cut(&mut ui, id, dialog);
            if revealed(&ui, id) {
                still.push(format!("{lang:?}: {name}"));
            }
        }
    }
    assert!(still.is_empty(), "password still shown after: {still:?}");
}

/// The secret must not reach logs: `Debug` of the widget (and of the tree) prints it.
#[test]
fn pw02_debug_output_does_not_contain_the_password() {
    let (ui, id, _) = held(Lang::En);
    let debug = format!("{:?}", ui.widget(id).unwrap());
    assert!(
        !debug.contains(SECRET),
        "Debug prints the password: {debug}"
    );
}

/// The eye button is announced as a button, but nothing can operate it without a pointer:
/// it is not focusable, no key of the focused field shows the password, so keyboard and
/// screen-reader users cannot check what they typed.
#[test]
fn pw03_the_password_can_be_shown_without_a_pointer() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 200.0));
    let root = ui.root();
    let field = TextField::password(Label::key("demo.device")).text(SECRET);
    let id = ui.add(root, field).unwrap();
    common::key(&mut ui, Key::Tab);
    assert_eq!(ui.focused(), Some(id));
    let eye = ui
        .a11y_tree()
        .into_iter()
        .find(|n| n.id == id && n.role == Role::Button)
        .unwrap();
    assert!(
        eye.focusable,
        "the show-password button cannot be reached by keyboard"
    );
}

/// After an interruption that drops hover (a disabled node enabled again, a flipped
/// direction) the resting pointer's hover comes back only with the next pointer event –
/// unless some unrelated layout happens first. Both pictures must be the same.
#[test]
fn hv01_hover_returns_when_a_node_is_enabled_again() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 200.0));
    let root = ui.root();
    let b = ui.add(root, Button::new(Label::key("demo.apply"))).unwrap();
    let pos = common::center(&mut ui, b);
    ui.handle(Event::PointerMove { pos });
    assert_eq!(ui.hovered(), Some(b));
    ui.set_disabled(b, true).unwrap();
    ui.set_disabled(b, false).unwrap();
    ui.layout();
    assert_eq!(
        ui.hovered(),
        Some(b),
        "the pointer still rests on the button"
    );
}

/// The tab under a resting pointer after the direction flips (the bar is mirrored): the
/// incremental picture keeps the hover on the tab that was there before.
#[test]
fn hv02_the_hovered_tab_follows_a_direction_change() {
    let tabs = || ["a", "b", "c"].map(Label::data).to_vec();
    let setup = |flip_first: bool| {
        let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(400.0, 120.0));
        let root = ui.root();
        ui.add(root, Tabs::new(Label::key("demo.tabs"), tabs()))
            .unwrap();
        let mut canvas =
            ultar_ui::canvas::Canvas::new(400, 120, ultar_ui::canvas::PixelFormat::Bgrx8).unwrap();
        let pos = Point::new(20.0, 20.0);
        if flip_first {
            ui.set_direction(Direction::Ltr);
        }
        ui.handle(Event::PointerMove { pos });
        ui.paint(&mut canvas);
        if !flip_first {
            ui.set_direction(Direction::Ltr);
            ui.paint(&mut canvas);
        }
        canvas
    };
    let (late, early) = (setup(false), setup(true));
    assert!(
        late.data() == early.data(),
        "hover stays on the tab that moved away"
    );
}

/// Tabs keep their target size (m5); a bar wider than the surface must scroll (boards:
/// "Tabs keep the target size and scroll when they do not fit"). Today the tabs past the
/// edge are drawn outside the surface: the chosen tab is invisible and cannot be clicked.
#[test]
fn tb01_the_chosen_tab_is_visible_when_the_bar_does_not_fit() {
    for lang in [Lang::Ar, Lang::En] {
        let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(200.0, 120.0));
        let root = ui.root();
        let names: Vec<Label> = (1..=8).map(|i| Label::data(&format!("Tab {i}"))).collect();
        let id = ui
            .add(root, Tabs::new(Label::key("demo.tabs"), names))
            .unwrap();
        common::key(&mut ui, Key::Tab);
        common::key(&mut ui, Key::End);
        let chosen = ui
            .a11y_tree()
            .into_iter()
            .find(|n| n.id == id && n.state.selected == Some(true))
            .expect("a chosen tab");
        let r = chosen.rect;
        assert!(
            r.x >= 0.0 && r.right() <= 200.0,
            "{lang:?}: chosen tab {:?} at {r:?} lies outside the 200-point surface",
            chosen.position
        );
    }
}

/// Memory per widget: every node carries a `Widget` enum by value.
#[test]
fn mem01_size_of_the_widget_enum() {
    let widget = std::mem::size_of::<Widget>();
    let a11y = std::mem::size_of::<ultar_ui::ui::A11yNode>();
    println!("size_of Widget {widget} B, A11yNode {a11y} B");
    assert!(widget <= 256, "Widget is {widget} bytes per node");
}
