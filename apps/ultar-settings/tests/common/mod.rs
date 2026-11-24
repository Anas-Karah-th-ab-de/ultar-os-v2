//! Shared test support: the data of the boards as fake services, app construction, input.
#![allow(dead_code)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use ultar_bus::messages::account_service::{AccountService, AccountState, DeviceEntry, Phase};
use ultar_bus::messages::device_service::{DeviceInfo, DevicePhase, DriverInfo};
use ultar_bus::messages::display_service::{Mode, Output};
use ultar_bus::messages::settings::{
    ChoiceInfo, ItemInfo, ItemKind, PanelInfo, RiskInfo, ScopeInfo, SettingValue,
};
use ultar_bus::{Text, TextArg};
use ultar_i18n::Lang;
use ultar_settings::areas::index_of;
use ultar_settings::service::fake::{ports, Shared, World};
use ultar_settings::App;
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::{Point, Size};

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub const INTERNAL: &str = "boe-0abc-0";
pub const DELL_A: &str = "del-u2422h-1";
pub const DELL_B: &str = "del-u2422h-2";

fn mode(w: u32, h: u32, hz: u32) -> Mode {
    Mode {
        width: w,
        height: h,
        refresh_mhz: hz * 1000,
    }
}

fn item(key: &str, kind: ItemKind, value: SettingValue) -> ItemInfo {
    let label = key
        .split('.')
        .filter(|p| !p.contains('-'))
        .collect::<Vec<_>>()
        .join(".");
    ItemInfo {
        key: key.into(),
        label: Text::key(&format!("settings.{label}.label")),
        description: Text::key(&format!("settings.{label}.desc")),
        kind,
        value: value.clone(),
        default: value,
        risk: RiskInfo::Safe,
        scope: ScopeInfo::System,
        writable: true,
        pending: false,
        template: false,
    }
}

fn choice(ids: &[&str], key: &str) -> ItemKind {
    ItemKind::Choice {
        choices: ids
            .iter()
            .map(|id| ChoiceInfo {
                id: (*id).into(),
                label: Text::key(&format!("settings.{key}.choice.{id}")),
            })
            .collect(),
    }
}

fn text(v: &str) -> SettingValue {
    SettingValue::Text(v.into())
}

fn int(min: i64, max: i64, step: i64) -> ItemKind {
    ItemKind::Integer { min, max, step }
}

fn fmt(format: &str) -> ItemKind {
    ItemKind::Text {
        format: format.into(),
    }
}

fn per_display(id: &str, x: i64, res: &str, scale: i64, brightness: i64) -> Vec<ItemInfo> {
    let k = |f: &str| format!("display.{id}.{f}");
    let orientations = choice(
        &[
            "landscape",
            "portrait",
            "landscape_flipped",
            "portrait_flipped",
        ],
        "display.orientation",
    );
    let hz = ItemKind::Decimal {
        min: 0.0,
        max: 1000.0,
        step: 0.0,
    };
    vec![
        item(
            &k("position_x"),
            int(-65536, 65536, 1),
            SettingValue::Integer(x),
        ),
        item(
            &k("position_y"),
            int(-65536, 65536, 1),
            SettingValue::Integer(0),
        ),
        item(&k("resolution"), fmt("resolution"), text(res)),
        item(&k("scale"), int(100, 300, 25), SettingValue::Integer(scale)),
        item(&k("refresh_rate"), hz, SettingValue::Decimal(60.0)),
        item(&k("orientation"), orientations, text("landscape")),
        item(&k("hdr"), ItemKind::Toggle, SettingValue::Flag(false)),
        item(&k("color_profile"), fmt("free"), text("")),
        item(
            &k("brightness"),
            int(0, 100, 1),
            SettingValue::Integer(brightness),
        ),
    ]
}

fn display_items() -> Vec<ItemInfo> {
    let modes = choice(&["extend", "mirror", "single"], "display.mode");
    let schedule = choice(
        &["sunset_to_sunrise", "custom", "always"],
        "display.night_light.schedule",
    );
    let lid = choice(&["stay_on", "suspend"], "display.lid_close_with_external");
    let list = ItemKind::List {
        format: "display_id".into(),
        max_len: 16,
    };
    vec![
        item("display.mode", modes, text("extend")),
        item("display.mirror_displays", list, SettingValue::List(vec![])),
        item("display.single_display", fmt("display_id"), text("")),
        item("display.primary", fmt("display_id"), text(INTERNAL)),
        item(
            "display.night_light.enabled",
            ItemKind::Toggle,
            SettingValue::Flag(true),
        ),
        item(
            "display.night_light.schedule",
            schedule,
            text("sunset_to_sunrise"),
        ),
        item(
            "display.night_light.temperature",
            int(1700, 6500, 100),
            SettingValue::Integer(4000),
        ),
        item(
            "display.remember_window_positions",
            ItemKind::Toggle,
            SettingValue::Flag(true),
        ),
        item("display.lid_close_with_external", lid, text("stay_on")),
    ]
}

/// settingsd as on the boards (three displays, extend, night light on …).
pub fn panels(displays: usize) -> Vec<PanelInfo> {
    let mut items = display_items();
    items.extend(per_display(INTERNAL, 1920, "3840x2160", 200, 78));
    if displays == 1 {
        // a phone: settingsd lists the light-sensor setting (capability `sensors`)
        items.push(item(
            "display.auto_brightness",
            ItemKind::Toggle,
            SettingValue::Flag(true),
        ));
    }
    if displays > 1 {
        items.extend(per_display(DELL_A, 0, "1920x1080", 100, 80));
        items.extend(per_display(DELL_B, 3840, "1920x1080", 100, 80));
    }
    let bar = vec![item(
        "bar.displays",
        choice(&["primary", "all"], "bar.displays"),
        text("all"),
    )];
    let appearance = vec![
        item(
            "appearance.theme",
            choice(&["dark", "light"], "appearance.theme"),
            text("dark"),
        ),
        item(
            "appearance.font_size",
            int(12, 20, 1),
            SettingValue::Integer(14),
        ),
    ];
    let p = |id: &str, items| PanelInfo {
        id: id.into(),
        title: Text::key(&format!("settings.panel.{id}")),
        items,
    };
    vec![
        p("display", items),
        p("desktop_bar", bar),
        p("appearance", appearance),
    ]
}

pub fn outputs(displays: usize) -> Vec<Output> {
    let o =
        |id: &str, connector: &str, name: &str, internal, diag, m: Mode, modes: Vec<Mode>| Output {
            id: id.into(),
            connector: connector.into(),
            name: name.into(),
            internal,
            diagonal_tenths: diag,
            preferred: m,
            current: m,
            modes,
            hdr: false,
            brightness: internal,
            profiles: vec![],
        };
    let big = mode(3840, 2160, 60);
    let fhd = mode(1920, 1080, 60);
    let mut list = vec![o(
        INTERNAL,
        "edp-1",
        "",
        true,
        156,
        big,
        vec![big, mode(3840, 2160, 48), mode(2560, 1440, 60), fhd],
    )];
    if displays > 1 {
        list.push(o(
            DELL_A,
            "dp-1",
            "Dell",
            false,
            240,
            fhd,
            vec![fhd, mode(1920, 1080, 75), mode(1280, 720, 60)],
        ));
        list.push(o(
            DELL_B,
            "dp-2",
            "Dell",
            false,
            240,
            fhd,
            vec![fhd, mode(1920, 1080, 75), mode(1280, 720, 60)],
        ));
    }
    list
}

fn driver(id: &str, class: &str) -> DriverInfo {
    DriverInfo {
        id: id.into(),
        version: "1.0.0".into(),
        class: class.into(),
        name_key: format!("driver.{id}"),
        quarantined: None,
    }
}

/// The devices of board SettingsDrivers.
pub fn devices() -> Vec<DeviceInfo> {
    let dev = |n: u8, label: &str, id: &str, class: &str, reason: Option<Text>| DeviceInfo {
        address: format!("pci:00:{n:02x}.0"),
        hardware_id: format!("pci:8086:{n:04x}"),
        label: Some(label.into()),
        phase: DevicePhase::Active,
        active: Some(driver(id, class)),
        alternatives: vec![driver(id, class), driver(&format!("{id}-legacy"), class)],
        previous: Some(format!("{id}-legacy")),
        capabilities: vec![class.into()],
        reason,
    };
    let mut list = vec![
        dev(
            2,
            "Intel Arc (مدمجة)",
            "ultar-gfx-intel",
            "gpu",
            Some(Text::key("app.settings.drivers.note.full_accel")),
        ),
        dev(20, "Intel AX211", "ultar-wifi-iwl", "wifi", None),
        dev(31, "Realtek RTL8125", "ultar-net-r8125", "net", None),
        dev(14, "Realtek ALC295", "ultar-audio-hda", "audio_out", None),
        dev(21, "Intel AX211", "ultar-bt-intel", "bluetooth", None),
        dev(25, "ELAN I2C", "ultar-input-i2c", "input", None),
        dev(
            26,
            "USB UVC",
            "ultar-cam-uvc",
            "camera",
            Some(
                Text::key("app.settings.drivers.limited.max_res")
                    .with(TextArg::Verbatim("720p".into())),
            ),
        ),
    ];
    list.push(DeviceInfo {
        address: "usb:1-7".into(),
        hardware_id: "usb:27c6:5395".into(),
        label: Some("Goodix 27C6".into()),
        phase: DevicePhase::NoDriver,
        active: None,
        alternatives: vec![],
        previous: None,
        capabilities: vec!["fingerprint".into()],
        reason: Some(Text::key("app.settings.drivers.note.no_fp_login")),
    });
    list
}

/// The account of board Account (signed in or out).
pub fn account(signed_in: bool) -> AccountState {
    let d = |id: &str, name: &str, kind: &str, this, seen| DeviceEntry {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        this_device: this,
        last_seen: seen,
    };
    AccountState {
        phase: if signed_in {
            Phase::SignedIn
        } else {
            Phase::SignedOut
        },
        user: if signed_in {
            "anas".into()
        } else {
            String::new()
        },
        server: "ultar.code-craft.tech".into(),
        number: if signed_in {
            "UOS-XXXX-XXXX".into()
        } else {
            String::new()
        },
        services: vec![
            AccountService::Vpn,
            AccountService::StorePurchases,
            AccountService::MeshOverInternet,
        ],
        devices: if signed_in {
            vec![
                d("1", "ultar-pc", "laptop", true, 0),
                d("2", "KM One", "phone", false, 1_800_000_000),
                d("3", "ultar-office", "desktop", false, 1_799_000_000),
            ]
        } else {
            vec![]
        },
        error: None,
        attempts_remaining: 3,
        locked_minutes: 0,
    }
}

/// The world of the boards.
pub fn board_world(displays: usize, signed_in: bool) -> Shared {
    Rc::new(RefCell::new(World {
        panels: Some(panels(displays)),
        outputs: Some(outputs(displays)),
        devices: Some(devices()),
        account: Some(account(signed_in)),
        ..World::default()
    }))
}

/// Catalogs of settingsd (installed on the system next to the app's).
pub fn translator(lang: Lang) -> ultar_i18n::Translator {
    let mut tr = ultar_settings::i18n::translator(lang, None);
    for l in Lang::ALL {
        let path = repo().join(format!(
            "system/core/ultar-settingsd/locales/{}.ftl",
            l.code()
        ));
        if let Ok(source) = std::fs::read_to_string(path) {
            tr.add_source(l, &source).expect("settingsd catalog");
        }
    }
    tr.set_lang(lang);
    tr
}

/// Content size of the board (the compositor draws the title bar above it).
pub fn content_size(board_w: f32, board_h: f32) -> Size {
    Size::new(board_w, board_h - ultar_ui::tokens::size::TITLEBAR_HEIGHT)
}

pub fn app(world: &Shared, lang: Lang, theme: ThemeKind, size: Size, area: &str) -> App {
    let env = PaintEnv::new(lang, theme);
    let ui = Ui::new(env, InputMode::Pointer, size, translator(lang)).expect("ui");
    App::new(ui, ports(world), index_of(area).expect("area")).expect("app")
}

pub fn center(app: &mut App, id: NodeId) -> Point {
    app.ui.layout();
    app.ui.rect(id).expect("rect").center()
}

pub fn click_at(app: &mut App, pos: Point) {
    app.handle(Event::PointerMove { pos }).expect("move");
    app.handle(Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    })
    .expect("down");
    app.handle(Event::PointerUp {
        pos,
        button: PointerButton::Primary,
    })
    .expect("up");
}

pub fn click(app: &mut App, id: NodeId) {
    let pos = center(app, id);
    click_at(app, pos);
}

pub fn key(app: &mut App, key: Key) {
    app.handle(Event::Key {
        key,
        modifiers: Modifiers::NONE,
    })
    .expect("key");
}

pub fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        key(app, Key::Char(c));
    }
}

pub fn log(world: &Shared) -> Vec<String> {
    world.borrow().log.clone()
}

pub fn write_png(path: &Path, ui: &mut Ui) {
    let canvas = ultar_ui::snapshot::render(ui).expect("render");
    let image = ultar_ui::snapshot::Image::from_canvas(&canvas);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    std::fs::write(path, image.encode_png().expect("png")).expect("write");
}
