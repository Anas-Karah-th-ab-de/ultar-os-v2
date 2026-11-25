//! `ultar-store` – the Store window (APP-RULES §2 `main.rs`: only start:
//! connect, register, loop). All logic lives in the library; this binary is
//! the wire between the compositor (window), the backend (launcher, files)
//! and the model.
//!
//! Environment:
//! * `ULTAR_COMPOSITOR_SOCKET` – compositor socket
//!   (default `/run/ultar/ui/compositor.sock`, the shipped socket).
//! * `ULTAR_BUS_SOCKET` – bus socket for `launcher.*` (the cell manager
//!   injects it; unset: operations honestly answer `store.error.no-launcher`).
//! * `ULTAR_STORE_CATALOG` – directory holding `ultar-catalog.toml` + `.sig`
//!   (the offline stick/folder). Unset: the store server (below), falling back
//!   to the catalogue built into the image.
//! * `ULTAR_STORE_URL` – the store server (`https://` only; default
//!   [`ultar_store::online::DEFAULT_URL`]); `ULTAR_STORE_OFFLINE=1` switches the
//!   server off. The server copy is verified like a stick and cached in
//!   `ULTAR_STORE_DATA/catalog-cache`.
//! * `ULTAR_OPEN_URI` – a `store://app/<id>[?action=remove]` deep link the launcher
//!   hands a fresh instance (the shell's "Details in Store" / overview search).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use ultar_store::action::{Command, StoreAction, StoreEvent};
use ultar_store::backend::{Backend, SystemBackend};
use ultar_store::follow::{SharedMetered, SharedSettings};
use ultar_store::i18n;
use ultar_store::online::{self, OnlineCatalogSource, UreqFetch};
use ultar_store::settings::AppUpdateSettings;
use ultar_store::source::{CatalogSource, EmbeddedCatalogSource, FileCatalogSource};
use ultar_store::state::{Area, Store};
use ultar_store::view::{self, HitMap};
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::ui::UiWindow;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::{Lang, Size};

/// The shipped compositor socket (rootfs/etc/ultar/services/compositor.toml `--socket`).
const COMPOSITOR_DEFAULT: &str = "/run/ultar/ui/compositor.sock";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lang = Lang::from_env();
    let translator = i18n::full_translator(lang)?;
    let env = PaintEnv::new(lang, ThemeKind::Dark);
    let mut ui = Ui::new(
        env,
        InputMode::Pointer,
        Size::new(1280.0, 800.0),
        translator,
    )?;

    let mut store = Store::new();
    let mut hits = HitMap::default();
    view::build(&mut ui, &store, &mut hits)?;

    let (cmd_tx, event_rx) = spawn_worker()?;
    let mut auto = spawn_followers();
    let socket =
        std::env::var("ULTAR_COMPOSITOR_SOCKET").unwrap_or_else(|_| COMPOSITOR_DEFAULT.to_string());
    let client = Client::connect(PathBuf::from(&socket).as_path(), "org.ultar.store")?;
    let mut win = UiWindow::new(client, &WindowConfig::toplevel(&store_title(lang)), ui)?;

    // First load: catalogue; the installed list follows as its answer.
    for cmd in store.act(StoreAction::Area(Area::Discover)) {
        let _ = cmd_tx.send(cmd);
    }
    let _ = cmd_tx.send(Command::ReloadCatalog);
    // A deep link (store://app/<id>, ULTAR_OPEN_URI) opens that app's page once the
    // catalogue is on its way; applied after Discover so it is not reset.
    if let Some(link) = ultar_store::deeplink::from_env() {
        for action in link.actions() {
            for cmd in store.act(action) {
                let _ = cmd_tx.send(cmd);
            }
        }
    }
    event_loop(
        &mut win, &mut store, &mut hits, &cmd_tx, &event_rx, &mut auto,
    )
}

/// Automatic app updates (`updates.auto_apps`): the shared settings and metered flag the
/// follower threads keep current, the tick they send on a relevant change, and when the
/// window loop last rechecked (it also rechecks periodically for the install window).
struct AutoUpdate {
    settings: SharedSettings,
    metered: SharedMetered,
    tick: Receiver<()>,
    last: Instant,
}

/// Starts the settings and network followers on the bus (when a socket is given). Without a
/// bus the defaults stay (automatic updates on, window 02:00–05:00, metered unknown) and no
/// tick ever comes, so nothing runs by itself — browsing still works.
fn spawn_followers() -> AutoUpdate {
    let settings: SharedSettings = Arc::new(Mutex::new(AppUpdateSettings::default()));
    let metered: SharedMetered = Arc::new(Mutex::new(None));
    let (tick_tx, tick_rx) = mpsc::channel();
    if let Some(socket) = bus_socket() {
        let options = ultar_bus::ClientOptions::named("app.store");
        if let Ok(client) = ultar_bus::Client::connect(PathBuf::from(&socket).as_path(), options) {
            ultar_store::follow::follow_settings(
                client.clone(),
                Arc::clone(&settings),
                tick_tx.clone(),
            );
            ultar_store::follow::follow_net(client, Arc::clone(&metered), tick_tx);
        }
    }
    AutoUpdate {
        settings,
        metered,
        tick: tick_rx,
        last: Instant::now(),
    }
}

/// Rechecks whether automatic app updates may run now and starts them if so. Runs when a
/// follower ticked (a setting or the connection changed) and at least once a minute (the
/// install window opens with time). The model decides; nothing runs while an operation is
/// in flight or the setting is off.
fn run_auto_update(store: &mut Store, cmd_tx: &Sender<Command>, auto: &mut AutoUpdate) -> bool {
    let mut ticked = false;
    while auto.tick.try_recv().is_ok() {
        ticked = true;
    }
    if !ticked && auto.last.elapsed() < Duration::from_secs(60) {
        return false;
    }
    auto.last = Instant::now();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let settings = auto
        .settings
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone();
    let metered = *auto
        .metered
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let cmds = store.auto_update(settings.allowed_now(now, metered));
    for cmd in &cmds {
        let _ = cmd_tx.send(cmd.clone());
    }
    !cmds.is_empty()
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
    auto: &mut AutoUpdate,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dirty = true;
    loop {
        while let Ok(ev) = event_rx.try_recv() {
            for cmd in store.event(ev) {
                let _ = cmd_tx.send(cmd);
            }
            dirty = true;
        }
        // Automatic app updates (updates.auto_apps), gated on the window and the connection.
        if run_auto_update(store, cmd_tx, auto) {
            dirty = true;
        }
        if dirty {
            // Rebuilding clears the input state; keep the caret in the
            // search field while the user types (it restarts at the end).
            let had_focus = hits
                .search_field()
                .is_some_and(|field| win.ui_mut().focused() == Some(field));
            view::build(win.ui_mut(), store, hits)?;
            if had_focus {
                if let Some(field) = hits.search_field() {
                    let _ = win.ui_mut().focus(field);
                }
            }
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

/// The catalogue source: the stick/folder if configured, else the store server
/// (verified, cached, offline first), else the image one.
fn catalog_source() -> Arc<dyn CatalogSource> {
    let trusted = ultar_store::source::trusted_dir_from_env();
    if let Ok(dir) = std::env::var("ULTAR_STORE_CATALOG") {
        return Arc::new(FileCatalogSource::new(dir, trusted));
    }
    let image: Arc<dyn CatalogSource> = Arc::new(EmbeddedCatalogSource::new(
        "image",
        ultar_store::BUILT_IN_CATALOG,
    ));
    if std::env::var("ULTAR_STORE_OFFLINE").is_ok_and(|v| v == "1") {
        return image;
    }
    let (Ok(data), Some(fetch)) = (std::env::var("ULTAR_STORE_DATA"), UreqFetch::new()) else {
        return image;
    };
    let url = std::env::var("ULTAR_STORE_URL").unwrap_or_else(|_| online::DEFAULT_URL.to_string());
    match OnlineCatalogSource::new(
        &url,
        PathBuf::from(data).join("catalog-cache"),
        trusted,
        Box::new(fetch),
        Arc::clone(&image),
    ) {
        Ok(source) => Arc::new(source),
        Err(_) => image,
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
