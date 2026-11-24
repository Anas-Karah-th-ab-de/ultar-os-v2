//! Every CLICKS.md entry of the boards SettingsDisplay(+Light), SettingsDrivers, Account,
//! CompactSettings and CompactDisplay, driven by real pointer and keyboard input: each click
//! must do what the click list says (service call, page change, dialog) – no dead clicks.

mod common;

use common::*;
use ultar_bus::messages::account_service::AccountService;
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::areas::index_of;
use ultar_settings::state::{DialogPurpose, SubPage};
use ultar_settings::App;
use ultar_ui::event::Key;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Size;

fn wide(area: &str, signed_in: bool) -> (ultar_settings::service::fake::Shared, App) {
    let world = board_world(3, signed_in);
    let app = app(
        &world,
        Lang::Ar,
        ThemeKind::Dark,
        content_size(1440.0, 900.0),
        area,
    );
    (world, app)
}

fn node(app: &App, act: Act) -> ultar_ui::ui::NodeId {
    app.view
        .node(act)
        .unwrap_or_else(|| panic!("{act:?} not shown"))
}

/// Opens a select with a click and takes the option after the selected one.
fn choose_next(app: &mut App, act: Act) {
    let n = node(app, act);
    click(app, n);
    assert!(app.ui.is_open(n), "{act:?} opens its list");
    key(app, Key::ArrowDown);
    key(app, Key::Enter);
}

fn last(world: &ultar_settings::service::fake::Shared) -> String {
    log(world).last().cloned().unwrap_or_default()
}

#[test]
fn settings_area_and_search() {
    let (world, mut app) = wide("displays", true);
    let bt = app
        .view
        .node(Act::Area(index_of("bluetooth").unwrap()))
        .unwrap();
    click(&mut app, bt);
    assert_eq!(app.state.area, index_of("bluetooth").unwrap(), "area opens");
    let power = app
        .view
        .node(Act::Area(index_of("power").unwrap()))
        .unwrap();
    click(&mut app, power);
    assert_eq!(app.state.area, index_of("power").unwrap());
    let search = node(&app, Act::Search);
    click(&mut app, search);
    type_text(&mut app, "مظهر");
    assert_eq!(
        app.view.nav_areas,
        vec![index_of("appearance").unwrap()],
        "search filters areas"
    );
    key(&mut app, Key::Enter);
    assert_eq!(
        app.state.area,
        index_of("appearance").unwrap(),
        "Enter opens the result"
    );
    assert!(
        app.view.node(Act::PanelItem(0)).is_some(),
        "generic settingsd page"
    );
    let theme = node(&app, Act::PanelItem(0));
    click(&mut app, theme);
    key(&mut app, Key::ArrowDown);
    key(&mut app, Key::Enter);
    assert!(
        last(&world).starts_with("settings.set appearance.theme="),
        "{:?}",
        log(&world)
    );
    let sound = app.view.node(Act::Area(index_of("sound").unwrap()));
    assert!(sound.is_none(), "filtered out");
}

#[test]
fn display_arrangement_mode_and_rows() {
    let (world, mut app) = wide("displays", true);
    let group = node(&app, Act::DisplaySelect);
    app.ui.layout();
    let r = app.ui.rect(group).unwrap();
    // the arrangement canvas: physical left to right; the first tile starts 81 in
    click_at(
        &mut app,
        ultar_ui::Point::new(r.x + 81.0 + 72.0, r.y + r.h / 2.0),
    );
    assert_eq!(
        app.state.display.current().unwrap().id,
        DELL_A,
        "first tile (physical left) selects display 2"
    );
    assert!(
        app.view.node(Act::DisplayMakePrimary).is_some(),
        "non-main display offers the button (§13 no. 64)"
    );
    let make = node(&app, Act::DisplayMakePrimary);
    click(&mut app, make);
    assert_eq!(
        last(&world),
        format!("settings.set display.primary=Text(\"{DELL_A}\")")
    );
    let identify = node(&app, Act::DisplayIdentify);
    click(&mut app, identify);
    assert!(last(&world).starts_with("display.identify "));
    let detect = node(&app, Act::DisplayDetect);
    let toasts = app.toasts_shown();
    click(&mut app, detect);
    assert_eq!(last(&world), "display.detect");
    assert_eq!(app.toasts_shown(), toasts + 1, "detect reports the result");
}

#[test]
fn display_mode_and_page_rows() {
    let (world, mut app) = wide("displays", true);
    let mode = node(&app, Act::DisplayMode);
    app.ui.layout();
    let r = app.ui.rect(mode).unwrap();
    // tiles side by side in reading order: "only one display" is the last, at the left
    click_at(&mut app, ultar_ui::Point::new(r.x + 12.0, r.y + r.h / 2.0));
    assert_eq!(last(&world), "settings.set display.mode=Text(\"single\")");
    assert!(
        app.view.node(Act::DisplaySingle).is_some(),
        "only one display → which stays on"
    );
    choose_next(&mut app, Act::DisplaySingle);
    assert!(last(&world).starts_with("settings.set display.single_display="));
    for (act, key_) in [
        (Act::DisplayNightlight, "display.night_light.enabled"),
        (Act::DisplayRemember, "display.remember_window_positions"),
    ] {
        let n = node(&app, act);
        click(&mut app, n);
        assert!(
            last(&world).starts_with(&format!("settings.set {key_}=Flag(false)")),
            "{:?}",
            log(&world)
        );
    }
    choose_next(&mut app, Act::DisplayLid);
    assert_eq!(
        last(&world),
        "settings.set display.lid_close_with_external=Text(\"suspend\")"
    );
    let bar = node(&app, Act::DisplayBar);
    click(&mut app, bar);
    key(&mut app, Key::ArrowUp);
    key(&mut app, Key::Enter);
    assert_eq!(last(&world), "settings.set bar.displays=Text(\"primary\")");
}

#[test]
fn display_card_choices_and_hdr_and_brightness() {
    let (world, mut app) = wide("displays", true);
    choose_next(&mut app, Act::DisplayRefresh);
    assert!(
        last(&world).starts_with(&format!(
            "settings.set display.{INTERNAL}.refresh_rate=Decimal(48"
        )),
        "{:?}",
        log(&world)
    );
    choose_next(&mut app, Act::DisplayResolution);
    assert_eq!(
        last(&world),
        format!("settings.set display.{INTERNAL}.resolution=Text(\"2560x1440\")")
    );
    choose_next(&mut app, Act::DisplayScale);
    assert_eq!(
        last(&world),
        format!("settings.set display.{INTERNAL}.scale=Integer(225)")
    );
    choose_next(&mut app, Act::DisplayOrientation);
    assert_eq!(
        last(&world),
        format!("settings.set display.{INTERNAL}.orientation=Text(\"portrait\")")
    );
    let hdr = node(&app, Act::DisplayHdr);
    assert!(app.ui.is_disabled(hdr), "HDR disabled: not supported");
    let before = log(&world).len();
    click(&mut app, hdr);
    assert_eq!(log(&world).len(), before, "a disabled switch does nothing");
    let brightness = node(&app, Act::DisplayBrightness);
    click(&mut app, brightness);
    key(&mut app, Key::ArrowRight);
    assert!(
        last(&world).starts_with(&format!(
            "settings.set display.{INTERNAL}.brightness=Integer("
        )),
        "{:?}",
        log(&world)
    );
    let profile = node(&app, Act::DisplayProfile);
    click(&mut app, profile);
    assert!(
        app.ui.is_open(profile),
        "profile choice opens (automatic only offered)"
    );
    key(&mut app, Key::Escape);
    let more = node(&app, Act::DisplayNightlightMore);
    click(&mut app, more);
    assert_eq!(app.state.sub, SubPage::NightLight, "night light page");
    choose_next(&mut app, Act::NightSchedule);
    assert_eq!(
        last(&world),
        "settings.set display.night_light.schedule=Text(\"custom\")"
    );
    let back = node(&app, Act::NightBack);
    click(&mut app, back);
    assert_eq!(app.state.sub, SubPage::None);
}

#[test]
fn drivers_clicks() {
    let (world, mut app) = wide("drivers", true);
    choose_next(&mut app, Act::DriverAlt(0));
    assert!(
        matches!(
            app.state.dialog.as_ref().map(|d| d.purpose),
            Some(DialogPurpose::DriverSwitch { row: 0, alt: 0 })
        ),
        "asks first"
    );
    let ok = node(&app, Act::DialogOk);
    click(&mut app, ok);
    assert_eq!(
        last(&world),
        "devices.switch pci:00:02.0 ultar-gfx-intel-legacy 1.0.0"
    );
    assert!(app.state.dialog.is_none());
    let back = node(&app, Act::DriverRollback(1));
    click(&mut app, back);
    assert_eq!(last(&world), "devices.rollback pci:00:14.0");
    let plan = node(&app, Act::DriverPlan(7));
    click(&mut app, plan);
    assert_eq!(last(&world), "files.save hardware-plan-usb-1-7.toml");
    let saved = String::from_utf8(world.borrow().files[0].1.clone()).unwrap();
    assert!(saved.contains("hardware_id = \"usb:27c6:5395\""));
    let check = node(&app, Act::FirmwareCheck);
    click(&mut app, check);
    assert_eq!(last(&world), "update.check");
    let usb = node(&app, Act::FirmwareUsb);
    click(&mut app, usb);
    assert_eq!(
        app.state.dialog.as_ref().map(|d| d.purpose),
        Some(DialogPurpose::FirmwareUsb)
    );
    let field = app.view.dialog_fields[0];
    click(&mut app, field);
    type_text(&mut app, "../x");
    let ok = node(&app, Act::DialogOk);
    click(&mut app, ok);
    assert!(
        app.state.dialog.as_ref().is_some_and(|d| d.error.is_some()),
        "relative path refused"
    );
    let field = app.view.dialog_fields[0];
    click(&mut app, field);
    for _ in 0..4 {
        key(&mut app, Key::Backspace);
    }
    type_text(&mut app, "/media/usb/ultar-update.uup");
    let ok = node(&app, Act::DialogOk);
    click(&mut app, ok);
    assert_eq!(last(&world), "update.offline /media/usb/ultar-update.uup");
    let test = node(&app, Act::SelfTest);
    click(&mut app, test);
    assert_eq!(last(&world), "devices.rescan");
}

#[test]
fn account_signed_in_clicks() {
    let (world, mut app) = wide("accounts", true);
    let copy = node(&app, Act::AccountCopy);
    click(&mut app, copy);
    assert_eq!(world.borrow().clipboard.as_deref(), Some("UOS-XXXX-XXXX"));
    let qr = node(&app, Act::AccountQr);
    click(&mut app, qr);
    assert_eq!(
        app.state.dialog.as_ref().map(|d| d.purpose),
        Some(DialogPurpose::Qr)
    );
    key(&mut app, Key::Escape);
    assert!(app.state.dialog.is_none(), "Esc closes");
    for s in AccountService::ALL {
        let n = node(&app, Act::AccountServiceSwitch(s));
        click(&mut app, n);
        assert!(last(&world).starts_with(&format!("account.set_service {s:?}")));
    }
    let change = node(&app, Act::AccountChangePassword);
    click(&mut app, change);
    let fields = app.view.dialog_fields.clone();
    for (f, t) in fields
        .iter()
        .zip(["old-pass-1", "new-pass-123", "other-pass-9"])
    {
        click(&mut app, *f);
        type_text(&mut app, t);
    }
    let ok = node(&app, Act::DialogOk);
    click(&mut app, ok);
    assert!(
        app.state.dialog.as_ref().is_some_and(|d| d.error.is_some()),
        "passwords differ"
    );
    let cancel = node(&app, Act::DialogCancel);
    click(&mut app, cancel);
    assert!(app.state.dialog.is_none());
    let dev_out = node(&app, Act::AccountDeviceSignOut(1));
    click(&mut app, dev_out);
    assert_eq!(last(&world), "account.device_sign_out 2");
    let remove = node(&app, Act::AccountDeviceRemove(2));
    click(&mut app, remove);
    let ok = node(&app, Act::DialogOk);
    click(&mut app, ok);
    assert_eq!(last(&world), "account.device_remove 3");
    let delete = node(&app, Act::AccountDelete);
    click(&mut app, delete);
    let f = app.view.dialog_fields[0];
    click(&mut app, f);
    type_text(&mut app, "secret-pass");
    let ok = node(&app, Act::DialogOk);
    click(&mut app, ok);
    assert_eq!(last(&world), "account.delete");
    assert!(
        app.view.node(Act::AccountSignIn).is_some(),
        "signed out after delete"
    );
}

#[test]
fn account_signed_out_clicks() {
    let (world, mut app) = wide("accounts", false);
    let user = node(&app, Act::AccountUser);
    click(&mut app, user);
    type_text(&mut app, "anas");
    let pw = node(&app, Act::AccountPassword);
    click(&mut app, pw);
    type_text(&mut app, "Str0ng-Passw0rd");
    let strength = app
        .view
        .live
        .iter()
        .filter(|(a, _)| *a == Act::AccountPassword)
        .map(|(_, n)| *n)
        .nth(1)
        .unwrap();
    assert_eq!(
        app.ui.progress(strength).unwrap(),
        Some(1.0),
        "strength meter follows typing"
    );
    let show = node(&app, Act::AccountShow);
    click(&mut app, show);
    let revealed = matches!(
        app.ui.widget(pw),
        Some(ultar_ui::widgets::Widget::TextField(f)) if f.is_revealed()
    );
    assert!(
        revealed,
        "account.show: the button beside the field shows the password"
    );
    let server = node(&app, Act::AccountServer);
    assert!(
        !app.ui.is_disabled(server),
        "server editable while signed out"
    );
    let signin = node(&app, Act::AccountSignIn);
    click(&mut app, signin);
    assert_eq!(last(&world), "account.sign_in anas");
    let server = node(&app, Act::AccountServer);
    assert!(app.ui.is_disabled(server), "server locked while signed in");
    let out = node(&app, Act::AccountSignOut);
    click(&mut app, out);
    assert_eq!(last(&world), "account.sign_out");
}

#[test]
fn account_create_page_and_back() {
    let (world, mut app) = wide("accounts", false);
    let create = node(&app, Act::AccountCreate);
    click(&mut app, create);
    assert_eq!(app.state.sub, SubPage::CreateAccount);
    let fields = app.view.dialog_fields.clone();
    for (f, t) in fields
        .iter()
        .zip(["layla", "Passw0rd-123", "Passw0rd-123", ""])
    {
        click(&mut app, *f);
        type_text(&mut app, t);
    }
    let ok = node(&app, Act::DialogOk);
    click(&mut app, ok);
    assert_eq!(last(&world), "account.create layla");
    let back = node(&app, Act::AccountBack);
    click(&mut app, back);
    assert_ne!(
        app.state.area,
        index_of("accounts").unwrap(),
        "back leaves the page"
    );
}

#[test]
fn compact_list_back_and_display() {
    let world = board_world(1, true);
    let mut app = app(
        &world,
        Lang::Ar,
        ThemeKind::Dark,
        Size::new(360.0, 712.0),
        "displays",
    );
    let back = node(&app, Act::Back);
    click(&mut app, back);
    assert!(app.state.compact_list, "settings.back returns to the list");
    let displays = node(&app, Act::Area(index_of("displays").unwrap()));
    click(&mut app, displays);
    assert!(!app.state.compact_list);
    let auto = node(&app, Act::DisplayAutoBrightness);
    click(&mut app, auto);
    assert_eq!(
        last(&world),
        "settings.set display.auto_brightness=Flag(false)"
    );
    assert!(
        app.view.node(Act::DisplaySelect).is_none(),
        "no arrangement with one display"
    );
    let wifi = {
        let back = node(&app, Act::Back);
        click(&mut app, back);
        node(&app, Act::Area(index_of("wifi").unwrap()))
    };
    click(&mut app, wifi);
    assert_eq!(
        app.state.area,
        index_of("wifi").unwrap(),
        "area without service opens its empty state"
    );
}

#[test]
fn network_bluetooth_and_sound_pages_show_what_the_services_report() {
    use ultar_bus::messages::{audio::AudioState, net::NetState, net::Wired};
    let world = board_world(1, true);
    world.borrow_mut().net = Some(NetState {
        wired: Some(Wired {
            speed_mbit: Some(1000),
        }),
        ..NetState::default()
    });
    world.borrow_mut().audio = Some(AudioState {
        present: true,
        volume_percent: 40,
        output_name: "Speakers".into(),
        ..AudioState::default()
    });
    for area in ["network", "wifi", "sound"] {
        let app = app(
            &world,
            Lang::En,
            ThemeKind::Dark,
            content_size(1440.0, 900.0),
            area,
        );
        assert_eq!(app.state.status.error, None, "{area}");
    }
    let net = ultar_settings::view::status::net_rows(world.borrow().net.as_ref().unwrap());
    assert!(net
        .iter()
        .any(|(k, v)| *k == "app.settings.status.net.wired" && format!("{v:?}").contains("1000")));
    // a service that does not answer: the page says why (never an empty page, never invented values)
    let app = app(
        &world,
        Lang::En,
        ThemeKind::Dark,
        content_size(1440.0, 900.0),
        "bluetooth",
    );
    assert_eq!(
        app.state.status.error.as_ref().map(|t| t.key.as_str()),
        Some("app.settings.error.unavailable")
    );
}
