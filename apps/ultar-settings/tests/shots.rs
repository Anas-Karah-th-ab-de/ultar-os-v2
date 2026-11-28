//! Writes the app's renders of the boards (with `ULTAR_CONFORMANCE_OUT=<dir>`).
mod common;
use common::*;
use ultar_i18n::Lang;
use ultar_ui::tokens::ThemeKind;

#[test]
fn renders_the_boards() {
    let Some(out) = std::env::var_os("ULTAR_CONFORMANCE_OUT") else {
        return;
    };
    let out = std::path::PathBuf::from(out);
    let cases = [
        (
            "SettingsDisplay",
            "displays",
            Lang::Ar,
            ThemeKind::Dark,
            true,
        ),
        (
            "SettingsDisplayLight",
            "displays",
            Lang::En,
            ThemeKind::Light,
            true,
        ),
        (
            "SettingsDrivers",
            "drivers",
            Lang::Ar,
            ThemeKind::Dark,
            true,
        ),
        ("Account", "accounts", Lang::Ar, ThemeKind::Dark, true),
        ("About", "about", Lang::Ar, ThemeKind::Dark, true),
        ("AboutLight", "about", Lang::En, ThemeKind::Light, true),
        ("Appearance", "appearance", Lang::De, ThemeKind::Dark, true),
        ("Desktop", "desktop", Lang::De, ThemeKind::Dark, true),
        (
            "Account-signedout",
            "accounts",
            Lang::Ar,
            ThemeKind::Dark,
            false,
        ),
    ];
    for (board, area, lang, theme, signed) in cases {
        let world = board_world(3, signed);
        let mut app = app(&world, lang, theme, content_size(1440.0, 900.0), area);
        write_png(&out.join(format!("{board}-app.png")), &mut app.ui);
    }
    let compact = ultar_ui::Size::new(360.0, 712.0);
    let world = board_world(1, true);
    let mut list = app(&world, Lang::Ar, ThemeKind::Dark, compact, "displays");
    list.state.compact_list = true;
    ultar_settings::view::build(&mut list.ui, &list.state, &mut list.view).unwrap();
    write_png(&out.join("CompactSettings-app.png"), &mut list.ui);
    let mut page = app(&world, Lang::Ar, ThemeKind::Dark, compact, "displays");
    write_png(&out.join("CompactDisplay-app.png"), &mut page.ui);
}
