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
    }

    /// A list item was activated: it reports `Activated`; in a selectable list it becomes
    /// the selected item and the list reports `Selected(index)`.
    fn activate_item(&mut self, id: NodeId, out: &mut Vec<Action>) {
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
        if selectable && !already && self.set_item_selected(id, true).is_ok() {
            if let Some(index) = self.item_index(list, id) {
                out.push(Action {
                    node: list,
                    kind: ActionKind::Selected(index),
                });
            }
        }
    }
}
