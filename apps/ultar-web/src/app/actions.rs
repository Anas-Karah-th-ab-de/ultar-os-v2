//! The clicks of the window (docs/ui/CLICKS.md, `browser.*`) and of the browser menu.

use super::{AppError, Browser};
use crate::chrome::panels::{
    self, MENU_DOWNLOADS, MENU_NEW_TAB, MENU_PRIVATE, MENU_SETTINGS, MENU_SITE,
};
use crate::chrome::Act;
use crate::engine::WebEngine;
use crate::model::history::MAX_SUGGESTIONS;
use ultar_ui::event::{Action, ActionKind};
use ultar_ui::widgets::{Label, Menu, MenuItem};

/// Menu ids of address suggestions start here.
pub(super) const SUGGESTION_BASE: u32 = 100;

impl<E: WebEngine> Browser<E> {
    /// Handles the actions of one UI event.
    pub fn handle_actions(&mut self, actions: &[Action]) -> Result<(), AppError> {
        for action in actions {
            if self.closed {
                break;
            }
            if let ActionKind::MenuChosen(id) = action.kind {
                self.menu_chosen(id)?;
                continue;
            }
            if self.overlay_action(action)? {
                continue;
            }
            if let Some(act) = self.chrome.act(action.node) {
                self.act(act, action.kind)?;
            }
        }
        Ok(())
    }

    fn act(&mut self, act: Act, kind: ActionKind) -> Result<(), AppError> {
        let active = self.state.tabs.active().map(|t| (t.engine, t.loading));
        match (act, kind) {
            (Act::Tab(i), ActionKind::Activated) => self.select_tab(i)?,
            (Act::Tab(i), ActionKind::MiddleClicked)
            | (Act::TabClose(i), ActionKind::Activated) => self.close_tab(i)?,
            (Act::NewTab, ActionKind::Activated) => self.new_tab(false)?,
            (Act::Back, ActionKind::Activated) => {
                if let Some((tab, _)) = active {
                    self.engine.back(tab)?;
                }
            }
            (Act::Forward, ActionKind::Activated) => {
                if let Some((tab, _)) = active {
                    self.engine.forward(tab)?;
                }
            }
            (Act::Reload, ActionKind::Activated) => self.reload_or_stop()?,
            (Act::Address | Act::StartSearch, ActionKind::Submitted) => {
                let Some(node) = self.chrome.node(act) else {
                    return Ok(());
                };
                let text = self.ui.text(node)?.to_string();
                self.open_typed(&text, node)?;
            }
            (Act::Address, ActionKind::TextChanged) => {
                let _ = self.ui.set_error(self.chrome.address, None);
            }
            (Act::Shield, ActionKind::Activated) => self.open_shield()?,
            (Act::Downloads, ActionKind::Activated) => self.open_downloads()?,
            _ => {}
        }
        Ok(())
    }

    /// Reload, or stop while the page loads (`browser.reload`).
    pub fn reload_or_stop(&mut self) -> Result<(), AppError> {
        let Some(tab) = self
            .state
            .tabs
            .active()
            .filter(|t| !t.on_start_page())
            .cloned()
        else {
            return Ok(());
        };
        if tab.loading {
            self.engine.stop(tab.engine)?;
        } else {
            self.engine.reload(tab.engine)?;
        }
        Ok(())
    }

    /// Opens typed text in the shown tab (address or search); a refused input is shown as
    /// the field's error.
    pub fn open_typed(&mut self, text: &str, field: ultar_ui::ui::NodeId) -> Result<(), AppError> {
        match crate::url::resolve(text, self.state.settings.search) {
            Ok(target) => {
                let _ = self.ui.set_error(field, None);
                self.load(&target.url)
            }
            Err(e) => {
                self.ui.set_error(field, Some(Label::key(e.key())))?;
                Ok(())
            }
        }
    }

    /// Loads a checked address in the shown tab.
    pub fn load(&mut self, url: &str) -> Result<(), AppError> {
        let Some(tab) = self.state.tabs.active_mut() else {
            return Ok(());
        };
        let id = tab.engine;
        tab.url = url.to_string();
        tab.title.clear();
        tab.crashed = false;
        let protects = crate::url::host_of(url).map_or(self.state.settings.protection, |h| {
            self.state.settings.protects(&h)
        });
        self.engine.set_protection(id, protects)?;
        self.engine.load(id, url)?;
        self.refresh()?;
        if let Some(page) = self.chrome.page {
            let _ = self.ui.focus(page);
        }
        Ok(())
    }

    /// Opens the browser menu at the downloads button (the end of the toolbar).
    pub fn open_menu(&mut self) -> Result<(), AppError> {
        self.close_overlay()?;
        let on_page = self.state.tabs.active().is_some_and(|t| !t.on_start_page());
        self.ui
            .open_menu(self.chrome.downloads, panels::menu(on_page))?;
        Ok(())
    }

    /// Opens the history suggestions for the typed address under the field.
    pub fn open_suggestions(&mut self) -> Result<bool, AppError> {
        let typed = self.ui.text(self.chrome.address)?.to_string();
        let items: Vec<MenuItem> = self
            .state
            .history
            .suggest(&typed)
            .iter()
            .take(MAX_SUGGESTIONS)
            .enumerate()
            .map(|(i, e)| MenuItem::new(SUGGESTION_BASE + i as u32, Label::data(&e.url)))
            .collect();
        if items.is_empty() {
            return Ok(false);
        }
        self.ui.open_menu(self.chrome.address, Menu::new(items))?;
        Ok(true)
    }

    fn menu_chosen(&mut self, id: u32) -> Result<(), AppError> {
        match id {
            MENU_NEW_TAB => self.new_tab(false),
            MENU_PRIVATE => self.new_tab(true),
            MENU_DOWNLOADS => self.open_downloads(),
            MENU_SITE => self.open_shield(),
            MENU_SETTINGS => self.open_settings(),
            id if id >= SUGGESTION_BASE => {
                let typed = self.ui.text(self.chrome.address)?.to_string();
                let url = self
                    .state
                    .history
                    .suggest(&typed)
                    .get((id - SUGGESTION_BASE) as usize)
                    .map(|e| e.url.clone());
                match url {
                    Some(url) => self.load(&url),
                    None => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }
}
