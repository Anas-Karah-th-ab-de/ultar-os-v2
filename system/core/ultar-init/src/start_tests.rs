use super::*;

fn env(root: bool) -> Environment {
    Environment {
        launcher: PathBuf::from("/nonexistent/launcher"),
        groups: GroupTree::none(),
        kernel_rights: false,
        root,
        stop_timeout: Duration::from_secs(1),
        edition: "server".into(),
        net_allowed: None,
        all_rights_allowed: Vec::new(),
    }
}

fn spec(text: &str) -> ServiceSpec {
    ServiceSpec::parse(text, "svc").unwrap()
}

#[test]
fn as_root_every_service_gets_its_ids_set_and_groups_cleared() {
    let root = env(true);
    let plain = identity(&spec("exec = \"/bin/true\""), &root).unwrap();
    assert_eq!(plain, Some(ultar_sys::Identity::default()));
    let confined = identity(
        &spec("exec = \"/bin/true\"\nuser = 802\ngroups = [801]"),
        &root,
    );
    let expected = ultar_sys::Identity {
        uid: 802,
        gid: 802,
        groups: vec![801],
    };
    assert_eq!(confined, Ok(Some(expected)));
}

#[test]
fn without_root_only_init_s_own_ids_are_possible() {
    let mine = env(false);
    assert_eq!(identity(&spec("exec = \"/bin/true\""), &mine), Ok(None));
    let own = format!("exec = \"/bin/true\"\nuser = {}", ultar_sys::euid());
    let own_gid_matches = ultar_sys::gid() == ultar_sys::euid();
    assert_eq!(identity(&spec(&own), &mine).is_ok(), own_gid_matches);
    let other = format!("exec = \"/bin/true\"\nuser = {}", ultar_sys::euid() + 1);
    assert_eq!(
        identity(&spec(&other), &mine).map_err(|e| e.word),
        Err("identity_needs_root")
    );
}

#[test]
fn a_missing_launcher_is_a_start_error_and_nothing_is_left_open() {
    let error = start(
        &spec("exec = \"/bin/true\"\nnotify = true"),
        &env(false),
        Instant::now(),
    );
    assert_eq!(error.map(|run| run.pid).map_err(|e| e.word), Err("spawn"));
}

#[test]
fn directories_are_created_private() {
    let base = std::env::temp_dir().join(format!("ultar-init-dirs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let text = format!(
        "exec = \"/bin/true\"\ndirs = [\"{}/state\"]",
        base.display()
    );
    let service = spec(&text);
    prepare_dirs(&service, &env(false), ultar_sys::euid(), ultar_sys::gid()).unwrap();
    let mode = std::fs::metadata(base.join("state"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_tree_left_by_a_root_service_is_handed_over() {
    if ultar_sys::euid() != 0 {
        return;
    }
    use std::os::unix::fs::MetadataExt;
    let base = std::env::temp_dir().join(format!("ultar-init-adopt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("sub")).unwrap();
    std::fs::write(base.join("sub/file"), "x").unwrap();
    std::os::unix::fs::symlink("/etc/passwd", base.join("link")).unwrap();
    adopt_tree(&base, 65534, 65534).unwrap();
    assert_eq!(
        std::fs::metadata(base.join("sub/file")).unwrap().uid(),
        65534
    );
    assert_eq!(
        std::fs::symlink_metadata(base.join("link")).unwrap().uid(),
        65534
    );
    // The link's target is untouched.
    assert_eq!(std::fs::metadata("/etc/passwd").unwrap().uid(), 0);
    std::fs::remove_dir_all(base).unwrap();
}

#[test]
fn a_file_asking_for_all_rights_is_refused_without_the_override() {
    let spec = spec("exec = \"/bin/true\"\nrights = [\"all\"]");
    assert!(spec.confinement.all_rights);
    let refused = start(&spec, &env(false), Instant::now()).map(|run| run.pid);
    assert_eq!(refused.map_err(|e| e.word), Err("rights_all_refused"));
    // With the override the start goes on (and fails later: no launcher).
    let mut allowed = env(false);
    allowed.all_rights_allowed = vec!["svc".into()];
    let next = start(&spec, &allowed, Instant::now()).map(|run| run.pid);
    assert_eq!(next.map_err(|e| e.word), Err("spawn"));
}
