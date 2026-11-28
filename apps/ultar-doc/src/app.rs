//! The word processor window: a tool bar (new, save, undo/redo, formatting, lists, direction,
//! alignment, table, find), the document drawn by [`TextView`] over the wrapped flat view, a
//! status line (word count, zoom, message) and three dialogs: "save changes?", the name of a
//! new document and find/replace. Three layout classes (compact, medium, wide) select which
//! buttons appear; the page stays white, the window dark or light.

use crate::state::{Doc, DocError, FlatView, MIN_CHARS};
use crate::{keys, place};
use std::collections::HashMap;
use std::path::PathBuf;
use ultar_textview::{Marks, Pos, TextView, ViewError};
use ultar_ui::event::{Action, ActionKind, Event, PointerButton};
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Dialog, Label, PixelArea, Text, TextField};

/// What the window has to do for the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Copy(String),
    Paste,
    Title(String),
    Close,
}

/// An open question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Unsaved { then_close: bool },
    Name { then_close: bool },
    Find,
}

/// One toolbar command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Cmd {
    New,
    Save,
    SaveAs,
    Undo,
    Redo,
    Bold,
    Italic,
    Underline,
    Strike,
    Title,
    H1,
    H2,
    H3,
    Normal,
    AlignStart,
    AlignCenter,
    AlignEnd,
    AlignJustify,
    DirRtl,
    DirLtr,
    Bullet,
    Number,
    Indent,
    Outdent,
    Table,
    RowAdd,
    RowDel,
    ColAdd,
    ColDel,
    ZoomIn,
    ZoomOut,
    PageToggle,
    Find,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Compact,
    Medium,
    Wide,
}

fn class(w: f32) -> Class {
    if w < 480.0 {
        Class::Compact
    } else if w < 800.0 {
        Class::Medium
    } else {
        Class::Wide
    }
}

fn commands(cls: Class) -> &'static [Cmd] {
    use Cmd::*;
    match cls {
        Class::Compact => &[
            New, Save, Undo, Redo, Bold, Italic, Underline, Bullet, Number, DirRtl, DirLtr, Find,
        ],
        Class::Medium => &[
            New, Save, Undo, Redo, Bold, Italic, Underline, Strike, Title, H1, H2, H3, AlignStart,
            AlignCenter, AlignEnd, Bullet, Number, DirRtl, DirLtr, Indent, Outdent, Find, ZoomIn,
            ZoomOut,
        ],
        Class::Wide => &[
            New, Save, SaveAs, Undo, Redo, Bold, Italic, Underline, Strike, Title, H1, H2, H3,
            Normal, AlignStart, AlignCenter, AlignEnd, AlignJustify, Bullet, Number, DirRtl, DirLtr,
            Indent, Outdent, Table, RowAdd, RowDel, ColAdd, ColDel, ZoomIn, ZoomOut, PageToggle, Find,
        ],
    }
}

#[derive(Default)]
struct Nodes {
    text: Option<NodeId>,
    status: Option<NodeId>,
    dialog: Option<NodeId>,
    name_field: Option<NodeId>,
    find_field: Option<NodeId>,
    replace_field: Option<NodeId>,
    ask_save: Option<NodeId>,
    ask_discard: Option<NodeId>,
    ask_cancel: Option<NodeId>,
}

/// The word processor: its tree, its document and its text view.
pub struct App {
    pub ui: Ui,
    pub doc: Doc,
    view: TextView,
    flat: FlatView,
    areas: PathBuf,
    open_uri: Option<String>,
    nodes: Nodes,
    cmds: HashMap<Cmd, NodeId>,
    ask: Option<Ask>,
    message: Option<Label>,
    selecting: bool,
    out: Vec<Out>,
}

impl App {
    /// A word processor over `ui` for `doc`; `areas` is where the user's places are bound.
    pub fn new(
        ui: Ui,
        doc: Doc,
        areas: PathBuf,
        open_uri: Option<String>,
    ) -> Result<App, ViewError> {
        let view = TextView::new(*ui.env(), false)?;
        let flat = doc.flatten(60);
        let mut e = App {
            ui,
            doc,
            view,
            flat,
            areas,
            open_uri,
            nodes: Nodes::default(),
            cmds: HashMap::new(),
            ask: None,
            message: None,
            selecting: false,
            out: Vec::new(),
        };
        e.rebuild().map_err(|err| ViewError(format!("{err:?}")))?;
        let title = e.title();
        e.out.push(Out::Title(title));
        Ok(e)
    }

    /// Opens `uri` (a `files://` URI) or starts a new document.
    pub fn open(uri: Option<&str>, areas: &std::path::Path) -> (Doc, Option<DocError>) {
        let Some(uri) = uri else {
            return (Doc::new(), None);
        };
        let opened = place::path_of(uri, areas)
            .ok_or(DocError::Missing)
            .and_then(|p| Doc::open(&p));
        match opened {
            Ok(doc) => (doc, None),
            Err(e) => (Doc::new(), Some(e)),
        }
    }

    /// Says in the status line why the file did not open.
    pub fn open_failed(&mut self, e: DocError) -> Result<(), UiError> {
        let tr = self.ui.translator();
        let why = tr.t(e.key()).into_owned();
        let message = Label::key_with("doc.open_failed", &[("reason", &why)]);
        self.say(message)
    }

    fn reason(&self, key: &'static str, name: &str, e: DocError) -> Label {
        let why = self.ui.translator().t(e.key()).into_owned();
        Label::key_with(key, &[("name", name), ("reason", &why)])
    }

    /// Shows `message` in the status line.
    pub fn say(&mut self, message: Label) -> Result<(), UiError> {
        self.message = Some(message);
        self.status()
    }

    /// The window title: the document's name, with a dot while unsaved.
    pub fn title(&self) -> String {
        let tr = self.ui.translator();
        let name = self
            .doc
            .name()
            .unwrap_or_else(|| tr.t("doc.untitled").into_owned());
        let dot = if self.doc.dirty() { "• " } else { "" };
        format!("{dot}{name} – {}", tr.t("app.doc.name"))
    }

    /// What the window has to do (taken once).
    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// The URI the document was opened from (for the device mesh), with its place fragment.
    pub fn open_uri(&self) -> Option<&str> {
        self.open_uri.as_deref()
    }

    /// Builds the whole tree (also after a size change).
    pub fn rebuild(&mut self) -> Result<(), UiError> {
        self.nodes = Nodes::default();
        let cls = class(self.ui.size().w);
        let mut cmds_new = HashMap::new();
        {
            let ui = &mut self.ui;
            ui.clear();
            ui.set_root_flex(Flex::column().cross(CrossAlign::Stretch));
            let root = ui.root();
            let bar = ui.add(
                root,
                Flex::row()
                    .gap(Space::S4)
                    .padding(Padding::all(Space::S8))
                    .cross(CrossAlign::Center),
            )?;
            for cmd in commands(cls) {
                let key = cmd_key(*cmd);
                let btn = if matches!(cmd, Cmd::Save) {
                    Button::new(Label::key(key))
                } else {
                    Button::new(Label::key(key)).variant(ButtonVariant::Plain)
                };
                let id = ui.add(bar, btn)?;
                cmds_new.insert(*cmd, id);
            }
            let name = self
                .doc
                .name()
                .map_or_else(|| Label::key("doc.untitled"), |n| Label::data(&n));
            let _title = ui.add_sized(
                bar,
                Text::new(name).color(ColorRole::TextMuted),
                Sizing::grow(1),
            )?;
            let text = ui.add_sized(
                root,
                PixelArea::new(Label::key("doc.text")),
                Sizing::grow(1),
            )?;
            self.nodes.text = Some(text);
            self.nodes.status = Some(ui.add(
                root,
                Flex::row().padding(Padding::all(Space::S4)).main(MainAlign::Start),
            )?);
            self.nodes.status = Some(
                ui.add(
                    self.nodes.status.unwrap_or(root),
                    Text::new(Label::data("")).color(ColorRole::TextMuted),
                )?,
            );
        }
        self.cmds = cmds_new;
        self.build_dialog()?;
        if self.ask.is_none() {
            let text = self.nodes.text;
            if let Some(t) = text {
                let _ = self.ui.focus(t);
            }
        }
        self.buttons()?;
        self.status()?;
        self.refresh()
    }

    fn build_dialog(&mut self) -> Result<(), UiError> {
        let Some(ask) = self.ask else {
            return Ok(());
        };
        let root = self.ui.root();
        let name = self
            .doc
            .name()
            .unwrap_or_else(|| self.ui.translator().t("doc.untitled").into_owned());
        let title = match ask {
            Ask::Unsaved { .. } => Label::key_with("doc.ask.title", &[("name", &name)]),
            Ask::Name { .. } => Label::key("doc.name.title"),
            Ask::Find => Label::key("doc.find.title"),
        };
        let dialog = self.ui.add(root, Dialog::new(title))?;
        self.nodes.dialog = Some(dialog);
        match ask {
            Ask::Unsaved { .. } => {
                self.ui.add(dialog, Text::new(Label::key("doc.ask.message")).color(ColorRole::TextMuted))?;
            }
            Ask::Name { .. } => {
                let field = TextField::new(Label::key("doc.name.field")).labelled();
                self.nodes.name_field = Some(self.ui.add(dialog, field)?);
            }
            Ask::Find => {
                let find = TextField::new(Label::key("doc.find.field")).labelled();
                self.nodes.find_field = Some(self.ui.add(dialog, find)?);
                let repl = TextField::new(Label::key("doc.replace.field")).labelled();
                self.nodes.replace_field = Some(self.ui.add(dialog, repl)?);
            }
        }
        let row = self.ui.add(dialog, Flex::row().gap(Space::S8).main(MainAlign::End))?;
        let cancel = self.ui.add(row, Button::new(Label::key("doc.ask.cancel")).variant(ButtonVariant::Plain))?;
        self.nodes.ask_cancel = Some(cancel);
        if matches!(ask, Ask::Unsaved { .. }) {
            self.nodes.ask_discard = Some(self.ui.add(row, Button::new(Label::key("doc.ask.discard")).variant(ButtonVariant::Danger))?);
        }
        let ok_key = match ask {
            Ask::Unsaved { .. } => "doc.ask.save",
            Ask::Name { .. } => "doc.name.ok",
            Ask::Find => "doc.find.ok",
        };
        let ok = self.ui.add(row, Button::new(Label::key(ok_key)))?;
        self.nodes.ask_save = Some(ok);
        self.ui.set_default_action(dialog, Some(ok))?;
        self.ui.open_dialog(dialog)?;
        if let Some(field) = self.nodes.name_field.or(self.nodes.find_field) {
            self.ui.focus(field)?;
        }
        Ok(())
    }

    fn buttons(&mut self) -> Result<(), UiError> {
        for cmd in [Cmd::Undo, Cmd::Redo] {
            if let Some(&n) = self.cmds.get(&cmd) {
                let off = if cmd == Cmd::Undo { !self.doc.can_undo() } else { !self.doc.can_redo() };
                self.ui.set_disabled(n, off)?;
            }
        }
        Ok(())
    }

    fn status(&mut self) -> Result<(), UiError> {
        let Some(node) = self.nodes.status else {
            return Ok(());
        };
        let tr = self.ui.translator();
        let words = self.doc.document.word_count();
        let zoom = (self.doc.zoom * 100.0).round() as u32;
        let place = Label::key_with(
            "doc.status",
            &[("words", &words.to_string()), ("zoom", &zoom.to_string())],
        )
        .resolve(tr)
        .into_owned();
        let mut text = place;
        if self.doc.dirty() {
            text.push(' ');
            text.push_str(&tr.t("doc.dirty").into_owned());
        }
        if let Some(m) = &self.message {
            text.push_str("  ·  ");
            text.push_str(&m.resolve(tr));
        }
        self.ui.set_label(node, Label::data(&text))
    }

    fn refresh(&mut self) -> Result<(), UiError> {
        let chars = self.max_chars();
        self.flat = self.doc.flatten(chars);
        self.draw_text()
    }

    fn max_chars(&self) -> usize {
        let Some(node) = self.nodes.text else {
            return 60;
        };
        let rect = self.ui.rect(node);
        let scale = self.ui.env().scale;
        let w = rect.map_or(800.0, |r| r.w * scale);
        // ~8 device pixels per Latin character at the body size
        let chars = (w / 8.0 / self.doc.zoom).max(MIN_CHARS as f32) as usize;
        chars.max(8)
    }

    fn text_size(&mut self) -> Option<(u32, u32)> {
        self.ui.layout();
        let rect = self.ui.rect(self.nodes.text?)?;
        let s = self.ui.env().scale;
        let (w, h) = ((rect.w * s).round(), (rect.h * s).round());
        (w >= 1.0 && h >= 1.0).then_some((w as u32, h as u32))
    }

    /// Draws the visible text into the picture.
    pub fn draw_text(&mut self) -> Result<(), UiError> {
        let (Some(node), Some((w, h))) = (self.nodes.text, self.text_size()) else {
            return Ok(());
        };
        self.view.set_env(*self.ui.env());
        let marks = self.marks();
        let frame = self
            .view
            .render(w, h, &self.flat, &marks)
            .map_err(|_| UiError::InvalidSize)?;
        self.ui.set_picture(node, frame)
    }

    /// The caret and selection as flat positions.
    fn marks(&self) -> Marks {
        Marks {
            caret: self.caret_pos(),
            selection: self.selection_pos(),
        }
    }

    fn caret_pos(&self) -> Option<Pos> {
        let c = self.doc.cursor;
        let line = self.flat.lines.iter().position(|l| {
            l.block == c.block
                && l.cell == c.cell
                && c.ch >= l.start
                && c.ch <= l.start + l.text.chars().count()
        })?;
        let byte = self.doc.byte_in(&self.flat, c)?;
        Some(Pos::new(line, byte))
    }

    fn selection_pos(&self) -> Option<(Pos, Pos)> {
        let (a, b) = self.doc.selection()?;
        let la = self.flat.lines.iter().position(|l| l.block == a.block && l.cell == a.cell && a.ch >= l.start && a.ch <= l.start + l.text.chars().count())?;
        let lb = self.flat.lines.iter().position(|l| l.block == b.block && l.cell == b.cell && b.ch >= l.start && b.ch <= l.start + l.text.chars().count())?;
        Some((
            Pos::new(la, self.doc.byte_in(&self.flat, a)?),
            Pos::new(lb, self.doc.byte_in(&self.flat, b)?),
        ))
    }

    fn rows(&mut self) -> usize {
        let h = self.text_size().map_or(1, |s| s.1);
        self.view.rows(h)
    }

    fn after(&mut self, effect: keys::Effect) -> Result<(), UiError> {
        match effect {
            keys::Effect::None => return Ok(()),
            keys::Effect::Copy(text) => self.out.push(Out::Copy(text)),
            keys::Effect::Paste => self.out.push(Out::Paste),
            keys::Effect::Save => return self.save(false),
            keys::Effect::New => return self.new_doc(),
            keys::Effect::Changed => {
                self.message = None;
                let title = self.title();
                self.out.push(Out::Title(title));
            }
            keys::Effect::Moved => {}
        }
        if let Some(pos) = self.caret_pos() {
            let rows = self.rows();
            self.view.reveal(pos.line, rows);
        }
        self.buttons()?;
        self.status()?;
        self.refresh()
    }

    /// A key the widgets did not use.
    pub fn key(&mut self, event: Event) -> Result<(), UiError> {
        if self.ask.is_some() {
            return Ok(());
        }
        let Event::Key { key, modifiers } = event else {
            return Ok(());
        };
        let rtl = self
            .flat
            .lines
            .iter()
            .find(|l| l.block == self.doc.cursor.block && l.cell == self.doc.cursor.cell)
            .map_or(false, |l| l.rtl);
        let effect = keys::apply(&mut self.doc, key, modifiers, rtl);
        self.after(effect)
    }

    /// Clipboard text that arrived.
    pub fn pasted(&mut self, text: &str) -> Result<(), UiError> {
        if self.ask.is_some() {
            return Ok(());
        }
        let effect = keys::paste(&mut self.doc, text);
        self.after(effect)
    }

    pub fn text_focused(&self) -> bool {
        self.ask.is_none() && self.ui.focused().is_some() && self.ui.focused() == self.nodes.text
    }

    /// A pointer event (the widgets saw it too): caret, selection and scrolling.
    pub fn pointer(&mut self, event: Event) -> Result<(), UiError> {
        if self.ask.is_some() {
            return Ok(());
        }
        let Some(node) = self.nodes.text else {
            return Ok(());
        };
        let (pos, down) = match event {
            Event::PointerDown { pos, button: PointerButton::Primary } => (pos, true),
            Event::PointerMove { pos } if self.selecting => (pos, false),
            Event::PointerUp { .. } => {
                self.selecting = false;
                return Ok(());
            }
            Event::Scroll { pos, lines } => {
                if self.ui.rect(node).is_some_and(|r| r.contains(pos)) {
                    let total = self.flat.lines.len();
                    let rows = self.rows();
                    self.view.scroll(lines.round() as isize * 3, total, rows);
                    self.draw_text()?;
                }
                return Ok(());
            }
            _ => return Ok(()),
        };
        let Some(rect) = self.ui.rect(node) else {
            return Ok(());
        };
        if down && !rect.contains(pos) {
            return Ok(());
        }
        let s = self.ui.env().scale;
        let width = (rect.w * s).round().max(1.0) as u32;
        let at = self.view.hit(width, (pos.x - rect.x) * s, (pos.y - rect.y) * s, &self.flat);
        let c = self.doc.cursor_at(&self.flat, at.line, at.byte);
        if down {
            self.doc.anchor = Some(c);
            self.selecting = true;
            let _ = self.ui.focus(node);
        }
        self.doc.cursor = c;
        self.after(keys::Effect::Moved)
    }

    /// The window was asked to close.
    pub fn close_requested(&mut self) -> Result<(), UiError> {
        if !self.doc.dirty() {
            self.out.push(Out::Close);
            return Ok(());
        }
        if self.ask.is_none() {
            self.ask = Some(Ask::Unsaved { then_close: true });
            self.rebuild()?;
        }
        Ok(())
    }

    fn new_doc(&mut self) -> Result<(), UiError> {
        if self.doc.dirty() {
            self.ask = Some(Ask::Unsaved { then_close: false });
            return self.rebuild();
        }
        self.reset_doc()
    }

    fn reset_doc(&mut self) -> Result<(), UiError> {
        self.doc = Doc::new();
        self.flat = self.doc.flatten(self.max_chars());
        self.view.first = 0;
        self.message = None;
        let title = self.title();
        self.out.push(Out::Title(title));
        self.rebuild()
    }

    fn save(&mut self, then_close: bool) -> Result<(), UiError> {
        if self.doc.path.is_none() {
            self.ask = Some(Ask::Name { then_close });
            return self.rebuild();
        }
        let name = self.doc.name().unwrap_or_default();
        match self.doc.save() {
            Ok(()) => {
                self.message = Some(Label::key_with("doc.saved", &[("name", &name)]));
                let title = self.title();
                self.out.push(Out::Title(title));
                if then_close {
                    self.out.push(Out::Close);
                }
            }
            Err(e) => self.message = Some(self.reason("doc.save_failed", &name, e)),
        }
        self.status()
    }

    fn save_named(&mut self, then_close: bool) -> Result<(), UiError> {
        let name = self
            .nodes
            .name_field
            .and_then(|f| self.ui.text(f).ok())
            .unwrap_or_default()
            .to_owned();
        let result = place::new_path(&name, &self.areas)
            .ok_or(DocError::BadName)
            .and_then(|p| self.doc.save_as(&p));
        self.ask = None;
        match result {
            Ok(()) => {
                let shown = self.doc.name().unwrap_or_default();
                self.message = Some(Label::key_with("doc.saved", &[("name", &shown)]));
                let title = self.title();
                self.out.push(Out::Title(title));
                if then_close {
                    self.out.push(Out::Close);
                }
            }
            Err(e) => self.message = Some(self.reason("doc.save_failed", &name, e)),
        }
        self.rebuild()
    }

    fn do_cmd(&mut self, cmd: Cmd) -> Result<(), UiError> {
        use ultar_office::doc::{Align, Dir, Style};
        match cmd {
            Cmd::New => self.new_doc()?,
            Cmd::Save => self.save(false)?,
            Cmd::SaveAs => {
                self.ask = Some(Ask::Name { then_close: false });
                self.rebuild()?;
            }
            Cmd::Undo => {
                if self.doc.undo() {
                    self.after(keys::Effect::Changed)?;
                }
            }
            Cmd::Redo => {
                if self.doc.redo() {
                    self.after(keys::Effect::Changed)?;
                }
            }
            Cmd::Bold => self.doc.for_each_run(|r| r.bold = !r.bold),
            Cmd::Italic => self.doc.for_each_run(|r| r.italic = !r.italic),
            Cmd::Underline => self.doc.for_each_run(|r| r.underline = !r.underline),
            Cmd::Strike => self.doc.for_each_run(|r| r.strike = !r.strike),
            Cmd::Title => self.doc.set_style(Style::Title),
            Cmd::H1 => self.doc.set_style(Style::H1),
            Cmd::H2 => self.doc.set_style(Style::H2),
            Cmd::H3 => self.doc.set_style(Style::H3),
            Cmd::Normal => self.doc.set_style(Style::Normal),
            Cmd::AlignStart => self.doc.set_align(Align::Start),
            Cmd::AlignCenter => self.doc.set_align(Align::Center),
            Cmd::AlignEnd => self.doc.set_align(Align::End),
            Cmd::AlignJustify => self.doc.set_align(Align::Justify),
            Cmd::DirRtl => self.doc.set_dir(Dir::Rtl),
            Cmd::DirLtr => self.doc.set_dir(Dir::Ltr),
            Cmd::Bullet => self.doc.toggle_list(true),
            Cmd::Number => self.doc.toggle_list(false),
            Cmd::Indent => self.doc.indent(24.0),
            Cmd::Outdent => self.doc.indent(-24.0),
            Cmd::Table => self.insert_table(),
            Cmd::RowAdd => self.table_row(1),
            Cmd::RowDel => self.table_row(-1),
            Cmd::ColAdd => self.table_col(1),
            Cmd::ColDel => self.table_col(-1),
            Cmd::ZoomIn => {
                self.doc.zoom = (self.doc.zoom * 1.25).min(3.0);
                self.after(keys::Effect::Changed)?;
            }
            Cmd::ZoomOut => {
                self.doc.zoom = (self.doc.zoom / 1.25).max(0.5);
                self.after(keys::Effect::Changed)?;
            }
            Cmd::PageToggle => {
                self.doc.page_view = !self.doc.page_view;
                self.after(keys::Effect::Changed)?;
            }
            Cmd::Find => {
                self.ask = Some(Ask::Find);
                self.rebuild()?;
            }
        }
        if !matches!(cmd, Cmd::New | Cmd::Save | Cmd::SaveAs | Cmd::Undo | Cmd::Redo | Cmd::Find | Cmd::ZoomIn | Cmd::ZoomOut | Cmd::PageToggle) {
            self.doc.burst_break();
            self.after(keys::Effect::Changed)?;
        }
        Ok(())
    }

    fn insert_table(&mut self) {
        let b = self.doc.cursor.block + 1;
        let rows = vec![vec![String::new(); 2]; 2];
        self.doc.document.blocks.insert(b, ultar_office::doc::Block::Table(rows));
        self.doc.dirty = true;
    }

    fn table_clone(&self) -> Option<(usize, Vec<Vec<String>>)> {
        let b = self.doc.cursor.block;
        match self.doc.document.blocks.get(b) {
            Some(ultar_office::doc::Block::Table(t)) => Some((b, t.clone())),
            _ => None,
        }
    }

    fn table_row(&mut self, delta: isize) {
        let Some((b, mut t)) = self.table_clone() else {
            let _ = self.say(Label::key("doc.table.need"));
            return;
        };
        let cols = t.first().map_or(1, |r| r.len());
        if delta > 0 {
            t.push(vec![String::new(); cols]);
        } else if t.len() > 1 {
            t.pop();
        }
        self.doc.document.blocks[b] = ultar_office::doc::Block::Table(t);
        self.doc.dirty = true;
    }

    fn table_col(&mut self, delta: isize) {
        let Some((b, mut t)) = self.table_clone() else {
            let _ = self.say(Label::key("doc.table.need"));
            return;
        };
        if delta > 0 {
            for row in &mut t {
                row.push(String::new());
            }
        } else if t.first().map_or(0, |r| r.len()) > 1 {
            for row in &mut t {
                row.pop();
            }
        }
        self.doc.document.blocks[b] = ultar_office::doc::Block::Table(t);
        self.doc.dirty = true;
    }

    fn find_next(&mut self, needle: &str, repl: &str) -> usize {
        if needle.is_empty() {
            return 0;
        }
        if !repl.is_empty() {
            let n = self.doc.document.replace(needle, repl);
            if n > 0 {
                self.doc.dirty = true;
            }
            return n;
        }
        // move the caret to the next match
        let blocks = self.doc.document.blocks.len();
        let start = self.doc.cursor.block;
        for off in 0..blocks {
            let bi = (start + off) % blocks;
            let Some(t) = self.doc.item_text(bi, None) else {
                continue;
            };
            let from = if bi == self.doc.cursor.block { self.doc.cursor.ch } else { 0 };
            if let Some(i) = find_in(&t, needle, from) {
                self.doc.anchor = Some(crate::state::Cursor { block: bi, cell: None, ch: i });
                self.doc.cursor = crate::state::Cursor { block: bi, cell: None, ch: i + needle.chars().count() };
                return 1;
            }
        }
        0
    }

    /// The actions of the widget tree.
    pub fn actions(&mut self, actions: Vec<Action>) -> Result<(), UiError> {
        for a in actions {
            let activated = matches!(a.kind, ActionKind::Activated | ActionKind::Submitted);
            if !activated {
                continue;
            }
            let n = &self.nodes;
            let node = Some(a.node);
            let dial = n.ask_cancel.is_some() && node == n.ask_cancel;
            let disc = n.ask_discard.is_some() && node == n.ask_discard;
            let ok = (n.ask_save.is_some() && node == n.ask_save)
                || (n.find_field.is_some() && node == n.find_field)
                || (n.name_field.is_some() && node == n.name_field && self.ask.is_some());
            let field_ok = (n.find_field.is_some() && node == n.find_field)
                || (n.name_field.is_some() && node == n.name_field && self.ask.is_some());
            if dial {
                self.ask = None;
                self.rebuild()?;
                continue;
            }
            if disc {
                let close = matches!(self.ask, Some(Ask::Unsaved { then_close: true }));
                self.ask = None;
                if close {
                    self.out.push(Out::Close);
                } else {
                    self.doc.dirty = false;
                    self.reset_doc()?;
                }
                continue;
            }
            if ok && field_ok {
                match self.ask {
                    Some(Ask::Unsaved { then_close }) => {
                        self.ask = None;
                        self.rebuild()?;
                        self.save(then_close)?;
                    }
                    Some(Ask::Name { then_close }) => self.save_named(then_close)?,
                    Some(Ask::Find) => {
                        let find = self
                            .nodes
                            .find_field
                            .and_then(|f| self.ui.text(f).ok())
                            .unwrap_or_default()
                            .to_owned();
                        let repl = self
                            .nodes
                            .replace_field
                            .and_then(|f| self.ui.text(f).ok())
                            .unwrap_or_default()
                            .to_owned();
                        self.ask = None;
                        let hit = self.find_next(&find, &repl);
                        self.message = Some(Label::key_with("doc.find.count", &[("n", &hit.to_string())]));
                        self.rebuild()?;
                    }
                    None => {}
                }
                continue;
            }
            if self.ask.is_some() {
                continue;
            }
            if let Some((&cmd, _)) = self.cmds.iter().find(|(_, &id)| id == a.node) {
                self.do_cmd(cmd)?;
            }
        }
        Ok(())
    }
}

fn cmd_key(cmd: Cmd) -> &'static str {
    use Cmd::*;
    match cmd {
        New => "doc.new",
        Save => "doc.save",
        SaveAs => "doc.saveas",
        Undo => "doc.undo",
        Redo => "doc.redo",
        Bold => "doc.bold",
        Italic => "doc.italic",
        Underline => "doc.underline",
        Strike => "doc.strike",
        Title => "doc.style.title",
        H1 => "doc.style.h1",
        H2 => "doc.style.h2",
        H3 => "doc.style.h3",
        Normal => "doc.style.normal",
        AlignStart => "doc.align.start",
        AlignCenter => "doc.align.center",
        AlignEnd => "doc.align.end",
        AlignJustify => "doc.align.justify",
        DirRtl => "doc.dir.rtl",
        DirLtr => "doc.dir.ltr",
        Bullet => "doc.list.bullet",
        Number => "doc.list.number",
        Indent => "doc.indent",
        Outdent => "doc.outdent",
        Table => "doc.table",
        RowAdd => "doc.table.row_add",
        RowDel => "doc.table.row_del",
        ColAdd => "doc.table.col_add",
        ColDel => "doc.table.col_del",
        ZoomIn => "doc.zoom.in",
        ZoomOut => "doc.zoom.out",
        PageToggle => "doc.page",
        Find => "doc.find",
    }
}

/// A naive character-based search; returns the character offset of the match.
fn find_in(hay: &str, needle: &str, from_ch: usize) -> Option<usize> {
    let hay: Vec<char> = hay.chars().collect();
    let nee: Vec<char> = needle.chars().collect();
    if nee.is_empty() || nee.len() > hay.len() {
        return None;
    }
    for i in from_ch..=(hay.len().saturating_sub(nee.len())) {
        if hay[i..i + nee.len()] == nee[..] {
            return Some(i);
        }
    }
    None
}