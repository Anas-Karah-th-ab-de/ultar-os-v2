//! Changing the tree after it was built: data of tables, trees and selects, removing and
//! reordering nodes, drop targets. Focus, scroll offsets and selections survive the change
//! where the item they belong to still exists.

use super::{NodeId, Ui, UiError};
use crate::geom::Point;
use crate::layout::Flex;
use crate::widgets::{Label, Table, Tree, TreeItem, Widget};

impl Ui {
    /// Replaces the rows of a table (rows beyond the limit are dropped). The selected row,
    /// the marked rows of a multi-selection table and the keyboard place are kept where
    /// they still exist; the scroll offset of the container around stays.
    pub fn set_table_rows(&mut self, id: NodeId, rows: Vec<Vec<Label>>) -> Result<(), UiError> {
        let Widget::Table(table) = &mut self.node_mut(id)?.widget else {
            return Err(UiError::WrongWidget {
                node: id,
                expected: "table",
            });
        };
        let fresh = Table::new(table.name.clone(), Vec::new()).rows(rows);
        table.rows = fresh.rows;
        let count = table.rows.len();
        table.selected = table.selected.filter(|r| *r < count);
        table.chosen.retain(|r| *r < count);
        table.anchor = table.anchor.filter(|r| *r < count);
        self.invalidate_node(id);
        Ok(())
    }

    /// Replaces the items of a tree (normalised as by `Tree::new`). The selection is kept
    /// when its index still exists.
    pub fn set_tree_items(&mut self, id: NodeId, items: Vec<TreeItem>) -> Result<(), UiError> {
        let Widget::Tree(tree) = &mut self.node_mut(id)?.widget else {
            return Err(UiError::WrongWidget {
                node: id,
                expected: "tree",
            });
        };
        let fresh = Tree::new(tree.name.clone(), items);
        tree.items = fresh.items;
        let count = tree.items.len();
        tree.selected = tree.selected.filter(|s| *s < count);
        self.invalidate_node(id);
        Ok(())
    }

    /// Replaces the options of a select; the chosen option is kept when its index still
    /// exists (else the last one is chosen). An open list closes.
    pub fn set_select_options(&mut self, id: NodeId, options: Vec<Label>) -> Result<(), UiError> {
        if options.is_empty() {
            return Err(UiError::OptionOutOfRange { node: id, index: 0 });
        }
        let select = self.select_mut(id)?;
        select.selected = select.selected.min(options.len() - 1);
        select.options = options;
        if self.is_open(id) {
            self.close_popup(&mut Vec::new());
        }
        self.invalidate_node(id);
        Ok(())
    }

    /// Moves child `from` of `parent` to place `to` (both indices among its children).
    pub fn move_child(&mut self, parent: NodeId, from: usize, to: usize) -> Result<(), UiError> {
        let children = &mut self.node_mut(parent)?.children;
        if from >= children.len() || to >= children.len() {
            return Err(UiError::OptionOutOfRange {
                node: parent,
                index: from.max(to),
            });
        }
        let child = children.remove(from);
        children.insert(to, child);
        self.invalidate_node(parent);
        self.repaint_all();
        Ok(())
    }

    /// Removes a node and everything below it. The root cannot be removed. Focus moves on
    /// to the next stop (as when a focused node is disabled); presses, hover, menus, an
    /// open list and dialogs inside the removed branch end. The identifiers of removed
    /// nodes are never reused: they answer `UiError::UnknownNode` from then on.
    pub fn remove(&mut self, id: NodeId) -> Result<(), UiError> {
        self.node(id)?;
        if id == self.root() {
            return Err(UiError::WrongWidget {
                node: id,
                expected: "a node below the root",
            });
        }
        // Disabling first ends every interaction inside the branch the way disabling does.
        self.set_disabled(id, true)?;
        for dialog in self.layers.open.clone() {
            if self.is_inside(dialog, id) {
                self.close_dialog(dialog)?;
            }
        }
        if let Some(parent) = self.nodes[id.index()].parent {
            self.nodes[parent.index()].children.retain(|c| *c != id);
            self.invalidate_node(parent);
        }
        let mut branch = vec![id];
        while let Some(node) = branch.pop() {
            branch.extend(self.nodes[node.index()].children.iter().copied());
            self.forget(node);
        }
        self.relayout();
        Ok(())
    }

    /// Clears a removed node and every reference to it.
    fn forget(&mut self, id: NodeId) {
        let node = &mut self.nodes[id.index()];
        node.removed = true;
        node.widget = Widget::Flex(Flex::column());
        node.children.clear();
        node.parent = None;
        node.loading = false;
        node.context_menu = None;
        node.tooltip = None;
        self.layers.dialogs.retain(|d| *d != id);
        self.layers.open.retain(|d| *d != id);
        let gone = |n: Option<NodeId>| n.filter(|n| *n != id);
        self.input.focus_hint = gone(self.input.focus_hint);
        self.input.middle = gone(self.input.middle);
        for index in 0..self.nodes.len() {
            if let Widget::Dialog(dialog) = &mut self.nodes[index].widget {
                dialog.default_action = gone(dialog.default_action);
                dialog.cancel_action = gone(dialog.cancel_action);
                dialog.return_focus = gone(dialog.return_focus);
                dialog.anchor = gone(dialog.anchor);
            }
        }
        self.layers.backdrop.ready = false;
    }

    /// Lets a node take drops: a drag that ends over it reports it as the target of
    /// `DragEnded`.
    pub fn set_drop_target(&mut self, id: NodeId, target: bool) -> Result<(), UiError> {
        self.node_mut(id)?.drop_target = target;
        Ok(())
    }

    /// The drop target under `pos` for a drag of `dragged` (never the dragged node or a
    /// node inside it).
    pub(crate) fn drop_target_at(&self, pos: Point, dragged: NodeId) -> Option<NodeId> {
        self.reachable().iter().rev().copied().find(|id| {
            self.nodes[id.index()].drop_target
                && !self.is_inside(*id, dragged)
                && !self.is_disabled(*id)
                && self.visible_at(*id, pos)
        })
    }
}
