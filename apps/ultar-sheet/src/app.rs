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
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, InputMode, SizeClass, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, EmptyState, Label, PixelArea, Text, TextField};
use ultar_ui::icon::IconId;

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
    pub(crate) recovery: Recovery,
    pub(crate) last_copy: Instant,
    pub(crate) drag: crate::input::Drag,
    pub(crate) mods: Modifiers,
    pub(crate) last_click: Option<(Instant, (u32, u32))>,
    pub(crate) out: Vec<Out>,
    pub(crate) title_version: u64,
    pub(crate) toggles: Vec<bool>,
    pub(crate) agent_reply: Option<crate::agent::Reply>,
    pub(crate) pending_spot: Option<crate::place::Spot>,
    pub(crate) focus_find: bool,
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
            recovery,
            last_copy: Instant::now(),
            drag: crate::input::Drag::None,
            mods: Modifiers::NONE,
            last_click: None,
            out: Vec::new(),
            title_version: 0,
            toggles: Vec::new(),
            agent_reply: None,
            pending_spot: None,
            focus_find: false,
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
        let name = self.core.book.name().unwrap_or_else(|| self.t("sheet.untitled"));
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
    pub fn loaded(&mut self, path: PathBuf, result: Result<Opened, DocError>) -> Result<(), UiError> {
        match result {
            Ok(o) => {
                let book = Book::of(o.book, Some(path.clone()), o.format);
                self.core = Core::new(book);
                let dir = self.recovery.dir().to_owned();
                self.recovery = Recovery::new(&dir, Some(&path));
                self.state = State::Ready;
                self.ask = None;
                if let Some(spot) = self.pending_spot.take() {
                    if let Some(i) = spot.sheet.as_deref().and_then(|n| self.core.book.wb.sheet_index(n)) {
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
        self.title_version = u64::MAX;
        self.rebuild()
    }

    /// Goes to the sheet and cell named by a URI fragment.
    pub fn go_to(&mut self, spot: &crate::place::Spot) -> Result<(), UiError> {
        if let Some(i) = spot.sheet.as_deref().and_then(|n| self.core.book.wb.sheet_index(n)) {
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
        self.ui.set_root_flex(Flex::column().cross(CrossAlign::Stretch));
        match self.state.clone() {
            State::Ready => self.build_main()?,
            other => self.build_state(&other)?,
        }
        self.build_dialog()?;
        if self.ask.is_none() {
            match (self.core.find.open && self.focus_find, self.nodes.find) {
                (true, Some(f)) => {
                    let _ = self.ui.focus(f);
                    self.focus_find = false;
                }
                _ if self.core.find.open => {}
                _ => self.focus_grid(),
            }
        }
        self.toggles = self.toggle_signature();
        self.refresh()
    }

    fn build_state(&mut self, state: &State) -> Result<(), UiError> {
        let (icon, title, message) = match state {
            State::Loading(name) => (IconId::Info, Label::key("sheet.loading"), Label::data(name)),
            State::TooBig => (
                IconId::Warning,
                Label::key("sheet.too_big.title"),
                Label::key_with("sheet.too_big", &[("mb", &(crate::file::MAX_BYTES >> 20).to_string())]),
            ),
            State::Damaged(why) => {
                let reason = self.t(why.key());
                (
                    IconId::Error,
                    Label::key("sheet.damaged.title"),
                    Label::key_with("sheet.damaged", &[("reason", &reason)]),
                )
            }
            State::Ready => return Ok(()),
        };
        let root = self.ui.root();
        self.ui.add_sized(root, EmptyState::new(icon, title).message(message), Sizing::grow(1))?;
        if !matches!(state, State::Loading(_)) {
            let row = self.ui.add(root, Flex::row().gap(Space::S8).padding(Padding::all(Space::S12)).main(MainAlign::Center))?;
            self.button(row, Cmd::New, "sheet.new", false, ButtonVariant::Primary)?;
            self.button(row, Cmd::Open, "sheet.open", false, ButtonVariant::Plain)?;
        }
        Ok(())
    }

    pub(crate) fn button(
        &mut self,
        parent: NodeId,
        cmd: Cmd,
        key: &str,
        on: bool,
        variant: ButtonVariant,
    ) -> Result<NodeId, UiError> {
        let b = Button::new(Label::key(key)).variant(variant).toggled(on);
        let id = self.ui.add(parent, b)?;
        self.cmds.push((id, cmd));
        Ok(id)
    }

    fn plain(&mut self, parent: NodeId, cmd: Cmd, key: &str, on: bool) -> Result<NodeId, UiError> {
        self.button(parent, cmd, key, on, ButtonVariant::Plain)
    }

    fn bar(&mut self, parent: NodeId) -> Result<NodeId, UiError> {
        self.ui.add(
            parent,
            Flex::row().wrap().gap(Space::S4).padding(Padding::symmetric(Space::S8, Space::S4)).cross(CrossAlign::Center),
        )
    }

    fn build_main(&mut self) -> Result<(), UiError> {
        let root = self.ui.root();
        let compact = self.ui.size_class() == SizeClass::Compact;
        let st = self.core.book.sheet().style(self.core.sel.active.0, self.core.sel.active.1);
        let sh = self.core.book.sheet();
        let (frozen_r, frozen_c, rtl) = (sh.frozen_rows > 0, sh.frozen_cols > 0, sh.rtl);
        // first bar: the file and the history
        let bar = self.bar(root)?;
        self.button(bar, Cmd::Save, "sheet.save", false, ButtonVariant::Primary)?;
        self.nodes.undo = Some(self.plain(bar, Cmd::Undo, "sheet.undo", false)?);
        self.nodes.redo = Some(self.plain(bar, Cmd::Redo, "sheet.redo", false)?);
        self.plain(bar, Cmd::Bold, "sheet.bold", st.bold)?;
        if compact {
            let open = self.tools_open;
            self.plain(bar, Cmd::Tools, "sheet.tools", open)?;
        } else {
            self.plain(bar, Cmd::New, "sheet.new", false)?;
            self.plain(bar, Cmd::Open, "sheet.open", false)?;
            self.plain(bar, Cmd::SaveAs, "sheet.save_as", false)?;
            let name = self.core.book.name().map_or_else(|| Label::key("sheet.untitled"), |n| Label::data(&n));
            self.ui.add_sized(bar, Text::new(name).color(ColorRole::TextMuted), Sizing::grow(1))?;
        }
        if !compact || self.tools_open {
            let tools = self.bar(root)?;
            self.build_tools(tools, &st, (frozen_r, frozen_c, rtl), compact)?;
        }
        // formula bar
        let fbar = self.ui.add(
            root,
            Flex::row().gap(Space::S8).padding(Padding::symmetric(Space::S8, Space::S4)).cross(CrossAlign::Center),
        )?;
        self.nodes.name_box = Some(self.ui.add_sized(
            fbar,
            Text::new(Label::data(&self.core.cell_name())).color(ColorRole::TextMuted),
            Sizing::width(48.0),
        )?);
        let field = TextField::new(Label::key("sheet.formula")).placeholder(Label::key("sheet.formula.hint"));
        self.nodes.formula = Some(self.ui.add_sized(fbar, field, Sizing::grow(1))?);
        // grid
        self.nodes.grid = Some(self.ui.add_sized(root, PixelArea::new(Label::key("sheet.grid")), Sizing::grow(1))?);
        if self.core.find.open {
            self.build_find(root)?;
        }
        self.build_tabs(root)?;
        let status = self.ui.add(root, Flex::row().padding(Padding::symmetric(Space::S8, Space::S4)))?;
        self.nodes.status =
            Some(self.ui.add(status, Text::new(Label::data("")).color(ColorRole::TextMuted))?);
        Ok(())
    }

    fn build_tools(
        &mut self,
        bar: NodeId,
        st: &ultar_office::style::CellStyle,
        (frozen_r, frozen_c, rtl): (bool, bool, bool),
        compact: bool,
    ) -> Result<(), UiError> {
        use crate::ops::NumFmt;
        use ultar_office::style::Align;
        if compact {
            self.plain(bar, Cmd::New, "sheet.new", false)?;
            self.plain(bar, Cmd::Open, "sheet.open", false)?;
            self.plain(bar, Cmd::SaveAs, "sheet.save_as", false)?;
        }
        self.plain(bar, Cmd::Italic, "sheet.italic", st.italic)?;
        self.plain(bar, Cmd::Wrap, "sheet.wrap", st.wrap)?;
        self.plain(bar, Cmd::Align(Align::Start), "sheet.align_start", st.align == Align::Start)?;
        self.plain(bar, Cmd::Align(Align::Center), "sheet.align_center", st.align == Align::Center)?;
        self.plain(bar, Cmd::Align(Align::End), "sheet.align_end", st.align == Align::End)?;
        self.plain(bar, Cmd::Color, "sheet.color", st.color.is_some())?;
        self.plain(bar, Cmd::Fill, "sheet.fill", st.fill.is_some())?;
        for (f, key) in [
            (NumFmt::General, "sheet.fmt_general"),
            (NumFmt::Number, "sheet.fmt_number"),
            (NumFmt::Percent, "sheet.fmt_percent"),
            (NumFmt::Currency, "sheet.fmt_currency"),
            (NumFmt::Date, "sheet.fmt_date"),
        ] {
            self.plain(bar, Cmd::Fmt(f), key, st.num_fmt == f.code())?;
        }
        self.plain(bar, Cmd::FreezeRow, "sheet.freeze_row", frozen_r)?;
        self.plain(bar, Cmd::FreezeCol, "sheet.freeze_col", frozen_c)?;
        self.plain(bar, Cmd::Rtl, "sheet.rtl", rtl)?;
        self.plain(bar, Cmd::Sort(true), "sheet.sort_asc", false)?;
        self.plain(bar, Cmd::Sort(false), "sheet.sort_desc", false)?;
        self.plain(bar, Cmd::FillDown, "sheet.fill_down", false)?;
        self.plain(bar, Cmd::FillRight, "sheet.fill_right", false)?;
        self.plain(bar, Cmd::InsRow, "sheet.row_add", false)?;
        self.plain(bar, Cmd::DelRow, "sheet.row_del", false)?;
        self.plain(bar, Cmd::InsCol, "sheet.col_add", false)?;
        self.plain(bar, Cmd::DelCol, "sheet.col_del", false)?;
        self.plain(bar, Cmd::Find, "sheet.find", self.core.find.open)?;
        if self.ui.input_mode() == InputMode::Touch {
            let on = self.core.extend;
            self.plain(bar, Cmd::Extend, "sheet.extend", on)?;
        }
        Ok(())
    }

    fn build_find(&mut self, root: NodeId) -> Result<(), UiError> {
        let bar = self.ui.add(
            root,
            Flex::row().wrap().gap(Space::S8).padding(Padding::symmetric(Space::S8, Space::S4)).cross(CrossAlign::Center),
        )?;
        let find = TextField::new(Label::key("sheet.find.field")).text(&self.core.find.needle);
        self.nodes.find = Some(self.ui.add_sized(bar, find, Sizing::width(180.0))?);
        if self.core.find.replace {
            let with = TextField::new(Label::key("sheet.replace.field")).text(&self.core.find.with);
            self.nodes.with = Some(self.ui.add_sized(bar, with, Sizing::width(180.0))?);
        }
        self.plain(bar, Cmd::FindPrev, "sheet.find_prev", false)?;
        self.plain(bar, Cmd::FindNext, "sheet.find_next", false)?;
        let case = self.core.find.match_case;
        self.plain(bar, Cmd::MatchCase, "sheet.match_case", case)?;
        if self.core.find.replace {
            self.plain(bar, Cmd::ReplaceOne, "sheet.replace_one", false)?;
            self.plain(bar, Cmd::ReplaceAll, "sheet.replace_all", false)?;
        }
        self.plain(bar, Cmd::CloseFind, "sheet.close", false)?;
        Ok(())
    }

    fn build_tabs(&mut self, root: NodeId) -> Result<(), UiError> {
        let bar = self.ui.add(
            root,
            Flex::row().wrap().gap(Space::S4).padding(Padding::symmetric(Space::S8, Space::S4)).cross(CrossAlign::Center),
        )?;
        let names: Vec<String> = self.core.book.wb.sheets.iter().map(|s| s.name.clone()).collect();
        for (i, name) in names.iter().enumerate() {
            let on = i == self.core.book.active;
            let b = Button::new(Label::data(name)).variant(ButtonVariant::Tab(on));
            let id = self.ui.add(bar, b)?;
            self.cmds.push((id, Cmd::Tab(i)));
        }
        self.plain(bar, Cmd::AddSheet, "sheet.sheet_add", false)?;
        self.plain(bar, Cmd::RenameSheet, "sheet.sheet_rename", false)?;
        self.plain(bar, Cmd::DeleteSheet, "sheet.sheet_delete", false)?;
        Ok(())
    }

    /// The states of the toggle buttons (when they change the tool bar is built again).
    pub(crate) fn toggle_signature(&self) -> Vec<bool> {
        let (r, c) = self.core.sel.active;
        let sh = self.core.book.sheet();
        let st = sh.style(r, c);
        vec![
            st.bold,
            st.italic,
            st.wrap,
            st.color.is_some(),
            st.fill.is_some(),
            st.align == ultar_office::style::Align::Start,
            st.align == ultar_office::style::Align::Center,
            st.align == ultar_office::style::Align::End,
            sh.frozen_rows > 0,
            sh.frozen_cols > 0,
            sh.rtl,
            st.num_fmt.is_empty(),
            st.num_fmt == "#,##0.00",
            st.num_fmt == "0.00%",
            st.num_fmt == "$#,##0.00",
            st.num_fmt == "yyyy-mm-dd",
        ]
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
        let hint = (self.core.book.sheet().cells.is_empty() && self.core.edit.is_none()).then(|| self.t("sheet.empty"));
        let canvas = self
            .drawer
            .draw(&mut self.core.book, &frame, &self.core.sel, self.core.edit.as_ref(), hint.as_deref(), px)
            .map_err(|_| UiError::InvalidSize)?;
        self.ui.set_picture(node, canvas)
    }

    /// Brings everything on screen up to date after a change: scroll, bars, grid, title.
    pub fn refresh(&mut self) -> Result<(), UiError> {
        if self.state != State::Ready {
            return Ok(());
        }
        self.grid_size();
        self.core.reveal();
        if self.toggle_signature() != self.toggles && self.ask.is_none() {
            return self.rebuild();
        }
        self.sync_bars()?;
        self.draw_grid()?;
        let v = self.core.book.version;
        if v != self.title_version {
            self.title_version = v;
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
            let text = self.core.edit.as_ref().map_or_else(|| self.core.raw(), |e| e.text.clone());
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
                &[("sum", &fmt(sum)), ("avg", &fmt(avg)), ("n", &n.to_string())],
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
            self.recovery.clear();
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
        if now.duration_since(self.last_copy).as_secs() >= AUTOSAVE_SECS {
            self.last_copy = now;
            let path = self.core.book.path.clone();
            let _ = self.recovery.save(&self.core.book, path.as_deref());
        }
    }

    /// The layout class of the window.
    pub fn class(&self) -> SizeClass {
        self.ui.size_class()
    }
}
