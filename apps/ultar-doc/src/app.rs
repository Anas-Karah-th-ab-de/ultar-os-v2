//! The word processor window: a tool bar (three layout classes), the document drawn by
//! [`Artist`] over a [`Layout`] (paper pages or a flowing column), a status line (name, page,
//! words, zoom, message) and the dialogs of [`crate::dialogs`]. The window around the
//! document follows the theme; the paper stays white.

use crate::action::{AgentResult, Out};
use crate::commands::{self, Class, Cmd};
use crate::dialogs::{Ask, Nodes};
use crate::layout::{build, Geo, Layout, ParaCache};
use crate::paint::{paper_x, Artist, Marks}; // physical
use crate::place;
use crate::pointer::PointerState;
use crate::recovery::{self, Timer};
use crate::search::Find;
use crate::state::{Doc, DocError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ultar_office::doc::Block;
use ultar_ui::event::{Action, ActionKind};
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Label, PixelArea, Text};

/// What the window remembers so that it need not do work twice: the laid out paragraphs, the
/// key of the layout, the formatting at the caret, the word count and the view mode.
pub(crate) struct Memo {
    pub(crate) cache: ParaCache,
    pub(crate) lay_key: Option<(u64, Geo)>,
    pub(crate) fmt_key: u64,
    pub(crate) words: (u64, usize),
    pub(crate) auto_view: bool,
}

impl Default for Memo {
    fn default() -> Memo {
        Memo {
            cache: ParaCache::default(),
            lay_key: None,
            fmt_key: 0,
            words: (u64::MAX, 0),
            auto_view: true,
        }
    }
}

/// What goes out of the window and what waits to be sent: events for the shell, the
/// assistant's answers, the autosave timer and the app's own clipboard.
#[derive(Default)]
pub(crate) struct Io {
    pub(crate) timer: Timer,
    pub(crate) out: Vec<Out>,
    pub(crate) agent_results: Vec<AgentResult>,
    pub(crate) rich: Option<(String, Vec<Block>)>,
}

/// The state of the pointer and of vertical caret movement.
#[derive(Default)]
pub(crate) struct Inp {
    pub(crate) goal_x: Option<f32>,
    pub(crate) pointer_state: PointerState,
}

/// The word processor: its tree, its document, its layout and its pictures.
pub struct App {
    pub ui: Ui,
    pub doc: Doc,
    pub(crate) artist: Artist,
    pub lay: Option<Layout>,
    pub scroll: (f32, f32),
    pub areas: PathBuf,
    pub(crate) open_uri: Option<String>,
    pub nodes: Nodes,
    cmds: HashMap<Cmd, NodeId>,
    pub ask: Option<Ask>,
    pub message: Option<Label>,
    pub find: Find,
    pub(crate) memo: Memo,
    pub(crate) io: Io,
    pub(crate) inp: Inp,
}

impl App {
    /// A word processor over `ui` for `doc`; `areas` is where the user's places are bound and
    /// `open_uri` the `files://` address `doc` was opened from.
    pub fn new(ui: Ui, doc: Doc, areas: PathBuf, open_uri: Option<String>) -> Result<App, String> {
        let artist = Artist::new().map_err(|e| format!("fonts: {e:?}"))?;
        let mut app = App {
            ui,
            doc,
            artist,
            lay: None,
            scroll: (0.0, 0.0),
            areas,
            open_uri,
            nodes: Nodes::default(),
            cmds: HashMap::new(),
            ask: None,
            message: None,
            find: Find::default(),
            memo: Memo::default(),
            io: Io::default(),
            inp: Inp::default(),
        };
        app.first_question();
        app.rebuild().map_err(|e| format!("{e:?}"))?;
        let title = app.title();
        app.io.out.push(Out::Title(title));
        Ok(app)
    }

    /// Opens `uri` (a `files://` URI) or starts a new document; the error says why not.
    pub fn open(uri: Option<&str>, areas: &Path) -> (Doc, Option<DocError>) {
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

    /// The first question of a new window: a crashed session's copy, else the warning about
    /// parts of the file that cannot be kept.
    fn first_question(&mut self) {
        if let Some(copy) = recovery::pending(self.doc.path.as_deref(), &self.areas) {
            self.ask = Some(Ask::Recover { copy });
        } else if !self.doc.lossy.is_empty() {
            self.ask = Some(Ask::Lossy);
        }
    }

    /// Says in the status line why the file did not open.
    pub fn open_failed(&mut self, e: DocError) -> Result<(), UiError> {
        let why = self.ui.translator().t(e.key()).into_owned();
        self.say(Label::key_with("doc.open_failed", &[("reason", &why)]))
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
        let dot = if self.doc.dirty() { "\u{2022} " } else { "" };
        format!("{dot}{name} \u{2013} {}", tr.t("app.doc.name"))
    }

    pub(crate) fn touch_title(&mut self) {
        let title = self.title();
        self.io.out.push(Out::Title(title));
    }

    /// What the window has to do (taken once).
    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.io.out)
    }

    /// The `files://` address of the open document without a place.
    pub fn open_uri(&self) -> Option<&str> {
        self.open_uri.as_deref()
    }

    /// The address for the device mesh: the document and the block the caret is in
    /// (`files://<area>/<file>#b<block>`).
    pub fn mesh_uri(&self) -> Option<String> {
        let base = match (&self.doc.path, &self.open_uri) {
            (Some(p), _) => place::uri_of(p, &self.areas)?,
            (None, Some(u)) => u.split('#').next()?.to_owned(),
            _ => return None,
        };
        Some(format!("{base}#b{}", self.doc.cursor.block))
    }

    /// The tool bar button of a command (none when this layout class does not show it).
    pub fn command_node(&self, cmd: Cmd) -> Option<NodeId> {
        self.cmds.get(&cmd).copied()
    }

    /// The text has the keyboard (the window routes keys to it only then).
    pub fn text_focused(&self) -> bool {
        self.ask.is_none() && self.ui.focused().is_some() && self.ui.focused() == self.nodes.text
    }

    // ---- the tree ----------------------------------------------------------

    /// Builds the whole tree (also after a size change).
    pub fn rebuild(&mut self) -> Result<(), UiError> {
        self.nodes = Nodes::default();
        let class = Class::of(self.ui.size().w);
        if self.memo.auto_view && self.ui.size().w > 8.0 {
            self.memo.auto_view = false;
            self.doc.page_view = class != Class::Compact;
        }
        self.cmds.clear();
        self.ui.clear();
        self.ui
            .set_root_flex(Flex::column().cross(CrossAlign::Stretch));
        let root = self.ui.root();
        let bar = self.ui.add(
            root,
            Flex::row()
                .gap(Space::S4)
                .padding(Padding::all(Space::S8))
                .cross(CrossAlign::Center)
                .wrap(),
        )?;
        for cmd in commands::shown(class) {
            let on = self.doc.is_on(cmd);
            let mut b = Button::new(Label::key(commands::key(cmd))).toggled(on);
            if cmd == Cmd::Save {
                b = b.variant(ButtonVariant::Primary);
            } else {
                b = b.variant(ButtonVariant::Plain);
            }
            let id = self.ui.add(bar, b)?;
            self.cmds.insert(cmd, id);
        }
        let text = self.ui.add_sized(
            root,
            PixelArea::new(Label::key("doc.text")),
            Sizing::grow(1),
        )?;
        self.nodes.text = Some(text);
        let row = self.ui.add(
            root,
            Flex::row()
                .padding(Padding::all(Space::S4))
                .main(MainAlign::Start),
        )?;
        self.nodes.status = Some(
            self.ui
                .add(row, Text::new(Label::data("")).color(ColorRole::TextMuted))?,
        );
        self.build_dialog()?;
        if self.ask.is_none() {
            let _ = self.ui.focus(text);
        }
        self.memo.fmt_key = self.doc.format_key();
        self.buttons()?;
        self.memo.lay_key = None;
        self.status()?;
        self.draw()
    }

    fn buttons(&mut self) -> Result<(), UiError> {
        for (cmd, off) in [
            (Cmd::Undo, !self.doc.can_undo()),
            (Cmd::Redo, !self.doc.can_redo()),
        ] {
            if let Some(&n) = self.cmds.get(&cmd) {
                self.ui.set_disabled(n, off)?;
            }
        }
        Ok(())
    }

    pub(crate) fn status(&mut self) -> Result<(), UiError> {
        let Some(node) = self.nodes.status else {
            return Ok(());
        };
        if self.memo.words.0 != self.doc.revision() {
            self.memo.words = (self.doc.revision(), self.doc.document.word_count());
        }
        let page = self
            .lay
            .as_ref()
            .and_then(|l| l.caret(&self.doc.cursor))
            .map_or(1, |c| c.page + 1);
        let pages = self.lay.as_ref().map_or(1, |l| l.pages.len());
        let tr = self.ui.translator();
        let name = self
            .doc
            .name()
            .unwrap_or_else(|| tr.t("doc.untitled").into_owned());
        let zoom = (self.doc.zoom * 100.0).round() as u32;
        let (w, z, p, n) = (
            self.memo.words.1.to_string(),
            zoom.to_string(),
            page.to_string(),
            pages.to_string(),
        );
        let mut text = Label::key_with(
            "doc.status",
            &[
                ("name", &name),
                ("page", &p),
                ("pages", &n),
                ("words", &w),
                ("zoom", &z),
            ],
        )
        .resolve(tr)
        .into_owned();
        if self.doc.dirty() {
            text.push(' ');
            text.push_str(&tr.t("doc.dirty"));
        }
        if let Some(m) = &self.message {
            text.push_str("  \u{b7}  ");
            text.push_str(&m.resolve(tr));
        }
        self.ui.set_label(node, Label::data(&text))
    }

    // ---- layout and drawing ------------------------------------------------

    /// The size of the document area in device pixels.
    pub(crate) fn view_size(&mut self) -> (f32, f32) {
        self.ui.layout();
        let s = self.ui.env().scale;
        self.nodes
            .text
            .and_then(|n| self.ui.rect(n))
            .map_or((800.0, 600.0), |r| {
                ((r.w * s).round().max(1.0), (r.h * s).round().max(1.0))
            })
    }

    pub(crate) fn lay_line_h(&self) -> f32 {
        self.lay
            .as_ref()
            .and_then(|l| l.placed.first())
            .map_or(20.0, |p| p.line().h)
    }

    /// Lays the document out again if it, the zoom or the width changed.
    pub(crate) fn ensure_layout(&mut self) {
        let (w, _) = self.view_size();
        let geo = Geo {
            page_view: self.doc.page_view,
            view_w: w,
            scale: self.ui.env().scale,
            zoom: self.doc.zoom,
        };
        let key = (self.doc.revision(), geo);
        if self.lay.is_some() && self.memo.lay_key == Some(key) {
            return;
        }
        self.lay = Some(build(
            &self.doc.document,
            &geo,
            &mut self.artist.eng,
            &mut self.memo.cache,
        ));
        self.memo.lay_key = Some(key);
    }

    pub(crate) fn clamp_scroll(&mut self) {
        let (w, h) = self.view_size();
        let Some(lay) = &self.lay else {
            return;
        };
        let max_y = (lay.height - h).max(0.0);
        let max_x = (lay.paper_w - w).max(0.0);
        self.scroll = (
            self.scroll.0.clamp(0.0, max_x),
            self.scroll.1.clamp(0.0, max_y),
        );
    }

    /// Scrolls so that the caret is in view.
    pub(crate) fn reveal(&mut self) {
        self.ensure_layout();
        let (w, h) = self.view_size();
        let Some(c) = self.lay.as_ref().and_then(|l| l.caret(&self.doc.cursor)) else {
            return;
        };
        let margin = (self.lay_line_h() * 1.5).min(h / 4.0);
        if c.y < self.scroll.1 + margin {
            self.scroll.1 = c.y - margin;
        } else if c.y + c.h > self.scroll.1 + h - margin {
            self.scroll.1 = c.y + c.h - h + margin;
        }
        if c.x < self.scroll.0 + 8.0 {
            self.scroll.0 = c.x - 8.0;
        } else if c.x > self.scroll.0 + w - 8.0 {
            self.scroll.0 = c.x - w + 8.0;
        }
        self.clamp_scroll();
    }

    fn marks(&self, lay: &Layout) -> Marks {
        let mut m = Marks {
            caret: lay.caret(&self.doc.cursor),
            ..Marks::default()
        };
        if let Some((a, b)) = self.doc.selection() {
            m.selection = lay.selection_rects(&a, &b);
        }
        if self.ask == Some(Ask::Find) {
            for (i, (a, b, current)) in self.find.marks().take(400).enumerate() {
                let rects = lay.selection_rects(&a, &b);
                if current {
                    m.current = rects.first().copied();
                } else {
                    m.found.extend(rects);
                }
                let _ = i;
            }
        }
        m
    }

    /// Draws the visible part of the document into the picture.
    pub fn draw(&mut self) -> Result<(), UiError> {
        let Some(node) = self.nodes.text else {
            return Ok(());
        };
        self.ensure_layout();
        self.clamp_scroll();
        let (w, h) = self.view_size();
        let Some(lay) = self.lay.take() else {
            return Ok(());
        };
        let marks = self.marks(&lay);
        let label = self.ui.translator().t("doc.picture.shown").into_owned();
        let frame = self.artist.render(
            &self.doc.document,
            &lay,
            (w as u32, h as u32),
            self.scroll,
            self.ui.env(),
            &marks,
            &label,
        );
        self.lay = Some(lay);
        self.ui
            .set_picture(node, frame.map_err(|_| UiError::InvalidSize)?)
    }

    /// The paper's left edge in the view (for tests and hit testing).
    pub fn paper_x(&mut self) -> f32 {
        self.ensure_layout();
        let (w, _) = self.view_size();
        self.lay
            .as_ref()
            .map_or(0.0, |l| paper_x(l, w, self.scroll.0)) // physical
    }

    // ---- after changes -----------------------------------------------------

    /// The document changed: title, tool bar state, caret in view, redraw.
    pub(crate) fn changed(&mut self) -> Result<(), UiError> {
        self.message = None;
        self.touch_title();
        if self.ask == Some(Ask::Find) || !self.find.hits.is_empty() {
            self.find.run(&self.doc);
        }
        self.moved()
    }

    /// The caret was moved from outside (a hand-off to a place): scroll to it and redraw.
    pub fn show_caret(&mut self) -> Result<(), UiError> {
        self.moved()
    }

    /// The caret moved (or the document changed).
    pub(crate) fn moved(&mut self) -> Result<(), UiError> {
        self.ensure_layout();
        self.reveal();
        if self.ask.is_none() && self.memo.fmt_key != self.doc.format_key() {
            return self.rebuild();
        }
        self.buttons()?;
        self.status()?;
        self.draw()
    }

    pub(crate) fn zoom(&mut self, factor: f32) {
        self.doc.zoom =
            (self.doc.zoom * factor).clamp(crate::state::ZOOM_MIN, crate::state::ZOOM_MAX);
        self.memo.lay_key = None;
    }

    // ---- files -------------------------------------------------------------

    /// Puts `doc` in the window (a new document, or one the assistant opened).
    pub(crate) fn install(&mut self, doc: Doc, uri: Option<String>) -> Result<(), UiError> {
        recovery::remove(self.doc.path.as_deref(), &self.areas);
        self.doc = doc;
        self.open_uri = uri;
        self.io.timer.reset();
        self.scroll = (0.0, 0.0);
        self.message = None;
        self.find = Find::default();
        self.memo.auto_view = true;
        self.first_question();
        self.touch_title();
        self.rebuild()
    }

    pub(crate) fn new_doc(&mut self) -> Result<(), UiError> {
        if self.doc.dirty() {
            return self.ask_for(Ask::Unsaved { then_close: false });
        }
        self.reset_doc()
    }

    pub(crate) fn reset_doc(&mut self) -> Result<(), UiError> {
        self.install(Doc::new(), None)
    }

    pub(crate) fn save(&mut self, then_close: bool) -> Result<(), UiError> {
        if !self.doc.saves_in_place() {
            return self.ask_for(Ask::Name {
                then_close,
                purpose: crate::dialogs::Purpose::Save,
            });
        }
        let name = self.doc.name().unwrap_or_default();
        let kept = !self.doc.lossy.is_empty();
        match self.doc.save() {
            Ok(()) => {
                recovery::remove(self.doc.path.as_deref(), &self.areas);
                self.io.timer.reset();
                self.message = Some(if kept {
                    Label::key_with("doc.saved_kept", &[("name", &name)])
                } else {
                    Label::key_with("doc.saved", &[("name", &name)])
                });
                self.touch_title();
                if then_close {
                    self.io.out.push(Out::Close);
                }
            }
            Err(e) => self.message = Some(self.reason("doc.save_failed", &name, e)),
        }
        self.rebuild()
    }

    pub(crate) fn reason(&self, key: &'static str, name: &str, e: DocError) -> Label {
        let why = self.ui.translator().t(e.key()).into_owned();
        Label::key_with(key, &[("name", name), ("reason", &why)])
    }

    /// The window was asked to close.
    pub fn close_requested(&mut self) -> Result<(), UiError> {
        if !self.doc.dirty() {
            recovery::remove(self.doc.path.as_deref(), &self.areas);
            self.io.out.push(Out::Close);
            return Ok(());
        }
        if !matches!(self.ask, Some(Ask::Unsaved { .. })) {
            self.ask_for(Ask::Unsaved { then_close: true })?;
        }
        Ok(())
    }

    /// Time passes (`now` from any steady clock): writes the crash-recovery copy of a
    /// changed document every [`recovery::INTERVAL`].
    pub fn tick(&mut self, now: Duration) {
        if self.io.timer.due(&self.doc, now) {
            let _ = recovery::write(&self.doc, &self.areas);
        }
    }

    // ---- the widgets -------------------------------------------------------

    /// The actions of the widget tree.
    pub fn actions(&mut self, actions: Vec<Action>) -> Result<(), UiError> {
        for a in actions {
            if self.ask.is_some() {
                self.dialog_action(&a)?;
                continue;
            }
            if !matches!(a.kind, ActionKind::Activated | ActionKind::Submitted) {
                continue;
            }
            if let Some((&cmd, _)) = self.cmds.iter().find(|(_, &id)| id == a.node) {
                self.do_cmd(cmd)?;
            }
        }
        Ok(())
    }
}
