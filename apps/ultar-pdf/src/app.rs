//! The reader window: a tool bar (Open, zoom, fit, rotate, the page field), the pages drawn
//! continuously into a picture, a start-side panel with the outline on wide windows, a search
//! bar and two dialogs: open a file, and the password of an encrypted file.

use crate::doc::{Doc, DocState, Evt};
use crate::keys::{self, Effect};
use crate::place::{self, ReadError, Recents};
use crate::state::{
    fit_page_scale, fit_width_scale, layout_pages, page_at, selection_text, visible_pages,
    words_in_rect, zoom_step, Fit, Hit, PageRow, Rot,
};
use std::path::PathBuf;
use ultar_office::pdf::PageImage;
use ultar_ui::canvas::Canvas;
use ultar_ui::event::{Action, ActionKind, Event, Modifiers, PointerButton};
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, SizeClass, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Dialog, Label, List, ListItem, PixelArea, Text, TextField,
};

/// What the window has to do for the reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// Offer this text on the clipboard.
    Copy(String),
    /// Show this title.
    Title(String),
    /// The window may close now.
    Close,
}

/// An open question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// Path or files:// URI of a PDF to open.
    Open,
    /// The password of an encrypted PDF.
    Password,
}

#[derive(Default)]
struct Nodes {
    open: Option<NodeId>,
    zoom_out: Option<NodeId>,
    zoom_in: Option<NodeId>,
    fit_width: Option<NodeId>,
    fit_page: Option<NodeId>,
    rotate: Option<NodeId>,
    page_field: Option<NodeId>,
    page_count: Option<NodeId>,
    panel: Option<NodeId>,
    page: Option<NodeId>,
    search_field: Option<NodeId>,
    search_next: Option<NodeId>,
    search_prev: Option<NodeId>,
    search_count: Option<NodeId>,
    search_close: Option<NodeId>,
    statement: Option<NodeId>,
    dialog: Option<NodeId>,
    open_field: Option<NodeId>,
    open_ok: Option<NodeId>,
    open_cancel: Option<NodeId>,
    recents: Option<NodeId>,
    password_field: Option<NodeId>,
    password_ok: Option<NodeId>,
    password_cancel: Option<NodeId>,
    password_wrong: Option<NodeId>,
}

/// The reader: its tree, its document and its view state.
pub struct App {
    pub ui: Ui,
    pub doc: Doc,
    areas: PathBuf,
    data: PathBuf,
    recents: Recents,
    nodes: Nodes,
    /// Page list of the side panel (list index → page), when present.
    panel_pages: Vec<usize>,
    // view
    fit: Fit,
    scale: f32,
    scroll: f32,
    rot: Rot,
    page: usize,
    initial_page: usize,
    rows: Vec<PageRow>,
    total_h: f32,
    // search
    searching: bool,
    query: String,
    hits: Vec<Hit>,
    current_hit: usize,
    // selection
    selection: Option<(usize, Vec<usize>)>,
    selecting: bool,
    drag_start: Option<(f32, f32)>,
    drag_cur: Option<(f32, f32)>,
    ask: Option<Ask>,
    open_failed: Option<ReadError>,
    wrong_password: bool,
    out: Vec<Out>,
}

impl App {
    /// Resolves `uri` (`files://…`) and reads the file; a file that cannot be read gives an
    /// empty document and the reason ([`App::show_open_failed`]).
    pub fn open(uri: Option<&str>, areas: &std::path::Path, _data: &PathBuf) -> (Doc, Option<ReadError>) {
        let Some(uri) = uri else {
            return (Doc::none(), None);
        };
        let Some(path) = place::path_of(uri, areas) else {
            return (Doc::none(), Some(ReadError::Missing));
        };
        match Doc::open(&path, Some(uri.to_owned()), areas, None) {
            Ok(doc) => (doc, None),
            Err(e) => (Doc::none(), Some(e)),
        }
    }

    /// A reader over `ui` for `doc`; `areas` binds the user's places, `data` is `app:data`.
    pub fn new(
        ui: Ui,
        doc: Doc,
        areas: PathBuf,
        data: PathBuf,
        initial_page: usize,
    ) -> Result<App, UiError> {
        let mut recents = Recents::load(&data);
        if let Some(u) = &doc.uri {
            recents.push(u);
            recents.save(&data);
        }
        let mut app = App {
            ui,
            doc,
            areas,
            data,
            recents,
            nodes: Nodes::default(),
            panel_pages: Vec::new(),
            fit: Fit::Width,
            scale: 1.0,
            scroll: 0.0,
            rot: Rot::None,
            page: initial_page,
            initial_page,
            rows: Vec::new(),
            total_h: 0.0,
            searching: false,
            query: String::new(),
            hits: Vec::new(),
            current_hit: 0,
            selection: None,
            selecting: false,
            drag_start: None,
            drag_cur: None,
            ask: None,
            open_failed: None,
            wrong_password: false,
            out: Vec::new(),
        };
        app.rebuild()?;
        let title = app.title();
        app.out.push(Out::Title(title));
        Ok(app)
    }

    /// Shows in the status why the file did not open.
    pub fn show_open_failed(&mut self, e: ReadError) {
        self.open_failed = Some(e);
        let _ = self.rebuild();
    }

    /// The window title: the document's name and the app name.
    pub fn title(&self) -> String {
        let tr = self.ui.translator();
        let name = self
            .doc
            .uri
            .as_deref()
            .map(|u| u.split(['#', '?']).next().unwrap_or(u))
            .and_then(|u| u.rsplit(['/', '\\']).next())
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| tr.t("app.pdf.name").into_owned());
        format!("{name} – {}", tr.t("app.pdf.name"))
    }

    /// The URI the mesh should report (document + page), or `None` without a document.
    pub fn activity_uri(&self) -> Option<String> {
        self.doc
            .uri
            .as_ref()
            .map(|u| format!("{}#page={}", u, self.page + 1))
    }

    /// What the window has to do (taken once).
    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// The page picture's size in device pixels.
    fn page_pixels(&mut self) -> Option<(u32, u32)> {
        self.ui.layout();
        let rect = self.ui.rect(self.nodes.page?)?;
        let s = self.ui.env().scale;
        Some((
            (rect.w * s).round().max(1.0) as u32,
            (rect.h * s).round().max(1.0) as u32,
        ))
    }

    /// Recomputes the rows and clamps the scroll.
    fn relayout(&mut self) -> (Vec<PageRow>, f32) {
        let (rows, total) = layout_pages(&self.doc.sizes, self.scale, self.rot);
        let view_h = self.page_pixels().map_or(0.0, |(_, h)| h as f32);
        self.scroll = self.scroll.clamp(0.0, (total - view_h).max(0.0));
        self.rows = rows;
        self.total_h = total;
        (self.rows.clone(), total)
    }

    /// The scale for page `page` under the current view.
    fn scale_for(&mut self, page: usize) -> f32 {
        let (w, h) = self
            .doc
            .sizes
            .get(page)
            .copied()
            .unwrap_or((595.0, 842.0));
        let (vw, vh) = self.page_pixels().unwrap_or((800, 600));
        match self.fit {
            Fit::Width => fit_width_scale(vw as f32, w),
            Fit::Page => fit_page_scale(vw as f32, vh as f32, w, h),
            Fit::Zoom(p) => p as f32 / 100.0,
        }
    }

    /// The zoom as percent.
    fn percent(&self) -> u16 {
        (self.scale * 100.0).round().clamp(1.0, 999.0) as u16
    }

    /// Applies the current fit/scale to the geometry and asks for the visible pages.
    fn apply_view(&mut self) -> Result<(), UiError> {
        if self.doc.state != DocState::Open {
            return Ok(());
        }
        self.scale = self.scale_for(self.page);
        let (rows, _) = self.relayout();
        self.request_visible(&rows);
        self.draw_page()
    }

    /// Asks the worker for every visible page that is not in the cache yet.
    fn request_visible(&mut self, rows: &[PageRow]) {
        let view_h = self.page_pixels().map_or(0.0, |(_, h)| h as f32);
        for page in visible_pages(rows, self.scroll, view_h.max(1.0)) {
            if self.doc.image(page, self.scale, self.rot).is_none() {
                self.doc.request_render(page, self.scale, self.rot);
            }
            self.doc.request_words(page);
        }
    }

    /// Builds the whole tree (also after size or state changes).
    pub fn rebuild(&mut self) -> Result<(), UiError> {
        let percent = self.percent();
        let pages = self.pages_count().to_string();
        let current_page = (self.page + 1).to_string();
        let wide = self.ui.size_class() == SizeClass::Wide && !self.doc.sizes.is_empty();
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
        let plain = |key| Button::new(Label::key(key)).variant(ButtonVariant::Plain);
        self.nodes.open = Some(ui.add(bar, plain("pdf.open"))?);
        self.nodes.zoom_out = Some(ui.add(bar, plain("pdf.zoom_out"))?);
        let zoom_label = Label::data(&format!("{}%", percent));
        ui.add(bar, Text::new(zoom_label).color(ColorRole::TextMuted))?;
        self.nodes.zoom_in = Some(ui.add(bar, plain("pdf.zoom_in"))?);
        self.nodes.fit_width = Some(ui.add(bar, plain("pdf.fit_width"))?);
        self.nodes.fit_page = Some(ui.add(bar, plain("pdf.fit_page"))?);
        self.nodes.rotate = Some(ui.add(bar, plain("pdf.rotate"))?);
        let field = TextField::new(Label::key("pdf.page.field")).text(&current_page);
        self.nodes.page_field = Some(ui.add_sized(bar, field, Sizing::width(64.0))?);
        let count = Label::key_with("pdf.page.count", &[("pages", &pages)]);
        self.nodes.page_count = Some(ui.add(bar, Text::new(count).color(ColorRole::TextMuted))?);

        let body = ui.add(
            root,
            Flex::row().gap(Space::S4).cross(CrossAlign::Stretch),
        )?;
        // A start-side panel with the outline (wide windows; mirrored automatically in RTL).
        self.nodes.panel = None;
        self.panel_pages.clear();
        if wide {
            let panel = ui.add_sized(body, List::navigation(), Sizing::width(220.0))?;
            self.nodes.panel = Some(panel);
            if self.doc.outline.is_empty() {
                for i in 0..self.doc.sizes.len() {
                    ui.add(panel, ListItem::new(Label::data(&format!("{}", i + 1))))?;
                    self.panel_pages.push(i);
                }
            } else {
                for (page, title) in &self.doc.outline {
                    ui.add(panel, ListItem::new(Label::data(title)))?;
                    self.panel_pages.push(*page);
                }
            }
        }

        let page = ui.add_sized(body, PixelArea::new(Label::key("app.pdf.name")), Sizing::grow(1))?;
        self.nodes.page = Some(page);

        if self.searching {
            self.build_search_bar(root)?;
        }
        self.build_status(root)?;
        self.build_dialog()?;
        self.buttons()?;
        self.draw_page()
    }

    fn pages_count(&self) -> usize {
        self.doc.sizes.len().max(self.doc.info.as_ref().map_or(0, |i| i.pages))
    }

    fn build_search_bar(&mut self, root: NodeId) -> Result<(), UiError> {
        let bar = self.ui.add(
            root,
            Flex::row()
                .gap(Space::S8)
                .padding(Padding::all(Space::S8))
                .cross(CrossAlign::Center),
        )?;
        let field = TextField::new(Label::key("pdf.search")).text(&self.query);
        self.nodes.search_field = Some(self.ui.add_sized(bar, field, Sizing::grow(1))?);
        self.nodes.search_prev = Some(
            self.ui
                .add(bar, Button::new(Label::key("pdf.search.prev")).variant(ButtonVariant::Plain))?,
        );
        self.nodes.search_next = Some(
            self.ui
                .add(bar, Button::new(Label::key("pdf.search.next")).variant(ButtonVariant::Plain))?,
        );
        let count = if self.hits.is_empty() {
            Label::key("pdf.search.none")
        } else {
            Label::key_with(
                "pdf.search.count",
                &[
                    ("pos", &(self.current_hit + 1).to_string()),
                    ("total", &self.hits.len().to_string()),
                ],
            )
        };
        self.nodes.search_count = Some(self.ui.add(bar, Text::new(count).color(ColorRole::TextMuted))?);
        self.nodes.search_close = Some(
            self.ui
                .add(bar, Button::new(Label::key("pdf.search.close")).variant(ButtonVariant::Plain))?,
        );
        if let Some(f) = self.nodes.search_field {
            let _ = self.ui.focus(f);
        }
        Ok(())
    }

    fn build_status(&mut self, root: NodeId) -> Result<(), UiError> {
        let key: &str = match self.doc.state {
            DocState::NoFile => match self.open_failed {
                Some(e) => e.key(),
                None => "pdf.state.no_file",
            },
            DocState::Opening => "pdf.state.loading",
            DocState::Damaged => "pdf.state.damaged",
            DocState::NeedsPassword => "pdf.state.password",
            DocState::Open => return Ok(()),
        };
        let hint: Option<&str> = match self.doc.state {
            DocState::NoFile if self.open_failed.is_none() => Some("pdf.state.no_file_hint"),
            DocState::Damaged => Some("pdf.state.damaged_hint"),
            DocState::NeedsPassword => Some("pdf.state.password_hint"),
            _ => None,
        };
        let col = self.ui.add(root, Flex::column().padding(Padding::all(Space::S16)))?;
        self.nodes.statement = Some(
            self.ui
                .add(col, Text::new(Label::key(key)).color(ColorRole::TextMuted))?,
        );
        if let Some(h) = hint {
            self.ui.add(col, Text::new(Label::key(h)).color(ColorRole::TextMuted))?;
        }
        Ok(())
    }

    fn build_dialog(&mut self) -> Result<(), UiError> {
        let Some(ask) = self.ask else {
            return Ok(());
        };
        let root = self.ui.root();
        let is_password = ask == Ask::Password;
        let title = if is_password {
            Label::key("pdf.state.password")
        } else {
            Label::key("pdf.open.title")
        };
        let dialog = self.ui.add(root, Dialog::new(title))?;
        self.nodes.dialog = Some(dialog);
        if is_password {
            self.nodes.password_field = Some(
                self.ui
                    .add(dialog, TextField::new(Label::key("pdf.password.label")).labelled())?,
            );
            if self.wrong_password {
                self.nodes.password_wrong = Some(
                    self.ui
                        .add(dialog, Text::new(Label::key("pdf.password.wrong")).color(ColorRole::TextMuted))?,
                );
            }
        } else {
            self.nodes.open_field = Some(
                self.ui
                    .add(dialog, TextField::new(Label::key("pdf.open.field")).labelled())?,
            );
            self.nodes.recents = None;
            if !self.recents.entries.is_empty() {
                self.ui.add(dialog, Text::new(Label::key("pdf.recents")).color(ColorRole::TextMuted))?;
                let list = self.ui.add(dialog, List::new().selectable())?;
                for entry in &self.recents.entries {
                    self.ui.add(list, ListItem::new(Label::data(entry)))?;
                }
                self.nodes.recents = Some(list);
            }
        }
        let row = self
            .ui
            .add(dialog, Flex::row().gap(Space::S8).main(MainAlign::End))?;
        let cancel = self.ui.add(
            row,
            Button::new(Label::key("pdf.open.cancel")).variant(ButtonVariant::Plain),
        )?;
        let ok = self.ui.add(row, Button::new(Label::key("pdf.open.ok")))?;
        if is_password {
            self.nodes.password_cancel = Some(cancel);
            self.nodes.password_ok = Some(ok);
        } else {
            self.nodes.open_cancel = Some(cancel);
            self.nodes.open_ok = Some(ok);
        }
        self.ui.set_default_action(dialog, Some(ok))?;
        self.ui.open_dialog(dialog)?;
        if is_password {
            if let Some(f) = self.nodes.password_field {
                self.ui.focus(f)?;
            }
        } else if let Some(f) = self.nodes.open_field {
            self.ui.focus(f)?;
        }
        Ok(())
    }

    fn buttons(&mut self) -> Result<(), UiError> {
        let disabled = self.doc.state != DocState::Open;
        for id in [
            self.nodes.zoom_out,
            self.nodes.zoom_in,
            self.nodes.fit_width,
            self.nodes.fit_page,
            self.nodes.rotate,
        ]
        .into_iter()
        .flatten()
        {
            self.ui.set_disabled(id, disabled)?;
        }
        Ok(())
    }

    /// Drains the worker and updates the view for the open result and the search results.
    pub fn poll(&mut self) -> Result<(), UiError> {
        let prev_state = self.doc.state;
        let mut search_hits: Option<Vec<Hit>> = None;
        for evt in self.doc.poll() {
            match evt {
                Evt::Search { hits } => search_hits = Some(hits),
                other => self.doc.accept(other),
            }
        }
        if self.doc.state != prev_state {
            match self.doc.state {
                DocState::Open => {
                    self.page = self.initial_page.min(self.doc.sizes.len().saturating_sub(1));
                    self.apply_view()?;
                    self.rebuild()?;
                    return Ok(());
                }
                DocState::NeedsPassword => {
                    self.ask = Some(Ask::Password);
                    self.rebuild()?;
                    return Ok(());
                }
                DocState::Damaged => {
                    self.rebuild()?;
                    return Ok(());
                }
                _ => {}
            }
        }
        if let Some(hits) = search_hits {
            self.on_search_hits(hits)?;
        }
        if self.doc.state == DocState::Open {
            self.relayout();
            self.request_visible(&self.rows.clone());
            let vh = self.page_pixels().map_or(0.0, |(_, h)| h as f32);
            self.page = page_at(&self.rows, self.scroll, vh).unwrap_or(self.page);
            self.draw_page()?;
        }
        Ok(())
    }

    fn on_search_hits(&mut self, hits: Vec<Hit>) -> Result<(), UiError> {
        self.hits = hits;
        self.current_hit = 0;
        if let Some(first) = self.hits.first() {
            self.page = first.page;
            let (rows, _) = layout_pages(&self.doc.sizes, self.scale, self.rot);
            self.scroll = crate::state::scroll_to_page(&rows, first.page);
            for h in self.hits.iter() {
                self.doc.request_words(h.page);
            }
        }
        self.rebuild()
    }

    /// Draws the visible pages into the picture node.
    pub fn draw_page(&mut self) -> Result<(), UiError> {
        let Some(node) = self.nodes.page else {
            return Ok(());
        };
        let Some((vw, vh)) = self.page_pixels() else {
            return Ok(());
        };
        let ground = self.ui.env().palette().get(ColorRole::Ground);
        let mut data = vec![ground.b, ground.g, ground.r, 255].repeat((vw * vh) as usize);
        if self.doc.state == DocState::Open {
            self.relayout();
            let range = visible_pages(&self.rows, self.scroll, vh as f32);
            for page in range {
                if let Some(img) = self.doc.image(page, self.scale, self.rot) {
                    let ox = ((vw as i32 - img.w as i32) / 2).max(0);
                    let oy = (self.rows[page].y - self.scroll).round() as i32;
                    blit(&mut data, vw, vh, ox, oy, &img);
                    self.highlights(&mut data, vw, vh, ox, oy, page);
                }
            }
        }
        let canvas = Canvas::from_bgrx(vw, vh, data).map_err(|_| UiError::InvalidSize)?;
        self.ui.set_picture(node, canvas)
    }

    /// Draws the search- and selection rectangles over `page` (unrotated pages only: the word
    /// boxes are in the page's own points).
    fn highlights(&self, data: &mut [u8], vw: u32, vh: u32, ox: i32, oy: i32, page: usize) {
        if self.rot != Rot::None {
            return;
        }
        let Some(lines) = self.doc.lines(page) else {
            return;
        };
        let page_h = self.doc.sizes.get(page).map_or(0.0, |s| s.1);
        let words: Vec<&ultar_office::pdf::Word> = lines.iter().flat_map(|l| l.words.iter()).collect();
        let mut rects: Vec<((f32, f32, f32, f32), bool)> = Vec::new();
        for (hi, hit) in self.hits.iter().enumerate() {
            if hit.page != page {
                continue;
            }
            for wi in hit.first..=hit.last {
                if let Some(w) = words.get(wi) {
                    let x0 = w.x0 * self.scale + ox as f32;
                    let x1 = w.x1 * self.scale + ox as f32;
                    let y0 = (page_h - w.y1) * self.scale + oy as f32;
                    let y1 = (page_h - w.y0) * self.scale + oy as f32;
                    rects.push(((x0, y0, x1, y1), hi == self.current_hit));
                }
            }
        }
        if let Some((sel_page, indices)) = &self.selection {
            if *sel_page == page {
                for i in indices {
                    if let Some(w) = words.get(*i) {
                        let x0 = w.x0 * self.scale + ox as f32;
                        let x1 = w.x1 * self.scale + ox as f32;
                        let y0 = (page_h - w.y1) * self.scale + oy as f32;
                        let y1 = (page_h - w.y0) * self.scale + oy as f32;
                        rects.push(((x0, y0, x1, y1), false));
                    }
                }
            }
        }
        for ((x0, y0, x1, y1), current) in rects {
            let color = if current { (0xFF, 0x90u8, 0x00) } else { (0x60, 0xA0, 0xE0) };
            fill_rect(data, vw, vh, x0, y0, x1, y1, color);
        }
    }

    /// A pointer event on the page area: scrolling and drag selection.
    pub fn pointer(&mut self, event: Event) -> Result<(), UiError> {
        if self.ask.is_some() || self.doc.state != DocState::Open {
            return Ok(());
        }
        let Some(node) = self.nodes.page else {
            return Ok(());
        };
        match event {
            Event::Scroll { pos, lines } => {
                if self.ui.rect(node).is_some_and(|r| r.contains(pos)) {
                    let vh = self.page_pixels().map_or(0.0, |(_, h)| h as f32);
                    self.relayout();
                    self.scroll = (self.scroll + lines * 32.0).clamp(0.0, (self.total_h - vh).max(0.0));
                    self.page = page_at(&self.rows, self.scroll, vh).unwrap_or(self.page);
                    self.draw_page()?;
                }
                return Ok(());
            }
            Event::PointerDown {
                pos,
                button: PointerButton::Primary,
            } => {
                if self.ui.rect(node).is_some_and(|r| r.contains(pos)) {
                    self.drag_start = Some((pos.x, pos.y));
                    self.drag_cur = Some((pos.x, pos.y));
                    self.selecting = true;
                    self.selection = None;
                }
                return Ok(());
            }
            Event::PointerMove { pos } if self.selecting => {
                self.drag_cur = Some((pos.x, pos.y));
                self.draw_page()?;
                return Ok(());
            }
            Event::PointerUp { .. } => {
                if self.selecting {
                    self.selecting = false;
                    self.finish_selection();
                    self.draw_page()?;
                }
                return Ok(());
            }
            _ => {}
        }
        Ok(())
    }

    /// Turns the drag rectangle into a selection of words on one page.
    fn finish_selection(&mut self) {
        let Some((sx, sy)) = self.drag_start else {
            return;
        };
        let (ex, ey) = self.drag_cur.unwrap_or((sx, sy));
        let Some(node) = self.nodes.page else {
            return;
        };
        let Some(rect) = self.ui.rect(node) else {
            return;
        };
        let s = self.ui.env().scale;
        // Screen logical → content device pixels.
        let cy0 = self.scroll + (sy.min(ey) - rect.y) * s;
        let cy1 = self.scroll + (sy.max(ey) - rect.y) * s;
        let page = page_at(&self.rows, (cy0 + cy1) / 2.0, 1.0);
        let Some(page) = page else {
            return;
        };
        let Some(lines) = self.doc.lines(page) else {
            return;
        };
        let words: Vec<ultar_office::pdf::Word> = lines.iter().flat_map(|l| l.words.clone()).collect();
        if words.is_empty() {
            return;
        }
        let (vw, _vh) = self.page_pixels().unwrap_or((0, 0));
        let page_w = self.doc.sizes.get(page).map_or(0.0, |p| p.0);
        let left = ((vw as f32 - page_w * self.scale) / 2.0).max(0.0);
        let page_h = self.doc.sizes.get(page).map_or(0.0, |p| p.1);
        let top = self.rows[page].y;
        // Convert the drag rectangle to page points (y up).
        let x0 = ((sx.min(ex) - rect.x) * s - left) / self.scale;
        let x1 = ((sx.max(ex) - rect.x) * s - left) / self.scale;
        let y0u = page_h - (cy0 - top) / self.scale;
        let y1u = page_h - (cy1 - top) / self.scale;
        let indices = words_in_rect(&words, (x0, y0u.min(y1u), x1, y0u.max(y1u)));
        if indices.is_empty() {
            self.selection = None;
        } else {
            self.selection = Some((page, indices));
        }
    }

    /// Copies the selection to the clipboard (Ctrl+C).
    fn copy_selection(&mut self) {
        let Some((page, indices)) = self.selection.clone() else {
            return;
        };
        let Some(lines) = self.doc.lines(page) else {
            return;
        };
        let words: Vec<_> = lines.iter().flat_map(|l| l.words.clone()).collect();
        let text = selection_text(&words, indices);
        if !text.is_empty() {
            self.out.push(Out::Copy(text));
        }
    }

    /// A key the widgets did not use (the shortcuts).
    pub fn key(&mut self, key: ultar_ui::event::Key, modifiers: Modifiers) -> Result<(), UiError> {
        if self.ask.is_some() {
            return Ok(());
        }
        let effect = keys::apply(key, modifiers, self.searching);
        match effect {
            Effect::None => {}
            Effect::ZoomIn => self.zoom_to(Fit::Zoom(zoom_step(self.percent(), 1)))?,
            Effect::ZoomOut => self.zoom_to(Fit::Zoom(zoom_step(self.percent(), -1)))?,
            Effect::ZoomReset => self.zoom_to(Fit::Zoom(100))?,
            Effect::FitWidth => self.fit_to(Fit::Width)?,
            Effect::FitPage => self.fit_to(Fit::Page)?,
            Effect::Rotate => self.rotate()?,
            Effect::Search => {
                self.searching = true;
                self.rebuild()?;
            }
            Effect::CloseSearch => {
                self.searching = false;
                self.query.clear();
                self.hits.clear();
                self.rebuild()?;
            }
            Effect::NextHit => self.next_hit()?,
            Effect::PrevHit => self.prev_hit()?,
            Effect::Copy => self.copy_selection(),
            Effect::PageDown | Effect::PageUp => {
                let vh = self.page_pixels().map_or(0.0, |(_, h)| h as f32);
                self.relayout();
                let step = if effect == Effect::PageDown { vh } else { -vh };
                self.scroll = (self.scroll + step).clamp(0.0, (self.total_h - vh).max(0.0));
                self.page = page_at(&self.rows, self.scroll, vh).unwrap_or(self.page);
                self.draw_page()?;
            }
            Effect::Open => {
                self.ask = Some(Ask::Open);
                self.rebuild()?;
            }
        }
        Ok(())
    }

    fn fit_to(&mut self, fit: Fit) -> Result<(), UiError> {
        self.fit = fit;
        self.apply_view()
    }

    fn zoom_to(&mut self, fit: Fit) -> Result<(), UiError> {
        self.fit = fit;
        self.scale = self.scale_for(self.page);
        self.relayout();
        let target = crate::state::scroll_to_page(&self.rows, self.page);
        self.scroll = target.min((self.total_h - 1.0).max(0.0));
        self.request_visible(&self.rows.clone());
        self.draw_page()
    }

    fn rotate(&mut self) -> Result<(), UiError> {
        self.rot = self.rot.next();
        self.apply_view()
    }

    fn next_hit(&mut self) -> Result<(), UiError> {
        if self.hits.is_empty() {
            return Ok(());
        }
        self.current_hit = (self.current_hit + 1) % self.hits.len();
        self.goto_hit()
    }

    fn prev_hit(&mut self) -> Result<(), UiError> {
        if self.hits.is_empty() {
            return Ok(());
        }
        self.current_hit = (self.current_hit + self.hits.len() - 1) % self.hits.len();
        self.goto_hit()
    }

    fn goto_hit(&mut self) -> Result<(), UiError> {
        if let Some(hit) = self.hits.get(self.current_hit) {
            self.goto(hit.page)?;
        }
        Ok(())
    }

    /// The actions of the widget tree.
    pub fn actions(&mut self, actions: Vec<Action>) -> Result<(), UiError> {
        for a in actions {
            let activated = matches!(a.kind, ActionKind::Activated | ActionKind::Submitted);
            let text_changed = matches!(a.kind, ActionKind::TextChanged);
            let n = &self.nodes;
            let node = Some(a.node);
            if node == n.open {
                self.ask = Some(Ask::Open);
                self.rebuild()?;
            } else if node == n.zoom_out {
                self.zoom_to(Fit::Zoom(zoom_step(self.percent(), -1)))?;
            } else if node == n.zoom_in {
                self.zoom_to(Fit::Zoom(zoom_step(self.percent(), 1)))?;
            } else if node == n.fit_width {
                self.fit_to(Fit::Width)?;
            } else if node == n.fit_page {
                self.fit_to(Fit::Page)?;
            } else if node == n.rotate {
                self.rotate()?;
            } else if node == n.page_field && activated {
                let page = self
                    .ui
                    .text(a.node)
                    .ok()
                    .and_then(|t| t.trim().parse::<usize>().ok())
                    .map(|p| p.saturating_sub(1))
                    .unwrap_or(self.page);
                self.goto(page)?;
            } else if node == n.search_field && text_changed {
                self.query = self.ui.text(a.node).unwrap_or("").to_owned();
                let q = self.query.clone();
                self.start_search(&q);
            } else if node == n.search_field && activated {
                self.next_hit()?;
            } else if node == n.search_close {
                self.searching = false;
                self.query.clear();
                self.hits.clear();
                self.rebuild()?;
            } else if node == n.search_next {
                self.next_hit()?;
            } else if node == n.search_prev {
                self.prev_hit()?;
            } else if node == n.open_cancel || node == n.password_cancel {
                self.ask = None;
                self.wrong_password = false;
                self.rebuild()?;
            } else if node == n.open_ok || (node == n.open_field && activated) {
                let entry = self
                    .nodes
                    .open_field
                    .and_then(|f| self.ui.text(f).ok())
                    .unwrap_or_default()
                    .to_owned();
                self.open_entry(&entry)?;
            } else if node == n.recents {
                if let ActionKind::Selected(i) = a.kind {
                    if let Some(entry) = self.recents.entries.get(i).cloned() {
                        self.open_entry(&entry)?;
                    }
                }
            } else if node == n.password_ok || (node == n.password_field && activated) {
                let pw = self
                    .nodes
                    .password_field
                    .and_then(|f| self.ui.text(f).ok())
                    .unwrap_or_default()
                    .to_owned();
                self.retry_password(&pw)?;
            } else if node == n.panel {
                if let ActionKind::Selected(i) = a.kind {
                    if let Some(page) = self.panel_pages.get(i).copied() {
                        self.goto(page)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn start_search(&mut self, query: &str) {
        if query.trim().is_empty() {
            self.hits.clear();
            self.current_hit = 0;
            return;
        }
        self.doc.request_search(query);
    }

    fn goto(&mut self, page: usize) -> Result<(), UiError> {
        if self.doc.sizes.is_empty() {
            return Ok(());
        }
        self.page = page.min(self.doc.sizes.len() - 1);
        self.relayout();
        let target = crate::state::scroll_to_page(&self.rows, self.page);
        self.scroll = target.min((self.total_h - 1.0).max(0.0));
        self.draw_page()?;
        let title = self.title();
        self.out.push(Out::Title(title));
        self.rebuild()
    }

    fn open_entry(&mut self, entry: &str) -> Result<(), UiError> {
        self.ask = None;
        self.open_failed = None;
        let (doc, err) = App::open(Some(entry), &self.areas, &self.data);
        if let Some(e) = err {
            self.doc = Doc::none();
            self.show_open_failed(e);
            return Ok(());
        }
        self.initial_page = place::page_of(entry);
        self.replace_doc(doc)?;
        Ok(())
    }

    fn retry_password(&mut self, pw: &str) -> Result<(), UiError> {
        let Some(uri) = self.doc.uri.clone() else {
            return Ok(());
        };
        let Some(path) = place::path_of(&uri, &self.areas) else {
            return Ok(());
        };
        let doc = match std::fs::read(&path) {
            Ok(bytes) => Doc::from_bytes(bytes, self.doc.uri.clone(), Some(pw)),
            Err(e) => {
                self.open_failed = Some(ReadError::of(&e));
                Doc::none()
            }
        };
        self.ask = None;
        self.wrong_password = false;
        self.replace_doc(doc)?;
        Ok(())
    }

    /// Swaps in a freshly loaded document.
    fn replace_doc(&mut self, doc: Doc) -> Result<(), UiError> {
        self.doc = doc;
        self.selection = None;
        self.hits.clear();
        self.current_hit = 0;
        self.scroll = 0.0;
        self.rows.clear();
        self.total_h = 0.0;
        self.page = 0;
        if let Some(u) = &self.doc.uri {
            self.recents.push(u);
            self.recents.save(&self.data);
        }
        self.draw_page()?;
        let title = self.title();
        self.out.push(Out::Title(title));
        self.rebuild()
    }

    /// The window was asked to close: a reader has nothing to save.
    pub fn close_requested(&mut self) {
        self.out.push(Out::Close);
    }
}

/// Blits an opaque RGBA picture into a BGRX buffer at `(ox, oy)`.
fn blit(dst: &mut [u8], vw: u32, vh: u32, ox: i32, oy: i32, img: &PageImage) {
    for y in 0..img.h {
        let dy = oy + y as i32;
        if dy < 0 {
            continue;
        }
        if dy >= vh as i32 {
            continue;
        }
        for x in 0..img.w {
            let dx = ox + x as i32;
            if dx < 0 || dx >= vw as i32 {
                continue;
            }
            let s = ((y * img.w + x) * 4) as usize;
            let d = ((dy as u32 * vw + dx as u32) * 4) as usize;
            dst[d] = img.rgba[s + 2];
            dst[d + 1] = img.rgba[s + 1];
            dst[d + 2] = img.rgba[s];
            dst[d + 3] = 255;
        }
    }
}

/// Fills a rectangle with a blended RGB colour (BGRX destination).
fn fill_rect(dst: &mut [u8], vw: u32, vh: u32, x0: f32, y0: f32, x1: f32, y1: f32, c: (u8, u8, u8)) {
    let x0 = x0.round().max(0.0) as i32;
    let y0 = y0.round().max(0.0) as i32;
    let x1 = x1.round().min(vw as f32) as i32;
    let y1 = y1.round().min(vh as f32) as i32;
    let (r, g, b) = c;
    for y in y0..y1 {
        for x in x0..x1 {
            let d = ((y as u32 * vw + x as u32) * 4) as usize;
            dst[d] = ((dst[d] as u16 * 3 + b as u16 * 7) / 10) as u8;
            dst[d + 1] = ((dst[d + 1] as u16 * 3 + g as u16 * 7) / 10) as u8;
            dst[d + 2] = ((dst[d + 2] as u16 * 3 + r as u16 * 7) / 10) as u8;
        }
    }
}