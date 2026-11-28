//! One worksheet part (`xl/worksheets/sheetN.xml`): cells, column widths, row heights,
//! the frozen pane and the direction.

use super::{attr, local, MAX_CELLS, TOO_LARGE};
use crate::sheet::{fmt_num, parse_cell, Sheet, Value};
use crate::style::CellStyle;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

/// The state while the events of one sheet go by.
struct SheetReader<'a> {
    sh: Sheet,
    ss: &'a [String],
    styles: &'a [CellStyle],
    /// Position of the cell whose content is being read.
    cur: Option<(u32, u32)>,
    typ: String,
    val: String,
    formula: Option<String>,
    in_v: bool,
    in_f: bool,
    in_is_t: bool,
    next_col: u32,
    row_no: u32,
    style_idx: Option<u32>,
}

impl SheetReader<'_> {
    fn row(&mut self, e: &BytesStart) {
        self.row_no = attr(e, b"r")
            .and_then(|v| v.parse::<u32>().ok())
            .map(|v| v.saturating_sub(1))
            .unwrap_or(self.row_no);
        self.next_col = 0;
        if let Some(h) = attr(e, b"ht").and_then(|v| v.parse::<f32>().ok()) {
            self.sh.heights.insert(self.row_no, h);
        }
    }

    fn pane(&mut self, e: &BytesStart) {
        let n = |k: &[u8]| {
            attr(e, k)
                .and_then(|v| v.parse::<f64>().ok())
                .map_or(0, |v| v as u32)
        };
        self.sh.frozen_cols = n(b"xSplit");
        self.sh.frozen_rows = n(b"ySplit");
    }

    fn cols(&mut self, e: &BytesStart) {
        if let (Some(a), Some(b), Some(w)) = (attr(e, b"min"), attr(e, b"max"), attr(e, b"width")) {
            if let (Ok(a), Ok(b), Ok(w)) = (a.parse::<u32>(), b.parse::<u32>(), w.parse::<f32>()) {
                for c in a..=b.min(a + 200) {
                    self.sh.widths.insert(c.saturating_sub(1), w);
                }
            }
        }
    }

    fn cell(&mut self, e: &BytesStart, empty: bool) {
        let pos = attr(e, b"r")
            .and_then(|v| parse_cell(&v))
            .unwrap_or((self.row_no, self.next_col));
        self.next_col = pos.1 + 1;
        self.typ = attr(e, b"t").unwrap_or_default();
        self.style_idx = attr(e, b"s").and_then(|v| v.parse().ok());
        self.val.clear();
        self.formula = None;
        self.cur = if empty { None } else { Some(pos) };
    }

    fn open(&mut self, e: &BytesStart, empty: bool) {
        match local(e.name().as_ref()) {
            b"row" => self.row(e),
            b"sheetView" => {
                self.sh.rtl =
                    matches!(attr(e, b"rightToLeft").as_deref(), Some("1") | Some("true"));
            }
            b"pane" if attr(e, b"state").as_deref() == Some("frozen") => self.pane(e),
            b"c" => self.cell(e, empty),
            b"v" if !empty => self.in_v = true,
            b"f" if !empty => {
                self.in_f = true;
                self.formula = Some(String::new());
            }
            b"t" if !empty => self.in_is_t = true,
            b"col" => self.cols(e),
            _ => {}
        }
    }

    fn text(&mut self, s: &str) {
        if self.in_v || self.in_is_t {
            self.val.push_str(s);
        } else if self.in_f {
            if let Some(f) = self.formula.as_mut() {
                f.push_str(s);
            }
        }
    }

    /// The content of the finished cell as the raw text of a sheet cell.
    fn raw(&self) -> String {
        let typ = self.typ.as_str();
        let raw = match (&self.formula, typ) {
            (Some(f), _) if !f.is_empty() => format!("={f}"),
            (_, "s") => self
                .val
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|i| self.ss.get(i).cloned())
                .unwrap_or_default(),
            (_, "b") => {
                if self.val.trim() == "1" {
                    "TRUE".into()
                } else {
                    "FALSE".into()
                }
            }
            (_, "str") | (_, "inlineStr") => self.val.clone(),
            // an error value typed into a cell: kept as the error it is
            (_, "e") => format!("={}", self.val.trim()),
            _ => match self.val.trim().parse::<f64>() {
                Ok(n) => fmt_num(n),
                Err(_) => self.val.clone(),
            },
        };
        // text that looks like a number or a formula stays text
        let looks_typed =
            raw.starts_with('=') || crate::sheet::literal(&raw) != Value::Text(raw.clone());
        if matches!(typ, "s" | "str" | "inlineStr")
            && self.formula.is_none()
            && looks_typed
            && !raw.is_empty()
        {
            format!("'{raw}")
        } else {
            raw
        }
    }

    fn end_cell(&mut self) -> Result<(), String> {
        let Some((r, c)) = self.cur.take() else {
            return Ok(());
        };
        let raw = self.raw();
        self.sh.set(r, c, &raw);
        if self.sh.cells.len() > MAX_CELLS {
            return Err(TOO_LARGE.into());
        }
        let style = self.style_idx.and_then(|i| self.styles.get(i as usize));
        if let Some(st) = style.filter(|st| !st.is_default()) {
            self.sh.styles.insert((r, c), st.clone());
        }
        Ok(())
    }

    fn close(&mut self, name: &[u8]) -> Result<(), String> {
        match name {
            b"v" => self.in_v = false,
            b"f" => self.in_f = false,
            b"t" => self.in_is_t = false,
            b"row" => self.row_no += 1,
            b"c" => return self.end_cell(),
            _ => {}
        }
        Ok(())
    }
}

pub(super) fn read_sheet(
    x: &[u8],
    name: &str,
    ss: &[String],
    styles: &[CellStyle],
) -> Result<Sheet, String> {
    let mut rd = SheetReader {
        sh: Sheet::new(name),
        ss,
        styles,
        cur: None,
        typ: String::new(),
        val: String::new(),
        formula: None,
        in_v: false,
        in_f: false,
        in_is_t: false,
        next_col: 0,
        row_no: 0,
        style_idx: None,
    };
    let mut r = Reader::from_reader(x);
    let mut buf = Vec::new();
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => rd.open(e, false),
            Ok(Event::Empty(ref e)) => rd.open(e, true),
            Ok(Event::Text(t)) => {
                rd.text(&t.unescape().map(|c| c.into_owned()).unwrap_or_default())
            }
            Ok(Event::End(ref e)) => rd.close(local(e.name().as_ref()))?,
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("{name}: {e}")),
            _ => {}
        }
        buf.clear();
    }
    Ok(rd.sh)
}
