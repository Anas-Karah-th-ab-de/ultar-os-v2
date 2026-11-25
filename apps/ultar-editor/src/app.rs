//! The editor window: a tool bar (New, Save, Undo, Redo, the document's name), the text as a
//! picture drawn by [`TextView`], a status line (line and column, encoding, line ends, the
//! last message) and two dialogs: "save changes?" before closing, and the name of a new
//! document.

use crate::doc::{Doc, DocError};
use crate::keys::{self, Effect};
use crate::place;
use std::path::PathBuf;
use ultar_textview::{Marks, Pos, Source, TextView, ViewError};
use ultar_ui::event::{Action, ActionKind, Event, PointerButton};
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Dialog, Label, PixelArea, Text, TextField};

/// What the window has to do for the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// Offer this text on the clipboard.
    Copy(String),
    /// Ask for the clipboard text (it comes back through [`Editor::pasted`]).
    Paste,
    /// Show this title.
    Title(String),
    /// The window may close now.
    Close,
}

/// An open question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// Unsaved changes; `then_close`: the window closes after the answer.
    Unsaved { then_close: bool },
    /// The name of a new document; `then_close` as above.
    Name { then_close: bool },
}

#[derive(Default)]
struct Nodes {
    text: Option<NodeId>,
    status: Option<NodeId>,
    new: Option<NodeId>,
    save: Option<NodeId>,
    undo: Option<NodeId>,
    redo: Option<NodeId>,
    title: Option<NodeId>,
    dialog: Option<NodeId>,
    name_field: Option<NodeId>,
    ask_save: Option<NodeId>,
    ask_discard: Option<NodeId>,
    ask_cancel: Option<NodeId>,
}

/// The editor: its tree, its document and its text view.
pub struct Editor {
    /// The widget tree (the window draws it).
    pub ui: Ui,
    /// The open document.
    pub doc: Doc,
    view: TextView,
    areas: PathBuf,
    nodes: Nodes,
    ask: Option<Ask>,
    message: Option<Label>,
    selecting: bool,
    out: Vec<Out>,
}

impl Editor {
    /// An editor over `ui` for `doc`; `areas` is where the user's places are bound.
    pub fn new(ui: Ui, doc: Doc, areas: PathBuf) -> Result<Editor, ViewError> {
        let mut view = TextView::new(*ui.env(), true)?;
        view.numbers = true;
        let mut e = Editor {
            ui,
            doc,
            view,
            areas,
            nodes: Nodes::default(),
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

    /// Opens `uri` (a `files://` URI) or starts a new document; a file that cannot be read
    /// gives a new document and the reason (show it with [`Editor::open_failed`]).
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
        let message = if e == DocError::TooBig {
            Label::key_with(
                "editor.too_big",
                &[("mb", &(crate::doc::MAX_BYTES >> 20).to_string())],
            )
        } else {
            self.reason("editor.open_failed", "", e)
        };
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
            .unwrap_or_else(|| tr.t("editor.untitled").into_owned());
        let dot = if self.doc.dirty() { "• " } else { "" };
        format!("{dot}{name} – {}", tr.t("app.editor.name"))
    }

    /// What the window has to do (taken once).
    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// Builds the whole tree (also after a size change).
    pub fn rebuild(&mut self) -> Result<(), UiError> {
        let ui = &mut self.ui;
        ui.clear();
        ui.set_root_flex(Flex::column().cross(CrossAlign::Stretch));
        let root = ui.root();
        let bar = ui.add(
            root,
            Flex::row()
                .gap(Space::S8)
                .padding(Padding::all(Space::S8))
                .cross(CrossAlign::Center),
        )?;
        let plain = |key| Button::new(Label::key(key)).variant(ButtonVariant::Plain);
        self.nodes.new = Some(ui.add(bar, plain("editor.new"))?);
        self.nodes.save = Some(ui.add(bar, Button::new(Label::key("editor.save")))?);
        self.nodes.undo = Some(ui.add(bar, plain("editor.undo"))?);
        self.nodes.redo = Some(ui.add(bar, plain("editor.redo"))?);
        let name = self
            .doc
            .name()
            .map_or_else(|| Label::key("editor.untitled"), |n| Label::data(&n));
        self.nodes.title = Some(ui.add_sized(
            bar,
            Text::new(name).color(ColorRole::TextMuted),
            Sizing::grow(1),
        )?);
        let text = ui.add_sized(
            root,
            PixelArea::new(Label::key("editor.text")),
            Sizing::grow(1),
        )?;
        self.nodes.text = Some(text);
        self.nodes.status = Some(
            ui.add(
                root,
                Flex::row()
                    .padding(Padding::all(Space::S4))
                    .main(MainAlign::Start),
            )?,
        );
        self.nodes.status = Some(ui.add(
            self.nodes.status.unwrap_or(root),
            Text::new(Label::data("")).color(ColorRole::TextMuted),
        )?);
        self.build_dialog()?;
        if self.ask.is_none() {
            let _ = self.ui.focus(text);
        }
        self.buttons()?;
        self.status()?;
        self.draw_text()
    }

    fn build_dialog(&mut self) -> Result<(), UiError> {
        let Some(ask) = self.ask else {
            return Ok(());
        };
        let root = self.ui.root();
        let name = self
            .doc
            .name()
            .unwrap_or_else(|| self.ui.translator().t("editor.untitled").into_owned());
        let (title, ok_key) = match ask {
            Ask::Unsaved { .. } => (
                Label::key_with("editor.ask.title", &[("name", &name)]),
                "editor.ask.save",
            ),
            Ask::Name { .. } => (Label::key("editor.name.title"), "editor.name.ok"),
        };
        let dialog = self.ui.add(root, Dialog::new(title))?;
        self.nodes.dialog = Some(dialog);
        match ask {
            Ask::Unsaved { .. } => {
                self.ui.add(
                    dialog,
                    Text::new(Label::key("editor.ask.message")).color(ColorRole::TextMuted),
                )?;
            }
            Ask::Name { .. } => {
                let field = TextField::new(Label::key("editor.name.field")).labelled();
                self.nodes.name_field = Some(self.ui.add(dialog, field)?);
            }
        }
        let row = self
            .ui
            .add(dialog, Flex::row().gap(Space::S8).main(MainAlign::End))?;
        let cancel = self.ui.add(
            row,
            Button::new(Label::key("editor.ask.cancel")).variant(ButtonVariant::Plain),
        )?;
        self.nodes.ask_cancel = Some(cancel);
        if matches!(ask, Ask::Unsaved { .. }) {
            self.nodes.ask_discard = Some(self.ui.add(
                row,
                Button::new(Label::key("editor.ask.discard")).variant(ButtonVariant::Danger),
            )?);
        }
        let ok = self.ui.add(row, Button::new(Label::key(ok_key)))?;
        self.nodes.ask_save = Some(ok);
        self.ui.set_default_action(dialog, Some(ok))?;
        self.ui.open_dialog(dialog)?;
        if let Some(field) = self.nodes.name_field {
            self.ui.focus(field)?;
        }
        Ok(())
    }

    fn buttons(&mut self) -> Result<(), UiError> {
        let b = &self.doc.buffer;
        let states = [
            (self.nodes.undo, !b.can_undo()),
            (self.nodes.redo, !b.can_redo()),
        ];
        for (node, off) in states {
            if let Some(n) = node {
                self.ui.set_disabled(n, off)?;
            }
        }
        Ok(())
    }

    fn status(&mut self) -> Result<(), UiError> {
        let Some(node) = self.nodes.status else {
            return Ok(());
        };
        let (line, col) = self.doc.line_col();
        let tr = self.ui.translator();
        let place = Label::key_with(
            "editor.status",
            &[("line", &line.to_string()), ("col", &col.to_string())],
        );
        let place = place.resolve(tr).into_owned();
        let b = &self.doc.buffer;
        let mut text = format!("{place}  ·  {}  ·  {}", b.enc.label(), b.eol.label());
        if let Some(m) = &self.message {
            text = format!("{text}  ·  {}", m.resolve(tr));
        }
        self.ui.set_label(node, Label::data(&text))
    }

    /// Pixel size of the text picture (after layout).
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
        let marks = Marks {
            caret: Some(self.doc.caret()),
            selection: self.doc.selection(),
        };
        let frame = self
            .view
            .render(w, h, &self.doc, &marks)
            .map_err(|_| UiError::InvalidSize)?;
        self.ui.set_picture(node, frame)
    }

    fn rows(&mut self) -> usize {
        let h = self.text_size().map_or(1, |s| s.1);
        self.view.rows(h)
    }

    fn after(&mut self, effect: Effect) -> Result<(), UiError> {
        match effect {
            Effect::None => return Ok(()),
            Effect::Copy(text) => self.out.push(Out::Copy(text)),
            Effect::Paste => self.out.push(Out::Paste),
            Effect::Save => return self.save(false),
            Effect::New => return self.new_doc(),
            Effect::Page(down) => {
                let rows = self.rows() as isize;
                let step = if down { rows - 1 } else { 1 - rows };
                let total = self.doc.lines();
                self.view.scroll(step, total, rows as usize);
                let caret = self.doc.caret();
                let line = (caret.line as isize + step).clamp(0, total as isize - 1) as usize;
                self.doc.buffer.anchor = None;
                self.doc.buffer.cur = self.doc.offset_of(Pos::new(line, caret.byte));
            }
            Effect::Moved => {}
            Effect::Changed => {
                self.message = None;
                let title = self.title();
                self.out.push(Out::Title(title));
            }
        }
        let rows = self.rows();
        self.view.reveal(self.doc.caret().line, rows);
        self.buttons()?;
        self.status()?;
        self.draw_text()
    }

    /// A key the widgets did not use (with the text focused, every key comes here).
    pub fn key(&mut self, event: Event) -> Result<(), UiError> {
        if self.ask.is_some() {
            return Ok(());
        }
        let Event::Key { key, modifiers } = event else {
            return Ok(());
        };
        let line = self.doc.caret().line;
        let rtl = self.view.is_rtl(&self.doc, line);
        let effect = keys::apply(&mut self.doc, key, modifiers, rtl);
        self.after(effect)
    }

    /// Clipboard text that arrived.
    pub fn pasted(&mut self, text: &str) -> Result<(), UiError> {
        let effect = keys::paste(&mut self.doc, text);
        self.after(effect)
    }

    /// `true` while the text has the keyboard (keys then bypass the widgets).
    pub fn text_focused(&self) -> bool {
        self.ask.is_none() && self.ui.focused().is_some() && self.ui.focused() == self.nodes.text
    }

    /// A pointer event (the widgets saw it too): caret, selection and scrolling in the text.
    pub fn pointer(&mut self, event: Event) -> Result<(), UiError> {
        if self.ask.is_some() {
            return Ok(());
        }
        let Some(node) = self.nodes.text else {
            return Ok(());
        };
        let (pos, down) = match event {
            Event::PointerDown {
                pos,
                button: PointerButton::Primary,
            } => (pos, true),
            Event::PointerMove { pos } if self.selecting => (pos, false),
            Event::PointerUp { .. } => {
                self.selecting = false;
                return Ok(());
            }
            Event::Scroll { pos, lines } => {
                if self.ui.rect(node).is_some_and(|r| r.contains(pos)) {
                    let rows = self.rows();
                    let total = self.doc.lines();
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
        let at = self
            .view
            .hit(width, (pos.x - rect.x) * s, (pos.y - rect.y) * s, &self.doc);
        let offset = self.doc.offset_of(at);
        let b = &mut self.doc.buffer;
        if down {
            b.anchor = Some(offset);
            self.selecting = true;
            let _ = self.ui.focus(node);
        }
        b.cur = offset;
        self.after(Effect::Moved)
    }

    /// The window was asked to close: `Out::Close` now, or after the "save changes?" answer.
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
                self.message = Some(Label::key_with("editor.saved", &[("name", &name)]));
                let title = self.title();
                self.out.push(Out::Title(title));
                if then_close {
                    self.out.push(Out::Close);
                }
            }
            Err(e) => self.message = Some(self.reason("editor.save_failed", &name, e)),
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
                self.message = Some(Label::key_with("editor.saved", &[("name", &shown)]));
                let title = self.title();
                self.out.push(Out::Title(title));
                if then_close {
                    self.out.push(Out::Close);
                }
            }
            Err(e) => self.message = Some(self.reason("editor.save_failed", &name, e)),
        }
        self.rebuild()
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
            if node == n.save && self.ask.is_none() {
                self.save(false)?;
            } else if node == n.new {
                self.after(Effect::New)?;
            } else if node == n.undo {
                let did = self.doc.buffer.undo();
                self.doc.reindex();
                self.after(if did { Effect::Changed } else { Effect::None })?;
            } else if node == n.redo {
                let did = self.doc.buffer.redo();
                self.doc.reindex();
                self.after(if did { Effect::Changed } else { Effect::None })?;
            } else if node == n.ask_cancel {
                self.ask = None;
                self.rebuild()?;
            } else if node == n.ask_discard {
                let close = matches!(self.ask, Some(Ask::Unsaved { then_close: true }));
                self.ask = None;
                if close {
                    self.out.push(Out::Close);
                } else {
                    self.doc.buffer.dirty = false;
                    self.reset_doc()?;
                }
            } else if node == n.ask_save || (node == n.name_field && self.ask.is_some()) {
                match self.ask {
                    Some(Ask::Unsaved { then_close }) => {
                        self.ask = None;
                        self.rebuild()?;
                        self.save(then_close)?;
                        if !then_close && !self.doc.dirty() && self.ask.is_none() {
                            self.reset_doc()?;
                        }
                    }
                    Some(Ask::Name { then_close }) => self.save_named(then_close)?,
                    None => {}
                }
            }
        }
        Ok(())
    }
}
