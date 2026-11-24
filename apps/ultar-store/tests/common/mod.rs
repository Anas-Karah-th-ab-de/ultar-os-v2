//! Shared headless harness: build a `Ui` with the Store's catalogs, drive
//! real pointer/keyboard events, and read the results.

#![allow(dead_code)]

use ultar_store::action::{Command, StoreAction, StoreEvent};
use ultar_store::catalog::Catalog;
use ultar_store::i18n;
use ultar_store::source::LoadedCatalog;
use ultar_store::state::{Area, InstalledApp, Store};
use ultar_store::view::{self, HitMap};
use ultar_ui::event::{Event, Key, PointerButton};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::{Lang, Point, Size};

/// Desktop and phone board sizes.
pub const WIDE: Size = Size::new(1280.0, 800.0);
pub const COMPACT: Size = Size::new(360.0, 800.0);

pub const LANGS: [Lang; 2] = [Lang::Ar, Lang::En];
pub const THEMES: [ThemeKind; 2] = [ThemeKind::Dark, ThemeKind::Light];

/// The built-in catalogue as a loaded, verified stick catalogue.
pub fn loaded_from_asset() -> LoadedCatalog {
    LoadedCatalog {
        catalog: Catalog::parse(ultar_store::BUILT_IN_CATALOG).expect("built-in catalog parses"),
        verified: true,
        signer: Some("test-key".into()),
        dir: Some("/stick".into()),
    }
}

/// Dark theme shorthand.
pub fn dark() -> ultar_ui::tokens::ThemeKind {
    ultar_ui::tokens::ThemeKind::Dark
}

/// A store with the built-in catalogue loaded and the board's five
/// components installed (Office tools, Coder, Assistant, Linux programs,
/// Firefox).
pub fn ready_store() -> Store {
    let mut s = Store::new();
    s.event(StoreEvent::CatalogLoaded(Ok(loaded_from_asset())));
    s.event(StoreEvent::Installed(
        [
            "km-office",
            "coder",
            "assistant",
            "linux-programs",
            "firefox",
        ]
        .iter()
        .map(|id| InstalledApp {
            id: (*id).to_string(),
            version: "1.0.0".into(),
            size: None,
            last_used_ms: None,
        })
        .collect(),
    ));
    s
}

/// A loading store (SurfaceStates panel 1).
pub fn loading_store() -> Store {
    Store::new()
}

/// A store whose catalogue read fails damaged (panel 4).
pub fn error_store() -> Store {
    let mut s = Store::new();
    s.event(StoreEvent::CatalogLoaded(Err(
        ultar_store::error::StoreError::from(ultar_store::error::SourceError::Damaged),
    )));
    s.installed = ready_installed();
    s
}

/// The offline surface keeps the installed list (panel 3).
pub fn offline_store() -> Store {
    let mut s = Store::new();
    s.event(StoreEvent::CatalogLoaded(Err(
        ultar_store::error::StoreError::from(ultar_store::error::SourceError::Unreadable),
    )));
    s.installed = ready_installed();
    s
}

/// The no-space surface with a blocked install (panel 5).
pub fn no_space_store() -> Store {
    let mut s = ready_store();
    s.act(StoreAction::Install("media".into()));
    let _ = s.event(StoreEvent::OperationDone {
        app: "media".into(),
        result: Err(ultar_store::error::StoreError::Backend(
            ultar_store::error::BackendError::NoSpace {
                required: None,
                available: None,
            },
        )),
    });
    s
}

fn ready_installed() -> Vec<InstalledApp> {
    ["km-office", "coder", "assistant"]
        .iter()
        .map(|id| InstalledApp {
            id: (*id).to_string(),
            version: "1.0.0".into(),
            size: None,
            last_used_ms: None,
        })
        .collect()
}

/// The headless scene: ui + model + the action map of the last build.
pub struct Scene {
    pub ui: Ui,
    pub store: Store,
    pub hits: HitMap,
}

impl Scene {
    pub fn new(store: Store, lang: Lang, theme: ThemeKind, size: Size) -> Scene {
        let translator = i18n::full_translator(lang).expect("catalogs parse");
        let mut ui = Ui::new(
            PaintEnv::new(lang, theme),
            InputMode::Pointer,
            size,
            translator,
        )
        .expect("ui");
        ui.set_size(size).expect("size");
        let mut hits = HitMap::default();
        view::build(&mut ui, &store, &mut hits).expect("build");
        ui.layout();
        Scene { ui, store, hits }
    }

    pub fn ready(lang: Lang, theme: ThemeKind, wide: bool) -> Scene {
        Scene::new(
            ready_store(),
            lang,
            theme,
            if wide { WIDE } else { COMPACT },
        )
    }

    /// A store in the given area, ready, then a scene.
    pub fn area(area: Area, lang: Lang, theme: ThemeKind, wide: bool) -> Scene {
        let mut s = ready_store();
        s.act(StoreAction::Area(area));
        Scene::new(s, lang, theme, if wide { WIDE } else { COMPACT })
    }

    /// Clicks the node at its centre with real pointer events; returns the
    /// commands the resulting action produced (empty when the node needs an
    /// answer event first, e.g. dialogs).
    pub fn click_node(&mut self, node: NodeId) -> Vec<Command> {
        let center = self
            .ui
            .rect(node)
            .map(|r| Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0))
            .expect("node has a rect");
        self.ui.handle(Event::PointerMove { pos: center });
        self.ui.handle(Event::PointerDown {
            pos: center,
            button: PointerButton::Primary,
        });
        let up = self.ui.handle(Event::PointerUp {
            pos: center,
            button: PointerButton::Primary,
        });
        let mut out = Vec::new();
        for action in up.actions {
            if let Some(store_action) = self.hits.translate(&self.ui, &action) {
                if matches!(store_action, StoreAction::More) {
                    let _ = self.hits.open_more(&mut self.ui, action.node);
                }
                out.extend(self.store.act(store_action));
            }
        }
        out
    }

    /// Clicks the first node bound to this action; panics when the action is
    /// not on screen (a dead click is a test failure).
    pub fn click(&mut self, action: &StoreAction) -> Vec<Command> {
        let node = self
            .hits
            .node_for(action)
            .unwrap_or_else(|| panic!("no node bound to {action:?} on this screen"));
        self.click_node(node)
    }

    /// Rebuilds the tree from the current store state (what the shell does
    /// after every change).
    pub fn rebuild(&mut self) {
        view::build(&mut self.ui, &self.store, &mut self.hits).expect("build");
        self.ui.layout();
    }

    /// Clicks the tab that leads to `area` on the phone tab bar (a real
    /// pointer event; the strip is scanned so RTL/LTR order and content-sized
    /// tabs do not matter). Panics when no tab position reaches the area.
    pub fn click_tab_to(&mut self, area: Area) -> Vec<Command> {
        let tabs = self.hits.tab_bar().expect("tab bar on the phone layout");
        let r = self.ui.rect(tabs).expect("tab bar has a rect");
        let attempts = 16usize;
        for i in 0..attempts {
            let frac = (i as f32 + 0.5) / attempts as f32;
            let pos = Point::new(r.x + r.w * frac, r.y + r.h / 2.0);
            self.ui.handle(Event::PointerMove { pos });
            let _ = self.ui.handle(Event::PointerDown {
                pos,
                button: PointerButton::Primary,
            });
            let up = self.ui.handle(Event::PointerUp {
                pos,
                button: PointerButton::Primary,
            });
            let hit = up
                .actions
                .iter()
                .filter_map(|a| self.hits.translate(&self.ui, a))
                .find(|a| *a == StoreAction::Area(area));
            if let Some(store_action) = hit {
                let out = self.store.act(store_action);
                self.rebuild();
                return out;
            }
        }
        panic!("no tab position selects {area:?}");
    }

    /// Answers events from the shell, collecting commands; rebuilds.
    pub fn answer(&mut self, ev: StoreEvent) -> Vec<Command> {
        let cmds = self.store.event(ev);
        self.rebuild();
        cmds
    }

    /// Types into the search field (real key events after focusing it).
    pub fn type_search(&mut self, text: &str) -> Vec<Command> {
        let node = self.hits.search_field().expect("search field on screen");
        self.ui.focus(node).expect("focus");
        let mut out = Vec::new();
        for ch in text.chars() {
            let resp = self.ui.handle(Event::Key {
                key: Key::Char(ch),
                modifiers: Default::default(),
            });
            for action in resp.actions {
                if let Some(a) = self.hits.translate(&self.ui, &action) {
                    out.extend(self.store.act(a));
                }
            }
        }
        self.rebuild();
        out
    }

    /// Render the surface headlessly; returns the pixel buffer size.
    pub fn render(&mut self) -> (u32, u32) {
        let canvas = ultar_ui::snapshot::render(&mut self.ui).expect("render");
        (canvas.width(), canvas.height())
    }

    /// All accessible texts currently shown (names resolved).
    pub fn a11y_texts(&mut self) -> Vec<String> {
        let tr = self.ui.translator().clone();
        self.ui
            .a11y_tree()
            .iter()
            .map(|n| n.describe(&tr))
            .collect()
    }
}
