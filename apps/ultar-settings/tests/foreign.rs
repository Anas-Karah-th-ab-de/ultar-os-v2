//! Settings › Apps from other systems (docs/FREMD-APPS.md, stage P3), driven by clicks: the
//! Linux area and its update, the page of one app, permissions (more waits for an
//! administrator, less applies at once), keeping the data and removing the app.
//! `ULTAR_SETTINGS_SHOTS=<dir>` writes the pages in Arabic and English, dark and light.

mod common;

use common::*;
use ultar_bus::messages::foreign::{
    Choices, ForeignApp, NetworkChoice, Progress, RuntimeStateReply, Source, Step,
};
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::service::fake::Shared;
use ultar_settings::service::foreign::ForeignWorld;
use ultar_settings::state::{DialogPurpose, SubPage};
use ultar_settings::App;
use ultar_ui::tokens::ThemeKind;

fn mousepad() -> ForeignApp {
    ForeignApp {
        id: "foreign.deb-mousepad".into(),
        name: "Mousepad".into(),
        names: vec![("ar".into(), "ماوس باد".into())],
        source: Source::Deb,
        package: "mousepad".into(),
        version: "0.6.2-1".into(),
        icon: String::new(),
        choices: Choices::default(),
    }
}

fn world_with(installed: bool) -> ForeignWorld {
    ForeignWorld {
        runtime: RuntimeStateReply {
            installed,
            version: if installed { "26.04-1".into() } else { String::new() },
            size_bytes: 812_000_000,
            busy: false,
        },
        apps: if installed { vec![mousepad()] } else { Vec::new() },
        asking: Some(7),
        ..ForeignWorld::default()
    }
}

fn open(lang: Lang, theme: ThemeKind, foreign: Option<ForeignWorld>) -> (Shared, App) {
    let world = board_world(1, true);
    world.borrow_mut().system.foreign = foreign;
    let app = app(&world, lang, theme, content_size(1440.0, 900.0), "foreign");
    (world, app)
}

fn press(app: &mut App, act: Act) {
    let node = app
        .view
        .node(act)
        .unwrap_or_else(|| panic!("{act:?} not shown"));
    // a person scrolls to what is below the fold
    app.ui.scroll_into_view(node).unwrap();
    click(app, node);
}

fn last(world: &Shared) -> String {
    log(world).last().cloned().unwrap_or_default()
}

fn choices(world: &Shared) -> Choices {
    let w = world.borrow();
    let f = w.system.foreign.as_ref().expect("service");
    f.apps.first().expect("app").choices.clone()
}

#[test]
fn without_the_service_or_the_area_the_page_says_so() {
    let (_, app) = open(Lang::En, ThemeKind::Dark, None);
    assert!(app.view.node(Act::ForeignUpdate).is_none());
    assert!(app.state.status.foreign.error.is_some());
    let (world, mut app) = open(Lang::En, ThemeKind::Dark, Some(world_with(false)));
    // the area is installed from the component (a drive or the Store's downloads)
    assert!(app.view.node(Act::ForeignUpdate).is_some(), "install the component");
    assert!(app.view.node(Act::ForeignApp(0)).is_none());
    // the service answers "missing" too if asked
    assert!(!app.state.status.foreign.runtime.as_ref().unwrap().installed);
    app.poll().unwrap();
    assert!(
        !log(&world).iter().any(|l| l.starts_with("foreign.")),
        "nothing asked of the service before the person chooses"
    );
}

#[test]
fn granting_more_asks_and_taking_back_applies_at_once() {
    let (world, mut app) = open(Lang::En, ThemeKind::Dark, Some(world_with(true)));
    press(&mut app, Act::ForeignApp(0));
    assert_eq!(app.state.sub, SubPage::ForeignApp);
    assert!(app.view.node(Act::ForeignLan).is_none(), "no LAN without internet");
    press(&mut app, Act::ForeignInternet);
    assert!(last(&world).starts_with("foreign.set_choices foreign.deb-mousepad"));
    assert_eq!(app.state.status.foreign.jobs, [7], "waits for the administrator");
    assert_eq!(choices(&world).network, NetworkChoice::None, "not before it is confirmed");
    // the administrator confirmed: the service changed it and says the job is done
    {
        let mut w = world.borrow_mut();
        let f = w.system.foreign.as_mut().unwrap();
        f.apps[0].choices.network = NetworkChoice::Internet;
        f.steps.push(Progress {
            job: 7,
            step: Step::Done,
            percent: 100,
            apps: Vec::new(),
        });
    }
    app.poll().unwrap();
    assert!(app.state.status.foreign.jobs.is_empty());
    assert!(app.view.node(Act::ForeignLan).is_some(), "now the LAN switch");
    // taking back needs no confirmation
    world.borrow_mut().system.foreign.as_mut().unwrap().asking = None;
    press(&mut app, Act::ForeignInternet);
    assert_eq!(choices(&world).network, NetworkChoice::None);
    press(&mut app, Act::ForeignFolder(1));
    assert_eq!(choices(&world).folders, ["downloads"]);
    press(&mut app, Act::ForeignBack);
    assert_eq!(app.state.sub, SubPage::None);
}

#[test]
fn removing_asks_and_can_keep_the_data() {
    let (world, mut app) = open(Lang::En, ThemeKind::Dark, Some(world_with(true)));
    press(&mut app, Act::ForeignApp(0));
    press(&mut app, Act::ForeignKeep);
    assert!(app.state.status.foreign.keep_data);
    press(&mut app, Act::ForeignRemove);
    let purpose = app.state.dialog.as_ref().map(|d| d.purpose);
    assert_eq!(purpose, Some(DialogPurpose::ForeignRemove));
    press(&mut app, Act::DialogOk);
    assert_eq!(last(&world), "foreign.remove foreign.deb-mousepad keep=true");
    assert!(app.state.dialog.is_none());
    assert_eq!(app.state.sub, SubPage::None, "the page of a removed app closes");
    assert!(app.view.node(Act::ForeignApp(0)).is_none());
}

#[test]
fn the_area_is_updated_and_the_end_is_said() {
    let (world, mut app) = open(Lang::En, ThemeKind::Dark, Some(world_with(true)));
    press(&mut app, Act::ForeignUpdate);
    assert_eq!(last(&world), "foreign.runtime_update");
    assert_eq!(app.state.status.foreign.jobs, [9]);
    world
        .borrow_mut()
        .system
        .foreign
        .as_mut()
        .unwrap()
        .steps
        .push(Progress {
            job: 9,
            step: Step::Done,
            percent: 100,
            apps: Vec::new(),
        });
    app.poll().unwrap();
    assert!(app.state.status.foreign.jobs.is_empty());
}

#[test]
fn the_pages_in_arabic_and_english_dark_and_light() {
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            let (_w, mut app) = open(lang, theme, Some(world_with(true)));
            let tag = format!("{lang:?}-{}", theme.name()).to_lowercase();
            save(&mut app, &format!("settings-foreign-{tag}"));
            press(&mut app, Act::ForeignApp(0));
            save(&mut app, &format!("settings-foreign-app-{tag}"));
            press(&mut app, Act::ForeignRemove);
            save(&mut app, &format!("settings-foreign-remove-{tag}"));
            let (_w, mut app) = open(lang, theme, Some(world_with(false)));
            save(&mut app, &format!("settings-foreign-empty-{tag}"));
        }
    }
}

fn save(app: &mut App, name: &str) {
    app.ui.layout();
    if let Some(dir) = std::env::var_os("ULTAR_SETTINGS_SHOTS") {
        write_png(&std::path::Path::new(&dir).join(format!("{name}.png")), &mut app.ui);
    }
}
