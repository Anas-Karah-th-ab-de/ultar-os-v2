//! Ctrl+A/C/X/V/Z in a text field: the field and the window's clipboard work
//! (`Ui::take_copy`, `Ui::take_paste`, `Ui::paste`); the shortcut works on the key of the US
//! layout under an Arabic layout too, and never copies out of a password field.

mod common;

use ultar_ui::event::{ActionKind, Event, Key, Modifiers};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::widgets::{Label, TextField};
use ultar_ui::{Lang, Size};

fn ctrl(ui: &mut ultar_ui::ui::Ui, ch: char) -> bool {
    ui.handle(Event::Key {
        key: Key::Char(ch),
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    })
    .consumed
}

#[test]
fn select_all_copy_paste_between_two_fields() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 300.0));
    let root = ui.root();
    let a = ui
        .add(root, TextField::new(Label::key("demo.name")).text("Ultar"))
        .unwrap();
    let b = ui
        .add(root, TextField::new(Label::key("demo.name")))
        .unwrap();
    ui.focus(a).unwrap();
    assert!(ctrl(&mut ui, 'a') && ctrl(&mut ui, 'c'));
    assert_eq!(ui.take_copy().as_deref(), Some("Ultar"));
    assert_eq!(ui.take_copy(), None, "taken once");
    ui.focus(b).unwrap();
    assert!(ctrl(&mut ui, 'v'));
    assert!(ui.take_paste());
    assert!(!ui.take_paste(), "asked once");
    let actions = ui.paste("Ultar");
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].kind, ActionKind::TextChanged);
    assert_eq!(ui.text(b).unwrap(), "Ultar");
    // Arabic layout: Ctrl on the key of A gives 'ش'
    assert!(ctrl(&mut ui, 'ش'));
    assert!(ctrl(&mut ui, 'x'));
    assert_eq!(ui.take_copy().as_deref(), Some("Ultar"));
    assert_eq!(ui.text(b).unwrap(), "");
    assert!(ctrl(&mut ui, 'z'));
    assert_eq!(ui.text(b).unwrap(), "Ultar");
}

#[test]
fn a_password_field_gives_nothing_to_the_clipboard() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 300.0));
    let root = ui.root();
    let p = ui
        .add(root, TextField::password(Label::key("demo.name")))
        .unwrap();
    ui.focus(p).unwrap();
    ui.set_text(p, "geheim").unwrap();
    assert!(ctrl(&mut ui, 'a') && ctrl(&mut ui, 'c') && ctrl(&mut ui, 'x'));
    assert_eq!(ui.take_copy(), None);
    assert_eq!(ui.text(p).unwrap(), "geheim");
}
