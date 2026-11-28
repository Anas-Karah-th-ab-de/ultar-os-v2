//! The commands of the tool bars and how they act on the window.

use crate::app::SheetApp;
use crate::ask::{Ask, Then};
use crate::ops::NumFmt;
use ultar_office::sheet_refs::Axis;
use ultar_office::style::Align;
use ultar_ui::ui::UiError;

/// Text colours the colour button goes through (`None`: the theme's).
const COLORS: [Option<u32>; 5] = [
    None,
    Some(0xC62828),
    Some(0x1565C0),
    Some(0x2E7D32),
    Some(0x6A1B9A),
];
/// Fill colours the fill button goes through.
const FILLS: [Option<u32>; 5] = [
    None,
    Some(0xFFF59D),
    Some(0xA5D6A7),
    Some(0x90CAF9),
    Some(0xE0E0E0),
];

/// A command a button stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    /// New workbook.
    New,
    /// Open a workbook by name.
    Open,
    /// Save.
    Save,
    /// Save under another name.
    SaveAs,
    /// Undo.
    Undo,
    /// Redo.
    Redo,
    /// Show or hide the second tool bar (narrow windows).
    Tools,
    /// Bold.
    Bold,
    /// Italic.
    Italic,
    /// Wrap text.
    Wrap,
    /// Alignment.
    Align(Align),
    /// Next text colour.
    Color,
    /// Next fill colour.
    Fill,
    /// Number format.
    Fmt(NumFmt),
    /// Freeze the first row.
    FreezeRow,
    /// Freeze the first column.
    FreezeCol,
    /// Turn the sheet around.
    Rtl,
    /// Sort the selection by the active column.
    Sort(bool),
    /// Fill down.
    FillDown,
    /// Fill right.
    FillRight, // physical
    /// Insert rows.
    InsRow,
    /// Delete rows.
    DelRow,
    /// Insert columns.
    InsCol,
    /// Delete columns.
    DelCol,
    /// Open the find bar.
    Find,
    /// Previous hit.
    FindPrev,
    /// Next hit.
    FindNext,
    /// Match case on or off.
    MatchCase,
    /// Replace here.
    ReplaceOne,
    /// Replace everywhere.
    ReplaceAll,
    /// Close the find bar.
    CloseFind,
    /// Touch: the next tap extends the selection.
    Extend,
    /// Show a sheet.
    Tab(usize),
    /// Add a sheet.
    AddSheet,
    /// Rename the shown sheet.
    RenameSheet,
    /// Delete the shown sheet.
    DeleteSheet,
}

fn next(list: &[Option<u32>], now: Option<u32>) -> Option<u32> {
    let i = list.iter().position(|c| *c == now).map_or(0, |i| i + 1);
    list[i % list.len()]
}

impl SheetApp {
    /// Runs a command of a button.
    pub fn run(&mut self, cmd: Cmd) -> Result<(), UiError> {
        if self.ask.is_some() {
            return Ok(());
        }
        if !matches!(
            cmd,
            Cmd::Tools
                | Cmd::Find
                | Cmd::FindPrev
                | Cmd::FindNext
                | Cmd::MatchCase
                | Cmd::CloseFind
        ) {
            self.finish_edit();
        }
        let st = self
            .core
            .book
            .sheet()
            .style(self.core.sel.active.0, self.core.sel.active.1);
        // on a phone the tools are a sheet; a command closes it
        let closing_tools = self.tools_open && cmd != Cmd::Tools;
        if closing_tools {
            self.tools_open = false;
        }
        match cmd {
            Cmd::New => return self.new_workbook(),
            Cmd::Open => return self.ask_open(),
            Cmd::Save => return self.save(Then::Nothing),
            Cmd::SaveAs => return self.ask_name(Then::Nothing),
            Cmd::Undo => self.core.undo(),
            Cmd::Redo => self.core.redo(),
            Cmd::Tools => {
                self.tools_open = !self.tools_open;
                return self.rebuild();
            }
            Cmd::Bold => self.core.toggle_bold(),
            Cmd::Italic => self.core.toggle_italic(),
            Cmd::Wrap => self.core.toggle_wrap(),
            Cmd::Align(a) => self
                .core
                .set_align(if st.align == a { Align::General } else { a }),
            Cmd::Color => self.core.set_color(next(&COLORS, st.color)),
            Cmd::Fill => self.core.set_fill(next(&FILLS, st.fill)),
            Cmd::Fmt(f) => self.core.set_num_fmt(f),
            Cmd::FreezeRow => self.core.toggle_freeze_row(),
            Cmd::FreezeCol => self.core.toggle_freeze_col(),
            Cmd::Rtl => self.core.toggle_rtl(),
            Cmd::Sort(asc) => self.core.sort(asc),
            Cmd::FillDown => self.core.fill(true),
            Cmd::FillRight => self.core.fill(false), // physical
            Cmd::InsRow => self.core.insert_lines(Axis::Rows),
            Cmd::DelRow => self.core.delete_lines(Axis::Rows),
            Cmd::InsCol => self.core.insert_lines(Axis::Cols),
            Cmd::DelCol => self.core.delete_lines(Axis::Cols),
            Cmd::Find => return self.open_find(false),
            Cmd::FindNext => {
                self.core.find_next(true);
            }
            Cmd::FindPrev => {
                self.core.find_next(false);
            }
            Cmd::MatchCase => {
                self.core.find.match_case = !self.core.find.match_case;
                return self.rebuild();
            }
            Cmd::ReplaceOne => self.core.replace_one(),
            Cmd::ReplaceAll => {
                self.core.replace_all();
            }
            Cmd::CloseFind => {
                self.core.find.open = false;
                return self.rebuild();
            }
            Cmd::Extend => {
                self.core.extend = !self.core.extend;
                return self.rebuild();
            }
            Cmd::Tab(i) => self.core.show_sheet(i),
            Cmd::AddSheet => {
                self.core.add_sheet();
                return self.rebuild();
            }
            Cmd::RenameSheet => return self.ask_rename(),
            Cmd::DeleteSheet => return self.delete_sheet(),
        }
        if closing_tools
            || matches!(
                cmd,
                Cmd::Tab(_) | Cmd::Undo | Cmd::Redo | Cmd::DelRow | Cmd::DelCol
            )
        {
            // the tabs may have changed
            return self.rebuild();
        }
        self.refresh()
    }

    /// Writes an open edit before a command acts on the cells.
    pub(crate) fn finish_edit(&mut self) {
        if self.core.edit.is_some() {
            self.core.commit();
        }
    }

    /// Opens the find bar (with the replace field when `replace`) and puts the focus in it.
    pub fn open_find(&mut self, replace: bool) -> Result<(), UiError> {
        self.core.find.open = true;
        self.core.find.replace = replace;
        self.focus_find = true;
        self.rebuild()
    }

    fn delete_sheet(&mut self) -> Result<(), UiError> {
        let i = self.core.book.active;
        match self.core.delete_sheet(i) {
            Ok(()) => self.core.say("sheet.sheet_deleted"),
            Err(_) => self.core.say("sheet.sheet_last"),
        }
        self.rebuild()
    }

    fn new_workbook(&mut self) -> Result<(), UiError> {
        if self.core.book.dirty() && self.state == crate::app::State::Ready {
            self.ask = Some(Ask::Unsaved(Then::New));
            return self.rebuild();
        }
        self.reset()
    }

    /// Replaces the workbook by an empty one.
    pub(crate) fn reset(&mut self) -> Result<(), UiError> {
        self.recovery.clear();
        let dir = self.recovery.dir().to_owned();
        self.core = crate::core::Core::new(crate::book::Book::new());
        self.recovery = crate::recover::Recovery::new(&dir, None);
        self.state = crate::app::State::Ready;
        self.title_version = u64::MAX;
        self.rebuild()
    }
}
