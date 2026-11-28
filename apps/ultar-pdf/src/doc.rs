//! The open document as the window sees it: its state, page sizes, outline, the caches of
//! pictures and page texts, and the channel to the render worker (`worker.rs`). Nothing here
//! waits for the worker: requests are sent, answers are taken with [`Doc::poll`].

use crate::cache::{Cache, Key};
use crate::place::ReadError;
use crate::search::Hit;
use crate::text::PageText;
use crate::worker::{self, Cmd, Evt, Failure, Source, Want};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use ultar_office::pdf::{OpenError, OutlineItem, PageImage, PdfInfo};

/// Bytes of page pictures kept (the manifest allows 120 MB for the whole app).
pub const PAGE_BUDGET: usize = 40 * 1024 * 1024;
/// Bytes of thumbnails kept.
pub const THUMB_BUDGET: usize = 8 * 1024 * 1024;
/// Page texts kept in the window.
const TEXT_CAP: usize = 96;

/// The state of the reader, one of the designed states (APP-RULES §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocState {
    NoFile,
    Opening,
    Open,
    NeedsPassword,
    /// Encrypted in a way the reader cannot open.
    Unsupported,
    Damaged,
}

/// What `poll` took from the worker.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// The state changed (opened, failed, needs a password).
    State,
    /// A page picture arrived (or a page failed).
    Picture,
    /// A thumbnail arrived.
    Thumb,
    /// The text layer of a page arrived.
    Text(usize),
    /// The hits on one page for search `id`.
    Found { id: u32, hits: Vec<Hit> },
}

/// An open document (or one that is not yet / could not be opened).
pub struct Doc {
    pub state: DocState,
    /// Why a file could not be read, when that was the reason.
    pub read_error: Option<ReadError>,
    pub info: Option<PdfInfo>,
    /// Page sizes in points (unrotated), one per page.
    pub sizes: Vec<(f32, f32)>,
    pub outline: Vec<OutlineItem>,
    /// The URI it was opened with (for the title and the mesh).
    pub uri: Option<String>,
    source: Option<Source>,
    pages: Cache,
    thumbs: Cache,
    texts: HashMap<usize, Arc<PageText>>,
    text_order: VecDeque<usize>,
    failed: HashSet<usize>,
    asked: Want,
    tx: Option<Sender<Cmd>>,
    rx: Option<Receiver<Evt>>,
}

impl Doc {
    fn new(
        state: DocState,
        uri: Option<String>,
        source: Option<Source>,
        channels: Option<(Sender<Cmd>, Receiver<Evt>)>,
    ) -> Doc {
        let (tx, rx) = match channels {
            Some((tx, rx)) => (Some(tx), Some(rx)),
            None => (None, None),
        };
        Doc {
            state,
            read_error: None,
            info: None,
            sizes: Vec::new(),
            outline: Vec::new(),
            uri,
            source,
            pages: Cache::new(PAGE_BUDGET),
            thumbs: Cache::new(THUMB_BUDGET),
            texts: HashMap::new(),
            text_order: VecDeque::new(),
            failed: HashSet::new(),
            asked: Want::default(),
            tx,
            rx,
        }
    }

    /// No document (the initial state).
    pub fn none() -> Doc {
        Doc::new(DocState::NoFile, None, None, None)
    }

    /// A document that could not be reached (the file is missing, the URI is wrong).
    pub fn unreadable(uri: Option<String>, why: ReadError) -> Doc {
        let mut doc = Doc::new(DocState::NoFile, uri, None, None);
        doc.read_error = Some(why);
        doc
    }

    /// Opens the file at `path`; the worker reads and parses it, the window does not wait.
    pub fn open_path(path: &Path, uri: Option<String>, password: Option<&str>) -> Doc {
        Doc::start(Source::Path(path.to_owned()), uri, password)
    }

    /// Opens from memory (tests, fixtures).
    pub fn from_bytes(bytes: Vec<u8>, uri: Option<String>, password: Option<&str>) -> Doc {
        Doc::start(Source::Bytes(Arc::new(bytes)), uri, password)
    }

    fn start(source: Source, uri: Option<String>, password: Option<&str>) -> Doc {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (evt_tx, evt_rx) = mpsc::channel();
        let password = password.map(str::to_owned);
        let for_worker = source.clone();
        std::thread::spawn(move || worker::run(for_worker, password, cmd_rx, evt_tx));
        Doc::new(DocState::Opening, uri, Some(source), Some((cmd_tx, evt_rx)))
    }

    /// The same file again with a password (the password dialog).
    pub fn with_password(&self, password: &str) -> Doc {
        match &self.source {
            Some(s) => Doc::start(s.clone(), self.uri.clone(), Some(password)),
            None => Doc::none(),
        }
    }

    fn send(&self, cmd: Cmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }

    /// Takes what the worker sent since the last call and applies it.
    pub fn poll(&mut self) -> Vec<Change> {
        let mut events = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(evt) = rx.try_recv() {
                events.push(evt);
            }
        }
        events.into_iter().filter_map(|e| self.apply(e)).collect()
    }

    fn apply(&mut self, evt: Evt) -> Option<Change> {
        match evt {
            Evt::Opened(Ok(o)) => {
                self.sizes = o.sizes;
                self.outline = o.outline;
                self.info = Some(o.info);
                self.state = DocState::Open;
                Some(Change::State)
            }
            Evt::Opened(Err(Failure::Pdf(OpenError::Password))) => self.fail(DocState::NeedsPassword),
            Evt::Opened(Err(Failure::Pdf(OpenError::Unsupported))) => self.fail(DocState::Unsupported),
            Evt::Opened(Err(Failure::Pdf(OpenError::Damaged))) => self.fail(DocState::Damaged),
            Evt::Opened(Err(Failure::Read(e))) => {
                self.read_error = Some(e);
                self.fail(DocState::NoFile)
            }
            Evt::Rendered { page, scale, img } => {
                self.pages.insert(Key::new(page, scale), img);
                Some(Change::Picture)
            }
            Evt::Thumb { page, scale, img } => {
                self.thumbs.insert(Key::new(page, scale), img);
                Some(Change::Thumb)
            }
            Evt::Failed { page } => {
                self.failed.insert(page);
                Some(Change::Picture)
            }
            Evt::Text { page, text } => {
                self.keep_text(page, text);
                Some(Change::Text(page))
            }
            Evt::Found { id, hits, .. } => Some(Change::Found { id, hits }),
        }
    }

    fn fail(&mut self, state: DocState) -> Option<Change> {
        self.state = state;
        Some(Change::State)
    }

    fn keep_text(&mut self, page: usize, text: Arc<PageText>) {
        if self.texts.insert(page, text).is_none() {
            self.text_order.push_back(page);
        }
        while self.text_order.len() > TEXT_CAP {
            if let Some(old) = self.text_order.pop_front() {
                self.texts.remove(&old);
            }
        }
    }

    /// Tells the worker what is wanted now, most important first; what is cached (or
    /// failed) is left out, and an unchanged wish is not sent again.
    pub fn want(&mut self, renders: Vec<(usize, f32)>, texts: Vec<usize>, thumbs: Vec<(usize, f32)>) {
        let want = Want {
            renders: renders
                .into_iter()
                .filter(|&(p, s)| !self.pages.has(Key::new(p, s)) && !self.failed.contains(&p))
                .collect(),
            texts: texts
                .into_iter()
                .filter(|p| !self.texts.contains_key(p))
                .collect(),
            thumbs: thumbs
                .into_iter()
                .filter(|&(p, s)| !self.thumbs.has(Key::new(p, s)) && !self.failed.contains(&p))
                .collect(),
        };
        if want != self.asked {
            self.asked = want.clone();
            self.send(Cmd::Want(want));
        }
    }

    /// Starts search `id` for `tokens`, beginning at page `start`.
    pub fn search(&self, id: u32, tokens: Vec<String>, start: usize) {
        self.send(Cmd::Search { id, tokens, start });
    }

    /// Something asked for has not arrived yet (the window polls faster meanwhile).
    pub fn pending(&self) -> bool {
        if self.state == DocState::Opening {
            return true;
        }
        let a = &self.asked;
        a.renders
            .iter()
            .any(|&(p, s)| !self.pages.has(Key::new(p, s)) && !self.failed.contains(&p))
            || a.texts.iter().any(|p| !self.texts.contains_key(p))
            || a.thumbs
                .iter()
                .any(|&(p, s)| !self.thumbs.has(Key::new(p, s)) && !self.failed.contains(&p))
    }

    /// The picture of `page` at exactly `scale`.
    pub fn image(&mut self, page: usize, scale: f32) -> Option<Arc<PageImage>> {
        self.pages.get(Key::new(page, scale))
    }

    /// The newest picture of `page` at any scale (drawn stretched until the right one arrives).
    pub fn stale_image(&self, page: usize) -> Option<Arc<PageImage>> {
        self.pages.newest_of(page)
    }

    /// The newest thumbnail of `page`.
    pub fn thumb(&self, page: usize) -> Option<Arc<PageImage>> {
        self.thumbs.newest_of(page)
    }

    /// The page's text layer, once the worker delivered it.
    pub fn text(&self, page: usize) -> Option<Arc<PageText>> {
        self.texts.get(&page).cloned()
    }

    /// The page could not be drawn.
    pub fn page_failed(&self, page: usize) -> bool {
        self.failed.contains(&page)
    }

    /// Bytes held by page pictures and thumbnails.
    pub fn cache_bytes(&self) -> usize {
        self.pages.bytes() + self.thumbs.bytes()
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        // The worker ends by itself; the window never waits for a render in progress.
        self.send(Cmd::Shutdown);
    }
}
