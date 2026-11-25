//! The Apps page end to end: the default-app choices come from the launcher and a change
//! reaches its table; the autostart list is handed out once after a sign-in; removable media
//! reaches the shell. A stand-in launcher on the launcher's socket answers the default-app
//! calls and follows `settings.changed` into the launcher's own [`Defaults`] and [`Autostart`].

use super::setup::Run;
use super::wired::{control, page, Follower, WAIT};
use super::{click, press, wait};
use std::sync::{Arc, Mutex, MutexGuard};
use ultar_bus::messages::launcher::{
    kind_of_uri, Candidates, CandidatesReply, Kind, List, ListReply, Resolve, ResolveReply,
};
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::settings::{Changed, Get, SettingValue};
use ultar_bus::{Client, ErrorCode};
use ultar_desktop::app::desktop::link::apply_setting;
use ultar_desktop::model::quick::RemovableMedia;
use ultar_desktop::model::Desktop;
use ultar_i18n::Lang;
use ultar_launcher::defaults::{Autostart, Defaults, AUTOSTART};
use ultar_settings::App;
use ultar_ui::event::Key;

/// A lock that recovers from a poisoned mutex (a panic in another thread must not hide the
/// effect this run checks).
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Opens the choice of `key` and takes the option `down` steps below the selected one; the
/// value the settings service holds afterwards.
fn choose(
    app: &mut App,
    admin: &Client,
    (panel, key): (&str, &str),
    down: usize,
) -> Result<SettingValue, String> {
    let node = control(app, panel, key)?;
    click(app, node)?;
    if !app.ui.is_open(node) {
        return Err(format!("{key}: the choice did not open"));
    }
    for _ in 0..down {
        press(app, Key::ArrowDown)?;
    }
    press(app, Key::Enter)?;
    let reply = admin
        .call(&Get { key: key.into() }, WAIT)
        .map_err(|e| format!("get {e}"))?;
    reply.value.ok_or(format!("{key}: no value"))
}

/// Types `text` into the field of `key` and submits it; the value afterwards.
fn edit(
    app: &mut App,
    admin: &Client,
    (panel, key): (&str, &str),
    text: &str,
) -> Result<SettingValue, String> {
    let node = control(app, panel, key)?;
    click(app, node)?;
    app.ui
        .set_text(node, text)
        .map_err(|e| format!("set_text {e:?}"))?;
    press(app, Key::Enter)?;
    let reply = admin
        .call(&Get { key: key.into() }, WAIT)
        .map_err(|e| format!("get {e}"))?;
    reply.value.ok_or(format!("{key}: no value"))
}

/// The installed apps a kind offers in this run (the launcher's `candidates` answer).
fn candidates(kind: Kind) -> Vec<String> {
    let ids: &[&str] = match kind {
        Kind::Web => &["browser", "store.web"],
        Kind::Mail => &["mail"],
        Kind::Calendar => &["calendar"],
        Kind::Music | Kind::Video => &["media"],
        Kind::Photos => &["images"],
        Kind::Documents => &["documents", "office"],
    };
    ids.iter().map(|s| (*s).to_owned()).collect()
}

/// A stand-in launcher: answers the default-app calls (so the page offers candidates) and
/// follows `settings.changed` into the launcher's own code, where the effect is checked.
struct FakeLauncher {
    defaults: Arc<Mutex<Defaults>>,
    autostart: Arc<Mutex<Autostart>>,
    _client: Client,
}

impl FakeLauncher {
    fn start(run: &Run) -> Result<FakeLauncher, String> {
        let client = run.client_at("launcher")?;
        let defaults = Arc::new(Mutex::new(Defaults::default()));
        let autostart = Arc::new(Mutex::new(Autostart::default()));
        let changes = client
            .subscribe::<Changed>()
            .map_err(|e| format!("launcher subscribe: {e}"))?;
        let endpoint = client
            .register("launcher")
            .map_err(|e| format!("launcher register: {e}"))?;
        {
            let (d, a) = (Arc::clone(&defaults), Arc::clone(&autostart));
            std::thread::spawn(move || {
                while let Ok(c) = changes.recv() {
                    if c.key == AUTOSTART {
                        lock(&a).set_list(&c.value);
                    } else {
                        lock(&d).on_changed(&c.key, &c.value);
                    }
                }
            });
        }
        {
            let d = Arc::clone(&defaults);
            std::thread::spawn(move || {
                while let Ok(req) = endpoint.recv() {
                    let _ = match req.method() {
                        "candidates" => req.respond::<Candidates>(|c| {
                            Ok(CandidatesReply {
                                apps: candidates(c.kind),
                            })
                        }),
                        "resolve" => req.respond::<Resolve>(|r| {
                            let app = kind_of_uri(&r.uri).map(|k| lock(&d).app(k).to_owned());
                            Ok(ResolveReply { app })
                        }),
                        "list" => req.respond::<List>(|_| Ok(ListReply::default())),
                        _ => req.fail(ErrorCode::UnknownMethod),
                    };
                }
            });
        }
        Ok(FakeLauncher {
            defaults,
            autostart,
            _client: client,
        })
    }
}

/// Everything the Apps page wires, changed through the page and checked where it takes effect.
pub(super) fn apps(
    bus: &Client,
    admin: &Client,
    run: &Run,
    shell: &Follower,
) -> Result<(), String> {
    let launcher = FakeLauncher::start(run)?;
    let mut app = page(bus, run, "apps")?;
    // the default web app: browser -> store.web (a candidate the launcher offered)
    let web = choose(&mut app, admin, ("apps", "apps.default.web"), 1)?;
    if web != SettingValue::Text("store.web".into()) {
        return Err(format!(
            "apps.default.web is {web:?}, not the chosen candidate"
        ));
    }
    wait("the launcher's default web app", || {
        (lock(&launcher.defaults).app(Kind::Web) == "store.web").then_some(())
    })?;
    // removable media: ask -> open_files; the shell's state follows
    let mut desk = Desktop::new(Lang::En);
    let media = choose(&mut app, admin, ("apps", "apps.removable_media"), 1)?;
    shell.told("apps.removable_media", &media)?;
    apply_setting(&mut desk, "apps.removable_media", &media);
    if desk.values.removable_media != RemovableMedia::OpenFiles {
        return Err(format!(
            "the shell did not follow removable_media: {:?}",
            desk.values.removable_media
        ));
    }
    // the autostart list: typed, then handed out once after a sign-in
    edit(&mut app, admin, ("apps", "apps.autostart"), "browser, mail")?;
    wait("the launcher's autostart list", || {
        (lock(&launcher.autostart).list() == ["browser", "mail"]).then_some(())
    })?;
    let signed_in = SessionState {
        user: Some("amal".into()),
        signed_in: vec!["amal".into()],
    };
    let due = lock(&launcher.autostart).on_session(&signed_in);
    if due != ["browser", "mail"] {
        return Err(format!("autostart hands out {due:?}"));
    }
    println!(
        "SETTINGS: apps page -> launcher (default web app store.web, autostart browser+mail) \
         and shell (removable media opens Files)"
    );
    Ok(())
}
