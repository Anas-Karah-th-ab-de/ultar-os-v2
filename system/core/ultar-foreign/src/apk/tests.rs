//! The APK readers against real and broken files.

use super::*;

fn report() -> Report {
    Report {
        package: "org.example.calc".into(),
        label: "Calculator".into(),
        labels: vec![("ar".into(), "آلة حاسبة".into())],
        version: "1.4.2".into(),
        version_code: 42,
        min_sdk: 21,
        activity: "org/example/calc/Main".into(),
        abi: Abi::Java,
        sig: Sig::Valid(2),
        signers: vec![("ab".repeat(32), "Jane".into())],
        permissions: vec!["android.permission.INTERNET".into()],
        icon: Some(b"\x89PNG\r\n\x1a\nrest".to_vec()),
    }
}

#[test]
fn a_report_survives_its_lines_and_odd_answers_are_refused() {
    let r = report();
    assert_eq!(Report::from_lines(&r.to_lines()), Some(r.clone()));
    let lines = r.to_lines();
    for (from, to) in [
        ("package=org.example.calc", "package=../etc"),
        ("activity=org/example/calc/Main", "activity=../../x"),
        ("sig=v2", "sig=v9"),
        ("\nend\n", "\n"),
        ("abi=java", "abi=sparc"),
        ("code=42", "code=x"),
    ] {
        assert!(
            Report::from_lines(&lines.replacen(from, to, 1)).is_none(),
            "{to}"
        );
    }
    let unsigned = lines.replacen("sig=v2", "sig=none", 1);
    assert!(
        Report::from_lines(&unsigned).is_none(),
        "signers without a signature"
    );
}

#[test]
fn refusals_follow_processor_and_signature() {
    use ultar_bus::messages::foreign::Refusal as R;
    let mut r = report();
    assert_eq!(r.refusal(), None);
    r.abi = Abi::Arm;
    assert_eq!(r.refusal(), Some(R::NeedsArm));
    r.abi = Abi::X86_64;
    r.sig = Sig::OldOnly;
    assert_eq!(r.refusal(), Some(R::OldSignature));
    r.sig = Sig::Valid(3);
    r.activity.clear();
    assert_eq!(r.refusal(), Some(R::NoStartScreen));
}

#[test]
fn names_and_libraries() {
    assert!(package_ok("org.happysanta.gd"));
    for bad in ["org", "org..x", "1org.x", "org.x/y", "org.x-y", ""] {
        assert!(!package_ok(bad), "{bad}");
    }
    assert_eq!(lib_name("lib/x86_64/libgame.so"), Some("libgame.so"));
    for bad in [
        "lib/arm64-v8a/libx.so",
        "lib/x86_64/../x.so",
        "lib/x86_64/a/b.so",
        "lib/x86_64/.so",
        "lib/x86_64/x.txt",
    ] {
        assert_eq!(lib_name(bad), None, "{bad}");
    }
    let names = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(abi_of(&names(&["classes.dex"])), Abi::Java);
    assert_eq!(
        abi_of(&names(&["lib/arm64-v8a/a.so", "lib/x86_64/a.so"])),
        Abi::X86_64
    );
    assert_eq!(
        abi_of(&names(&["lib/arm64-v8a/a.so", "lib/armeabi-v7a/a.so"])),
        Abi::Arm
    );
    assert_eq!(abi_of(&names(&["lib/x86/a.so"])), Abi::Other);
}

#[test]
fn base64_round_trips() {
    for n in 0..20 {
        let data: Vec<u8> = (0..n).map(|i| (i * 37) as u8).collect();
        assert_eq!(unbase64(&base64(&data)), Some(data));
    }
    assert_eq!(unbase64("abc"), None);
    assert_eq!(unbase64("a=bc"), None);
}

#[test]
fn a_whole_apk_is_read() {
    use crate::apk::zip::tests::build;
    let manifest = axml::tests::sample();
    let table = arsc::tests::sample(
        "Calculator",
        "آلة حاسبة",
        &[
            (240, "res/mipmap-hdpi/ic.png"),
            (640, "res/mipmap-xxxhdpi/ic.png"),
        ],
    );
    let big_icon = b"\x89PNG\r\n\x1a\nbig".to_vec();
    let apk = build(
        &[
            ("AndroidManifest.xml", &manifest),
            ("resources.arsc", &table),
            ("res/mipmap-hdpi/ic.png", b"\x89PNG\r\n\x1a\nsmall"),
            ("res/mipmap-xxxhdpi/ic.png", &big_icon),
            ("lib/x86_64/libx.so", b"\x7fELF"),
            ("classes.dex", b"dex"),
        ],
        true,
    );
    let path = std::env::temp_dir().join(format!("apk-{}.apk", std::process::id()));
    std::fs::write(&path, &apk).unwrap();
    let r = analyze(std::fs::File::open(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(r.package, "org.example.calc");
    assert_eq!(r.label, "Calculator");
    assert_eq!(r.labels, vec![("ar".to_owned(), "آلة حاسبة".to_owned())]);
    assert_eq!(r.version, "1.4.2");
    assert_eq!(r.activity, "org/example/calc/Main");
    assert_eq!(r.abi, Abi::X86_64);
    assert_eq!(r.sig, Sig::Unsigned);
    assert_eq!(r.icon, Some(big_icon));
    assert_eq!(
        r.refusal(),
        Some(ultar_bus::messages::foreign::Refusal::Unsigned)
    );
    assert_eq!(Report::from_lines(&r.to_lines()), Some(r));
    // signed: nothing refused, the signer is known
    let signed = crate::apk::testsign::resign(&apk, "Ultar Test A").unwrap();
    std::fs::write(&path, &signed).unwrap();
    let r = analyze(std::fs::File::open(&path).unwrap()).unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(r.sig, Sig::Valid(2));
    assert_eq!(r.refusal(), None);
    assert_eq!(r.signers.len(), 1);
    assert_eq!(r.signers[0].1, "Ultar Test A");
    assert_eq!(Report::from_lines(&r.to_lines()), Some(r));
}
