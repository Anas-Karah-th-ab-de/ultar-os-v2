//! Changing the tree after it was built: data of tables, trees and selects, removing and
//! reordering nodes, drop targets. Focus, scroll offsets and selections survive the change
//! where the item they belong to still exists.

use super::{NodeId, Ui, UiError};
use crate::geom::Point;
use crate::layout::Flex;
use crate::widgets::{Label, Table, Tree, TreeItem, Widget};

impl Ui {
    /// Replaces the rows of a table (rows beyond the limit are dropped). Selection, marked
    /// rows, the Shift anchor and the keyboard place follow their rows: a row is the same
    /// row when its cells are equal (the first unused equal row takes it), so sorting keeps
    /// the same files selected. Rows that are gone lose their selection. The scroll offset
    /// of the container around stays. With rows that may have equal cells, use
    /// [`Ui::set_table_rows_keyed`].
    pub fn set_table_rows(&mut self, id: NodeId, rows: Vec<Vec<Label>>) -> Result<(), UiError> {
        self.replace_rows(id, rows, None)
    }

    /// [`Ui::set_table_rows`] with a stable key per row (a file's inode, a database id):
    /// the selection follows the keys. `keys` must have one entry per row.
    pub fn set_table_rows_keyed(
        &mut self,
        id: NodeId,
        rows: Vec<Vec<Label>>,
        keys: Vec<u64>,
    ) -> Result<(), UiError> {
        if keys.len() != rows.len() {
            return Err(UiError::OptionOutOfRange {
                node: id,
                index: keys.len(),
            });
        }
        self.replace_rows(id, rows, Some(keys))
    }

    fn replace_rows(
        &mut self,
        id: NodeId,
        rows: Vec<Vec<Label>>,
        keys: Option<Vec<u64>>,
    ) -> Result<(), UiError> {
        let Widget::Table(table) = &mut self.node_mut(id)?.widget else {
            return Err(UiError::WrongWidget {
                node: id,
                expected: "table",
            });
        };
        let fresh = Table::new(table.name.clone(), Vec::new()).rows(rows);
        let mut keys = keys.unwrap_or_default();
        keys.truncate(fresh.rows.len());
        let map = follow(table, &fresh.rows, &keys);
        let moved = |r: Option<usize>| r.and_then(|r| map.get(r).copied().flatten());
        table.selected = moved(table.selected);
        table.anchor = moved(table.anchor);
        let mut chosen: Vec<usize> = table
            .chosen
            .iter()
            .filter_map(|r| moved(Some(*r)))
            .collect();
        chosen.sort_unstable();
        table.chosen = chosen;
        table.rows = fresh.rows;
        table.keys = keys;
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
        // The focus continues after the place of the branch: remember the node before it
        // in reading order (it stays).
        self.layout();
        let before = self
            .order
            .iter()
            .position(|n| *n == id)
            .and_then(|at| at.checked_sub(1))
            .and_then(|at| self.order.get(at).copied());
        let had_focus = self.input.focus.is_some_and(|f| self.is_inside(f, id));
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
        if had_focus {
            self.input.focus_hint = before;
        }
        self.relayout();
        Ok(())
    }

    /// Clears a removed node and every reference to it.
    fn forget(&mut self, id: NodeId) {
        let node = &mut self.nodes[id.index()];
        node.slot.removed = true;
        node.widget = Widget::Flex(Flex::column());
        node.children.clear();
        node.parent = None;
        node.loading = false;
        node.context_menu = None;
        node.tooltip = None;
        self.layers.free_slots.push(id.index());
        let gestures = &mut self.layers.gestures;
        gestures.grab = gestures.grab.filter(|g| g.node != id);
        gestures.last_click = gestures.last_click.filter(|c| c.0 != id);
        self.layers.drag = self.layers.drag.filter(|d| d.scroller != id);
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

/// For every old row of `table`, its place among `rows` (by key when both sides have keys,
/// else by equal cells; each new row is taken once).
fn follow(table: &Table, rows: &[Vec<Label>], keys: &[u64]) -> Vec<Option<usize>> {
    use std::collections::HashMap;
    let keyed = !keys.is_empty() && table.keys.len() == table.rows.len();
    if keyed {
        let place: HashMap<u64, usize> = keys.iter().enumerate().map(|(i, k)| (*k, i)).collect();
        return table.keys.iter().map(|k| place.get(k).copied()).collect();
    }
    let mut place: HashMap<&[Label], std::collections::VecDeque<usize>> = HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        place.entry(row.as_slice()).or_default().push_back(i);
    }
    table
        .rows
        .iter()
        .map(|row| place.get_mut(row.as_slice()).and_then(|q| q.pop_front()))
        .collect()
}
