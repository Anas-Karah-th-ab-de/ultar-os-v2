//! Every CLICKS.md entry of the boards Installer, InstallerDisk, InstallerDrivers and
//! InstallerSummary, driven by pointer and keyboard input.

mod common;

use common::*;
use ultar_i18n::Lang;
use ultar_installer_ui::action::Act;
use ultar_installer_ui::components::{Component, Scope};
use ultar_installer_ui::wizard::{DiskMode, Encryption, Step};
use ultar_installer_ui::InstallerApp;
use ultar_ui::event::{Event, Key, Modifiers};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Size;

fn key(app: &mut InstallerApp, k: Key) {
    app.handle(Event::Key {
        key: k,
        modifiers: Modifiers::NONE,
    })
    .expect("key");
}

fn node(app: &InstallerApp, act: Act) -> ultar_ui::ui::NodeId {
    app.view
        .registry
        .node(act)
        .unwrap_or_else(|| panic!("{act:?} not shown"))
}

fn at(
    step: Step,
) -> (
    std::path::PathBuf,
    ultar_installer_ui::ports::fake::Shared,
    InstallerApp,
) {
    let dir = scratch(&format!("clicks-{step:?}"));
    let src = medium(&dir);
    let tgt = target(&dir, 300 * MIB);
    let world = world();
    let mut app = app_with(
        &world,
        wizard_at(&src, vec![disk_info(&tgt, "Samsung SSD 980")], step),
        Lang::Ar,
        ThemeKind::Dark,
        Size::new(1280.0, 800.0),
    );
    if step == Step::Drivers {
        app.load_devices();
        app.rebuild().unwrap();
    }
    (dir, world, app)
}

fn radio(app: &mut InstallerApp, act: Act, index: usize) {
    let group = node(app, act);
    let tree = app.ui.a11y_tree();
    let part = tree
        .iter()
        .filter(|n| n.id == group && n.part.is_some())
        .nth(index)
        .expect("radio part");
    let c = part.rect.center();
    app.handle(Event::PointerMove { pos: c }).unwrap();
    app.handle(Event::PointerDown {
        pos: c,
        button: ultar_ui::event::PointerButton::Primary,
    })
    .unwrap();
    app.handle(Event::PointerUp {
        pos: c,
        button: ultar_ui::event::PointerButton::Primary,
    })
    .unwrap();
}

#[test]
fn steps_prev_next_scope_and_components() {
    let (dir, _w, mut app) = at(Step::Components);
    radio(&mut app, Act::Scope, 0);
    assert_eq!(app.model.wizard.scope, Scope::Base);
    assert!(
        app.model.wizard.components.is_empty(),
        "base: nothing extra"
    );
    radio(&mut app, Act::Scope, 1);
    assert!(
        app.model.wizard.components.contains(&Component::Firefox),
        "extended fills the list"
    );
    let linux = node(&app, Act::Component(Component::Linux));
    click(&mut app, linux);
    assert!(
        !app.model.wizard.components.contains(&Component::Firefox),
        "Firefox goes with Linux programs"
    );
    assert_eq!(app.model.wizard.scope, Scope::Custom);
    let firefox = node(&app, Act::Component(Component::Firefox));
    click(&mut app, firefox);
    assert!(
        app.model.wizard.components.contains(&Component::Linux),
        "Firefox selects Linux programs"
    );
    let prev = node(&app, Act::Prev);
    click(&mut app, prev);
    assert_eq!(app.model.wizard.step, Step::Drivers);
    let next = node(&app, Act::Next);
    click(&mut app, next);
    assert_eq!(app.model.wizard.step, Step::Components, "entries stay");
    assert!(app.model.wizard.components.contains(&Component::Firefox));
    let list = app.view.steps.unwrap();
    let item = app.view.registry.node(Act::Step(Step::Keyboard)).unwrap();
    let _ = list;
    click(&mut app, item);
    assert_eq!(
        app.model.wizard.step,
        Step::Keyboard,
        "jump back to a finished step"
    );
    let later = app.view.registry.node(Act::Step(Step::Summary)).unwrap();
    assert!(app.ui.is_disabled(later), "later steps cannot be jumped to");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn disk_and_encryption() {
    let (dir, _w, mut app) = at(Step::Disk);
    let next = node(&app, Act::Next);
    assert!(
        !app.ui.is_disabled(next),
        "whole disk, no encryption: can go on"
    );
    radio(&mut app, Act::DiskMode, 1);
    assert_eq!(app.model.wizard.disk_mode, DiskMode::Beside);
    let next = node(&app, Act::Next);
    assert!(
        app.ui.is_disabled(next),
        "beside: not available, reason shown"
    );
    let share = node(&app, Act::DiskShare);
    assert!(!app.ui.is_disabled(share));
    click(&mut app, share);
    key(&mut app, Key::ArrowRight);
    assert_ne!(app.model.wizard.share, 40, "share follows the slider");
    radio(&mut app, Act::DiskMode, 0);
    radio(&mut app, Act::EncMode, 1);
    assert_eq!(app.model.wizard.encryption, Encryption::Passphrase);
    let [a, b] = [app.view.passes[0], app.view.passes[1]];
    click(&mut app, a);
    for c in "abc".chars() {
        key(&mut app, Key::Char(c));
    }
    click(&mut app, b);
    for c in "abd".chars() {
        key(&mut app, Key::Char(c));
    }
    assert!(app.ui.error(b).is_some(), "passphrases differ");
    let tree = app.ui.a11y_tree();
    assert!(
        tree.iter().any(|n| n.id == a && n.part.is_some()),
        "installer.enc.show: show button of the field"
    );
    radio(&mut app, Act::EncMode, 0);
    let select = node(&app, Act::DiskSelect);
    click(&mut app, select);
    assert!(app.ui.is_open(select), "disk choice opens");
    key(&mut app, Key::Enter);
    let next = node(&app, Act::Next);
    click(&mut app, next);
    assert_eq!(app.model.wizard.step, Step::Account);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn drivers_page() {
    let (dir, world, mut app) = at(Step::Drivers);
    let fetch = node(&app, Act::DriversFetch);
    assert!(app.ui.is_disabled(fetch), "no network: disabled");
    assert!(app.ui.tooltip(fetch).is_some(), "with the reason");
    let all = node(&app, Act::DriversPlan(None));
    click(&mut app, all);
    assert_eq!(
        world.borrow().log.last().unwrap(),
        "files.save hardware-plan.toml"
    );
    let one = node(&app, Act::DriversPlan(Some(7)));
    click(&mut app, one);
    assert_eq!(
        world.borrow().log.last().unwrap(),
        "files.save hardware-plan-7.toml"
    );
    let alt = node(&app, Act::DriversAlt(1));
    click(&mut app, alt);
    key(&mut app, Key::ArrowDown);
    key(&mut app, Key::Enter);
    assert_eq!(
        world.borrow().log.last().unwrap(),
        "devices.switch pci:00:14.0 ultar-generic 1.0.0",
        "tried live at once"
    );
    let test = node(&app, Act::DriversTest(2));
    click(&mut app, test);
    assert_eq!(
        world.borrow().log.last().unwrap(),
        "devices.rescan pci:00:1f.0"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn summary_edit_returns_and_confirm() {
    let (dir, _w, mut app) = at(Step::Summary);
    let edit = node(&app, Act::Edit(Step::TimeZone));
    click(&mut app, edit);
    assert_eq!(app.model.wizard.step, Step::TimeZone);
    let next = node(&app, Act::Next);
    click(&mut app, next);
    assert_eq!(
        app.model.wizard.step,
        Step::Summary,
        "Next returns to the summary after Edit"
    );
    let confirm = node(&app, Act::Confirm);
    click(&mut app, confirm);
    assert!(app.model.wizard.erase_confirmed);
    std::fs::remove_dir_all(dir).unwrap();
}
