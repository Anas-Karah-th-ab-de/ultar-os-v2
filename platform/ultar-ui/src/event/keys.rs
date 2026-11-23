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
        let list: Vec<NodeId> = self.focusable().collect();
        let count = list.len();
        if count == 0 {
            return false;
        }
        let position = current.and_then(|c| list.iter().position(|id| *id == c));
        let target = match (position, forward) {
            (None, _) => {
                self.after_hint(&list, forward)
                    .or(Some(if forward { 0 } else { count - 1 }))
            }
            (Some(p), true) if p + 1 < count => Some(p + 1),
            (Some(p), false) if p > 0 => Some(p - 1),
            (Some(_), true) => wrap.then_some(0),
            (Some(_), false) => wrap.then_some(count - 1),
        };
        let next = target.and_then(|t| list.get(t).copied());
        match next {
            Some(id) if Some(id) != current || !self.input.focus_visible => {
                self.set_focus(Some(id), true);
                true
            }
            _ => false,
        }
    }

    /// Where focus continues after it was lost: the stop after (or before) the place of the
    /// node that had it.
    fn after_hint(&self, list: &[NodeId], forward: bool) -> Option<usize> {
        let hint = self.input.focus_hint?;
        let order = self.reachable();
        let place = order.iter().position(|id| *id == hint)?;
        let index_of = |id: &NodeId| order.iter().position(|o| o == id).unwrap_or(0);
        if forward {
            list.iter().position(|id| index_of(id) > place)
        } else {
            list.iter().rposition(|id| index_of(id) < place)
        }
    }

    /// Escape in a search field that holds text: clear it and leave the field.
    fn search_escape(&mut self, id: NodeId, out: &mut Vec<Action>) -> bool {
        let Ok(field) = self.text_field_mut(id) else {
            return false;
        };
        if !(field.is_search() && field.clear()) {
            return false;
        }
        out.push(Action {
            node: id,
            kind: ActionKind::TextChanged,
        });
        self.set_focus(None, false);
        self.input.focus_hint = Some(id);
        true
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
        if let (Some(id), Key::Escape) = (focus, key) {
            if self.search_escape(id, out) {
                return true;
            }
        }
        if let Some(id) = focus {
            if self.widget_key(id, key, modifiers, out) {
                // Operating a widget by keyboard shows where the focus is.
                self.set_focus(Some(id), true);
                self.default_after_submit(id, key, out);
                return true;
            }
        }
        if key == Key::Enter && modifiers.ctrl {
            return self.toast_key(out);
        }
        self.unclaimed_key(focus, key, out)
    }

    /// Ctrl+Enter runs the action of the newest toast.
    fn toast_key(&mut self, out: &mut Vec<Action>) -> bool {
        let Some(toast) = self.newest_toast().filter(|t| t.action.is_some()) else {
            return false;
        };
        let id = toast.id;
        out.push(Action {
            node: self.root(),
            kind: ActionKind::ToastAction(id),
        });
        self.close_toast(id, out);
        true
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
            // One Escape: the tooltip goes and the layer under it closes too.
            Key::Escape => {
                let tip = self.layers.tip.is_some();
                self.hide_tooltip();
                self.layers.hover_since = None;
                let closed = self.dismiss_modal(out);
                let toast = !closed && self.close_newest_toast(out);
                tip || closed || toast
            }
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
            Key::Char(ch) => self.popup_type_ahead(ch),
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
            Widget::Table(_) if plain => {
                let towards_end = self.towards_end(key);
                self.table_key(id, key, towards_end, out)
            }
            Widget::Tree(_) if plain => {
                let towards_end = self.towards_end(key);
                self.tree_key(id, key, towards_end, out)
            }
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

/// `true` when the visible text starts with `ch` (case and direction marks ignored).
pub(crate) fn starts_with(text: &str, ch: char) -> bool {
    let first = text
        .chars()
        .find(|c| !crate::text::is_bidi_control(*c) && !c.is_whitespace());
    first.is_some_and(|f| f.to_lowercase().eq(ch.to_lowercase()))
}

/// The first index after `from` (cyclic, `count` entries) for which `matches` holds.
pub(crate) fn next_match(
    from: usize,
    count: usize,
    matches: impl Fn(usize) -> bool,
) -> Option<usize> {
    (1..=count)
        .map(|step| (from + step) % count)
        .find(|i| matches(*i))
}

impl Ui {
    /// A letter in the open list of a select: highlight the next option starting with it.
    fn popup_type_ahead(&mut self, ch: char) {
        let Some(popup) = self.input.popup else {
            return;
        };
        let Ok(select) = self.select(popup.select) else {
            return;
        };
        let count = select.options.len();
        let hit = next_match(popup.highlight, count, |i| {
            select
                .options
                .get(i)
                .is_some_and(|o| starts_with(&o.resolve(&self.tr), ch))
        });
        if let Some(index) = hit {
            self.highlight(index);
        }
    }

    fn close_newest_toast(&mut self, out: &mut Vec<Action>) -> bool {
        match self.newest_toast().map(|t| t.id) {
            Some(id) => {
                self.close_toast(id, out);
                true
            }
            None => false,
        }
    }
}
