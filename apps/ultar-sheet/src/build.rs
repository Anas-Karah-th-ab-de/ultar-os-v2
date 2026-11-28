//! The widget tree of the sheet window: tool bars, formula bar, grid, find bar, sheet tabs,
//! status line and the pages of a workbook that is not open.

use crate::app::{Cmd, SheetApp, State};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, InputMode, SizeClass, Space};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Dialog, EmptyState, Label, PixelArea, Text, TextField,
};

impl SheetApp {
    pub(crate) fn build_state(&mut self, state: &State) -> Result<(), UiError> {
        let (icon, title, message) = match state {
            State::Loading(name) => (IconId::Info, Label::key("sheet.loading"), Label::data(name)),
            State::TooBig => (
                IconId::Warning,
                Label::key("sheet.too_big.title"),
                Label::key_with(
                    "sheet.too_big",
                    &[("mb", &(crate::file::MAX_BYTES >> 20).to_string())],
                ),
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
        self.ui.add_sized(
            root,
            EmptyState::new(icon, title).message(message),
            Sizing::grow(1),
        )?;
        if !matches!(state, State::Loading(_)) {
            let row = self.ui.add(
                root,
                Flex::row()
                    .gap(Space::S8)
                    .padding(Padding::all(Space::S12))
                    .main(MainAlign::Center),
            )?;
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
            Flex::row()
                .wrap()
                .gap(Space::S4)
                .padding(Padding::symmetric(Space::S8, Space::S4))
                .cross(CrossAlign::Center),
        )
    }

    pub(crate) fn build_main(&mut self) -> Result<(), UiError> {
        let root = self.ui.root();
        let compact = self.ui.size_class() == SizeClass::Compact;
        let st = self
            .core
            .book
            .sheet()
            .style(self.core.sel.active.0, self.core.sel.active.1);
        let sh = self.core.book.sheet();
        let look = (sh.frozen_rows > 0, sh.frozen_cols > 0, sh.rtl);
        self.build_file_bar(root, compact, &st)?;
        if !compact {
            let tools = self.bar(root)?;
            self.build_tools(tools, &st, look, false)?;
        }
        self.build_formula_bar(root)?;
        self.nodes.grid = Some(self.ui.add_sized(
            root,
            PixelArea::new(Label::key("sheet.grid")),
            Sizing::grow(1),
        )?);
        if self.core.find.open {
            self.build_find(root)?;
        }
        self.build_tabs(root)?;
        let status = self.ui.add(
            root,
            Flex::row().padding(Padding::symmetric(Space::S8, Space::S4)),
        )?;
        self.nodes.status = Some(self.ui.add(
            status,
            Text::new(Label::data("")).color(ColorRole::TextMuted),
        )?);
        if compact && self.tools_open && self.ask.is_none() {
            self.build_tools_sheet(root, &st, look)?;
        }
        Ok(())
    }

    /// The first bar: the file, the history and what a narrow window keeps in reach.
    fn build_file_bar(
        &mut self,
        root: NodeId,
        compact: bool,
        st: &ultar_office::style::CellStyle,
    ) -> Result<(), UiError> {
        let bar = self.bar(root)?;
        self.button(bar, Cmd::Save, "sheet.save", false, ButtonVariant::Primary)?;
        self.nodes.undo = Some(self.plain(bar, Cmd::Undo, "sheet.undo", false)?);
        self.nodes.redo = Some(self.plain(bar, Cmd::Redo, "sheet.redo", false)?);
        self.plain(bar, Cmd::Bold, "sheet.bold", st.bold)?;
        if compact {
            let open = self.tools_open;
            self.plain(bar, Cmd::Tools, "sheet.tools", open)?;
            if self.ui.input_mode() == InputMode::Touch {
                let on = self.core.extend;
                self.plain(bar, Cmd::Extend, "sheet.extend", on)?;
            }
            return Ok(());
        }
        self.plain(bar, Cmd::New, "sheet.new", false)?;
        self.plain(bar, Cmd::Open, "sheet.open", false)?;
        self.plain(bar, Cmd::SaveAs, "sheet.save_as", false)?;
        let name = self
            .core
            .book
            .name()
            .map_or_else(|| Label::key("sheet.untitled"), |n| Label::data(&n));
        self.ui.add_sized(
            bar,
            Text::new(name).color(ColorRole::TextMuted),
            Sizing::grow(1),
        )?;
        Ok(())
    }

    /// The cell name and the formula field.
    fn build_formula_bar(&mut self, root: NodeId) -> Result<(), UiError> {
        let fbar = self.ui.add(
            root,
            Flex::row()
                .gap(Space::S8)
                .padding(Padding::symmetric(Space::S8, Space::S4))
                .cross(CrossAlign::Center),
        )?;
        self.nodes.name_box = Some(self.ui.add_sized(
            fbar,
            Text::new(Label::data(&self.core.cell_name())).color(ColorRole::TextMuted),
            Sizing::width(48.0),
        )?);
        let field = TextField::new(Label::key("sheet.formula"))
            .placeholder(Label::key("sheet.formula.hint"));
        self.nodes.formula = Some(self.ui.add_sized(fbar, field, Sizing::grow(1))?);
        Ok(())
    }

    /// A phone: the tools are a sheet over the grid, not rows that squeeze it away.
    fn build_tools_sheet(
        &mut self,
        root: NodeId,
        st: &ultar_office::style::CellStyle,
        look: (bool, bool, bool),
    ) -> Result<(), UiError> {
        let sheet = self
            .ui
            .add(root, Dialog::sheet(Label::key("sheet.tools")))?;
        let tools = self.bar(sheet)?;
        self.build_tools(tools, st, look, true)?;
        let row = self.ui.add(sheet, Flex::row().main(MainAlign::End))?;
        self.button(
            row,
            Cmd::Tools,
            "sheet.close",
            false,
            ButtonVariant::Primary,
        )?;
        self.ui.open_dialog(sheet)
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
        self.plain(
            bar,
            Cmd::Align(Align::Start),
            "sheet.align_start",
            st.align == Align::Start,
        )?;
        self.plain(
            bar,
            Cmd::Align(Align::Center),
            "sheet.align_center",
            st.align == Align::Center,
        )?;
        self.plain(
            bar,
            Cmd::Align(Align::End),
            "sheet.align_end",
            st.align == Align::End,
        )?;
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
        self.plain(bar, Cmd::FillRight, "sheet.fill_right", false)?; // physical
        self.plain(bar, Cmd::InsRow, "sheet.row_add", false)?;
        self.plain(bar, Cmd::DelRow, "sheet.row_del", false)?;
        self.plain(bar, Cmd::InsCol, "sheet.col_add", false)?;
        self.plain(bar, Cmd::DelCol, "sheet.col_del", false)?;
        self.plain(bar, Cmd::Find, "sheet.find", self.core.find.open)?;
        Ok(())
    }

    fn build_find(&mut self, root: NodeId) -> Result<(), UiError> {
        let bar = self.ui.add(
            root,
            Flex::row()
                .wrap()
                .gap(Space::S8)
                .padding(Padding::symmetric(Space::S8, Space::S4))
                .cross(CrossAlign::Center),
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
            Flex::row()
                .wrap()
                .gap(Space::S4)
                .padding(Padding::symmetric(Space::S8, Space::S4))
                .cross(CrossAlign::Center),
        )?;
        let names: Vec<String> = self
            .core
            .book
            .wb
            .sheets
            .iter()
            .map(|s| s.name.clone())
            .collect();
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
}
