//! Keyboard operation: focus traversal in logical order and per-widget keys.

use super::{Action, ActionKind, Key, Modifiers};
use crate::ui::{NodeId, Ui};
use crate::widgets::Widget;

impl Ui {
    /// The widgets keyboard focus can reach, in logical (start-to-end, top-to-bottom) order.
    /// The order is the same for both directions; only its visual mapping differs.
    pub fn focus_order(&mut self) -> Vec<NodeId> {
        self.layout();
        self.focusable().collect()
    }

    fn focusable(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.order
            .iter()
            .copied()
            .filter(|id| self.nodes[id.index()].widget.is_interactive() && !self.is_disabled(*id))
    }

    /// Moves keyboard focus to the next or previous widget. `wrap` continues at the other
    /// end. Returns `false` when focus did not move.
    pub(crate) fn move_focus(&mut self, forward: bool, wrap: bool) -> bool {
        let current = self.input.focus;
        let position = current.and_then(|c| self.focusable().position(|id| id == c));
        let count = self.focusable().count();
        if count == 0 {
            return false;
        }
        let target = match (position, forward) {
            (None, true) => Some(0),
            (None, false) => Some(count - 1),
            (Some(p), true) if p + 1 < count => Some(p + 1),
            (Some(p), false) if p > 0 => Some(p - 1),
            (Some(_), true) => wrap.then_some(0),
            (Some(_), false) => wrap.then_some(count - 1),
        };
        let next = target.and_then(|t| self.focusable().nth(t));
        match next {
            Some(id) if Some(id) != current || !self.input.focus_visible => {
                self.set_focus(Some(id), true);
                true
            }
            _ => false,
        }
    }

    /// `true` when a horizontal arrow key points towards the end edge.
    fn towards_end(&self, key: Key) -> bool {
        (key == Key::ArrowRight) != self.env.direction.is_rtl()
    }

    pub(crate) fn key(&mut self, key: Key, modifiers: Modifiers, out: &mut Vec<Action>) -> bool {
        if self.input.popup.is_some() {
            return self.popup_key(key, modifiers, out);
        }
        if key == Key::Tab {
            return !modifiers.command() && self.move_focus(!modifiers.shift, true);
        }
        let focus = self.input.focus.filter(|id| !self.is_disabled(*id));
        if let Some(id) = focus {
            if self.widget_key(id, key, modifiers, out) {
                // Operating a widget by keyboard shows where the focus is.
                self.set_focus(Some(id), true);
                return true;
            }
        }
        match key {
            Key::ArrowDown => self.move_focus(true, false),
            Key::ArrowUp => self.move_focus(false, false),
            Key::ArrowLeft | Key::ArrowRight => self.move_focus(self.towards_end(key), false),
            _ => false,
        }
    }

    fn popup_key(&mut self, key: Key, modifiers: Modifiers, out: &mut Vec<Action>) -> bool {
        let Some(popup) = self.input.popup else {
            return false;
        };
        match key {
            Key::ArrowDown => self.highlight(popup.highlight + 1),
            Key::ArrowUp => self.highlight(popup.highlight.saturating_sub(1)),
            Key::Home => self.highlight(0),
            Key::End => self.highlight(usize::MAX),
            Key::Enter | Key::Space => self.choose(popup.highlight, out),
            Key::Escape => self.close_popup(out),
            Key::Tab => {
                self.close_popup(out);
                self.move_focus(!modifiers.shift, true);
            }
            _ => return false,
        }
        true
    }

    /// Keys a focused widget handles itself.
    fn widget_key(
        &mut self,
        id: NodeId,
        key: Key,
        modifiers: Modifiers,
        out: &mut Vec<Action>,
    ) -> bool {
        match self.nodes[id.index()].widget {
            Widget::Button(_) | Widget::IconButton(_) | Widget::Switch(_) | Widget::Select(_) => {
                let activates = matches!(key, Key::Enter | Key::Space) && !modifiers.command();
                if activates {
                    self.activate(id, out);
                }
                activates
            }
            Widget::Slider(_) => self.slider_key(id, key, out),
            Widget::TextField(_) => self.field_key(id, key, modifiers, out),
            _ => false,
        }
    }

    fn slider_key(&mut self, id: NodeId, key: Key, out: &mut Vec<Action>) -> bool {
        let towards_end = self.towards_end(key);
        let Ok(slider) = self.slider_mut(id) else {
            return false;
        };
        let step = slider.key_step();
        let target = match key {
            Key::ArrowUp => slider.value + step,
            Key::ArrowDown => slider.value - step,
            Key::ArrowLeft | Key::ArrowRight if towards_end => slider.value + step,
            Key::ArrowLeft | Key::ArrowRight => slider.value - step,
            Key::Home => slider.min,
            Key::End => slider.max,
            _ => return false,
        };
        let value = slider.snap(target);
        if value != slider.value {
            slider.value = value;
            out.push(Action {
                node: id,
                kind: ActionKind::ValueChanged(value),
            });
            self.touch(id);
        }
        true
    }

    fn field_key(
        &mut self,
        id: NodeId,
        key: Key,
        modifiers: Modifiers,
        out: &mut Vec<Action>,
    ) -> bool {
        let towards_end = self.towards_end(key);
        let Ok(field) = self.text_field_mut(id) else {
            return false;
        };
        let mut changed = false;
        match key {
            Key::Char(_) | Key::Space if modifiers.command() => return false,
            Key::Char(ch) => changed = field.insert(ch),
            Key::Space => changed = field.insert(' '),
            Key::Backspace => changed = field.backspace(),
            Key::Delete => changed = field.delete(),
            Key::ArrowLeft | Key::ArrowRight => {
                field.move_caret(towards_end);
            }
            Key::Home => field.set_caret(0),
            Key::End => field.set_caret(usize::MAX),
            Key::Enter => out.push(Action {
                node: id,
                kind: ActionKind::Submitted,
            }),
            _ => return false,
        }
        if changed {
            out.push(Action {
                node: id,
                kind: ActionKind::TextChanged,
            });
        }
        self.touch(id);
        true
    }
}
