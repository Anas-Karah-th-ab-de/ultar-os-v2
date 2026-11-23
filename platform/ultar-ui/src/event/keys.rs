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

    /// Operable, enabled nodes in logical order; only those of the topmost dialog while one
    /// is open (the focus trap).
    fn focusable(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.reachable()
            .iter()
            .copied()
            .filter(|id| self.interactive(*id) && !self.is_disabled(*id))
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
        if self.layers.menu.is_some() {
            let towards_end = self.towards_end(key);
            return self.menu_key(key, towards_end, out);
        }
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
                self.default_after_submit(id, key, out);
                return true;
            }
        }
        self.unclaimed_key(focus, key, out)
    }

    /// Enter in a text field of a dialog also triggers the dialog's default action.
    fn default_after_submit(&mut self, id: NodeId, key: Key, out: &mut Vec<Action>) {
        let field = matches!(self.nodes[id.index()].widget, Widget::TextField(_));
        if field && key == Key::Enter {
            self.modal_default(out);
        }
    }

    /// Keys the focused widget did not use: Escape, the menu key, default action, focus
    /// movement by arrows and scrolling.
    fn unclaimed_key(&mut self, focus: Option<NodeId>, key: Key, out: &mut Vec<Action>) -> bool {
        match key {
            Key::Escape if self.layers.tip.is_some() => {
                self.hide_tooltip();
                self.layers.hover_since = None;
                true
            }
            Key::Escape => self.dismiss_modal(out),
            Key::Enter => self.modal_default(out),
            Key::Menu => focus.is_some_and(|id| {
                self.set_focus(Some(id), true);
                self.open_registered_menu(id, None, out)
            }),
            Key::PageUp | Key::PageDown | Key::Home | Key::End => {
                focus.is_some_and(|id| self.scroll_key(id, key, false, out))
            }
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
        let plain = !modifiers.command();
        match self.nodes[id.index()].widget {
            Widget::Button(_)
            | Widget::IconButton(_)
            | Widget::Switch(_)
            | Widget::Select(_)
            | Widget::ListItem(_) => {
                let activates = matches!(key, Key::Enter | Key::Space) && plain;
                if activates {
                    self.activate(id, None, out);
                }
                activates
            }
            // Enter is left to the default action of a dialog, as in every form.
            Widget::Checkbox(_) => {
                let toggles = key == Key::Space && plain;
                if toggles {
                    self.activate(id, None, out);
                }
                toggles
            }
            Widget::RadioGroup(_) | Widget::Tabs(_) => plain && self.choice_key(id, key, out),
            Widget::Slider(_) => self.slider_key(id, key, out),
            Widget::TextField(_) => self.field_key(id, key, modifiers, out),
            Widget::Scroll(_) => self.scroll_key(id, key, true, out),
            _ => false,
        }
    }

    /// Arrow keys, Home and End in a radio group or tab bar choose a neighbour at once.
    /// Radio options wrap around; tabs stop at the ends.
    fn choice_key(&mut self, id: NodeId, key: Key, out: &mut Vec<Action>) -> bool {
        let towards_end = self.towards_end(key);
        let (count, current, radio) = match &self.nodes[id.index()].widget {
            Widget::RadioGroup(group) => (group.options.len(), group.current(), true),
            Widget::Tabs(tabs) => (tabs.tabs.len(), tabs.selected, false),
            _ => return false,
        };
        if count == 0 {
            return false;
        }
        let forward = match key {
            Key::ArrowDown if radio => true,
            Key::ArrowUp if radio => false,
            Key::ArrowLeft | Key::ArrowRight => towards_end,
            Key::Home => return self.choose_part(id, 0, out),
            Key::End => return self.choose_part(id, count - 1, out),
            Key::Space if radio => return self.choose_part(id, current, out),
            _ => return false,
        };
        let target = match (forward, radio) {
            (true, true) => (current + 1) % count,
            (false, true) => (current + count - 1) % count,
            (true, false) => (current + 1).min(count - 1),
            (false, false) => current.saturating_sub(1),
        };
        self.choose_part(id, target, out)
    }

    fn choose_part(&mut self, id: NodeId, part: usize, out: &mut Vec<Action>) -> bool {
        self.activate(id, Some(part), out);
        true
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
        let ui_direction = self.env.direction;
        let Ok(field) = self.text_field_mut(id) else {
            return false;
        };
        let base_rtl = field.direction.resolve(ui_direction).is_rtl();
        let mut changed = false;
        match key {
            Key::Char(_) | Key::Space if modifiers.command() => return false,
            // Escape empties a search field that holds text; otherwise it is not used here.
            Key::Escape if field.is_search() && !field.content().is_empty() => {
                changed = field.clear();
            }
            Key::Char(ch) => changed = field.insert(ch),
            Key::Space => changed = field.insert(' '),
            Key::Backspace => changed = field.backspace(),
            Key::Delete => changed = field.delete(),
            Key::ArrowLeft | Key::ArrowRight => {
                field.move_caret_visual(key == Key::ArrowRight, base_rtl);
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
