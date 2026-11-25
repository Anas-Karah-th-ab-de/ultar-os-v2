//! End-to-end run of the Settings app against the real settings service under the service
//! manager: `ultar-init` (not PID 1) starts the **shipped** definitions of logd, bus and settingsd
//! (paths moved below a work directory, settingsd with a laptop-with-modem profile), each as its
//! own user. The app runs headless with its real bus ports as `app.settings` and opens every area
//! in Arabic, English and German: no page may say "not available in this version", every
//! settingsd page must show its rows (no "could not be read"), no label may be an untranslated
//! key, the wide sidebar shows the six group headings, the search finds a row of Sharing, a
//! setting nothing follows yet is disabled with its note and stores nothing, and every setting
//! that takes effect is changed through its page and checked where it takes effect
//! (`settingstest/wired.rs`; the volume service runs too). Last line: `SETTINGS: ALLES OK` or
//! `SETTINGS: FEHLER <what>`.
//!
//! ```text
//! settingstest --bin DIR --work DIR [--group-root DIR]
//! ```

#[path = "settingstest/setup.rs"]
mod setup;
#[path = "settingstest/wired.rs"]
mod wired;

use setup::Run;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use ultar_bus::messages::init::{ServiceState, Status};
use ultar_bus::messages::settings::{Get, ListRequest, SettingValue};
use ultar_bus::Client;
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::areas::{index_of, PageKind, AREAS, GROUPS};
use ultar_settings::service::bus::{
    BusAccount, BusDevices, BusDisplay, BusSettings, BusStatus, BusUpdate, ClipboardQueue,
    DocumentsFiles,
};
use ultar_settings::service::Ports;
use ultar_settings::App;
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

const WAIT: Duration = Duration::from_secs(120);

fn wait<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> Result<T, String> {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(v) = f() {
            return Ok(v);
        }
        if Instant::now() > deadline {
            return Err(what.to_owned());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn running(admin: &Client) -> usize {
    admin
        .call(&Status {}, Duration::from_secs(5))
        .map(|r| {
            r.services
                .iter()
                .filter(|s| s.state == ServiceState::Running)
                .count()
        })
        .unwrap_or(0)
}

fn ports(bus: &Client, run: &Run) -> Ports {
    Ports {
        os_release: None,
        settings: Box::new(BusSettings::new(bus.clone())),
        display: Box::new(BusDisplay::new(bus.clone())),
        devices: Box::new(BusDevices(bus.clone())),
        update: Box::new(BusUpdate::new(bus.clone())),
        account: Box::new(BusAccount(bus.clone())),
        mesh: Box::new(ultar_settings::service::mesh::BusMesh::new(bus)),
        cloud: Box::new(ultar_settings::service::cloud::BusCloud::new(bus)),
        status: Box::new(BusStatus::new(bus)),
        system: Box::new(ultar_settings::service::system::BusSystem::new(bus.clone())),
        files: Box::new(DocumentsFiles {
            dir: run.at("documents"),
        }),
        clipboard: Box::new(ClipboardQueue::default()),
    }
}

fn open(bus: &Client, run: &Run, lang: Lang, size: Size, area: usize) -> Result<App, String> {
    let tr = ultar_settings::i18n::translator(lang, Some(&run.at("catalogs")));
    let env = PaintEnv::new(lang, ThemeKind::Dark);
    let ui = Ui::new(env, InputMode::Pointer, size, tr).map_err(|e| format!("ui {e:?}"))?;
    App::new(ui, ports(bus, run), area).map_err(|e| format!("app {e:?}"))
}

/// Every name, value and description of the screen-reader tree, translated.
fn texts(app: &mut App) -> Vec<String> {
    let tree = app.ui.a11y_tree();
    let tr = app.ui.translator().clone();
    tree.iter()
        .flat_map(|n| [n.name.clone(), n.value.clone(), n.description.clone()])
        .flatten()
        .map(|l| l.resolve(&tr).into_owned())
        .collect()
}

/// Opens one area in a running app and checks its page.
fn page(app: &mut App, lang: Lang, area: usize) -> Result<(), String> {
    let id = AREAS[area].id;
    let what = |msg: &str| format!("{lang:?} {id}: {msg}");
    app.open_area(area).map_err(|e| what(&format!("{e:?}")))?;
    if app.state.area != area || app.state.page() == PageKind::NoService {
        return Err(what("not opened or no service"));
    }
    let tr = app.ui.translator().clone();
    let mut banned = vec![
        tr.t("app.settings.no_service").into_owned(),
        tr.t("app.settings.no_service.sub").into_owned(),
    ];
    if let PageKind::Panel(_) = app.state.page() {
        banned.push(tr.t("app.settings.panel.unavailable").into_owned());
        if app.view.node(Act::PanelItem(0)).is_none() {
            return Err(what("no rows"));
        }
    }
    for text in texts(app) {
        if banned.contains(&text) {
            return Err(what(&format!("shows {text:?}")));
        }
        if text.starts_with("settings.") || text.starts_with("app.settings.") {
            return Err(what(&format!("untranslated {text}")));
        }
    }
    Ok(())
}

/// Every area in every language: on a wide window, the mobile network on a phone-sized one.
fn every_page(bus: &Client, run: &Run) -> Result<(), String> {
    let cellular = index_of("cellular").ok_or("cellular")?;
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        let mut wide = open(bus, run, lang, Size::new(1280.0, 800.0), 0)?;
        let mut phone = open(bus, run, lang, Size::new(360.0, 712.0), 0)?;
        for area in 0..AREAS.len() {
            let app = if area == cellular {
                &mut phone
            } else {
                &mut wide
            };
            page(app, lang, area)?;
        }
        println!(
            "SETTINGS: {lang:?}: all {} pages open, none unavailable",
            AREAS.len()
        );
    }
    Ok(())
}

fn press(app: &mut App, key: Key) -> Result<(), String> {
    let event = Event::Key {
        key,
        modifiers: Modifiers::NONE,
    };
    app.handle(event).map(|_| ()).map_err(|e| format!("{e:?}"))
}

fn click(app: &mut App, node: ultar_ui::ui::NodeId) -> Result<(), String> {
    app.ui.layout();
    let pos = app.ui.rect(node).ok_or("no rect")?.center();
    for event in [
        Event::PointerMove { pos },
        Event::PointerDown {
            pos,
            button: PointerButton::Primary,
        },
        Event::PointerUp {
            pos,
            button: PointerButton::Primary,
        },
    ] {
        app.handle(event).map_err(|e| format!("{e:?}"))?;
    }
    Ok(())
}

/// The grouped sidebar, the search and a switch nothing follows yet.
fn sidebar(bus: &Client, admin: &Client, run: &Run) -> Result<(), String> {
    let displays = index_of("displays").ok_or("displays")?;
    let mut app = open(bus, run, Lang::En, Size::new(1280.0, 800.0), displays)?;
    let nav = app.view.nav.ok_or("no sidebar")?;
    let children = app.ui.children(nav).len();
    if children != app.view.nav_areas.len() + GROUPS.len() {
        return Err(format!("sidebar: {children} rows"));
    }
    let search = app.view.search.ok_or("no search")?;
    click(&mut app, search)?;
    for c in "ssh".chars() {
        press(&mut app, Key::Char(c))?;
    }
    let sharing = index_of("sharing").ok_or("sharing")?;
    if app.view.nav_areas != [sharing] {
        return Err(format!("search ssh: {:?}", app.view.nav_areas));
    }
    press(&mut app, Key::Enter)?;
    let panel = app.state.panel("sharing").ok_or("sharing panel")?.clone();
    let row = panel
        .items
        .iter()
        .position(|i| i.key == "sharing.screen")
        .ok_or("sharing.screen row")?;
    // Nothing shares the screen in this version: the switch says so and stores nothing.
    let switch = app.view.node(Act::PanelItem(row)).ok_or("switch")?;
    let note = app
        .ui
        .translator()
        .t("app.settings.panel.not_yet")
        .into_owned();
    if !app.ui.is_disabled(switch) || !texts(&mut app).contains(&note) {
        return Err("screen sharing is offered although nothing follows it".into());
    }
    click(&mut app, switch)?;
    let get = Get {
        key: "sharing.screen".into(),
    };
    let value = admin
        .call(&get, Duration::from_secs(5))
        .map_err(|e| format!("get {e}"))?;
    if value.value != Some(SettingValue::Flag(false)) {
        return Err(format!("sharing.screen is {:?}", value.value));
    }
    println!(
        "SETTINGS: grouped sidebar, search \"ssh\" -> Sharing, screen sharing disabled with its note"
    );
    Ok(())
}

fn stop(init: &Child) -> Result<(), String> {
    let pid = rustix::process::Pid::from_raw(init.id() as i32).ok_or("pid")?;
    rustix::process::kill_process(pid, rustix::process::Signal::TERM).map_err(|e| e.to_string())
}

fn test(run: &Run, group_root: Option<&str>) -> Result<(), String> {
    let config = run.layout(group_root).map_err(|e| format!("layout {e}"))?;
    let mut init = Command::new(run.bin.join("ultar-init"))
        .arg("--config")
        .arg(config)
        .spawn()
        .map_err(|e| format!("init {e}"))?;
    let checked = (|| {
        let admin = run.client("system.admin")?;
        wait("services running", || {
            (running(&admin) == setup::SERVICES.len()).then_some(())
        })?;
        let bus = run.client(setup::APP)?;
        let panels = wait("settings.list", || {
            bus.call(&ListRequest::default(), Duration::from_secs(5))
                .ok()
        })?;
        println!("SETTINGS: settingsd lists {} panels", panels.panels.len());
        every_page(&bus, run)?;
        sidebar(&bus, &admin, run)?;
        wired::wired(&bus, &admin, run)
    })();
    let stopped = stop(&init);
    let ended = wait("init ended", || init.try_wait().ok().flatten());
    checked
        .and(stopped)
        .and(ended.and_then(|s| s.success().then_some(()).ok_or(format!("init ended {s}"))))
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        let index = words.iter().position(|word| word == name)?;
        words.get(index + 1).map(std::path::PathBuf::from)
    };
    let (Some(bin), Some(work)) = (value("--bin"), value("--work")) else {
        println!("SETTINGS: FEHLER usage: settingstest --bin DIR --work DIR [--group-root DIR]");
        return;
    };
    let group_root = value("--group-root").map(|p| p.display().to_string());
    match test(&Run { bin, work }, group_root.as_deref()) {
        Ok(()) => println!("SETTINGS: ALLES OK"),
        Err(what) => println!("SETTINGS: FEHLER {what}"),
    }
}
