//! What the engine reports.

use super::{AppError, Browser, Pending, MAX_PENDING_PROMPTS};
use crate::engine::{EngineEvent, PermissionKind, RequestId, TabId, WebEngine};
use crate::model::permissions::Decision;
use std::time::Duration;

impl<E: WebEngine> Browser<E> {
    /// Takes the engine's events (waiting up to `timeout`) and applies them.
    pub fn pump(&mut self, timeout: Option<Duration>) -> Result<(), AppError> {
        let events = self.engine.poll(timeout)?;
        let changed = !events.is_empty();
        for event in events {
            self.engine_event(event)?;
        }
        if changed && !self.closed {
            self.refresh()?;
        }
        Ok(())
    }

    /// Applies one engine event.
    pub fn engine_event(&mut self, event: EngineEvent) -> Result<(), AppError> {
        match event {
            EngineEvent::Navigated {
                tab,
                url,
                can_back,
                can_forward,
            } => self.navigated(tab, url, can_back, can_forward)?,
            event @ (EngineEvent::Title { .. }
            | EngineEvent::Progress { .. }
            | EngineEvent::Security { .. }
            | EngineEvent::Blocked { .. }) => self.tab_state(event),
            EngineEvent::Frame { tab, damage } => self.frame_arrived(tab, damage)?,
            EngineEvent::Permission {
                tab,
                request,
                host,
                kind,
            } => self.permission(tab, request, host, kind)?,
            EngineEvent::DownloadStarted {
                download,
                url,
                name,
            } => self.state.downloads.start(download, &url, &name),
            EngineEvent::DownloadProgress {
                download,
                received,
                total,
            } => self.state.downloads.progress(download, received, total),
            EngineEvent::DownloadFinished { download, ok } => {
                self.state.downloads.finish(download, ok)
            }
            EngineEvent::NewTab { opener, url } => self.open_from(opener, &url)?,
            EngineEvent::Crashed { tab } => {
                if let Some(t) = self.state.tabs.by_engine(tab) {
                    t.crashed = true;
                    t.loading = false;
                    self.notify("web.crashed");
                }
            }
        }
        Ok(())
    }

    /// Title, progress, connection state and blocked count of a tab.
    fn tab_state(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Title { tab, title } => {
                if let Some(t) = self.state.tabs.by_engine(tab) {
                    t.title = title.chars().take(512).collect();
                    if !t.private {
                        let (url, title) = (t.url.clone(), t.title.clone());
                        self.state.history.set_title(&url, &title);
                    }
                }
            }
            EngineEvent::Progress {
                tab,
                loading,
                progress,
            } => {
                if let Some(t) = self.state.tabs.by_engine(tab) {
                    t.loading = loading;
                    t.progress = progress;
                }
            }
            EngineEvent::Security { tab, secure } => {
                if let Some(t) = self.state.tabs.by_engine(tab) {
                    t.secure = secure;
                }
            }
            EngineEvent::Blocked { tab, count } => {
                if let Some(t) = self.state.tabs.by_engine(tab) {
                    t.blocked = Some(count);
                }
            }
            _ => {}
        }
    }

    fn navigated(
        &mut self,
        tab: TabId,
        url: String,
        can_back: bool,
        can_forward: bool,
    ) -> Result<(), AppError> {
        let Some(t) = self.state.tabs.by_engine(tab) else {
            return Ok(());
        };
        t.url = url.clone();
        t.can_back = can_back;
        t.can_forward = can_forward;
        t.blocked = None;
        let (private, title) = (t.private, t.title.clone());
        if !private && crate::url::host_of(&url).is_some() {
            self.state.history.visit(&url, &title);
            self.note_visit()?;
        }
        Ok(())
    }

    fn permission(
        &mut self,
        tab: TabId,
        request: RequestId,
        host: String,
        kind: PermissionKind,
    ) -> Result<(), AppError> {
        let private = self
            .state
            .tabs
            .list()
            .iter()
            .any(|t| t.engine == tab && t.private);
        let store = if private {
            &self.state.private_permissions
        } else {
            &self.state.permissions
        };
        match store.get(&host, kind) {
            Decision::Allow => self.engine.answer_permission(request, true)?,
            Decision::Block => self.engine.answer_permission(request, false)?,
            Decision::Ask if self.state.prompts.len() >= MAX_PENDING_PROMPTS => {
                self.engine.answer_permission(request, false)?
            }
            Decision::Ask => self.state.prompts.push_back(Pending {
                tab,
                request,
                host,
                kind,
            }),
        }
        Ok(())
    }

    /// A page asked for a new tab: opens it after its opener, with the opener's privacy.
    fn open_from(&mut self, opener: TabId, url: &str) -> Result<(), AppError> {
        let Ok(target) = crate::url::resolve(url, self.state.settings.search) else {
            return Ok(());
        };
        if target.searched {
            return Ok(());
        }
        let private = self
            .state
            .tabs
            .list()
            .iter()
            .any(|t| t.engine == opener && t.private);
        if let Some(i) = self.state.tabs.index_of(opener) {
            self.state.tabs.select(i);
        }
        self.new_tab(private)?;
        self.load(&target.url)
    }
}
