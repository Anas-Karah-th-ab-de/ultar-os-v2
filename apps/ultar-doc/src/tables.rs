//! Tables: insert, add and remove rows and columns, remove the table. The caret decides
//! which table, row and column; every operation is one undo step.

use crate::state::{Cell, Cursor, Doc};
use ultar_office::doc::{Block, Para};

/// Largest table the app inserts.
pub const MAX_ROWS: usize = 200;
pub const MAX_COLS: usize = 12;

impl Doc {
    /// `true` when the caret is in a table cell.
    pub fn in_table(&self) -> bool {
        self.cursor.cell.is_some()
            && matches!(
                self.document.blocks.get(self.cursor.block),
                Some(Block::Table(_))
            )
    }

    /// Inserts an empty table after the caret's block; the caret goes into its first cell.
    pub fn insert_table(&mut self, rows: usize, cols: usize) {
        let (rows, cols) = (rows.clamp(1, MAX_ROWS), cols.clamp(1, MAX_COLS));
        self.begin();
        let at = self.cursor.block + 1;
        self.document
            .blocks
            .insert(at, Block::Table(vec![vec![String::new(); cols]; rows]));
        if !matches!(self.document.blocks.get(at + 1), Some(Block::Para(_))) {
            self.document
                .blocks
                .insert(at + 1, Block::Para(Para::default()));
        }
        self.cursor = Cursor {
            block: at,
            cell: Some(Cell::default()),
            ch: 0,
        };
        self.anchor = None;
        self.hist.end(&self.document);
        self.clamp();
    }

    fn table_mut(&mut self) -> Option<(&mut Vec<Vec<String>>, Cell)> {
        let cell = self.cursor.cell?;
        match self.document.blocks.get_mut(self.cursor.block) {
            Some(Block::Table(t)) => Some((t, cell)),
            _ => None,
        }
    }

    /// Adds an empty row below the caret's row.
    pub fn table_add_row(&mut self) -> bool {
        if !self.in_table() {
            return false;
        }
        self.begin();
        let Some((t, cell)) = self.table_mut() else {
            return false;
        };
        if t.len() >= MAX_ROWS {
            self.hist.end(&self.document);
            return false;
        }
        let cols = t.get(cell.row).map_or(1, Vec::len);
        t.insert(cell.row + 1, vec![String::new(); cols]);
        self.cursor = Cursor {
            cell: Some(Cell {
                row: cell.row + 1,
                col: cell.col,
            }),
            ch: 0,
            ..self.cursor
        };
        self.hist.end(&self.document);
        self.clamp();
        true
    }

    /// Removes the caret's row (the last row of a table stays: use `table_delete`).
    pub fn table_remove_row(&mut self) -> bool {
        if !self.in_table() {
            return false;
        }
        self.begin();
        let Some((t, cell)) = self.table_mut() else {
            return false;
        };
        let done = t.len() > 1;
        if done {
            t.remove(cell.row);
        }
        self.cursor.ch = 0;
        self.hist.end(&self.document);
        self.clamp();
        done
    }

    /// Adds an empty column after the caret's column.
    pub fn table_add_col(&mut self) -> bool {
        if !self.in_table() {
            return false;
        }
        self.begin();
        let Some((t, cell)) = self.table_mut() else {
            return false;
        };
        let cols = t.iter().map(Vec::len).max().unwrap_or(0);
        if cols >= MAX_COLS {
            self.hist.end(&self.document);
            return false;
        }
        for row in t.iter_mut() {
            let at = (cell.col + 1).min(row.len());
            row.insert(at, String::new());
        }
        self.cursor = Cursor {
            cell: Some(Cell {
                row: cell.row,
                col: cell.col + 1,
            }),
            ch: 0,
            ..self.cursor
        };
        self.hist.end(&self.document);
        self.clamp();
        true
    }

    /// Removes the caret's column (the last column stays: use `table_delete`).
    pub fn table_remove_col(&mut self) -> bool {
        if !self.in_table() {
            return false;
        }
        self.begin();
        let Some((t, cell)) = self.table_mut() else {
            return false;
        };
        let cols = t.iter().map(Vec::len).max().unwrap_or(0);
        let done = cols > 1;
        if done {
            for row in t.iter_mut() {
                if cell.col < row.len() {
                    row.remove(cell.col);
                }
            }
        }
        self.cursor.ch = 0;
        self.hist.end(&self.document);
        self.clamp();
        done
    }

    /// Removes the whole table.
    pub fn table_delete(&mut self) -> bool {
        if !self.in_table() {
            return false;
        }
        self.begin();
        self.document.blocks.remove(self.cursor.block);
        self.cursor = Cursor {
            block: self.cursor.block,
            cell: None,
            ch: 0,
        };
        self.hist.end(&self.document);
        self.clamp();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_of(d: &Doc) -> Vec<Vec<String>> {
        match d
            .document
            .blocks
            .iter()
            .find(|b| matches!(b, Block::Table(_)))
        {
            Some(Block::Table(t)) => t.clone(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn rows_and_columns_come_and_go_with_the_caret() {
        let mut d = Doc::new();
        assert!(!d.table_add_row(), "no table at the caret");
        d.insert_table(2, 2);
        assert!(d.in_table());
        d.insert("a");
        assert_eq!(table_of(&d)[0][0], "a");
        assert!(d.table_add_row());
        assert_eq!(table_of(&d).len(), 3);
        assert!(d.table_add_col());
        assert_eq!(table_of(&d)[0].len(), 3);
        assert_eq!(d.cursor.cell, Some(Cell { row: 1, col: 1 }));
        assert!(d.table_remove_col() && d.table_remove_row());
        assert_eq!((table_of(&d).len(), table_of(&d)[0].len()), (2, 2));
        assert!(
            matches!(d.document.blocks.last(), Some(Block::Para(_))),
            "a paragraph follows"
        );
        assert!(d.undo() && d.undo());
        assert_eq!(table_of(&d).len(), 3, "each operation is one undo step");
    }

    #[test]
    fn the_last_row_and_column_stay_and_the_table_can_go() {
        let mut d = Doc::new();
        d.insert_table(1, 1);
        assert!(!d.table_remove_row() && !d.table_remove_col());
        assert!(d.table_delete());
        assert!(table_of(&d).is_empty());
        assert!(!d.in_table());
    }

    #[test]
    fn typing_walks_over_the_cells() {
        let mut d = Doc::new();
        d.insert_table(1, 2);
        d.insert("x");
        d.move_chars(1, false);
        assert_eq!(d.cursor.cell, Some(Cell { row: 0, col: 1 }));
        d.insert("مرحبا");
        assert_eq!(table_of(&d)[0], ["x", "مرحبا"]);
    }
}
