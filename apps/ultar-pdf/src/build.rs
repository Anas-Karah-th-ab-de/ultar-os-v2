//! The widget tree of the reader: tool bar, start-side panel, search bar, the page area, the
//! empty states and the two dialogs. Rebuilt whenever the structure changes; smaller changes
//! (the zoom label, the page field, the hit count) go through [`App::sync_chrome`].

use crate::app::{App, Ask, Nodes, PanelMode};
use crate::doc::DocState;
use crate::place::ReadError;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, SizeClass, Space};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Dialog, EmptyState, Label, List, ListItem, PixelArea, Scroll,
    SearchField, Text, TextField,
};

/// Width of the start-side panel in logical points.
pub const PANEL_WIDTH: f32 = 208.0;

fn plain(key: &str) -> Button {
    Button::new(Label::key(key)).variant(ButtonVariant::Plain)
}

impl App {
    /// Builds the whole tree again and paints it.
    pub fn rebuild(&mut self) -> Result<(), UiError> {
        self.nodes = Nodes::default();
        self.ui.clear();
        self.ui.set_root_flex(Flex::column().cross(CrossAlign::Stretch));
        let root = self.ui.root();
        self.build_toolbar(root)?;
        if self.searching && self.doc.state == DocState::Open {
            self.build_search_bar(root)?;
        }
        if self.doc.state == DocState::Open {
            self.build_body(root)?;
        } else {
            self.build_state(root)?;
        }
        self.build_dialog()?;
        self.refresh()
    }

    fn build_toolbar(&mut self, root: NodeId) -> Result<(), UiError> {
        let open = self.doc.state == DocState::Open;
        let bar = self.ui.add(
            root,
            Flex::row()
                .wrap()
                .gap(Space::S4)
                .padding(Padding::all(Space::S8))
                .cross(CrossAlign::Center),
        )?;
        let ui = &mut self.ui;
        let n = &mut self.nodes;
        n.open = Some(ui.add(bar, plain("pdf.open"))?);
        n.find = Some(ui.add(bar, plain("pdf.search"))?);
        n.zoom_out = Some(ui.add(bar, plain("pdf.zoom_out"))?);
        n.zoom_label = Some(ui.add(bar, Text::new(Label::data("100%")).color(ColorRole::TextMuted))?);
        n.zoom_in = Some(ui.add(bar, plain("pdf.zoom_in"))?);
        n.fit_width = Some(ui.add(bar, plain("pdf.fit_width"))?);
        n.fit_page = Some(ui.add(bar, plain("pdf.fit_page"))?);
        n.rotate = Some(ui.add(bar, plain("pdf.rotate"))?);
        let field = TextField::new(Label::key("pdf.page.field"));
        n.page_field = Some(ui.add_sized(bar, field, Sizing::width(72.0))?);
        let pages = self.doc.sizes.len().to_string();
        let count = Label::key_with("pdf.page.count", &[("pages", &pages)]);
        n.page_count = Some(ui.add(bar, Text::new(count).color(ColorRole::TextMuted))?);
        for id in [
            n.find, n.zoom_out, n.zoom_in, n.fit_width, n.fit_page, n.rotate, n.page_field,
        ]
        .into_iter()
        .flatten()
        {
            ui.set_disabled(id, !open)?;
        }
        Ok(())
    }

    fn build_search_bar(&mut self, root: NodeId) -> Result<(), UiError> {
        let bar = self.ui.add(
            root,
            Flex::row()
                .wrap()
                .gap(Space::S8)
                .padding(Padding::all(Space::S8))
                .cross(CrossAlign::Center),
        )?;
        let field = SearchField::new(Label::key("pdf.search"))
            .placeholder(Label::key("pdf.search.hint"))
            .text(&self.search.query)
            .into_field();
        let label = self.search_label();
        let ui = &mut self.ui;
        let n = &mut self.nodes;
        let field = ui.add_sized(bar, field, Sizing::grow(1))?;
        n.search_field = Some(field);
        n.search_prev = Some(ui.add(bar, plain("pdf.search.prev"))?);
        n.search_next = Some(ui.add(bar, plain("pdf.search.next"))?);
        let count = Text::new(label).color(ColorRole::TextMuted);
        n.search_count = Some(ui.add(bar, count)?);
        let close = Button::new(Label::key("pdf.search.close")).variant(ButtonVariant::Quiet);
        self.nodes.search_close = Some(self.ui.add(bar, close)?);
        self.ui.focus(field)
    }

    /// The row below the tool bar: the panel (wide windows only) and the page area.
    fn build_body(&mut self, root: NodeId) -> Result<(), UiError> {
        let body = self
            .ui
            .add_sized(root, Flex::row().cross(CrossAlign::Stretch), Sizing::grow(1))?;
        if self.ui.size_class() == SizeClass::Wide {
            self.build_panel(body)?;
        }
        let area = PixelArea::new(Label::key("pdf.view"));
        self.nodes.page = Some(self.ui.add_sized(body, area, Sizing::grow(1))?);
        Ok(())
    }

    fn build_panel(&mut self, body: NodeId) -> Result<(), UiError> {
        if self.doc.outline.is_empty() {
            self.panel = PanelMode::Pages;
        }
        let col = self.ui.add_sized(
            body,
            Flex::column().cross(CrossAlign::Stretch),
            Sizing::width(PANEL_WIDTH),
        )?;
        if !self.doc.outline.is_empty() {
            let tabs = self.ui.add(
                col,
                Flex::row().gap(Space::S4).padding(Padding::all(Space::S4)),
            )?;
            let on = |mode| self.panel == mode;
            let pages = Button::new(Label::key("pdf.panel.pages"))
                .variant(ButtonVariant::Quiet)
                .toggled(on(PanelMode::Pages));
            let outline = Button::new(Label::key("pdf.panel.outline"))
                .variant(ButtonVariant::Quiet)
                .toggled(on(PanelMode::Outline));
            self.nodes.pages_tab = Some(self.ui.add(tabs, pages)?);
            self.nodes.outline_tab = Some(self.ui.add(tabs, outline)?);
        }
        match self.panel {
            PanelMode::Pages => {
                let area = PixelArea::new(Label::key("pdf.panel.pages"));
                self.nodes.thumbs = Some(self.ui.add_sized(col, area, Sizing::grow(1))?);
            }
            PanelMode::Outline => {
                let scroll = self.ui.add_sized(col, Scroll::new(), Sizing::grow(1))?;
                let list = List::navigation().name(Label::key("pdf.panel.outline"));
                let list = self.ui.add(scroll, list)?;
                for item in &self.doc.outline {
                    let indent = "\u{2003}".repeat(usize::from(item.level.min(6)));
                    let label = Label::data(&format!("{indent}{}", item.title));
                    self.ui.add(list, ListItem::new(label))?;
                }
                self.nodes.outline = Some(list);
            }
        }
        Ok(())
    }

    /// The empty states: no file, opening, password, unsupported, damaged, unreadable.
    fn build_state(&mut self, root: NodeId) -> Result<(), UiError> {
        let (icon, title, hint): (IconId, &str, Option<&str>) = match self.doc.state {
            DocState::Opening => (IconId::Ring, "pdf.state.loading", None),
            DocState::NeedsPassword => (IconId::Lock, "pdf.state.password", Some("pdf.state.password_hint")),
            DocState::Unsupported => {
                (IconId::Warning, "pdf.state.unsupported", Some("pdf.state.unsupported_hint"))
            }
            DocState::Damaged => (IconId::Warning, "pdf.state.damaged", Some("pdf.state.damaged_hint")),
            _ => match self.doc.read_error {
                Some(e) => (IconId::Warning, read_error_key(e), Some("pdf.error.hint")),
                None => (IconId::KindText, "pdf.state.no_file", Some("pdf.state.no_file_hint")),
            },
        };
        let mut empty = EmptyState::new(icon, Label::key(title));
        if let Some(h) = hint {
            empty = empty.message(Label::key(h));
        }
        let node = self.ui.add_sized(root, empty, Sizing::grow(1))?;
        if self.doc.state != DocState::Opening {
            let open = Button::new(Label::key("pdf.open")).variant(ButtonVariant::Primary);
            self.nodes.empty_open = Some(self.ui.add(node, open)?);
        }
        Ok(())
    }

    fn build_dialog(&mut self) -> Result<(), UiError> {
        let Some(ask) = self.ask else {
            return Ok(());
        };
        let root = self.ui.root();
        let password = ask == Ask::Password;
        let title = Label::key(if password { "pdf.state.password" } else { "pdf.open.title" });
        let dialog = self.ui.add(root, Dialog::new(title))?;
        self.nodes.dialog = Some(dialog);
        let focus = if password {
            self.password_fields(dialog)?
        } else {
            self.open_fields(dialog)?
        };
        let row = self
            .ui
            .add(dialog, Flex::row().gap(Space::S8).main(MainAlign::End))?;
        let (cancel_key, ok_key) = if password {
            ("pdf.password.cancel", "pdf.password.ok")
        } else {
            ("pdf.open.cancel", "pdf.open.ok")
        };
        let cancel = self.ui.add(row, plain(cancel_key))?;
        let ok = self.ui.add(row, Button::new(Label::key(ok_key)))?;
        if password {
            self.nodes.password_cancel = Some(cancel);
            self.nodes.password_ok = Some(ok);
        } else {
            self.nodes.open_cancel = Some(cancel);
            self.nodes.open_ok = Some(ok);
        }
        self.ui.set_default_action(dialog, Some(ok))?;
        self.ui.open_dialog(dialog)?;
        self.ui.focus(focus)
    }

    fn password_fields(&mut self, dialog: NodeId) -> Result<NodeId, UiError> {
        let field = self
            .ui
            .add(dialog, TextField::password(Label::key("pdf.password.label")))?;
        self.nodes.password_field = Some(field);
        if self.tried_password {
            let wrong = Text::new(Label::key("pdf.password.wrong")).color(ColorRole::TextMuted);
            self.ui.add(dialog, wrong)?;
        }
        Ok(field)
    }

    fn open_fields(&mut self, dialog: NodeId) -> Result<NodeId, UiError> {
        let field = TextField::new(Label::key("pdf.open.field")).labelled();
        let field = self.ui.add(dialog, field)?;
        self.nodes.open_field = Some(field);
        if !self.recents.entries.is_empty() {
            let heading = Text::new(Label::key("pdf.recents")).color(ColorRole::TextMuted);
            self.ui.add(dialog, heading)?;
            let list = List::new().selectable().name(Label::key("pdf.recents"));
            let list = self.ui.add(dialog, list)?;
            for entry in &self.recents.entries {
                self.ui.add(list, ListItem::new(Label::data(&crate::place::decode_name(entry))))?;
            }
            self.nodes.recents = Some(list);
        }
        Ok(field)
    }

    /// Brings the small, changing parts of the chrome up to date without a rebuild.
    pub(crate) fn sync_chrome(&mut self) -> Result<(), UiError> {
        if let Some(l) = self.nodes.zoom_label {
            let label = Label::data(&format!("{}%", self.zoom_percent()));
            self.ui.set_label(l, label)?;
        }
        if let Some(f) = self.nodes.page_field {
            if self.ui.focused() != Some(f) {
                self.ui.set_text(f, &(self.page + 1).to_string())?;
            }
        }
        if let Some(c) = self.nodes.search_count {
            let label = self.search_label();
            self.ui.set_label(c, label)?;
        }
        Ok(())
    }

    /// "n of m", "No results", "Searching…" or nothing.
    pub(crate) fn search_label(&self) -> Label {
        let s = &self.search;
        if !s.active() {
            return Label::data("");
        }
        if let (Some(pos), true) = (s.position(), s.count() > 0) {
            let (pos, total) = ((pos + 1).to_string(), s.count().to_string());
            return Label::key_with("pdf.search.count", &[("pos", &pos), ("total", &total)]);
        }
        Label::key(if s.done() { "pdf.search.none" } else { "pdf.search.running" })
    }
}

/// The message key of a read error.
fn read_error_key(e: ReadError) -> &'static str {
    e.key()
}
