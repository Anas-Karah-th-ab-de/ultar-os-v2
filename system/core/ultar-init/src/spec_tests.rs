use super::*;

#[test]
fn a_confined_service_is_read_with_every_key() {
    let text = r#"
exec = "/usr/bin/ultar-logd"
args = ["--dir", "/data/ultar/log"]
after = ["bus"]
user = 802
groups = [801, 803]
rights = ["manage_groups", "set_id"]
slice = "session"
dirs = ["/data/ultar/log"]
public_dirs = ["/run/ultar/log"]
notify = true
start_timeout_ms = 5000
stop_timeout_ms = 2000
[cgroup]
"memory.max" = "48M"
"#;
    let spec = ServiceSpec::parse(text, "logd").unwrap();
    assert_eq!(spec.name, "logd");
    let confinement = &spec.confinement;
    assert_eq!((confinement.uid, confinement.gid), (Some(802), Some(802)));
    assert_eq!(confinement.groups, vec![801, 803]);
    assert_eq!(confinement.rights, (1 << 7) | (1 << 8));
    assert_eq!(confinement.slice, Slice::Session);
    assert_eq!(confinement.limits["memory.max"], "48M");
    assert_eq!(
        spec.confinement.dirs,
        vec![PathBuf::from("/data/ultar/log")]
    );
    assert_eq!(
        spec.confinement.public_dirs,
        vec![PathBuf::from("/run/ultar/log")]
    );
    assert!(spec.life.notify);
    assert_eq!(
        (spec.life.start_timeout_ms, spec.life.stop_timeout_ms),
        (5000, Some(2000))
    );
}

#[test]
fn defaults_are_root_without_rights_in_the_system_slice() {
    let spec = ServiceSpec::parse("exec = \"/bin/true\"", "t").unwrap();
    assert_eq!(spec.confinement, Confinement::default());
    assert_eq!(spec.life.start_timeout_ms, DEFAULT_START_TIMEOUT_MS);
    assert!(!spec.life.notify && spec.life.restart == Restart::Always);
    assert_eq!(rights_mask(&["all".to_owned()]), Ok(ALL_RIGHTS));
}

#[test]
fn faulty_files_are_refused_as_a_whole() {
    let cases: [(&str, &str); 10] = [
        ("exec = \"/bin/true\"\nfrob = 1", "syntax"),
        ("exec = \"/bin/true\"\nrights = [\"root\"]", "unknown right"),
        ("exec = \"bin/true\"", "path"),
        ("exec = \"/bin/true\"\ndirs = [\"rel\"]", "path"),
        ("exec = \"/bin/true\"\nname = \"Bad Name\"", "name"),
        ("exec = \"/bin/true\"\nafter = [\"../x\"]", "name"),
        ("exec = \"/bin/true\"\nstart_timeout_ms = 0", "bounds"),
        ("exec = \"/bin/true\"\nstop_timeout_ms = 600001", "bounds"),
        ("exec = \"/bin/true\"\nuser = -1", "syntax"),
        ("exec = \"/bin/true\"\nslice = \"cells\"", "syntax"),
    ];
    for (text, expected) in cases {
        let error = ServiceSpec::parse(text, "t").unwrap_err().to_string();
        assert!(error.contains(expected), "{text:?}: {error}");
    }
    let groups: Vec<String> = (0..17).map(|n| n.to_string()).collect();
    let text = format!("exec = \"/bin/true\"\ngroups = [{}]", groups.join(","));
    assert_eq!(
        ServiceSpec::parse(&text, "t"),
        Err(SpecError::Bound("groups"))
    );
}

#[test]
fn a_directory_is_read_in_name_order_and_a_second_name_is_refused() {
    let dir = std::env::temp_dir().join(format!("ultar-init-spec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("b.toml"), "exec = \"/bin/b\"").unwrap();
    std::fs::write(dir.join("a.toml"), "exec = \"/bin/a\"").unwrap();
    std::fs::write(dir.join("c.toml"), "name = \"a\"\nexec = \"/bin/c\"").unwrap();
    std::fs::write(dir.join("d.toml"), "exec = 1").unwrap();
    std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
    let (specs, refused) = load_dir(std::slice::from_ref(&dir));
    let names: Vec<&str> = specs.iter().map(|spec| spec.name.as_str()).collect();
    assert_eq!(names, ["a", "b"]);
    assert_eq!(refused.len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_shipped_service_files_parse() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../rootfs/etc/ultar/services");
    let (specs, refused) = load_dir(&[dir]);
    assert!(refused.is_empty(), "{refused:?}");
    assert!(specs.len() >= 4);
}

#[test]
fn boot_profiles_decide_which_services_exist_and_start() {
    let plain = ServiceSpec::parse("exec = \"/usr/bin/a\"", "a").unwrap();
    assert!(plain
        .clone()
        .in_profile("system")
        .is_some_and(|s| !s.life.manual));
    assert!(plain.in_profile("installer").is_none());
    let manual = ServiceSpec::parse("exec = \"/usr/bin/a\"\nmanual = true", "a").unwrap();
    assert!(manual
        .clone()
        .in_profile("system")
        .is_some_and(|s| s.life.manual));
    assert!(manual.in_profile("installer").is_none());
    let both = "exec = \"/usr/bin/c\"\n[profiles]\nsystem = \"manual\"\ninstaller = \"start\"\n";
    let both = ServiceSpec::parse(both, "c").unwrap();
    assert!(both.life.manual);
    assert!(both
        .clone()
        .in_profile("system")
        .is_some_and(|s| s.life.manual));
    assert!(both
        .clone()
        .in_profile("installer")
        .is_some_and(|s| !s.life.manual));
    assert!(both.in_profile("rescue").is_none());
    let only = ServiceSpec::parse(
        "exec = \"/usr/bin/i\"\n[profiles]\ninstaller = \"start\"\n",
        "i",
    )
    .unwrap();
    assert!(only.clone().in_profile("system").is_none());
    assert!(only.in_profile("installer").is_some());
}

#[test]
fn faulty_profile_tables_are_refused() {
    let refused = |text: &str| ServiceSpec::parse(text, "x").is_err();
    assert!(refused(
        "exec = \"/a\"\nmanual = true\n[profiles]\ninstaller = \"start\"\n"
    ));
    assert!(refused("exec = \"/a\"\n[profiles]\n"));
    assert!(refused(
        "exec = \"/a\"\n[profiles]\ninstaller = \"sometimes\"\n"
    ));
    assert!(refused("exec = \"/a\"\n[profiles]\n\"../x\" = \"start\"\n"));
    let many: String = (0..9).map(|i| format!("p{i} = \"start\"\n")).collect();
    assert!(refused(&format!("exec = \"/a\"\n[profiles]\n{many}")));
}
