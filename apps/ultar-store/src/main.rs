//! `ultar-store` – the Store window (APP-RULES §2 `main.rs`: only start;
//! connect, build, loop). The logic lives in the library; this file only
//! wires it to the compositor and the backend.
//!
//! Environment:
//! * `ULTAR_COMPOSITOR` – compositor socket (window). Without it the store
//!   runs headless-checkable but cannot open a window.
//! * `ULTAR_BUS` – bus socket (launcher operations). Without it operations
//!   answer `store.error.no-launcher` (never a fake success).
//! * `ULTAR_STORE_CATALOG` – directory holding `ultar-catalog.toml` +
//!   `.sig` (the offline stick/folder). Without it the built-in catalogue of
//!   the image is used.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use ultar_store::action::{Command, StoreAction};
use ultar_store::backend::{Backend, SystemBackend};
use ultar_store::catalog::Catalog;
use ultar_store::i18n;
use ultar_store::source::{CatalogSource, EmbeddedCatalogSource, FileCatalogSource};
use ultar_store::state::Store;
use ultar_store::{event, view};
use ultar_surface::client::Client;
use ultar_surface::ui::UiWindow;
use ultar_surface::WindowConfig;
use ultar_ui::event::Event;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::{Lang, Size};

/// Answers that the backend worker posts back to the UI loop.
struct Worker {
    tx: Sender<Event>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lang = Lang::from_env();
    let translator = i18n::full_translator(lang)?;
    let env = PaintEnv::new(lang, ThemeKind::Dark);
    let mut ui = Ui::new(env, InputMode::Pointer, Size::new(1280.0, 800.0), translator)?;
    let mut store = Store::new();
    let mut hits = view::HitMap::default();
    view::build(&mut ui, &store, &mut hits)?;

    let backend = make_backend()?;
    let (tx, rx): (Sender<Event>, Receiver<Event>) = mpsc::channel();
    let _worker = Worker { tx };
    // First load straight away (offline reads are fast; the launcher answers
    // when it is there).
    dispatch(&backend, store.act(StoreAction::Area(ultar_store::state::Area::Discover)), &tx, &mut store, &mut ui, &mut hits)?;

    let socket = std::env::var("ULTAR_COMPOSITOR").ok();
    let Some(socket) = socket else {
        // No compositor: a headless start still loads the catalogue and prints
        // the page titles it would show (used by smoke tests).
        pump(&rx, &mut store, &mut ui, &mut hits, &backend);
        return Ok(());
    };
    let client = Client::connect(std::path::Path::new(&socket), "org.ultar.store")?;
    let mut win = UiWindow::new(client, &WindowConfig::toplevel("Ultar Store"), ui)?;
    let mut last_wake = 0u64;
    loop {
        let actions = win.dispatch(Some(Duration::from_millis(16)))?;
        let ui_now = &mut win.ui_mut().state;
        for action in actions {
            if let Some(store_action) = hits.translate(&ui_now, &action).cloned() {
                let cmds = store.act(store_action);
                dispatch(&backend, cmds, &tx, &mut store, &mut ui_now.ui, &mut hits)?;
            }
        }
        pump(&rx, &mut store, &mut ui_now.ui, &mut hits, &backend);
        if store.surface != old_surface {
            view::build(&mut ui_now.ui, &store, &mut hits)?;
        }
        win.draw()?;
        if ui_now.ui.animating() {
            let now = now_ms();
            if now != last_wake {
                last_wake = now;
                ui_now.ui.advance_to(now);
            }
        }
        if win.window().close_requested() {
            return Ok(());
        }
    }
}

fn now_ms() -> u64 {
    u64::try_from(Instant::now().elapsed().as_millis()).unwrap_or_default()
}
