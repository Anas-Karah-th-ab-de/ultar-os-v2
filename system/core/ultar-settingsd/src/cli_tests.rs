use super::*;

fn parsed(words: &[&str]) -> Result<Invocation, UsageError> {
    parse(words.iter().map(|word| (*word).to_owned()), None)
}

fn command(words: &[&str]) -> Command {
    parsed(words).unwrap().command
}

#[test]
fn commands() {
    assert_eq!(command(&[]), Command::Help);
    assert_eq!(command(&["help"]), Command::Help);
    assert_eq!(command(&["get", "a.b", "--help"]), Command::Help);
    assert_eq!(command(&["get", "a.b"]), Command::Get("a.b".into()));
    assert_eq!(
        command(&["set", "a.b", "on"]),
        Command::Set("a.b".into(), "on".into())
    );
    assert_eq!(command(&["reset", "a.b"]), Command::Reset("a.b".into()));
    assert_eq!(command(&["list"]), Command::List(None));
    assert_eq!(
        command(&["list", "power"]),
        Command::List(Some("power".into()))
    );
    assert_eq!(command(&["watch"]), Command::Watch(None));
    assert_eq!(command(&["watch", "--count", "3"]), Command::Watch(Some(3)));
    assert_eq!(command(&["--count", "3", "watch"]), Command::Watch(Some(3)));
    assert_eq!(command(&["confirm"]), Command::Confirm(None));
    assert_eq!(
        command(&["revert", "a.b"]),
        Command::Revert(Some("a.b".into()))
    );
}

#[test]
fn options() {
    let invocation = parsed(&["--socket", "/tmp/s", "--lang", "de", "list"]).unwrap();
    assert_eq!(invocation.socket, Some(PathBuf::from("/tmp/s")));
    assert_eq!(invocation.lang, Some(Lang::De));
    let with_file = parsed(&["--token-file", "/t", "list"]).unwrap();
    assert_eq!(with_file.token, Some(SecretSource::File("/t".into())));
    let with_fd = parsed(&["--token-fd", "3", "list"]).unwrap();
    assert_eq!(with_fd.token, Some(SecretSource::Fd(3)));
    assert!(parsed(&["--token-fd", "2", "list"]).is_err());
    assert!(parsed(&["--token-fd", "3", "--token-file", "/t", "list"]).is_err());
    assert!(parsed(&["--token", "t", "list"]).is_err());
    let env = |socket: &str| parse(["list".to_owned()], Some(socket.to_owned())).unwrap();
    assert_eq!(env("/run/bus").socket, Some(PathBuf::from("/run/bus")));
    assert_eq!(env("").socket, None);
    assert_eq!(parsed(&["list"]).unwrap().lang, None);
}

#[test]
fn language_hint_survives_a_broken_command_line() {
    let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        lang_hint(&words(&["--lang", "en", "frobnicate"])),
        Some(Lang::En)
    );
    assert_eq!(
        lang_hint(&words(&["x", "--frob", "--lang", "de"])),
        Some(Lang::De)
    );
    assert_eq!(lang_hint(&words(&["--lang"])), None);
    assert_eq!(lang_hint(&words(&["--lang", "xx"])), None);
    assert_eq!(lang_hint(&words(&["list"])), None);
}

#[test]
fn usage_errors() {
    for words in [
        &["get"][..],
        &["get", "a", "b"],
        &["set", "a"],
        &["set", "a", "b", "c"],
        &["reset"],
        &["list", "a", "b"],
        &["watch", "x"],
        &["frobnicate"],
        &["--socket"],
        &["--lang", "xx", "list"],
        &["--count", "0", "watch"],
        &["--count", "x", "watch"],
        &["--count", "2", "list"],
    ] {
        assert_eq!(parsed(words), Err(UsageError::Arguments), "{words:?}");
    }
    assert_eq!(
        parsed(&["--frob", "list"]),
        Err(UsageError::UnknownOption("--frob".into()))
    );
    assert!(!UsageError::Arguments.to_string().is_empty());
    assert!(UsageError::UnknownOption("--x".into())
        .to_string()
        .contains("--x"));
}

#[test]
fn errors_map_to_message_keys() {
    let cases = [
        (CliError::UnknownKey, "settingsd.error.unknown_key"),
        (
            CliError::Value(ValueError::OutOfRange),
            "settingsd.error.out_of_range",
        ),
        (
            CliError::Bus(BusError::Remote(ErrorCode::PermissionDenied)),
            "settingsd.error.not_permitted",
        ),
        (
            CliError::Bus(BusError::Timeout),
            "settingsd.error.no_service",
        ),
        (CliError::Bus(BusError::Closed), "settingsd.error.bus"),
        (
            CliError::from(io::Error::from(io::ErrorKind::BrokenPipe)),
            "usettings.error.output",
        ),
    ];
    let translator = crate::i18n::translator(Lang::Ar).unwrap();
    for (error, key) in cases {
        assert_eq!(error.message_key(), key);
        assert!(translator.has(key), "{key}");
        assert!(!error.to_string().is_empty());
    }
}
