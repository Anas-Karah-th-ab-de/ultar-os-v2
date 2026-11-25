//! The grouped sidebar (headings, search across every area, keyboard, right-to-left) and every
//! area page: none may show "not available in this version", every settingsd page shows its rows,
//! and text rows store what is typed.

mod common;

use common::*;
use ultar_bus::messages::account_local::{PictureInfo, UserKind};
use ultar_bus::messages::settings::{
    ChoiceInfo, ItemInfo, ItemKind, PanelInfo, RiskInfo, ScopeInfo, SettingValue,
};
use ultar_bus::Text;
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::areas::{index_of, PageKind, AREAS, GROUPS};
use ultar_settings::service::fake::Shared;
use ultar_settings::service::system::{AppEntry, Drive, LocalUser};
use ultar_settings::state::DialogPurpose;
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

/// settingsd as on a phone-capable laptop: the panels of the boards, the mobile network, a
/// text row on Region, the two wired rows and one waiting row on Mouse, and Sharing in full.
fn all_panels() -> Vec<PanelInfo> {
    let board = board_world(3, true).borrow().panels.clone();
    let mut panels = board.unwrap_or_default();
    panels.retain(|p| !["sharing", "region", "pointer"].contains(&p.id.as_str()));
    panels.push(panel("mobile", vec![flag("mobile.data")]));
    let currency = ItemKind::Text {
        format: "currency".into(),
    };
    let region = row(
        "region.format.currency",
        currency,
        SettingValue::Text(String::new()),
    );
    panels.push(panel("region", vec![region]));
    let pointer = vec![
        flag("pointer.natural_scroll"),
        flag("pointer.touchpad.gestures"),
    ];
    panels.push(panel("pointer", pointer));
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
    {
        let mut w = world.borrow_mut();
        w.panels = Some(all_panels());
        w.system.users = Some(vec![user("anas", true), user("leila", false)]);
        w.system.apps = Some(vec![AppEntry {
            id: "org.ultar.files".into(),
            version: "0.8.0".into(),
            permissions: vec![("network".into(), true), ("background".into(), false)],
        }]);
        let gib = 1u64 << 30;
        w.system.drives = Some(vec![
            Drive {
                name: String::new(),
                total_bytes: 64 * gib,
                free_bytes: 16 * gib,
            },
            Drive {
                name: "KM-USB".into(),
                total_bytes: 8 * gib,
                free_bytes: 6 * gib,
            },
        ]);
    }
    world
}

fn user(name: &str, current: bool) -> LocalUser {
    LocalUser {
        name: name.into(),
        uid: 1000,
        display_name: name.into(),
        kind: if current {
            UserKind::Administrator
        } else {
            UserKind::Standard
        },
        picture: PictureInfo::Initials,
        auto_login: false,
        current,
    }
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

/// Every name, value and description of the screen-reader tree, translated.
fn texts(app: &mut App) -> Vec<String> {
    let tree = app.ui.a11y_tree();
    let tr = app.ui.translator().clone();
    tree.iter()
        .flat_map(|n| [n.name.clone(), n.value.clone(), n.description.clone()])
        .flatten()
        // Names of users, apps and drives are shown isolated from the text direction.
        .map(|l| l.resolve(&tr).replace(['\u{2068}', '\u{2069}'], ""))
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
fn a_text_row_stores_what_is_typed_on_enter() {
    let world = world();
    let mut app = wide(&world, Lang::En, "region");
    let currency = app.view.node(Act::PanelItem(0)).expect("currency field");
    click(&mut app, currency);
    type_text(&mut app, "SYP");
    assert!(log(&world).is_empty(), "nothing stored before Enter");
    key(&mut app, Key::Enter);
    assert_eq!(
        log(&world),
        ["settings.set region.format.currency=Text(\"SYP\")"]
    );
}

#[test]
fn settings_nothing_follows_are_disabled_with_a_note() {
    let world = world();
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        let mut app = wide(&world, lang, "sharing");
        let note = app
            .ui
            .translator()
            .t("app.settings.panel.not_yet")
            .into_owned();
        let shown = texts(&mut app);
        let notes = shown.iter().filter(|t| **t == note).count();
        assert!(notes >= 7, "{lang:?}: a note on every row, found {notes}");
        for index in 0..7 {
            let control = app.view.node(Act::PanelItem(index)).expect("control");
            assert!(
                app.ui.is_disabled(control),
                "{lang:?}: row {index} is enabled"
            );
        }
        // A click on the screen sharing switch stores nothing.
        let screen = app.view.node(Act::PanelItem(5)).expect("screen switch");
        click(&mut app, screen);
        assert!(log(&world).is_empty(), "{:?}", log(&world));
    }
    // Mouse: scrolling direction reaches the compositor, touchpad gestures nothing yet.
    let mut app = wide(&world, Lang::En, "mouse");
    let natural = app.view.node(Act::PanelItem(0)).expect("natural scrolling");
    let gestures = app.view.node(Act::PanelItem(1)).expect("gestures");
    assert!(!app.ui.is_disabled(natural));
    assert!(app.ui.is_disabled(gestures));
    click(&mut app, natural);
    assert_eq!(
        log(&world),
        ["settings.set pointer.natural_scroll=Flag(true)"]
    );
}

#[test]
fn areas_without_their_hardware_are_hidden() {
    let world = world();
    {
        let mut w = world.borrow_mut();
        let panels = w.panels.as_mut().expect("panels");
        panels.retain(|p| p.id != "mobile" && p.id != "pointer");
    }
    let app = wide(&world, Lang::En, "displays");
    let ids: Vec<&str> = app.view.nav_areas.iter().map(|&i| AREAS[i].id).collect();
    assert!(
        !ids.contains(&"cellular") && !ids.contains(&"mouse"),
        "{ids:?}"
    );
    assert!(ids.contains(&"keyboard") && ids.contains(&"sharing"));
    assert_eq!(ids.len(), AREAS.len() - 2);
    // While settingsd cannot be read nothing is hidden: the pages say why.
    world.borrow_mut().panels = None;
    let app = wide(&world, Lang::En, "displays");
    assert_eq!(app.view.nav_areas.len(), AREAS.len());
}

#[test]
fn users_are_added_and_removed_through_the_account_service() {
    let world = world();
    let mut app = wide(&world, Lang::En, "users");
    let shown = texts(&mut app);
    assert!(shown.iter().any(|t| t == "anas") && shown.iter().any(|t| t == "leila"));
    // The signed-in user cannot be removed.
    let own = app.view.node(Act::UserRemove(0)).expect("remove anas");
    assert!(app.ui.is_disabled(own));
    let add = app.view.node(Act::UserAdd).expect("add user");
    click(&mut app, add);
    assert_eq!(
        app.state.dialog.as_ref().map(|d| d.purpose),
        Some(DialogPurpose::UserAdd)
    );
    for (field, text) in [(0, "sami"), (1, "sesame-1"), (2, "sesame-2")] {
        let node = app.view.node(Act::DialogField(field)).expect("field");
        click(&mut app, node);
        type_text(&mut app, text);
    }
    let ok = app.view.node(Act::DialogOk).expect("ok");
    click(&mut app, ok);
    assert!(log(&world).is_empty(), "different passwords: nothing sent");
    assert!(app.state.dialog.as_ref().is_some_and(|d| d.error.is_some()));
    for (field, text) in [(0, "sami"), (1, "sesame-1"), (2, "sesame-1")] {
        let node = app.view.node(Act::DialogField(field)).expect("field");
        click(&mut app, node);
        type_text(&mut app, text);
    }
    let ok = app.view.node(Act::DialogOk).expect("ok");
    click(&mut app, ok);
    assert_eq!(log(&world), ["account.user_create sami"]);
    assert!(app.state.dialog.is_none());
    assert_eq!(app.state.more.users.len(), 3);
    let leila = app.view.node(Act::UserRemove(1)).expect("remove leila");
    click(&mut app, leila);
    let ok = app.view.node(Act::DialogOk).expect("ok");
    click(&mut app, ok);
    assert_eq!(
        log(&world).last().expect("call"),
        "account.user_remove leila"
    );
    assert_eq!(app.state.more.users.len(), 2);
}

#[test]
fn apps_storage_and_updates_show_their_services() {
    let world = world();
    let mut app = wide(&world, Lang::En, "apps");
    let granted = app.view.node(Act::AppRevoke(0, 0)).expect("network switch");
    let asked = app
        .view
        .node(Act::AppRevoke(0, 1))
        .expect("background switch");
    assert!(app.ui.is_disabled(asked), "granting is the system dialog's");
    click(&mut app, granted);
    assert_eq!(log(&world), ["launcher.revoke org.ultar.files network"]);
    assert!(!app.state.more.apps[0].permissions[0].1);

    let mut app = wide(&world, Lang::En, "storage");
    let shown = texts(&mut app);
    assert!(shown.iter().any(|t| t == "KM-USB"), "{shown:?}");
    assert!(shown.iter().any(|t| t == "This device"), "{shown:?}");
    assert!(
        shown.iter().any(|t| t == "16.0 GB free of 64.0 GB"),
        "{shown:?}"
    );
    assert!(shown.iter().any(|t| t == "6.0 GB free of 8.0 GB"));

    let mut app = wide(&world, Lang::En, "updates");
    let check = app.view.node(Act::FirmwareCheck).expect("check now");
    click(&mut app, check);
    assert_eq!(log(&world).last().expect("call"), "update.check");
    let usb = app
        .view
        .node(Act::FirmwareUsb)
        .expect("update from a drive");
    click(&mut app, usb);
    assert_eq!(
        app.state.dialog.as_ref().map(|d| d.purpose),
        Some(DialogPurpose::FirmwareUsb)
    );
}

#[test]
fn the_window_follows_the_accessibility_switches_it_shows() {
    let world = world();
    let mut app = wide(&world, Lang::En, "accessibility");
    assert!(!app.ui.env().high_contrast);
    let contrast = app.view.node(Act::PanelItem(0)).expect("high contrast");
    assert!(!app.ui.is_disabled(contrast), "the shell follows it");
    click(&mut app, contrast);
    assert_eq!(
        log(&world),
        ["settings.set accessibility.high_contrast=Flag(true)"]
    );
    assert!(app.ui.env().high_contrast, "the palette changed at once");
    // A window opened later starts with it.
    let mut later = wide(&world, Lang::Ar, "displays");
    assert!(later.ui.env().high_contrast);
    // Switched off elsewhere (quick settings): the open window follows.
    {
        let mut w = world.borrow_mut();
        let panels = w.panels.as_mut().expect("panels");
        let access = panels.iter_mut().find(|p| p.id == "accessibility");
        access.expect("panel").items[0].value = SettingValue::Flag(false);
        w.changed = true;
    }
    later.poll().expect("poll");
    assert!(!later.ui.env().high_contrast);
}

#[test]
fn the_default_app_rows_offer_the_launchers_candidates() {
    use ultar_bus::messages::launcher::Kind;
    let world = world();
    {
        let mut w = world.borrow_mut();
        let panels = w.panels.as_mut().expect("panels");
        let apps = panels.iter_mut().find(|p| p.id == "apps").expect("apps");
        let media = ItemKind::Choice {
            choices: ["ask", "open_files", "nothing"]
                .iter()
                .map(|c| ChoiceInfo {
                    id: (*c).into(),
                    label: Text::key(&format!("settings.apps.removable_media.choice.{c}")),
                })
                .collect(),
        };
        apps.items = vec![
            row(
                "apps.default.web",
                ItemKind::Text {
                    format: "name".into(),
                },
                SettingValue::Text("browser".into()),
            ),
            row(
                "apps.removable_media",
                media,
                SettingValue::Text("ask".into()),
            ),
        ];
        w.system.candidates = vec![(Kind::Web, vec!["browser".into(), "store.web".into()])];
    }
    let mut app = wide(&world, Lang::En, "apps");
    // both rows take effect now: neither is disabled with the "not yet" note
    let web = app.view.node(Act::PanelItem(0)).expect("web row");
    let media = app.view.node(Act::PanelItem(1)).expect("media row");
    assert!(!app.ui.is_disabled(web) && !app.ui.is_disabled(media));
    // the web row is a choice of the launcher's candidates; pick the second (store.web)
    click(&mut app, web);
    assert!(app.ui.is_open(web), "the default app offers a list");
    key(&mut app, Key::ArrowDown);
    key(&mut app, Key::Enter);
    assert_eq!(
        log(&world).last().expect("a set"),
        "settings.set apps.default.web=Text(\"store.web\")"
    );
    assert_eq!(
        app.state.panel("apps").unwrap().items[0].value,
        SettingValue::Text("store.web".into())
    );
}
