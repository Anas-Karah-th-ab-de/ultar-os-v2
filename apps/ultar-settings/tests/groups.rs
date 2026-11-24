//! The grouped sidebar (headings, search across every area, keyboard, right-to-left) and every
//! area page: none may show "not available in this version", every settingsd page shows its rows,
//! and text rows store what is typed.

mod common;

use common::*;
use ultar_bus::messages::settings::{
    ChoiceInfo, ItemInfo, ItemKind, PanelInfo, RiskInfo, ScopeInfo, SettingValue,
};
use ultar_bus::Text;
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::areas::{index_of, PageKind, AREAS, GROUPS};
use ultar_settings::service::fake::Shared;
use ultar_settings::App;
use ultar_ui::event::Key;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::NodeId;
use ultar_ui::Size;

fn row(key: &str, kind: ItemKind, value: SettingValue) -> ItemInfo {
    let stem: Vec<&str> = key.split('.').filter(|s| *s != "<id>").collect();
    let stem = stem.join(".");
    ItemInfo {
        key: key.into(),
        label: Text::key(&format!("settings.{stem}.label")),
        description: Text::key(&format!("settings.{stem}.desc")),
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

fn flag(key: &str) -> ItemInfo {
    row(key, ItemKind::Toggle, SettingValue::Flag(false))
}

/// A panel for every settingsd page of the app, as settingsd lists it (labels from its catalog).
fn all_panels() -> Vec<PanelInfo> {
    let mut panels = board_world(3, true)
        .borrow()
        .panels
        .clone()
        .unwrap_or_default();
    let first: [(&str, &str); 22] = [
        ("power", "power.charge_limit.enabled"),
        ("windows", "windows.edge_snap"),
        ("notifications", "notifications.do_not_disturb"),
        ("region", "region.language"),
        ("date_time", "datetime.automatic"),
        ("privacy", "privacy.camera"),
        ("mobile", "mobile.data"),
        ("search", "search.history"),
        ("apps", "apps.autostart"),
        ("pointer", "pointer.acceleration"),
        ("keyboard", "keyboard.per_window"),
        ("printers", "printers.discover"),
        ("colour", "colour.management"),
        ("accessibility", "accessibility.zoom"),
        ("users", "users.list_on_login"),
        ("storage", "storage.trim"),
        ("updates", "updates.auto_system"),
        ("backup", "backup.enabled"),
        ("mesh", "mesh.enabled"),
        ("assistant", "assistant.enabled"),
        ("cells", "cells.enabled"),
        ("emergency", "emergency.sos_light"),
    ];
    for (id, key) in first {
        panels.push(panel(id, vec![flag(key)]));
    }
    panels.push(panel("sharing", sharing_rows()));
    panels
}

/// Sharing as settingsd lists it: a host name, switches, a choice and a list of folders.
fn sharing_rows() -> Vec<ItemInfo> {
    let networks = ItemKind::Choice {
        choices: ["private", "all"]
            .iter()
            .map(|c| ChoiceInfo {
                id: (*c).into(),
                label: Text::key(&format!("settings.sharing.networks.choice.{c}")),
            })
            .collect(),
    };
    vec![
        row(
            "sharing.device_name",
            ItemKind::Text {
                format: "host".into(),
            },
            SettingValue::Text("ultar".into()),
        ),
        flag("sharing.visible"),
        row(
            "sharing.networks",
            networks,
            SettingValue::Text("private".into()),
        ),
        flag("sharing.files"),
        row(
            "sharing.files.folders",
            ItemKind::List {
                format: "path".into(),
                max_len: 16,
            },
            SettingValue::List(Vec::new()),
        ),
        flag("sharing.screen"),
        flag("sharing.ssh"),
    ]
}

fn panel(id: &str, items: Vec<ItemInfo>) -> PanelInfo {
    PanelInfo {
        id: id.into(),
        title: Text::key(&format!("settings.panel.{id}")),
        items,
    }
}

fn world() -> Shared {
    let world = board_world(3, true);
    world.borrow_mut().panels = Some(all_panels());
    world
}

fn wide(world: &Shared, lang: Lang, area: &str) -> App {
    app(
        world,
        lang,
        ThemeKind::Dark,
        content_size(1440.0, 900.0),
        area,
    )
}

/// Every name and value of the screen-reader tree, translated.
fn texts(app: &mut App) -> Vec<String> {
    let tree = app.ui.a11y_tree();
    let tr = app.ui.translator().clone();
    tree.iter()
        .flat_map(|n| [n.name.clone(), n.value.clone()])
        .flatten()
        .map(|l| l.resolve(&tr).into_owned())
        .collect()
}

/// The headings of the navigation list (its children that are not areas), translated.
fn headings(app: &mut App) -> Vec<(String, NodeId)> {
    let nav = app.view.nav.expect("nav");
    let tr = app.ui.translator().clone();
    let children = app.ui.children(nav).to_vec();
    let mut out = Vec::new();
    for child in children {
        if app.view.shell.act(child).is_some() {
            continue;
        }
        let text = app
            .ui
            .children(child)
            .first()
            .copied()
            .expect("heading text");
        let name = app.ui.a11y_node(text).and_then(|n| n.name).expect("name");
        out.push((name.resolve(&tr).into_owned(), text));
    }
    out
}

#[test]
fn every_page_opens_and_none_is_unavailable() {
    let world = world();
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        for area in AREAS.iter() {
            // The mobile network is only listed on phones: open it on a phone-sized window.
            let size = if area.id == "cellular" {
                Size::new(360.0, 712.0)
            } else {
                content_size(1440.0, 900.0)
            };
            let mut app = app(&world, lang, ThemeKind::Dark, size, area.id);
            assert_ne!(app.state.page(), PageKind::NoService, "{}", area.id);
            let tr = app.ui.translator().clone();
            let banned = [
                tr.t("app.settings.no_service").into_owned(),
                tr.t("app.settings.no_service.sub").into_owned(),
                tr.t("app.settings.panel.unavailable").into_owned(),
            ];
            let shown = texts(&mut app);
            for text in &shown {
                assert!(
                    !banned.contains(text),
                    "{lang:?} {}: shows {text:?}",
                    area.id
                );
                assert!(
                    !text.starts_with("settings."),
                    "{}: untranslated {text}",
                    area.id
                );
            }
            if let PageKind::Panel(_) = area.page {
                assert!(
                    app.view.node(Act::PanelItem(0)).is_some(),
                    "{lang:?} {}: no rows",
                    area.id
                );
            }
        }
    }
}

#[test]
fn the_sidebar_is_grouped_under_headings() {
    let world = world();
    for (lang, first, last) in [
        (Lang::En, "Connections", "System"),
        (Lang::Ar, "الاتصالات", "النظام"),
        (Lang::De, "Verbindungen", "System"),
    ] {
        let mut app = wide(&world, lang, "displays");
        let heads = headings(&mut app);
        assert_eq!(heads.len(), GROUPS.len(), "{lang:?}");
        assert_eq!(heads[0].0, first);
        assert_eq!(heads[GROUPS.len() - 1].0, last);
        // Every heading comes right before the first area of its group.
        let nav = app.view.nav.expect("nav");
        let children = app.ui.children(nav).to_vec();
        for (g, group) in GROUPS.iter().enumerate() {
            let at = children
                .iter()
                .position(|c| app.ui.children(*c).first() == Some(&heads[g].1))
                .expect("heading in nav");
            let first_area = group
                .areas
                .iter()
                .find(|id| **id != "cellular")
                .expect("area");
            let next = app.view.shell.act(children[at + 1]);
            assert_eq!(next, Some(Act::Area(index_of(first_area).expect("id"))));
        }
        assert_eq!(children.len(), app.view.nav_areas.len() + GROUPS.len());
    }
}

#[test]
fn headings_follow_the_reading_direction() {
    let world = world();
    for (lang, sidebar_at_end) in [(Lang::En, false), (Lang::Ar, true)] {
        let mut app = wide(&world, lang, "displays");
        app.ui.layout();
        let width = app.ui.size().w;
        let heads = headings(&mut app);
        let nav = app.view.nav.expect("nav");
        let nav_rect = app.ui.rect(nav).expect("nav rect");
        for (name, node) in heads {
            let r = app.ui.rect(node).expect("rect");
            assert_eq!(
                r.center().x > width / 2.0,
                sidebar_at_end,
                "{lang:?} {name}"
            );
            // The text starts at the start edge of the list (start padding 12).
            let gap = if sidebar_at_end {
                nav_rect.x + nav_rect.w - (r.x + r.w)
            } else {
                r.x - nav_rect.x
            };
            assert!((gap - 12.0).abs() < 1.0, "{lang:?} {name}: {gap}");
        }
    }
}

#[test]
fn search_finds_areas_rows_and_groups_and_keeps_the_headings() {
    let world = world();
    let mut app = wide(&world, Lang::En, "displays");
    let search = app.view.search.expect("search");
    click(&mut app, search);
    // A row of a page: "Remote login (SSH)" is on Sharing.
    type_text(&mut app, "ssh");
    assert_eq!(app.view.nav_areas, vec![index_of("sharing").unwrap()]);
    let heads = headings(&mut app);
    assert_eq!(heads.len(), 1);
    assert_eq!(heads[0].0, "Connections");
    key(&mut app, Key::Enter);
    assert_eq!(app.state.area, index_of("sharing").unwrap());

    // A heading: every area of its group.
    let mut app = wide(&world, Lang::Ar, "displays");
    let search = app.view.search.expect("search");
    click(&mut app, search);
    type_text(&mut app, "الأمان والحماية");
    let ids: Vec<&str> = app.view.nav_areas.iter().map(|&i| AREAS[i].id).collect();
    assert_eq!(ids, ["privacy", "cells", "emergency"]);
    // Nothing: no heading either.
    for _ in 0.."الأمان والحماية".chars().count() {
        key(&mut app, Key::Backspace);
    }
    type_text(&mut app, "zzz");
    assert!(app.view.nav_areas.is_empty());
    assert!(headings(&mut app).is_empty());
}

#[test]
fn arrow_keys_skip_the_headings() {
    let world = world();
    let mut app = wide(&world, Lang::Ar, "displays");
    let mesh = app
        .view
        .node(Act::Area(index_of("mesh").unwrap()))
        .expect("mesh");
    click(&mut app, mesh);
    assert_eq!(app.state.area, index_of("mesh").unwrap());
    assert_eq!(app.ui.focused(), Some(mesh));
    // The next item is the first of the next group, not its heading.
    key(&mut app, Key::ArrowDown);
    let displays = app.view.node(Act::Area(index_of("displays").unwrap()));
    assert_eq!(app.ui.focused(), displays);
    key(&mut app, Key::Enter);
    assert_eq!(app.state.area, index_of("displays").unwrap());
}

#[test]
fn sharing_rows_store_text_lists_and_switches() {
    let world = world();
    let mut app = wide(&world, Lang::En, "sharing");
    // Row 4: shared folders, a list typed as "a, b".
    let folders = app.view.node(Act::PanelItem(4)).expect("folders field");
    click(&mut app, folders);
    type_text(&mut app, "/home/anas/Public, /srv/media");
    key(&mut app, Key::Enter);
    assert!(
        log(&world).iter().any(|l| l
            == "settings.set sharing.files.folders=List([\"/home/anas/Public\", \"/srv/media\"])"),
        "{:?}",
        log(&world)
    );
    // Row 5: screen sharing, a switch.
    let screen = app.view.node(Act::PanelItem(5)).expect("screen switch");
    click(&mut app, screen);
    assert!(log(&world)
        .last()
        .is_some_and(|l| l == "settings.set sharing.screen=Flag(true)"));
    let shown = texts(&mut app);
    for label in [
        "File sharing",
        "Shared folders",
        "Device name",
        "Screen sharing",
    ] {
        assert!(
            shown.iter().any(|t| t == label),
            "{label} missing: {shown:?}"
        );
    }
}
