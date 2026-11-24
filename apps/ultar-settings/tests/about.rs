//! Settings › About (docs/BRAND.md §4): the brand facts from `ultar-brand`, version and build
//! from the installed release's os-release, never a constant; "not known" without one.

mod common;

use common::*;
use ultar_i18n::Lang;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Size;

fn texts(app: &mut ultar_settings::App) -> Vec<String> {
    app.ui
        .a11y_tree()
        .iter()
        .filter_map(|n| n.name.as_ref())
        .map(|l| l.resolve(app.ui.translator()).into_owned())
        .collect()
}

#[test]
fn about_shows_the_brand_and_the_installed_release() {
    let dir = std::env::temp_dir().join(format!("ultar-about-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let release = dir.join("os-release");
    std::fs::write(
        &release,
        format!(
            "{}PRETTY_NAME=\"Ultar OS 0.9.1\"\nVERSION_ID=0.9.1\nBUILD_ID=20261124.7\n",
            ultar_brand::release::OS_RELEASE
        ),
    )
    .unwrap();
    for lang in [Lang::Ar, Lang::En] {
        let world = board_world(1, false);
        world.borrow_mut().os_release = Some(release.clone());
        let mut first = app(
            &world,
            lang,
            ThemeKind::Dark,
            Size::new(1440.0, 860.0),
            "about",
        );
        let all = texts(&mut first).join("\n");
        for fact in [
            ultar_brand::PRODUCT,
            ultar_brand::VENDOR,
            ultar_brand::WEBSITE,
            ultar_brand::CONTACT_EMAIL,
            "0.9.1",
            "20261124.7",
        ] {
            assert!(all.contains(fact), "{lang:?}: {fact} in\n{all}");
        }
        let developer = if lang == Lang::Ar {
            ultar_brand::DEVELOPER_AR
        } else {
            ultar_brand::DEVELOPER
        };
        assert!(all.contains(developer), "{lang:?}: developer");
        assert!(
            first
                .ui
                .a11y_tree()
                .iter()
                .any(|n| n.role == ultar_ui::ui::Role::Image
                    || n.name
                        .as_ref()
                        .is_some_and(|l| l.key_name() == Some("brand.label.mark"))),
            "the mark is named"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn about_without_a_release_file_says_so() {
    for lang in [Lang::Ar, Lang::En] {
        let world = board_world(1, false);
        let mut app2 = app(
            &world,
            lang,
            ThemeKind::Dark,
            Size::new(1440.0, 860.0),
            "about",
        );
        let none = texts(&mut app2).join("\n");
        assert!(!none.contains("0.9.1"));
        assert!(
            none.contains(&*app2.ui.translator().t("app.settings.about.unknown")),
            "{none}"
        );
    }
}
