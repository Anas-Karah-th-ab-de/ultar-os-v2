//! The Apps page's wired rows: the default-app row offers the launcher's candidates and
//! stores the chosen one; the removable-media row takes effect too (neither is disabled).

mod common;

use common::*;
use ultar_bus::messages::launcher::Kind;
use ultar_bus::messages::settings::{
    ChoiceInfo, ItemInfo, ItemKind, RiskInfo, ScopeInfo, SettingValue,
};
use ultar_bus::Text;
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::service::fake::Shared;
use ultar_settings::service::system::AppEntry;
use ultar_settings::App;
use ultar_ui::event::Key;
use ultar_ui::tokens::ThemeKind;

fn wide(world: &Shared, lang: Lang, area: &str) -> App {
    app(
        world,
        lang,
        ThemeKind::Dark,
        content_size(1440.0, 900.0),
        area,
    )
}

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

#[test]
fn the_default_app_rows_offer_the_launchers_candidates() {
    let world = board_world(3, true);
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
        w.system.apps = Some(vec![AppEntry {
            id: "org.ultar.files".into(),
            version: "0.8.0".into(),
            permissions: vec![("network".into(), true), ("background".into(), false)],
        }]);
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
