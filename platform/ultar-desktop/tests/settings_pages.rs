//! Regression (quick settings, owner): the launcher hands the page of `settings://<page>` to
//! the Settings app as `--area <page>`, and Settings refuses to start with a word that is
//! none of its areas. Every page the shell opens must therefore be an area of the real
//! Settings app (its own list, `ultar_settings::areas`) – a wrong word would be a click
//! that opens nothing.

mod common;

use common::*;
use ultar_desktop::model::quick::TileId;
use ultar_desktop::model::search::{panel_area, Hit, SETTING_PANELS};
use ultar_desktop::shell::{Effect, Opening};
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_ui::tokens::ThemeKind;

/// The word the launcher makes of a page (`network/vpn` → `network`).
fn area(page: &str) -> &str {
    page.split('/').next().unwrap_or(page)
}

#[test]
fn every_page_the_shell_opens_is_an_area_of_settings() {
    let mut pages: Vec<&str> = vec!["wifi", "power", "keyboard", "apps/org.x", "network/vpn"];
    pages.extend(TileId::ALL.iter().map(|t| t.settings_area()));
    pages.extend(SETTING_PANELS.iter().map(|p| panel_area(p)));
    for page in pages {
        assert!(
            ultar_settings::areas::index_of(area(page)).is_some(),
            "settings://{page}: Settings has no area {:?}",
            area(page)
        );
    }
}

/// REAL-INPUT: typed into the search field, the result clicked with the pointer – the
/// panel "display" opens Settings on its area "displays".
#[test]
fn a_search_result_opens_settings_on_its_area() {
    let mut h = Harness::new(Lang::En, ThemeKind::Dark);
    h.click_bar(Target::Search);
    h.type_popup("display");
    let hit = Target::Result(Hit::Setting("display"));
    assert!(h.popup_target(&hit), "the panel is found");
    h.take();
    h.click_popup(hit);
    // The panel opens and the opened result is remembered (search.history is on).
    assert_eq!(
        h.take(),
        vec![
            Effect::Launch {
                app: "org.ultar.settings".into(),
                open: Opening::settings_page("displays"),
            },
            Effect::History,
        ]
    );
}
