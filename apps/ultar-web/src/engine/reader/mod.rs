//! The reader engine: shows web pages without a browser engine, in pure Rust. Pages are
//! fetched over HTTP(S) on a worker thread ([`fetch`]), read as text with links ([`html`]),
//! laid out with the system's fonts (Arabic from the right) and drawn into the tab's frame
//! ([`page`]). No script runs, nothing is stored on disk; images and styles are not shown.
//! Used when the full engine bundle (`/usr/lib/ultar/web`) is not installed, e.g. on the PC
//! image and on the Ultar kernel.

pub mod fetch;
pub mod html;
pub mod page;

use super::{
    DownloadId, EngineError, EngineEvent, FrameDamage, FrameView, KeyInput, PointerInput,
    RequestId, TabId, WebEngine, MAX_TABS,
};
use html::{Block, Document, Kind};
use page::{Laid, Painting};
use rustix::fd::{AsFd, BorrowedFd, OwnedFd};
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;
use ultar_i18n::{Arg, Lang, Translator};
use ultar_ui::canvas::Canvas;

/// Lines one arrow key scrolls, in points.
const STEP: f32 = 48.0;
/// The search page that works without scripts (a DuckDuckGo query is sent there).
const PLAIN_SEARCH: &str = "https://html.duckduckgo.com/html/?q=";

/// Linux key codes the page reacts to.
mod code {
    pub const SPACE: u16 = 57;
    pub const HOME: u16 = 102;
    pub const UP: u16 = 103;
    pub const PAGE_UP: u16 = 104;
    pub const END: u16 = 107;
    pub const DOWN: u16 = 108;
    pub const PAGE_DOWN: u16 = 109;
}

struct Tab {
    history: Vec<String>,
    index: usize,
    doc: Option<Document>,
    laid: Option<Laid>,
    frame: Option<Canvas>,
    size: (u32, u32, f32),
    scroll: f32,
    /// Number of the load in flight (an older answer is dropped).
    loading: Option<u64>,
    pressed: Option<(f32, f32)>,
}

struct Done {
    tab: TabId,
    load: u64,
    url: String,
    result: Result<fetch::Fetched, fetch::FetchError>,
}

/// The engine.
pub struct ReaderEngine {
    tabs: BTreeMap<TabId, Tab>,
    next_tab: u32,
    next_load: u64,
    events: Vec<EngineEvent>,
    client: fetch::Client,
    painting: Painting,
    tr: Translator,
    done_tx: Sender<Done>,
    done_rx: Receiver<Done>,
    wake_read: OwnedFd,
    wake_write: OwnedFd,
}

impl ReaderEngine {
    /// An engine whose own pages (errors) are in `lang`; `user_agent` names the browser.
    pub fn new(lang: Lang, user_agent: &str) -> Result<ReaderEngine, EngineError> {
        let (wake_read, wake_write) = rustix::pipe::pipe_with(
            rustix::pipe::PipeFlags::CLOEXEC | rustix::pipe::PipeFlags::NONBLOCK,
        )
        .map_err(|e| EngineError::Start(e.into()))?;
        let painting = Painting::new(lang).ok_or(EngineError::Limit)?;
        let tr = crate::catalog::translator(lang).map_err(|_| EngineError::Limit)?;
        let (done_tx, done_rx) = channel();
        Ok(ReaderEngine {
            tabs: BTreeMap::new(),
            next_tab: 0,
            next_load: 0,
            events: Vec::new(),
            client: fetch::Client::new(user_agent),
            painting,
            tr,
            done_tx,
            done_rx,
            wake_read,
            wake_write,
        })
    }

    fn tab(&mut self, tab: TabId) -> Result<&mut Tab, EngineError> {
        self.tabs.get_mut(&tab).ok_or(EngineError::UnknownTab(tab))
    }

    fn start_load(&mut self, tab: TabId, url: String) -> Result<(), EngineError> {
        self.next_load += 1;
        let load = self.next_load;
        self.tab(tab)?.loading = Some(load);
        self.events.push(EngineEvent::Progress {
            tab,
            loading: true,
            progress: 0.1,
        });
        if url == "about:blank" {
            let _ = self.done_tx.send(Done {
                tab,
                load,
                url: url.clone(),
                result: Ok(fetch::Fetched {
                    url,
                    mime: "text/html".into(),
                    text: String::new(),
                }),
            });
            let _ = rustix::io::write(&self.wake_write, &[1]);
            return Ok(());
        }
        let client = self.client.clone();
        let tx = self.done_tx.clone();
        let wake = self.wake_write.try_clone().map_err(EngineError::Io)?;
        let fetch_url = plain_search(&url);
        std::thread::spawn(move || {
            let result = client.get(&fetch_url);
            let _ = tx.send(Done {
                tab,
                load,
                url,
                result,
            });
            let _ = rustix::io::write(&wake, &[1]);
        });
        Ok(())
    }

    fn arrived(&mut self, done: Done) {
        let current = self
            .tabs
            .get(&done.tab)
            .is_some_and(|t| t.loading == Some(done.load));
        if !current {
            return;
        }
        let tab = done.tab;
        let (url, doc, secure) = self.document(done);
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        t.loading = None;
        // a redirect replaces the address in the history
        if let Some(entry) = t.history.get_mut(t.index) {
            *entry = url.clone();
        }
        let title = if doc.title.is_empty() {
            url.clone()
        } else {
            doc.title.clone()
        };
        t.doc = Some(doc);
        t.scroll = 0.0;
        t.laid = None;
        let (can_back, can_forward) = (t.index > 0, t.index + 1 < t.history.len());
        self.events.extend([
            EngineEvent::Navigated {
                tab,
                url,
                can_back,
                can_forward,
            },
            EngineEvent::Title { tab, title },
            EngineEvent::Security { tab, secure },
            EngineEvent::Progress {
                tab,
                loading: false,
                progress: 1.0,
            },
        ]);
        self.render(tab);
    }

    /// The page that came (or the page that says why none came): address, text, secure?
    fn document(&self, done: Done) -> (String, Document, bool) {
        match done.result {
            Ok(f) => {
                let secure = f.url.starts_with("https://");
                let doc = if f.mime == "text/plain" {
                    Document {
                        title: String::new(),
                        blocks: vec![Block {
                            kind: Kind::Pre,
                            text: f.text,
                            links: Vec::new(),
                        }],
                    }
                } else {
                    html::read(&f.text, &f.url)
                };
                (f.url, doc, secure)
            }
            Err(e) => {
                let doc = self.error_page(&done.url, &e);
                (done.url, doc, false)
            }
        }
    }

    fn error_page(&self, url: &str, e: &fetch::FetchError) -> Document {
        let reason = match e {
            fetch::FetchError::Unreachable(why) => self
                .tr
                .t_args("web.reader.unreachable", &[("reason", Arg::Str(why))]),
            fetch::FetchError::Status(code) => self
                .tr
                .t_args("web.reader.status", &[("code", Arg::Int(i64::from(*code)))]),
            fetch::FetchError::NotText(mime) => self
                .tr
                .t_args("web.reader.not_text", &[("type", Arg::Str(mime))]),
            fetch::FetchError::TooLarge => self.tr.t("web.reader.too_large").into_owned(),
        };
        let para = |text: String| Block {
            kind: Kind::Para,
            text,
            links: Vec::new(),
        };
        Document {
            title: self.tr.t("web.reader.failed").into_owned(),
            blocks: vec![
                Block {
                    kind: Kind::Heading(2),
                    text: self.tr.t("web.reader.failed").into_owned(),
                    links: Vec::new(),
                },
                para(url.to_owned()),
                para(reason),
            ],
        }
    }

    /// Lays out (when needed) and draws the visible part of a tab; a `Frame` event follows.
    fn render(&mut self, tab: TabId) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let (w, h, scale) = t.size;
        if w == 0 || h == 0 {
            return;
        }
        let Some(doc) = &t.doc else {
            return;
        };
        if t.laid.is_none() {
            t.laid = Some(self.painting.lay_out(doc, w, scale));
        }
        let Some(laid) = &t.laid else {
            return;
        };
        t.scroll = t.scroll.clamp(0.0, (laid.height - h as f32).max(0.0));
        t.frame = self.painting.draw(laid, w, h, t.scroll);
        if t.frame.is_some() {
            self.events.push(EngineEvent::Frame {
                tab,
                damage: FrameDamage {
                    x0: 0,
                    y0: 0,
                    x1: w,
                    y1: h,
                },
            });
        }
    }

    fn scroll_by(&mut self, tab: TabId, pixels: f32) -> Result<(), EngineError> {
        self.tab(tab)?.scroll += pixels;
        self.render(tab);
        Ok(())
    }

    fn navigate(&mut self, tab: TabId, url: String) -> Result<(), EngineError> {
        let t = self.tab(tab)?;
        t.history.truncate(t.index + 1);
        if !t.history.is_empty() {
            t.index += 1;
        }
        t.history.push(url.clone());
        t.index = t.history.len() - 1;
        self.start_load(tab, url)
    }
}

/// A DuckDuckGo query goes to its page without scripts.
fn plain_search(url: &str) -> String {
    match url.strip_prefix("https://duckduckgo.com/?q=") {
        Some(q) => format!("{PLAIN_SEARCH}{q}"),
        None => url.to_owned(),
    }
}

impl WebEngine for ReaderEngine {
    fn create_tab(&mut self, _private: bool) -> Result<TabId, EngineError> {
        if self.tabs.len() >= MAX_TABS {
            return Err(EngineError::Limit);
        }
        self.next_tab = self.next_tab.wrapping_add(1).max(1);
        let tab = TabId(self.next_tab);
        self.tabs.insert(
            tab,
            Tab {
                history: Vec::new(),
                index: 0,
                doc: None,
                laid: None,
                frame: None,
                size: (0, 0, 1.0),
                scroll: 0.0,
                loading: None,
                pressed: None,
            },
        );
        Ok(tab)
    }
    fn close_tab(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.tabs.remove(&tab).ok_or(EngineError::UnknownTab(tab))?;
        Ok(())
    }
    fn load(&mut self, tab: TabId, url: &str) -> Result<(), EngineError> {
        self.navigate(tab, url.to_owned())
    }
    fn back(&mut self, tab: TabId) -> Result<(), EngineError> {
        let t = self.tab(tab)?;
        if t.index == 0 {
            return Ok(());
        }
        t.index -= 1;
        let url = t.history[t.index].clone();
        self.start_load(tab, url)
    }
    fn forward(&mut self, tab: TabId) -> Result<(), EngineError> {
        let t = self.tab(tab)?;
        if t.index + 1 >= t.history.len() {
            return Ok(());
        }
        t.index += 1;
        let url = t.history[t.index].clone();
        self.start_load(tab, url)
    }
    fn reload(&mut self, tab: TabId) -> Result<(), EngineError> {
        let t = self.tab(tab)?;
        match t.history.get(t.index).cloned() {
            Some(url) => self.start_load(tab, url),
            None => Ok(()),
        }
    }
    fn stop(&mut self, tab: TabId) -> Result<(), EngineError> {
        let t = self.tab(tab)?;
        if t.loading.take().is_some() {
            self.events.push(EngineEvent::Progress {
                tab,
                loading: false,
                progress: 0.0,
            });
        }
        Ok(())
    }
    fn resize(
        &mut self,
        tab: TabId,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<(), EngineError> {
        let t = self.tab(tab)?;
        let size = (width, height, scale);
        if t.size != size {
            if t.size.0 != width || t.size.2 != scale {
                t.laid = None;
            }
            t.size = size;
            self.render(tab);
        }
        Ok(())
    }
    fn set_visible(&mut self, tab: TabId, visible: bool) -> Result<(), EngineError> {
        self.tab(tab)?;
        if visible {
            self.render(tab);
        }
        Ok(())
    }
    fn pointer(&mut self, tab: TabId, input: PointerInput) -> Result<(), EngineError> {
        match input {
            PointerInput::Scroll { dy, .. } => {
                let s = self.tab(tab)?.size.2;
                self.scroll_by(tab, dy * STEP * s / 3.0)?;
            }
            PointerInput::Down { x, y, button: 1 } => self.tab(tab)?.pressed = Some((x, y)),
            PointerInput::Up { x, y, button: 1 } => {
                let t = self.tab(tab)?;
                let pressed = t.pressed.take();
                let near =
                    pressed.is_some_and(|(px, py)| (px - x).abs() < 8.0 && (py - y).abs() < 8.0);
                let link = t
                    .laid
                    .as_ref()
                    .filter(|_| near)
                    .and_then(|l| page::link_at(l, x, y + t.scroll));
                if let Some(href) = link {
                    self.navigate(tab, href)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn key(&mut self, tab: TabId, input: KeyInput) -> Result<(), EngineError> {
        if !input.pressed {
            return Ok(());
        }
        let (h, s) = {
            let t = self.tab(tab)?;
            (t.size.1 as f32, t.size.2)
        };
        let page = (h - STEP * s).max(STEP * s);
        let by = match input.code {
            code::UP => -STEP * s,
            code::DOWN => STEP * s,
            code::PAGE_UP => -page,
            code::PAGE_DOWN | code::SPACE => page,
            code::HOME => f32::MIN / 4.0,
            code::END => f32::MAX / 4.0,
            _ => return Ok(()),
        };
        self.scroll_by(tab, by)
    }
    fn set_protection(&mut self, tab: TabId, _on: bool) -> Result<(), EngineError> {
        // nothing runs and nothing third-party is fetched: there is nothing to block
        self.tab(tab).map(|_| ())
    }
    fn answer_permission(&mut self, _request: RequestId, _allow: bool) -> Result<(), EngineError> {
        Ok(())
    }
    fn cancel_download(&mut self, _download: DownloadId) -> Result<(), EngineError> {
        Ok(())
    }
    fn poll(&mut self, timeout: Option<Duration>) -> Result<Vec<EngineEvent>, EngineError> {
        let mut buf = [0u8; 64];
        while rustix::io::read(&self.wake_read, &mut buf).is_ok_and(|n| n > 0) {}
        let mut arrived: Vec<Done> = self.done_rx.try_iter().collect();
        if arrived.is_empty() && self.events.is_empty() {
            if let Some(t) = timeout.filter(|t| !t.is_zero()) {
                if let Ok(done) = self.done_rx.recv_timeout(t) {
                    arrived.push(done);
                }
            }
        }
        for done in arrived {
            self.arrived(done);
        }
        Ok(std::mem::take(&mut self.events))
    }
    fn frame(&self, tab: TabId) -> Option<FrameView<'_>> {
        let canvas = self.tabs.get(&tab)?.frame.as_ref()?;
        Some(FrameView {
            width: canvas.width(),
            height: canvas.height(),
            stride: canvas.width() as usize * 4,
            pixels: canvas.data(),
        })
    }
    fn wake_fd(&self) -> Option<BorrowedFd<'_>> {
        Some(self.wake_read.as_fd())
    }
}

#[cfg(test)]
mod tests;
