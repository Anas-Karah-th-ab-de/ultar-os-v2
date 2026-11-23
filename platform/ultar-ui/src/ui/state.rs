//! Reading and changing the state of the widgets added in wave 3 from application code:
//! checkbox, radio group, progress, list selection, tooltips. Setters never emit actions.

use super::{NodeId, Ui, UiError};
use crate::widgets::{Label, Widget};

impl Ui {
    /// State of a checkbox.
    pub fn is_checked(&self, id: NodeId) -> Result<bool, UiError> {
        Ok(self.checkbox(id)?.checked)
    }

    /// Sets a checkbox.
    pub fn set_checked(&mut self, id: NodeId, checked: bool) -> Result<(), UiError> {
        self.checkbox_mut(id)?.checked = checked;
        self.touch(id);
        Ok(())
    }

    /// Chosen option of a radio group; `None` while nothing is chosen.
    pub fn radio_selected(&self, id: NodeId) -> Result<Option<usize>, UiError> {
        Ok(self.radio(id)?.selected)
    }

    /// The share a progress bar shows, 0..=1; `None` while it is indeterminate.
    pub fn progress(&self, id: NodeId) -> Result<Option<f32>, UiError> {
        Ok(self.progress_bar(id)?.value)
    }

    /// Sets the share of a progress bar (clamped to 0..=1) or makes it indeterminate.
    pub fn set_progress(&mut self, id: NodeId, value: Option<f32>) -> Result<(), UiError> {
        self.progress_bar_mut(id)?.set(value);
        self.touch(id);
        Ok(())
    }

    /// `true` when the list item is the selected one.
    pub fn is_item_selected(&self, id: NodeId) -> Result<bool, UiError> {
        Ok(self.list_item(id)?.selected)
    }

    /// Marks or unmarks a list item as selected. In a selectable list, marking one item
    /// unmarks the others.
    pub fn set_item_selected(&mut self, id: NodeId, selected: bool) -> Result<(), UiError> {
        self.list_item(id)?;
        let list = self.node(id)?.parent.filter(
            |parent| matches!(&self.nodes[parent.index()].widget, Widget::List(l) if l.selectable),
        );
        if let (true, Some(list)) = (selected, list) {
            for n in 0..self.nodes[list.index()].children.len() {
                let sibling = self.nodes[list.index()].children[n];
                if let Widget::ListItem(item) = &mut self.nodes[sibling.index()].widget {
                    item.selected = false;
                }
            }
        }
        self.list_item_mut(id)?.selected = selected;
        // The label weight changes with the selection, and with it the item's height.
        self.invalidate_node(id);
        Ok(())
    }

    /// Index (among the list's items) of the selected item of a list.
    pub fn selected_item(&self, list: NodeId) -> Result<Option<usize>, UiError> {
        let node = self.node(list)?;
        if !matches!(node.widget, Widget::List(_)) {
            return Err(UiError::WrongWidget {
                node: list,
                expected: "list",
            });
        }
        Ok(self.list_items(list).position(|(_, selected)| selected))
    }

    /// The list items of a list with their selection flag, in order.
    fn list_items(&self, list: NodeId) -> impl Iterator<Item = (NodeId, bool)> + '_ {
        self.children(list)
            .iter()
            .filter_map(|id| match &self.nodes[id.index()].widget {
                Widget::ListItem(item) => Some((*id, item.selected)),
                _ => None,
            })
    }

    /// Position of a list item among the items of its list.
    pub(crate) fn item_index(&self, list: NodeId, item: NodeId) -> Option<usize> {
        self.list_items(list).position(|(id, _)| id == item)
    }

    /// Sets or removes the tooltip of an operable widget: a short explanation shown after
    /// the pointer rests on it and at once when keyboard focus reaches it. It is also the
    /// widget's accessible description.
    pub fn set_tooltip(&mut self, id: NodeId, tooltip: Option<Label>) -> Result<(), UiError> {
        self.node_mut(id)?.tooltip = tooltip;
        if self.layers.tip.is_some_and(|tip| tip.node == id) {
            self.hide_tooltip();
        }
        if self.input.hover == Some(id) {
            self.watch_hover(Some(id));
        }
        Ok(())
    }

    /// The tooltip of a node.
    pub fn tooltip(&self, id: NodeId) -> Option<&Label> {
        self.nodes.get(id.index()).and_then(|n| n.tooltip.as_ref())
    }

    /// The node whose tooltip shows now.
    pub fn tooltip_shown(&self) -> Option<NodeId> {
        self.layers.tip.map(|tip| tip.node)
    }
}
