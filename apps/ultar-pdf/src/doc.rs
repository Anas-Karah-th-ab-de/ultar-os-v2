//! The open document: the bytes of the PDF, the render worker that owns the `PdfDoc`, and the
//! picture cache. Rendering never blocks the window: the worker renders pages on request and
//! sends the finished pictures back; the cache keeps the newest ones within a memory limit, so
//! a long document stays smooth.

use crate::place::ReadError;
use crate::state::{find_hits, rotate_rgba, Hit, Rot};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use ultar_office::pdf::{Line, OpenError, PageImage, PdfDoc, PdfInfo};

/// What the worker found out while opening.
#[derive(Debug, Clone)]
pub struct Opened {
    pub info: PdfInfo,
    pub sizes: Vec<(f32, f32)>,
    /// Bookmark titles with their page (empty when the document has none).
    pub outline: Vec<(usize, String)>,
}

/// A request to the worker.
enum Cmd {
    Render {
        page: usize,
        scale: f32,
        rot: Rot,
    },
    Words {
        page: usize,
    },
    Search {
        query: String,
    },
    Shutdown,
}

/// What the worker answered.
pub enum Evt {
    Opened(Result<Opened, OpenError>),
    Rendered {
        page: usize,
        scale: f32,
        rot: Rot,
        img: Arc<PageImage>,
    },
    Words {
        page: usize,
        lines: Arc<Vec<Line>>,
    },
    Search {
        hits: Vec<Hit>,
    },
}

/// The state of the reader, one of the designed states (APP-RULES §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocState {
    NoFile,
    Opening,
    Open,
    NeedsPassword,
    Damaged,
}

/// Pictures older than this many bytes are evicted first.
const CACHE_BUDGET: usize = 48 * 1024 * 1024;
/// Text pages kept for highlighting a search.
const WORDS_CAP: usize = 64;

/// The least-recently-used picture cache with a byte budget.
struct Cache {
    budget: usize,
    bytes: usize,
    map: HashMap<(usize, u32, u8), Arc<PageImage>>,
    order: Vec<(usize, u32, u8)>,
}

impl Cache {
    fn new(budget: usize) -> Cache {
        Cache {
            budget,
            bytes: 0,
            map: HashMap::new(),
            order: Vec::new(),
        }
    }

    fn key(page: usize, scale: f32, rot: Rot) -> (usize, u32, u8) {
        (page, scale.to_bits(), rot.turns())
    }

    fn get(&self, k: &(usize, u32, u8)) -> Option<Arc<PageImage>> {
        self.map.get(k).cloned()
    }

    fn insert(&mut self, k: (usize, u32, u8), img: Arc<PageImage>) {
        let size = img.rgba.len();
        self.map.insert(k, img);
        self.order.push(k);
        self.bytes += size;
        while self.bytes > self.budget {
            let Some(old) = self.order.first().copied() else {
                break;
            };
            self.order.remove(0);
            if let Some(dropped) = self.map.remove(&old) {
                self.bytes = self.bytes.saturating_sub(dropped.rgba.len());
            }
        }
    }
}

/// An open document (or one that is not yet / could not be opened).
pub struct Doc {
    /// The state for the view.
    pub state: DocState,
    /// The document's info after a successful open.
    pub info: Option<PdfInfo>,
    /// Page sizes in points (unrotated), one per page.
    pub sizes: Vec<(f32, f32)>,
    /// Bookmark titles with their page (empty when the document has none).
    pub outline: Vec<(usize, String)>,
    /// The URI it was opened with (for meshing and the title).
    pub uri: Option<String>,
    cache: Cache,
    words: HashMap<usize, Arc<Vec<Line>>>,
    tx: Option<Sender<Cmd>>,
    rx: Option<Receiver<Evt>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Doc {
    /// No document (the initial state).
    pub fn none() -> Doc {
        Doc {
            state: DocState::NoFile,
            info: None,
            sizes: Vec::new(),
            outline: Vec::new(),
            uri: None,
            cache: Cache::new(CACHE_BUDGET),
            words: HashMap::new(),
            tx: None,
            rx: None,
            worker: None,
        }
    }

    /// Opens `path` (a `files://` URI, so the mesh can hand the page over).
    pub fn open(
        path: &Path,
        uri: Option<String>,
        areas: &Path,
        password: Option<&str>,
    ) -> Result<Doc, ReadError> {
        let bytes = std::fs::read(path).map_err(|e| ReadError::of(&e))?;
        let _ = areas;
        Ok(Doc::from_bytes(bytes, uri, password))
    }

    /// Opens from memory (tests, fixtures).
    pub fn from_bytes(bytes: Vec<u8>, uri: Option<String>, password: Option<&str>) -> Doc {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (evt_tx, evt_rx) = mpsc::channel();
        let password = password.map(str::to_owned);
        let bytes = Arc::new(bytes);
        let worker_bytes = bytes.clone();
        let worker = std::thread::spawn(move || worker(worker_bytes, password, cmd_rx, evt_tx));
        Doc {
            state: DocState::Opening,
            info: None,
            sizes: Vec::new(),
            outline: Vec::new(),
            uri,
            cache: Cache::new(CACHE_BUDGET),
            words: HashMap::new(),
            tx: Some(cmd_tx),
            rx: Some(evt_rx),
            worker: Some(worker),
        }
    }

    /// Takes the events the worker sent since the last call (the open result first).
    pub fn poll(&mut self) -> Vec<Evt> {
        let mut out = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(evt) = rx.try_recv() {
                out.push(evt);
            }
        }
        out
    }

    /// Asks the worker to render `page`.
    pub fn request_render(&mut self, page: usize, scale: f32, rot: Rot) {
        let _ = self
            .tx
            .as_ref()
            .map(|tx| tx.send(Cmd::Render { page, scale, rot }));
    }

    /// Asks the worker for the words (lines) of `page` (search highlight, selection).
    pub fn request_words(&mut self, page: usize) {
        let _ = self.tx.as_ref().map(|tx| tx.send(Cmd::Words { page }));
    }

    /// Asks the worker to search the whole document.
    pub fn request_search(&mut self, query: &str) {
        let _ = self
            .tx
            .as_ref()
            .map(|tx| tx.send(Cmd::Search { query: query.to_owned() }));
    }

    /// Applies one worker event to the document's state and caches (the app handles
    /// `Evt::Search` itself first, everything else is internal).
    pub fn accept(&mut self, evt: Evt) {
        match evt {
            Evt::Opened(res) => match res {
                Ok(Opened { info, sizes, outline }) => {
                    self.info = Some(info);
                    self.sizes = sizes;
                    self.outline = outline;
                    self.state = DocState::Open;
                }
                Err(OpenError::Password) => self.state = DocState::NeedsPassword,
                Err(OpenError::Damaged) => self.state = DocState::Damaged,
            },
            Evt::Rendered {
                page,
                scale,
                rot,
                img,
            } => {
                let key = Cache::key(page, scale, rot);
                self.cache.insert(key, img);
            }
            Evt::Words { page, lines } => {
                if self.words.len() < WORDS_CAP {
                    self.words.insert(page, lines);
                }
            }
            Evt::Search { .. } => {}
        }
    }

    /// A rendered page from the cache, if present.
    pub fn image(&self, page: usize, scale: f32, rot: Rot) -> Option<Arc<PageImage>> {
        self.cache.get(&Cache::key(page, scale, rot))
    }

    /// The cached lines of a page, if the worker has delivered them.
    pub fn lines(&self, page: usize) -> Option<&Arc<Vec<Line>>> {
        self.words.get(&page)
    }
}

/// The worker: opens the PDF once and answers render/word/search requests.
fn worker(
    bytes: Arc<Vec<u8>>,
    password: Option<String>,
    rx: Receiver<Cmd>,
    tx: Sender<Evt>,
) {
    let doc = match password {
        Some(pw) if !pw.is_empty() => PdfDoc::open_with_password_kind((*bytes).clone(), &pw),
        _ => PdfDoc::open_kind((*bytes).clone()),
    };
    let doc = match doc {
        Ok(d) => d,
        Err(e) => {
            let _ = tx.send(Evt::Opened(Err(e)));
            return;
        }
    };
    let info = doc.info();
    let sizes = (0..info.pages).map(|i| doc.page_size(i)).collect();
    let outline = doc.outline();
    if tx
        .send(Evt::Opened(Ok(Opened {
            info,
            sizes,
            outline,
        })))
        .is_err()
    {
        return;
    }
    let page_count = doc.pages();
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Shutdown => break,
            Cmd::Render { page, scale, rot } => {
                if let Some(img) = doc.render_at(page, scale) {
                    let img = if rot.turns() == 0 {
                        img
                    } else {
                        let (w, h, rgba) = rotate_rgba(img.w, img.h, &img.rgba, rot.turns());
                        PageImage { w, h, rgba }
                    };
                    let _ = tx.send(Evt::Rendered {
                        page,
                        scale,
                        rot,
                        img: Arc::new(img),
                    });
                }
            }
            Cmd::Words { page } => {
                let lines = Arc::new(doc.page_lines(page));
                let _ = tx.send(Evt::Words { page, lines });
            }
            Cmd::Search { query } => {
                let mut pages = Vec::with_capacity(page_count.min(4096));
                for i in 0..page_count {
                    pages.push(doc.page_words(i));
                }
                let hits = find_hits(&pages, &query);
                let _ = tx.send(Evt::Search { hits });
            }
        }
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Shutdown);
        }
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}