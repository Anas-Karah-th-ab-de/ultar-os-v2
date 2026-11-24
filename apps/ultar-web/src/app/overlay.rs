//! Panels, menu and dialogs of the window and what their answers do.

use super::{AppError, Browser, Pending};
use crate::chrome::panels::{
    self, DownloadsPanel, PromptDialog, SettingsDialog, ShieldPanel, ShieldView,
};
use crate::engine::{PermissionKind, WebEngine};
use crate::model::permissions::Decision;
use crate::url::SearchEngine;
use ultar_ui::event::{Action, ActionKind};
use ultar_ui::ui::NodeId;

/// What is open above the window.
#[derive(Debug, Clone, Default)]
pub enum Overlay {
    /// Nothing.
    #[default]
    None,
    /// The page panel.
    Shield(ShieldPanel),
    /// The download list.
    Downloads(DownloadsPanel),
    /// The settings.
    Settings(SettingsDialog),
    /// A permission prompt and the request it answers.
    Prompt(PromptDialog, Pending),
}

impl Overlay {
    /// The dialog node, if one is open.
    pub fn dialog(&self) -> Option<NodeId> {
        match self {
            Overlay::None => None,
            Overlay::Shield(p) => Some(p.dialog),
            Overlay::Downloads(p) => Some(p.dialog),
            Overlay::Settings(d) => Some(d.dialog),
            Overlay::Prompt(d, _) => Some(d.dialog),
        }
    }

    /// Forgets the open panel (its nodes are gone after a rebuild).
    pub(super) fn forget(&mut self) {
        *self = Overlay::None;
    }
}

impl<E: WebEngine> Browser<E> {
    /// Closes what is open above the window. A prompt closed this way is answered "block"
    /// (not remembered) – a request never stays unanswered.
    pub fn close_overlay(&mut self) -> Result<(), AppError> {
        let overlay = std::mem::take(&mut self.overlay);
        if let Some(dialog) = overlay.dialog() {
            panels::close(&mut self.ui, dialog);
        }
        if let Overlay::Prompt(_, pending) = overlay {
            self.engine.answer_permission(pending.request, false)?;
        }
        Ok(())
    }

    fn shield_view(&self) -> Option<ShieldView> {
        let tab = self.state.tabs.active()?;
        let host = crate::url::host_of(&tab.url)?;
        let store = if tab.private {
            &self.state.private_permissions
        } else {
            &self.state.permissions
        };
        Some(ShieldView {
            rights: PermissionKind::ALL
                .iter()
                .map(|k| (*k, store.get(&host, *k)))
                .collect(),
            site_on: self.state.settings.protects(&host),
            blocked: tab.blocked,
            secure: tab.secure,
            host,
        })
    }

    /// Opens the page panel (`browser.shield`). It switches nothing by itself.
    pub fn open_shield(&mut self) -> Result<(), AppError> {
        self.close_overlay()?;
        if let Some(view) = self.shield_view() {
            let panel = panels::open_shield(&mut self.ui, self.chrome.shield, &view)?;
            self.overlay = Overlay::Shield(panel);
        }
        Ok(())
    }

    /// Opens the download list (`browser.downloads`).
    pub fn open_downloads(&mut self) -> Result<(), AppError> {
        self.close_overlay()?;
        let panel = panels::open_downloads(
            &mut self.ui,
            self.chrome.downloads,
            self.state.downloads.list(),
        )?;
        self.overlay = Overlay::Downloads(panel);
        Ok(())
    }

    /// Opens the settings.
    pub fn open_settings(&mut self) -> Result<(), AppError> {
        self.close_overlay()?;
        let dialog = panels::open_settings(&mut self.ui, &self.state.settings)?;
        self.overlay = Overlay::Settings(dialog);
        Ok(())
    }

    /// Shows the next waiting permission request when nothing else is open.
    pub(super) fn show_next_prompt(&mut self) -> Result<(), AppError> {
        if !matches!(self.overlay, Overlay::None) {
            return Ok(());
        }
        let Some(pending) = self.state.prompts.pop_front() else {
            return Ok(());
        };
        let private = self
            .state
            .tabs
            .list()
            .iter()
            .any(|t| t.engine == pending.tab && t.private);
        let dialog = panels::open_prompt(&mut self.ui, &pending.host, pending.kind, private)?;
        self.overlay = Overlay::Prompt(dialog, pending);
        Ok(())
    }

    fn answer_prompt(&mut self, allow: bool) -> Result<(), AppError> {
        let Overlay::Prompt(dialog, pending) = std::mem::take(&mut self.overlay) else {
            return Ok(());
        };
        let remember = self.ui.is_checked(dialog.remember).unwrap_or(false);
        panels::close(&mut self.ui, dialog.dialog);
        self.engine.answer_permission(pending.request, allow)?;
        let private = self
            .state
            .tabs
            .list()
            .iter()
            .any(|t| t.engine == pending.tab && t.private);
        let decision = if allow {
            Decision::Allow
        } else {
            Decision::Block
        };
        if private {
            self.state
                .private_permissions
                .set(&pending.host, pending.kind, decision);
        } else if remember {
            self.state
                .permissions
                .set(&pending.host, pending.kind, decision);
            self.save()?;
        }
        self.show_next_prompt()
    }

    /// Handles an action inside the open overlay; `false` when it was not one.
    pub(super) fn overlay_action(&mut self, action: &Action) -> Result<bool, AppError> {
        let node = action.node;
        match self.overlay.clone() {
            Overlay::None => return Ok(false),
            _ if action.kind == ActionKind::Closed && Some(node) == self.overlay.dialog() => {
                self.close_overlay()?;
            }
            Overlay::Prompt(d, _) if node == d.allow && action.kind == ActionKind::Activated => {
                self.answer_prompt(true)?
            }
            Overlay::Prompt(d, _) if node == d.block && action.kind == ActionKind::Activated => {
                self.answer_prompt(false)?
            }
            Overlay::Shield(p) => return self.shield_action(&p, action),
            Overlay::Downloads(p) => return self.downloads_action(&p, action),
            Overlay::Settings(d) => return self.settings_action(&d, action),
            Overlay::Prompt(..) => return Ok(false),
        }
        Ok(true)
    }

    fn shield_action(&mut self, p: &ShieldPanel, action: &Action) -> Result<bool, AppError> {
        let Some(tab) = self.state.tabs.active().cloned() else {
            return Ok(false);
        };
        let Some(host) = crate::url::host_of(&tab.url) else {
            return Ok(false);
        };
        match action.kind {
            ActionKind::Toggled(on) if action.node == p.site => {
                self.state.settings.set_site_protection(&host, on);
                self.engine
                    .set_protection(tab.engine, self.state.settings.protects(&host))?;
                self.save()?;
                self.engine.reload(tab.engine)?;
                self.close_overlay()?;
                self.refresh()?;
            }
            ActionKind::Selected(i) => {
                let Some((_, kind)) = p.rights.iter().find(|(n, _)| *n == action.node) else {
                    return Ok(false);
                };
                let decision = crate::model::permissions::Decision::ALL
                    .get(i)
                    .copied()
                    .unwrap_or_default();
                if tab.private {
                    self.state.private_permissions.set(&host, *kind, decision);
                } else {
                    self.state.permissions.set(&host, *kind, decision);
                    self.save()?;
                }
            }
            ActionKind::Activated if action.node == p.close => self.close_overlay()?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn downloads_action(&mut self, p: &DownloadsPanel, action: &Action) -> Result<bool, AppError> {
        if action.kind != ActionKind::Activated {
            return Ok(false);
        }
        let find = |list: &[(NodeId, crate::engine::DownloadId)]| {
            list.iter()
                .find(|(n, _)| *n == action.node)
                .map(|(_, d)| *d)
        };
        if let Some(id) = find(&p.cancel) {
            self.engine.cancel_download(id)?;
            self.state.downloads.cancel(id);
            self.open_downloads()?;
        } else if let Some(id) = find(&p.open) {
            self.close_overlay()?;
            self.open_download(id)?;
        } else if let Some(id) = find(&p.show) {
            self.show_download(id)?;
        } else if Some(action.node) == p.clear {
            self.state.downloads.clear_finished();
            self.open_downloads()?;
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    fn settings_action(&mut self, d: &SettingsDialog, action: &Action) -> Result<bool, AppError> {
        match action.kind {
            ActionKind::Selected(i) if action.node == d.search => {
                if let Some(engine) = SearchEngine::ALL.get(i) {
                    self.state.settings.search = *engine;
                }
            }
            ActionKind::Toggled(on) if action.node == d.protection => {
                self.state.settings.protection = on;
                let tabs: Vec<_> = self
                    .state
                    .tabs
                    .list()
                    .iter()
                    .map(|t| (t.engine, crate::url::host_of(&t.url)))
                    .collect();
                for (id, host) in tabs {
                    let on = host.map_or(self.state.settings.protection, |h| {
                        self.state.settings.protects(&h)
                    });
                    self.engine.set_protection(id, on)?;
                }
            }
            ActionKind::Activated if action.node == d.clear => {
                self.state.history.clear();
                self.notify("web.settings.clear");
            }
            ActionKind::Activated if action.node == d.close => {
                self.close_overlay()?;
                self.save()?;
                self.refresh()?;
                return Ok(true);
            }
            _ => return Ok(false),
        }
        self.save()?;
        Ok(true)
    }
}
