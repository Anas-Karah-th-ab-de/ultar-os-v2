//! What the assistant may do in an open workbook (manifest `[agent]`): `open`, `read_range`,
//! `write_range` and `add_sheet`. Reading is free; writing and adding a sheet wait for the
//! user's yes (`confirm` in the manifest) and are one undo step each.

use crate::core::Core;
use ultar_office::sheet::{parse_range, Value};
use ultar_office::sheet_edit::NameError;

/// Most cells one request reads or writes.
pub const MAX_CELLS: usize = 10_000;

/// A request of the assistant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Open a workbook (`files://documents/a.xlsx`).
    Open {
        /// The URI.
        uri: String,
    },
    /// Read a range (`A1:C5`, `Sheet2!B2`); answers with the shown values and the formulas.
    ReadRange {
        /// The range.
        range: String,
    },
    /// Write values or formulas (rows of cells) from the top-left cell of the range.
    WriteRange {
        /// The range (its first cell is where writing starts).
        range: String,
        /// Rows of cell contents.
        values: Vec<Vec<String>>,
    },
    /// Add a sheet.
    AddSheet {
        /// Its name.
        name: String,
    },
}

/// The answer to a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// Done; the text says what (for a read: the cells).
    Done(String),
    /// Refused with a reason.
    Refused(String),
    /// Waits for the user's yes.
    Confirm,
    /// The window opens this URI.
    Open(String),
}

impl Request {
    /// Does it change the workbook (and so ask the user first)?
    pub fn needs_confirm(&self) -> bool {
        matches!(self, Request::WriteRange { .. } | Request::AddSheet { .. })
    }
}

impl Core {
    /// Handles a request of the assistant.
    pub fn agent(&mut self, req: Request) -> Reply {
        if req.needs_confirm() {
            if let Err(why) = self.check(&req) {
                return Reply::Refused(why);
            }
            self.pending = Some(req);
            return Reply::Confirm;
        }
        match req {
            Request::Open { uri } => Reply::Open(uri),
            Request::ReadRange { range } => self.read_range(&range),
            _ => Reply::Refused("not a request".into()),
        }
    }

    /// The user answers the question of a waiting request.
    pub fn agent_answer(&mut self, allow: bool) -> Reply {
        let Some(req) = self.pending.take() else {
            return Reply::Refused("nothing waits".into());
        };
        if !allow {
            return Reply::Refused("the user said no".into());
        }
        match req {
            Request::WriteRange { range, values } => self.write_range(&range, &values),
            Request::AddSheet { name } => match self.add_named_sheet(&name) {
                Ok(()) => Reply::Done(format!("sheet {name} added")),
                Err(e) => Reply::Refused(format!("{e:?}")),
            },
            _ => Reply::Refused("not a request".into()),
        }
    }

    /// Checks a writing request before it is shown to the user.
    fn check(&self, req: &Request) -> Result<(), String> {
        match req {
            Request::WriteRange { range, values } => {
                let (sheet, ..) = parse_range(range).ok_or("bad range")?;
                if let Some(n) = sheet {
                    self.book.wb.sheet_index(&n).ok_or("no such sheet")?;
                }
                let cells: usize = values.iter().map(Vec::len).sum();
                if cells == 0 || cells > MAX_CELLS {
                    return Err(format!("between 1 and {MAX_CELLS} cells"));
                }
                for raw in values.iter().flatten().filter(|v| v.starts_with('=')) {
                    ultar_office::sheet::check_formula(raw).map_err(|e| format!("formula: {e}"))?;
                }
                Ok(())
            }
            Request::AddSheet { name } => {
                if !ultar_office::sheet_edit::valid_sheet_name(name) {
                    return Err("bad sheet name".into());
                }
                if self.book.wb.sheet_index(name).is_some() {
                    return Err("a sheet of this name exists".into());
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The cells of a range as text: one line per row, cells separated by tabs; a formula
    /// shows its value and, in brackets, the formula.
    pub fn read_range(&mut self, range: &str) -> Reply {
        let Some((sheet, a, b)) = parse_range(range) else {
            return Reply::Refused("bad range".into());
        };
        let si = match sheet {
            Some(n) => match self.book.wb.sheet_index(&n) {
                Some(i) => i,
                None => return Reply::Refused("no such sheet".into()),
            },
            None => self.book.active,
        };
        let (r0, r1, c0, c1) = (a.0.min(b.0), a.0.max(b.0), a.1.min(b.1), a.1.max(b.1));
        if (u64::from(r1 - r0) + 1) * (u64::from(c1 - c0) + 1) > MAX_CELLS as u64 {
            return Reply::Refused(format!("at most {MAX_CELLS} cells"));
        }
        let mut out = String::new();
        for r in r0..=r1 {
            for c in c0..=c1 {
                if c > c0 {
                    out.push('\t');
                }
                let raw = self.book.wb.sheets[si].get(r, c).to_owned();
                let v = self.book.value_in(si, r, c);
                let text = if matches!(v, Value::Empty) { String::new() } else { v.text() };
                out.push_str(&text);
                if raw.starts_with('=') {
                    out.push_str(&format!(" [{raw}]"));
                }
            }
            out.push('\n');
        }
        Reply::Done(out)
    }

    fn write_range(&mut self, range: &str, values: &[Vec<String>]) -> Reply {
        let Some((sheet, a, _)) = parse_range(range) else {
            return Reply::Refused("bad range".into());
        };
        if let Some(i) = sheet.and_then(|n| self.book.wb.sheet_index(&n)) {
            self.show_sheet(i);
        }
        let (r0, c0) = a;
        let mut cells = Vec::new();
        for (i, row) in values.iter().enumerate() {
            for (j, raw) in row.iter().enumerate() {
                let (r, c) = (r0 + i as u32, c0 + j as u32);
                if r < ultar_office::sheet_refs::MAX_ROWS && c < ultar_office::sheet_refs::MAX_COLS {
                    cells.push(((r, c), raw.clone(), None));
                }
            }
        }
        let n = cells.len();
        self.book.put(cells);
        Reply::Done(format!("{n} cells written"))
    }

    /// Adds a sheet of this name and shows it.
    pub fn add_named_sheet(&mut self, name: &str) -> Result<(), NameError> {
        if !ultar_office::sheet_edit::valid_sheet_name(name) {
            return Err(NameError::Bad);
        }
        if self.book.wb.sheet_index(name).is_some() {
            return Err(NameError::Taken);
        }
        let i = self.book.structural(|wb, _| {
            wb.sheets.push(ultar_office::sheet::Sheet::new(name.trim()));
            wb.sheets.len() - 1
        });
        self.views.resize(self.book.wb.sheets.len(), (crate::sel::Sel::default(), (0, 0)));
        self.show_sheet(i);
        Ok(())
    }
}
