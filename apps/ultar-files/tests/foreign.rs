//! Installing an app from another system from Files (docs/FREMD-APPS.md, stage P3): the
//! sheet asks the service, starts with every permission off, says that nothing was checked,
//! follows the job and opens the app; what cannot be installed says why. Every button is
//! clicked; the sheet is drawn in Arabic and English, dark and light
//! (`ULTAR_FILES_SHOTS=<dir>` writes the pictures).
mod common;

use common::*;
use ultar_bus::messages::foreign::{
    Choices, Failure, InspectOutcome, Inspected, NetworkChoice, Progress, Source, Step,
};
use ultar_files::action::{Command, Target};
use ultar_files::foreign::Phase;
use ultar_files::model::Dialog;
use ultar_files::service::ForeignEvent;
use ultar_files::App;
use ultar_files::Mode;
use ultar_i18n::Lang;
use ultar_ui::event::ActionKind;
use ultar_ui::snapshot::{render, Image};
use ultar_ui::tokens::ThemeKind;

fn found(source: Source) -> InspectOutcome {
    InspectOutcome::Found(Inspected {
        source,
        name: "mousepad".into(),
        version: "0.6.2-1".into(),
        publisher: "Xubuntu Developers".into(),
        size_bytes: 412_000,
        verified: false,
        installable: source == Source::Deb,
        suggested: Choices {
            network: NetworkChoice::Internet,
            lan: true,
            folders: vec!["documents".into()],
        },
    })
}

fn phase(app: &App) -> Phase {
    match &app.state.dialog {
        Some(Dialog::Foreign(sheet)) => sheet.phase.clone(),
        other => panic!("no install sheet: {other:?}"),
    }
}

fn shot(app: &mut App, name: &str) {
    app.ui.layout();
    let img = Image::from_canvas(&render(&mut app.ui).unwrap());
    let first = &img.rgba[..4];
    assert!(img.rgba.chunks(4).any(|p| p != first), "{name}: painted");
    if let Ok(dir) = std::env::var("ULTAR_FILES_SHOTS") {
        let png = img.encode_png().unwrap();
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        ultar_ui::snapshot::write_atomic(&path, &png).unwrap();
    }
}

fn open_package(lang: Lang, file: &str, answer: InspectOutcome) -> (Root, App, Shared) {
    let r = Root::new();
    r.write(&format!("downloads/{file}"), b"!<arch>\n");
    let (mut app, fake) = app(&r, Mode::Window, lang, Some("files://downloads/"));
    fake.borrow_mut().inspect_answer = Some(answer);
    let item = node(&app, &Target::Item(file.into()));
    double_click(&mut app, item);
    assert_eq!(phase(&app), Phase::Reading);
    assert_eq!(
        fake.borrow().inspected,
        [format!("/data/ultar/areas/downloads/{file}")]
    );
    app.poll().unwrap();
    app.ui.layout();
    (r, app, Shared(fake))
}

#[test]
fn a_linux_package_installs_with_nothing_granted_until_the_person_chooses() {
    let (_r, mut app, fake) = open_package(Lang::En, "mousepad.deb", found(Source::Deb));
    assert!(matches!(phase(&app), Phase::Ready(_)));
    // the file suggests network and a folder: the sheet still starts with nothing
    assert!(app.built().node(&Target::ForeignLan).is_none());
    tap(&mut app, &Target::ForeignInternet);
    tap(&mut app, &Target::ForeignLan);
    tap(&mut app, &Target::ForeignLan);
    tap(&mut app, &Target::ForeignFolder("documents".into()));
    let list = app.built().sheet.expect("the sheet's list");
    assert!(
        app.ui.scroll_offset(list).unwrap() > 0.0,
        "the list stays where it was scrolled to"
    );
    let ok = node(&app, &Target::DialogOk);
    click(&mut app, ok);
    let installs = fake.0.borrow().installs.clone();
    assert_eq!(installs.len(), 1);
    let (path, choices) = &installs[0];
    assert_eq!(path, "/data/ultar/areas/downloads/mousepad.deb");
    assert_eq!(choices.network, NetworkChoice::Internet);
    assert!(!choices.lan, "clicked twice: off again");
    assert_eq!(choices.folders, ["documents"]);
    app.poll().unwrap();
    assert!(matches!(phase(&app), Phase::Running { job: 1, .. }));
    step(&mut app, &fake, Step::Install, &[]);
    assert!(matches!(
        phase(&app),
        Phase::Running {
            step: Step::Install,
            ..
        }
    ));
    step(&mut app, &fake, Step::Done, &["foreign.deb-mousepad"]);
    assert_eq!(phase(&app), Phase::Done(vec!["foreign.deb-mousepad".into()]));
    let open = node(&app, &Target::DialogOk);
    click(&mut app, open);
    assert!(app.state.dialog.is_none());
    assert_eq!(
        fake.0.borrow().opened,
        [("foreign.deb-mousepad".to_owned(), String::new())]
    );
}

/// Scrolls the sheet to `target` like a person would, then clicks it.
fn tap(app: &mut App, target: &Target) {
    let id = node(app, target);
    app.ui.scroll_into_view(id).unwrap();
    click(app, id);
}

fn step(app: &mut App, fake: &Shared, step: Step, apps: &[&str]) {
    fake.0
        .borrow_mut()
        .foreign_events
        .push_back(ForeignEvent::Progress(Progress {
            job: 1,
            step,
            percent: 60,
            apps: apps.iter().map(|a| (*a).to_owned()).collect(),
        }));
    app.poll().unwrap();
    app.ui.layout();
}

#[test]
fn closing_a_running_installation_lets_it_finish_and_says_so() {
    let (_r, mut app, fake) = open_package(Lang::En, "mousepad.deb", found(Source::Deb));
    let ok = node(&app, &Target::DialogOk);
    click(&mut app, ok);
    app.poll().unwrap();
    app.ui.layout();
    let close = node(&app, &Target::DialogCancel);
    click(&mut app, close);
    assert!(app.state.dialog.is_none());
    assert_eq!(app.state.foreign_job, Some((1, "mousepad.deb".into())));
    step(&mut app, &fake, Step::Failed(Failure::Refused), &[]);
    let notice = app.state.notice.clone().expect("a notice");
    assert_eq!(notice.key, "files.notice.install_failed");
    assert!(app.state.foreign_job.is_none());
}

#[test]
fn what_cannot_be_installed_yet_says_why_and_closes() {
    for (file, source, key) in [
        ("setup.exe", Source::Exe, "files.foreign.not_yet"),
        ("app.apk", Source::Apk, "files.foreign.not_yet"),
        ("odd.msi", Source::Unknown, "files.foreign.unknown"),
    ] {
        let (_r, mut app, fake) = open_package(Lang::En, file, found(source));
        assert_eq!(phase(&app), Phase::Problem(key));
        assert!(app.built().node(&Target::DialogOk).is_none());
        let close = node(&app, &Target::DialogCancel);
        click(&mut app, close);
        assert!(app.state.dialog.is_none());
        assert!(fake.0.borrow().installs.is_empty(), "nothing installed");
    }
}

#[test]
fn without_the_service_the_sheet_says_it_does_not_answer() {
    let r = Root::new();
    r.write("downloads/a.deb", b"x");
    let (mut app, _fake) = app(&r, Mode::Window, Lang::En, Some("files://downloads/"));
    let item = node(&app, &Target::Item("a.deb".into()));
    double_click(&mut app, item);
    app.poll().unwrap();
    assert_eq!(phase(&app), Phase::Problem("files.foreign.no_service"));
}

#[test]
fn the_context_menu_offers_install_for_one_package_only() {
    let r = Root::new();
    r.write("downloads/a.deb", b"x");
    r.write("downloads/b.txt", b"x");
    let (mut app, fake) = app(&r, Mode::Window, Lang::En, Some("files://downloads/"));
    fake.borrow_mut().inspect_answer = Some(found(Source::Deb));
    let menu_ids = |app: &App| -> Vec<u32> {
        ultar_files::view::item_menu(&app.state)
            .items
            .iter()
            .map(|i| i.id)
            .collect()
    };
    app.state.selected = vec!["b.txt".into()];
    assert!(!menu_ids(&app).contains(&Command::Install.id()));
    app.state.selected = vec!["a.deb".into()];
    assert!(menu_ids(&app).contains(&Command::Install.id()));
    let items = node(&app, &Target::Items);
    act(
        &mut app,
        items,
        ActionKind::MenuChosen(Command::Install.id()),
    );
    assert_eq!(phase(&app), Phase::Reading);
    assert_eq!(fake.borrow().inspected, ["/data/ultar/areas/downloads/a.deb"]);
}

#[test]
fn the_sheet_in_arabic_and_english_dark_and_light() {
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            let (_r, mut app, fake) = open_package(lang, "mousepad.deb", found(Source::Deb));
            app.ui.set_theme(theme);
            app.rebuild().unwrap();
            let tag = format!("{lang:?}-{}", theme.name()).to_lowercase();
            shot(&mut app, &format!("files-foreign-ready-{tag}"));
            tap(&mut app, &Target::ForeignInternet);
            assert!(app.built().node(&Target::ForeignLan).is_some(), "internet is on");
            shot(&mut app, &format!("files-foreign-choices-{tag}"));
            let ok = node(&app, &Target::DialogOk);
            click(&mut app, ok);
            app.poll().unwrap();
            step(&mut app, &fake, Step::Install, &[]);
            shot(&mut app, &format!("files-foreign-running-{tag}"));
            step(&mut app, &fake, Step::Done, &["foreign.deb-mousepad"]);
            shot(&mut app, &format!("files-foreign-done-{tag}"));
            let (_r2, mut app, _f) = open_package(lang, "setup.exe", found(Source::Exe));
            app.ui.set_theme(theme);
            app.rebuild().unwrap();
            shot(&mut app, &format!("files-foreign-not-yet-{tag}"));
        }
    }
}

#[test]
fn a_package_on_the_desktop_installs_from_its_menu() {
    let r = Root::new();
    r.write("desktop/mousepad.deb", b"!<arch>\n");
    let (mut app, fake) = app(&r, Mode::Desk, Lang::Ar, None);
    fake.borrow_mut().inspect_answer = Some(found(Source::Deb));
    let item = node(&app, &Target::Item("mousepad.deb".into()));
    click_with(
        &mut app,
        item,
        ultar_ui::event::Modifiers::default(),
        ultar_ui::event::PointerButton::Secondary,
    );
    assert_eq!(app.ui.menu_owner(), Some(item));
    act(&mut app, item, ActionKind::MenuChosen(Command::Install.id()));
    assert_eq!(phase(&app), Phase::Reading);
    assert_eq!(
        fake.borrow().inspected,
        ["/data/ultar/areas/desktop/mousepad.deb"]
    );
}
