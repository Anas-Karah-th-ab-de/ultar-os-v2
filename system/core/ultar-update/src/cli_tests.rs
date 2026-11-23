use super::*;

fn policy() -> PolicyMessage {
    PolicyMessage {
        automatic: true,
        window: None,
        channel: "stable".into(),
        allow_metered: false,
        rate_limit_kib: 0,
    }
}

#[test]
fn policy_changes() {
    let mut p = policy();
    for change in [
        "automatic=no",
        "metered=yes",
        "channel=beta",
        "rate=512",
        "window=23:00-05:30",
    ] {
        apply_policy_change(&mut p, change).unwrap();
    }
    assert!(!p.automatic && p.allow_metered);
    assert_eq!((p.channel.as_str(), p.rate_limit_kib), ("beta", 512));
    assert_eq!(p.window, Some((23 * 60, 5 * 60 + 30)));
    apply_policy_change(&mut p, "window=any").unwrap();
    assert_eq!(p.window, None);
    for bad in [
        "automatic",
        "automatic=maybe",
        "window=25:00-01:00",
        "window=1:00",
        "window=01:60-02:00",
        "rate=fast",
        "colour=blue",
    ] {
        assert!(apply_policy_change(&mut p, bad).is_err(), "{bad}");
    }
}

#[test]
fn bus_errors_map_to_message_keys() {
    let code = UpdateError::Metered.code();
    assert_eq!(
        bus_error_key(&BusError::Remote(ErrorCode::Service(code))),
        "update.error.metered"
    );
    assert_eq!(
        bus_error_key(&BusError::Remote(ErrorCode::Service(9999))),
        "update.error.io"
    );
    assert_eq!(
        bus_error_key(&BusError::Remote(ErrorCode::PermissionDenied)),
        "update.error.permission_denied"
    );
    assert_eq!(bus_error_key(&BusError::Timeout), "update.cli.timeout");
    assert_eq!(bus_error_key(&BusError::Closed), "update.cli.no_bus");
}

#[test]
fn local_verify_can_use_the_keys_in_force_on_the_device() {
    use crate::keys::{KeySet, Rotation, TrustStore, KEY_FILE_SUFFIX};
    use crate::sign::public_of;
    const OLD: [u8; 32] = [3; 32];
    const NEW: [u8; 32] = [4; 32];
    let dir = tempfile::tempdir().unwrap();
    let (keys, state) = (dir.path().join("keys"), dir.path().join("state"));
    std::fs::create_dir_all(&keys).unwrap();
    let burned = keys.join(format!("old{KEY_FILE_SUFFIX}"));
    std::fs::write(burned, hex::encode(public_of(&OLD))).unwrap();
    // A bundle signed with the burned-in key ...
    let payload = dir.path().join("k");
    std::fs::write(&payload, b"kernel").unwrap();
    let mut spec = crate::manifest::tests::sample();
    spec.payloads.clear();
    let files =
        [crate::signtool::parse_payload_arg(&format!("kernel={}", payload.display())).unwrap()];
    let manifest = crate::signtool::build_manifest(spec, &files).unwrap();
    let signed = crate::signtool::sign_manifest(&manifest, &OLD).unwrap();
    let bundle = dir.path().join("b.uup");
    crate::signtool::make_bundle(&signed, &[], &files, &bundle).unwrap();
    // ... and a device that has retired that key since.
    let set = KeySet {
        serial: 1,
        keys: vec![public_of(&NEW)],
    };
    let rotation = Rotation { set, created: 1 }.sign(&OLD).unwrap();
    TrustStore::open(&keys, &state)
        .unwrap()
        .accept(&rotation)
        .unwrap();
    let tr = crate::msg::translator(ultar_i18n::Lang::En);
    let run_verify = |with_state: bool| {
        let mut words = vec!["verify".to_string(), bundle.display().to_string()];
        words.extend(["--keys".to_string(), keys.display().to_string()]);
        if with_state {
            words.extend(["--state".to_string(), state.display().to_string()]);
        }
        let mut out = Vec::new();
        (
            run(&words, None, &tr, &mut out),
            String::from_utf8(out).unwrap(),
        )
    };
    assert_eq!(
        run_verify(false).0,
        EXIT_OK,
        "burned-in set alone still knows the key"
    );
    let (code, text) = run_verify(true);
    assert_eq!(code, EXIT_FAILED, "{text}");
    assert!(text.contains("does not trust"), "{text}");
}

#[test]
fn usage_and_unreachable_bus() {
    let tr = crate::msg::translator(ultar_i18n::Lang::En);
    let run_words = |text: &str, env: Option<&str>| {
        let words: Vec<String> = text.split_whitespace().map(str::to_string).collect();
        let mut out = Vec::new();
        let code = run(&words, env.map(str::to_string), &tr, &mut out);
        (code, String::from_utf8(out).unwrap())
    };
    for bad in [
        "",
        "status",
        "--socket",
        "frobnicate --socket /x --bogus y",
        "verify file.uup",
    ] {
        let (code, text) = run_words(bad, None);
        assert_eq!(code, EXIT_USAGE, "{bad}");
        assert!(text.starts_with("Usage: uupdate"), "{text}");
    }
    let (code, text) = run_words("status", Some("/nonexistent/ultar-bus.sock"));
    assert_eq!(code, EXIT_FAILED);
    assert_eq!(text.trim(), "Could not reach the update service");
    let (code, text) = run_words("verify /nonexistent.uup --keys /nonexistent-keys", None);
    assert_eq!(code, EXIT_FAILED);
    assert!(text.contains("storage"), "{text}");
}
