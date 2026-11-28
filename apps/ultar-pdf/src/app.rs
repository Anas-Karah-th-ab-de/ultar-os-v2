//! The reader window's state and what moves it: opening, the view (zoom, fit, rotation,
//! scroll, current page), what the worker delivers and what is asked of it. The tree is built
//! in `build.rs`, the pictures in `paint.rs`, keys and pointer in `input.rs`.

use crate::doc::{Change, Doc, DocState};
use crate::layout::Layout;
use crate::place::{self, ReadError, Recents};
use crate::search::{Hit, Search};
use crate::state::{fit_page_scale, fit_width_scale, page_display, zoom_step, Fit, Rot};
use crate::touch::Touches;
use std::path::{Path, PathBuf};
use ultar_ui::tokens::SizeClass;
use ultar_ui::ui::{NodeId, Ui, UiError};

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
pub enum Ask {
    /// Path or files:// URI of a PDF to open.
    Open,
    /// The password of an encrypted PDF.
    Password,
}

/// What the start-side panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelMode {
    Pages,
    Outline,
}

/// A selected run of words on one page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub page: usize,
    pub first: usize,
    pub last: usize,
}

/// A drag that is selecting text.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Drag {
    pub page: usize,
    pub anchor: usize,
    pub start: (f32, f32),
    pub moved: bool,
}

/// The tool bar's nodes.
#[derive(Default)]
pub(crate) struct BarNodes {
    pub open: Option<NodeId>,
    pub find: Option<NodeId>,
    pub zoom_out: Option<NodeId>,
    pub zoom_in: Option<NodeId>,
    pub zoom_label: Option<NodeId>,
    pub fit_width: Option<NodeId>,
    pub fit_page: Option<NodeId>,
    pub rotate: Option<NodeId>,
    pub page_field: Option<NodeId>,
    pub page_count: Option<NodeId>,
}

/// The start-side panel's nodes (wide windows).
#[derive(Default)]
pub(crate) struct SideNodes {
    pub pages_tab: Option<NodeId>,
    pub outline_tab: Option<NodeId>,
    pub thumbs: Option<NodeId>,
    pub outline: Option<NodeId>,
}

/// The search bar's nodes.
#[derive(Default)]
pub(crate) struct FinderNodes {
    pub search_field: Option<NodeId>,
    pub search_next: Option<NodeId>,
    pub search_prev: Option<NodeId>,
    pub search_count: Option<NodeId>,
    pub search_close: Option<NodeId>,
}

/// The nodes of the open and password questions and the empty state.
#[derive(Default)]
pub(crate) struct DialogNodes {
    pub empty_open: Option<NodeId>,
    pub dialog: Option<NodeId>,
    pub open_field: Option<NodeId>,
    pub open_ok: Option<NodeId>,
    pub open_cancel: Option<NodeId>,
    pub recents: Option<NodeId>,
    pub password_field: Option<NodeId>,
    pub password_ok: Option<NodeId>,
    pub password_cancel: Option<NodeId>,
}

/// The nodes the reader needs to find again.
#[derive(Default)]
pub(crate) struct Nodes {
    pub bar: BarNodes,
    pub side: SideNodes,
    pub finder: FinderNodes,
    pub dialogs: DialogNodes,
    pub page: Option<NodeId>,
}

/// Where the reader looks: zoom, scroll, rotation, the page and the page rows.
pub(crate) struct View {
    pub fit: Fit,
    pub scale: f32,
    pub scroll: f32,
    pub scroll_x: f32,
    pub rot: Rot,
    pub page: usize,
    pub start_page: usize,
    pub layout: Layout,
}

/// The start-side panel's state.
pub(crate) struct Rail {
    pub mode: PanelMode,
    pub thumbs: Layout,
    pub scroll: f32,
}

/// The search bar's state.
pub(crate) struct Find {
    pub open: bool,
    pub hits: Search,
    pub seq: u32,
}

/// Selecting text and the fingers on the page.
pub(crate) struct Pick {
    pub selection: Option<Selection>,
    pub drag: Option<Drag>,
    pub copy_page: Option<usize>,
    pub touches: Touches,
    pub pinch_from: u16,
}

/// The reader: its tree, its document and its view state.
pub struct App {
    pub ui: Ui,
    pub doc: Doc,
    pub(crate) areas: PathBuf,
    pub(crate) data: PathBuf,
    pub(crate) recents: Recents,
    pub(crate) nodes: Nodes,
    pub(crate) view: View,
    pub(crate) rail: Rail,
    pub(crate) find: Find,
    pub(crate) pick: Pick,
    pub(crate) ask: Option<Ask>,
    pub(crate) tried_password: bool,
    pub(crate) out: Vec<Out>,
}

impl App {
    /// Resolves `uri` (`files://…`) and starts opening the file (the worker reads it); a URI
    /// that names no file below the areas gives a document that says so.
    pub fn open(uri: Option<&str>, areas: &Path) -> Doc {
        let Some(uri) = uri else {
            return Doc::none();
        };
        match place::path_of(uri, areas) {
            Some(path) => Doc::open_path(&path, Some(uri.to_owned()), None),
            None => Doc::unreadable(Some(uri.to_owned()), ReadError::Missing),
        }
    }

    /// A reader over `ui` for `doc`; `areas` binds the user's places, `data` is `app:data`.
    pub fn new(ui: Ui, doc: Doc, areas: PathBuf, data: PathBuf) -> Result<App, UiError> {
        let mut recents = Recents::load(&data);
        let start_page = doc.uri.as_deref().map_or(0, place::page_of);
        if doc.read_error.is_none() {
            if let Some(u) = &doc.uri {
                recents.push(place::without_page(u));
                recents.save(&data);
            }
        }
        let mut app = App {
            ui,
            doc,
            areas,
            data,
            recents,
            nodes: Nodes::default(),
            view: View {
                fit: Fit::Width,
                scale: 1.0,
                scroll: 0.0,
                scroll_x: 0.0,
                rot: Rot::None,
                page: start_page,
                start_page,
                layout: Layout::default(),
            },
            rail: Rail {
                mode: PanelMode::Pages,
                thumbs: Layout::default(),
                scroll: 0.0,
            },
            find: Find {
                open: false,
                hits: Search::default(),
                seq: 0,
            },
            pick: Pick {
                selection: None,
                drag: None,
                copy_page: None,
                touches: Touches::default(),
                pinch_from: 100,
            },
            ask: None,
            tried_password: false,
            out: Vec::new(),
        };
        app.rebuild()?;
        let title = app.title();
        app.out.push(Out::Title(title));
        Ok(app)
    }

    /// The window title: the document's name and the app name.
    pub fn title(&self) -> String {
        let tr = self.ui.translator();
        let name = self
            .doc
            .uri
            .as_deref()
            .map(place::without_page)
            .and_then(|u| u.rsplit('/').next().map(place::decode_name))
            .filter(|n| !n.is_empty());
        let app = tr.t("app.pdf.name");
        match name {
            Some(n) => format!("{n} – {app}"),
            None => app.into_owned(),
        }
    }

    /// The URI the mesh should report (document + page), or `None` without an open document.
    pub fn activity_uri(&self) -> Option<String> {
        if self.doc.state != DocState::Open {
            return None;
        }
        let uri = self.doc.uri.as_deref()?;
        Some(format!(
            "{}#page={}",
            place::without_page(uri),
            self.view.page + 1
        ))
    }

    /// What the window has to do (taken once).
    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    /// The window was asked to close: a reader has nothing to save.
    pub fn close_requested(&mut self) {
        self.out.push(Out::Close);
    }

    /// The window wants to know whether to poll quickly (work is under way).
    pub fn busy(&self) -> bool {
        self.doc.pending() || (self.find.open && self.find.hits.active() && !self.find.hits.done())
    }

    /// The size class of the window.
    pub fn size_class(&self) -> SizeClass {
        self.ui.size_class()
    }

    /// The current page (from 0).
    pub fn current_page(&self) -> usize {
        self.view.page
    }

    /// The pages of the document.
    pub fn page_count(&self) -> usize {
        self.doc.sizes.len()
    }

    /// The zoom in percent (100 = one point per logical pixel).
    pub fn zoom_percent(&self) -> u16 {
        let s = self.ui.env().scale.max(0.01);
        (self.view.scale / s * 100.0).round().clamp(1.0, 9999.0) as u16
    }

    /// The zoom mode.
    pub fn fit(&self) -> Fit {
        self.view.fit
    }

    /// The user's rotation.
    pub fn rotation(&self) -> Rot {
        self.view.rot
    }

    /// The vertical scroll in device pixels.
    pub fn scroll_offset(&self) -> f32 {
        self.view.scroll
    }

    /// All search hits found so far.
    pub fn hits(&self) -> &[Hit] {
        self.find.hits.hits()
    }

    /// The selected text, if any is selected and its page text is loaded.
    pub fn selected_text(&self) -> Option<String> {
        let s = self.pick.selection?;
        let text = self.doc.text(s.page)?;
        Some(text.text(s.first, s.last))
    }

    /// The size of the page area in device pixels.
    pub(crate) fn view_px(&mut self) -> (f32, f32) {
        self.ui.layout();
        let s = self.ui.env().scale;
        self.nodes
            .page
            .and_then(|n| self.ui.rect(n))
            .map_or((0.0, 0.0), |r| (r.w * s, r.h * s))
    }

    /// The scale (points → device pixels) the current zoom mode gives for the current page.
    fn scale_for(&mut self) -> f32 {
        let (vw, vh) = self.view_px();
        let size = self
            .doc
            .sizes
            .get(self.view.page)
            .copied()
            .unwrap_or((595.0, 842.0));
        let (w, h) = page_display(size.0, size.1, self.view.rot);
        let scale = match self.view.fit {
            Fit::Width => fit_width_scale(vw, w),
            Fit::Page => fit_page_scale(vw, vh, w, h),
            Fit::Zoom(p) => p as f32 / 100.0 * self.ui.env().scale,
        };
        // three decimals: the same view always asks for the same picture
        (scale * 1000.0).round() / 1000.0
    }

    /// Keeps the scroll offsets inside the content.
    pub(crate) fn clamp_scroll(&mut self) {
        let (vw, vh) = self.view_px();
        self.view.scroll = self.view.scroll.clamp(0.0, self.view.layout.max_scroll(vh));
        self.view.scroll_x = self
            .view
            .scroll_x
            .clamp(0.0, self.view.layout.max_scroll_x(vw));
    }

    /// Recomputes scale and layout for the current view (the reading position stays), asks the
    /// worker for what is visible and paints.
    pub fn refresh(&mut self) -> Result<(), UiError> {
        if self.doc.state == DocState::Open {
            let anchor = if self.view.layout.rows.is_empty() {
                (
                    self.view.page.min(self.doc.sizes.len().saturating_sub(1)),
                    0.0,
                )
            } else {
                self.view.layout.anchor(self.view.scroll)
            };
            self.view.scale = self.scale_for();
            self.view.layout = Layout::new(&self.doc.sizes, self.view.scale, self.view.rot);
            self.view.scroll = self.view.layout.scroll_of(anchor);
            self.clamp_scroll();
            self.build_thumbs();
        }
        self.after_move(false)
    }

    /// After the view moved: the current page, the chrome, the wishes to the worker, the picture.
    pub(crate) fn after_move(&mut self, follow: bool) -> Result<(), UiError> {
        if self.doc.state == DocState::Open {
            self.clamp_scroll();
            if follow {
                let (_, vh) = self.view_px();
                self.view.page = self
                    .view
                    .layout
                    .page_at(self.view.scroll, vh)
                    .unwrap_or(self.view.page);
            }
            self.follow_thumbs();
            self.sync_chrome()?;
            self.update_wants();
        }
        self.draw()
    }

    /// Tells the worker what the view needs: the visible pages first (the middle one first),
    /// then the neighbours; texts of the visible pages; thumbnails of the panel.
    pub(crate) fn update_wants(&mut self) {
        if self.doc.state != DocState::Open {
            return;
        }
        let (_, vh) = self.view_px();
        let vis = self.view.layout.visible(self.view.scroll, vh);
        let mid = self
            .view
            .layout
            .page_at(self.view.scroll, vh)
            .unwrap_or(self.view.page);
        let mut order: Vec<usize> = vis.clone().collect();
        order.sort_by_key(|p| p.abs_diff(mid));
        let n = self.doc.sizes.len();
        let mut texts = order.clone();
        for p in [vis.end, vis.start.wrapping_sub(1)] {
            if p < n {
                order.push(p);
            }
        }
        if let Some(p) = self.pick.copy_page.filter(|p| *p < n) {
            texts.push(p);
        }
        // Ask only for what the cache can hold together: at a high zoom a picture is big, and
        // asking for more than fits would render pages that are thrown out again at once.
        let mut room = crate::doc::PAGE_BUDGET;
        let mut renders: Vec<(usize, f32)> = Vec::new();
        for p in order {
            let bytes = self.picture_bytes(p);
            if renders.is_empty() || bytes <= room {
                room = room.saturating_sub(bytes);
                renders.push((p, self.view.scale));
            }
        }
        let thumbs = self.wanted_thumbs();
        self.doc.want(renders, texts, thumbs);
    }

    /// The bytes the picture of `page` takes in the cache at the current scale (the worker draws
    /// at most `MAX_PAGE_PIXELS`, the window stretches the picture beyond that).
    fn picture_bytes(&self, page: usize) -> usize {
        let size = self.doc.sizes.get(page).copied().unwrap_or((595.0, 842.0));
        let (w, h) = page_display(size.0, size.1, self.view.rot);
        let pixels = (w * self.view.scale) * (h * self.view.scale);
        (pixels.min(crate::worker::MAX_PAGE_PIXELS) as usize).saturating_mul(4)
    }

    /// Applies a zoom mode and keeps the reading position.
    pub fn set_fit(&mut self, fit: Fit) -> Result<(), UiError> {
        self.view.fit = fit;
        self.refresh()
    }

    /// One zoom step in (`dir > 0`) or out.
    pub fn zoom_by(&mut self, dir: i8) -> Result<(), UiError> {
        let to = zoom_step(self.zoom_percent(), dir);
        self.set_fit(Fit::Zoom(to))
    }

    /// Turns the pages a quarter clockwise.
    pub fn rotate(&mut self) -> Result<(), UiError> {
        self.view.rot = self.view.rot.next();
        self.pick.selection = None;
        self.refresh()
    }

    /// Scrolls so that `page` (from 0) is at the top.
    pub fn goto(&mut self, page: usize) -> Result<(), UiError> {
        if self.doc.sizes.is_empty() {
            return Ok(());
        }
        self.view.page = page.min(self.doc.sizes.len() - 1);
        self.view.scroll = self.view.layout.scroll_to(self.view.page);
        self.after_move(false)
    }

    /// Scrolls by device pixels (the wheel, a touch pan, the keys).
    pub(crate) fn scroll_by(&mut self, dx: f32, dy: f32) -> Result<(), UiError> {
        self.view.scroll += dy;
        self.view.scroll_x += dx;
        self.after_move(true)
    }

    /// The document changed its state (opened, failed, needs a password).
    fn state_changed(&mut self) -> Result<(), UiError> {
        match self.doc.state {
            DocState::Open => {
                self.view.page = self
                    .view
                    .start_page
                    .min(self.doc.sizes.len().saturating_sub(1));
                self.view.layout = Layout::default();
                self.rebuild()?;
            }
            DocState::NeedsPassword => {
                self.ask = Some(Ask::Password);
                self.rebuild()?;
            }
            _ => {
                self.ask = None;
                self.rebuild()?;
            }
        }
        let title = self.title();
        self.out.push(Out::Title(title));
        Ok(())
    }

    /// Takes what the worker sent and updates the view; call it every turn of the loop.
    pub fn poll(&mut self) -> Result<(), UiError> {
        let mut redraw = false;
        for change in self.doc.poll() {
            match change {
                Change::State => return self.state_changed(),
                Change::Picture | Change::Thumb => redraw = true,
                Change::Text(page) => {
                    redraw = true;
                    self.text_arrived(page);
                }
                Change::Found { id, hits } => {
                    redraw = true;
                    self.found(id, hits)?;
                }
            }
        }
        if redraw && self.doc.state == DocState::Open {
            self.update_wants();
            self.draw()?;
        }
        Ok(())
    }

    fn text_arrived(&mut self, page: usize) {
        if self.pick.copy_page == Some(page) {
            self.pick.copy_page = None;
            if let Some(text) = self.doc.text(page) {
                self.out.push(Out::Copy(text.all()));
            }
        }
    }
}
