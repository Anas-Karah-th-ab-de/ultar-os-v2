//! The engine when none is installed: the browser window, its tabs, settings and history work,
//! every page load ends at once as a failed page (`Crashed` – the page view says that the page
//! could not be shown). Used by `ultar-web` when the engine bundle (`/usr/lib/ultar/web`, WPE
//! WebKit built by `scripts/build-ultar-web.sh`) is missing or does not start, e.g. in the
//! VMware image, which has no x86 engine bundle: the window must still open (found in VMware:
//! the browser ended at start and nothing opened). Nothing is rendered, nothing is fetched.

use super::{
    DownloadId, EngineError, EngineEvent, FrameView, KeyInput, PointerInput, RequestId, TabId,
    WebEngine, MAX_TABS,
};
use std::time::Duration;

/// An engine without a page renderer.
#[derive(Debug, Default)]
pub struct OfflineEngine {
    tabs: Vec<TabId>,
    next: u32,
    events: Vec<EngineEvent>,
}

impl OfflineEngine {
    /// No tabs yet.
    pub fn new() -> OfflineEngine {
        OfflineEngine::default()
    }

    fn known(&self, tab: TabId) -> Result<(), EngineError> {
        if self.tabs.contains(&tab) {
            Ok(())
        } else {
            Err(EngineError::UnknownTab(tab))
        }
    }

    fn fail(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.known(tab)?;
        self.events.push(EngineEvent::Crashed { tab });
        Ok(())
    }
}

impl WebEngine for OfflineEngine {
    fn create_tab(&mut self, _private: bool) -> Result<TabId, EngineError> {
        if self.tabs.len() >= MAX_TABS {
            return Err(EngineError::Limit);
        }
        self.next = self.next.wrapping_add(1).max(1);
        let tab = TabId(self.next);
        self.tabs.push(tab);
        Ok(tab)
    }
    fn close_tab(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.known(tab)?;
        self.tabs.retain(|t| *t != tab);
        self.events
            .retain(|e| !matches!(e, EngineEvent::Crashed { tab: t } if *t == tab));
        Ok(())
    }
    fn load(&mut self, tab: TabId, _url: &str) -> Result<(), EngineError> {
        self.fail(tab)
    }
    fn back(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.known(tab)
    }
    fn forward(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.known(tab)
    }
    fn reload(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.fail(tab)
    }
    fn stop(&mut self, tab: TabId) -> Result<(), EngineError> {
        self.known(tab)
    }
    fn resize(&mut self, tab: TabId, _: u32, _: u32, _: f32) -> Result<(), EngineError> {
        self.known(tab)
    }
    fn set_visible(&mut self, tab: TabId, _visible: bool) -> Result<(), EngineError> {
        self.known(tab)
    }
    fn pointer(&mut self, tab: TabId, _input: PointerInput) -> Result<(), EngineError> {
        self.known(tab)
    }
    fn key(&mut self, tab: TabId, _input: KeyInput) -> Result<(), EngineError> {
        self.known(tab)
    }
    fn set_protection(&mut self, tab: TabId, _on: bool) -> Result<(), EngineError> {
        self.known(tab)
    }
    fn answer_permission(&mut self, _request: RequestId, _allow: bool) -> Result<(), EngineError> {
        Ok(())
    }
    fn cancel_download(&mut self, _download: DownloadId) -> Result<(), EngineError> {
        Ok(())
    }
    fn poll(&mut self, _timeout: Option<Duration>) -> Result<Vec<EngineEvent>, EngineError> {
        Ok(std::mem::take(&mut self.events))
    }
    fn frame(&self, _tab: TabId) -> Option<FrameView<'_>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_exist_and_every_load_fails_at_once() {
        let mut e = OfflineEngine::new();
        let a = e.create_tab(false).unwrap();
        let b = e.create_tab(true).unwrap();
        assert_ne!(a, b);
        e.load(a, "https://example.org/").unwrap();
        assert_eq!(e.poll(None).unwrap(), vec![EngineEvent::Crashed { tab: a }]);
        assert!(e.poll(None).unwrap().is_empty());
        assert!(e.frame(a).is_none());
        e.close_tab(a).unwrap();
        assert!(matches!(e.load(a, "x"), Err(EngineError::UnknownTab(_))));
        for _ in 0..MAX_TABS - 1 {
            e.create_tab(false).unwrap();
        }
        assert!(matches!(e.create_tab(false), Err(EngineError::Limit)));
    }
}
