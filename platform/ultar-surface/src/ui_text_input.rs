//! Which text field of the tree has the keyboard focus, as the compositor's input method
//! needs to know it (`Request::TextInput`): an on-screen keyboard appears for it and types
//! into it. The kind codes are the ones of `ultar-ime` (`Purpose::code`).

use ultar_ui::ui::Ui;
use ultar_ui::widgets::Widget;

/// Kind code of a plain text field.
pub const TEXT: u8 = 0;
/// Kind code of a password field (no suggestions, no learning).
pub const PASSWORD: u8 = 2;

/// The kind of the focused text field, `None` when the focus is not in one.
pub fn focused(ui: &Ui) -> Option<u8> {
    let id = ui.focused()?;
    match ui.widget(id)? {
        Widget::TextField(field) if field.is_password() => Some(PASSWORD),
        Widget::TextField(_) => Some(TEXT),
        _ => None,
    }
}
