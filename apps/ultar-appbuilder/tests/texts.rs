//! Every key the app uses exists in Arabic, English and German, and every screen state builds.

use ultar_appbuilder::state::{Event, Phase, State};
use ultar_appbuilder::{i18n, view, AppError, Issue};
use ultar_i18n::Lang;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

fn all_keys() -> Vec<&'static str> {
    let mut keys = vec![
        "app.appbuilder.name",
        "app.appbuilder.desc",
        "appbuilder.project",
        "appbuilder.check",
        "appbuilder.build",
        "appbuilder.install",
        "appbuilder.state.idle",
        "appbuilder.state.busy",
        "appbuilder.state.checked",
        "appbuilder.state.built",
        "appbuilder.state.installed",
        "appbuilder.key.title",
        "appbuilder.key.id",
        "appbuilder.key.public",
        "appbuilder.key.trust",
    ];
    for e in [
        AppError::Io,
        AppError::Key,
        AppError::Pack,
        AppError::Verify,
        AppError::NotChecked,
        AppError::NoLauncher,
        AppError::Refused,
    ] {
        keys.push(e.message_key());
    }
    for i in [
        Issue::NoManifest,
        Issue::ManifestDamaged,
        Issue::BadId,
        Issue::BadVersion,
        Issue::NoPublisher,
        Issue::BadKind,
        Issue::NameNotKey,
        Issue::BadArch,
        Issue::NoIcon,
        Issue::NoPayload,
        Issue::ExecMissing,
        Issue::BadNetwork,
        Issue::MissingSection("form"),
    ] {
        keys.push(i.message_key());
    }
    keys
}

#[test]
fn every_key_in_every_language() {
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        let tr = i18n::translator(lang);
        for key in all_keys() {
            assert!(tr.catalog(lang).contains(key), "{key} missing in {lang:?}");
        }
    }
}

#[test]
fn every_phase_builds_in_arabic_and_english() {
    let tmp = tempfile::tempdir().unwrap();
    let key = ultar_appbuilder::devkey::load_or_create(tmp.path()).unwrap();
    let phases = [
        Phase::Idle,
        Phase::Busy,
        Phase::Checked {
            id: "acme.notes".into(),
            version: "1.0.0".into(),
        },
        Phase::Installed("acme.notes".into()),
        Phase::Failed(AppError::Refused),
    ];
    for lang in [Lang::Ar, Lang::En] {
        for phase in phases.clone() {
            let mut ui = Ui::new(
                PaintEnv::new(lang, ThemeKind::Dark),
                InputMode::Pointer,
                Size::new(960.0, 640.0),
                i18n::translator(lang),
            )
            .unwrap();
            let mut s = State::default();
            s.event(Event::Built(Ok((
                tmp.path().join("acme.notes-1.0.0.uap"),
                key.clone(),
            ))));
            s.issues = vec![Issue::MissingSection("limits"), Issue::BadId];
            s.phase = phase;
            let built = view::build(&mut ui, &s).unwrap();
            assert_eq!(built.targets.len(), 4);
        }
    }
}
