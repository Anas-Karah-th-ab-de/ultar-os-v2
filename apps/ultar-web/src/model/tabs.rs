//! The tabs of a browser window.

use crate::engine::TabId;

/// One tab.
#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    /// The engine's tab.
    pub engine: TabId,
    /// Address shown (empty for the start page).
    pub url: String,
    /// Page title.
    pub title: String,
    /// Loading.
    pub loading: bool,
    /// Share loaded.
    pub progress: f32,
    /// History behind.
    pub can_back: bool,
    /// History ahead.
    pub can_forward: bool,
    /// Verified encrypted connection.
    pub secure: bool,
    /// Private tab (nothing kept).
    pub private: bool,
    /// Trackers blocked on the current page (`None` when the engine does not count them).
    pub blocked: Option<u32>,
    /// The page process ended unexpectedly.
    pub crashed: bool,
}

impl Tab {
    /// A tab on the start page.
    pub fn new(engine: TabId, private: bool) -> Tab {
        Tab {
            engine,
            url: String::new(),
            title: String::new(),
            loading: false,
            progress: 0.0,
            can_back: false,
            can_forward: false,
            secure: false,
            private,
            blocked: None,
            crashed: false,
        }
    }

    /// `true` while the tab shows the start page.
    pub fn on_start_page(&self) -> bool {
        self.url.is_empty()
    }

    /// The tab's label: the title, else the host, else `None` (the "New tab" text).
    pub fn label(&self) -> Option<String> {
        if !self.title.trim().is_empty() {
            return Some(self.title.chars().take(80).collect());
        }
        (!self.url.is_empty()).then(|| crate::url::short_name(&self.url))
    }
}

/// The tabs, one of them shown.
#[derive(Debug, Clone, Default)]
pub struct Tabs {
    list: Vec<Tab>,
    active: usize,
}

impl Tabs {
    /// Adds a tab after the shown one and shows it; returns its index.
    pub fn open(&mut self, tab: Tab) -> usize {
        let at = if self.list.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.list.insert(at, tab);
        self.active = at;
        at
    }

    /// Closes tab `index`; returns the removed tab. The neighbour after it (or before it,
    /// for the last tab) is shown.
    pub fn close(&mut self, index: usize) -> Option<Tab> {
        if index >= self.list.len() {
            return None;
        }
        let tab = self.list.remove(index);
        if self.list.is_empty() {
            self.active = 0;
        } else if index < self.active || self.active >= self.list.len() {
            self.active = self.active.saturating_sub(1).min(self.list.len() - 1);
        }
        Some(tab)
    }

    /// Shows tab `index` (ignored when out of range).
    pub fn select(&mut self, index: usize) -> bool {
        if index < self.list.len() {
            self.active = index;
            return true;
        }
        false
    }

    /// Index of the shown tab.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// The shown tab.
    pub fn active(&self) -> Option<&Tab> {
        self.list.get(self.active)
    }

    /// The shown tab, changeable.
    pub fn active_mut(&mut self) -> Option<&mut Tab> {
        self.list.get_mut(self.active)
    }

    /// The tab of an engine tab.
    pub fn by_engine(&mut self, id: TabId) -> Option<&mut Tab> {
        self.list.iter_mut().find(|t| t.engine == id)
    }

    /// Index of an engine tab.
    pub fn index_of(&self, id: TabId) -> Option<usize> {
        self.list.iter().position(|t| t.engine == id)
    }

    /// All tabs in strip order.
    pub fn list(&self) -> &[Tab] {
        &self.list
    }

    /// Number of tabs.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// `true` without tabs.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_close_select() {
        let mut t = Tabs::default();
        assert_eq!(t.open(Tab::new(TabId(1), false)), 0);
        assert_eq!(t.open(Tab::new(TabId(2), false)), 1);
        t.select(0);
        assert_eq!(t.open(Tab::new(TabId(3), true)), 1);
        assert_eq!(
            t.list().iter().map(|t| t.engine.0).collect::<Vec<_>>(),
            [1, 3, 2]
        );
        assert!(!t.select(9));
        t.select(2);
        assert_eq!(t.close(2).unwrap().engine, TabId(2));
        assert_eq!(t.active_index(), 1);
        t.select(1);
        t.close(0);
        assert_eq!(t.active().unwrap().engine, TabId(3));
        assert_eq!(t.index_of(TabId(3)), Some(0));
        t.by_engine(TabId(3)).unwrap().title = "T".into();
        assert_eq!(t.active().unwrap().label().as_deref(), Some("T"));
        assert!(t.close(5).is_none());
        t.close(0);
        assert!(t.is_empty() && t.active().is_none());
        let mut tab = Tab::new(TabId(4), false);
        assert!(tab.on_start_page() && tab.label().is_none());
        tab.url = "https://a.example/x".into();
        assert_eq!(tab.label().as_deref(), Some("a.example"));
    }
}
