//! Multi-selection in lists and tables: Ctrl toggles one row, Shift selects the range from
//! the anchor (the last row clicked or toggled) – with a click, Space or the arrow keys.

use super::{Action, ActionKind, Key, Modifiers};
use crate::ui::{NodeId, Ui, UiError};
use crate::widgets::{TableCursor, Widget};

/// How a modified click or key changes a multi-selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    /// Ctrl: the row flips.
    Toggle,
    /// Shift: the rows from the anchor to this one are selected, no others.
    Range,
}

impl Mark {
    /// The mark the held modifiers ask for (Shift wins over Ctrl).
    pub(crate) fn from_modifiers(modifiers: Modifiers) -> Option<Mark> {
        if modifiers.shift {
            Some(Mark::Range)
        } else if modifiers.ctrl {
            Some(Mark::Toggle)
        } else {
            None
        }
    }
}

fn changed(node: NodeId) -> Action {
    Action {
        node,
        kind: ActionKind::SelectionChanged,
    }
}

/// `chosen` after marking `row` (`anchor` is where a range starts).
fn apply(chosen: &mut Vec<usize>, anchor: Option<usize>, row: usize, mark: Mark) {
    match mark {
        Mark::Toggle => match chosen.binary_search(&row) {
            Ok(at) => {
                chosen.remove(at);
            }
            Err(at) => chosen.insert(at, row),
        },
        Mark::Range => {
            let from = anchor.unwrap_or(row);
            *chosen = (from.min(row)..=from.max(row)).collect();
        }
    }
}

impl Ui {
    /// Ctrl or Shift on row `row` of a multi-selection table.
    pub(crate) fn mark_table_row(
        &mut self,
        id: NodeId,
        row: usize,
        mark: Mark,
        out: &mut Vec<Action>,
    ) {
        let Some(Widget::Table(table)) = self.slot_mut(id).map(|n| &mut n.widget) else {
            return;
        };
        if !table.multi || row >= table.rows.len() {
            return;
        }
        apply(&mut table.chosen, table.anchor, row, mark);
        if mark == Mark::Toggle || table.anchor.is_none() {
            table.anchor = Some(row);
        }
        table.selected = Some(row);
        table.cursor = TableCursor::Rows;
        out.push(changed(id));
        self.touch(id);
        self.reveal_row(id);
    }

    /// Ctrl+Space and Shift+Up/Down in a multi-selection table. `None` when the key is not
    /// one of them.
    pub(crate) fn table_mark_key(
        &mut self,
        id: NodeId,
        key: Key,
        modifiers: Modifiers,
        out: &mut Vec<Action>,
    ) -> Option<bool> {
        let Some(Widget::Table(table)) = self.widget(id) else {
            return None;
        };
        if !table.multi || modifiers.alt || modifiers.meta {
            return None;
        }
        let rows = table.rows.len();
        let current = table.selected;
        let target = match (key, modifiers.shift, modifiers.ctrl) {
            (Key::Space, false, true) => {
                return Some(current.is_some_and(|r| {
                    self.mark_table_row(id, r, Mark::Toggle, out);
                    true
                }))
            }
            (Key::ArrowDown, true, false) if rows > 0 => {
                current.map_or(0, |r| (r + 1).min(rows - 1))
            }
            (Key::ArrowUp, true, false) => current.map_or(0, |r| r.saturating_sub(1)),
            _ => return None,
        };
        self.mark_table_row(id, target, Mark::Range, out);
        Some(true)
    }

    /// Ctrl or Shift on a list item of a multi-selection list. `false` when the item is not
    /// in such a list.
    pub(crate) fn mark_list_item(
        &mut self,
        item: NodeId,
        mark: Mark,
        out: &mut Vec<Action>,
    ) -> bool {
        let Some(list) = self.slot(item).and_then(|n| n.parent) else {
            return false;
        };
        let Some(Widget::List(l)) = self.widget(list) else {
            return false;
        };
        let (true, anchor) = (l.multi, l.anchor) else {
            return false;
        };
        let items: Vec<NodeId> = self
            .children(list)
            .iter()
            .copied()
            .filter(|c| matches!(self.nodes[c.index()].widget, Widget::ListItem(_)))
            .collect();
        let Some(row) = items.iter().position(|c| *c == item) else {
            return false;
        };
        let mut chosen: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(
                |(_, c)| matches!(&self.nodes[c.index()].widget, Widget::ListItem(i) if i.selected),
            )
            .map(|(i, _)| i)
            .collect();
        apply(&mut chosen, anchor, row, mark);
        for (i, c) in items.iter().enumerate() {
            if let Widget::ListItem(it) = &mut self.nodes[c.index()].widget {
                it.selected = chosen.binary_search(&i).is_ok();
            }
            self.invalidate_node(*c);
        }
        if let Some(Widget::List(l)) = self.slot_mut(list).map(|n| &mut n.widget) {
            if mark == Mark::Toggle || l.anchor.is_none() {
                l.anchor = Some(row);
            }
        }
        out.push(changed(list));
        true
    }

    /// The selected rows of a table (the marked ones of a multi-selection table) or the
    /// selected items of a list, ascending.
    pub fn selected_rows(&self, id: NodeId) -> Result<Vec<usize>, UiError> {
        match &self.node(id)?.widget {
            Widget::Table(t) if t.multi => Ok(t.chosen.clone()),
            Widget::Table(t) => Ok(t.selected.into_iter().collect()),
            Widget::List(_) => Ok(self
                .children(id)
                .iter()
                .filter(|c| matches!(&self.nodes[c.index()].widget, Widget::ListItem(_)))
                .enumerate()
                .filter(|(_, c)| matches!(&self.nodes[c.index()].widget, Widget::ListItem(i) if i.selected))
                .map(|(i, _)| i)
                .collect()),
            _ => Err(UiError::WrongWidget { node: id, expected: "table or list" }),
        }
    }

    /// Marks exactly `rows` of a multi-selection table (indices out of range are ignored).
    /// Emits no action.
    pub fn set_selected_rows(&mut self, id: NodeId, rows: &[usize]) -> Result<(), UiError> {
        let Widget::Table(table) = &mut self.node_mut(id)?.widget else {
            return Err(UiError::WrongWidget {
                node: id,
                expected: "table",
            });
        };
        let count = table.rows.len();
        let mut chosen: Vec<usize> = rows.iter().copied().filter(|r| *r < count).collect();
        chosen.sort_unstable();
        chosen.dedup();
        table.chosen = chosen;
        self.touch(id);
        Ok(())
    }
}
