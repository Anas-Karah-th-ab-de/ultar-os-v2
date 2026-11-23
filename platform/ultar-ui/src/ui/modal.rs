//! Dialogs and sheets in the tree: opening, closing, the focus trap, default and cancel
//! actions.

use super::{NodeId, Ui, UiError};
use crate::event::{Action, ActionKind};
use crate::geom::Point;
use crate::widgets::{DialogKind, Widget};

impl Ui {
    /// `true` when `id` is `ancestor` or below it.
    pub(crate) fn is_inside(&self, id: NodeId, ancestor: NodeId) -> bool {
        let mut current = Some(id);
        while let Some(node) = current {
            if node == ancestor {
                return true;
            }
            current = self.nodes.get(node.index()).and_then(|n| n.parent);
        }
        false
    }

    /// The nodes the user can reach, in logical order: all visible ones, or only those of
    /// the topmost dialog while one is open.
    pub(crate) fn reachable(&self) -> &[NodeId] {
        self.order.get(self.layers.modal_start..).unwrap_or(&[])
    }

    /// The topmost open dialog or sheet.
    pub fn open_dialog_node(&self) -> Option<NodeId> {
        self.layers.open.last().copied()
    }

    /// `true` while the dialog is shown.
    pub fn is_dialog_open(&self, id: NodeId) -> bool {
        self.layers.open.contains(&id)
    }

    /// Shows a dialog or sheet above everything else. Focus moves into it (to its default
    /// action, else to its first operable widget) and stays there until it closes.
    pub fn open_dialog(&mut self, id: NodeId) -> Result<(), UiError> {
        if self.dialog(id)?.open {
            return Ok(());
        }
        let mut dropped = Vec::new();
        self.close_popup(&mut dropped);
        self.close_menu_with(&mut dropped);
        self.hide_tooltip();
        self.set_hover(None);
        self.input.mouse = None;
        self.input.finger = None;
        self.layers.drag = None;
        let (focus, visible) = (self.input.focus, self.input.focus_visible);
        let dialog = self.dialog_mut(id)?;
        dialog.open = true;
        dialog.return_focus = focus;
        let preferred = dialog.default_action;
        self.layers.open.push(id);
        self.invalidate_layout();
        self.layout();
        let first = preferred
            .filter(|target| !self.is_disabled(*target))
            .or_else(|| self.focus_order().first().copied());
        self.set_focus(first, visible);
        Ok(())
    }

    /// Hides a dialog or sheet; focus returns to where it was before it opened. When
    /// another dialog is open above it, that one keeps the focus and inherits the place to
    /// return to.
    pub fn close_dialog(&mut self, id: NodeId) -> Result<(), UiError> {
        if !self.dialog(id)?.open {
            return Ok(());
        }
        let mut dropped = Vec::new();
        self.close_popup(&mut dropped);
        self.close_menu_with(&mut dropped);
        self.hide_tooltip();
        self.set_hover(None);
        self.input.mouse = None;
        self.input.finger = None;
        self.layers.drag = None;
        let visible = self.input.focus_visible;
        let dialog = self.dialog_mut(id)?;
        dialog.open = false;
        let back = dialog.return_focus.take();
        let place = self.layers.open.iter().position(|open| *open == id);
        let above = place.and_then(|at| self.layers.open.get(at + 1).copied());
        self.layers.open.retain(|open| *open != id);
        self.invalidate_layout();
        self.layout();
        if let Some(upper) = above {
            self.dialog_mut(upper)?.return_focus = back;
            return Ok(());
        }
        let back = back
            .filter(|target| self.focus_order().contains(target))
            .or_else(|| {
                // Still inside a dialog: focus must not fall behind it.
                let trapped = self.open_dialog_node().is_some();
                self.focus_order().first().copied().filter(|_| trapped)
            });
        self.set_focus(back, visible);
        Ok(())
    }

    fn check_action(&self, dialog: NodeId, action: Option<NodeId>) -> Result<(), UiError> {
        self.dialog(dialog)?;
        match action {
            Some(button) => {
                let is_button = matches!(self.node(button)?.widget, Widget::Button(_));
                if is_button && self.is_inside(button, dialog) {
                    Ok(())
                } else {
                    Err(UiError::WrongWidget {
                        node: button,
                        expected: "button inside the dialog",
                    })
                }
            }
            None => Ok(()),
        }
    }

    /// Names the button of the dialog that Enter activates (unless the focused widget uses
    /// Enter itself, as buttons do).
    pub fn set_default_action(
        &mut self,
        dialog: NodeId,
        button: Option<NodeId>,
    ) -> Result<(), UiError> {
        self.check_action(dialog, button)?;
        self.dialog_mut(dialog)?.default_action = button;
        Ok(())
    }

    /// Names the button of the dialog that Escape activates before the dialog closes.
    pub fn set_cancel_action(
        &mut self,
        dialog: NodeId,
        button: Option<NodeId>,
    ) -> Result<(), UiError> {
        self.check_action(dialog, button)?;
        self.dialog_mut(dialog)?.cancel_action = button;
        Ok(())
    }

    /// Escape, or a press beside a sheet: the cancel action is activated (if there is one
    /// and it can be), the dialog closes and reports `Closed`.
    pub(crate) fn dismiss_modal(&mut self, out: &mut Vec<Action>) -> bool {
        let Some(id) = self.open_dialog_node() else {
            return false;
        };
        let cancel = self.dialog(id).ok().and_then(|d| d.cancel_action);
        if let Some(button) = cancel {
            self.activate(button, None, out);
        }
        // The application may have reacted to nothing yet: closing is the toolkit's part.
        if self.close_dialog(id).is_ok() {
            out.push(Action {
                node: id,
                kind: ActionKind::Closed,
            });
        }
        true
    }

    /// Enter in a dialog: activates its default action. `false` when there is none.
    pub(crate) fn modal_default(&mut self, out: &mut Vec<Action>) -> bool {
        let default = self
            .open_dialog_node()
            .and_then(|id| self.dialog(id).ok())
            .and_then(|d| d.default_action);
        match default {
            Some(button) => {
                self.activate(button, None, out);
                true
            }
            None => false,
        }
    }

    /// A press that hit no widget while a dialog is open: it never reaches what is behind.
    /// Beside a sheet it dismisses the sheet.
    pub(crate) fn modal_press(&mut self, pos: Point, out: &mut Vec<Action>) -> bool {
        let Some(id) = self.open_dialog_node() else {
            return false;
        };
        let beside = !self.rect(id).is_some_and(|r| r.contains(pos));
        let sheet = self.dialog(id).is_ok_and(|d| d.kind == DialogKind::Sheet);
        if beside && sheet {
            self.dismiss_modal(out);
        }
        true
    }
}
