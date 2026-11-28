//! Texteditor-Kern (Notepad++-Art): Puffer mit Schreibmarke, Rückgängig/Wiederholen, Suchen/Ersetzen
//! (auch regulär), Zeilenwerkzeuge, Kodierung (UTF-8, UTF-16, Windows-1256 für Arabisch, Latin-1) und
//! Zeilenenden (LF/CRLF) erhalten, Syntaxhervorhebung für gängige Sprachen.

use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, WINDOWS_1252, WINDOWS_1256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enc {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    /// Arabisch (Windows)
    Win1256,
    /// Westeuropäisch (Windows)
    Win1252,
}

impl Enc {
    pub fn label(self) -> &'static str {
        match self {
            Enc::Utf8 => "UTF-8",
            Enc::Utf8Bom => "UTF-8 BOM",
            Enc::Utf16Le => "UTF-16 LE",
            Enc::Utf16Be => "UTF-16 BE",
            Enc::Win1256 => "Windows-1256",
            Enc::Win1252 => "Windows-1252",
        }
    }
    pub const ALL: [Enc; 6] = [
        Enc::Utf8,
        Enc::Utf8Bom,
        Enc::Utf16Le,
        Enc::Utf16Be,
        Enc::Win1256,
        Enc::Win1252,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
    Cr,
}

impl Eol {
    pub fn label(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::CrLf => "CRLF",
            Eol::Cr => "CR",
        }
    }
    fn s(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
            Eol::Cr => "\r",
        }
    }
}

/// Bytes → Text, erkannte Kodierung, Zeilenende. Ohne BOM: gültiges UTF-8, sonst arabisch (1256), wenn
/// Bytes im arabischen Bereich überwiegen, sonst Windows-1252.
pub fn decode(b: &[u8]) -> (String, Enc, Eol) {
    let (text, enc) = if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        (String::from_utf8_lossy(&b[3..]).into_owned(), Enc::Utf8Bom)
    } else if b.starts_with(&[0xFF, 0xFE]) {
        (
            UTF_16LE.decode_without_bom_handling(&b[2..]).0.into_owned(),
            Enc::Utf16Le,
        )
    } else if b.starts_with(&[0xFE, 0xFF]) {
        (
            UTF_16BE.decode_without_bom_handling(&b[2..]).0.into_owned(),
            Enc::Utf16Be,
        )
    } else if let Ok(s) = std::str::from_utf8(b) {
        (s.to_string(), Enc::Utf8)
    } else {
        // Arabische Buchstaben liegen in 1256 überwiegend in C1–DF und E1–FE
        let hi = b.iter().filter(|&&c| c >= 0x80).count().max(1);
        let ar = b
            .iter()
            .filter(|&&c| (0xC1..=0xDF).contains(&c) || (0xE1..=0xEF).contains(&c))
            .count();
        if ar * 10 >= hi * 6 {
            (
                WINDOWS_1256.decode_without_bom_handling(b).0.into_owned(),
                Enc::Win1256,
            )
        } else {
            (
                WINDOWS_1252.decode_without_bom_handling(b).0.into_owned(),
                Enc::Win1252,
            )
        }
    };
    let eol = if text.contains("\r\n") {
        Eol::CrLf
    } else if text.contains('\r') {
        Eol::Cr
    } else {
        Eol::Lf
    };
    // intern immer \n
    let text = if eol == Eol::Lf {
        text
    } else {
        text.replace("\r\n", "\n").replace('\r', "\n")
    };
    (text, enc, eol)
}

pub fn encode(text: &str, enc: Enc, eol: Eol) -> Vec<u8> {
    let t = if eol == Eol::Lf {
        text.to_string()
    } else {
        text.replace('\n', eol.s())
    };
    let enc_of = |e: &'static Encoding| e.encode(&t).0.into_owned();
    match enc {
        Enc::Utf8 => t.into_bytes(),
        Enc::Utf8Bom => [&[0xEF, 0xBB, 0xBF][..], t.as_bytes()].concat(),
        Enc::Utf16Le => {
            let mut v = vec![0xFF, 0xFE];
            for u in t.encode_utf16() {
                v.extend_from_slice(&u.to_le_bytes());
            }
            v
        }
        Enc::Utf16Be => {
            let mut v = vec![0xFE, 0xFF];
            for u in t.encode_utf16() {
                v.extend_from_slice(&u.to_be_bytes());
            }
            v
        }
        Enc::Win1256 => enc_of(WINDOWS_1256),
        Enc::Win1252 => enc_of(WINDOWS_1252),
    }
}

// ------------------------------------------------------------------ Puffer

#[derive(Clone)]
struct Snap {
    text: String,
    cur: usize,
}

/// Bearbeitbarer Text mit Schreibmarke (Byte-Position, immer auf Zeichengrenze) und Auswahl
pub struct Buffer {
    pub text: String,
    pub cur: usize,
    /// Auswahl-Anker (Auswahl = anchor..cur)
    pub anchor: Option<usize>,
    pub enc: Enc,
    pub eol: Eol,
    pub dirty: bool,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    /// letzte Eingabe war Tippen (aufeinanderfolgendes Tippen = ein Rückgängig-Schritt)
    typing: bool,
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        Buffer {
            text: text.into(),
            cur: 0,
            anchor: None,
            enc: Enc::Utf8,
            eol: Eol::Lf,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            typing: false,
        }
    }

    pub fn from_bytes(b: &[u8]) -> Self {
        let (t, enc, eol) = decode(b);
        let mut s = Buffer::new(&t);
        s.enc = enc;
        s.eol = eol;
        s
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        encode(&self.text, self.enc, self.eol)
    }

    fn snap(&mut self, typing: bool) {
        if !(typing && self.typing) {
            self.undo.push(Snap {
                text: self.text.clone(),
                cur: self.cur,
            });
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
        }
        self.typing = typing;
        self.redo.clear();
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(s) => {
                self.redo.push(Snap {
                    text: std::mem::replace(&mut self.text, s.text),
                    cur: self.cur,
                });
                self.cur = s.cur.min(self.text.len());
                self.anchor = None;
                self.typing = false;
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(s) => {
                self.undo.push(Snap {
                    text: std::mem::replace(&mut self.text, s.text),
                    cur: self.cur,
                });
                self.cur = s.cur.min(self.text.len());
                self.anchor = None;
                self.typing = false;
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        if a == self.cur {
            return None;
        }
        Some((a.min(self.cur), a.max(self.cur)))
    }

    pub fn selected_text(&self) -> String {
        self.selection()
            .map(|(a, b)| self.text[a..b].to_string())
            .unwrap_or_default()
    }

    fn delete_sel(&mut self) -> bool {
        if let Some((a, b)) = self.selection() {
            self.text.replace_range(a..b, "");
            self.cur = a;
            self.anchor = None;
            true
        } else {
            self.anchor = None;
            false
        }
    }

    pub fn insert(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        self.snap(!s.contains('\n') && s.chars().count() == 1);
        self.delete_sel();
        self.text.insert_str(self.cur, &s);
        self.cur += s.len();
    }

    /// Enter mit Einrückung der aktuellen Zeile (und nach `{`/`:` eine Stufe mehr)
    pub fn newline(&mut self, indent_unit: &str) {
        let ls = self.line_start(self.cur);
        let line = &self.text[ls..self.cur];
        let mut ind: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let tr = line.trim_end();
        if tr.ends_with('{') || tr.ends_with('[') || tr.ends_with('(') || tr.ends_with(':') {
            ind.push_str(indent_unit);
        }
        self.insert(&format!("\n{ind}"));
    }

    pub fn backspace(&mut self) {
        self.snap(true);
        if self.delete_sel() {
            return;
        }
        if self.cur == 0 {
            return;
        }
        let p = prev_char(&self.text, self.cur);
        self.text.replace_range(p..self.cur, "");
        self.cur = p;
    }

    pub fn delete(&mut self) {
        self.snap(false);
        if self.delete_sel() {
            return;
        }
        if self.cur >= self.text.len() {
            return;
        }
        let n = next_char(&self.text, self.cur);
        self.text.replace_range(self.cur..n, "");
    }

    pub fn set_text(&mut self, t: &str) {
        self.snap(false);
        self.text = t.to_string();
        self.cur = self.cur.min(self.text.len());
        while !self.text.is_char_boundary(self.cur) {
            self.cur -= 1;
        }
        self.anchor = None;
    }

    // ------------- Bewegung

    pub fn left(&mut self, select: bool) {
        self.sel(select);
        self.cur = prev_char(&self.text, self.cur);
        self.typing = false;
    }
    pub fn right(&mut self, select: bool) {
        self.sel(select);
        self.cur = next_char(&self.text, self.cur);
        self.typing = false;
    }
    fn sel(&mut self, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
        } else {
            self.anchor = None;
        }
    }
    pub fn up(&mut self, select: bool) {
        self.sel(select);
        let (l, c) = self.line_col(self.cur);
        if l > 0 {
            self.cur = self.pos(l - 1, c);
        } else {
            self.cur = 0;
        }
        self.typing = false;
    }
    pub fn down(&mut self, select: bool) {
        self.sel(select);
        let (l, c) = self.line_col(self.cur);
        if l + 1 < self.line_count() {
            self.cur = self.pos(l + 1, c);
        } else {
            self.cur = self.text.len();
        }
        self.typing = false;
    }
    pub fn home(&mut self, select: bool) {
        self.sel(select);
        let ls = self.line_start(self.cur);
        // erst zum ersten Nicht-Leerzeichen, beim zweiten Mal an den Zeilenanfang
        let first = ls
            + self.text[ls..]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .map(|c| c.len_utf8())
                .sum::<usize>();
        self.cur = if self.cur == first { ls } else { first };
        self.typing = false;
    }
    pub fn end(&mut self, select: bool) {
        self.sel(select);
        self.cur = self.line_end(self.cur);
        self.typing = false;
    }
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cur = self.text.len();
    }
    /// Wort unter der Schreibmarke auswählen
    pub fn select_word(&mut self) {
        let is_w = |c: char| c.is_alphanumeric() || c == '_';
        let mut a = self.cur;
        while a > 0 {
            let p = prev_char(&self.text, a);
            if !self.text[p..a].chars().next().is_some_and(is_w) {
                break;
            }
            a = p;
        }
        let mut b = self.cur;
        while b < self.text.len() {
            let n = next_char(&self.text, b);
            if !self.text[b..n].chars().next().is_some_and(is_w) {
                break;
            }
            b = n;
        }
        if a < b {
            self.anchor = Some(a);
            self.cur = b;
        }
    }

    // ------------- Zeilen

    pub fn line_count(&self) -> usize {
        self.text.matches('\n').count() + 1
    }
    pub fn line_start(&self, pos: usize) -> usize {
        self.text[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0)
    }
    pub fn line_end(&self, pos: usize) -> usize {
        self.text[pos..]
            .find('\n')
            .map(|p| pos + p)
            .unwrap_or(self.text.len())
    }
    /// (Zeile, Spalte in Zeichen), beide ab 0
    pub fn line_col(&self, pos: usize) -> (usize, usize) {
        let pos = pos.min(self.text.len());
        let l = self.text[..pos].matches('\n').count();
        let ls = self.line_start(pos);
        (l, self.text[ls..pos].chars().count())
    }
    /// Position aus Zeile/Spalte (Spalte wird auf Zeilenlänge begrenzt)
    pub fn pos(&self, line: usize, col: usize) -> usize {
        let mut ls = 0;
        for _ in 0..line {
            match self.text[ls..].find('\n') {
                Some(p) => ls += p + 1,
                None => return self.text.len(),
            }
        }
        let le = self.line_end(ls);
        let mut p = ls;
        for (i, c) in self.text[ls..le].chars().enumerate() {
            if i >= col {
                break;
            }
            p += c.len_utf8();
        }
        p
    }
    pub fn line(&self, l: usize) -> &str {
        self.text.split('\n').nth(l).unwrap_or("")
    }
    pub fn goto_line(&mut self, l: usize) {
        self.anchor = None;
        self.cur = self.pos(l.saturating_sub(1), 0);
    }
    pub fn duplicate_line(&mut self) {
        self.snap(false);
        let (ls, le) = (self.line_start(self.cur), self.line_end(self.cur));
        let line = self.text[ls..le].to_string();
        self.text.insert_str(le, &format!("\n{line}"));
        self.cur += line.len() + 1;
    }
    pub fn delete_line(&mut self) {
        self.snap(false);
        let (ls, le) = (self.line_start(self.cur), self.line_end(self.cur));
        let end = if le < self.text.len() { le + 1 } else { le };
        let start = if end == le && ls > 0 { ls - 1 } else { ls };
        self.text.replace_range(start..end, "");
        self.cur = start.min(self.text.len());
        self.cur = self.line_start(self.cur);
    }
    pub fn move_line(&mut self, down: bool) {
        let (l, c) = self.line_col(self.cur);
        let n = self.line_count();
        if (down && l + 1 >= n) || (!down && l == 0) {
            return;
        }
        self.snap(false);
        let mut lines: Vec<String> = self.text.split('\n').map(String::from).collect();
        let to = if down { l + 1 } else { l - 1 };
        lines.swap(l, to);
        self.text = lines.join("\n");
        self.cur = self.pos(to, c);
    }
    /// Zeilenkommentar umschalten (für die erkannte Sprache)
    pub fn toggle_comment(&mut self, lang: Lang) {
        let Some(tok) = lang.line_comment() else {
            return;
        };
        self.snap(false);
        let (a, b) = self.selection().unwrap_or((self.cur, self.cur));
        let (l0, _) = self.line_col(a);
        let (l1, _) = self.line_col(b);
        let mut lines: Vec<String> = self.text.split('\n').map(String::from).collect();
        let all =
            (l0..=l1).all(|i| lines[i].trim_start().starts_with(tok) || lines[i].trim().is_empty());
        for line in lines.iter_mut().take(l1 + 1).skip(l0) {
            if line.trim().is_empty() {
                continue;
            }
            let ind = line.len() - line.trim_start().len();
            if all {
                let rest = &line[ind + tok.len()..];
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                *line = format!("{}{}", &line[..ind], rest);
            } else {
                *line = format!("{}{} {}", &line[..ind], tok, &line[ind..]);
            }
        }
        let (cl, cc) = self.line_col(self.cur);
        self.text = lines.join("\n");
        self.anchor = None;
        self.cur = self.pos(cl, cc);
    }
    /// Einrücken / Ausrücken der Zeile(n)
    pub fn indent(&mut self, unit: &str, out: bool) {
        self.snap(false);
        let (a, b) = self.selection().unwrap_or((self.cur, self.cur));
        let (l0, _) = self.line_col(a);
        let (l1, _) = self.line_col(b);
        let (cl, cc) = self.line_col(self.cur);
        let mut lines: Vec<String> = self.text.split('\n').map(String::from).collect();
        for line in lines.iter_mut().take(l1 + 1).skip(l0) {
            if out {
                if let Some(r) = line.strip_prefix(unit) {
                    *line = r.to_string();
                } else if let Some(r) = line.strip_prefix('\t') {
                    *line = r.to_string();
                } else {
                    *line = line.trim_start_matches(' ').to_string();
                }
            } else {
                *line = format!("{unit}{line}");
            }
        }
        self.text = lines.join("\n");
        self.anchor = None;
        let d = if out { 0 } else { unit.chars().count() };
        self.cur = self.pos(cl, cc + d);
    }

    // ------------- Suchen

    /// Nächstes Vorkommen ab Schreibmarke (umlaufend). Markiert den Fund.
    pub fn find_next(&mut self, q: &str, opt: &FindOpt) -> bool {
        let hits = find_all(&self.text, q, opt);
        if hits.is_empty() {
            return false;
        }
        let from = self.cur;
        let h = hits
            .iter()
            .find(|(a, _)| *a >= from && Some(*a) != self.anchor)
            .or_else(|| hits.first())
            .copied()
            .unwrap();
        self.anchor = Some(h.0);
        self.cur = h.1;
        self.typing = false;
        true
    }

    pub fn find_prev(&mut self, q: &str, opt: &FindOpt) -> bool {
        let hits = find_all(&self.text, q, opt);
        if hits.is_empty() {
            return false;
        }
        let from = self.selection().map(|s| s.0).unwrap_or(self.cur);
        let h = hits
            .iter()
            .rev()
            .find(|(a, _)| *a < from)
            .or_else(|| hits.last())
            .copied()
            .unwrap();
        self.anchor = Some(h.0);
        self.cur = h.1;
        true
    }

    /// Alles ersetzen; liefert Anzahl
    pub fn replace_all(&mut self, q: &str, repl: &str, opt: &FindOpt) -> Result<usize, String> {
        let (t, n) = replace_all(&self.text, q, repl, opt)?;
        if n > 0 {
            self.set_text(&t);
        }
        Ok(n)
    }

    /// Aktuellen Fund ersetzen und weiter suchen
    pub fn replace_one(&mut self, q: &str, repl: &str, opt: &FindOpt) -> bool {
        if let Some((a, b)) = self.selection() {
            let sel = &self.text[a..b];
            let ok = find_all(sel, q, opt)
                .first()
                .is_some_and(|&(x, y)| x == 0 && y == sel.len());
            if ok {
                let r = if opt.regex {
                    match regex_of(q, opt) {
                        Ok(re) => re.replace(sel, repl).into_owned(),
                        Err(_) => repl.to_string(),
                    }
                } else {
                    repl.to_string()
                };
                self.snap(false);
                self.text.replace_range(a..b, &r);
                self.cur = a + r.len();
                self.anchor = None;
            }
        }
        self.find_next(q, opt)
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        (
            self.line_count(),
            self.text.split_whitespace().count(),
            self.text.chars().count(),
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct FindOpt {
    pub case: bool,
    pub word: bool,
    pub regex: bool,
}

fn regex_of(q: &str, opt: &FindOpt) -> Result<regex::Regex, String> {
    let mut p = if opt.regex {
        q.to_string()
    } else {
        regex::escape(q)
    };
    if opt.word {
        p = format!(r"\b(?:{p})\b");
    }
    regex::RegexBuilder::new(&p)
        .case_insensitive(!opt.case)
        .multi_line(true)
        .size_limit(1 << 22)
        .build()
        .map_err(|e| e.to_string())
}

/// Alle Fundstellen (Byte-Bereiche)
pub fn find_all(text: &str, q: &str, opt: &FindOpt) -> Vec<(usize, usize)> {
    if q.is_empty() {
        return Vec::new();
    }
    match regex_of(q, opt) {
        Ok(re) => re
            .find_iter(text)
            .filter(|m| m.start() != m.end())
            .map(|m| (m.start(), m.end()))
            .take(100_000)
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn replace_all(
    text: &str,
    q: &str,
    repl: &str,
    opt: &FindOpt,
) -> Result<(String, usize), String> {
    if q.is_empty() {
        return Ok((text.to_string(), 0));
    }
    let re = regex_of(q, opt)?;
    let n = re.find_iter(text).count();
    let out = if opt.regex {
        re.replace_all(text, repl).into_owned()
    } else {
        re.replace_all(text, regex::NoExpand(repl)).into_owned()
    };
    Ok((out, n))
}

fn prev_char(s: &str, p: usize) -> usize {
    if p == 0 {
        return 0;
    }
    let mut q = p - 1;
    while !s.is_char_boundary(q) {
        q -= 1;
    }
    q
}

fn next_char(s: &str, p: usize) -> usize {
    if p >= s.len() {
        return s.len();
    }
    let mut q = p + 1;
    while q < s.len() && !s.is_char_boundary(q) {
        q += 1;
    }
    q
}

// ------------------------------------------------------------------ Sprachen und Hervorhebung

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Plain,
    Rust,
    C,
    Cpp,
    Python,
    JavaScript,
    TypeScript,
    Json,
    Toml,
    Yaml,
    Html,
    Xml,
    Css,
    Shell,
    Markdown,
    Sql,
    Go,
    Java,
    Kotlin,
    CSharp,
    Php,
    Ini,
}

impl Lang {
    pub const ALL: [Lang; 22] = [
        Lang::Plain,
        Lang::Rust,
        Lang::C,
        Lang::Cpp,
        Lang::Python,
        Lang::JavaScript,
        Lang::TypeScript,
        Lang::Json,
        Lang::Toml,
        Lang::Yaml,
        Lang::Html,
        Lang::Xml,
        Lang::Css,
        Lang::Shell,
        Lang::Markdown,
        Lang::Sql,
        Lang::Go,
        Lang::Java,
        Lang::Kotlin,
        Lang::CSharp,
        Lang::Php,
        Lang::Ini,
    ];

    pub fn from_path(p: &str) -> Lang {
        let name = p.rsplit('/').next().unwrap_or(p).to_ascii_lowercase();
        if name == "makefile"
            || name == "dockerfile"
            || name.ends_with(".sh")
            || name.ends_with(".bash")
            || name == ".bashrc"
            || name == ".profile"
        {
            return Lang::Shell;
        }
        let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        match ext {
            "rs" => Lang::Rust,
            "c" | "h" => Lang::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "ino" => Lang::Cpp,
            "py" | "pyw" => Lang::Python,
            "js" | "mjs" | "cjs" | "jsx" => Lang::JavaScript,
            "ts" | "tsx" => Lang::TypeScript,
            "json" | "geojson" => Lang::Json,
            "toml" | "lock" => Lang::Toml,
            "yml" | "yaml" => Lang::Yaml,
            "html" | "htm" | "vue" | "svelte" => Lang::Html,
            "xml" | "svg" | "xaml" | "csproj" | "plist" => Lang::Xml,
            "css" | "scss" | "less" => Lang::Css,
            "sh" | "bash" | "zsh" | "ps1" | "bat" | "cmd" => Lang::Shell,
            "md" | "markdown" => Lang::Markdown,
            "sql" => Lang::Sql,
            "go" => Lang::Go,
            "java" => Lang::Java,
            "kt" | "kts" => Lang::Kotlin,
            "cs" => Lang::CSharp,
            "php" => Lang::Php,
            "ini" | "cfg" | "conf" | "properties" | "env" | "service" | "desktop" => Lang::Ini,
            _ => Lang::Plain,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Plain => "Text",
            Lang::Rust => "Rust",
            Lang::C => "C",
            Lang::Cpp => "C++",
            Lang::Python => "Python",
            Lang::JavaScript => "JavaScript",
            Lang::TypeScript => "TypeScript",
            Lang::Json => "JSON",
            Lang::Toml => "TOML",
            Lang::Yaml => "YAML",
            Lang::Html => "HTML",
            Lang::Xml => "XML",
            Lang::Css => "CSS",
            Lang::Shell => "Shell",
            Lang::Markdown => "Markdown",
            Lang::Sql => "SQL",
            Lang::Go => "Go",
            Lang::Java => "Java",
            Lang::Kotlin => "Kotlin",
            Lang::CSharp => "C#",
            Lang::Php => "PHP",
            Lang::Ini => "INI",
        }
    }

    pub fn line_comment(self) -> Option<&'static str> {
        match self {
            Lang::Rust
            | Lang::C
            | Lang::Cpp
            | Lang::JavaScript
            | Lang::TypeScript
            | Lang::Go
            | Lang::Java
            | Lang::Kotlin
            | Lang::CSharp
            | Lang::Php => Some("//"),
            Lang::Python | Lang::Toml | Lang::Yaml | Lang::Shell => Some("#"),
            Lang::Sql => Some("--"),
            Lang::Ini => Some(";"),
            _ => None,
        }
    }

    fn block_comment(self) -> Option<(&'static str, &'static str)> {
        match self {
            Lang::Rust
            | Lang::C
            | Lang::Cpp
            | Lang::JavaScript
            | Lang::TypeScript
            | Lang::Go
            | Lang::Java
            | Lang::Kotlin
            | Lang::CSharp
            | Lang::Php
            | Lang::Css
            | Lang::Sql => Some(("/*", "*/")),
            Lang::Html | Lang::Xml | Lang::Markdown => Some(("<!--", "-->")),
            Lang::Python => Some(("\"\"\"", "\"\"\"")),
            _ => None,
        }
    }

    fn keywords(self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
                "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
                "super", "trait", "true", "type", "unsafe", "use", "where", "while", "Some",
                "None", "Ok", "Err",
            ],
            Lang::C | Lang::Cpp => &[
                "auto",
                "break",
                "case",
                "char",
                "const",
                "continue",
                "default",
                "do",
                "double",
                "else",
                "enum",
                "extern",
                "float",
                "for",
                "goto",
                "if",
                "int",
                "long",
                "register",
                "return",
                "short",
                "signed",
                "sizeof",
                "static",
                "struct",
                "switch",
                "typedef",
                "union",
                "unsigned",
                "void",
                "volatile",
                "while",
                "class",
                "namespace",
                "template",
                "typename",
                "public",
                "private",
                "protected",
                "virtual",
                "new",
                "delete",
                "this",
                "true",
                "false",
                "nullptr",
                "bool",
                "using",
                "include",
                "define",
                "ifdef",
                "ifndef",
                "endif",
                "uint8_t",
                "uint16_t",
                "uint32_t",
                "size_t",
            ],
            Lang::Python => &[
                "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
                "continue", "def", "del", "elif", "else", "except", "finally", "for", "from",
                "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass",
                "raise", "return", "try", "while", "with", "yield", "self", "print",
            ],
            Lang::JavaScript | Lang::TypeScript => &[
                "async",
                "await",
                "break",
                "case",
                "catch",
                "class",
                "const",
                "continue",
                "default",
                "delete",
                "do",
                "else",
                "export",
                "extends",
                "false",
                "finally",
                "for",
                "from",
                "function",
                "if",
                "import",
                "in",
                "instanceof",
                "let",
                "new",
                "null",
                "of",
                "return",
                "static",
                "super",
                "switch",
                "this",
                "throw",
                "true",
                "try",
                "typeof",
                "undefined",
                "var",
                "void",
                "while",
                "yield",
                "interface",
                "type",
                "enum",
                "implements",
                "readonly",
                "public",
                "private",
                "string",
                "number",
                "boolean",
                "any",
            ],
            Lang::Go => &[
                "break",
                "case",
                "chan",
                "const",
                "continue",
                "default",
                "defer",
                "else",
                "fallthrough",
                "for",
                "func",
                "go",
                "goto",
                "if",
                "import",
                "interface",
                "map",
                "package",
                "range",
                "return",
                "select",
                "struct",
                "switch",
                "type",
                "var",
                "nil",
                "true",
                "false",
                "string",
                "int",
                "error",
            ],
            Lang::Java | Lang::Kotlin | Lang::CSharp => &[
                "abstract",
                "boolean",
                "break",
                "byte",
                "case",
                "catch",
                "char",
                "class",
                "const",
                "continue",
                "default",
                "do",
                "double",
                "else",
                "enum",
                "extends",
                "final",
                "finally",
                "float",
                "for",
                "if",
                "implements",
                "import",
                "instanceof",
                "int",
                "interface",
                "long",
                "new",
                "package",
                "private",
                "protected",
                "public",
                "return",
                "short",
                "static",
                "super",
                "switch",
                "this",
                "throw",
                "throws",
                "try",
                "void",
                "while",
                "true",
                "false",
                "null",
                "fun",
                "val",
                "var",
                "when",
                "object",
                "namespace",
                "using",
                "string",
                "var",
                "override",
            ],
            Lang::Php => &[
                "echo",
                "function",
                "if",
                "else",
                "elseif",
                "foreach",
                "as",
                "return",
                "class",
                "public",
                "private",
                "new",
                "array",
                "true",
                "false",
                "null",
                "while",
                "for",
                "use",
                "namespace",
            ],
            Lang::Shell => &[
                "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "case",
                "esac", "function", "return", "local", "export", "echo", "exit", "set", "unset",
                "read", "sudo", "cd",
            ],
            Lang::Sql => &[
                "select", "from", "where", "insert", "into", "values", "update", "set", "delete",
                "create", "table", "drop", "alter", "and", "or", "not", "null", "join", "left",
                "right", "inner", "on", "group", "by", "order", "having", "limit", "as", "primary",
                "key", "index", "distinct", "count", "integer", "text", "varchar",
            ],
            Lang::Css => &["important", "media", "import", "font-face", "keyframes"],
            Lang::Json | Lang::Toml | Lang::Yaml => &["true", "false", "null"],
            _ => &[],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tk {
    Text,
    Keyword,
    Str,
    Comment,
    Num,
    /// Funktion/Typ/Tag
    Name,
    Punct,
}

/// Zustand am Zeilenende (mehrzeilige Kommentare/Zeichenketten)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LineState {
    #[default]
    Normal,
    InBlock,
}

/// Zeile einfärben: Liste (Bereich in Bytes, Art) und Zustand für die nächste Zeile
pub fn highlight(line: &str, lang: Lang, st: LineState) -> (Vec<(usize, usize, Tk)>, LineState) {
    let mut out = Vec::new();
    let b = line.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut st = st;
    let blk = lang.block_comment();
    let lc = lang.line_comment();
    if lang == Lang::Plain {
        return (vec![(0, n, Tk::Text)], st);
    }
    if lang == Lang::Markdown {
        let t = line.trim_start();
        let k = if t.starts_with('#') {
            Tk::Keyword
        } else if t.starts_with("```") || line.starts_with("    ") {
            Tk::Str
        } else if t.starts_with('>') {
            Tk::Comment
        } else if t.starts_with("- ") || t.starts_with("* ") {
            Tk::Name
        } else {
            Tk::Text
        };
        return (vec![(0, n, k)], st);
    }
    let kws = lang.keywords();
    let ci = lang == Lang::Sql;
    while i < n {
        if st == LineState::InBlock {
            let end = blk.map(|(_, e)| e).unwrap_or("*/");
            match line[i..].find(end) {
                Some(p) => {
                    out.push((i, i + p + end.len(), Tk::Comment));
                    i += p + end.len();
                    st = LineState::Normal;
                }
                None => {
                    out.push((i, n, Tk::Comment));
                    i = n;
                }
            }
            continue;
        }
        let rest = &line[i..];
        if let Some((s, e)) = blk {
            if let Some(after) = rest.strip_prefix(s) {
                match after.find(e) {
                    Some(p) => {
                        let end = i + s.len() + p + e.len();
                        out.push((i, end, Tk::Comment));
                        i = end;
                    }
                    None => {
                        out.push((i, n, Tk::Comment));
                        i = n;
                        st = LineState::InBlock;
                    }
                }
                continue;
            }
        }
        if let Some(c) = lc {
            // in Shell/TOML nur '#' am Wortanfang
            if rest.starts_with(c)
                && !(c == "#" && i > 0 && !b[i - 1].is_ascii_whitespace() && lang == Lang::Shell)
            {
                out.push((i, n, Tk::Comment));
                break;
            }
        }
        let ch = b[i];
        if ch == b'"' || ch == b'\'' || ch == b'`' {
            // in Rust ist ' meist Lebensdauer: nur 'x' als Zeichen werten
            if ch == b'\''
                && lang == Lang::Rust
                && !(i + 2 < n && (b[i + 2] == b'\'' || b[i + 1] == b'\\'))
            {
                out.push((i, i + 1, Tk::Punct));
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < n {
                if b[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if b[j] == ch {
                    j += 1;
                    break;
                }
                j += 1;
            }
            let j = j.min(n);
            out.push((i, j, Tk::Str));
            i = j;
            continue;
        }
        if ch.is_ascii_digit() {
            let mut j = i;
            while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'.' || b[j] == b'_') {
                j += 1;
            }
            out.push((i, j, Tk::Num));
            i = j;
            continue;
        }
        if ch.is_ascii_alphabetic()
            || ch == b'_'
            || ch >= 0x80
            || (ch == b'$' && matches!(lang, Lang::Shell | Lang::Php))
            || (ch == b'-' && lang == Lang::Css)
        {
            let mut j = i + 1;
            while j < n
                && (b[j].is_ascii_alphanumeric()
                    || b[j] == b'_'
                    || b[j] >= 0x80
                    || (b[j] == b'-' && matches!(lang, Lang::Css | Lang::Html | Lang::Xml)))
            {
                j += 1;
            }
            while !line.is_char_boundary(j) {
                j += 1;
            }
            let w = &line[i..j];
            let next = line[j..].trim_start();
            let after_tag = i > 0 && (b[i - 1] == b'<' || b[i - 1] == b'/');
            let k = if (ci && kws.iter().any(|k| k.eq_ignore_ascii_case(w)))
                || (!ci && kws.contains(&w))
            {
                Tk::Keyword
            } else if word_is_name(lang, ch, w, next, after_tag) {
                Tk::Name
            } else {
                Tk::Text
            };
            out.push((i, j, k));
            i = j;
            continue;
        }
        if ch == b'['
            && matches!(lang, Lang::Toml | Lang::Ini)
            && i == line.len() - line.trim_start().len()
        {
            out.push((i, n, Tk::Keyword));
            break;
        }
        let mut j = i + 1;
        while !line.is_char_boundary(j) {
            j += 1;
        }
        out.push((
            i,
            j,
            if ch.is_ascii_punctuation() {
                Tk::Punct
            } else {
                Tk::Text
            },
        ));
        i = j;
    }
    (out, st)
}

/// Whether the word `w` (first byte `first`, followed by `next`) is shown as a name: `$vars`,
/// calls, HTML/XML tags, keys of data files and capitalised types of the typed languages.
fn word_is_name(lang: Lang, first: u8, w: &str, next: &str, after_tag: bool) -> bool {
    let key_lang = matches!(
        lang,
        Lang::Json | Lang::Yaml | Lang::Toml | Lang::Ini | Lang::Css
    );
    let typed_lang = matches!(
        lang,
        Lang::Rust
            | Lang::Java
            | Lang::Kotlin
            | Lang::CSharp
            | Lang::TypeScript
            | Lang::Go
            | Lang::Cpp
    );
    first == b'$'
        || next.starts_with('(')
        || (matches!(lang, Lang::Html | Lang::Xml) && after_tag)
        || (key_lang && (next.starts_with(':') || next.starts_with('=')))
        || (typed_lang && w.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kodierungen() {
        for enc in Enc::ALL {
            let text = if matches!(enc, Enc::Win1252) {
                "Grüße\nZeile 2"
            } else {
                "مرحبا Welt\nZeile 2"
            };
            for eol in [Eol::Lf, Eol::CrLf] {
                let b = encode(text, enc, eol);
                let (t, e, l) = decode(&b);
                assert_eq!(t, text, "{enc:?} {eol:?}");
                assert_eq!(l, eol);
                assert_eq!(e, enc, "{enc:?}");
            }
        }
    }

    #[test]
    fn bearbeiten() {
        let mut b = Buffer::new("fn main() {\n    let x = 1;\n}");
        b.cur = b.pos(1, 14);
        b.newline("    ");
        b.insert("y");
        assert_eq!(b.line(2), "    y");
        b.undo();
        b.undo();
        assert_eq!(b.text, "fn main() {\n    let x = 1;\n}");
        b.redo();
        assert_eq!(b.line_count(), 4);
        b.goto_line(2);
        b.duplicate_line();
        assert_eq!(b.line(2), "    let x = 1;");
        b.toggle_comment(Lang::Rust);
        assert!(b.line(2).trim_start().starts_with("// let"));
        b.toggle_comment(Lang::Rust);
        assert_eq!(b.line(2), "    let x = 1;");
        b.move_line(false);
        b.delete_line();
        assert_eq!(b.line_count(), 4);
    }

    #[test]
    fn suchen_ersetzen() {
        let mut b = Buffer::new("Apfel apfel Apfelsaft");
        let opt = FindOpt {
            case: true,
            ..Default::default()
        };
        assert!(b.find_next("Apfel", &opt));
        assert_eq!(b.selected_text(), "Apfel");
        let w = FindOpt {
            word: true,
            ..Default::default()
        };
        assert_eq!(b.replace_all("apfel", "Birne", &w).unwrap(), 2);
        assert_eq!(b.text, "Birne Birne Apfelsaft");
        let re = FindOpt {
            regex: true,
            ..Default::default()
        };
        assert_eq!(b.replace_all(r"(\w+)saft", "$1-Saft", &re).unwrap(), 1);
        assert_eq!(b.text, "Birne Birne Apfel-Saft");
    }

    #[test]
    fn hervorhebung() {
        let (t, st) = highlight("let s = \"hi\"; // gruß /*", Lang::Rust, LineState::Normal);
        assert_eq!(st, LineState::Normal);
        assert!(t.iter().any(|&(a, _, k)| k == Tk::Keyword && a == 0));
        assert!(t.iter().any(|&(_, _, k)| k == Tk::Str));
        assert_eq!(t.last().unwrap().2, Tk::Comment);
        let (_, st) = highlight("x = 1 /* start", Lang::C, LineState::Normal);
        assert_eq!(st, LineState::InBlock);
        let (t, st) = highlight("ende */ y", Lang::C, st);
        assert_eq!(st, LineState::Normal);
        assert_eq!(t[0].2, Tk::Comment);
        assert_eq!(Lang::from_path("/data/a/main.rs"), Lang::Rust);
        assert_eq!(Lang::from_path("Makefile"), Lang::Shell);
    }
}
