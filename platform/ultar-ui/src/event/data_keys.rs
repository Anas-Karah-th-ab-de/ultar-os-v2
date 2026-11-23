//! Keyboard and activation of the data widgets: table and tree.

use super::selection::Mark;
use super::{Action, ActionKind, Key};
use crate::ui::{NodeId, Ui};
use crate::widgets::{TableCursor, TablePart, Widget};

fn act(node: NodeId, kind: ActionKind) -> Action {
    Action { node, kind }
}

impl Ui {
    /// A click on a part of a table: a header sorts, a row is selected.
    pub(crate) fn activate_table(
        &mut self,
        id: NodeId,
        part: Option<usize>,
        out: &mut Vec<Action>,
    ) {
        let mark = Mark::from_modifiers(self.input.modifiers);
        let Some(Widget::Table(table)) = self.nodes.get_mut(id.index()).map(|n| &mut n.widget)
        else {
            return;
        };
        match part.map(TablePart::decode) {
            Some(TablePart::Header(column)) => {
                let (column, ascending) = table.next_sort(column);
                table.sort = Some((column, ascending));
                table.cursor = TableCursor::Header(column);
                out.push(act(id, ActionKind::Sorted(column, ascending)));
            }
            Some(TablePart::Row(row)) => {
                table.cursor = TableCursor::Rows;
                if table.multi {
                    if let Some(mark) = mark {
                        return self.mark_table_row(id, row, mark, out);
                    }
                    table.chosen = vec![row];
                    table.anchor = Some(row);
                }
                if table.selected != Some(row) {
                    table.selected = Some(row);
                    out.push(act(id, ActionKind::Selected(row)));
                }
            }
            None => return,
        }
        self.touch(id);
    }

    /// A click on a part of a tree: the toggle opens or closes, the row is selected.
    pub(crate) fn activate_tree(&mut self, id: NodeId, part: Option<usize>, out: &mut Vec<Action>) {
        let Some(part) = part else {
            return;
        };
        let (index, toggle) = (part / 2, part % 2 == 1);
        if toggle {
            self.expand(id, index, None, out);
        } else {
            self.select_item(id, index, out);
        }
    }

    fn select_item(&mut self, id: NodeId, index: usize, out: &mut Vec<Action>) {
        if let Some(Widget::Tree(tree)) = self.slot_mut(id).map(|n| &mut n.widget) {
            if index < tree.items.len() && tree.selected != Some(index) {
                tree.selected = Some(index);
                out.push(act(id, ActionKind::Selected(index)));
            }
        }
        self.touch(id);
        self.reveal_row(id);
    }

    /// Opens (`Some(true)`), closes or flips item `index` of a tree that has children.
    fn expand(&mut self, id: NodeId, index: usize, open: Option<bool>, out: &mut Vec<Action>) {
        let Some(Widget::Tree(tree)) = self.slot_mut(id).map(|n| &mut n.widget) else {
            return;
        };
        if !tree.has_children(index) {
            return;
        }
        let Some(item) = tree.items.get_mut(index) else {
            return;
        };
        let target = open.unwrap_or(!item.expanded);
        if item.expanded != target {
            item.expanded = target;
            out.push(act(id, ActionKind::Expanded(index, target)));
            // The selection never stays hidden in a closed branch: it moves to the item
            // that was closed (as in every desktop tree).
            let hidden = tree.selected.is_some_and(|s| tree.is_descendant(s, index));
            if !target && hidden {
                tree.selected = Some(index);
                out.push(act(id, ActionKind::Selected(index)));
            }
            self.invalidate_node(id);
        }
    }

    /// Scrolls the selected row of a table or tree into view (not the whole widget).
    pub(crate) fn reveal_row(&mut self, id: NodeId) {
        let Some(rect) = self.rect(id) else {
            return;
        };
        let row = match self.widget(id) {
            Some(Widget::Table(t)) => match t.cursor {
                TableCursor::Rows => t.selected.map(|r| t.band(rect, Some(r), self.mode)),
                TableCursor::Header(_) => Some(t.band(rect, None, self.mode)),
            },
            Some(Widget::Tree(t)) => t.selected.and_then(|s| {
                let slot = t.visible().iter().position(|i| *i == s)?;
                Some(t.row(rect, slot, self.mode))
            }),
            _ => None,
        };
        if let Some(row) = row {
            self.reveal_rect(id, row, &mut Vec::new());
        }
    }

    /// Keys of a focused table: Up/Down/Home/End choose a row, Enter activates it; from the
    /// first row Up goes to the headers, where Left/Right move and Enter/Space sort.
    pub(crate) fn table_key(
        &mut self,
        id: NodeId,
        key: Key,
        towards_end: bool,
        out: &mut Vec<Action>,
    ) -> bool {
        let Some(Widget::Table(table)) = self.slot_mut(id).map(|n| &mut n.widget) else {
            return false;
        };
        let rows = table.rows.len();
        let columns = table.columns.len();
        let current = table.selected;
        let row_target = match (table.cursor, key) {
            (TableCursor::Header(_), Key::ArrowDown) if rows > 0 => Some(0),
            (TableCursor::Header(c), Key::ArrowLeft | Key::ArrowRight) => {
                let next = if towards_end {
                    (c + 1).min(columns.saturating_sub(1))
                } else {
                    c.saturating_sub(1)
                };
                table.cursor = TableCursor::Header(next);
                None
            }
            (TableCursor::Header(c), Key::Enter | Key::Space) => {
                self.activate_table(id, Some(TablePart::Header(c).encode()), out);
                return true;
            }
            (TableCursor::Rows, Key::ArrowUp) if current.is_none_or(|r| r == 0) && columns > 0 => {
                table.cursor = TableCursor::Header(0);
                None
            }
            (TableCursor::Rows, Key::ArrowUp) => current.map(|r| r.saturating_sub(1)),
            (TableCursor::Rows, Key::ArrowDown) if rows > 0 => {
                Some(current.map_or(0, |r| (r + 1).min(rows - 1)))
            }
            (TableCursor::Rows, Key::Home) if rows > 0 => Some(0),
            (TableCursor::Rows, Key::End) if rows > 0 => Some(rows - 1),
            (TableCursor::Rows, Key::Enter) => {
                if let Some(row) = current {
                    out.push(act(id, ActionKind::RowActivated(row)));
                }
                return current.is_some();
            }
            _ => return false,
        };
        if let Some(row) = row_target {
            self.activate_table(id, Some(TablePart::Row(row).encode()), out);
        }
        self.touch(id);
        self.reveal_row(id);
        true
    }

    /// Keys of a focused tree: Up/Down/Home/End over the visible items; towards the end
    /// opens (then enters) an item, towards the start closes it (then goes to the parent);
    /// Enter activates.
    pub(crate) fn tree_key(
        &mut self,
        id: NodeId,
        key: Key,
        towards_end: bool,
        out: &mut Vec<Action>,
    ) -> bool {
        let Some(Widget::Tree(tree)) = self.widget(id) else {
            return false;
        };
        let visible = tree.visible();
        let current = tree
            .selected
            .unwrap_or(visible.first().copied().unwrap_or(0));
        let slot = visible.iter().position(|i| *i == current);
        let open = tree.items.get(current).is_some_and(|i| i.expanded);
        let parent = tree.parent(current);
        let children = tree.has_children(current);
        let target = match key {
            // Down from the last item stays there; without a selection it starts at the first.
            Key::ArrowDown => match slot.filter(|_| tree.selected.is_some()) {
                Some(s) => visible.get(s + 1).copied(),
                None => visible.first().copied(),
            },
            Key::ArrowUp => slot
                .and_then(|s| s.checked_sub(1))
                .and_then(|s| visible.get(s))
                .copied(),
            Key::Home => visible.first().copied(),
            Key::End => visible.last().copied(),
            Key::ArrowLeft | Key::ArrowRight if towards_end && children && !open => {
                self.expand(id, current, Some(true), out);
                return true;
            }
            Key::ArrowLeft | Key::ArrowRight if towards_end && children => Some(current + 1),
            Key::ArrowLeft | Key::ArrowRight if !towards_end && open => {
                self.expand(id, current, Some(false), out);
                return true;
            }
            Key::ArrowLeft | Key::ArrowRight if !towards_end => parent,
            Key::Enter => {
                out.push(act(id, ActionKind::RowActivated(current)));
                return true;
            }
            Key::Space => Some(current),
            _ => return false,
        };
        if let Some(index) = target {
            self.select_item(id, index, out);
        }
        true
    }
}
