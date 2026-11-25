//! login.access: the view of the accessibility menu of the sign-in (its state is shared:
//! `ultar_lock_core::access`).

pub use ultar_lock_core::access::*;

use super::LockState;
use crate::view::{Target, View};
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Card, IconButton, Label, Switch, Text};

/// Width of the menu (`g(64)`).
const WIDTH: f32 = 64.0 * Space::S4.points();

/// The accessibility button at the end edge (`login.access`).
pub(super) fn button(view: &mut View, parent: NodeId, s: &LockState) -> Result<(), UiError> {
    let name = Label::key("lock.access");
    let button = IconButton::new(ultar_ui::icon::IconId::Access, name);
    let id = view.ui.add(parent, button)?;
    view.mark(id, Target::Access);
    if s.access.values.is_none() {
        view.ui.set_disabled(id, true)?;
        view.ui
            .set_tooltip(id, Some(Label::key("lock.reason.no_settings")))?;
    }
    Ok(())
}

/// The open menu, above the end edge's tools.
pub(super) fn menu(view: &mut View, root: NodeId, s: &LockState) -> Result<(), UiError> {
    let (true, Some(values)) = (s.access.open, s.access.values) else {
        return Ok(());
    };
    let row = view.ui.add(root, Flex::row().main(MainAlign::End))?;
    let flex = Flex::column()
        .gap(Space::S8)
        .padding(Padding::symmetric(Space::S16, Space::S12));
    let card = Card::padded().flex(flex);
    let card = view.ui.add_sized(row, card, Sizing::width(WIDTH))?;
    for (i, ((_, name), on)) in SWITCHES.iter().zip(values).enumerate() {
        // The switch carries only its accessible name: the visible name is a text beside it
        // (found in VMware: four switches without any word).
        let line = view
            .ui
            .add(card, Flex::row().cross(CrossAlign::Center).gap(Space::S12))?;
        let names = view
            .ui
            .add_sized(line, Flex::column().gap(Space::S4), Sizing::grow(1))?;
        view.ui.add(names, Text::new(Label::key(*name)))?;
        let id = view.ui.add(line, Switch::new(Label::key(*name), on))?;
        view.mark(id, Target::AccessSwitch(i as u8));
        if !AVAILABLE[i] {
            view.ui.add(
                names,
                Text::new(Label::key("lock.access.unavailable"))
                    .spec(TextSpec::new(TypeSize::S12))
                    .color(ColorRole::TextMuted),
            )?;
            view.ui.set_disabled(id, true)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_toggles_and_the_menu() {
        let mut a = AccessState::default();
        a.flip();
        assert!(!a.open, "no service: no menu");
        assert_eq!(a.toggle(0, true), None);
        a.reported(&[
            ("accessibility.high_contrast".into(), true),
            ("appearance.theme".into(), true),
        ]);
        assert_eq!(a.values, Some([false, true, false, false]));
        a.flip();
        assert!(a.open);
        assert_eq!(a.toggle(0, true), Some(("accessibility.large_text", true)));
        assert_eq!(a.values, Some([true, true, false, false]));
        // no screen reader and no on-screen keyboard program yet: those switch nothing
        assert_eq!(a.toggle(2, true), None);
        assert_eq!(a.toggle(3, true), None);
        assert_eq!(a.values, Some([true, true, false, false]));
        assert_eq!(a.toggle(9, true), None);
        a.flip();
        assert!(!a.open);
    }
}
