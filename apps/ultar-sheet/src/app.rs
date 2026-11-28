//! The sheet window: a tool bar, the formula bar, the grid, the find bar, the sheet tabs and
//! a status line, plus dialogs ([`crate::ask`]) and the states of a workbook that is not open
//! (loading, damaged, too large). Input handling is in [`crate::input`].

use crate::book::Book;
use crate::core::Core;
use crate::draw::Drawer;
use crate::file::{DocError, Opened};
use crate::recover::Recovery;
use std::path::PathBuf;
use std::time::Instant;
use ultar_ui::event::Modifiers;
use ultar_ui::layout::{CrossAlign, Flex};
use ultar_ui::tokens::{InputMode, SizeClass};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::Label;

pub use crate::cmd::Cmd;

/// What the window has to do for the sheet app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// Offer this text on the clipboard.
    Copy(String),
    /// Ask for the clipboard text (it comes back through [`SheetApp::pasted`]).
    Paste,
    /// Show this title.
    Title(String),
    /// The window may close now.
    Close,
    /// Read this file in the background (answer with [`SheetApp::loaded`]).
    Load(PathBuf),
}

/// Whether a workbook is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The workbook is open.
    Ready,
    /// A file is being read.
    Loading(String),
    /// The file could not be read as a workbook.
    Damaged(DocError),
    /// The file is too large.
    TooBig,
}

/// What the mesh is told: the open workbook, sheet and cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// `files://` URI of the workbook, with sheet and cell as fragment.
    pub uri: Option<String>,
    /// File name (or the name of a new workbook).
    pub name: String,
    /// Sheet name.
    pub sheet: String,
    /// Cell, like `B7`.
    pub cell: String,
}

#[derive(Default)]
pub(crate) struct Nodes {
    pub grid: Option<NodeId>,
    pub status: Option<NodeId>,
    pub name_box: Option<NodeId>,
    pub formula: Option<NodeId>,
    pub undo: Option<NodeId>,
    pub redo: Option<NodeId>,
    pub find: Option<NodeId>,
    pub with: Option<NodeId>,
    pub dialog_field: Option<NodeId>,
    pub dialog_ok: Option<NodeId>,
    pub dialog_cancel: Option<NodeId>,
    pub dialog_discard: Option<NodeId>,
}

/// The sheet window: tree, state and everything the window needs from the app.
pub struct SheetApp {
    /// The widget tree (the window draws it).
    pub ui: Ui,
    /// The workbook and the editing state.
    pub core: Core,
    pub(crate) drawer: Drawer,
    pub(crate) areas: PathBuf,
    pub(crate) nodes: Nodes,
    pub(crate) cmds: Vec<(NodeId, Cmd)>,
    pub(crate) ask: Option<crate::ask::Ask>,
    pub(crate) state: State,
    pub(crate) tools_open: bool,
    pub(crate) out: Vec<Out>,
    pub(crate) toggles: Vec<bool>,
    pub(crate) pointer: Pointer,
    pub(crate) upkeep: Upkeep,
    pub(crate) waiting: Waiting,
}

/// The mouse and the modifier keys.
pub(crate) struct Pointer {
    /// What a press started and a move continues.
    pub drag: crate::input::Drag,
    /// The modifier keys held now.
    pub mods: Modifiers,
    /// The last click (for a double click).
    pub last_click: Option<(Instant, (u32, u32))>,
}

/// What keeps the window's files and title up to date.
pub(crate) struct Upkeep {
    /// The copies for crash recovery.
    pub recovery: Recovery,
    /// When the last copy was written.
    pub last_copy: Instant,
    /// The workbook version the title shows.
    pub title_version: u64,
}

/// Things that wait for the next tree or the next answer.
#[derive(Default)]
pub(crate) struct Waiting {
    /// The answer to an assistant's request that waited for a question.
    pub agent_reply: Option<crate::agent::Reply>,
    /// The sheet and cell to go to when the file has been read.
    pub spot: Option<crate::place::Spot>,
    /// The find field gets the focus when the tree is built.
    pub focus_find: bool,
}

/// Seconds between two copies for crash recovery.
pub const AUTOSAVE_SECS: u64 = 20;

impl SheetApp {
    /// A window over `ui` for `book`; `areas` is where the user's places are bound,
    /// `recover` the folder of recovery copies.
    pub fn new(ui: Ui, book: Book, areas: PathBuf, recover: PathBuf) -> Result<SheetApp, String> {
        let drawer = Drawer::new(*ui.env()).map_err(|e| e.0)?;
        let recovery = Recovery::new(&recover, book.path.as_deref());
        let mut app = SheetApp {
            ui,
            core: Core::new(book),
            drawer,
            areas,
            nodes: Nodes::default(),
            cmds: Vec::new(),
            ask: None,
            state: State::Ready,
            tools_open: false,
            out: Vec::new(),
            toggles: Vec::new(),
            pointer: Pointer {
                drag: crate::input::Drag::None,
                mods: Modifiers::NONE,
                last_click: None,
            },
            upkeep: Upkeep {
                recovery,
                last_copy: Instant::now(),
                title_version: 0,
            },
            waiting: Waiting::default(),
        };
        app.rebuild().map_err(|e| format!("{e:?}"))?;
        let title = app.title();
        app.out.push(Out::Title(title));
        Ok(app)
    }

    /// What the window has to do (taken once).
    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// Text of a message key in the window's language.
    pub(crate) fn t(&self, key: &str) -> String {
        self.ui.translator().t(key).into_owned()
    }

    /// The window title: the workbook's name, with a dot while unsaved.
    pub fn title(&self) -> String {
        let name = self
            .core
            .book
            .name()
            .unwrap_or_else(|| self.t("sheet.untitled"));
        let dot = if self.core.book.dirty() { "• " } else { "" };
        format!("{dot}{name} – {}", self.t("app.sheet.name"))
    }

    /// What the mesh is told.
    pub fn report(&self) -> Report {
        let b = &self.core.book;
        let sheet = b.sheet().name.clone();
        let (r, c) = self.core.sel.active;
        let uri = b
            .path
            .as_deref()
            .and_then(|p| crate::place::uri_of(p, &self.areas))
            .map(|u| crate::place::with_spot(&u, &sheet, r, c));
        Report {
            uri,
            name: b.name().unwrap_or_else(|| self.t("sheet.untitled")),
            sheet,
            cell: ultar_office::sheet::cell_name(r, c),
        }
    }

    /// The saved file of the workbook (it travels with the activity to another device);
    /// `None` while there are unsaved changes, because the file would not show them.
    pub fn file_on_this_device(&self) -> Option<PathBuf> {
        let b = &self.core.book;
        b.path.clone().filter(|_| !b.dirty())
    }

    /// The node of a command's button (tests and the window).
    pub fn node_of(&self, cmd: Cmd) -> Option<NodeId> {
        self.cmds.iter().find(|(_, c)| *c == cmd).map(|(n, _)| *n)
    }

    /// The nodes of the open question: OK, cancel, "don't save" and the text field.
    pub fn dialog_nodes(&self) -> [Option<NodeId>; 4] {
        let n = &self.nodes;
        [
            n.dialog_ok,
            n.dialog_cancel,
            n.dialog_discard,
            n.dialog_field,
        ]
    }

    /// The open question.
    pub fn question(&self) -> Option<&crate::ask::Ask> {
        self.ask.as_ref()
    }

    /// The node of the formula bar.
    pub fn formula_node(&self) -> Option<NodeId> {
        self.nodes.formula
    }

    /// The node of the sheet picture.
    pub fn grid_node(&self) -> Option<NodeId> {
        self.nodes.grid
    }

    /// The window state.
    pub fn state(&self) -> &State {
        &self.state
    }

    /// A file is being read: show that.
    pub fn loading(&mut self, name: &str) -> Result<(), UiError> {
        self.state = State::Loading(name.to_owned());
        self.rebuild()
    }

    /// The background read ended.
    pub fn loaded(
        &mut self,
        path: PathBuf,
        result: Result<Opened, DocError>,
    ) -> Result<(), UiError> {
        match result {
            Ok(o) => {
                let book = Book::of(o.book, Some(path.clone()), o.format);
                self.core = Core::new(book);
                let dir = self.upkeep.recovery.dir().to_owned();
                self.upkeep.recovery = Recovery::new(&dir, Some(&path));
                self.state = State::Ready;
                self.ask = None;
                if let Some(spot) = self.waiting.spot.take() {
                    if let Some(i) = spot
                        .sheet
                        .as_deref()
                        .and_then(|n| self.core.book.wb.sheet_index(n))
                    {
                        self.core.show_sheet(i);
                    }
                    if let Some((r, c)) = spot.cell {
                        self.core.sel.select(r, c);
                    }
                }
                self.offer_recovery(&path);
            }
            Err(DocError::TooBig) => self.state = State::TooBig,
            Err(e) => self.state = State::Damaged(e),
        }
        self.upkeep.title_version = u64::MAX;
        self.rebuild()
    }

    /// Goes to the sheet and cell named by a URI fragment.
    pub fn go_to(&mut self, spot: &crate::place::Spot) -> Result<(), UiError> {
        if let Some(i) = spot
            .sheet
            .as_deref()
            .and_then(|n| self.core.book.wb.sheet_index(n))
        {
            self.core.show_sheet(i);
        }
        if let Some((r, c)) = spot.cell {
            self.core.sel.select(r, c);
        }
        self.refresh()
    }

    // ---------------------------------------------------------------- building

    /// Builds the whole tree (after a size change, a state change, a new sheet ...).
    pub fn rebuild(&mut self) -> Result<(), UiError> {
        self.cmds.clear();
        self.nodes = Nodes::default();
        self.ui.clear();
        self.ui
            .set_root_flex(Flex::column().cross(CrossAlign::Stretch));
        match self.state.clone() {
            State::Ready => self.build_main()?,
            other => self.build_state(&other)?,
        }
        self.build_dialog()?;
        if self.ask.is_none() {
            match (
                self.core.find.open && self.waiting.focus_find,
                self.nodes.find,
            ) {
                (true, Some(f)) => {
                    let _ = self.ui.focus(f);
                    self.waiting.focus_find = false;
                }
                _ if self.core.find.open => {}
                _ => self.focus_grid(),
            }
        }
        self.toggles = self.toggle_signature();
        self.refresh()
    }

    // ---------------------------------------------------------------- drawing

    /// Pixel size of the grid picture (after layout); also tells the core the size in points.
    fn grid_size(&mut self) -> Option<(u32, u32)> {
        self.ui.layout();
        let rect = self.ui.rect(self.nodes.grid?)?;
        self.core.view = (rect.w, rect.h);
        self.core.touch = self.ui.input_mode() == InputMode::Touch;
        let s = self.ui.env().scale;
        let (w, h) = ((rect.w * s).round(), (rect.h * s).round());
        (w >= 1.0 && h >= 1.0).then_some((w as u32, h as u32))
    }

    /// Draws the grid into its picture.
    pub fn draw_grid(&mut self) -> Result<(), UiError> {
        let (Some(node), Some(px)) = (self.nodes.grid, self.grid_size()) else {
            return Ok(());
        };
        self.drawer.env = *self.ui.env();
        let frame = self.core.frame();
        let hint = (self.core.book.sheet().cells.is_empty() && self.core.edit.is_none())
            .then(|| self.t("sheet.empty"));
        let canvas = self
            .drawer
            .draw(
                &mut self.core.book,
                &frame,
                &self.core.sel,
                self.core.edit.as_ref(),
                hint.as_deref(),
                px,
            )
            .map_err(|_| UiError::InvalidSize)?;
        self.ui.set_picture(node, canvas)
    }

    /// Brings everything on screen up to date after a change: scroll, bars, grid, title.
    pub fn refresh(&mut self) -> Result<(), UiError> {
        if self.state != State::Ready {
            return Ok(());
        }
        if self.toggle_signature() != self.toggles && self.ask.is_none() {
            return self.rebuild();
        }
        self.sync_bars()?;
        // the bars may have changed the size of the grid: scroll to the cell after that
        self.grid_size();
        self.core.reveal();
        self.draw_grid()?;
        let v = self.core.book.version;
        if v != self.upkeep.title_version {
            self.upkeep.title_version = v;
            let title = self.title();
            self.out.push(Out::Title(title));
        }
        Ok(())
    }

    fn sync_bars(&mut self) -> Result<(), UiError> {
        if let Some(n) = self.nodes.name_box {
            let name = self.core.cell_name();
            self.ui.set_label(n, Label::data(&name))?;
        }
        if let Some(f) = self.nodes.formula {
            let text = self
                .core
                .edit
                .as_ref()
                .map_or_else(|| self.core.raw(), |e| e.text.clone());
            if self.ui.text(f).map(|t| t != text).unwrap_or(false) {
                self.ui.set_text(f, &text)?;
            }
        }
        let (can_undo, can_redo) = (self.core.book.can_undo(), self.core.book.can_redo());
        for (n, ok) in [(self.nodes.undo, can_undo), (self.nodes.redo, can_redo)] {
            if let Some(n) = n {
                self.ui.set_disabled(n, !ok)?;
            }
        }
        if let Some(s) = self.nodes.status {
            let text = self.status_text();
            self.ui.set_label(s, Label::data(&text))?;
        }
        Ok(())
    }

    fn status_text(&mut self) -> String {
        let mut parts = Vec::new();
        if let Some(m) = &self.core.message {
            parts.push(m.resolve(self.ui.translator()).into_owned());
        } else if let Some((sum, avg, n)) = self.core.stats() {
            let tr = self.ui.translator();
            let fmt = |x: f64| ultar_office::sheet::fmt_num((x * 1e6).round() / 1e6);
            let line = Label::key_with(
                "sheet.stats",
                &[
                    ("sum", &fmt(sum)),
                    ("avg", &fmt(avg)),
                    ("n", &n.to_string()),
                ],
            );
            parts.push(line.resolve(tr).into_owned());
        }
        parts.join("  ·  ")
    }
}

impl SheetApp {
    /// The window asked to close: `Out::Close` now, or after the answer to "save changes?".
    pub fn close_requested(&mut self) -> Result<(), UiError> {
        if !self.core.book.dirty() {
            self.upkeep.recovery.clear();
            self.out.push(Out::Close);
            return Ok(());
        }
        if self.ask.is_none() {
            self.ask = Some(crate::ask::Ask::Unsaved(crate::ask::Then::Close));
            self.rebuild()?;
        }
        Ok(())
    }

    /// Gives the clock to the window: writes the copy for crash recovery now and then.
    pub fn tick(&mut self, now: Instant) {
        if now.duration_since(self.upkeep.last_copy).as_secs() >= AUTOSAVE_SECS {
            self.upkeep.last_copy = now;
            let path = self.core.book.path.clone();
            let _ = self.upkeep.recovery.save(&self.core.book, path.as_deref());
        }
    }

    /// The layout class of the window.
    pub fn class(&self) -> SizeClass {
        self.ui.size_class()
    }
}
