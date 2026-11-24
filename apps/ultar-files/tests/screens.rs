//! Headless renders of the surfaces in dark and light, Arabic and English: they paint, the
//! layout mirrors, nothing is cut off at the board size, compact screens drop the sidebar.
//! With `ULTAR_FILES_SHOTS=<dir>` the pictures are written there for review.
mod common;

use common::*;
use ultar_files::action::Target;
use ultar_files::model::{Dialog, ViewMode};
use ultar_files::{App, Mode};
use ultar_i18n::Lang;
use ultar_ui::snapshot::{render, Image};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Size;

fn board_root() -> Root {
    let r = Root::new();
    for d in ["مشاريع", "عقود", "فواتير"] {
        std::fs::create_dir_all(r.path(&format!("documents/{d}"))).unwrap();
    }
    r.write("documents/تقرير.docx", b"x");
    r.write("documents/ميزانية.xlsx", b"x");
    r.write("documents/report-mixed تقرير 2026.txt", b"x");
    r
}

fn shot(app: &mut App, name: &str) {
    app.ui.layout();
    let canvas = render(&mut app.ui).unwrap();
    let img = Image::from_canvas(&canvas);
    let first = &img.rgba[..4];
    assert!(
        img.rgba.chunks(4).any(|p| p != first),
        "{name}: something is painted"
    );
    if let Ok(dir) = std::env::var("ULTAR_FILES_SHOTS") {
        let png = img.encode_png().unwrap();
        ultar_ui::snapshot::write_atomic(
            &std::path::Path::new(&dir).join(format!("{name}.png")),
            &png,
        )
        .unwrap();
    }
}

fn set_theme(app: &mut App, theme: ThemeKind) {
    app.ui.set_theme(theme);
    app.rebuild().unwrap();
}

#[test]
fn window_in_four_variants_and_states() {
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            let r = board_root();
            let (mut app, _) = app(&r, Mode::Window, lang, Some("files://documents/"));
            set_theme(&mut app, theme);
            let tag = format!("{lang:?}-{}", theme.name()).to_lowercase();
            shot(&mut app, &format!("files-grid-{tag}"));
            let p0 = app.ui.rect(node(&app, &Target::Place(0))).unwrap();
            let rtl = lang == Lang::Ar;
            assert_eq!(
                p0.x > app.ui.size().w / 2.0,
                rtl,
                "the sidebar is at the start edge"
            );
            app.state.look.view = ViewMode::List;
            app.rebuild().unwrap();
            shot(&mut app, &format!("files-list-{tag}"));
            app.state.look.view = ViewMode::Grid;
            app.state.dialog = Some(Dialog::Properties(
                app.state.folder.child("تقرير.docx").unwrap(),
            ));
            app.rebuild().unwrap();
            shot(&mut app, &format!("files-properties-{tag}"));
            app.state.dialog = None;
            let n = node(&app, &Target::TrashPlace);
            click(&mut app, n);
            shot(&mut app, &format!("files-trash-empty-{tag}"));
            app.state.error = Some(ultar_files::fs::FsError::Permission);
            app.rebuild().unwrap();
            shot(&mut app, &format!("files-permission-{tag}"));
        }
    }
}

#[test]
fn desktop_icons_and_compact_layout() {
    for lang in [Lang::Ar, Lang::En] {
        let r = board_root();
        let (mut app, _) = app(&r, Mode::Desk, lang, None);
        shot(&mut app, &format!("desk-{lang:?}").to_lowercase());
        let (mut app, _) = app_sized(&r, lang, Size::new(360.0, 720.0));
        assert!(
            app.built().node(&Target::Place(0)).is_none(),
            "no sidebar on a phone"
        );
        let menu = node(&app, &Target::PlacesMenu);
        click(&mut app, menu);
        assert_eq!(app.ui.menu_owner(), Some(node(&app, &Target::PlacesMenu)));
        shot(&mut app, &format!("files-compact-{lang:?}").to_lowercase());
    }
}

fn app_sized(
    r: &Root,
    lang: Lang,
    size: Size,
) -> (
    App,
    std::rc::Rc<std::cell::RefCell<ultar_files::service::Fake>>,
) {
    let (mut a, f) = app(r, Mode::Window, lang, Some("files://documents/"));
    a.ui.set_size(size).unwrap();
    a.rebuild().unwrap();
    a.ui.layout();
    (a, f)
}

#[test]
fn every_operable_node_has_a_name() {
    let r = board_root();
    let (mut app, _) = app(&r, Mode::Window, Lang::Ar, Some("files://documents/"));
    for n in app.ui.a11y_tree() {
        if n.focusable {
            assert!(n.name.is_some(), "{:?} has no accessible name", n.role);
        }
    }
}
