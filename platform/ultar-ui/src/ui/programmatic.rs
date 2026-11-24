//! What an application sets on nodes from its own code: the keyboard focus (after it rebuilt
//! a page, DESKTOP-SPEC §3: focus never gets lost) and an accessible name that differs from
//! the visible label (boards: "الخطوة 1: اللغة" on a step that shows "اللغة").

use super::{NodeId, Ui, UiError};
use crate::widgets::Label;

impl Ui {
    /// Moves the keyboard focus to `id` (a control that takes focus, not disabled, reachable:
    /// inside the topmost open dialog while one is open). The focus ring shows when the user
    /// was working with the keyboard. Use it after replacing content: the old focused node
    /// is gone, the new one gets the focus with this call.
    pub fn focus(&mut self, id: NodeId) -> Result<(), UiError> {
        self.node(id)?;
        self.layout();
        if !self.focus_order().contains(&id) {
            return Err(UiError::WrongWidget {
                node: id,
                expected: "a focusable node that is shown and enabled",
            });
        }
        let visible = self.input.focus_visible;
        self.set_focus(Some(id), visible);
        self.input.focus_hint = None;
        Ok(())
    }

    /// Gives a node an accessible name of its own (`None` goes back to the visible label).
    /// For a control whose visible label needs its context to be understood out of the
    /// screen: "استرجاع السابق" is announced "بطاقة الرسوميات: استرجاع التعريف السابق".
    pub fn set_accessible_name(&mut self, id: NodeId, name: Option<Label>) -> Result<(), UiError> {
        self.node(id)?;
        match name {
            Some(name) => {
                self.layers.names.insert(id, name);
            }
            None => {
                self.layers.names.remove(&id);
            }
        }
        Ok(())
    }

    /// Shows or masks a password field whose show button is outside it
    /// (`TextField::reveal_outside`): the toggle of that button. It stays as set until set
    /// again (a held reveal ends on release).
    pub fn set_password_shown(&mut self, id: NodeId, shown: bool) -> Result<(), UiError> {
        let field = self.text_field_mut(id)?;
        if !field.is_password() {
            return Err(UiError::NotApplicable(id));
        }
        field.shown = shown;
        self.touch(id);
        Ok(())
    }

    /// The accessible name set with [`Ui::set_accessible_name`], if any.
    pub fn accessible_name(&self, id: NodeId) -> Option<&Label> {
        self.layers.names.get(&id)
    }
}
