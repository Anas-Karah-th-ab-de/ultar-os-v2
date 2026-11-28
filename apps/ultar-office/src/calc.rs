//! Recalculation by dependencies for the spreadsheet app.
//!
//! [`Calc`] keeps the value of every formula cell it has calculated. A cell is calculated after
//! the cells it reads (an explicit stack, so a chain of 100 000 formulas needs no deep
//! recursion); a change of a cell throws away exactly the values that depend on it, directly
//! or through other formulas, and nothing else. A cycle gives `#CIRC!` in its cells.

use crate::sheet::{parse, Ast, Eval, Value, Workbook};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// A cell: sheet, row, column.
pub type Key = (usize, u32, u32);

/// The calculated values (shared with the evaluator).
pub(crate) type Memo = RefCell<HashMap<Key, Value>>;

/// A rectangle some formula reads.
#[derive(Clone, Copy)]
struct RangeDep {
    sheet: usize,
    r0: u32,
    c0: u32,
    r1: u32,
    c1: u32,
    owner: Key,
}

impl RangeDep {
    fn contains(&self, k: Key) -> bool {
        k.0 == self.sheet
            && (self.r0..=self.r1).contains(&k.1)
            && (self.c0..=self.c1).contains(&k.2)
    }
}

/// What a parsed formula reads.
struct Parsed {
    ast: Option<Rc<Ast>>,
    /// Single cells read.
    cells: Vec<Key>,
    /// Rectangles read.
    ranges: Vec<(usize, u32, u32, u32, u32)>,
    volatile: bool,
}

fn walk(a: &Ast, wb: &Workbook, home: usize, p: &mut Parsed) {
    match a {
        Ast::Ref(sh, s, e, is_range) => {
            let sheet = match sh {
                Some(n) => wb.sheet_index(n),
                None => Some(home),
            };
            if let Some(si) = sheet {
                if *is_range {
                    p.ranges.push((si, s.0, s.1, e.0, e.1));
                } else {
                    p.cells.push((si, s.0, s.1));
                }
            }
        }
        Ast::Un(_, x) => walk(x, wb, home, p),
        Ast::Bin(_, x, y) => {
            walk(x, wb, home, p);
            walk(y, wb, home, p);
        }
        Ast::Call(n, args) => {
            if n == "NOW" || n == "TODAY" {
                p.volatile = true;
            }
            for x in args {
                walk(x, wb, home, p);
            }
        }
        Ast::Num(_) | Ast::Str(_) | Ast::Bool(_) | Ast::Err(_) => {}
    }
}

/// The calculator: values of formula cells with their dependencies.
pub struct Calc {
    memo: Memo,
    parsed: HashMap<Key, Rc<Parsed>>,
    readers: HashMap<Key, Vec<Key>>,
    ranges: Vec<RangeDep>,
    volatile: HashSet<Key>,
    now: f64,
    /// How many formula cells were calculated since the start (for tests and budgets).
    pub calculated: u64,
}

impl Calc {
    /// A calculator for the time `now` (Excel serial number, see [`crate::sheet::excel_now`]).
    pub fn new(now: f64) -> Calc {
        Calc {
            memo: RefCell::new(HashMap::new()),
            parsed: HashMap::new(),
            readers: HashMap::new(),
            ranges: Vec::new(),
            volatile: HashSet::new(),
            now,
            calculated: 0,
        }
    }

    /// Forgets everything (after a change that moves cells around).
    pub fn clear(&mut self) {
        self.memo.borrow_mut().clear();
        self.parsed.clear();
        self.readers.clear();
        self.ranges.clear();
        self.volatile.clear();
    }

    /// Sets the time `TODAY` and `NOW` see; the cells that use them are calculated again.
    pub fn set_now(&mut self, now: f64) {
        self.now = now;
        let v: Vec<Key> = self.volatile.iter().copied().collect();
        self.changed(&v);
    }

    /// The value of a cell.
    pub fn value(&mut self, wb: &Workbook, sheet: usize, r: u32, c: u32) -> Value {
        let Some(sh) = wb.sheets.get(sheet) else {
            return Value::Err("#REF!".into());
        };
        let raw = sh.get(r, c);
        if !raw.starts_with('=') {
            return crate::sheet::literal(raw);
        }
        self.ensure(wb, (sheet, r, c));
        self.memo
            .borrow()
            .get(&(sheet, r, c))
            .cloned()
            .unwrap_or(Value::Empty)
    }

    /// Cells `cells` were changed: their values and every value that depends on them are
    /// thrown away (the formulas are read again from the workbook when needed).
    pub fn changed(&mut self, cells: &[Key]) {
        let mut queue: Vec<Key> = cells.to_vec();
        queue.extend(self.volatile.iter().copied());
        let mut gone: HashSet<Key> = HashSet::new();
        for k in cells {
            self.parsed.remove(k);
        }
        while let Some(k) = queue.pop() {
            if !gone.insert(k) {
                continue;
            }
            self.memo.borrow_mut().remove(&k);
            if let Some(owners) = self.readers.get(&k) {
                queue.extend(owners.iter().copied());
            }
            queue.extend(
                self.ranges
                    .iter()
                    .filter(|d| d.contains(k))
                    .map(|d| d.owner),
            );
        }
        // the cells that were thrown away are parsed again when read
        for k in &gone {
            if !cells.contains(k) {
                self.parsed.remove(k);
            }
        }
        self.ranges.retain(|d| !gone.contains(&d.owner));
        for k in &gone {
            self.volatile.remove(k);
        }
        for owners in self.readers.values_mut() {
            owners.retain(|o| !gone.contains(o));
        }
    }

    /// Parses the formula of `k` (cached) and records what it reads.
    fn parse_cell(&mut self, wb: &Workbook, k: Key) -> Rc<Parsed> {
        if let Some(p) = self.parsed.get(&k) {
            return p.clone();
        }
        let raw = wb.sheets.get(k.0).map_or("", |s| s.get(k.1, k.2));
        let mut p = Parsed {
            ast: None,
            cells: Vec::new(),
            ranges: Vec::new(),
            volatile: false,
        };
        if let Ok(a) = parse(raw.trim_start_matches('=')) {
            walk(&a, wb, k.0, &mut p);
            p.ast = Some(Rc::new(a));
        }
        for c in &p.cells {
            let owners = self.readers.entry(*c).or_default();
            if !owners.contains(&k) {
                owners.push(k);
            }
        }
        for &(sheet, r0, c0, r1, c1) in &p.ranges {
            self.ranges.push(RangeDep {
                sheet,
                r0,
                c0,
                r1,
                c1,
                owner: k,
            });
        }
        if p.volatile {
            self.volatile.insert(k);
        }
        let p = Rc::new(p);
        self.parsed.insert(k, p.clone());
        p
    }

    /// The formula cells `p` reads that are not calculated yet.
    fn pending(&self, wb: &Workbook, p: &Parsed) -> Vec<Key> {
        let mut out = Vec::new();
        let formula = |k: Key| {
            wb.sheets
                .get(k.0)
                .is_some_and(|s| s.get(k.1, k.2).starts_with('='))
        };
        for &k in &p.cells {
            if formula(k) && !self.memo.borrow().contains_key(&k) {
                out.push(k);
            }
        }
        for &(si, r0, c0, r1, c1) in &p.ranges {
            let Some(sh) = wb.sheets.get(si) else {
                continue;
            };
            let (rows, _) = sh.extent();
            let last = r1.min(rows.saturating_sub(1));
            if r0 > last {
                continue;
            }
            for (&(r, c), raw) in sh.cells.range((r0, 0)..=(last, u32::MAX)) {
                if c >= c0 && c <= c1 && raw.starts_with('=') {
                    let k = (si, r, c);
                    if !self.memo.borrow().contains_key(&k) {
                        out.push(k);
                    }
                }
            }
        }
        out
    }

    /// Calculates `start` after everything it reads.
    fn ensure(&mut self, wb: &Workbook, start: Key) {
        let mut stack: Vec<(Key, bool)> = vec![(start, false)];
        let mut open: HashSet<Key> = HashSet::new();
        while let Some((k, expanded)) = stack.pop() {
            if expanded {
                open.remove(&k);
            }
            if self.memo.borrow().contains_key(&k) {
                continue;
            }
            let p = self.parse_cell(wb, k);
            if !expanded {
                open.insert(k);
                stack.push((k, true));
                for d in self.pending(wb, &p) {
                    if !open.contains(&d) {
                        stack.push((d, false));
                    }
                }
                continue;
            }
            let v = match &p.ast {
                Some(a) => {
                    let mut ev = Eval::new(wb, self.now, Some(&self.memo));
                    ev.visiting.insert(k);
                    ev.ast(k.0, a)
                }
                None => Value::Err("#ERROR!".into()),
            };
            self.calculated += 1;
            self.memo.borrow_mut().insert(k, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(cells: &[(&str, &str)]) -> Workbook {
        let mut w = Workbook::new();
        for (c, v) in cells {
            w.set_ref(0, c, v).unwrap_or_default();
        }
        w
    }

    #[test]
    fn a_long_chain_needs_no_deep_recursion() {
        let mut w = Workbook::new();
        w.sheets[0].set(0, 0, "1");
        for r in 1..50_000 {
            w.sheets[0].set(r, 0, &format!("=A{r}+1"));
        }
        let mut c = Calc::new(45_000.0);
        assert_eq!(c.value(&w, 0, 49_999, 0), Value::Num(50_000.0));
        // a second look costs nothing
        let n = c.calculated;
        assert_eq!(c.value(&w, 0, 49_999, 0), Value::Num(50_000.0));
        assert_eq!(c.calculated, n);
    }

    #[test]
    fn a_change_recalculates_only_what_depends_on_it() {
        let mut w = book(&[
            ("A1", "1"),
            ("A2", "2"),
            ("B1", "=A1*10"),
            ("B2", "=A2*10"),
            ("C1", "=SUM(B1:B2)"),
            ("D1", "=C1+1"),
        ]);
        let mut c = Calc::new(45_000.0);
        assert_eq!(c.value(&w, 0, 0, 3), Value::Num(31.0));
        let n = c.calculated;
        w.sheets[0].set(0, 0, "5");
        c.changed(&[(0, 0, 0)]);
        assert_eq!(c.value(&w, 0, 0, 3), Value::Num(71.0));
        // B1, C1 and D1 again; B2 was not touched
        assert_eq!(c.calculated - n, 3);
    }

    #[test]
    fn a_range_dependency_is_followed() {
        let mut w = book(&[("A1", "1"), ("A2", "2"), ("B1", "=SUM(A1:A5)")]);
        let mut c = Calc::new(45_000.0);
        assert_eq!(c.value(&w, 0, 0, 1), Value::Num(3.0));
        w.sheets[0].set(4, 0, "10");
        c.changed(&[(0, 4, 0)]);
        assert_eq!(c.value(&w, 0, 0, 1), Value::Num(13.0));
    }

    #[test]
    fn a_cycle_is_found_and_resolved_by_an_edit() {
        let mut w = book(&[("A1", "=B1"), ("B1", "=A1"), ("C1", "=A1+1")]);
        let mut c = Calc::new(45_000.0);
        assert_eq!(c.value(&w, 0, 0, 0), Value::Err("#CIRC!".into()));
        assert_eq!(c.value(&w, 0, 0, 1), Value::Err("#CIRC!".into()));
        assert!(c.value(&w, 0, 0, 2).is_err());
        w.sheets[0].set(0, 1, "5");
        c.changed(&[(0, 0, 1)]);
        assert_eq!(c.value(&w, 0, 0, 0), Value::Num(5.0));
        assert_eq!(c.value(&w, 0, 0, 2), Value::Num(6.0));
    }

    #[test]
    fn a_cell_reading_itself_is_a_cycle() {
        let w = book(&[("A1", "=A1+1")]);
        let mut c = Calc::new(45_000.0);
        assert_eq!(c.value(&w, 0, 0, 0), Value::Err("#CIRC!".into()));
    }

    #[test]
    fn other_sheets_and_the_clock() {
        let mut w = book(&[("A1", "=Daten!B2*2"), ("B1", "=TODAY()")]);
        w.set_ref(0, "Daten!B2", "21").unwrap_or_default();
        let mut c = Calc::new(45_000.5);
        assert_eq!(c.value(&w, 0, 0, 0), Value::Num(42.0));
        assert_eq!(c.value(&w, 0, 0, 1), Value::Num(45_000.0));
        c.set_now(45_001.5);
        assert_eq!(c.value(&w, 0, 0, 1), Value::Num(45_001.0));
        w.sheets[1].set(1, 1, "4");
        c.changed(&[(1, 1, 1)]);
        assert_eq!(c.value(&w, 0, 0, 0), Value::Num(8.0));
    }
}
