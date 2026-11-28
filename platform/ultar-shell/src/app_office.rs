//! Büro-Apps (Untermodul von `app`): PDF-Leser, Dokumente (Word/.docx), Tabellen (Excel/.xlsx, .csv) und
//! Code-Editor (Notepad++-Art). Logik und Dateiformate: Crate `ultar-office` (reines Rust, ohne Ubuntu-Bereich).
//! Der KI-Agent bedient dieselben Dateien über Werkzeuge (`exec_office`).

use super::{App, Screen};
use crate::gfx::{rgb, Color, Rect};
use crate::ui::{Al, Ic, Ui};
use std::collections::{HashMap, HashSet};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;
use ultar_office as uo;
use uo::code::{Buffer, FindOpt, Lang, LineState, Tk};
use uo::doc::{Align, Block, Document, Para, Style};
use uo::sheet::{cell_name, col_name, Value, Workbook};

/// Art der Büro-App: 0 PDF, 1 Dokument, 2 Tabelle, 3 Code
#[derive(Clone, PartialEq, Debug)]
pub enum Ox {
    List(u8),
    Pdf(String),
    Doc(String),
    Sheet(String),
    Code(String),
}

pub const KIND_NAMES: [&str; 4] = ["PDF reader", "Documents", "Sheets", "Code editor"];

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
#[serde(default)]
pub struct OfficeCfg {
    pub recent: Vec<String>,
    /// Schriftgröße im Code-Editor
    pub code_size: f32,
}

#[derive(Default)]
pub struct OfficeState {
    cfg: Option<OfficeCfg>,
    /// Dateiliste je Art: (Art, Zeitpunkt, [(Pfad, Größe, Änderungszeit)])
    files: Option<(u8, Instant, Vec<(String, u64, u64)>)>,
    pdf: Option<PdfView>,
    doc: Option<DocView>,
    pub(super) sheet: Option<SheetView>,
    pub(super) code: Option<CodeView>,
    last_tap: Option<Instant>,
    /// Menü/Blatt im Code-Editor bzw. Dokument offen
    menu: bool,
    /// Prüfansicht (Bildschirmfoto): Bearbeitungszustand herstellen
    pub(super) debug_edit: bool,
}

// ------------------------------------------------------------------ PDF

struct PagePic {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

#[derive(Default)]
struct PdfShared {
    info: Option<(usize, Vec<(f32, f32)>)>,
    err: Option<String>,
    /// Seite → (angeforderte Breite, Bild)
    pics: HashMap<usize, (u32, Arc<PagePic>)>,
    pending: HashSet<(usize, u32)>,
    /// gewünschte Seiten (sichtbar ± 1) – ältere Aufträge werden übersprungen
    wanted: HashSet<usize>,
    text: HashMap<usize, String>,
    results: Option<Vec<(usize, String)>>,
    busy: bool,
}

enum PdfReq {
    Render(usize, u32),
    Text(usize),
    Search(String),
}

struct PdfView {
    path: String,
    tx: mpsc::Sender<PdfReq>,
    sh: Arc<Mutex<PdfShared>>,
    zoom: f32,
    hx: f32,
    cur: usize,
    text_mode: bool,
    search: bool,
    jump: Option<usize>,
}

fn pdf_open(path: &str, bg: crate::sys::Bg) -> PdfView {
    let (tx, rx) = mpsc::channel::<PdfReq>();
    let sh = Arc::new(Mutex::new(PdfShared::default()));
    let sh2 = sh.clone();
    let p = path.to_string();
    std::thread::spawn(move || {
        let doc = match std::fs::read(&p)
            .map_err(|e| e.to_string())
            .and_then(uo::pdf::PdfDoc::open)
        {
            Ok(d) => d,
            Err(e) => {
                sh2.lock().unwrap().err = Some(e);
                bg.poke();
                return;
            }
        };
        let n = doc.pages();
        let sizes = (0..n).map(|i| doc.page_size(i)).collect();
        sh2.lock().unwrap().info = Some((n, sizes));
        bg.poke();
        while let Ok(first) = rx.recv() {
            let mut q = vec![first];
            while let Ok(r) = rx.try_recv() {
                q.push(r);
            }
            for r in q {
                match r {
                    PdfReq::Render(i, w) => {
                        if !sh2.lock().unwrap().wanted.contains(&i) {
                            sh2.lock().unwrap().pending.remove(&(i, w));
                            continue;
                        }
                        let img = doc.render(i, w);
                        let mut s = sh2.lock().unwrap();
                        s.pending.remove(&(i, w));
                        if let Some(img) = img {
                            s.pics.insert(
                                i,
                                (
                                    w,
                                    Arc::new(PagePic {
                                        w: img.w,
                                        h: img.h,
                                        rgba: img.rgba,
                                    }),
                                ),
                            );
                        }
                    }
                    PdfReq::Text(i) => {
                        let t = doc.page_text(i);
                        sh2.lock().unwrap().text.insert(i, t);
                    }
                    PdfReq::Search(q) => {
                        sh2.lock().unwrap().busy = true;
                        bg.poke();
                        let r = doc.search(&q, 60);
                        let mut s = sh2.lock().unwrap();
                        s.results = Some(r);
                        s.busy = false;
                    }
                }
                bg.poke();
            }
        }
    });
    PdfView {
        path: path.into(),
        tx,
        sh,
        zoom: 1.0,
        hx: 0.0,
        cur: 0,
        text_mode: false,
        search: false,
        jump: None,
    }
}

// ------------------------------------------------------------------ Dokument

struct DocView {
    path: String,
    doc: Document,
    sel: Option<usize>,
    buf: Buffer,
    dirty: bool,
    err: Option<String>,
    /// gewählten Absatz in den sichtbaren Bereich holen
    follow: bool,
    /// waagerechte Verschiebung der Formatvorlagen-Leiste
    pill_x: f32,
}

// ------------------------------------------------------------------ Tabelle

pub(super) struct SheetView {
    path: String,
    wb: Workbook,
    si: usize,
    pub(super) sel: (u32, u32),
    sx: f32,
    pub(super) editing: bool,
    dirty: bool,
    err: Option<String>,
}

// ------------------------------------------------------------------ Code

pub(super) struct CodeTab {
    path: String,
    pub(super) buf: Buffer,
    lang: Lang,
    hx: f32,
    /// Bildlauf (Zeilen-Pixel), eigene Verwaltung je Reiter
    sy: f32,
    follow: bool,
}

pub(super) struct CodeView {
    pub(super) tabs: Vec<CodeTab>,
    cur: usize,
    find: bool,
    opt: FindOpt,
    select: bool,
    hits: usize,
}

/// Anzeigespalte (Tabulator = bis zur nächsten Vierer-Stufe) für eine Byte-Position einer Zeile
fn vcol(line: &str, byte: usize) -> usize {
    let mut c = 0;
    for ch in line[..byte.min(line.len())].chars() {
        c = if ch == '\t' { (c / 4 + 1) * 4 } else { c + 1 };
    }
    c
}

/// Byte-Position zur Anzeigespalte (nächste Zeichengrenze)
fn byte_at_vcol(line: &str, col: usize) -> usize {
    let mut c = 0;
    for (b, ch) in line.char_indices() {
        let n = if ch == '\t' { (c / 4 + 1) * 4 } else { c + 1 };
        if col <= c {
            return b;
        }
        if col < n {
            return if col - c > (n - c) / 2 {
                b + ch.len_utf8()
            } else {
                b
            };
        }
        c = n;
    }
    line.len()
}

fn kind_of(path: &str) -> Option<u8> {
    match uo::Kind::of(path) {
        uo::Kind::Pdf => Some(0),
        uo::Kind::Doc => Some(1),
        uo::Kind::Sheet => Some(2),
        uo::Kind::Text => Some(3),
        uo::Kind::Other => None,
    }
}

fn media_root() -> String {
    std::env::var("ULTAR_MEDIA").unwrap_or_else(|_| "/data".into())
}

/// Dateien einer Art im Nutzerbereich (ohne /data/ultar), neueste zuerst
fn scan(kind: u8) -> Vec<(String, u64, u64)> {
    let mut out = Vec::new();
    let root = media_root();
    let mut stack = vec![(root.clone(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let p = format!("{}/{}", d.trim_end_matches('/'), name);
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                if depth < 5
                    && !(d == root
                        && matches!(
                            name.as_str(),
                            "ultar" | "compat" | "android" | "lost+found" | "cache"
                        ))
                {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let k = kind_of(&p);
            let ok = match kind {
                3 => k == Some(3) && is_code_name(&name),
                _ => k == Some(kind),
            };
            if ok {
                let mt = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                out.push((p, m.len(), mt));
                if out.len() > 600 {
                    break;
                }
            }
        }
    }
    out.sort_by(|a, b| b.2.cmp(&a.2));
    out
}

/// Für die Code-Liste nur Dateien mit bekannter Endung (sonst landen alle Protokolle darin)
fn is_code_name(name: &str) -> bool {
    Lang::from_path(name) != Lang::Plain || name.ends_with(".txt") || name.ends_with(".log")
}

fn human_size(b: u64) -> String {
    if b < 1024 {
        format!("{b} B")
    } else if b < 1024 * 1024 {
        format!("{:.0} KB", b as f64 / 1024.0)
    } else {
        format!("{:.1} MB", b as f64 / 1048576.0)
    }
}

fn file_name(p: &str) -> String {
    std::path::Path::new(p)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Farben der Syntaxhervorhebung (dunkel / hell)
fn tk_color(k: Tk, dark: bool, text: Color) -> Color {
    match (k, dark) {
        (Tk::Keyword, true) => rgb(0xC792EA),
        (Tk::Keyword, false) => rgb(0x7C3AED),
        (Tk::Str, true) => rgb(0xC3E88D),
        (Tk::Str, false) => rgb(0x15803D),
        (Tk::Comment, true) => rgb(0x7F8A99),
        (Tk::Comment, false) => rgb(0x6B7280),
        (Tk::Num, true) => rgb(0xF78C6C),
        (Tk::Num, false) => rgb(0xC2410C),
        (Tk::Name, true) => rgb(0x82AAFF),
        (Tk::Name, false) => rgb(0x1D4ED8),
        (Tk::Punct, true) => rgb(0x89DDFF),
        (Tk::Punct, false) => rgb(0x0E7490),
        _ => text,
    }
}

const PAPER: u32 = 0xFFFFFF;
const INK: u32 = 0x1F2328;

impl App {
    fn office_cfg(&mut self) -> &mut OfficeCfg {
        if self.office.cfg.is_none() {
            let mut c: OfficeCfg = crate::sys::load("office.toml");
            if c.code_size <= 0.0 {
                c.code_size = 13.0;
            }
            self.office.cfg = Some(c);
        }
        self.office.cfg.as_mut().unwrap()
    }

    fn office_recent(&mut self, path: &str) {
        let c = self.office_cfg();
        c.recent.retain(|p| p != path);
        c.recent.insert(0, path.to_string());
        c.recent.truncate(30);
        let c = c.clone();
        let _ = crate::sys::save("office.toml", &c);
    }

    /// Kann eine Büro-App die Datei öffnen? Dann Bildschirm dafür (Dateien-App, Verbund, Agent)
    pub(super) fn office_screen_for(path: &str) -> Option<Screen> {
        let k = kind_of(path)?;
        if k == 3 {
            // nur echte Textdateien
            let b = std::fs::read(path)
                .ok()
                .map(|b| b.into_iter().take(8192).collect::<Vec<u8>>())
                .unwrap_or_default();
            if !uo::looks_text(&b) {
                return None;
            }
        }
        Some(Screen::Office(match k {
            0 => Ox::Pdf(path.into()),
            1 => Ox::Doc(path.into()),
            2 => Ox::Sheet(path.into()),
            _ => Ox::Code(path.into()),
        }))
    }

    /// Beim Öffnen eines Büro-Bildschirms: Datei laden
    pub(super) fn office_on_go(&mut self, s: &Screen) {
        let Screen::Office(x) = s else { return };
        match x {
            Ox::List(k) => {
                self.office.files = None;
                let _ = k;
            }
            Ox::Pdf(p) => {
                if self.office.pdf.as_ref().map(|v| v.path.as_str()) != Some(p.as_str()) {
                    self.office.pdf = Some(pdf_open(p, self.bg.clone()));
                }
                self.office_recent(p);
            }
            Ox::Doc(p) => {
                if self.office.doc.as_ref().map(|v| v.path.as_str()) != Some(p.as_str()) {
                    let (doc, err) = if std::path::Path::new(p).exists() {
                        match uo::read_doc(p) {
                            Ok(d) => (d, None),
                            Err(e) => (Document::default(), Some(e)),
                        }
                    } else {
                        (
                            Document {
                                blocks: vec![
                                    Block::Para(Para::new(Style::Title, "")),
                                    Block::Para(Para::new(Style::Normal, "")),
                                ],
                                ..Document::default()
                            },
                            None,
                        )
                    };
                    self.office.doc = Some(DocView {
                        path: p.clone(),
                        doc,
                        sel: None,
                        buf: Buffer::new(""),
                        dirty: false,
                        err,
                        follow: false,
                        pill_x: 0.0,
                    });
                }
                self.office_recent(p);
            }
            Ox::Sheet(p) => {
                if self.office.sheet.as_ref().map(|v| v.path.as_str()) != Some(p.as_str()) {
                    let (wb, err) = if std::path::Path::new(p).exists() {
                        match uo::read_book(p) {
                            Ok(w) => (w, None),
                            Err(e) => (Workbook::new(), Some(e)),
                        }
                    } else {
                        (Workbook::new(), None)
                    };
                    self.office.sheet = Some(SheetView {
                        path: p.clone(),
                        wb,
                        si: 0,
                        sel: (0, 0),
                        sx: 0.0,
                        editing: false,
                        dirty: false,
                        err,
                    });
                }
                self.office_recent(p);
            }
            Ox::Code(p) => {
                let v = self.office.code.get_or_insert_with(|| CodeView {
                    tabs: Vec::new(),
                    cur: 0,
                    find: false,
                    opt: FindOpt::default(),
                    select: false,
                    hits: 0,
                });
                match v.tabs.iter().position(|t| &t.path == p) {
                    Some(i) => v.cur = i,
                    None => {
                        let buf = match std::fs::read(p) {
                            Ok(b) => Buffer::from_bytes(&b),
                            Err(_) => Buffer::new(""),
                        };
                        v.tabs.push(CodeTab {
                            path: p.clone(),
                            buf,
                            lang: Lang::from_path(p),
                            hx: 0.0,
                            sy: 0.0,
                            follow: false,
                        });
                        v.cur = v.tabs.len() - 1;
                    }
                }
                self.office_recent(p);
            }
        }
    }

    /// Verlassen: offene Änderungen sichern (wie Notizen – nichts geht verloren)
    pub(super) fn office_leave(&mut self) {
        self.doc_commit();
        self.sheet_commit();
        let mut msgs = Vec::new();
        if let Some(d) = self.office.doc.as_mut() {
            if d.dirty {
                match uo::write_doc(&d.path, &d.doc) {
                    Ok(_) => d.dirty = false,
                    Err(e) => msgs.push(e),
                }
            }
        }
        if let Some(s) = self.office.sheet.as_mut() {
            if s.dirty {
                match uo::write_book(&s.path, &s.wb, s.si) {
                    Ok(_) => s.dirty = false,
                    Err(e) => msgs.push(e),
                }
            }
        }
        if let Some(c) = self.office.code.as_mut() {
            for t in &mut c.tabs {
                if t.buf.dirty {
                    match uo::write_atomic(&t.path, &t.buf.to_bytes()) {
                        Ok(_) => t.buf.dirty = false,
                        Err(e) => msgs.push(e),
                    }
                }
            }
        }
        if let Some(m) = msgs.first() {
            self.toast(format!("{}: {m}", self.t("Could not save")));
        }
    }

    pub(super) fn office_needs_tick(&self) -> bool {
        self.office.pdf.as_ref().is_some_and(|p| {
            let s = p.sh.lock().unwrap();
            !s.pending.is_empty() || s.busy || (s.info.is_none() && s.err.is_none())
        }) && matches!(
            self.stack.last().map(|s| &s.0),
            Some(Screen::Office(Ox::Pdf(_)))
        )
    }

    pub(super) fn s_office(&mut self, ui: &mut Ui, area: Rect, x: Ox, sc: &mut f32) {
        match x {
            Ox::List(k) => self.s_office_list(ui, area, k, sc),
            Ox::Pdf(p) => self.s_pdf(ui, area, &p, sc),
            Ox::Doc(p) => self.s_doc(ui, area, &p, sc),
            Ox::Sheet(p) => self.s_sheet(ui, area, &p, sc),
            Ox::Code(p) => self.s_code(ui, area, &p, sc),
        }
    }

    /// Kleine Werkzeugleiste aus Symbolknöpfen; liefert den Index des angetippten Knopfs
    fn office_toolbar(&mut self, ui: &mut Ui, r: Rect, items: &[(Ic, bool)]) -> Option<usize> {
        ui.cv
            .rect(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), ui.th.line);
        let n = items.len().max(1);
        let step = (r.w / n as f32).min(ui.dp(52.0));
        let total = step * n as f32;
        let x0 = r.x + (r.w - total) / 2.0;
        let mut hit = None;
        for (i, (ic, active)) in items.iter().enumerate() {
            let k = if ui.rtl { n - 1 - i } else { i };
            let cx = x0 + step * (k as f32 + 0.5);
            let (bg, fg) = if *active {
                (ui.th.accent.alpha(0.22), ui.th.accent)
            } else {
                (ui.th.card.alpha(0.0), ui.th.text)
            };
            if ui.icon_btn(cx, r.cy(), ui.dp(17.0), *ic, bg, fg) {
                hit = Some(i);
            }
        }
        hit
    }

    /// Den Agenten zu dieser Datei fragen (Feld vorbefüllen, Gespräch öffnen)
    fn office_ask_agent(&mut self, path: &str, what: &str) {
        self.office_leave();
        let q = match self.lang() {
            "ar" => format!("{what} الملف {path}"),
            "de" => format!("{what}: Datei {path}"),
            _ => format!("{what}: file {path}"),
        };
        self.fields.insert("chat", q);
        self.go(Screen::Agent);
        self.focus = Some("chat");
        self.focus_reset = true;
    }

    // ------------------------------------------------------------------ Liste

    fn s_office_list(&mut self, ui: &mut Ui, area: Rect, kind: u8, sc: &mut f32) {
        let title = self.t(KIND_NAMES[(kind as usize).min(3)]);
        let area = self.header(ui, area, title);
        let fresh = self
            .office
            .files
            .as_ref()
            .is_some_and(|f| f.0 == kind && f.1.elapsed().as_secs() < 4);
        if !fresh {
            self.office.files = Some((kind, Instant::now(), scan(kind)));
        }
        let files = self
            .office
            .files
            .as_ref()
            .map(|f| f.2.clone())
            .unwrap_or_default();
        let recent: Vec<String> = self
            .office_cfg()
            .recent
            .iter()
            .filter(|p| kind_of(p) == Some(kind) && std::path::Path::new(p).exists())
            .take(4)
            .cloned()
            .collect();
        // Neu-Knöpfe (außer PDF)
        let m = ui.dp(16.0);
        let mut y = area.y + ui.dp(6.0);
        if kind != 0 {
            let opts: &[(&str, &str)] = match kind {
                1 => &[("New document", "docx"), ("Markdown", "md")],
                2 => &[("New sheet", "xlsx"), ("CSV", "csv")],
                _ => &[
                    ("New file", "txt"),
                    ("Python", "py"),
                    ("Rust", "rs"),
                    ("HTML", "html"),
                ],
            };
            let mut x = m;
            for (label, ext) in opts {
                let l = format!("+ {}", self.t(label));
                let active = *label == opts[0].0;
                let pw = ui.text_w(&l, 13.0, active) + ui.dp(26.0);
                let px = if ui.rtl { ui.w - x - pw } else { x };
                let (w, tap) = ui.pill(px, y, &l, active);
                x += w + ui.dp(8.0);
                if tap {
                    let p = self.office_new_path(ext);
                    let _ = match kind {
                        1 if *ext == "docx" => uo::create(&p, "# \n"),
                        1 => uo::create(&p, "# \n"),
                        2 => uo::write_book(&p, &Workbook::new(), 0),
                        _ => uo::write_atomic(&p, b""),
                    };
                    if let Some(s) = Self::office_screen_for(&p) {
                        self.go(s);
                        if kind == 1 {
                            self.doc_select(0);
                        } else if kind == 3 {
                            self.focus = Some("office_ed");
                            self.focus_reset = true;
                        }
                    }
                    ui.dirty = true;
                    return;
                }
            }
            y += ui.dp(44.0);
        }
        let body = Rect::new(area.x, y, area.w, (area.bottom() - y).max(0.0));
        if files.is_empty() && recent.is_empty() {
            let ic = list_icon(kind);
            self.empty(
                ui,
                body,
                ic,
                self.t("No files yet"),
                self.t("Files in Documents, Download and other folders appear here."),
            );
            return;
        }
        ui.scroll_begin(body, sc);
        let mut yy = body.y - *sc;
        let mut open = None;
        if !recent.is_empty() {
            ui.section(&mut yy, self.t("Recent"));
            for p in &recent {
                let sub = std::path::Path::new(p)
                    .parent()
                    .map(|d| {
                        d.to_string_lossy()
                            .replace(&media_root(), "")
                            .trim_start_matches('/')
                            .to_string()
                    })
                    .unwrap_or_default();
                if ui.row(
                    &mut yy,
                    Some(list_icon(kind)),
                    &file_name(p),
                    Some(&sub),
                    None,
                    true,
                ) {
                    open = Some(p.clone());
                }
            }
        }
        ui.section(&mut yy, self.t("All files"));
        let now = crate::sys::unix_now() as u64;
        for (p, size, mt) in files.iter().take(300) {
            let age = now.saturating_sub(*mt);
            let when = if age < 3600 {
                format!("{} min", age / 60)
            } else if age < 86400 {
                format!("{} h", age / 3600)
            } else {
                format!("{} d", age / 86400)
            };
            let sub = format!("{} · {}", human_size(*size), when);
            if ui.row(
                &mut yy,
                Some(list_icon(kind)),
                &file_name(p),
                Some(&sub),
                None,
                true,
            ) {
                open = Some(p.clone());
            }
        }
        let h = yy + *sc - body.y + ui.dp(40.0);
        ui.scroll_end(body, h, sc);
        if let Some(p) = open {
            if let Some(s) = Self::office_screen_for(&p) {
                self.go(s);
            } else {
                self.toast(self.t("Not a text file").into());
            }
            ui.dirty = true;
        }
    }

    fn office_new_path(&self, ext: &str) -> String {
        let dir = format!("{}/Documents", media_root());
        let _ = std::fs::create_dir_all(&dir);
        let base = match ext {
            "docx" | "md" => self.t("Document"),
            "xlsx" | "csv" => self.t("Sheet"),
            _ => self.t("New file"),
        };
        for i in 1..1000 {
            let p = format!("{dir}/{base} {i}.{ext}");
            if !std::path::Path::new(&p).exists() {
                return p;
            }
        }
        format!("{dir}/{base}.{ext}")
    }

    // ------------------------------------------------------------------ PDF-Leser

    fn s_pdf(&mut self, ui: &mut Ui, area: Rect, path: &str, sc: &mut f32) {
        let Some(mut v) = self.office.pdf.take() else {
            self.office_on_go(&Screen::Office(Ox::Pdf(path.into())));
            ui.dirty = true;
            return;
        };
        let name = file_name(path);
        let area = self.header(ui, area, &name);
        let (info, err) = {
            let s = v.sh.lock().unwrap();
            (s.info.clone(), s.err.clone())
        };
        // Werkzeugleiste: Suchen, Text, Verkleinern, Vergrößern, Agent
        let tb = Rect::new(area.x, area.y, area.w, ui.dp(46.0));
        let items = [
            (Ic::Search, v.search),
            (Ic::Doc, v.text_mode),
            (Ic::Svg("minus"), false),
            (Ic::Plus, v.zoom > 1.01),
            (Ic::Spark, false),
        ];
        match self.office_toolbar(ui, tb, &items) {
            Some(0) => {
                v.search = !v.search;
                if v.search {
                    self.focus = Some("pdf_q");
                    self.focus_reset = true;
                } else {
                    self.focus = None;
                    self.focus_reset = true;
                }
                ui.dirty = true;
            }
            Some(1) => {
                v.text_mode = !v.text_mode;
                ui.dirty = true;
            }
            Some(2) => {
                let old = v.zoom;
                v.zoom = (v.zoom / 1.25).max(1.0);
                *sc *= v.zoom / old;
                v.hx *= v.zoom / old;
                ui.dirty = true;
            }
            Some(3) => {
                let old = v.zoom;
                v.zoom = (v.zoom * 1.25).min(4.0);
                *sc *= v.zoom / old;
                v.hx *= v.zoom / old;
                ui.dirty = true;
            }
            Some(4) => {
                let what = self.t("Summarize").to_string();
                self.office.pdf = Some(v);
                self.office_ask_agent(path, &what);
                ui.dirty = true;
                return;
            }
            _ => {}
        }
        let mut top = tb.bottom();
        if v.search {
            let fr = Rect::new(
                ui.dp(12.0),
                top + ui.dp(6.0),
                ui.w - ui.dp(24.0),
                ui.dp(44.0),
            );
            let q = self.f("pdf_q");
            ui.field(fr, "pdf_q", &q, self.t("Search in document"));
            top = fr.bottom() + ui.dp(6.0);
            let (busy, res) = {
                let s = v.sh.lock().unwrap();
                (s.busy, s.results.clone())
            };
            if busy {
                let c = ui.th.sub;
                ui.label(
                    Rect::new(ui.dp(16.0), top, ui.w - ui.dp(32.0), ui.dp(24.0)),
                    self.t("Searching…"),
                    13.0,
                    c,
                    false,
                    Al::Start,
                );
                top += ui.dp(26.0);
            } else if let Some(res) = res {
                let rh = ui.dp(40.0);
                let shown = res.len().min(5);
                if res.is_empty() {
                    let c = ui.th.sub;
                    ui.label(
                        Rect::new(ui.dp(16.0), top, ui.w - ui.dp(32.0), ui.dp(24.0)),
                        self.t("No results"),
                        13.0,
                        c,
                        false,
                        Al::Start,
                    );
                    top += ui.dp(26.0);
                }
                for (pg, line) in res.iter().take(shown) {
                    let r = Rect::new(ui.dp(12.0), top, ui.w - ui.dp(24.0), rh - ui.dp(4.0));
                    ui.card_h(r, ui.held(r));
                    let (c, cs) = (ui.th.text, ui.th.accent);
                    let lab = format!("{}", pg + 1);
                    ui.label(
                        Rect::new(r.x + ui.dp(10.0), r.y, ui.dp(36.0), r.h),
                        &lab,
                        13.0,
                        cs,
                        true,
                        Al::Start,
                    );
                    let lf = ui.fit(line, 13.0, false, r.w - ui.dp(56.0));
                    ui.label(
                        Rect::new(r.x + ui.dp(46.0), r.y, r.w - ui.dp(56.0), r.h),
                        &lf,
                        13.0,
                        c,
                        false,
                        Al::Start,
                    );
                    if ui.tap(r) {
                        v.jump = Some(*pg);
                        v.text_mode = false;
                        self.focus = None;
                        self.focus_reset = true;
                        ui.dirty = true;
                    }
                    top += rh;
                }
                if res.len() > shown {
                    let c = ui.th.sub;
                    let t = format!("+{} …", res.len() - shown);
                    ui.label(
                        Rect::new(ui.dp(16.0), top, ui.w - ui.dp(32.0), ui.dp(20.0)),
                        &t,
                        12.0,
                        c,
                        false,
                        Al::Start,
                    );
                    top += ui.dp(22.0);
                }
            }
        }
        let body = Rect::new(0.0, top, ui.w, (area.bottom() - top).max(0.0));
        if let Some(e) = err {
            let c = ui.th.danger;
            ui.para(
                ui.dp(20.0),
                body.y + ui.dp(30.0),
                ui.w - ui.dp(40.0),
                &format!("{}: {e}", self.t("Could not open")),
                15.0,
                c,
            );
            self.office.pdf = Some(v);
            return;
        }
        let Some((n, sizes)) = info else {
            let c = ui.th.sub;
            ui.label(body, self.t("Opening…"), 15.0, c, false, Al::Center);
            self.office.pdf = Some(v);
            return;
        };
        if v.text_mode {
            // Lesemodus: Text der aktuellen Seite, gut für kleine Bildschirme und zum Kopieren
            let cur = v.cur.min(n.saturating_sub(1));
            let txt = v.sh.lock().unwrap().text.get(&cur).cloned();
            ui.scroll_begin(body, sc);
            let mut y = body.y - *sc + ui.dp(10.0);
            let m = ui.dp(18.0);
            let bar = Rect::new(m, y, ui.w - 2.0 * m, ui.dp(40.0));
            let (c, cs) = (ui.th.text, ui.th.sub);
            let lab = format!("{} {} / {}", self.t("Page"), cur + 1, n);
            ui.label(
                Rect::new(bar.x, bar.y, bar.w / 2.0, bar.h),
                &lab,
                14.0,
                cs,
                true,
                Al::Start,
            );
            // Blättern und Kopieren
            let bx = |k: f32| {
                if ui.rtl {
                    bar.x + ui.dp(20.0) + k * ui.dp(44.0)
                } else {
                    bar.right() - ui.dp(20.0) - k * ui.dp(44.0)
                }
            };
            let (x0, x1, x2) = (bx(0.0), bx(1.0), bx(2.0));
            let tc = ui.th.text;
            let nb = ui.th.card;
            if ui.icon_btn(x0, bar.cy(), ui.dp(16.0), Ic::Copy, nb, tc) {
                if let Some(t) = &txt {
                    self.copy_text(t);
                }
            }
            if ui.icon_btn(x1, bar.cy(), ui.dp(16.0), Ic::Svg("fwd"), nb, tc) && cur + 1 < n {
                v.cur = cur + 1;
                *sc = 0.0;
                ui.dirty = true;
            }
            if ui.icon_btn(x2, bar.cy(), ui.dp(16.0), Ic::Back, nb, tc) && cur > 0 {
                v.cur = cur - 1;
                *sc = 0.0;
                ui.dirty = true;
            }
            y += ui.dp(50.0);
            let h = match &txt {
                Some(t) if t.trim().is_empty() => ui.para(
                    m,
                    y,
                    ui.w - 2.0 * m,
                    self.t("No text on this page (scanned image?)"),
                    15.0,
                    cs,
                ),
                Some(t) => ui.para(m, y, ui.w - 2.0 * m, t, 16.0, c),
                None => {
                    if !v.sh.lock().unwrap().text.contains_key(&cur) {
                        let _ = v.tx.send(PdfReq::Text(cur));
                    }
                    ui.para(m, y, ui.w - 2.0 * m, self.t("Reading…"), 15.0, cs)
                }
            };
            ui.scroll_end(body, h + ui.dp(80.0), sc);
            self.office.pdf = Some(v);
            return;
        }
        // Seitenansicht: alle Seiten untereinander, Breite = Bildschirm × Zoom
        let m = ui.dp(10.0);
        let gap = ui.dp(12.0);
        let pw = ((body.w - 2.0 * m) * v.zoom).max(ui.dp(100.0));
        let heights: Vec<f32> = sizes.iter().map(|(w, h)| pw * h / w.max(1.0)).collect();
        let mut tops = Vec::with_capacity(n);
        let mut acc = ui.dp(8.0);
        for h in &heights {
            tops.push(acc);
            acc += h + gap;
        }
        if let Some(j) = v.jump.take() {
            *sc = tops.get(j).copied().unwrap_or(0.0) - ui.dp(4.0);
        }
        // Doppeltippen: Zoom 1× ↔ 2×
        let dbl_tap = ui.tap(body) && {
            let d = self
                .office
                .last_tap
                .is_some_and(|t| t.elapsed().as_millis() < 320);
            self.office.last_tap = if d { None } else { Some(Instant::now()) };
            d
        };
        if dbl_tap {
            let old = v.zoom;
            v.zoom = if v.zoom > 1.01 { 1.0 } else { 2.0 };
            let k = v.zoom / old;
            let cy = ui.p.py - body.y + *sc;
            *sc = cy * k - (ui.p.py - body.y);
            v.hx = ((ui.p.px + v.hx) * k - ui.p.px).max(0.0);
            ui.dirty = true;
        }
        let maxhx = (pw + 2.0 * m - body.w).max(0.0);
        if v.zoom > 1.01
            && ui.p.down
            && ui.p.moved
            && body.contains(ui.p.px, ui.p.py)
            && ui.p.capture.is_none()
        {
            v.hx = (v.hx - ui.p.dx).clamp(0.0, maxhx);
            ui.p.dx = 0.0;
        }
        v.hx = v.hx.clamp(0.0, maxhx);
        ui.scroll_begin(body, sc);
        let want_w = (pw.round() as u32 / 32 * 32).clamp(64, 4096);
        let mut wanted = HashSet::new();
        let mut visible_first = None;
        let mid = *sc + body.h / 2.0;
        for i in 0..n {
            let y = body.y + tops[i] - *sc;
            let r = Rect::new(m - v.hx, y, pw, heights[i]);
            if tops[i] <= mid && mid <= tops[i] + heights[i] + gap {
                v.cur = i;
            }
            let near = r.bottom() > body.y - body.h && r.y < body.bottom() + body.h;
            if !near {
                continue;
            }
            wanted.insert(i);
            if !ui.visible(r) {
                continue;
            }
            visible_first.get_or_insert(i);
            ui.cv.soft_shadow(r, ui.dp(4.0), rgb(0));
            ui.cv.rect(r, rgb(PAPER));
            let pic = v.sh.lock().unwrap().pics.get(&i).cloned();
            match pic {
                Some((w, p)) => {
                    ui.cv.blit_rgba(&p.rgba, p.w, p.h, r);
                    if w != want_w {
                        self.pdf_request(&v, i, want_w);
                    }
                }
                None => {
                    self.pdf_request(&v, i, want_w);
                    ui.label(r, "…", 22.0, rgb(0x9CA3AF), true, Al::Center);
                }
            }
        }
        // auch knapp außerhalb vorbereiten
        for i in wanted.clone() {
            let y = body.y + tops[i] - *sc;
            if (y > body.bottom() || y + heights[i] < body.y)
                && !v.sh.lock().unwrap().pics.contains_key(&i)
            {
                self.pdf_request(&v, i, want_w);
            }
        }
        {
            let mut s = v.sh.lock().unwrap();
            // Speicher: nur Seiten um die aktuelle behalten
            let cur = v.cur;
            s.pics
                .retain(|k, _| (*k as isize - cur as isize).abs() <= 4);
            s.wanted = wanted;
        }
        ui.scroll_end(body, acc + ui.dp(60.0), sc);
        // Seitenanzeige unten
        let lab = format!("{} / {}", v.cur + 1, n);
        let w = ui.text_w(&lab, 13.0, true) + ui.dp(26.0);
        let r = Rect::new(
            (ui.w - w) / 2.0,
            body.bottom() - ui.dp(46.0),
            w,
            ui.dp(32.0),
        );
        ui.cv.rrect(r, r.h / 2.0, rgb(0x111318).alpha(0.82));
        ui.label(r, &lab, 13.0, rgb(0xFFFFFF), true, Al::Center);
        if v.zoom > 1.01 {
            let z = format!("{:.0} %", v.zoom * 100.0);
            let zw = ui.text_w(&z, 12.0, false) + ui.dp(20.0);
            let zr = Rect::new(
                if ui.rtl {
                    ui.dp(12.0)
                } else {
                    ui.w - zw - ui.dp(12.0)
                },
                body.bottom() - ui.dp(46.0),
                zw,
                ui.dp(32.0),
            );
            ui.cv.rrect(zr, zr.h / 2.0, rgb(0x111318).alpha(0.7));
            ui.label(zr, &z, 12.0, rgb(0xFFFFFF), false, Al::Center);
        }
        self.office.pdf = Some(v);
    }

    fn pdf_request(&self, v: &PdfView, i: usize, w: u32) {
        let mut s = v.sh.lock().unwrap();
        if s.pending.iter().any(|(p, _)| *p == i) {
            return;
        }
        s.pending.insert((i, w));
        s.wanted.insert(i);
        let _ = v.tx.send(PdfReq::Render(i, w));
    }

    pub(super) fn office_pdf_search(&mut self) {
        let q = self.f("pdf_q");
        if let Some(v) = &self.office.pdf {
            if !q.trim().is_empty() {
                v.sh.lock().unwrap().results = None;
                let _ = v.tx.send(PdfReq::Search(q));
            }
        }
        self.focus = None;
        self.focus_reset = true;
    }

    // ------------------------------------------------------------------ Dokumente (Word)

    pub(super) fn doc_select(&mut self, i: usize) {
        self.doc_commit();
        if let Some(d) = self.office.doc.as_mut() {
            if let Some(Block::Para(p)) = d.doc.blocks.get(i) {
                d.buf = Buffer::new(&p.text());
                d.buf.cur = d.buf.text.len();
                d.sel = Some(i);
                d.follow = true;
                self.focus = Some("office_ed");
                self.focus_reset = true;
            }
        }
    }

    /// Bearbeiteten Absatz in das Dokument zurückschreiben
    fn doc_commit(&mut self) {
        if let Some(d) = self.office.doc.as_mut() {
            if let Some(i) = d.sel {
                if let Some(Block::Para(p)) = d.doc.blocks.get_mut(i) {
                    if p.text() != d.buf.text {
                        p.set_text(&d.buf.text);
                        d.dirty = true;
                    }
                }
            }
        }
    }

    fn s_doc(&mut self, ui: &mut Ui, area: Rect, path: &str, sc: &mut f32) {
        let Some(mut d) = self.office.doc.take() else {
            self.office_on_go(&Screen::Office(Ox::Doc(path.into())));
            ui.dirty = true;
            return;
        };
        if std::mem::take(&mut self.office.debug_edit) {
            self.office.doc = Some(d);
            self.doc_select(1);
            if let Some(dd) = self.office.doc.as_mut() {
                dd.buf.cur = dd.buf.text.len() / 2;
            }
            d = self.office.doc.take().unwrap();
        }
        let title = format!(
            "{}{}",
            if d.dirty || d.buf.dirty { "• " } else { "" },
            file_name(path)
        );
        let area = self.header(ui, area, &title);
        let editing = d.sel.is_some() && self.focus == Some("office_ed");
        if !editing && d.sel.is_some() && self.focus.is_none() {
            // Tastatur geschlossen: Absatz übernehmen, Auswahl bleibt für die Formatleiste
            if let Some(Block::Para(p)) = d.sel.and_then(|i| d.doc.blocks.get_mut(i)) {
                if p.text() != d.buf.text {
                    p.set_text(&d.buf.text);
                    d.dirty = true;
                }
            }
        }
        // Werkzeugleiste
        let tb = Rect::new(area.x, area.y, area.w, ui.dp(46.0));
        let mut top = tb.bottom();
        if let Some(si) = d.sel {
            let style = match d.doc.blocks.get(si) {
                Some(Block::Para(p)) => p.style,
                _ => Style::Normal,
            };
            let bold = matches!(d.doc.blocks.get(si), Some(Block::Para(p)) if p.all_bold());
            let al = match d.doc.blocks.get(si) {
                Some(Block::Para(p)) => p.align,
                _ => Align::Start,
            };
            let al_ic = match al {
                Align::Center => Ic::Svg("align-c"),
                Align::End => Ic::Svg("align-e"),
                _ => Ic::Svg("align-s"),
            };
            let items = [
                (Ic::Svg("bold"), bold),
                (al_ic, al != Align::Start),
                (Ic::Back, false),
                (Ic::Svg("fwd"), false),
                (Ic::Plus, false),
                (Ic::Trash, false),
                (Ic::Check, false),
            ];
            match self.office_toolbar(ui, tb, &items) {
                Some(0) => {
                    if let Some(Block::Para(p)) = d.doc.blocks.get_mut(si) {
                        p.set_text(&d.buf.text);
                        for r in &mut p.runs {
                            r.bold = !bold;
                        }
                        d.dirty = true;
                    }
                }
                Some(1) => {
                    if let Some(Block::Para(p)) = d.doc.blocks.get_mut(si) {
                        p.align = match al {
                            Align::Start => Align::Center,
                            Align::Center => Align::End,
                            _ => Align::Start,
                        };
                        d.dirty = true;
                    }
                }
                Some(2) => {
                    if ui.rtl {
                        d.buf.right(false)
                    } else {
                        d.buf.left(false)
                    }
                }
                Some(3) => {
                    if ui.rtl {
                        d.buf.left(false)
                    } else {
                        d.buf.right(false)
                    }
                }
                Some(4) => {
                    // neuer Absatz darunter
                    if let Some(Block::Para(p)) = d.doc.blocks.get_mut(si) {
                        p.set_text(&d.buf.text);
                    }
                    let st = if matches!(style, Style::Bullet | Style::Number) {
                        style
                    } else {
                        Style::Normal
                    };
                    d.doc.blocks.insert(si + 1, Block::Para(Para::new(st, "")));
                    d.sel = Some(si + 1);
                    d.buf = Buffer::new("");
                    d.dirty = true;
                    self.focus = Some("office_ed");
                    self.focus_reset = true;
                }
                Some(5) => {
                    d.doc.blocks.remove(si);
                    if d.doc.blocks.is_empty() {
                        d.doc.blocks.push(Block::Para(Para::new(Style::Normal, "")));
                    }
                    d.sel = None;
                    d.dirty = true;
                    self.focus = None;
                    self.focus_reset = true;
                }
                Some(6) => {
                    if let Some(Block::Para(p)) = d.doc.blocks.get_mut(si) {
                        if p.text() != d.buf.text {
                            p.set_text(&d.buf.text);
                            d.dirty = true;
                        }
                    }
                    d.sel = None;
                    self.focus = None;
                    self.focus_reset = true;
                }
                _ => {}
            }
            if d.sel.is_some() {
                // Formatvorlagen als eine Pillenreihe; zu breit → seitlich wischen
                let names: [(&str, Style); 8] = [
                    ("Text", Style::Normal),
                    ("Title", Style::Title),
                    ("H1", Style::H1),
                    ("H2", Style::H2),
                    ("H3", Style::H3),
                    ("• List", Style::Bullet),
                    ("1. List", Style::Number),
                    ("Quote", Style::Quote),
                ];
                let py = top + ui.dp(6.0);
                let row = Rect::new(0.0, py - ui.dp(2.0), ui.w, ui.dp(38.0));
                if ui.p.down
                    && ui.p.moved
                    && row.contains(ui.p.px, ui.p.py)
                    && ui.p.capture.is_none()
                {
                    d.pill_x -= if ui.rtl { -ui.p.dx } else { ui.p.dx };
                    ui.p.dx = 0.0;
                }
                let widths: Vec<f32> = names
                    .iter()
                    .map(|(lab, st)| ui.text_w(self.t(lab), 13.0, *st == style) + ui.dp(26.0))
                    .collect();
                let total: f32 = widths.iter().sum::<f32>()
                    + ui.dp(6.0) * (widths.len() as f32 - 1.0)
                    + ui.dp(24.0);
                d.pill_x = d.pill_x.clamp(0.0, (total - ui.w).max(0.0));
                let old_clip = ui.clip;
                ui.set_clip(Some(row));
                let mut x = ui.dp(12.0) - d.pill_x;
                for (k, (lab, st)) in names.iter().enumerate() {
                    let l = self.t(lab);
                    let w = widths[k];
                    let px = if ui.rtl { ui.w - x - w } else { x };
                    let (_, tap) = ui.pill(px, py, l, *st == style);
                    x += w + ui.dp(6.0);
                    if tap {
                        if let Some(Block::Para(p)) = d.sel.and_then(|i| d.doc.blocks.get_mut(i)) {
                            p.style = *st;
                            d.dirty = true;
                        }
                        ui.dirty = true;
                    }
                }
                ui.set_clip(old_clip);
                top = py + ui.dp(40.0);
            }
        } else {
            let items = [
                (Ic::Plus, false),
                (Ic::Download, false),
                (Ic::Copy, false),
                (Ic::Spark, false),
            ];
            match self.office_toolbar(ui, tb, &items) {
                Some(0) => {
                    d.doc.blocks.push(Block::Para(Para::new(Style::Normal, "")));
                    let i = d.doc.blocks.len() - 1;
                    d.sel = Some(i);
                    d.buf = Buffer::new("");
                    d.dirty = true;
                    self.focus = Some("office_ed");
                    self.focus_reset = true;
                    *sc = f32::MAX / 4.0;
                }
                Some(1) => match uo::write_doc(&d.path, &d.doc) {
                    Ok(_) => {
                        d.dirty = false;
                        self.toast(self.t("Saved").into());
                    }
                    Err(e) => self.toast(format!("{}: {e}", self.t("Could not save"))),
                },
                Some(2) => {
                    let t = d.doc.to_text();
                    self.copy_text(&t);
                }
                Some(3) => {
                    if d.dirty {
                        let _ = uo::write_doc(&d.path, &d.doc).map(|_| d.dirty = false);
                    }
                    let what = self.t("Read and help me with").to_string();
                    self.office.doc = Some(d);
                    self.office_ask_agent(path, &what);
                    ui.dirty = true;
                    return;
                }
                _ => {}
            }
        }
        let body = Rect::new(0.0, top, ui.w, (area.bottom() - top - ui.dp(24.0)).max(0.0));
        if let Some(e) = &d.err {
            let c = ui.th.danger;
            ui.para(
                ui.dp(16.0),
                body.y + ui.dp(6.0),
                ui.w - ui.dp(32.0),
                e,
                13.0,
                c,
            );
        }
        // Papier
        ui.scroll_begin(body, sc);
        let pm = ui.dp(10.0);
        let paper_w = (body.w - 2.0 * pm).min(ui.dp(820.0));
        let px = (body.w - paper_w) / 2.0;
        let inner_x = px + ui.dp(20.0);
        let inner_w = paper_w - ui.dp(40.0);
        let k = 1.25; // Punkt → dp
        let mut y = body.y - *sc + ui.dp(10.0) + ui.dp(22.0);
        let paper_top = body.y - *sc + ui.dp(10.0);
        // Höhe erst nach dem Zeichnen bekannt: Papier zuerst groß genug zeichnen (Vorjahreshöhe aus Bildlauf)
        let est = SCRATCH_H.with(|h| h.get()).max(body.h);
        ui.cv
            .soft_shadow(Rect::new(px, paper_top, paper_w, est), ui.dp(6.0), rgb(0));
        ui.cv
            .rect(Rect::new(px, paper_top, paper_w, est), rgb(PAPER));
        let ink = rgb(INK);
        let mut num = 0;
        let mut tap_block = None;
        let mut sel_rect: Option<(f32, f32)> = None;
        let blocks = d.doc.blocks.clone();
        for (i, b) in blocks.iter().enumerate() {
            match b {
                Block::Para(p) => {
                    let sel = d.sel == Some(i);
                    let fs = p.style.size() * k * 0.9;
                    let bold = p.style.bold() || p.all_bold();
                    if p.style == Style::Number {
                        num += 1;
                    } else {
                        num = 0;
                    }
                    let (ind, prefix) = match p.style {
                        Style::Bullet => (ui.dp(22.0), "•".to_string()),
                        Style::Number => (ui.dp(26.0), format!("{num}.")),
                        Style::Quote => (ui.dp(16.0), String::new()),
                        _ => (0.0, String::new()),
                    };
                    let mut text = if sel { d.buf.text.clone() } else { p.text() };
                    if sel && self.focus == Some("office_ed") {
                        let c = d.buf.cur.min(text.len());
                        text.insert(c, '|');
                    }
                    let shown = if text.is_empty() && !sel {
                        if p.style == Style::Title {
                            self.t("Title").to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        text.clone()
                    };
                    let rtl = uo::doc::is_rtl(&shown) || (shown.is_empty() && ui.rtl);
                    let tw = inner_w - ind;
                    let sz = ui.dp(fs);
                    let (mw, mh) =
                        ui.tx
                            .measure(if shown.is_empty() { " " } else { &shown }, sz, bold, tw);
                    let pad_top = match p.style {
                        Style::Title => ui.dp(4.0),
                        Style::H1 | Style::H2 | Style::H3 => ui.dp(10.0),
                        _ => 0.0,
                    };
                    y += pad_top;
                    let h = mh.max(sz * 1.35);
                    let br = Rect::new(
                        inner_x - ui.dp(8.0),
                        y - ui.dp(4.0),
                        inner_w + ui.dp(16.0),
                        h + ui.dp(8.0),
                    );
                    if sel {
                        ui.cv.rrect(br, ui.dp(8.0), ui.th.accent.alpha(0.16));
                    }
                    if p.style == Style::Quote {
                        let qx = if rtl {
                            inner_x + inner_w - ui.dp(3.0)
                        } else {
                            inner_x
                        };
                        ui.cv.rect(Rect::new(qx, y, ui.dp(3.0), h), rgb(0xC9A227));
                    }
                    if p.style == Style::Code {
                        ui.cv.rrect(
                            Rect::new(
                                inner_x - ui.dp(4.0),
                                y - ui.dp(2.0),
                                inner_w + ui.dp(8.0),
                                h + ui.dp(4.0),
                            ),
                            ui.dp(4.0),
                            rgb(0xF3F4F6),
                        );
                    }
                    if !prefix.is_empty() {
                        let pxx = if rtl {
                            inner_x + inner_w - ind + ui.dp(4.0)
                        } else {
                            inner_x
                        };
                        ui.tx.draw(ui.cv, &prefix, pxx, y, sz, ink, false, 0.0);
                    }
                    let col = if shown == self.t("Title") && p.text().is_empty() {
                        rgb(0x9CA3AF)
                    } else if p.style == Style::Quote {
                        rgb(0x555555)
                    } else if matches!(p.style, Style::H1) {
                        rgb(0x8A6D0B)
                    } else {
                        ink
                    };
                    // Ausrichtung: Text einzeilig messen, sonst über die Breite umbrechen
                    let tx0 = if rtl { inner_x } else { inner_x + ind };
                    let off = match (p.align, rtl) {
                        (Align::Center, _) if mh <= sz * 1.4 => (tw - mw) / 2.0,
                        (Align::End, false) if mh <= sz * 1.4 => tw - mw,
                        (Align::Start, true) if mh <= sz * 1.4 => 0.0,
                        _ => 0.0,
                    };
                    if ui.visible(br) && !shown.is_empty() {
                        ui.tx
                            .draw(ui.cv, &shown, tx0 + off.max(0.0), y, sz, col, bold, tw);
                    }
                    if ui.tap(br) && !sel {
                        tap_block = Some(i);
                    }
                    if sel {
                        sel_rect = Some((y - paper_top, h));
                    }
                    y += h + match p.style {
                        Style::Bullet | Style::Number | Style::Code => ui.dp(4.0),
                        Style::Title => ui.dp(14.0),
                        _ => ui.dp(10.0),
                    };
                }
                Block::Table(t) => {
                    num = 0;
                    let cols = t.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
                    let cw = inner_w / cols as f32;
                    let rh = ui.dp(30.0);
                    for (ri, row) in t.iter().enumerate() {
                        for c in 0..cols {
                            let cc = if ui.rtl { cols - 1 - c } else { c };
                            let r = Rect::new(inner_x + cc as f32 * cw, y, cw, rh);
                            ui.cv.rect(Rect::new(r.x, r.y, r.w, 1.0), rgb(0xD1D5DB));
                            ui.cv.rect(Rect::new(r.x, r.y, 1.0, r.h), rgb(0xD1D5DB));
                            if ri == 0 {
                                ui.cv.rect(
                                    Rect::new(r.x + 1.0, r.y + 1.0, r.w - 1.0, r.h - 1.0),
                                    rgb(0xF3F4F6),
                                );
                            }
                            let s = row.get(c).map(|s| s.as_str()).unwrap_or("");
                            let f = ui.fit(s, 12.0, ri == 0, cw - ui.dp(10.0));
                            let old = ui.rtl;
                            ui.label(
                                Rect::new(r.x + ui.dp(5.0), r.y, cw - ui.dp(10.0), rh),
                                &f,
                                12.0,
                                ink,
                                ri == 0,
                                Al::Start,
                            );
                            ui.rtl = old;
                        }
                        y += rh;
                    }
                    ui.cv
                        .rect(Rect::new(inner_x, y, inner_w, 1.0), rgb(0xD1D5DB));
                    ui.cv.rect(
                        Rect::new(
                            inner_x + inner_w,
                            y - rh * t.len() as f32,
                            1.0,
                            rh * t.len() as f32,
                        ),
                        rgb(0xD1D5DB),
                    );
                    y += ui.dp(12.0);
                }
                // the old shell shows a picture only as a framed place (the Word app draws it)
                Block::Picture(_) => {
                    num = 0;
                    let h = ui.dp(60.0);
                    ui.cv.rect(Rect::new(inner_x, y, inner_w, h), rgb(0xF3F4F6));
                    y += h + ui.dp(12.0);
                }
            }
        }
        // Tippen unter den letzten Absatz: ans Ende schreiben
        let tail = Rect::new(px, y, paper_w, ui.dp(80.0));
        if ui.tap(tail) && d.sel.is_none() {
            let last_empty =
                matches!(d.doc.blocks.last(), Some(Block::Para(p)) if p.text().is_empty());
            if !last_empty {
                d.doc.blocks.push(Block::Para(Para::new(Style::Normal, "")));
            }
            tap_block = Some(d.doc.blocks.len() - 1);
        }
        let content_h = y - paper_top + ui.dp(60.0);
        SCRATCH_H.with(|h| h.set(content_h));
        // gewählten Absatz (mit Schreibmarke) sichtbar halten – auch wenn die Tastatur Platz nimmt
        if let Some((ry, rh)) = sel_rect {
            let top_ok = ui.dp(10.0) + ry - ui.dp(12.0);
            let bot = ui.dp(10.0) + ry + rh + ui.dp(12.0);
            if d.follow || self.focus == Some("office_ed") {
                if top_ok < *sc {
                    *sc = top_ok.max(0.0);
                    ui.dirty = true;
                } else if bot > *sc + body.h && rh < body.h {
                    *sc = bot - body.h;
                    ui.dirty = true;
                }
                d.follow = false;
            }
        }
        ui.scroll_end(body, content_h + ui.dp(20.0), sc);
        // Fußzeile: Wörter
        let words = d.doc.word_count();
        let state = if d.dirty {
            self.t("Not saved")
        } else {
            self.t("Saved")
        };
        let foot = if self.lang() == "ar" {
            format!("{}: {} · {}", self.t("words"), words, state)
        } else {
            format!("{} {} · {}", words, self.t("words"), state)
        };
        let c = ui.th.sub;
        ui.label(
            Rect::new(
                ui.dp(16.0),
                area.bottom() - ui.dp(22.0),
                ui.w - ui.dp(32.0),
                ui.dp(20.0),
            ),
            &foot,
            12.0,
            c,
            false,
            Al::Start,
        );
        self.office.doc = Some(d);
        if let Some(i) = tap_block {
            self.doc_select(i);
            ui.dirty = true;
        }
    }

    /// Tastatur im Dokument/Code-Editor
    pub(super) fn office_kb(&mut self, o: crate::kbd::KbOut) {
        use crate::kbd::KbOut;
        match o {
            KbOut::Insert(t) => self.office_insert(&t),
            KbOut::Backspace => self.office_key(crate::ui::Key::Backspace),
            KbOut::Enter => self.office_key(crate::ui::Key::Enter),
            KbOut::Hide => {
                self.focus = None;
                self.focus_reset = true;
            }
            KbOut::Paste(i) => {
                if let Some(t) = self.mx.cfg.clip.get(i).cloned() {
                    self.office_insert(&t);
                }
            }
            KbOut::CopyField => {
                let s = self.office_selected();
                self.copy_text(&s);
            }
            KbOut::Cut => {
                let s = self.office_selected();
                if !s.is_empty() {
                    self.copy_text(&s);
                    self.office_cut();
                }
            }
            KbOut::Helper => {
                self.buddy.open = true;
                self.focus = Some("buddy");
                self.focus_reset = true;
            }
            KbOut::CfgChanged => {
                let _ = crate::sys::save("keyboard.toml", &self.kb.cfg);
            }
        }
    }

    fn office_cur_screen(&self) -> Option<Ox> {
        match self.stack.last().map(|s| &s.0) {
            Some(Screen::Office(x)) => Some(x.clone()),
            _ => None,
        }
    }

    fn office_selected(&self) -> String {
        match self.office_cur_screen() {
            Some(Ox::Code(_)) => self
                .office
                .code
                .as_ref()
                .and_then(|c| c.tabs.get(c.cur))
                .map(|t| {
                    let s = t.buf.selected_text();
                    if s.is_empty() {
                        t.buf.line(t.buf.line_col(t.buf.cur).0).to_string()
                    } else {
                        s
                    }
                })
                .unwrap_or_default(),
            Some(Ox::Doc(_)) => self
                .office
                .doc
                .as_ref()
                .map(|d| d.buf.text.clone())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn office_cut(&mut self) {
        match self.office_cur_screen() {
            Some(Ox::Code(_)) => {
                if let Some(t) = self
                    .office
                    .code
                    .as_mut()
                    .and_then(|c| c.tabs.get_mut(c.cur))
                {
                    if t.buf.selection().is_some() {
                        t.buf.backspace();
                    } else {
                        t.buf.delete_line();
                    }
                    t.follow = true;
                }
            }
            Some(Ox::Doc(_)) => {
                if let Some(d) = self.office.doc.as_mut() {
                    d.buf.set_text("");
                }
            }
            _ => {}
        }
    }

    fn office_insert(&mut self, s: &str) {
        match self.office_cur_screen() {
            Some(Ox::Code(_)) => {
                if let Some(t) = self
                    .office
                    .code
                    .as_mut()
                    .and_then(|c| c.tabs.get_mut(c.cur))
                {
                    // automatische Klammerpaare
                    let pair = match s {
                        "(" => Some(")"),
                        "[" => Some("]"),
                        "{" => Some("}"),
                        "\"" if t.lang != Lang::Plain && t.lang != Lang::Markdown => Some("\""),
                        _ => None,
                    };
                    let next = t.buf.text[t.buf.cur..].chars().next();
                    if [")", "]", "}", "\""].contains(&s)
                        && next.map(|c| c.to_string()).as_deref() == Some(s)
                        && t.buf.selection().is_none()
                    {
                        t.buf.right(false);
                    } else {
                        t.buf.insert(s);
                        if let Some(p) = pair {
                            if t.buf.selection().is_none()
                                && next.map_or(true, |c| c.is_whitespace() || ")]};,".contains(c))
                            {
                                t.buf.insert(p);
                                t.buf.left(false);
                            }
                        }
                    }
                    t.follow = true;
                }
            }
            Some(Ox::Doc(_)) => {
                if let Some(d) = self.office.doc.as_mut() {
                    d.buf.insert(&s.replace('\n', " "));
                    d.dirty = true;
                }
            }
            _ => {}
        }
    }

    pub(super) fn office_key(&mut self, k: crate::ui::Key) {
        use crate::ui::Key;
        match self.office_cur_screen() {
            Some(Ox::Code(_)) => {
                if let Some(t) = self
                    .office
                    .code
                    .as_mut()
                    .and_then(|c| c.tabs.get_mut(c.cur))
                {
                    match k {
                        Key::Char(c) => {
                            let s = c.to_string();
                            let _ = t;
                            self.office_insert(&s);
                            return;
                        }
                        Key::Backspace => {
                            // leeres Klammerpaar zusammen löschen
                            let prev = t.buf.text[..t.buf.cur].chars().last();
                            let next = t.buf.text[t.buf.cur..].chars().next();
                            if t.buf.selection().is_none()
                                && matches!(
                                    (prev, next),
                                    (Some('('), Some(')'))
                                        | (Some('['), Some(']'))
                                        | (Some('{'), Some('}'))
                                        | (Some('"'), Some('"'))
                                )
                            {
                                t.buf.delete();
                            }
                            t.buf.backspace();
                        }
                        Key::Enter => {
                            let unit = if t.buf.text.contains("\n\t") {
                                "\t"
                            } else if t.lang == Lang::Yaml
                                || t.lang == Lang::Html
                                || t.lang == Lang::Xml
                            {
                                "  "
                            } else {
                                "    "
                            };
                            t.buf.newline(unit);
                        }
                    }
                    t.follow = true;
                }
            }
            Some(Ox::Doc(_)) => {
                let Some(d) = self.office.doc.as_mut() else {
                    return;
                };
                let Some(si) = d.sel else { return };
                match k {
                    Key::Char(c) => {
                        d.buf.insert(&c.to_string());
                        d.dirty = true;
                    }
                    Key::Backspace => {
                        if d.buf.cur == 0 && si > 0 && d.buf.selection().is_none() {
                            // mit dem vorigen Absatz verbinden
                            if let Some(Block::Para(prev)) = d.doc.blocks.get(si - 1).cloned() {
                                let pt = prev.text();
                                let joined = format!("{pt}{}", d.buf.text);
                                d.doc.blocks.remove(si);
                                if let Some(Block::Para(p)) = d.doc.blocks.get_mut(si - 1) {
                                    p.set_text(&joined);
                                }
                                d.sel = Some(si - 1);
                                d.buf = Buffer::new(&joined);
                                d.buf.cur = pt.len();
                                d.dirty = true;
                            }
                        } else {
                            d.buf.backspace();
                            d.dirty = true;
                        }
                    }
                    Key::Enter => {
                        // Absatz an der Schreibmarke teilen
                        let cur = d.buf.cur.min(d.buf.text.len());
                        let (a, b) = d.buf.text.split_at(cur);
                        let (a, b) = (a.to_string(), b.to_string());
                        let style = match d.doc.blocks.get(si) {
                            Some(Block::Para(p)) => p.style,
                            _ => Style::Normal,
                        };
                        // leerer Listenpunkt + Enter = Liste beenden
                        if a.is_empty()
                            && b.is_empty()
                            && matches!(style, Style::Bullet | Style::Number)
                        {
                            if let Some(Block::Para(p)) = d.doc.blocks.get_mut(si) {
                                p.style = Style::Normal;
                            }
                            d.dirty = true;
                            return;
                        }
                        if let Some(Block::Para(p)) = d.doc.blocks.get_mut(si) {
                            p.set_text(&a);
                        }
                        let st = if matches!(
                            style,
                            Style::Bullet | Style::Number | Style::Code | Style::Quote
                        ) {
                            style
                        } else {
                            Style::Normal
                        };
                        d.doc.blocks.insert(si + 1, Block::Para(Para::new(st, &b)));
                        d.sel = Some(si + 1);
                        d.buf = Buffer::new(&b);
                        d.buf.cur = 0;
                        d.dirty = true;
                    }
                }
            }
            _ => {}
        }
    }

    /// Enter in Suchfeld/Zeilenfeld des Code-Editors
    pub(super) fn office_find_enter(&mut self, id: &str) {
        let q = self.f("code_find");
        let g = self.f("code_goto");
        let mut none = false;
        let mut close_menu = false;
        if let Some(v) = self.office.code.as_mut() {
            let opt = v.opt.clone();
            if let Some(t) = v.tabs.get_mut(v.cur) {
                if id == "code_goto" {
                    if let Ok(n) = g.trim().parse::<usize>() {
                        t.buf.goto_line(n);
                        close_menu = true;
                    }
                } else if !t.buf.find_next(&q, &opt) {
                    none = true;
                }
                t.follow = true;
            }
        }
        if close_menu {
            self.office.menu = false;
        }
        if none {
            self.toast(self.t("No results").into());
        }
        if id == "code_goto" {
            self.focus = Some("office_ed");
            self.focus_reset = true;
        }
    }

    // ------------------------------------------------------------------ Tabellen (Excel)

    fn sheet_commit(&mut self) {
        let raw = self.f("office_cell");
        if let Some(s) = self.office.sheet.as_mut() {
            if s.editing {
                let (r, c) = s.sel;
                let old =
                    s.wb.sheets
                        .get(s.si)
                        .map(|sh| sh.get(r, c).to_string())
                        .unwrap_or_default();
                if old != raw {
                    if let Some(sh) = s.wb.sheets.get_mut(s.si) {
                        sh.set(r, c, raw.trim_end());
                    }
                    s.dirty = true;
                }
                s.editing = false;
            }
        }
    }

    /// Enter in der Eingabezeile: übernehmen, eine Zelle nach unten
    pub(super) fn office_cell_enter(&mut self) {
        self.sheet_commit();
        if let Some(s) = self.office.sheet.as_mut() {
            s.sel.0 += 1;
            let raw =
                s.wb.sheets
                    .get(s.si)
                    .map(|sh| sh.get(s.sel.0, s.sel.1).to_string())
                    .unwrap_or_default();
            self.fields.insert("office_cell", raw);
            if let Some(s) = self.office.sheet.as_mut() {
                s.editing = true;
            }
        }
    }

    fn s_sheet(&mut self, ui: &mut Ui, area: Rect, path: &str, sc: &mut f32) {
        let Some(mut s) = self.office.sheet.take() else {
            self.office_on_go(&Screen::Office(Ox::Sheet(path.into())));
            ui.dirty = true;
            return;
        };
        if s.si >= s.wb.sheets.len() {
            s.si = 0;
        }
        if std::mem::take(&mut self.office.debug_edit) {
            s.sel = (4, 1);
            s.editing = true;
            self.fields.insert("office_cell", "=SUM(B2:B3)*".into());
            self.focus = Some("office_cell");
            self.focus_reset = true;
        }
        let title = format!("{}{}", if s.dirty { "• " } else { "" }, file_name(path));
        let area = self.header(ui, area, &title);
        // Eingabe geschlossen (Tastatur weg) → übernehmen
        if s.editing && self.focus != Some("office_cell") {
            self.office.sheet = Some(s);
            self.sheet_commit();
            s = self.office.sheet.take().unwrap();
        }
        // Eingabezeile: Zellname + Inhalt
        let m = ui.dp(10.0);
        let fh = ui.dp(44.0);
        let fy = area.y + ui.dp(4.0);
        let nw = ui.dp(64.0);
        let name = cell_name(s.sel.0, s.sel.1);
        let nr = Rect::new(if ui.rtl { ui.w - m - nw } else { m }, fy, nw, fh);
        ui.cv.rrect(nr, ui.dp(12.0), ui.th.accent.alpha(0.18));
        let ac = ui.th.accent;
        ui.label(nr, &name, 15.0, ac, true, Al::Center);
        let fr = Rect::new(
            if ui.rtl { m } else { m + nw + ui.dp(8.0) },
            fy,
            ui.w - 2.0 * m - nw - ui.dp(8.0),
            fh,
        );
        let raw_sel = s.wb.sheets[s.si].get(s.sel.0, s.sel.1).to_string();
        if !s.editing {
            self.fields.insert("office_cell", raw_sel.clone());
        }
        let val = self.f("office_cell");
        let before = self.focus;
        ui.field(fr, "office_cell", &val, "fx");
        if ui.focus == Some("office_cell") && before != Some("office_cell") {
            s.editing = true;
            self.focus = Some("office_cell");
            self.focus_reset = true;
        }
        let mut top = fy + fh + ui.dp(6.0);
        // Schnelltasten beim Bearbeiten: Formelzeichen
        if s.editing && self.focus == Some("office_cell") {
            let keys = [
                "=", "SUM(", "AVERAGE(", "IF(", "(", ")", ":", ";", "+", "-", "*", "/", "&", "\"",
                "ROUND(", "COUNT(", "MAX(", "MIN(",
            ];
            let mut x = m;
            for k in keys {
                let w = ui.text_w(k, 13.0, false) + ui.dp(22.0);
                if x + w > ui.w - m {
                    break;
                }
                let px = if ui.rtl { ui.w - x - w } else { x };
                let (_, tap) = ui.pill(px, top, k, false);
                if tap {
                    let k2 = if k == ";" { "," } else { k };
                    self.fields.entry("office_cell").or_default().push_str(k2);
                    ui.dirty = true;
                }
                x += w + ui.dp(6.0);
            }
            top += ui.dp(40.0);
        } else {
            // Werkzeuge: Zeile/Spalte einfügen/löschen, Speichern, Agent
            let tb = Rect::new(area.x, top - ui.dp(2.0), area.w, ui.dp(44.0));
            let items = [
                (Ic::Svg("row+"), false),
                (Ic::Svg("col+"), false),
                (Ic::Svg("row-"), false),
                (Ic::Svg("col-"), false),
                (Ic::Download, false),
                (Ic::Spark, false),
            ];
            match self.office_toolbar(ui, tb, &items) {
                Some(0) => {
                    s.wb.sheets[s.si].insert_row(s.sel.0);
                    s.dirty = true;
                }
                Some(1) => {
                    s.wb.sheets[s.si].insert_col(s.sel.1);
                    s.dirty = true;
                }
                Some(2) => {
                    s.wb.sheets[s.si].delete_row(s.sel.0);
                    s.dirty = true;
                }
                Some(3) => {
                    s.wb.sheets[s.si].delete_col(s.sel.1);
                    s.dirty = true;
                }
                Some(4) => match uo::write_book(&s.path, &s.wb, s.si) {
                    Ok(_) => {
                        s.dirty = false;
                        self.toast(self.t("Saved").into());
                    }
                    Err(e) => self.toast(format!("{}: {e}", self.t("Could not save"))),
                },
                Some(5) => {
                    if s.dirty {
                        let _ = uo::write_book(&s.path, &s.wb, s.si).map(|_| s.dirty = false);
                    }
                    let what = self.t("Analyze").to_string();
                    self.office.sheet = Some(s);
                    self.office_ask_agent(path, &what);
                    ui.dirty = true;
                    return;
                }
                _ => {}
            }
            top = tb.bottom() + ui.dp(2.0);
        }
        if let Some(e) = s.err.take() {
            self.toast(e);
        }
        // Raster
        let tabs_h = ui.dp(40.0);
        let grid = Rect::new(
            0.0,
            top,
            ui.w,
            (area.bottom() - top - tabs_h).max(ui.dp(60.0)),
        );
        let rh = ui.dp(34.0);
        let hw = ui.dp(40.0); // Zeilennummern
        let hh = ui.dp(28.0); // Spaltenköpfe
        let (er, ec) = s.wb.sheets[s.si].extent();
        let rows = (er + 30).max(60);
        let cols = (ec + 6).max(12);
        let ws: Vec<f32> = (0..cols)
            .map(|c| {
                ui.dp(
                    (s.wb.sheets[s.si].widths.get(&c).copied().unwrap_or(11.0) * 8.0)
                        .clamp(56.0, 260.0),
                )
            })
            .collect();
        let colw = |c: u32| -> f32 { ws[c as usize] };
        let mut cx = Vec::with_capacity(cols as usize + 1);
        let mut acc = 0.0;
        for c in 0..cols {
            cx.push(acc);
            acc += colw(c);
        }
        let total_w = acc;
        let maxsx = (total_w + hw - grid.w).max(0.0);
        if ui.p.down && ui.p.moved && grid.contains(ui.p.px, ui.p.py) && ui.p.capture.is_none() {
            let d = if ui.rtl { -ui.p.dx } else { ui.p.dx };
            s.sx = (s.sx - d).clamp(0.0, maxsx);
            ui.p.dx = 0.0;
        }
        s.sx = s.sx.clamp(0.0, maxsx);
        let cells_area = Rect::new(grid.x, grid.y + hh, grid.w, grid.h - hh);
        ui.scroll_begin(cells_area, sc);
        let dark = ui.th.bg.r < 128;
        let (line, text, sub) = (ui.th.line, ui.th.text, ui.th.sub);
        let bg_cells = ui.th.bg;
        ui.cv.rect(cells_area, bg_cells);
        let r0 = ((*sc / rh).floor().max(0.0)) as u32;
        let r1 = (r0 + (cells_area.h / rh) as u32 + 2).min(rows);
        // Spaltenbereich
        let (uw, rtl, ssx) = (ui.w, ui.rtl, s.sx);
        let xpos = |c: u32| -> f32 {
            let x = hw + cx[c as usize] - ssx;
            if rtl {
                uw - x - colw(c)
            } else {
                x
            }
        };
        let mut tap_cell = None;
        for r in r0..r1 {
            let y = cells_area.y + r as f32 * rh - *sc;
            ui.cv.rect(Rect::new(0.0, y + rh - 1.0, ui.w, 1.0), line);
            for c in 0..cols {
                let x = xpos(c);
                let w = colw(c);
                if x + w < 0.0 || x > ui.w {
                    continue;
                }
                let rr = Rect::new(x, y, w, rh);
                let raw = s.wb.sheets[s.si].get(r, c);
                if !raw.is_empty() {
                    let v = s.wb.value(s.si, r, c);
                    let (txt, col, al) = match &v {
                        Value::Num(_) => (v.text(), text, Al::End),
                        Value::Err(e) => (e.clone(), ui.th.danger, Al::Center),
                        Value::Bool(_) => (v.text(), text, Al::Center),
                        _ => (v.text(), text, Al::Start),
                    };
                    let f = ui.fit(&txt, 13.0, false, w - ui.dp(10.0));
                    let old = ui.rtl;
                    // Zahlen immer links-nach-rechts, Text nach eigener Schrift
                    ui.rtl = matches!(v, Value::Text(_)) && uo::doc::is_rtl(&txt);
                    let al2 = if ui.rtl { Al::Start } else { al };
                    ui.label(
                        Rect::new(x + ui.dp(5.0), y, w - ui.dp(10.0), rh),
                        &f,
                        13.0,
                        col,
                        false,
                        al2,
                    );
                    ui.rtl = old;
                    if raw.starts_with('=') {
                        // kleines Dreieck = Formel
                        ui.cv.rect(
                            Rect::new(
                                if ui.rtl { x + 1.0 } else { x + w - ui.dp(5.0) },
                                y + 1.0,
                                ui.dp(4.0),
                                ui.dp(4.0),
                            ),
                            ui.th.accent.alpha(0.6),
                        );
                    }
                }
                ui.cv.rect(
                    Rect::new(if ui.rtl { x } else { x + w - 1.0 }, y, 1.0, rh),
                    line,
                );
                if ui.tap(rr) {
                    tap_cell = Some((r, c));
                }
            }
        }
        // Auswahlrahmen
        {
            let (r, c) = s.sel;
            let y = cells_area.y + r as f32 * rh - *sc;
            if (c as usize) < cx.len() {
                let x = xpos(c);
                let rr = Rect::new(x, y, colw(c), rh);
                let a = ui.th.accent;
                for (rx, ry, rw, rh2) in [
                    (rr.x, rr.y, rr.w, 2.0),
                    (rr.x, rr.bottom() - 2.0, rr.w, 2.0),
                    (rr.x, rr.y, 2.0, rr.h),
                    (rr.right() - 2.0, rr.y, 2.0, rr.h),
                ] {
                    ui.cv.rect(Rect::new(rx, ry, rw, rh2), a);
                }
            }
        }
        // Zeilennummern (fest links)
        let head_bg = if dark { rgb(0x1A1D24) } else { rgb(0xF1F3F5) };
        let nx = if ui.rtl { ui.w - hw } else { 0.0 };
        ui.cv
            .rect(Rect::new(nx, cells_area.y, hw, cells_area.h), head_bg);
        for r in r0..r1 {
            let y = cells_area.y + r as f32 * rh - *sc;
            let selr = r == s.sel.0;
            let lab = (r + 1).to_string();
            ui.label(
                Rect::new(nx, y, hw, rh),
                &lab,
                11.0,
                if selr { ui.th.accent } else { sub },
                selr,
                Al::Center,
            );
            ui.cv.rect(Rect::new(nx, y + rh - 1.0, hw, 1.0), line);
        }
        ui.scroll_end(cells_area, rows as f32 * rh + ui.dp(40.0), sc);
        // Spaltenköpfe (fest oben)
        ui.cv.rect(Rect::new(0.0, grid.y, ui.w, hh), head_bg);
        for c in 0..cols {
            let x = xpos(c);
            let w = colw(c);
            if x + w < 0.0 || x > ui.w {
                continue;
            }
            let selc = c == s.sel.1;
            let lab = col_name(c);
            ui.label(
                Rect::new(x, grid.y, w, hh),
                &lab,
                11.0,
                if selc { ui.th.accent } else { sub },
                selc,
                Al::Center,
            );
            ui.cv.rect(
                Rect::new(if ui.rtl { x } else { x + w - 1.0 }, grid.y, 1.0, hh),
                line,
            );
        }
        ui.cv.rect(Rect::new(nx, grid.y, hw, hh), head_bg);
        ui.cv
            .rect(Rect::new(0.0, grid.y + hh - 1.0, ui.w, 1.0), line);
        if let Some((r, c)) = tap_cell {
            let same = s.sel == (r, c);
            // zweites Antippen der gewählten Zelle = bearbeiten (wie in Tabellen-Apps auf dem Handy)
            let dbl = same;
            self.office.last_tap = Some(Instant::now());
            if s.editing && self.focus == Some("office_cell") && !same {
                // während einer Formel: Zellbezug einfügen statt wechseln
                let cur = self.f("office_cell");
                if cur.starts_with('=') && cur.ends_with(|ch: char| "=(+-*/,:&<>".contains(ch)) {
                    self.fields
                        .entry("office_cell")
                        .or_default()
                        .push_str(&cell_name(r, c));
                    ui.dirty = true;
                    self.office.sheet = Some(s);
                    return;
                }
            }
            self.office.sheet = Some(s);
            self.sheet_commit();
            s = self.office.sheet.take().unwrap();
            s.sel = (r, c);
            let raw = s.wb.sheets[s.si].get(r, c).to_string();
            self.fields.insert("office_cell", raw);
            if dbl {
                s.editing = true;
                self.focus = Some("office_cell");
                self.focus_reset = true;
            } else if self.focus == Some("office_cell") {
                self.focus = None;
                self.focus_reset = true;
            }
            ui.dirty = true;
        }
        // Blätter unten
        let ty = grid.bottom();
        ui.cv.rect(Rect::new(0.0, ty, ui.w, tabs_h), head_bg);
        let mut x = ui.dp(8.0);
        let names: Vec<String> = s.wb.sheets.iter().map(|sh| sh.name.clone()).collect();
        for (i, n) in names.iter().enumerate() {
            let w = ui.text_w(n, 13.0, i == s.si) + ui.dp(26.0);
            let px = if ui.rtl { ui.w - x - w } else { x };
            let (_, tap) = ui.pill(px, ty + ui.dp(4.0), n, i == s.si);
            if tap && i != s.si {
                self.office.sheet = Some(s);
                self.sheet_commit();
                s = self.office.sheet.take().unwrap();
                s.si = i;
                s.sel = (0, 0);
                s.sx = 0.0;
                *sc = 0.0;
                ui.dirty = true;
            }
            x += w + ui.dp(6.0);
        }
        let px = if ui.rtl { ui.w - x - ui.dp(34.0) } else { x };
        let (_, add) = ui.pill(px, ty + ui.dp(4.0), "+", false);
        if add {
            let n = s.wb.sheets.len() + 1;
            s.wb.sheets
                .push(uo::sheet::Sheet::new(&format!("Sheet{n}")));
            s.si = s.wb.sheets.len() - 1;
            s.sel = (0, 0);
            s.dirty = true;
            *sc = 0.0;
            ui.dirty = true;
        }
        self.office.sheet = Some(s);
    }

    // ------------------------------------------------------------------ Code-Editor (Notepad++)

    fn s_code(&mut self, ui: &mut Ui, area: Rect, path: &str, _sc: &mut f32) {
        let Some(mut v) = self.office.code.take() else {
            self.office_on_go(&Screen::Office(Ox::Code(path.into())));
            ui.dirty = true;
            return;
        };
        if let Some(i) = v.tabs.iter().position(|t| t.path == path) {
            v.cur = i;
        }
        if v.tabs.is_empty() {
            self.office.code = Some(v);
            self.back();
            return;
        }
        if std::mem::take(&mut self.office.debug_edit) {
            v.find = true;
            self.office.menu = true;
            self.fields.insert("code_find", "total".into());
            self.fields.insert("code_repl", "summe_gesamt".into());
            let t = &mut v.tabs[v.cur];
            t.buf.find_next("total", &FindOpt::default());
        }
        let dirty = v.tabs[v.cur].buf.dirty;
        let title = format!("{}{}", if dirty { "• " } else { "" }, file_name(path));
        let area = self.header(ui, area, &title);
        let fs = self.office_cfg().code_size;
        // Werkzeugleiste
        let tb = Rect::new(area.x, area.y, area.w, ui.dp(44.0));
        let (cu, cr) = (v.tabs[v.cur].buf.can_undo(), v.tabs[v.cur].buf.can_redo());
        let items = [
            (Ic::Svg("undo"), false),
            (Ic::Svg("redo"), false),
            (Ic::Search, v.find),
            (Ic::Svg("sel"), v.select),
            (Ic::Download, false),
            (Ic::Svg("more"), self.office.menu),
        ];
        let _ = (cu, cr);
        match self.office_toolbar(ui, tb, &items) {
            Some(0) => {
                v.tabs[v.cur].buf.undo();
                v.tabs[v.cur].follow = true;
            }
            Some(1) => {
                v.tabs[v.cur].buf.redo();
                v.tabs[v.cur].follow = true;
            }
            Some(2) => {
                v.find = !v.find;
                if v.find {
                    let sel = v.tabs[v.cur].buf.selected_text();
                    if !sel.is_empty() && !sel.contains('\n') {
                        self.fields.insert("code_find", sel);
                    }
                    self.focus = Some("code_find");
                    self.focus_reset = true;
                } else {
                    self.focus = None;
                    self.focus_reset = true;
                }
            }
            Some(3) => {
                v.select = !v.select;
                if !v.select {
                    v.tabs[v.cur].buf.anchor = None;
                }
            }
            Some(4) => {
                let t = &mut v.tabs[v.cur];
                match uo::write_atomic(&t.path, &t.buf.to_bytes()) {
                    Ok(_) => {
                        t.buf.dirty = false;
                        self.toast(self.t("Saved").into());
                    }
                    Err(e) => self.toast(format!("{}: {e}", self.t("Could not save"))),
                }
            }
            Some(5) => {
                // Menü braucht Platz: Tastatur dafür schließen
                self.office.menu = !self.office.menu;
                if self.office.menu {
                    self.focus = None;
                    self.focus_reset = true;
                }
            }
            _ => {}
        }
        let mut top = tb.bottom();
        // Reiter (mehrere offene Dateien)
        if v.tabs.len() > 1 {
            let th = ui.dp(36.0);
            let mut x = ui.dp(8.0);
            let mut close = None;
            let mut sw = None;
            for (i, t) in v.tabs.iter().enumerate() {
                let n = format!(
                    "{}{}",
                    if t.buf.dirty { "• " } else { "" },
                    file_name(&t.path)
                );
                let w = ui.text_w(&n, 12.0, i == v.cur) + ui.dp(46.0);
                let px = if ui.rtl { ui.w - x - w } else { x };
                let r = Rect::new(px, top + ui.dp(3.0), w, th - ui.dp(6.0));
                ui.cv.rrect(
                    r,
                    ui.dp(8.0),
                    if i == v.cur {
                        ui.th.accent.alpha(0.2)
                    } else {
                        ui.th.card
                    },
                );
                let c = if i == v.cur { ui.th.accent } else { ui.th.text };
                ui.label(
                    Rect::new(r.x + ui.dp(10.0), r.y, w - ui.dp(36.0), r.h),
                    &n,
                    12.0,
                    c,
                    i == v.cur,
                    Al::Start,
                );
                let xr = Rect::new(r.right() - ui.dp(28.0), r.y, ui.dp(28.0), r.h);
                ui.icon(Ic::Close, xr.cx(), xr.cy(), ui.dp(10.0), ui.th.sub);
                if ui.tap(xr) {
                    close = Some(i);
                } else if ui.tap(r) {
                    sw = Some(i);
                }
                x += w + ui.dp(6.0);
            }
            top += th;
            if let Some(i) = close {
                let t = v.tabs.remove(i);
                if t.buf.dirty {
                    let _ = uo::write_atomic(&t.path, &t.buf.to_bytes());
                }
                v.cur = v.cur.min(v.tabs.len().saturating_sub(1));
                if let Some(t) = v.tabs.get(v.cur) {
                    let p = t.path.clone();
                    if let Some(l) = self.stack.last_mut() {
                        l.0 = Screen::Office(Ox::Code(p));
                    }
                }
                ui.dirty = true;
            } else if let Some(i) = sw {
                let p = v.tabs[i].path.clone();
                v.cur = i;
                if let Some(l) = self.stack.last_mut() {
                    l.0 = Screen::Office(Ox::Code(p));
                }
                ui.dirty = true;
            }
            if v.tabs.is_empty() {
                self.office.code = Some(v);
                self.back();
                return;
            }
        }
        // Suchen/Ersetzen
        if v.find {
            let m = ui.dp(10.0);
            let fh = ui.dp(40.0);
            let bw = ui.dp(40.0);
            let f1 = Rect::new(
                m,
                top + ui.dp(4.0),
                ui.w - 2.0 * m - 2.0 * bw - ui.dp(8.0),
                fh,
            );
            let q = self.f("code_find");
            ui.field(
                if ui.rtl {
                    Rect::new(m + 2.0 * bw + ui.dp(8.0), f1.y, f1.w, f1.h)
                } else {
                    f1
                },
                "code_find",
                &q,
                self.t("Find"),
            );
            let bxs = |k: f32| {
                if ui.rtl {
                    m + bw * (k + 0.5)
                } else {
                    ui.w - m - bw * (k + 0.5)
                }
            };
            let (tc, nb) = (ui.th.text, ui.th.card);
            let (bprev, bnext) = (bxs(1.0), bxs(0.0));
            if ui.icon_btn(bprev, f1.cy(), ui.dp(15.0), Ic::Svg("up"), nb, tc) {
                let o = v.opt.clone();
                if !v.tabs[v.cur].buf.find_prev(&q, &o) {
                    self.toast(self.t("No results").into());
                }
                v.tabs[v.cur].follow = true;
            }
            if ui.icon_btn(bnext, f1.cy(), ui.dp(15.0), Ic::Svg("down"), nb, tc) {
                let o = v.opt.clone();
                if !v.tabs[v.cur].buf.find_next(&q, &o) {
                    self.toast(self.t("No results").into());
                }
                v.tabs[v.cur].follow = true;
            }
            let y2 = f1.bottom() + ui.dp(6.0);
            let f2 = Rect::new(m, y2, ui.w - 2.0 * m - 2.0 * bw - ui.dp(8.0), fh);
            let rq = self.f("code_repl");
            ui.field(
                if ui.rtl {
                    Rect::new(m + 2.0 * bw + ui.dp(8.0), f2.y, f2.w, f2.h)
                } else {
                    f2
                },
                "code_repl",
                &rq,
                self.t("Replace with"),
            );
            if ui.icon_btn(bnext, f2.cy(), ui.dp(15.0), Ic::Check, nb, tc) {
                let o = v.opt.clone();
                v.tabs[v.cur].buf.replace_one(&q, &rq, &o);
                v.tabs[v.cur].follow = true;
            }
            if ui.icon_btn(bprev, f2.cy(), ui.dp(15.0), Ic::Svg("all"), nb, tc) {
                let o = v.opt.clone();
                match v.tabs[v.cur].buf.replace_all(&q, &rq, &o) {
                    Ok(n) => self.toast(format!("{n} {}", self.t("replaced"))),
                    Err(e) => self.toast(e),
                }
            }
            // Optionen
            let y3 = f2.bottom() + ui.dp(6.0);
            let mut x = m;
            let opts = [("Aa", v.opt.case), ("W", v.opt.word), (".*", v.opt.regex)];
            for (i, (l, on)) in opts.iter().enumerate() {
                let w = ui.text_w(l, 13.0, *on) + ui.dp(26.0);
                let px = if ui.rtl { ui.w - x - w } else { x };
                let (_, tap) = ui.pill(px, y3, l, *on);
                if tap {
                    match i {
                        0 => v.opt.case = !v.opt.case,
                        1 => v.opt.word = !v.opt.word,
                        _ => v.opt.regex = !v.opt.regex,
                    }
                }
                x += w + ui.dp(6.0);
            }
            v.hits = uo::code::find_all(&v.tabs[v.cur].buf.text, &q, &v.opt).len();
            let hl = format!("{} {}", v.hits, self.t("hits"));
            let c = ui.th.sub;
            let lr = if ui.rtl {
                Rect::new(m, y3, ui.w - x - m, ui.dp(32.0))
            } else {
                Rect::new(x, y3, ui.w - x - m, ui.dp(32.0))
            };
            ui.label(lr, &hl, 12.0, c, false, Al::Start);
            top = y3 + ui.dp(38.0);
        }
        // Menü
        if self.office.menu {
            top = self.code_menu(ui, &mut v, top);
        }
        // Code ist immer links-nach-rechts (wie in jedem Code-Editor), auch bei arabischer Oberfläche
        // Tastenleiste und Statuszeile unten
        let bar_h = ui.dp(40.0);
        let st_h = ui.dp(22.0);
        let ed = Rect::new(
            0.0,
            top,
            ui.w,
            (area.bottom() - top - bar_h - st_h).max(ui.dp(60.0)),
        );
        let dark = ui.th.bg.r < 128;
        let ed_bg = if dark { rgb(0x0F1115) } else { rgb(0xFBFBFC) };
        ui.cv.rect(ed, ed_bg);
        let sz = ui.dp(fs);
        let (cw, _) = ui.tx.measure_mono("M", sz);
        let cw = cw.max(1.0);
        let lh = (sz * 1.5).round();
        let t = &mut v.tabs[v.cur];
        let nlines = t.buf.line_count();
        let gut = cw * (nlines.to_string().len().max(2) as f32 + 1.5);
        let gx = if false { ed.right() - gut } else { ed.x };
        let tx0 = if false {
            ed.x + ui.dp(6.0)
        } else {
            ed.x + gut + ui.dp(6.0)
        };
        let text_w = ed.w - gut - ui.dp(12.0);
        // Schreibmarke sichtbar halten
        let (cl, _) = t.buf.line_col(t.buf.cur);
        let cline = t.buf.line(cl).to_string();
        let cvc = vcol(&cline, t.buf.cur - t.buf.line_start(t.buf.cur));
        if t.follow {
            let cy = cl as f32 * lh;
            if cy < t.sy {
                t.sy = cy;
            } else if cy + lh > t.sy + ed.h {
                t.sy = cy + lh - ed.h;
            }
            let cxp = cvc as f32 * cw;
            if cxp < t.hx {
                t.hx = (cxp - cw * 4.0).max(0.0);
            } else if cxp > t.hx + text_w - cw * 2.0 {
                t.hx = cxp - text_w + cw * 6.0;
            }
            t.follow = false;
        }
        // Ziehen: senkrecht und waagerecht rollen
        if ui.p.down && ui.p.moved && ed.contains(ui.p.px, ui.p.py) && ui.p.capture.is_none() {
            t.sy -= ui.p.dy;
            t.hx -= ui.p.dx;
            ui.p.dy = 0.0;
            ui.p.dx = 0.0;
        }
        let maxsy = (nlines as f32 * lh - ed.h + lh * 3.0).max(0.0);
        t.sy = t.sy.clamp(0.0, maxsy);
        t.hx = t.hx.max(0.0);
        let old_clip = ui.clip;
        ui.set_clip(Some(ed));
        ui.cv.rect(
            Rect::new(gx, ed.y, gut, ed.h),
            if dark { rgb(0x14171C) } else { rgb(0xF0F1F3) },
        );
        let first = (t.sy / lh).floor() as usize;
        let last = ((t.sy + ed.h) / lh).ceil() as usize + 1;
        // Zustand mehrzeiliger Kommentare bis zur ersten sichtbaren Zeile vorrechnen
        let mut st = LineState::Normal;
        let sel = t.buf.selection();
        let mut ls = 0usize;
        let text_ref = t.buf.text.clone();
        let tcol = ui.th.text;
        for (i, line) in text_ref.split('\n').enumerate() {
            if i > last.min(nlines) {
                break;
            }
            let (toks, nst) = if t.lang == Lang::Plain {
                (vec![(0, line.len(), Tk::Text)], st)
            } else {
                uo::code::highlight(line, t.lang, st)
            };
            st = nst;
            let le = ls + line.len();
            if i >= first {
                let y = ed.y + i as f32 * lh - t.sy;
                if i == cl {
                    ui.cv.rect(
                        Rect::new(ed.x, y, ed.w, lh),
                        ui.th.accent.alpha(if dark { 0.07 } else { 0.08 }),
                    );
                }
                // Auswahl
                if let Some((a, b)) = sel {
                    if a <= le && b >= ls && !(a == le && b == le && a != b && a > ls) {
                        let ca = vcol(line, a.saturating_sub(ls).min(line.len()));
                        let cb = if b > le {
                            vcol(line, line.len()) + 1
                        } else {
                            vcol(line, b - ls)
                        };
                        if cb > ca {
                            let x0 = tx0 + ca as f32 * cw - t.hx;
                            ui.cv.rect(
                                Rect::new(x0, y, (cb - ca) as f32 * cw, lh),
                                rgb(0x3B82F6).alpha(0.35),
                            );
                        }
                    }
                }
                // Zeilennummer
                let ln = (i + 1).to_string();
                let lnw = ln.len() as f32 * cw;
                let lnx = if false {
                    gx + cw * 0.5
                } else {
                    gx + gut - lnw - cw * 0.6
                };
                ui.tx.draw_mono(
                    ui.cv,
                    &ln,
                    lnx,
                    y + (lh - sz * 1.3) / 2.0,
                    sz * 0.92,
                    if i == cl {
                        ui.th.accent
                    } else {
                        ui.th.sub.alpha(0.8)
                    },
                );
                // Text in Farbe
                let old2 = ui.clip;
                let tclip = Rect::new(tx0 - ui.dp(2.0), ed.y, text_w + ui.dp(4.0), ed.h);
                ui.set_clip(Some(tclip));
                for (a, b, k) in toks {
                    let s = &line[a..b];
                    if s.trim().is_empty() {
                        continue;
                    }
                    let x = tx0 + vcol(line, a) as f32 * cw - t.hx;
                    if x > tclip.right() {
                        break;
                    }
                    if x + (b - a) as f32 * cw < tx0 - cw * 40.0 {
                        continue;
                    }
                    let s2 = if s.contains('\t') {
                        s.replace('\t', "    ")
                    } else {
                        s.to_string()
                    };
                    ui.tx.draw_mono(
                        ui.cv,
                        &s2,
                        x,
                        y + (lh - sz * 1.3) / 2.0,
                        sz,
                        tk_color(k, dark, tcol),
                    );
                }
                // Schreibmarke
                if i == cl && self.focus == Some("office_ed") {
                    let x = tx0 + cvc as f32 * cw - t.hx;
                    ui.cv.rect(
                        Rect::new(x - 1.0, y + ui.dp(2.0), ui.dp(2.0), lh - ui.dp(4.0)),
                        ui.th.accent,
                    );
                }
                ui.set_clip(old2);
            }
            ls = le + 1;
        }
        // Antippen: Schreibmarke setzen; langer Druck = Wort auswählen
        if ui.tap(ed) {
            let l = (((ui.p.py - ed.y + t.sy) / lh).floor().max(0.0) as usize).min(nlines - 1);
            let lt = t.buf.line(l).to_string();
            let col = (((ui.p.px - tx0 + t.hx) / cw) + 0.5).floor().max(0.0) as usize;
            let b = byte_at_vcol(&lt, col);
            let p = t.buf.pos(l, 0) + b;
            if v.select {
                if t.buf.anchor.is_none() {
                    t.buf.anchor = Some(t.buf.cur);
                }
            } else {
                t.buf.anchor = None;
            }
            t.buf.cur = p.min(t.buf.text.len());
            let dbl = self
                .office
                .last_tap
                .is_some_and(|x| x.elapsed().as_millis() < 330);
            self.office.last_tap = if dbl { None } else { Some(Instant::now()) };
            if dbl {
                t.buf.select_word();
            }
            self.focus = Some("office_ed");
            self.focus_reset = true;
            self.office.menu = false;
            ui.dirty = true;
        }
        ui.set_clip(old_clip);
        // Tastenleiste (wie die Zusatzleiste von Code-Apps): Tab, Pfeile, Pos1/Ende, Klammern, Kopieren, Einfügen
        let by = ed.bottom();
        let bar = Rect::new(0.0, by, ui.w, bar_h);
        ui.cv.rect(bar, ui.th.card);
        ui.cv.rect(Rect::new(0.0, by, ui.w, 1.0), ui.th.line);
        let labels: [&str; 14] = [
            "Tab",
            "←",
            "↑",
            "↓",
            "→",
            "Line start",
            "Line end",
            "Paste",
            "{",
            "}",
            "(",
            ")",
            ";",
            "\"",
        ];
        let mut x = ui.dp(6.0);
        let mut hit = None;
        for (i, l) in labels.iter().enumerate() {
            let lab = self.t(l);
            let w = (ui.text_w(lab, 13.0, false) + ui.dp(20.0)).max(ui.dp(36.0));
            if x + w > ui.w {
                break;
            }
            let px = x;
            let r = Rect::new(px, by + ui.dp(4.0), w, bar_h - ui.dp(8.0));
            ui.cv.rrect(
                r,
                ui.dp(8.0),
                if ui.held(r) { ui.th.press } else { ui.th.bg },
            );
            let c = ui.th.text;
            ui.label(r, lab, 13.0, c, false, Al::Center);
            if ui.tap(r) {
                hit = Some(i);
            }
            x += w + ui.dp(4.0);
        }
        let selm = v.select;
        let t = &mut v.tabs[v.cur];
        if let Some(i) = hit {
            match i {
                0 => {
                    if t.buf.selection().is_some() {
                        t.buf.indent("    ", false);
                    } else {
                        let unit = if t.buf.text.contains("\n\t") || t.path.ends_with("Makefile") {
                            "\t"
                        } else {
                            "    "
                        };
                        t.buf.insert(unit);
                    }
                }
                1 => t.buf.left(selm),
                2 => t.buf.up(selm),
                3 => t.buf.down(selm),
                4 => t.buf.right(selm),
                5 => t.buf.home(selm),
                6 => t.buf.end(selm),
                7 => {
                    if let Some(c) = self.mx.cfg.clip.first().cloned() {
                        t.buf.insert(&c);
                    }
                }
                8 => t.buf.insert("{"),
                9 => t.buf.insert("}"),
                10 => t.buf.insert("("),
                11 => t.buf.insert(")"),
                12 => t.buf.insert(";"),
                _ => t.buf.insert("\""),
            }
            t.follow = true;
            self.focus = Some("office_ed");
            self.focus_reset = true;
            ui.dirty = true;
        }
        // Statuszeile
        let (l, c) = t.buf.line_col(t.buf.cur);
        let sel_n = t.buf.selected_text().chars().count();
        let status = format!(
            "{} {}, {} {}{}  ·  {}  ·  {}  ·  {}",
            self.t("Ln"),
            l + 1,
            self.t("Col"),
            c + 1,
            if sel_n > 0 {
                format!(" ({sel_n})")
            } else {
                String::new()
            },
            t.lang.name(),
            t.buf.enc.label(),
            t.buf.eol.label()
        );
        let sr = Rect::new(ui.dp(10.0), bar.bottom(), ui.w - ui.dp(20.0), st_h);
        let cs = ui.th.sub;
        let old = ui.rtl;
        ui.rtl = false;
        ui.label(sr, &status, 11.0, cs, false, Al::Start);
        ui.rtl = old;
        self.office.code = Some(v);
    }

    /// Menü des Code-Editors; liefert die neue Oberkante für den Editor
    fn code_menu(&mut self, ui: &mut Ui, v: &mut CodeView, top: f32) -> f32 {
        let m = ui.dp(10.0);
        let mut y = top + ui.dp(6.0);
        let items: [&str; 12] = [
            "Go to line",
            "Duplicate line",
            "Delete line",
            "Move up",
            "Move down",
            "Comment",
            "Indent",
            "Outdent",
            "Language",
            "Encoding",
            "Line ending",
            "Ask assistant",
        ];
        let mut x = m;
        let mut hit = None;
        for (i, l) in items.iter().enumerate() {
            let lab = self.t(l);
            let w = ui.text_w(lab, 13.0, false) + ui.dp(26.0);
            if x + w > ui.w - m {
                x = m;
                y += ui.dp(38.0);
            }
            let px = if ui.rtl { ui.w - x - w } else { x };
            let (_, tap) = ui.pill(px, y, lab, false);
            if tap {
                hit = Some(i);
            }
            x += w + ui.dp(6.0);
        }
        y += ui.dp(38.0);
        // Schriftgröße und Zeile
        let fs = self.office_cfg().code_size;
        let lab = format!("A−   {:.0}   A+", fs);
        let r = Rect::new(m, y, ui.dp(150.0), ui.dp(32.0));
        let r = if ui.rtl {
            Rect::new(ui.w - m - r.w, r.y, r.w, r.h)
        } else {
            r
        };
        ui.cv.rrect(r, r.h / 2.0, ui.th.card);
        let c = ui.th.text;
        ui.label(r, &lab, 13.0, c, false, Al::Center);
        let (minus, plus) = (
            Rect::new(r.x, r.y, r.w / 3.0, r.h),
            Rect::new(r.right() - r.w / 3.0, r.y, r.w / 3.0, r.h),
        );
        let (minus, plus) = if ui.rtl { (plus, minus) } else { (minus, plus) };
        if ui.tap(minus) || ui.tap(plus) {
            let d = if ui.tap(plus) { 1.0 } else { -1.0 };
            let c = self.office_cfg();
            c.code_size = (c.code_size + d).clamp(9.0, 24.0);
            let c = c.clone();
            let _ = crate::sys::save("office.toml", &c);
            ui.dirty = true;
        }
        let gr = Rect::new(
            if ui.rtl { m } else { m + ui.dp(160.0) },
            y,
            ui.w - 2.0 * m - ui.dp(160.0),
            ui.dp(36.0),
        );
        let gq = self.f("code_goto");
        ui.field(gr, "code_goto", &gq, self.t("Line number"));
        y += ui.dp(44.0);
        let t = &mut v.tabs[v.cur];
        if let Some(i) = hit {
            match i {
                0 => {
                    if let Ok(n) = self.f("code_goto").trim().parse::<usize>() {
                        t.buf.goto_line(n);
                        self.office.menu = false;
                    } else {
                        self.focus = Some("code_goto");
                        self.focus_reset = true;
                    }
                }
                1 => t.buf.duplicate_line(),
                2 => t.buf.delete_line(),
                3 => t.buf.move_line(false),
                4 => t.buf.move_line(true),
                5 => t.buf.toggle_comment(t.lang),
                6 => t.buf.indent("    ", false),
                7 => t.buf.indent("    ", true),
                8 => {
                    let k = Lang::ALL.iter().position(|l| *l == t.lang).unwrap_or(0);
                    t.lang = Lang::ALL[(k + 1) % Lang::ALL.len()];
                    self.toast(t.lang.name().into());
                }
                9 => {
                    let k = uo::code::Enc::ALL
                        .iter()
                        .position(|e| *e == t.buf.enc)
                        .unwrap_or(0);
                    t.buf.enc = uo::code::Enc::ALL[(k + 1) % uo::code::Enc::ALL.len()];
                    t.buf.dirty = true;
                    self.toast(format!("{} → {}", self.t("Save as"), t.buf.enc.label()));
                }
                10 => {
                    t.buf.eol = match t.buf.eol {
                        uo::code::Eol::Lf => uo::code::Eol::CrLf,
                        _ => uo::code::Eol::Lf,
                    };
                    t.buf.dirty = true;
                    self.toast(t.buf.eol.label().into());
                }
                _ => {
                    if t.buf.dirty {
                        let _ = uo::write_atomic(&t.path, &t.buf.to_bytes())
                            .map(|_| t.buf.dirty = false);
                    }
                    let p = t.path.clone();
                    let what = self.t("Explain and improve").to_string();
                    self.office.menu = false;
                    self.fields.insert(
                        "chat",
                        match self.lang() {
                            "ar" => format!("{what} الملف {p}"),
                            "de" => format!("{what}: Datei {p}"),
                            _ => format!("{what}: file {p}"),
                        },
                    );
                    self.go(Screen::Agent);
                    self.focus = Some("chat");
                    self.focus_reset = true;
                }
            }
            t.follow = true;
            ui.dirty = true;
        }
        y
    }

    // ------------------------------------------------------------------ Agent-Werkzeuge

    /// Ist die Datei gerade in einer Büro-App offen? Dann nach einer Änderung durch den Agenten neu laden.
    fn office_reload(&mut self, path: &str) {
        if self.office.doc.as_ref().is_some_and(|d| d.path == path) {
            self.office.doc = None;
        }
        if self.office.sheet.as_ref().is_some_and(|s| s.path == path) {
            self.office.sheet = None;
        }
        if let Some(c) = self.office.code.as_mut() {
            for t in &mut c.tabs {
                if t.path == path {
                    if let Ok(b) = std::fs::read(path) {
                        t.buf = Buffer::from_bytes(&b);
                    }
                }
            }
        }
        if let Some(Screen::Office(x)) = self.stack.last().map(|s| s.0.clone()) {
            self.office_on_go(&Screen::Office(x));
        }
    }

    /// Werkzeuge des Agenten für Dokumente, Tabellen, PDF und Textdateien. None = hier nicht bekannt.
    pub(super) fn exec_office(&mut self, c: &crate::agent::Call) -> Option<String> {
        let path = c.arg("path");
        let need_path = |p: &str| -> Result<(), String> {
            if super::app_agent::agent_path_ok(p) && !p.ends_with('/') && !p.is_empty() {
                Ok(())
            } else {
                Err("error: only files under /data are allowed (not /data/ultar)".into())
            }
        };
        let res = match c.tool.as_str() {
            "doc_current" => {
                self.office_leave();
                match self.stack.last().map(|s| s.0.clone()) {
                    Some(Screen::Office(Ox::Pdf(p))) => {
                        let page = self.office.pdf.as_ref().map(|v| v.cur + 1).unwrap_or(1);
                        format!("open: {p} (PDF, page {page})")
                    }
                    Some(Screen::Office(Ox::Doc(p))) => {
                        let sel = self
                            .office
                            .doc
                            .as_ref()
                            .and_then(|d| d.sel.and_then(|i| d.doc.blocks.get(i)))
                            .map(|b| {
                                if let Block::Para(p) = b {
                                    p.text()
                                } else {
                                    String::new()
                                }
                            })
                            .unwrap_or_default();
                        format!(
                            "open: {p} (document){}",
                            if sel.is_empty() {
                                String::new()
                            } else {
                                format!(", selected paragraph: {sel}")
                            }
                        )
                    }
                    Some(Screen::Office(Ox::Sheet(p))) => {
                        let (cell, raw) = self
                            .office
                            .sheet
                            .as_ref()
                            .map(|s| {
                                (
                                    cell_name(s.sel.0, s.sel.1),
                                    s.wb.sheets
                                        .get(s.si)
                                        .map(|sh| format!("{}!", sh.name))
                                        .unwrap_or_default()
                                        + &s.wb
                                            .sheets
                                            .get(s.si)
                                            .map(|sh| sh.get(s.sel.0, s.sel.1).to_string())
                                            .unwrap_or_default(),
                                )
                            })
                            .unwrap_or_default();
                        format!("open: {p} (sheet), selected cell {cell}: {raw}")
                    }
                    Some(Screen::Office(Ox::Code(p))) => {
                        let (l, col, sel) = self
                            .office
                            .code
                            .as_ref()
                            .and_then(|v| v.tabs.get(v.cur))
                            .map(|t| {
                                let (l, c) = t.buf.line_col(t.buf.cur);
                                (l + 1, c + 1, t.buf.selected_text())
                            })
                            .unwrap_or((1, 1, String::new()));
                        format!(
                            "open: {p} (text), cursor line {l} column {col}{}",
                            if sel.is_empty() {
                                String::new()
                            } else {
                                format!(
                                    ", selection: {}",
                                    sel.chars().take(400).collect::<String>()
                                )
                            }
                        )
                    }
                    _ => "no document is open".into(),
                }
            }
            "open_doc" => {
                if let Err(e) = need_path(&path) {
                    return Some(e);
                }
                match Self::office_screen_for(&path) {
                    Some(s) if std::path::Path::new(&path).exists() => {
                        self.go(s);
                        "ok: opened".into()
                    }
                    Some(_) => "error: file not found".into(),
                    None => "error: not a document, sheet, PDF or text file".into(),
                }
            }
            "doc_read" => {
                if let Err(e) = need_path(&path) {
                    return Some(e);
                }
                self.office_leave();
                let page = c.arg("page").trim().parse::<usize>().ok();
                match (uo::Kind::of(&path), page) {
                    (uo::Kind::Pdf, Some(p)) if p > 0 => match std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .and_then(uo::pdf::PdfDoc::open)
                    {
                        Ok(d) if p <= d.pages() => format!(
                            "page {p}/{}:\n{}",
                            d.pages(),
                            d.page_text(p - 1).chars().take(3000).collect::<String>()
                        ),
                        Ok(d) => format!("error: page 1..{}", d.pages()),
                        Err(e) => format!("error: {e}"),
                    },
                    _ => match uo::read_text(&path, 3000) {
                        Ok(t) => t,
                        Err(e) => format!("error: {e}"),
                    },
                }
            }
            "doc_info" => {
                if let Err(e) = need_path(&path) {
                    return Some(e);
                }
                match uo::info(&path) {
                    Ok(v) => v.to_string(),
                    Err(e) => format!("error: {e}"),
                }
            }
            "doc_create" => {
                if let Err(e) = need_path(&path) {
                    return Some(e);
                }
                let content = c.arg("content");
                match uo::create(&path, &content) {
                    Ok(_) => {
                        self.office_reload(&path);
                        format!("ok: {path} written ({} chars)", content.chars().count())
                    }
                    Err(e) => format!("error: {e}"),
                }
            }
            "doc_replace" => {
                if let Err(e) = need_path(&path) {
                    return Some(e);
                }
                self.office_leave();
                let regex = matches!(c.arg("regex").as_str(), "true" | "1");
                match uo::replace_in_file(&path, &c.arg("find"), &c.arg("replace"), regex) {
                    Ok(n) => {
                        self.office_reload(&path);
                        format!("ok: {n} replaced")
                    }
                    Err(e) => format!("error: {e}"),
                }
            }
            "doc_append" => {
                if let Err(e) = need_path(&path) {
                    return Some(e);
                }
                self.office_leave();
                let text = c.arg("text");
                let r = match uo::Kind::of(&path) {
                    uo::Kind::Doc => uo::read_doc(&path)
                        .or_else(|_| Ok::<_, String>(Document::default()))
                        .and_then(|mut d| {
                            d.blocks.extend(Document::from_markdown(&text).blocks);
                            uo::write_doc(&path, &d)
                        }),
                    uo::Kind::Text => {
                        let b = std::fs::read(&path).unwrap_or_default();
                        let mut buf = Buffer::from_bytes(&b);
                        buf.cur = buf.text.len();
                        let nl = if buf.text.is_empty() || buf.text.ends_with('\n') {
                            ""
                        } else {
                            "\n"
                        };
                        buf.insert(&format!("{nl}{text}"));
                        uo::write_atomic(&path, &buf.to_bytes())
                    }
                    _ => Err("append works for documents and text files".into()),
                };
                match r {
                    Ok(_) => {
                        self.office_reload(&path);
                        "ok: appended".into()
                    }
                    Err(e) => format!("error: {e}"),
                }
            }
            "sheet_read" => {
                if let Err(e) = need_path(&path) {
                    return Some(e);
                }
                self.office_leave();
                match uo::sheet_get(&path, &c.arg("range")) {
                    Ok(t) => t.chars().take(3000).collect(),
                    Err(e) => format!("error: {e}"),
                }
            }
            "sheet_set" => {
                if let Err(e) = need_path(&path) {
                    return Some(e);
                }
                self.office_leave();
                match uo::sheet_set(&path, &c.arg("cell"), &c.arg("value")) {
                    Ok(v) => {
                        self.office_reload(&path);
                        format!("ok: {} = {v}", c.arg("cell"))
                    }
                    Err(e) => format!("error: {e}"),
                }
            }
            "sheet_eval" => {
                let wb = if path.is_empty() {
                    Ok(Workbook::new())
                } else {
                    need_path(&path).and_then(|_| uo::read_book(&path))
                };
                match wb {
                    Ok(wb) => wb.eval_formula(0, &c.arg("formula")).text(),
                    Err(e) => format!("error: {e}"),
                }
            }
            "pdf_search" => {
                if let Err(e) = need_path(&path) {
                    return Some(e);
                }
                match std::fs::read(&path)
                    .map_err(|e| e.to_string())
                    .and_then(uo::pdf::PdfDoc::open)
                {
                    Ok(d) => {
                        let r = d.search(&c.arg("query"), 20);
                        if r.is_empty() {
                            "no results".into()
                        } else {
                            r.iter()
                                .map(|(p, s)| format!("page {}: {s}", p + 1))
                                .collect::<Vec<_>>()
                                .join("\n")
                        }
                    }
                    Err(e) => format!("error: {e}"),
                }
            }
            "list_docs" => {
                let k: Vec<u8> = match c.arg("kind").as_str() {
                    "pdf" => vec![0],
                    "doc" | "docx" | "word" => vec![1],
                    "sheet" | "xlsx" | "excel" => vec![2],
                    "code" | "text" => vec![3],
                    _ => vec![0, 1, 2],
                };
                let mut v: Vec<(String, u64, u64)> = k.into_iter().flat_map(scan).collect();
                v.sort_by(|a, b| b.2.cmp(&a.2));
                if v.is_empty() {
                    "no documents found".into()
                } else {
                    v.iter()
                        .take(40)
                        .map(|f| f.0.clone())
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            _ => return None,
        };
        Some(res)
    }
}

fn list_icon(kind: u8) -> Ic {
    match kind {
        0 => Ic::Svg("pdf"),
        1 => Ic::Doc,
        2 => Ic::Svg("xls"),
        _ => Ic::Svg("code"),
    }
}

thread_local! {
    /// Höhe des zuletzt gezeichneten Dokuments (für das Papier hinter dem Text)
    static SCRATCH_H: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spalten_mit_tabulator() {
        assert_eq!(vcol("\tab", 1), 4);
        assert_eq!(vcol("a\tb", 2), 4);
        assert_eq!(byte_at_vcol("\tab", 4), 1);
        assert_eq!(byte_at_vcol("abc", 2), 2);
        assert_eq!(byte_at_vcol("abc", 9), 3);
    }
}
