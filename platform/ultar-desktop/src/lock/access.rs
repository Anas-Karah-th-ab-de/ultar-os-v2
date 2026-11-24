//! login.access (board Login, DESKTOP-SPEC §5.3): the accessibility menu of the sign-in –
//! large text, high contrast, screen reader, on-screen keyboard. Each switch changes its
//! setting at once through the settings service's narrow right
//! (`settings.set_accessibility`, only `accessibility.*`); without the service the button is
//! disabled with its reason.

use super::LockState;
use crate::view::{Target, View};
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Card, IconButton, Label, Switch};

/// The switches of the menu in board order: (setting key, catalog key of the name).
pub const SWITCHES: [(&str, &str); 4] = [
    ("accessibility.large_text", "lock.access.large_text"),
    ("accessibility.high_contrast", "lock.access.high_contrast"),
    ("accessibility.screen_reader", "lock.access.screen_reader"),
    (
        "accessibility.on_screen_keyboard",
        "lock.access.on_screen_keyboard",
    ),
];

/// Width of the menu (`g(64)`).
const WIDTH: f32 = 64.0 * Space::S4.points();

/// The accessibility part of the lock screen's state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessState {
    /// The menu is open.
    pub open: bool,
    /// The values of [`SWITCHES`] as the settings service reported them; `None`: the
    /// service did not answer (the button is disabled with its reason).
    pub values: Option<[bool; 4]>,
}

impl AccessState {
    /// Keeps the values the service reported (keys it did not name stay off).
    pub fn reported(&mut self, values: &[(String, bool)]) {
        let mut out = [false; 4];
        for (slot, (key, _)) in out.iter_mut().zip(SWITCHES) {
            *slot = values.iter().any(|(k, on)| k == key && *on);
        }
        self.values = Some(out);
    }

    /// Switch `index` to `on`: the call to make, or `None` (no service, no such switch).
    pub fn toggle(&mut self, index: usize, on: bool) -> Option<(&'static str, bool)> {
        let values = self.values.as_mut()?;
        let slot = values.get_mut(index)?;
        *slot = on;
        SWITCHES.get(index).map(|(key, _)| (*key, on))
    }

    /// Opens or closes the menu (only with the service).
    pub fn flip(&mut self) {
        self.open = !self.open && self.values.is_some();
    }
}

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
        let line = view.ui.add(card, Flex::row().cross(CrossAlign::Center))?;
        let id = view
            .ui
            .add_sized(line, Switch::new(Label::key(*name), on), Sizing::grow(1))?;
        view.mark(id, Target::AccessSwitch(i as u8));
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
        assert_eq!(
            a.toggle(3, true),
            Some(("accessibility.on_screen_keyboard", true))
        );
        assert_eq!(a.values, Some([false, true, false, true]));
        assert_eq!(a.toggle(9, true), None);
        a.flip();
        assert!(!a.open);
    }
}
