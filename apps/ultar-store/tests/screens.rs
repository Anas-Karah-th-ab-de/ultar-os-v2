//! Screen tests (APP-RULES §11): every page and every state, rendered with
//! the headless renderer, in Arabic and English, dark and light, desktop and
//! phone. Checks: it renders, the key texts of the boards are present, no
//! text leaks as a bare catalog key, and the board's Arabic strings are the
//! ones from the catalog (ar first).

mod common;

use common::{
    error_store, loading_store, no_space_store, offline_store, ready_store, Scene, LANGS, THEMES,
};
use ultar_store::state::Area;
use ultar_ui::Lang;

fn text_present(scene: &mut Scene, want: &str) -> bool {
    scene.a11y_texts().iter().any(|t| t.contains(want))
}

/// Every page of the app, both layouts.
fn every_page(wide: bool, f: impl Fn(&mut Scene, Lang)) {
    for lang in LANGS {
        for theme in THEMES {
            for area in [
                Area::Discover,
                Area::Categories,
                Area::Installed,
                Area::Updates,
                Area::InstallFile,
                Area::OfflineSources,
                Area::Account,
            ] {
                let mut s = Scene::area(area, lang, theme, wide);
                f(&mut s, lang);
            }
            // The app page and the search pages.
            let mut s = Scene::ready(lang, theme, wide);
            s.store
                .act(ultar_store::action::StoreAction::Open("km-office".into()));
            s.rebuild();
            f(&mut s, lang);
            let mut s = Scene::ready(lang, theme, wide);
            s.store
                .act(ultar_store::action::StoreAction::Search("coder".into()));
            s.rebuild();
            f(&mut s, lang);
            let mut s = Scene::ready(lang, theme, wide);
            s.store.act(ultar_store::action::StoreAction::Search(
                "nothing-matches-this".into(),
            ));
            s.rebuild();
            f(&mut s, lang);
        }
    }
}

#[test]
fn desktop_pages_all_render_with_their_texts() {
    every_page(true, |s, lang| {
        let (w, h) = s.render();
        assert_eq!((w, h), (1280, 800));
        let texts = s.a11y_texts();
        assert!(!texts.is_empty(), "empty a11y tree {lang:?}");
        // No unresolved catalog keys on screen.
        for t in &texts {
            assert!(
                !t.starts_with("store.") && !t.starts_with("app.store."),
                "untranslated key shown: {t}"
            );
        }
    });
}

#[test]
fn phone_pages_all_render_with_their_texts() {
    every_page(false, |s, lang| {
        let (w, h) = s.render();
        assert_eq!((w, h), (360, 800));
        let texts = s.a11y_texts();
        assert!(!texts.is_empty(), "empty a11y tree {lang:?}");
        for t in &texts {
            assert!(!t.starts_with("store."), "untranslated key shown: {t}");
        }
    });
}

#[test]
fn discover_shows_the_board_components_verified() {
    for lang in LANGS {
        for theme in THEMES {
            let mut s = Scene::area(Area::Discover, lang, theme, true);
            s.render();
            let (want_name, want_section) = match lang {
                Lang::Ar => ("الأدوات المكتبية", "مكوّنات من Ultar"),
                _ => ("Office tools", "Components by Ultar"),
            };
            assert!(
                text_present(&mut s, want_section),
                "section {want_section:?} missing ({lang:?})"
            );
            assert!(
                text_present(&mut s, want_name),
                "card {want_name:?} missing ({lang:?})"
            );
        }
    }
}

#[test]
fn compact_store_shows_tabs_and_rows() {
    for lang in LANGS {
        for theme in THEMES {
            let mut s = Scene::ready(lang, theme, false);
            s.render();
            let (tab, row) = match lang {
                Lang::Ar => ("استكشاف", "الأدوات المكتبية"),
                _ => ("Discover", "Office tools"),
            };
            assert!(text_present(&mut s, tab), "tab missing ({lang:?})");
            assert!(text_present(&mut s, row), "row missing ({lang:?})");
        }
    }
}

#[test]
fn compact_app_page_shows_permissions_in_plain_words() {
    let mut s = Scene::new(
        ready_store(),
        Lang::Ar,
        ultar_ui::tokens::ThemeKind::Dark,
        common::COMPACT,
    );
    s.store
        .act(ultar_store::action::StoreAction::Open("km-office".into()));
    s.rebuild();
    s.render();
    for want in [
        "الصلاحيات بلغة واضحة",
        "يفتح فقط الملفات التي تختارها أنت",
        "لا يتّصل بالإنترنت",
        "يطبع على الطابعة التي تختارها",
        "تثبيت",
        "الحجم",
        "الإصدار",
        "المصدر",
    ] {
        assert!(text_present(&mut s, want), "missing {want:?}");
    }
}

/// Renders `store` and asserts each `(ar, en, what)` text of the board.
fn expect_board_texts(
    store: ultar_store::state::Store,
    lang: Lang,
    theme: ultar_ui::tokens::ThemeKind,
    texts: &[(&str, &str, &str)],
) {
    let mut s = Scene::new(store, lang, theme, common::WIDE);
    s.render();
    for (ar, en, what) in texts {
        let want = if lang == Lang::Ar { *ar } else { *en };
        assert!(text_present(&mut s, want), "{what} ({lang:?})");
    }
}

#[test]
fn surface_states_match_the_board_panels() {
    for lang in LANGS {
        for theme in THEMES {
            let loading = [("يجري تحميل المتجر", "Loading the store", "loading text")];
            expect_board_texts(loading_store(), lang, theme, &loading);
            let offline = [
                (
                    "غير متصل بالإنترنت",
                    "Not connected to the internet",
                    "offline banner",
                ),
                ("إعادة المحاولة", "Retry", "retry"),
            ];
            expect_board_texts(offline_store(), lang, theme, &offline);
            let error = [
                (
                    "تعذّر تحميل المتجر",
                    "Could not load the store",
                    "error title",
                ),
                ("حفظ السجلّ", "Save the log", "log button"),
            ];
            expect_board_texts(error_store(), lang, theme, &error);
            let no_space = [
                ("ما يمكن تحريره", "What can be freed", "free heading"),
                (
                    "تحرير المساحة والمتابعة",
                    "Free the space and continue",
                    "free go",
                ),
            ];
            expect_board_texts(no_space_store(), lang, theme, &no_space);
        }
    }
}

#[test]
fn no_results_shows_the_question_and_clear() {
    for lang in LANGS {
        let mut s = Scene::ready(lang, ultar_ui::tokens::ThemeKind::Dark, true);
        s.store.act(ultar_store::action::StoreAction::Search(
            "جدول المناوبات".into(),
        ));
        s.rebuild();
        s.render();
        let want = match lang {
            Lang::Ar => "لا نتائج لـ",
            _ => "No results for",
        };
        assert!(text_present(&mut s, want), "no-results text ({lang:?})");
        assert!(
            text_present(
                &mut s,
                if lang == Lang::Ar {
                    "مسح البحث"
                } else {
                    "Clear search"
                }
            ),
            "clear button ({lang:?})"
        );
    }
}
