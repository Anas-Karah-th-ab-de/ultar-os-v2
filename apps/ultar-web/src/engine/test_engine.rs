//! A deterministic engine for headless tests: every page is one solid colour derived from
//! its address; history, loading, downloads, permission requests, blocked trackers, new-tab
//! requests and crashes are scripted by the address path:
//!
//! | Path part | Effect |
//! |---|---|
//! | `/download/<name>` | a download of `<name>` (no navigation) |
//! | `/ask/<camera\|microphone\|location\|notifications>` | a permission request after loading |
//! | `/trackers/<n>` | `n` blocked requests (0 with protection off) |
//! | `/popup` | the page asks for a new tab with `https://popup.example/` |
//! | `/slow` | loading does not end until `stop` |
//! | `/crash` | the page process ends |
//! | `/title/<t>` | the page title is `<t>` (else "Page <host>") |

use super::{
    DownloadId, EngineError, EngineEvent, FrameDamage, FrameView, KeyInput, PermissionKind,
    PointerInput, RequestId, TabId, WebEngine, MAX_TABS,
};
use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

/// Most entries of a tab's history.
const MAX_HISTORY: usize = 50;

#[derive(Debug, Default)]
struct TestTab {
    private: bool,
    history: Vec<String>,
    index: usize,
    size: (u32, u32),
    pixels: Vec<u8>,
    protection: bool,
    visible: bool,
}

/// Everything the browser sent that is not navigation (for assertions).
#[derive(Debug, Clone, PartialEq)]
pub enum Recorded {
    /// Pointer input.
    Pointer(TabId, PointerInput),
    /// Key input.
    Key(TabId, KeyInput),
    /// A permission answer.
    Answer(RequestId, bool),
    /// A cancelled download.
    Cancel(DownloadId),
    /// A resize (device pixels, scale).
    Resize(TabId, u32, u32, f32),
}

/// The headless test engine.
#[derive(Debug, Default)]
pub struct TestEngine {
    tabs: BTreeMap<TabId, TestTab>,
    next_tab: u32,
    next_request: u32,
    next_download: u32,
    events: VecDeque<EngineEvent>,
    recorded: Vec<Recorded>,
}

/// The colour a test page of `url` shows (BGRX bytes): a hash of the address.
pub fn page_colour(url: &str) -> [u8; 4] {
    let mut h: u32 = 0x811c_9dc5;
    for b in url.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    let [b, g, r, _] = h.to_le_bytes();
    [b, g, r, 255]
}

/// The path of an address (after scheme and host).
fn path(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.find('/').map_or("", |i| &rest[i..])
}

fn segment<'a>(url: &'a str, name: &str) -> Option<&'a str> {
    let url = path(url);
    let marker = format!("/{name}");
    let at = url.find(&marker)?;
    let rest = &url[at + marker.len()..];
    Some(
        rest.strip_prefix('/')
            .unwrap_or(rest)
            .split(['/', '?', '#'])
            .next()
            .unwrap_or(""),
    )
}

impl TestEngine {
    /// A test engine without tabs.
    pub fn new() -> TestEngine {
        TestEngine::default()
    }

    /// What the browser sent besides navigation, oldest first.
    pub fn recorded(&self) -> &[Recorded] {
        &self.recorded
    }

    /// The address a tab shows.
    pub fn url(&self, tab: TabId) -> Option<&str> {
        let t = self.tabs.get(&tab)?;
        t.history.get(t.index).map(String::as_str)
    }

    /// `true` when the tab is private.
    pub fn is_private(&self, tab: TabId) -> Option<bool> {
        self.tabs.get(&tab).map(|t| t.private)
    }

    /// `true` when the tab is shown.
    pub fn is_visible(&self, tab: TabId) -> Option<bool> {
        self.tabs.get(&tab).map(|t| t.visible)
    }

    /// Number of open tabs.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    fn tab(&mut self, tab: TabId) -> Result<&mut TestTab, EngineError> {
        self.tabs.get_mut(&tab).ok_or(EngineError::UnknownTab(tab))
    }

    fn paint(&mut self, tab: TabId) {
        let Some(t) = self.tabs.get_mut(&tab) else {
            return;
        };
        let (w, h) = t.size;
        let Some(url) = t.history.get(t.index) else {
            return;
        };
        if w == 0 || h == 0 {
            return;
        }
        t.pixels = page_colour(url).repeat(w as usize * h as usize);
        self.events.push_back(EngineEvent::Frame {
            tab,
            damage: FrameDamage {
                x0: 0,
                y0: 0,
                x1: w,
                y1: h,
            },
        });
    }

    fn show_current(&mut self, tab: TabId) {
        let Some(t) = self.tabs.get(&tab) else {
            return;
        };
        let Some(url) = t.history.get(t.index).cloned() else {
            return;
        };
        let (can_back, can_forward) = (t.index > 0, t.index + 1 < t.history.len());
        let protection = t.protection;
        let host = crate::url::host_of(&url).unwrap_or_default();
        let title = segment(&url, "title").map_or_else(|| format!("Page {host}"), str::to_string);
        self.events.push_back(EngineEvent::Progress {
            tab,
            loading: true,
            progress: 0.1,
        });
        self.events.push_back(EngineEvent::Navigated {
            tab,
            url: url.clone(),
            can_back,
            can_forward,
        });
        self.events.push_back(EngineEvent::Title { tab, title });
        self.events.push_back(EngineEvent::Security {
            tab,
            secure: crate::url::is_secure(&url),
        });
        let trackers = segment(&url, "trackers").and_then(|n| n.parse::<u32>().ok());
        let count = if protection { trackers.unwrap_or(0) } else { 0 };
        self.events.push_back(EngineEvent::Blocked { tab, count });
        if path(&url).contains("/crash") {
            self.events.push_back(EngineEvent::Crashed { tab });
            return;
        }
        if !path(&url).contains("/slow") {
            self.events.push_back(EngineEvent::Progress {
                tab,
                loading: false,
                progress: 1.0,
            });
        }
        self.paint(tab);
        self.scripted_requests(tab, &url, host);
    }

    fn scripted_requests(&mut self, tab: TabId, url: &str, host: String) {
        if let Some(kind) = segment(url, "ask").and_then(|k| {
            PermissionKind::ALL
                .into_iter()
                .find(|p| p.key().ends_with(k) && !k.is_empty())
        }) {
            self.next_request += 1;
            let request = RequestId(self.next_request);
            self.events.push_back(EngineEvent::Permission {
                tab,
                request,
                host,
                kind,
            });
        }
        if path(url).contains("/popup") {
            self.events.push_back(EngineEvent::NewTab {
                opener: tab,
                url: "https://popup.example/".into(),
            });
        }
    }

    fn download(&mut self, url: &str, name: &str) {
        self.next_download += 1;
        let download = DownloadId(self.next_download);
        self.events.push_back(EngineEvent::DownloadStarted {
            download,
            url: url.into(),
            name: name.into(),
        });
        self.events.push_back(EngineEvent::DownloadProgress {
            download,
            received: 512,
            total: 1024,
        });
        self.events
            .push_back(EngineEvent::DownloadFinished { download, ok: true });
    }
}

impl WebEngine for TestEngine {
    fn create_tab(&mut self, private: bool) -> Result<TabId, EngineError> {
        if self.tabs.len() >= MAX_TABS {
            return Err(EngineError::Limit);
        }
        self.next_tab += 1;
        let id = TabId(self.next_tab);
        self.tabs.insert(
            id,
            TestTab {
                private,
                protection: true,
                ..TestTab::default()
            },
        );
        Ok(id)
    }

    fn close_tab(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.tabs
            .remove(&tab)
            .map(|_| ())
            .ok_or(EngineError::UnknownTab(tab))
    }

    fn load(&mut self, tab: TabId, url: &str) -> Result<(), EngineError> {
        if let Some(name) = segment(url, "download") {
            self.tab(tab)?;
            self.download(url, name);
            return Ok(());
        }
        let t = self.tab(tab)?;
        t.history
            .truncate(if t.history.is_empty() { 0 } else { t.index + 1 });
        t.history.push(url.to_string());
        if t.history.len() > MAX_HISTORY {
            t.history.remove(0);
        }
        t.index = t.history.len() - 1;
        self.show_current(tab);
        Ok(())
    }

    fn back(&mut self, tab: TabId) -> Result<(), EngineError> {
        let t = self.tab(tab)?;
        if t.index > 0 {
            t.index -= 1;
            self.show_current(tab);
        }
        Ok(())
    }

    fn forward(&mut self, tab: TabId) -> Result<(), EngineError> {
        let t = self.tab(tab)?;
        if t.index + 1 < t.history.len() {
            t.index += 1;
            self.show_current(tab);
        }
        Ok(())
    }

    fn reload(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.tab(tab)?;
        self.show_current(tab);
        Ok(())
    }

    fn stop(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.tab(tab)?;
        self.events.push_back(EngineEvent::Progress {
            tab,
            loading: false,
            progress: 1.0,
        });
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
        let changed = t.size != (width, height);
        t.size = (width, height);
        self.recorded
            .push(Recorded::Resize(tab, width, height, scale));
        if changed {
            self.paint(tab);
        }
        Ok(())
    }

    fn set_visible(&mut self, tab: TabId, visible: bool) -> Result<(), EngineError> {
        self.tab(tab)?.visible = visible;
        Ok(())
    }

    fn pointer(&mut self, tab: TabId, input: PointerInput) -> Result<(), EngineError> {
        self.tab(tab)?;
        self.recorded.push(Recorded::Pointer(tab, input));
        Ok(())
    }

    fn key(&mut self, tab: TabId, input: KeyInput) -> Result<(), EngineError> {
        self.tab(tab)?;
        self.recorded.push(Recorded::Key(tab, input));
        Ok(())
    }

    fn set_protection(&mut self, tab: TabId, on: bool) -> Result<(), EngineError> {
        self.tab(tab)?.protection = on;
        Ok(())
    }

    fn answer_permission(&mut self, request: RequestId, allow: bool) -> Result<(), EngineError> {
        self.recorded.push(Recorded::Answer(request, allow));
        Ok(())
    }

    fn cancel_download(&mut self, download: DownloadId) -> Result<(), EngineError> {
        self.recorded.push(Recorded::Cancel(download));
        Ok(())
    }

    fn poll(&mut self, _timeout: Option<Duration>) -> Result<Vec<EngineEvent>, EngineError> {
        Ok(self.events.drain(..).collect())
    }

    fn frame(&self, tab: TabId) -> Option<FrameView<'_>> {
        let t = self.tabs.get(&tab)?;
        let (width, height) = t.size;
        (!t.pixels.is_empty()).then_some(FrameView {
            width,
            height,
            stride: width as usize * 4,
            pixels: &t.pixels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_frames_and_scripts() {
        let mut e = TestEngine::new();
        let t = e.create_tab(false).unwrap();
        e.resize(t, 4, 2, 1.0).unwrap();
        e.load(t, "https://a.example/").unwrap();
        e.load(t, "https://b.example/title/Hi/trackers/3/ask/camera")
            .unwrap();
        let ev = e.poll(None).unwrap();
        assert!(ev.contains(&EngineEvent::Title {
            tab: t,
            title: "Hi".into()
        }));
        assert!(ev.contains(&EngineEvent::Blocked { tab: t, count: 3 }));
        assert!(ev.iter().any(|e| matches!(
            e,
            EngineEvent::Permission {
                kind: PermissionKind::Camera,
                ..
            }
        )));
        assert_eq!(e.frame(t).unwrap().pixels.len(), 32);
        e.back(t).unwrap();
        assert_eq!(e.url(t), Some("https://a.example/"));
        e.forward(t).unwrap();
        e.forward(t).unwrap();
        assert!(e.url(t).unwrap().starts_with("https://b.example/"));
        e.load(t, "https://x.example/download/a.pdf").unwrap();
        let ev = e.poll(None).unwrap();
        assert!(ev
            .iter()
            .any(|e| matches!(e, EngineEvent::DownloadFinished { ok: true, .. })));
        assert!(matches!(
            e.load(TabId(99), "https://a/"),
            Err(EngineError::UnknownTab(_))
        ));
        e.close_tab(t).unwrap();
        assert!(e.close_tab(t).is_err());
        for _ in 0..MAX_TABS {
            e.create_tab(true).unwrap();
        }
        assert!(matches!(e.create_tab(false), Err(EngineError::Limit)));
    }
}
