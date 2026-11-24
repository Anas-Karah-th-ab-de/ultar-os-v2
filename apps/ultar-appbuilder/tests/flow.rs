//! Check → build → install, with real files, the real package format and a launcher double.

use std::path::{Path, PathBuf};
use ultar_appbuilder::state::{run, Command, Event, Phase, State, Target};
use ultar_appbuilder::{AppError, Issue};

const MANIFEST: &str = r#"id = "acme.notes"
version = "1.2.3"
publisher = "acme"
kind = "native"
arch = "x86_64"
name = "app.notes.name"
description = "app.notes.desc"
icon = "assets/icon.svg"
exec = "/usr/lib/ultar/apps/acme.notes/notes"

[form]
factors = ["desktop"]

[permissions]
network = "none"

[limits]
memory_mb = 64
"#;

fn project(dir: &Path, manifest: &str) -> PathBuf {
    let p = dir.join("notes");
    std::fs::create_dir_all(p.join("assets")).unwrap();
    std::fs::create_dir_all(p.join("payload/usr/lib/ultar/apps/acme.notes")).unwrap();
    std::fs::write(p.join("assets/icon.svg"), "<svg/>").unwrap();
    std::fs::write(
        p.join("payload/usr/lib/ultar/apps/acme.notes/notes"),
        b"\x7fELF",
    )
    .unwrap();
    std::fs::write(p.join("ultar-app.toml"), manifest).unwrap();
    p
}

fn no_launcher(_: &Path) -> Result<String, AppError> {
    Err(AppError::NoLauncher)
}

#[test]
fn check_build_and_install_a_project() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = project(tmp.path(), MANIFEST);
    let data = tmp.path().join("data");
    let mut s = State::default();
    s.set_project(dir.to_str().unwrap());
    assert_eq!(s.act(Target::Install), None); // nothing built yet
    assert_eq!(s.phase, Phase::Failed(AppError::NotChecked));

    let cmd = s.act(Target::Check).unwrap();
    assert!(s.busy());
    assert_eq!(s.act(Target::Build), None); // busy: nothing starts twice
    s.event(run(cmd, &data, &no_launcher));
    assert_eq!(
        s.phase,
        Phase::Checked {
            id: "acme.notes".into(),
            version: "1.2.3".into()
        }
    );

    let cmd = s.act(Target::Build).unwrap();
    s.event(run(cmd, &data, &no_launcher));
    let pkg = s.package.clone().unwrap();
    assert_eq!(pkg, dir.join("out/acme.notes-1.2.3.uap"));
    let key = s.key.clone().unwrap();
    // The package verifies with the developer key under the publisher's name, and only then.
    let trust = tmp.path().join("trust");
    std::fs::create_dir_all(&trust).unwrap();
    assert!(ultar_pkg::verify(&pkg, &trust).is_err());
    std::fs::write(trust.join("acme.ed25519"), &key.public_hex).unwrap();
    let m = ultar_pkg::verify(&pkg, &trust).unwrap();
    assert_eq!((m.id.as_str(), m.arch.as_str()), ("acme.notes", "x86_64"));
    // No leftovers next to the package.
    let names: Vec<String> = std::fs::read_dir(dir.join("out"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["acme.notes-1.2.3.uap".to_string()]);

    // Install: no launcher → honest error; with the launcher → installed.
    let cmd = s.act(Target::Install).unwrap();
    s.event(run(cmd.clone(), &data, &no_launcher));
    assert_eq!(s.phase, Phase::Failed(AppError::NoLauncher));
    let ok = |p: &Path| -> Result<String, AppError> {
        assert!(p.ends_with("acme.notes-1.2.3.uap"));
        Ok("acme.notes".into())
    };
    s.event(run(cmd, &data, &ok));
    assert_eq!(s.phase, Phase::Installed("acme.notes".into()));

    // The same developer key signs the next build.
    let again = run(Command::Build(dir), &data, &no_launcher);
    let Event::Built(Ok((_, k2))) = again else {
        panic!("second build failed")
    };
    assert_eq!(k2.public_hex, key.public_hex);
}

#[test]
fn every_problem_of_a_bad_project_is_listed_and_nothing_is_built() {
    let tmp = tempfile::tempdir().unwrap();
    let bad = MANIFEST
        .replace("id = \"acme.notes\"", "id = \"notes\"")
        .replace("version = \"1.2.3\"", "version = \"1.2\"")
        .replace("kind = \"native\"", "kind = \"compat-apk\"")
        .replace("arch = \"x86_64\"", "")
        .replace("name = \"app.notes.name\"", "name = \"Notes\"")
        .replace("network = \"none\"", "network = \"everywhere\"")
        .replace("[limits]\nmemory_mb = 64\n", "")
        .replace("acme.notes/notes\"", "acme.notes/missing\"");
    let dir = project(tmp.path(), &bad);
    let ev = run(
        Command::Build(dir.clone()),
        &tmp.path().join("d"),
        &no_launcher,
    );
    let Event::Built(Err(AppError::Issues(issues))) = ev else {
        panic!("expected issues")
    };
    for expected in [
        Issue::BadId,
        Issue::BadVersion,
        Issue::BadKind,
        Issue::NameNotKey,
        Issue::BadArch,
        Issue::ExecMissing,
        Issue::MissingSection("limits"),
        Issue::BadNetwork,
    ] {
        assert!(issues.contains(&expected), "{expected:?} in {issues:?}");
    }
    assert!(!dir.join("out").exists());
    // No manifest / damaged manifest / empty payload.
    let empty = tmp.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert_eq!(
        ultar_appbuilder::project::check(&empty),
        Err(vec![Issue::NoManifest])
    );
    std::fs::write(empty.join("ultar-app.toml"), "id = [").unwrap();
    assert_eq!(
        ultar_appbuilder::project::check(&empty),
        Err(vec![Issue::ManifestDamaged])
    );
    let dir = project(tmp.path(), MANIFEST);
    std::fs::remove_dir_all(dir.join("payload")).unwrap();
    std::fs::create_dir_all(dir.join("payload")).unwrap();
    assert_eq!(
        ultar_appbuilder::project::check(&dir),
        Err(vec![Issue::NoPayload])
    );
}

#[test]
fn empty_project_field_is_refused_without_work() {
    let mut s = State::default();
    assert_eq!(s.act(Target::Check), None);
    assert!(matches!(s.phase, Phase::Failed(_)));
    assert_eq!(s.act(Target::Project), None);
}
