//! Tabellenkalkulation: Arbeitsmappe, Zellen, Formel-Engine (Excel-kompatible Grundfunktionen).
//!
//! Eine Zelle speichert ihren Rohinhalt (`raw`): Zahl, Text oder Formel (`=…`). Werte werden bei Bedarf
//! berechnet (`Workbook::value`), Zirkelbezüge ergeben `#ZIRKEL!`/`#CIRC!`.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Empty,
    Num(f64),
    Text(String),
    Bool(bool),
    Err(String),
}

impl Value {
    pub fn num(&self) -> Result<f64, Value> {
        match self {
            Value::Empty => Ok(0.0),
            Value::Num(n) => Ok(*n),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Text(t) => t
                .trim()
                .replace(',', ".")
                .parse::<f64>()
                .map_err(|_| Value::Err("#VALUE!".into())),
            Value::Err(_) => Err(self.clone()),
        }
    }
    pub fn text(&self) -> String {
        match self {
            Value::Empty => String::new(),
            Value::Num(n) => fmt_num(*n),
            Value::Text(t) => t.clone(),
            Value::Bool(b) => {
                if *b {
                    "TRUE".into()
                } else {
                    "FALSE".into()
                }
            }
            Value::Err(e) => e.clone(),
        }
    }
    pub fn truthy(&self) -> Result<bool, Value> {
        match self {
            Value::Bool(b) => Ok(*b),
            Value::Text(t) if t.eq_ignore_ascii_case("true") => Ok(true),
            Value::Text(t) if t.eq_ignore_ascii_case("false") => Ok(false),
            _ => Ok(self.num()? != 0.0),
        }
    }
    pub fn is_err(&self) -> bool {
        matches!(self, Value::Err(_))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text())
    }
}

/// Zahl ohne unnötige Nachkommastellen ausgeben (wie eine Tabellenkalkulation)
pub fn fmt_num(n: f64) -> String {
    if !n.is_finite() {
        return "#NUM!".into();
    }
    if n == n.trunc() && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let s = format!("{:.10}", n);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

// ------------------------------------------------------------------ Adressen

/// Spaltennummer (0 = A) → Buchstaben
pub fn col_name(mut c: u32) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

pub fn cell_name(r: u32, c: u32) -> String {
    format!("{}{}", col_name(c), r + 1)
}

/// "B12" / "$B$12" → (Zeile 11, Spalte 1)
pub fn parse_cell(s: &str) -> Option<(u32, u32)> {
    let s = s.trim().replace('$', "");
    let b = s.as_bytes();
    let mut i = 0;
    let mut col: u32 = 0;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        col = col
            .checked_mul(26)?
            .checked_add((b[i].to_ascii_uppercase() - b'A') as u32 + 1)?;
        i += 1;
    }
    if i == 0 || i == b.len() || i > 3 {
        return None;
    }
    let row: u32 = s[i..].parse().ok()?;
    if row == 0 || row > 1_048_576 {
        return None;
    }
    Some((row - 1, col - 1))
}

/// "A1:C5" / "A1" (optional mit "Blatt!") → (Blatt, (r0,c0), (r1,c1))
pub fn parse_range(s: &str) -> Option<(Option<String>, (u32, u32), (u32, u32))> {
    let (sheet, rest) = match s.rsplit_once('!') {
        Some((sh, r)) => (Some(sh.trim_matches('\'').to_string()), r),
        None => (None, s),
    };
    let (a, b) = match rest.split_once(':') {
        Some((a, b)) => (parse_cell(a)?, parse_cell(b)?),
        None => {
            let a = parse_cell(rest)?;
            (a, a)
        }
    };
    Some((
        sheet,
        (a.0.min(b.0), a.1.min(b.1)),
        (a.0.max(b.0), a.1.max(b.1)),
    ))
}

// ------------------------------------------------------------------ Modell

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub name: String,
    /// (Zeile, Spalte) → Rohinhalt
    pub cells: BTreeMap<(u32, u32), String>,
    /// Spaltenbreiten in Zeichen (optional)
    pub widths: BTreeMap<u32, f32>,
}

impl Sheet {
    pub fn new(name: &str) -> Self {
        Sheet {
            name: name.into(),
            ..Default::default()
        }
    }
    pub fn get(&self, r: u32, c: u32) -> &str {
        self.cells.get(&(r, c)).map(|s| s.as_str()).unwrap_or("")
    }
    pub fn set(&mut self, r: u32, c: u32, raw: &str) {
        if raw.is_empty() {
            self.cells.remove(&(r, c));
        } else {
            self.cells.insert((r, c), raw.to_string());
        }
    }
    /// Benutzter Bereich: (Zeilen, Spalten)
    pub fn extent(&self) -> (u32, u32) {
        let mut rows = 0;
        let mut cols = 0;
        for &(r, c) in self.cells.keys() {
            rows = rows.max(r + 1);
            cols = cols.max(c + 1);
        }
        (rows, cols)
    }
    pub fn insert_row(&mut self, at: u32) {
        let old = std::mem::take(&mut self.cells);
        for ((r, c), v) in old {
            self.cells.insert((if r >= at { r + 1 } else { r }, c), v);
        }
    }
    pub fn delete_row(&mut self, at: u32) {
        let old = std::mem::take(&mut self.cells);
        for ((r, c), v) in old {
            if r == at {
                continue;
            }
            self.cells.insert((if r > at { r - 1 } else { r }, c), v);
        }
    }
    pub fn insert_col(&mut self, at: u32) {
        let old = std::mem::take(&mut self.cells);
        for ((r, c), v) in old {
            self.cells.insert((r, if c >= at { c + 1 } else { c }), v);
        }
    }
    pub fn delete_col(&mut self, at: u32) {
        let old = std::mem::take(&mut self.cells);
        for ((r, c), v) in old {
            if c == at {
                continue;
            }
            self.cells.insert((r, if c > at { c - 1 } else { c }), v);
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
}

impl Workbook {
    pub fn new() -> Self {
        Workbook {
            sheets: vec![Sheet::new("Sheet1")],
        }
    }

    pub fn sheet_index(&self, name: &str) -> Option<usize> {
        self.sheets
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// Wert einer Zelle (Formeln werden berechnet)
    pub fn value(&self, sheet: usize, r: u32, c: u32) -> Value {
        let mut ev = Eval {
            wb: self,
            visiting: HashSet::new(),
            depth: 0,
        };
        ev.cell(sheet, r, c)
    }

    /// Formel oder Ausdruck im Kontext eines Blatts auswerten (ohne Zelle zu verändern)
    pub fn eval_formula(&self, sheet: usize, f: &str) -> Value {
        let mut ev = Eval {
            wb: self,
            visiting: HashSet::new(),
            depth: 0,
        };
        ev.formula(sheet, f.trim_start_matches('='))
    }

    /// Zelle per Text setzen: "Blatt!B3" oder "B3" (dann erstes/angegebenes Blatt)
    pub fn set_ref(
        &mut self,
        default_sheet: usize,
        cell: &str,
        raw: &str,
    ) -> Result<(usize, u32, u32), String> {
        let (sh, (r, c), _) =
            parse_range(cell).ok_or_else(|| format!("bad cell reference: {cell}"))?;
        let si = match sh {
            Some(n) => match self.sheet_index(&n) {
                Some(i) => i,
                None => {
                    self.sheets.push(Sheet::new(&n));
                    self.sheets.len() - 1
                }
            },
            None => default_sheet.min(self.sheets.len().saturating_sub(1)),
        };
        if self.sheets.is_empty() {
            self.sheets.push(Sheet::new("Sheet1"));
        }
        self.sheets[si].set(r, c, raw);
        Ok((si, r, c))
    }

    /// Bereich als Tabelle aus Werten (für den Agenten und CSV)
    pub fn range_values(&self, sheet: usize, a: (u32, u32), b: (u32, u32)) -> Vec<Vec<Value>> {
        (a.0..=b.0)
            .map(|r| (a.1..=b.1).map(|c| self.value(sheet, r, c)).collect())
            .collect()
    }
}

// ------------------------------------------------------------------ Formeln

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ref(String),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let ch: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < ch.len() {
        let c = ch[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && i + 1 < ch.len() && ch[i + 1].is_ascii_digit()) {
            let st = i;
            while i < ch.len() && (ch[i].is_ascii_digit() || ch[i] == '.') {
                i += 1;
            }
            if i < ch.len()
                && (ch[i] == 'e' || ch[i] == 'E')
                && i + 1 < ch.len()
                && (ch[i + 1].is_ascii_digit() || ch[i + 1] == '-' || ch[i + 1] == '+')
            {
                i += 2;
                while i < ch.len() && ch[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let t: String = ch[st..i].iter().collect();
            out.push(Tok::Num(t.parse().map_err(|_| "#VALUE!".to_string())?));
            continue;
        }
        if c == '"' {
            i += 1;
            let mut t = String::new();
            while i < ch.len() {
                if ch[i] == '"' {
                    if i + 1 < ch.len() && ch[i + 1] == '"' {
                        t.push('"');
                        i += 2;
                        continue;
                    }
                    break;
                }
                t.push(ch[i]);
                i += 1;
            }
            i += 1;
            out.push(Tok::Str(t));
            continue;
        }
        if c.is_alphabetic() || c == '$' || c == '_' || c == '\'' {
            let st = i;
            if c == '\'' {
                // 'Blatt Name'!A1
                i += 1;
                while i < ch.len() && ch[i] != '\'' {
                    i += 1;
                }
                i += 1;
            }
            while i < ch.len()
                && (ch[i].is_alphanumeric()
                    || ch[i] == '$'
                    || ch[i] == '_'
                    || ch[i] == '.'
                    || ch[i] == '!'
                    || ch[i] == ':')
            {
                if ch[i] == '!' && i + 1 < ch.len() && ch[i + 1] == '\'' {
                    break;
                }
                i += 1;
            }
            let t: String = ch[st..i].iter().collect();
            if i < ch.len() && ch[i] == '(' && !t.contains('!') {
                out.push(Tok::Ident(t.to_ascii_uppercase()));
            } else if t.eq_ignore_ascii_case("TRUE") || t.eq_ignore_ascii_case("FALSE") {
                out.push(Tok::Ident(t.to_ascii_uppercase()));
            } else {
                out.push(Tok::Ref(t));
            }
            continue;
        }
        let two: String = ch[i..(i + 2).min(ch.len())].iter().collect();
        let op2 = match two.as_str() {
            "<=" => Some("<="),
            ">=" => Some(">="),
            "<>" => Some("<>"),
            _ => None,
        };
        if let Some(o) = op2 {
            out.push(Tok::Op(o));
            i += 2;
            continue;
        }
        let t = match c {
            '+' => Tok::Op("+"),
            '-' => Tok::Op("-"),
            '*' => Tok::Op("*"),
            '/' => Tok::Op("/"),
            '^' => Tok::Op("^"),
            '&' => Tok::Op("&"),
            '=' => Tok::Op("="),
            '<' => Tok::Op("<"),
            '>' => Tok::Op(">"),
            '%' => Tok::Op("%"),
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            ',' | ';' => Tok::Comma,
            _ => return Err(format!("#NAME? ({c})")),
        };
        out.push(t);
        i += 1;
    }
    Ok(out)
}

#[derive(Clone, Debug)]
enum Ast {
    Num(f64),
    Str(String),
    Bool(bool),
    Ref(Option<String>, (u32, u32), (u32, u32), bool),
    Un(&'static str, Box<Ast>),
    Bin(&'static str, Box<Ast>, Box<Ast>),
    Call(String, Vec<Ast>),
}

struct Parser {
    t: Vec<Tok>,
    i: usize,
}

fn prec(op: &str) -> u8 {
    match op {
        "=" | "<>" | "<" | ">" | "<=" | ">=" => 1,
        "&" => 2,
        "+" | "-" => 3,
        "*" | "/" => 4,
        "^" => 5,
        _ => 0,
    }
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.t.get(self.i).cloned();
        self.i += 1;
        t
    }
    fn expr(&mut self, min: u8) -> Result<Ast, String> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op(o)) if prec(o) >= min && prec(o) > 0 => *o,
                _ => break,
            };
            let p = prec(op);
            self.i += 1;
            let rhs = if op == "^" {
                self.expr(p)?
            } else {
                self.expr(p + 1)?
            };
            lhs = Ast::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }
    fn unary(&mut self) -> Result<Ast, String> {
        if let Some(Tok::Op(o)) = self.peek() {
            if *o == "-" || *o == "+" {
                let o = *o;
                self.i += 1;
                let e = self.unary()?;
                return Ok(Ast::Un(o, Box::new(e)));
            }
        }
        let mut e = self.atom()?;
        while let Some(Tok::Op("%")) = self.peek() {
            self.i += 1;
            e = Ast::Bin("/", Box::new(e), Box::new(Ast::Num(100.0)));
        }
        Ok(e)
    }
    fn atom(&mut self) -> Result<Ast, String> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Ast::Num(n)),
            Some(Tok::Str(s)) => Ok(Ast::Str(s)),
            Some(Tok::LParen) => {
                let e = self.expr(1)?;
                match self.next() {
                    Some(Tok::RParen) => Ok(e),
                    _ => Err("#ERROR! missing )".into()),
                }
            }
            Some(Tok::Ident(n)) if n == "TRUE" || n == "FALSE" => {
                if let Some(Tok::LParen) = self.peek() {
                    self.i += 1;
                    let _ = self.next();
                }
                Ok(Ast::Bool(n == "TRUE"))
            }
            Some(Tok::Ident(n)) => {
                self.i += 1; // (
                let mut args = Vec::new();
                if let Some(Tok::RParen) = self.peek() {
                    self.i += 1;
                    return Ok(Ast::Call(n, args));
                }
                loop {
                    args.push(self.expr(1)?);
                    match self.next() {
                        Some(Tok::Comma) => continue,
                        Some(Tok::RParen) => break,
                        _ => return Err("#ERROR! missing )".into()),
                    }
                }
                Ok(Ast::Call(n, args))
            }
            Some(Tok::Ref(r)) => {
                let (sh, a, b) = parse_range(&r).ok_or_else(|| format!("#NAME? {r}"))?;
                let is_range = r.contains(':');
                Ok(Ast::Ref(sh, a, b, is_range))
            }
            t => Err(format!("#ERROR! unexpected {t:?}")),
        }
    }
}

fn parse(f: &str) -> Result<Ast, String> {
    let t = lex(f)?;
    let mut p = Parser { t, i: 0 };
    let e = p.expr(1)?;
    if p.i < p.t.len() {
        return Err("#ERROR! trailing input".into());
    }
    Ok(e)
}

/// Prüft eine Formel auf Syntaxfehler (für die Eingabezeile)
pub fn check_formula(f: &str) -> Result<(), String> {
    parse(f.trim_start_matches('=')).map(|_| ())
}

/// Liste der unterstützten Funktionen (Hilfe in der App, Agent)
pub const FUNCTIONS: &[&str] = &[
    "SUM",
    "AVERAGE",
    "MIN",
    "MAX",
    "COUNT",
    "COUNTA",
    "COUNTBLANK",
    "PRODUCT",
    "MEDIAN",
    "IF",
    "IFERROR",
    "AND",
    "OR",
    "NOT",
    "ROUND",
    "ROUNDUP",
    "ROUNDDOWN",
    "INT",
    "ABS",
    "SQRT",
    "POWER",
    "MOD",
    "PI",
    "LEN",
    "UPPER",
    "LOWER",
    "TRIM",
    "LEFT",
    "RIGHT",
    "MID",
    "CONCAT",
    "CONCATENATE",
    "TEXTJOIN",
    "SUMIF",
    "COUNTIF",
    "AVERAGEIF",
    "VLOOKUP",
    "HLOOKUP",
    "INDEX",
    "MATCH",
    "ISNUMBER",
    "ISBLANK",
    "ISTEXT",
    "VALUE",
    "TEXT",
    "STDEV",
    "VAR",
];

struct Eval<'a> {
    wb: &'a Workbook,
    visiting: HashSet<(usize, u32, u32)>,
    depth: u32,
}

/// Argument: einzelner Wert oder Bereich (Zeilen × Spalten)
enum Arg {
    One(Value),
    Range(Vec<Vec<Value>>),
}

impl Arg {
    fn flat(&self) -> Vec<Value> {
        match self {
            Arg::One(v) => vec![v.clone()],
            Arg::Range(r) => r.iter().flatten().cloned().collect(),
        }
    }
    fn one(self) -> Value {
        match self {
            Arg::One(v) => v,
            Arg::Range(r) => r
                .into_iter()
                .next()
                .and_then(|row| row.into_iter().next())
                .unwrap_or(Value::Empty),
        }
    }
}

fn verr(s: &str) -> Value {
    Value::Err(s.into())
}

/// Kriterium wie in SUMIF/COUNTIF: ">5", "<>x", "=a", "Text", 3
fn crit_match(v: &Value, crit: &Value) -> bool {
    let cs = crit.text();
    let (op, rhs) = if let Some(r) = cs.strip_prefix(">=") {
        (">=", r)
    } else if let Some(r) = cs.strip_prefix("<=") {
        ("<=", r)
    } else if let Some(r) = cs.strip_prefix("<>") {
        ("<>", r)
    } else if let Some(r) = cs.strip_prefix('>') {
        (">", r)
    } else if let Some(r) = cs.strip_prefix('<') {
        ("<", r)
    } else if let Some(r) = cs.strip_prefix('=') {
        ("=", r)
    } else {
        ("=", cs.as_str())
    };
    if let (Ok(a), Ok(b)) = (v.num(), rhs.trim().parse::<f64>()) {
        if !matches!(v, Value::Empty) || op != "=" {
            return match op {
                ">=" => a >= b,
                "<=" => a <= b,
                "<>" => a != b,
                ">" => a > b,
                "<" => a < b,
                _ => a == b,
            };
        }
    }
    let a = v.text().to_lowercase();
    let b = rhs.to_lowercase();
    match op {
        "<>" => a != b,
        "=" => {
            if b.contains('*') {
                let parts: Vec<&str> = b.split('*').collect();
                let mut pos = 0;
                for (i, p) in parts.iter().enumerate() {
                    if p.is_empty() {
                        continue;
                    }
                    match a[pos..].find(p) {
                        Some(k) if i > 0 || k == 0 => pos += k + p.len(),
                        _ => return false,
                    }
                }
                parts
                    .last()
                    .map_or(true, |l| l.is_empty() || a.ends_with(l))
            } else {
                a == b
            }
        }
        ">" => a > b,
        "<" => a < b,
        ">=" => a >= b,
        _ => a <= b,
    }
}

impl Eval<'_> {
    fn cell(&mut self, sheet: usize, r: u32, c: u32) -> Value {
        let Some(sh) = self.wb.sheets.get(sheet) else {
            return verr("#REF!");
        };
        let raw = sh.get(r, c);
        if raw.is_empty() {
            return Value::Empty;
        }
        if let Some(f) = raw.strip_prefix('=') {
            if !self.visiting.insert((sheet, r, c)) || self.depth > 200 {
                return verr("#CIRC!");
            }
            self.depth += 1;
            let v = self.formula(sheet, f);
            self.depth -= 1;
            self.visiting.remove(&(sheet, r, c));
            return v;
        }
        literal(raw)
    }

    fn formula(&mut self, sheet: usize, f: &str) -> Value {
        match parse(f) {
            Ok(a) => self.eval(sheet, &a).one(),
            Err(e) => Value::Err(e.split(' ').next().unwrap_or("#ERROR!").to_string()),
        }
    }

    fn sheet_of(&self, cur: usize, name: &Option<String>) -> Option<usize> {
        match name {
            Some(n) => self.wb.sheet_index(n),
            None => Some(cur),
        }
    }

    fn eval(&mut self, sheet: usize, a: &Ast) -> Arg {
        match a {
            Ast::Num(n) => Arg::One(Value::Num(*n)),
            Ast::Str(s) => Arg::One(Value::Text(s.clone())),
            Ast::Bool(b) => Arg::One(Value::Bool(*b)),
            Ast::Ref(sh, p, q, is_range) => {
                let Some(si) = self.sheet_of(sheet, sh) else {
                    return Arg::One(verr("#REF!"));
                };
                if !is_range {
                    return Arg::One(self.cell(si, p.0, p.1));
                }
                // große Bereiche (A:A) auf den benutzten Bereich begrenzen
                let (er, ec) = self.wb.sheets[si].extent();
                let r1 = q.0.min(er.saturating_sub(1).max(p.0));
                let c1 = q.1.min(ec.saturating_sub(1).max(p.1));
                Arg::Range(
                    (p.0..=r1)
                        .map(|r| (p.1..=c1).map(|c| self.cell(si, r, c)).collect())
                        .collect(),
                )
            }
            Ast::Un(o, e) => {
                let v = self.eval(sheet, e).one();
                match v.num() {
                    Ok(n) => Arg::One(Value::Num(if *o == "-" { -n } else { n })),
                    Err(e) => Arg::One(e),
                }
            }
            Ast::Bin(o, l, r) => {
                let l = self.eval(sheet, l).one();
                let r = self.eval(sheet, r).one();
                Arg::One(binop(o, &l, &r))
            }
            Ast::Call(n, args) => Arg::One(self.call(sheet, n, args)),
        }
    }

    fn nums(&mut self, sheet: usize, args: &[Ast]) -> Result<Vec<f64>, Value> {
        let mut v = Vec::new();
        for a in args {
            match self.eval(sheet, a) {
                Arg::One(x) => match x {
                    Value::Empty => {}
                    Value::Err(_) => return Err(x),
                    _ => v.push(x.num()?),
                },
                Arg::Range(r) => {
                    for x in r.into_iter().flatten() {
                        match x {
                            Value::Num(n) => v.push(n),
                            Value::Err(_) => return Err(x),
                            _ => {}
                        }
                    }
                }
            }
        }
        Ok(v)
    }

    fn call(&mut self, sheet: usize, name: &str, args: &[Ast]) -> Value {
        macro_rules! need {
            ($n:expr) => {
                if args.len() < $n {
                    return verr("#N/A");
                }
            };
        }
        macro_rules! num {
            ($i:expr) => {
                match self.eval(sheet, &args[$i]).one().num() {
                    Ok(n) => n,
                    Err(e) => return e,
                }
            };
        }
        macro_rules! txt {
            ($i:expr) => {{
                let v = self.eval(sheet, &args[$i]).one();
                if v.is_err() {
                    return v;
                }
                v.text()
            }};
        }
        macro_rules! nums {
            () => {
                match self.nums(sheet, args) {
                    Ok(v) => v,
                    Err(e) => return e,
                }
            };
        }
        match name {
            "SUM" => Value::Num(nums!().iter().sum()),
            "PRODUCT" => Value::Num(nums!().iter().product()),
            "AVERAGE" => {
                let v = nums!();
                if v.is_empty() {
                    verr("#DIV/0!")
                } else {
                    Value::Num(v.iter().sum::<f64>() / v.len() as f64)
                }
            }
            "MIN" => Value::Num(nums!().into_iter().fold(f64::INFINITY, f64::min)).fix_inf(),
            "MAX" => Value::Num(nums!().into_iter().fold(f64::NEG_INFINITY, f64::max)).fix_inf(),
            "COUNT" => Value::Num(nums!().len() as f64),
            "MEDIAN" => {
                let mut v = nums!();
                if v.is_empty() {
                    return verr("#NUM!");
                }
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let m = v.len() / 2;
                Value::Num(if v.len() % 2 == 1 {
                    v[m]
                } else {
                    (v[m - 1] + v[m]) / 2.0
                })
            }
            "STDEV" | "VAR" => {
                let v = nums!();
                if v.len() < 2 {
                    return verr("#DIV/0!");
                }
                let mean = v.iter().sum::<f64>() / v.len() as f64;
                let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (v.len() - 1) as f64;
                Value::Num(if name == "VAR" { var } else { var.sqrt() })
            }
            "COUNTA" | "COUNTBLANK" => {
                let mut n = 0;
                let mut blank = 0;
                for a in args {
                    for v in self.eval(sheet, a).flat() {
                        if matches!(v, Value::Empty) {
                            blank += 1
                        } else {
                            n += 1
                        }
                    }
                }
                Value::Num(if name == "COUNTA" { n } else { blank } as f64)
            }
            "IF" => {
                need!(2);
                let c = self.eval(sheet, &args[0]).one();
                match c.truthy() {
                    Ok(true) => self.eval(sheet, &args[1]).one(),
                    Ok(false) => {
                        if args.len() > 2 {
                            self.eval(sheet, &args[2]).one()
                        } else {
                            Value::Bool(false)
                        }
                    }
                    Err(e) => e,
                }
            }
            "IFERROR" => {
                need!(2);
                let v = self.eval(sheet, &args[0]).one();
                if v.is_err() {
                    self.eval(sheet, &args[1]).one()
                } else {
                    v
                }
            }
            "AND" | "OR" => {
                let mut acc = name == "AND";
                for a in args {
                    for v in self.eval(sheet, a).flat() {
                        if matches!(v, Value::Empty) {
                            continue;
                        }
                        match v.truthy() {
                            Ok(b) => {
                                if name == "AND" {
                                    acc &= b
                                } else {
                                    acc |= b
                                }
                            }
                            Err(e) => return e,
                        }
                    }
                }
                Value::Bool(acc)
            }
            "NOT" => {
                need!(1);
                match self.eval(sheet, &args[0]).one().truthy() {
                    Ok(b) => Value::Bool(!b),
                    Err(e) => e,
                }
            }
            "ROUND" | "ROUNDUP" | "ROUNDDOWN" => {
                need!(1);
                let x = num!(0);
                let d = if args.len() > 1 { num!(1) } else { 0.0 };
                let k = 10f64.powi(d as i32);
                let y = x * k;
                let y = match name {
                    "ROUND" => (y.abs() * (1.0 + 4.0 * f64::EPSILON) + 0.5)
                        .floor()
                        .copysign(y),
                    "ROUNDUP" => (y.abs() - 1e-9).ceil().copysign(y),
                    _ => (y.abs() + 1e-9).floor().copysign(y),
                };
                Value::Num(y / k)
            }
            "INT" => {
                need!(1);
                Value::Num(num!(0).floor())
            }
            "ABS" => {
                need!(1);
                Value::Num(num!(0).abs())
            }
            "SQRT" => {
                need!(1);
                let x = num!(0);
                if x < 0.0 {
                    verr("#NUM!")
                } else {
                    Value::Num(x.sqrt())
                }
            }
            "POWER" => {
                need!(2);
                Value::Num(num!(0).powf(num!(1)))
            }
            "MOD" => {
                need!(2);
                let (a, b) = (num!(0), num!(1));
                if b == 0.0 {
                    verr("#DIV/0!")
                } else {
                    Value::Num(a - b * (a / b).floor())
                }
            }
            "PI" => Value::Num(std::f64::consts::PI),
            "LEN" => {
                need!(1);
                Value::Num(txt!(0).chars().count() as f64)
            }
            "UPPER" => {
                need!(1);
                Value::Text(txt!(0).to_uppercase())
            }
            "LOWER" => {
                need!(1);
                Value::Text(txt!(0).to_lowercase())
            }
            "TRIM" => {
                need!(1);
                Value::Text(txt!(0).split_whitespace().collect::<Vec<_>>().join(" "))
            }
            "LEFT" | "RIGHT" => {
                need!(1);
                let s: Vec<char> = txt!(0).chars().collect();
                let n = if args.len() > 1 { num!(1) as usize } else { 1 }.min(s.len());
                Value::Text(if name == "LEFT" {
                    s[..n].iter().collect()
                } else {
                    s[s.len() - n..].iter().collect()
                })
            }
            "MID" => {
                need!(3);
                let s: Vec<char> = txt!(0).chars().collect();
                let st = (num!(1) as usize).max(1) - 1;
                let n = num!(2) as usize;
                Value::Text(s.iter().skip(st).take(n).collect())
            }
            "CONCAT" | "CONCATENATE" => {
                let mut s = String::new();
                for a in args {
                    for v in self.eval(sheet, a).flat() {
                        if v.is_err() {
                            return v;
                        }
                        s.push_str(&v.text());
                    }
                }
                Value::Text(s)
            }
            "TEXTJOIN" => {
                need!(3);
                let sep = txt!(0);
                let skip = match self.eval(sheet, &args[1]).one().truthy() {
                    Ok(b) => b,
                    Err(e) => return e,
                };
                let mut parts = Vec::new();
                for a in &args[2..] {
                    for v in self.eval(sheet, a).flat() {
                        if skip && matches!(v, Value::Empty) {
                            continue;
                        }
                        parts.push(v.text());
                    }
                }
                Value::Text(parts.join(&sep))
            }
            "SUMIF" | "COUNTIF" | "AVERAGEIF" => {
                need!(2);
                let range = self.eval(sheet, &args[0]).flat();
                let crit = self.eval(sheet, &args[1]).one();
                let sumr = if args.len() > 2 {
                    self.eval(sheet, &args[2]).flat()
                } else {
                    range.clone()
                };
                let mut n = 0.0;
                let mut s = 0.0;
                for (i, v) in range.iter().enumerate() {
                    if crit_match(v, &crit) {
                        n += 1.0;
                        if let Some(Value::Num(x)) = sumr.get(i) {
                            s += x;
                        }
                    }
                }
                match name {
                    "COUNTIF" => Value::Num(n),
                    "SUMIF" => Value::Num(s),
                    _ => {
                        if n == 0.0 {
                            verr("#DIV/0!")
                        } else {
                            Value::Num(s / n)
                        }
                    }
                }
            }
            "VLOOKUP" | "HLOOKUP" => {
                need!(3);
                let key = self.eval(sheet, &args[0]).one();
                let Arg::Range(t) = self.eval(sheet, &args[1]) else {
                    return verr("#N/A");
                };
                let idx = num!(2) as usize;
                let exact = if args.len() > 3 {
                    !self.eval(sheet, &args[3]).one().truthy().unwrap_or(true)
                } else {
                    false
                };
                let t: Vec<Vec<Value>> = if name == "HLOOKUP" {
                    let w = t.first().map_or(0, |r| r.len());
                    (0..w)
                        .map(|c| t.iter().map(|r| r[c].clone()).collect())
                        .collect()
                } else {
                    t
                };
                if idx == 0 {
                    return verr("#VALUE!");
                }
                let mut hit: Option<usize> = None;
                for (i, row) in t.iter().enumerate() {
                    let k = &row[0];
                    if eq_val(k, &key) {
                        hit = Some(i);
                        break;
                    }
                    if !exact {
                        if let (Ok(a), Ok(b)) = (k.num(), key.num()) {
                            if matches!(k, Value::Num(_)) && a <= b {
                                hit = Some(i);
                            }
                        }
                    }
                }
                match hit {
                    Some(i) => t[i].get(idx - 1).cloned().unwrap_or_else(|| verr("#REF!")),
                    None => verr("#N/A"),
                }
            }
            "INDEX" => {
                need!(2);
                let Arg::Range(t) = self.eval(sheet, &args[0]) else {
                    return verr("#REF!");
                };
                let r = num!(1) as usize;
                let c = if args.len() > 2 { num!(2) as usize } else { 1 };
                let (r, c) = if t.len() == 1 && args.len() == 2 {
                    (1, r)
                } else {
                    (r, c)
                };
                t.get(r.wrapping_sub(1))
                    .and_then(|row| row.get(c.wrapping_sub(1)))
                    .cloned()
                    .unwrap_or_else(|| verr("#REF!"))
            }
            "MATCH" => {
                need!(2);
                let key = self.eval(sheet, &args[0]).one();
                let v = self.eval(sheet, &args[1]).flat();
                match v.iter().position(|x| eq_val(x, &key)) {
                    Some(i) => Value::Num((i + 1) as f64),
                    None => verr("#N/A"),
                }
            }
            "ISNUMBER" => {
                need!(1);
                Value::Bool(matches!(self.eval(sheet, &args[0]).one(), Value::Num(_)))
            }
            "ISBLANK" => {
                need!(1);
                Value::Bool(matches!(self.eval(sheet, &args[0]).one(), Value::Empty))
            }
            "ISTEXT" => {
                need!(1);
                Value::Bool(matches!(self.eval(sheet, &args[0]).one(), Value::Text(_)))
            }
            "VALUE" => {
                need!(1);
                match Value::Text(txt!(0)).num() {
                    Ok(n) => Value::Num(n),
                    Err(e) => e,
                }
            }
            "TEXT" => {
                need!(1);
                let x = num!(0);
                let fmt = if args.len() > 1 {
                    txt!(1)
                } else {
                    String::new()
                };
                let dec = fmt
                    .split_once('.')
                    .map(|(_, d)| d.chars().filter(|c| *c == '0' || *c == '#').count())
                    .unwrap_or(0);
                let pct = fmt.ends_with('%');
                let x = if pct { x * 100.0 } else { x };
                Value::Text(format!("{:.*}{}", dec, x, if pct { "%" } else { "" }))
            }
            _ => verr("#NAME?"),
        }
    }
}

trait FixInf {
    fn fix_inf(self) -> Value;
}
impl FixInf for Value {
    fn fix_inf(self) -> Value {
        match self {
            Value::Num(n) if n.is_infinite() => Value::Num(0.0),
            v => v,
        }
    }
}

fn eq_val(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => (x - y).abs() < 1e-12,
        _ => a.text().to_lowercase() == b.text().to_lowercase(),
    }
}

fn binop(o: &str, l: &Value, r: &Value) -> Value {
    if let Value::Err(_) = l {
        return l.clone();
    }
    if let Value::Err(_) = r {
        return r.clone();
    }
    match o {
        "&" => Value::Text(l.text() + &r.text()),
        "=" | "<>" | "<" | ">" | "<=" | ">=" => {
            let ord = match (l.num(), r.num(), l, r) {
                (
                    Ok(a),
                    Ok(b),
                    Value::Num(_) | Value::Empty | Value::Bool(_),
                    Value::Num(_) | Value::Empty | Value::Bool(_),
                ) => a.partial_cmp(&b),
                _ => Some(l.text().to_lowercase().cmp(&r.text().to_lowercase())),
            };
            let Some(ord) = ord else { return verr("#NUM!") };
            use std::cmp::Ordering::*;
            Value::Bool(match o {
                "=" => ord == Equal,
                "<>" => ord != Equal,
                "<" => ord == Less,
                ">" => ord == Greater,
                "<=" => ord != Greater,
                _ => ord != Less,
            })
        }
        _ => {
            let (a, b) = match (l.num(), r.num()) {
                (Ok(a), Ok(b)) => (a, b),
                (Err(e), _) | (_, Err(e)) => return e,
            };
            match o {
                "+" => Value::Num(a + b),
                "-" => Value::Num(a - b),
                "*" => Value::Num(a * b),
                "/" => {
                    if b == 0.0 {
                        verr("#DIV/0!")
                    } else {
                        Value::Num(a / b)
                    }
                }
                "^" => Value::Num(a.powf(b)),
                _ => verr("#ERROR!"),
            }
        }
    }
}

/// Rohtext einer Zelle (keine Formel) als Wert: Zahl, Wahrheitswert oder Text
pub fn literal(raw: &str) -> Value {
    if let Some(t) = raw.strip_prefix('\'') {
        // 'Text erzwingt Text (z. B. führende Nullen)
        return Value::Text(t.to_string());
    }
    let t = raw.trim();
    if t.is_empty() {
        return Value::Empty;
    }
    if let Ok(n) = t.parse::<f64>() {
        if t.chars().all(|c| c.is_ascii_digit() || ".-+eE".contains(c)) {
            return Value::Num(n);
        }
    }
    if let Some(p) = t.strip_suffix('%') {
        if let Ok(n) = p.trim().parse::<f64>() {
            return Value::Num(n / 100.0);
        }
    }
    if t.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if t.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    Value::Text(raw.to_string())
}

// ------------------------------------------------------------------ CSV

/// CSV lesen; Trennzeichen (`,` `;` Tab) wird erkannt.
pub fn read_csv(text: &str) -> Sheet {
    let text = text.trim_start_matches('\u{feff}');
    let first = text.lines().next().unwrap_or("");
    let sep = [';', '\t', ',']
        .into_iter()
        .max_by_key(|c| first.matches(*c).count())
        .unwrap_or(',');
    let sep = if first.matches(sep).count() == 0 {
        ','
    } else {
        sep
    };
    let mut sh = Sheet::new("Sheet1");
    let mut r = 0u32;
    let mut c = 0u32;
    let mut cur = String::new();
    let mut q = false;
    let ch: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut row_has = false;
    while i < ch.len() {
        let x = ch[i];
        if q {
            if x == '"' {
                if i + 1 < ch.len() && ch[i + 1] == '"' {
                    cur.push('"');
                    i += 1;
                } else {
                    q = false;
                }
            } else {
                cur.push(x);
            }
        } else if x == '"' {
            q = true;
            row_has = true;
        } else if x == sep {
            sh.set(r, c, &cur);
            cur.clear();
            c += 1;
            row_has = true;
        } else if x == '\n' || x == '\r' {
            if x == '\r' && i + 1 < ch.len() && ch[i + 1] == '\n' {
                i += 1;
            }
            if row_has || !cur.is_empty() {
                sh.set(r, c, &cur);
            }
            cur.clear();
            r += 1;
            c = 0;
            row_has = false;
        } else {
            cur.push(x);
            row_has = true;
        }
        i += 1;
    }
    if row_has || !cur.is_empty() {
        sh.set(r, c, &cur);
    }
    sh
}

/// Blatt als CSV (Werte, nicht Formeln)
pub fn write_csv(wb: &Workbook, sheet: usize, sep: char) -> String {
    let Some(sh) = wb.sheets.get(sheet) else {
        return String::new();
    };
    let (rows, cols) = sh.extent();
    let mut out = String::new();
    for r in 0..rows {
        let line: Vec<String> = (0..cols)
            .map(|c| {
                let v = wb.value(sheet, r, c).text();
                if v.contains(sep) || v.contains('"') || v.contains('\n') {
                    format!("\"{}\"", v.replace('"', "\"\""))
                } else {
                    v
                }
            })
            .collect();
        out.push_str(&line.join(&sep.to_string()));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wb(cells: &[(&str, &str)]) -> Workbook {
        let mut w = Workbook::new();
        for (c, v) in cells {
            w.set_ref(0, c, v).unwrap();
        }
        w
    }

    #[test]
    fn adressen() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(col_name(701), "ZZ");
        assert_eq!(parse_cell("AA10"), Some((9, 26)));
        assert_eq!(parse_cell("$b$2"), Some((1, 1)));
        assert_eq!(
            parse_range("Sheet2!A1:B3").unwrap().0.as_deref(),
            Some("Sheet2")
        );
    }

    #[test]
    fn rechnen() {
        let w = wb(&[
            ("A1", "10"),
            ("A2", "20"),
            ("A3", "=A1+A2*2"),
            ("B1", "=SUM(A1:A3)"),
            ("B2", "=AVERAGE(A1:A2)"),
            ("B3", "=IF(A1>5,\"gross\",\"klein\")"),
            ("C1", "=2^3^2"),
            ("C2", "=-A1+5%"),
            ("C3", "=\"a\"&\"b\"&A1"),
        ]);
        assert_eq!(w.value(0, 2, 0), Value::Num(50.0));
        assert_eq!(w.value(0, 0, 1), Value::Num(80.0));
        assert_eq!(w.value(0, 1, 1), Value::Num(15.0));
        assert_eq!(w.value(0, 2, 1), Value::Text("gross".into()));
        assert_eq!(w.value(0, 0, 2), Value::Num(512.0));
        assert_eq!(w.value(0, 1, 2), Value::Num(-9.95));
        assert_eq!(w.value(0, 2, 2), Value::Text("ab10".into()));
    }

    #[test]
    fn funktionen() {
        let w = wb(&[
            ("A1", "Apfel"),
            ("B1", "3"),
            ("A2", "Birne"),
            ("B2", "5"),
            ("A3", "Apfel"),
            ("B3", "7"),
            ("D1", "=SUMIF(A1:A3,\"apfel\",B1:B3)"),
            ("D2", "=COUNTIF(B1:B3,\">4\")"),
            ("D3", "=VLOOKUP(\"Birne\",A1:B3,2,FALSE)"),
            ("D4", "=ROUND(2.345,2)"),
            ("D5", "=IFERROR(1/0,\"x\")"),
            ("D6", "=MATCH(\"Birne\",A1:A3,0)"),
            ("D7", "=INDEX(B1:B3,3)"),
            ("D8", "=TEXTJOIN(\"-\",TRUE,A1:A3)"),
            ("D9", "=MEDIAN(B1:B3)"),
        ]);
        assert_eq!(w.value(0, 0, 3), Value::Num(10.0));
        assert_eq!(w.value(0, 1, 3), Value::Num(2.0));
        assert_eq!(w.value(0, 2, 3), Value::Num(5.0));
        assert_eq!(w.value(0, 3, 3).text(), "2.35");
        assert_eq!(w.value(0, 4, 3), Value::Text("x".into()));
        assert_eq!(w.value(0, 5, 3), Value::Num(2.0));
        assert_eq!(w.value(0, 6, 3), Value::Num(7.0));
        assert_eq!(w.value(0, 7, 3).text(), "Apfel-Birne-Apfel");
        assert_eq!(w.value(0, 8, 3), Value::Num(5.0));
    }

    #[test]
    fn fehler_und_zirkel() {
        let w = wb(&[
            ("A1", "=B1"),
            ("B1", "=A1"),
            ("C1", "=1/0"),
            ("C2", "=FOO(1)"),
            ("C3", "=Blatt2!A1"),
        ]);
        assert_eq!(w.value(0, 0, 0), Value::Err("#CIRC!".into()));
        assert_eq!(w.value(0, 0, 2), Value::Err("#DIV/0!".into()));
        assert_eq!(w.value(0, 1, 2), Value::Err("#NAME?".into()));
        assert_eq!(w.value(0, 2, 2), Value::Err("#REF!".into()));
    }

    #[test]
    fn mehrere_blaetter() {
        let mut w = Workbook::new();
        w.set_ref(0, "Kosten!A1", "100").unwrap();
        w.set_ref(0, "A1", "=Kosten!A1*2").unwrap();
        assert_eq!(w.value(0, 0, 0), Value::Num(200.0));
    }

    #[test]
    fn csv() {
        let sh = read_csv("Name;Preis\n\"Brot; frisch\";2,5\nMilch;1\n");
        assert_eq!(sh.get(1, 0), "Brot; frisch");
        assert_eq!(sh.get(2, 1), "1");
        let mut w = Workbook { sheets: vec![sh] };
        w.set_ref(0, "C3", "=B3*3").unwrap();
        let out = write_csv(&w, 0, ',');
        assert!(out.contains("Brot; frisch,\"2,5\""));
        assert!(out.contains("Milch,1,3"));
    }
}
