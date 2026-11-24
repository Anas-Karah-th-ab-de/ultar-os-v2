//! `ultar-store` – the Store window (APP-RULES §2 `main.rs`: only start:
//! connect, register, loop). All logic lives in the library; this binary is
//! the wire between the compositor (window), the backend (launcher, files)
//! and the model.
//!
//! Environment:
//! * `ULTAR_COMPOSITOR_SOCKET` – compositor socket
//!   (default `/run/ultar/ui/compositor`).
//! * `ULTAR_BUS_SOCKET` – bus socket for `launcher.*` (the cell manager
//!   injects it; unset: operations honestly answer `store.error.no-launcher`).
//! * `ULTAR_STORE_CATALOG` – directory holding `ultar-catalog.toml` + `.sig`
//!   (the offline stick/folder). Unset: the catalogue built into the image.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;
use ultar_store::action::{Command, StoreAction, StoreEvent};
use ultar_store::backend::{Backend, SystemBackend};
use ultar_store::i18n;
use ultar_store::source::{CatalogSource, EmbeddedCatalogSource, FileCatalogSource};
use ultar_store::state::{Area, Store};
use ultar_store::view::{self, HitMap};
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::ui::UiWindow;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::{Lang, Size};

const COMPOSITOR_DEFAULT: &str = "/run/ultar/ui/compositor";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lang = Lang::from_env();
    let translator = i18n::full_translator(lang)?;
    let env = PaintEnv::new(lang, ThemeKind::Dark);
    let ui = Ui::new(env, InputMode::Pointer, Size::new(1280.0, 800.0), translator)?;

    let mut store = Store::new();
    let mut hits = HitMap::default();
    view::build(&mut ui, &store, &mut hits)?;

    let (cmd_tx, event_rx) = spawn_worker()?;
    let socket =
        std::env::var("ULTAR_COMPOSITOR_SOCKET").unwrap_or_else(|_| COMPOSITOR_DEFAULT.to_string());
    let client = Client::connect(PathBuf::from(&socket).as_path(), "org.ultar.store")?;
    let mut win = UiWindow::new(client, &WindowConfig::toplevel(&store_title(lang)), ui)?;

    // First load: catalogue; the installed list follows as its answer.
    for cmd in store.act(StoreAction::Area(Area::Discover)) {
        let _ = cmd_tx.send(cmd);
    }
    let _ = cmd_tx.send(Command::ReloadCatalog);
    event_loop(&mut win, &mut store, &mut hits, &cmd_tx, &event_rx)
}

/// The backend worker: commands in, events out, nothing blocks the UI thread.
fn spawn_worker() -> Result<(Sender<Command>, Receiver<StoreEvent>), Box<dyn std::error::Error>> {
    let backend = Arc::new(SystemBackend::new(
        catalog_source(),
        bus_socket().map(PathBuf::from),
    )?);
    let (event_tx, event_rx) = mpsc::channel::<StoreEvent>();
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
    std::thread::spawn(move || {
        while let Ok(cmd) = cmd_rx.recv() {
            let tx = &event_tx;
            backend.run(cmd, &mut |ev| {
                let _ = tx.send(ev);
            });
        }
    });
    Ok((cmd_tx, event_rx))
}

/// The frame loop: answers → state, input → actions → state, redraw on change.
fn event_loop(
    win: &mut UiWindow,
    store: &mut Store,
    hits: &mut HitMap,
    cmd_tx: &Sender<Command>,
    event_rx: &Receiver<StoreEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dirty = true;
    loop {
        while let Ok(ev) = event_rx.try_recv() {
            for cmd in store.event(ev) {
                let _ = cmd_tx.send(cmd);
            }
            dirty = true;
        }
        if dirty {
            view::build(win.ui_mut(), store, hits)?;
            dirty = false;
        }
        let actions = win.dispatch(Some(Duration::from_millis(30)))?;
        for action in actions {
            let node = action.node;
            let Some(store_action) = hits.translate(win.ui_mut(), &action) else {
                continue;
            };
            if matches!(store_action, StoreAction::More) {
                hits.open_more(win.ui_mut(), node)?;
            }
            for cmd in store.act(store_action) {
                let _ = cmd_tx.send(cmd);
            }
            dirty = true;
        }
        if win.window().close_requested() {
            return Ok(());
        }
    }
}

/// The catalogue source: the stick/folder if configured, else the image one.
fn catalog_source() -> Arc<dyn CatalogSource> {
    match std::env::var("ULTAR_STORE_CATALOG") {
        Ok(dir) => Arc::new(FileCatalogSource::new(
            dir,
            ultar_store::source::trusted_dir_from_env(),
        )),
        Err(_) => Arc::new(EmbeddedCatalogSource::new("image", ultar_store::BUILT_IN_CATALOG)),
    }
}

fn bus_socket() -> Option<String> {
    std::env::var("ULTAR_BUS_SOCKET").ok()
}

/// The window title is the app name from the catalog (brand rule: app
/// windows show the app name, never the product name).
fn store_title(lang: Lang) -> String {
    i18n::full_translator(lang)
        .map(|tr| tr.t("app.store.name").into_owned())
        .unwrap_or_else(|_| "Ultar Store".to_string())
}
