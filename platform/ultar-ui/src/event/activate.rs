//! Activation: what a click, tap, Enter or Space does to each widget.

use super::{Action, ActionKind};
use crate::ui::{NodeId, Ui};
use crate::widgets::Widget;

impl Ui {
    /// Performs the widget's main action. `part` is the part that was clicked (a tab, a
    /// radio option, the clear button); `None` from the keyboard.
    pub(crate) fn activate(&mut self, id: NodeId, part: Option<usize>, out: &mut Vec<Action>) {
        if self.is_disabled(id) || self.nodes[id.index()].loading {
            return;
        }
        let now = self.now_ms;
        let animate = !self.reduce_motion();
        let open = self.is_open(id);
        let kind = match &mut self.nodes[id.index()].widget {
            Widget::Button(_) | Widget::IconButton(_) => ActionKind::Activated,
            Widget::Switch(switch) => {
                switch.toggle(now, animate);
                ActionKind::Toggled(switch.on)
            }
            Widget::Checkbox(checkbox) => ActionKind::Toggled(checkbox.toggle()),
            Widget::Select(_) if open => return self.close_popup(out),
            Widget::Select(_) => return self.open_popup(id, out),
            Widget::RadioGroup(group) => {
                let target = part.unwrap_or(group.current());
                if group.selected == Some(target) || target >= group.options.len() {
                    return;
                }
                group.selected = Some(target);
                ActionKind::Selected(target)
            }
            Widget::Tabs(tabs) => match part {
                Some(target) if target != tabs.selected && target < tabs.tabs.len() => {
                    tabs.selected = target;
                    ActionKind::Selected(target)
                }
                _ => return,
            },
            Widget::TextField(field) => match part {
                Some(0) if field.clear() => ActionKind::TextChanged,
                _ => return,
            },
            Widget::ListItem(_) => return self.activate_item(id, out),
            Widget::Table(_) => return self.activate_table(id, part, out),
            Widget::Tree(_) => return self.activate_tree(id, part, out),
            _ => return,
        };
        out.push(Action { node: id, kind });
        self.touch(id);
        self.keep_tab_visible(id);
    }

    /// A tab strip that does not fit scrolls so that its chosen tab is visible.
    pub(crate) fn keep_tab_visible(&mut self, id: NodeId) {
        let Some(rect) = self.rect(id) else {
            return;
        };
        if let Some(Widget::Tabs(tabs)) = self.nodes.get_mut(id.index()).map(|n| &mut n.widget) {
            if tabs.keep_chosen_visible(rect) {
                self.touch(id);
            }
        }
    }

    /// A list item was activated: it reports `Activated`; in a selectable list it becomes
    /// the selected item and the list reports `Selected(index)`.
    fn activate_item(&mut self, id: NodeId, out: &mut Vec<Action>) {
        if let Some(mark) = super::selection::Mark::from_modifiers(self.input.modifiers) {
            if self.mark_list_item(id, mark, out) {
                return;
            }
        }
        out.push(Action {
            node: id,
            kind: ActionKind::Activated,
        });
        self.touch(id);
        let Some(list) = self.nodes[id.index()].parent else {
            return;
        };
        let selectable =
            matches!(&self.nodes[list.index()].widget, Widget::List(l) if l.selectable);
        let already = self.list_item(id).is_ok_and(|item| item.selected);
        // In a multi-selection list a plain click keeps only this item selected.
        let others = self.selected_rows(list).is_ok_and(|rows| rows.len() > 1);
        let index = self.item_index(list, id);
        if let Some(Widget::List(l)) = self.nodes.get_mut(list.index()).map(|n| &mut n.widget) {
            if l.multi {
                l.anchor = index;
            }
        }
        if selectable && (!already || others) && self.set_item_selected(id, true).is_ok() {
            if let Some(index) = self.item_index(list, id) {
                out.push(Action {
                    node: list,
                    kind: ActionKind::Selected(index),
                });
            }
        }
    }
}
