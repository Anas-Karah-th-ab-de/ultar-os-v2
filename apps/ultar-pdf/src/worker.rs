//! The render worker: one thread owns the opened PDF, reads the file, and answers requests
//! for page pictures, thumbnails, page text and search. The window never waits for it: it
//! sends what it wants ([`Want`], newest wins, visible pages first) and takes the answers
//! with `try_recv`.

use crate::place::ReadError;
use crate::search::{find_in_text, Hit};
use crate::text::PageText;
use std::collections::{HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use ultar_office::pdf::{OpenError, OutlineItem, PageImage, PdfDoc, PdfInfo};

/// Most pixels of one page picture (a larger zoom is drawn from a smaller picture).
pub const MAX_PAGE_PIXELS: f32 = 4_000_000.0;
/// Most pixels of one thumbnail.
pub const MAX_THUMB_PIXELS: f32 = 120_000.0;
/// Largest file the reader opens.
pub const MAX_FILE: u64 = 512 * 1024 * 1024;
/// Page texts the worker keeps.
const TEXT_CAP: usize = 128;

/// Where the bytes of the document come from.
#[derive(Clone, Debug)]
pub enum Source {
    Path(PathBuf),
    Bytes(Arc<Vec<u8>>),
}

/// Why the document did not open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Read(ReadError),
    Pdf(OpenError),
}

/// What the worker found out while opening.
#[derive(Debug, Clone)]
pub struct Opened {
    pub info: PdfInfo,
    pub sizes: Vec<(f32, f32)>,
    pub outline: Vec<OutlineItem>,
}

/// What the window wants, most important first. A new `Want` replaces the old one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Want {
    /// (page, scale) pictures.
    pub renders: Vec<(usize, f32)>,
    /// Pages whose text layer is needed (selection, highlights).
    pub texts: Vec<usize>,
    /// (page, scale) thumbnails.
    pub thumbs: Vec<(usize, f32)>,
}

pub enum Cmd {
    Want(Want),
    /// Search `tokens` over all pages, beginning at page `start` and wrapping around.
    Search {
        id: u32,
        tokens: Vec<String>,
        start: usize,
    },
    Shutdown,
}

pub enum Evt {
    Opened(Result<Opened, Failure>),
    Rendered {
        page: usize,
        scale: f32,
        img: Arc<PageImage>,
    },
    Thumb {
        page: usize,
        scale: f32,
        img: Arc<PageImage>,
    },
    /// The page could not be drawn (damaged content).
    Failed {
        page: usize,
    },
    Text {
        page: usize,
        text: Arc<PageText>,
    },
    /// The hits on one page of search `id` (empty when the page has none).
    Found {
        id: u32,
        page: usize,
        hits: Vec<Hit>,
    },
}

struct Scan {
    id: u32,
    tokens: Vec<String>,
    order: Vec<usize>,
    next: usize,
}

struct Worker {
    doc: PdfDoc,
    rx: Receiver<Cmd>,
    tx: Sender<Evt>,
    want: Want,
    scan: Option<Scan>,
    texts: HashMap<usize, Arc<PageText>>,
    text_order: VecDeque<usize>,
}

fn load(source: Source) -> Result<Vec<u8>, Failure> {
    match source {
        Source::Bytes(b) => Ok((*b).clone()),
        Source::Path(path) => {
            let meta = std::fs::metadata(&path).map_err(|e| Failure::Read(ReadError::of(&e)))?;
            if meta.len() > MAX_FILE {
                return Err(Failure::Read(ReadError::Io));
            }
            std::fs::read(&path).map_err(|e| Failure::Read(ReadError::of(&e)))
        }
    }
}

/// The worker thread: opens the document, then serves requests until the window drops it.
pub fn run(source: Source, password: Option<String>, rx: Receiver<Cmd>, tx: Sender<Evt>) {
    let opened = load(source).and_then(|bytes| {
        let pw = password.as_deref().unwrap_or("");
        // A damaged file may panic deep inside the parser: that is "damaged", not a crash.
        catch_unwind(AssertUnwindSafe(|| {
            PdfDoc::open_with_password_kind(bytes, pw)
        }))
        .unwrap_or(Err(OpenError::Damaged))
        .map_err(Failure::Pdf)
    });
    let doc = match opened {
        Ok(doc) => doc,
        Err(e) => {
            let _ = tx.send(Evt::Opened(Err(e)));
            return;
        }
    };
    let info = doc.info();
    let sizes = (0..info.pages).map(|i| doc.page_size(i)).collect();
    let outline = catch_unwind(AssertUnwindSafe(|| doc.outline())).unwrap_or_default();
    let opened = Opened {
        info,
        sizes,
        outline,
    };
    if tx.send(Evt::Opened(Ok(opened))).is_err() {
        return;
    }
    Worker {
        doc,
        rx,
        tx,
        want: Want::default(),
        scan: None,
        texts: HashMap::new(),
        text_order: VecDeque::new(),
    }
    .serve();
}

impl Worker {
    fn idle(&self) -> bool {
        self.want.renders.is_empty()
            && self.want.texts.is_empty()
            && self.want.thumbs.is_empty()
            && self.scan.is_none()
    }

    fn serve(mut self) {
        loop {
            // Commands first, so a new `Want` cancels work nobody waits for any more.
            let cmd = if self.idle() {
                match self.rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => return,
                }
            } else {
                match self.rx.try_recv() {
                    Ok(c) => Some(c),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            };
            match cmd {
                Some(Cmd::Shutdown) => return,
                Some(Cmd::Want(w)) => self.want = w,
                Some(Cmd::Search { id, tokens, start }) => self.begin_scan(id, tokens, start),
                None => {
                    if !self.step() {
                        return;
                    }
                }
            }
        }
    }

    fn begin_scan(&mut self, id: u32, tokens: Vec<String>, start: usize) {
        let n = self.doc.pages();
        self.scan = (!tokens.is_empty() && n > 0).then(|| {
            let start = start.min(n - 1);
            Scan {
                id,
                tokens,
                order: (start..n).chain(0..start).collect(),
                next: 0,
            }
        });
    }

    /// One unit of work (a picture, a text, a thumbnail or a page of the search); `false`
    /// when the window is gone.
    fn step(&mut self) -> bool {
        if !self.want.renders.is_empty() {
            let (page, scale) = self.want.renders.remove(0);
            return self.draw(page, scale, MAX_PAGE_PIXELS, false);
        }
        if !self.want.texts.is_empty() {
            let page = self.want.texts.remove(0);
            let text = self.text_of(page);
            return self.tx.send(Evt::Text { page, text }).is_ok();
        }
        if !self.want.thumbs.is_empty() {
            let (page, scale) = self.want.thumbs.remove(0);
            return self.draw(page, scale, MAX_THUMB_PIXELS, true);
        }
        self.scan_page()
    }

    fn draw(&mut self, page: usize, scale: f32, max_pixels: f32, thumb: bool) -> bool {
        let img = catch_unwind(AssertUnwindSafe(|| {
            self.doc.render_limited(page, scale, max_pixels)
        }))
        .ok()
        .flatten();
        let evt = match img {
            Some(img) if thumb => Evt::Thumb {
                page,
                scale,
                img: Arc::new(img),
            },
            Some(img) => Evt::Rendered {
                page,
                scale,
                img: Arc::new(img),
            },
            None => Evt::Failed { page },
        };
        self.tx.send(evt).is_ok()
    }

    /// The text layer of a page (cached; an unreadable page has none).
    fn text_of(&mut self, page: usize) -> Arc<PageText> {
        if let Some(t) = self.texts.get(&page) {
            return t.clone();
        }
        let lines =
            catch_unwind(AssertUnwindSafe(|| self.doc.page_lines(page))).unwrap_or_default();
        let text = Arc::new(PageText::new(&lines));
        self.texts.insert(page, text.clone());
        self.text_order.push_back(page);
        while self.text_order.len() > TEXT_CAP {
            if let Some(old) = self.text_order.pop_front() {
                self.texts.remove(&old);
            }
        }
        text
    }

    fn scan_page(&mut self) -> bool {
        let Some(scan) = self.scan.as_mut() else {
            return true;
        };
        let Some(&page) = scan.order.get(scan.next) else {
            self.scan = None;
            return true;
        };
        scan.next += 1;
        let (id, tokens, finished) = (scan.id, scan.tokens.clone(), scan.next >= scan.order.len());
        let text = self.text_of(page);
        let hits = find_in_text(page, &text, &tokens);
        if finished {
            self.scan = None;
        }
        self.tx.send(Evt::Found { id, page, hits }).is_ok()
    }
}
