//! The on-screen keyboard's widget tree: an optional row of word suggestions above the rows
//! of the page shown, each key a button weighted by its width in key units (`ultar-ime`).

use super::Keyboard;
use crate::view::{new_ui, Look, Target, View};
use ultar_i18n::Translator;
use ultar_ime::{Act, Lang, Shift};
use ultar_ui::geom::Size;
use ultar_ui::layout::{Flex, Padding, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::UiError;
use ultar_ui::widgets::{Button, ButtonVariant, Label};

/// The keyboard on a surface of `size`.
pub fn build(k: &Keyboard, look: Look, size: Size, tr: &Translator) -> Result<View, UiError> {
    let mut look = look;
    // The keys keep the layout's direction: Arabic right to left, Latin left to right.
    look.lang = match k.lang {
        Lang::Ar => ultar_i18n::Lang::Ar,
        Lang::En => ultar_i18n::Lang::En,
        Lang::De => ultar_i18n::Lang::De,
    };
    let mut ui = new_ui(look, size, tr)?;
    ui.set_root_flex(
        Flex::column()
            .padding(Padding::all(Space::S8))
            .gap(Space::S8),
    );
    let mut view = View::new(ui);
    let root = view.ui.root();
    let suggestions = k.suggestions();
    if !suggestions.is_empty() {
        let row = view
            .ui
            .add_sized(root, Flex::row().gap(Space::S8), Sizing::grow(1))?;
        for (i, word) in suggestions.iter().enumerate() {
            let button = Button::new(Label::data(word)).variant(ButtonVariant::Quiet);
            let node = view.ui.add_sized(row, button, Sizing::grow(1))?;
            view.mark(node, Target::OskSuggestion(i));
        }
    }
    for (r, row) in k.rows().iter().enumerate() {
        let line = view
            .ui
            .add_sized(root, Flex::row().gap(Space::S8), Sizing::grow(1))?;
        for (i, key) in row.keys.iter().enumerate() {
            let label = Label::data(&caption(k, &key.act, &key.label));
            let mut button = Button::new(label);
            if key.act == Act::Action {
                button = button.variant(ButtonVariant::Primary);
            }
            let grow = (key.weight * 10.0).round().clamp(1.0, 255.0) as u8;
            let node = view.ui.add_sized(line, button, Sizing::grow(grow))?;
            view.mark(node, Target::OskKey(r, i));
        }
    }
    Ok(view)
}

/// What a key shows: its label, or a sign for the keys without one.
fn caption(k: &Keyboard, act: &Act, label: &str) -> String {
    match act {
        Act::Shift if k.shift == Shift::Lock => "⇪".to_owned(),
        Act::Shift => "⇧".to_owned(),
        Act::Backspace => "⌫".to_owned(),
        Act::Action => "↵".to_owned(),
        Act::Lang => k.lang.native().to_owned(),
        _ if label.is_empty() => " ".to_owned(),
        _ => label.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_ui::tokens::ThemeKind;

    #[test]
    fn every_key_and_suggestion_is_a_target() {
        let tr = crate::i18n::translator(ultar_i18n::Lang::En).unwrap();
        let mut k = Keyboard::default();
        k.ime_state(true, 0);
        let look = Look::new(ultar_i18n::Lang::En, ThemeKind::Dark);
        let view = build(&k, look, Size::new(800.0, 320.0), &tr).unwrap();
        let keys: usize = k.rows().iter().map(|r| r.keys.len()).sum();
        assert_eq!(view.targets.len(), keys, "no suggestions without a word");
        assert!(view.node(&Target::OskKey(0, 0)).is_some());
        k.press(&Act::Lang);
        k.press(&Act::Text("t".into()));
        let view = build(&k, look, Size::new(800.0, 320.0), &tr).unwrap();
        assert!(view.node(&Target::OskSuggestion(0)).is_some());
    }
}
