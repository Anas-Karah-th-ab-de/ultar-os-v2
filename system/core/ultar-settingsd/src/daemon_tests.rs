//! Tests of the daemon's command line and start-up.

use super::*;

fn parsed(words: &[&str], env: Option<&str>) -> Result<DaemonCommand, DaemonArgError> {
    parse(
        words.iter().map(|w| (*w).to_owned()),
        env.map(str::to_owned),
    )
}

fn args(words: &[&str], env: Option<&str>) -> DaemonArgs {
    match parsed(words, env).unwrap() {
        DaemonCommand::Run(args) => args,
        DaemonCommand::Help => panic!("expected a run command"),
    }
}

#[test]
fn minimal_and_full_command_lines() {
    let minimal = args(&["--store", "/var/lib/s", "--allow-all"], Some("/run/bus"));
    assert_eq!(minimal.store, PathBuf::from("/var/lib/s"));
    assert_eq!(minimal.socket, PathBuf::from("/run/bus"));
    assert_eq!(minimal.user, "default");
    assert_eq!(minimal.policy, None);
    assert_eq!(minimal.write_delay, DEFAULT_WRITE_DELAY);
    assert!(!minimal.log_apply);

    let full = args(
        &[
            "--store",
            "/s",
            "--socket",
            "/b",
            "--user",
            "anas",
            "--policy",
            "/p.toml",
            "--profile",
            "/d.toml",
            "--write-delay-ms",
            "40",
            "--token-file",
            "/run/tk",
            "--log-socket",
            "/run/log.sock",
            "--allow-root",
            "--log-apply",
            "--lang",
            "en",
        ],
        Some("/ignored"),
    );
    assert_eq!(full.socket, PathBuf::from("/b"));
    assert_eq!(full.user, "anas");
    assert_eq!(full.policy, Some(PathBuf::from("/p.toml")));
    assert_eq!(full.profile, Some(PathBuf::from("/d.toml")));
    assert_eq!(full.write_delay, Duration::from_millis(40));
    assert_eq!(full.token_file, Some(PathBuf::from("/run/tk")));
    assert_eq!(full.log_socket, Some(PathBuf::from("/run/log.sock")));
    assert!(full.allow_root && full.token_fd.is_none());
    assert!(!minimal.allow_root && minimal.token_file.is_none());
    assert_eq!(full.lang, Some(Lang::En));
    assert!(full.log_apply);
}

#[test]
fn write_delay_is_capped_and_the_token_may_come_from_a_descriptor() {
    let capped = args(
        &[
            "--store",
            "/s",
            "--allow-all",
            "--write-delay-ms",
            "999999999",
        ],
        Some("/b"),
    );
    assert_eq!(capped.write_delay, MAX_WRITE_DELAY);
    let by_fd = args(
        &["--store", "/s", "--allow-all", "--token-fd", "4"],
        Some("/b"),
    );
    assert_eq!(by_fd.token_fd, Some(4));
}

#[test]
fn help_and_rejections() {
    assert_eq!(parsed(&["--help"], None), Ok(DaemonCommand::Help));
    assert_eq!(
        parsed(&["-h", "--frob"], None),
        Err(DaemonArgError::Arguments)
    );
    assert_eq!(
        parsed(&["--store", "/s"], Some("/b")),
        Err(DaemonArgError::NoPolicy)
    );
    // Review finding 7: both together used to mean "allow all".
    assert_eq!(
        parsed(
            &["--store", "/s", "--allow-all", "--policy", "/p"],
            Some("/b")
        ),
        Err(DaemonArgError::PolicyConflict)
    );
    for words in [
        // A secret on the command line is not accepted at all.
        &["--store", "/s", "--allow-all", "--token", "tk"][..],
        &["--store", "/s", "--allow-all", "--token-fd", "x"],
        &[
            "--store",
            "/s",
            "--allow-all",
            "--token-fd",
            "4",
            "--token-file",
            "/t",
        ],
        &["--allow-all"],
        &["--store"],
        &["--store", "/s", "--allow-all", "--write-delay-ms", "soon"],
        &["--store", "/s", "--allow-all", "--lang", "xx"],
        &["--store", "/s", "--allow-all", "extra"],
    ] {
        assert_eq!(
            parsed(words, Some("/b")),
            Err(DaemonArgError::Arguments),
            "{words:?}"
        );
    }
    // No socket anywhere.
    assert_eq!(
        parsed(&["--store", "/s", "--allow-all"], None),
        Err(DaemonArgError::Arguments)
    );
    assert_eq!(
        parsed(&["--store", "/s", "--allow-all"], Some("")),
        Err(DaemonArgError::Arguments)
    );
}

#[test]
fn every_message_key_of_the_daemon_exists_in_all_languages() {
    let errors = [
        DaemonError::Policy("p".into()),
        DaemonError::Profile("p".into()),
        DaemonError::Schema(SchemaError::InvalidKey("k")),
        DaemonError::Store(SettingsError::Limit),
        DaemonError::Bus(BusError::Timeout),
        DaemonError::Token(SecretError::TooPermissive),
    ];
    let mut keys: Vec<&str> = errors.iter().map(DaemonError::message_key).collect();
    keys.extend([
        DaemonArgError::Arguments.message_key(),
        DaemonArgError::NoPolicy.message_key(),
        DaemonArgError::PolicyConflict.message_key(),
        "settingsd.daemon.root_refused",
        "settingsd.daemon.hardening_failed",
        "settingsd.daemon.usage",
        "settingsd.daemon.allow_all",
        "settingsd.daemon.write_error",
        "settingsd.daemon.ready",
        "settingsd.daemon.stopped",
    ]);
    for (lang, _) in crate::i18n::sources() {
        let translator = crate::i18n::translator(lang).unwrap();
        for key in &keys {
            assert!(translator.catalog(lang).contains(key), "{key}");
        }
    }
    for error in errors {
        assert!(!error.detail().is_empty());
        assert!(error.to_string().starts_with("settingsd.daemon."));
    }
    assert!(!DaemonArgError::NoPolicy.to_string().is_empty());
    assert!(!DaemonArgError::PolicyConflict.to_string().is_empty());
}

#[test]
fn unusable_policy_and_profile_files_stop_the_start() {
    let dir = crate::testdir::TestDir::new("daemon-files");
    let mut base = args(&["--store", "/s", "--allow-all"], Some("/b"));
    assert!(load_access(&base).is_ok());
    assert_eq!(load_capabilities(&base).unwrap(), Capabilities::default());
    base.policy = Some(dir.path().join("missing.toml"));
    assert!(matches!(load_access(&base), Err(DaemonError::Policy(_))));
    let bad = dir.path().join("bad.toml");
    std::fs::write(&bad, "this is = not [a profile").unwrap();
    base.profile = Some(bad);
    assert!(matches!(
        load_capabilities(&base),
        Err(DaemonError::Profile(_))
    ));
    base.profile = Some(dir.path().join("missing.toml"));
    assert!(matches!(
        load_capabilities(&base),
        Err(DaemonError::Profile(_))
    ));
}
