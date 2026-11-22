use super::*;

fn check_format(format: TextFormat, good: &[&str], bad: &[&str]) {
    for text in good {
        assert!(
            format.accepts(text),
            "{} should accept {text:?}",
            format.name()
        );
    }
    for text in bad {
        assert!(
            !format.accepts(text),
            "{} should reject {text:?}",
            format.name()
        );
    }
}

#[test]
fn text_formats_of_values_people_type() {
    check_format(
        TextFormat::Free { max_chars: 3 },
        &["", "abc", "عرب"],
        &["abcd", "a\nb"],
    );
    check_format(
        TextFormat::Time,
        &["00:00", "23:59"],
        &["24:00", "12:60", "1:00", "12-00", "١٢:٠٠", ""],
    );
    check_format(
        TextFormat::Resolution,
        &["auto", "1920x1080", "65535x1"],
        &[
            "0x100",
            "1920",
            "1920x",
            "x1080",
            "65536x1",
            "01x1",
            "1920X1080",
        ],
    );
    check_format(
        TextFormat::Path,
        &["", "/home/a/صور/b.png"],
        &["relative", "/a\u{0}b"],
    );
}

#[test]
fn text_formats_of_identifiers() {
    check_format(TextFormat::DisplayId, &["", "edp-1"], &["EDP", "a.b"]);
    check_format(TextFormat::Name, &["ultar-noir"], &["", "Noir", "a b"]);
    check_format(TextFormat::Currency, &["SYP", "EUR"], &["syp", "EURO", ""]);
    check_format(
        TextFormat::TimeZone,
        &[
            "UTC",
            "Asia/Damascus",
            "America/Argentina/Salta",
            "Etc/GMT+3",
        ],
        &["Damascus", "Asia/", "/x", "Asia/Dam ascus", ""],
    );
    check_format(
        TextFormat::Host,
        &["ultar.code-craft.tech", "localhost", "server:8443"],
        &["", "a..b", "-a.b", "A.b", "a:0", "a:99999", "a:b"],
    );
}

#[test]
fn bool_and_mismatch() {
    assert_eq!(ValueType::Bool.validate(&Value::Bool(true)), Ok(()));
    assert_eq!(
        ValueType::Bool.validate(&Value::Int(1)),
        Err(ValueError::TypeMismatch {
            expected: "bool",
            found: "int"
        })
    );
    assert!(ValueType::Enum(&["a"])
        .validate(&Value::Bool(true))
        .is_err());
}

#[test]
fn int_range_and_step() {
    let scale = ValueType::Int {
        min: 100,
        max: 300,
        step: 25,
    };
    for good in [100, 125, 300] {
        assert_eq!(scale.validate(&Value::Int(good)), Ok(()));
    }
    assert_eq!(scale.validate(&Value::Int(99)), Err(ValueError::OutOfRange));
    assert_eq!(
        scale.validate(&Value::Int(325)),
        Err(ValueError::OutOfRange)
    );
    assert_eq!(scale.validate(&Value::Int(110)), Err(ValueError::OffStep));
    let wide = ValueType::Int {
        min: i64::MIN,
        max: i64::MAX,
        step: 1,
    };
    assert_eq!(wide.validate(&Value::Int(i64::MAX)), Ok(()));
    assert_eq!(wide.validate(&Value::Int(i64::MIN)), Ok(()));
}

#[test]
fn float_range_step_and_coercion() {
    let rate = ValueType::Float {
        min: 0.0,
        max: 10.0,
        step: 0.5,
    };
    assert_eq!(rate.validate(&Value::Float(2.5)), Ok(()));
    assert_eq!(rate.validate(&Value::Float(2.3)), Err(ValueError::OffStep));
    assert_eq!(
        rate.validate(&Value::Float(10.5)),
        Err(ValueError::OutOfRange)
    );
    assert_eq!(
        rate.validate(&Value::Float(f64::NAN)),
        Err(ValueError::OutOfRange)
    );
    assert_eq!(
        rate.validate(&Value::Float(f64::INFINITY)),
        Err(ValueError::OutOfRange)
    );
    assert_eq!(rate.accept(Value::Int(3)), Ok(Value::Float(3.0)));
    assert!(rate.accept(Value::Int(11)).is_err());
    assert!(rate.validate(&Value::Int(3)).is_err());
    let free = ValueType::Float {
        min: 0.0,
        max: 1.0,
        step: 0.0,
    };
    assert_eq!(free.validate(&Value::Float(0.123_456)), Ok(()));
}

#[test]
fn enum_text_and_list() {
    let theme = ValueType::Enum(&["dark", "light"]);
    assert_eq!(theme.validate(&Value::text("dark")), Ok(()));
    assert_eq!(
        theme.validate(&Value::text("pink")),
        Err(ValueError::UnknownChoice)
    );
    let time = ValueType::Text(TextFormat::Time);
    assert_eq!(
        time.validate(&Value::text("7pm")),
        Err(ValueError::BadFormat { format: "time" })
    );
    let list = ValueType::List {
        item: TextFormat::Name,
        max_len: 2,
    };
    assert_eq!(list.validate(&Value::list(&["a", "b"])), Ok(()));
    assert_eq!(list.validate(&Value::list(&[])), Ok(()));
    assert_eq!(
        list.validate(&Value::list(&["a", "b", "c"])),
        Err(ValueError::TooLong)
    );
    assert_eq!(
        list.validate(&Value::list(&["a", "a"])),
        Err(ValueError::Duplicate)
    );
    assert!(list.validate(&Value::list(&["A"])).is_err());
    let ids = ValueType::List {
        item: TextFormat::DisplayId,
        max_len: 4,
    };
    assert!(ids.validate(&Value::list(&[""])).is_err());
}

#[test]
fn parsing_typed_text() {
    assert_eq!(ValueType::Bool.parse_text("on"), Ok(Value::Bool(true)));
    assert_eq!(ValueType::Bool.parse_text("0"), Ok(Value::Bool(false)));
    assert!(ValueType::Bool.parse_text("maybe").is_err());
    let int = ValueType::Int {
        min: 0,
        max: 10,
        step: 1,
    };
    assert_eq!(int.parse_text("7"), Ok(Value::Int(7)));
    assert_eq!(int.parse_text("11"), Err(ValueError::OutOfRange));
    assert!(int.parse_text("x").is_err());
    let float = ValueType::Float {
        min: 0.0,
        max: 1.0,
        step: 0.0,
    };
    assert_eq!(float.parse_text("0.5"), Ok(Value::Float(0.5)));
    assert!(float.parse_text("nan").is_err());
    let list = ValueType::List {
        item: TextFormat::Name,
        max_len: 3,
    };
    assert_eq!(
        list.parse_text("a, b,,c"),
        Ok(Value::list(&["a", "b", "c"]))
    );
    assert_eq!(list.parse_text(""), Ok(Value::List(Vec::new())));
    assert_eq!(
        ValueType::Enum(&["x"]).parse_text("y"),
        Err(ValueError::UnknownChoice)
    );
}

#[test]
fn errors_have_keys_and_text() {
    let all = [
        ValueError::TypeMismatch {
            expected: "bool",
            found: "int",
        },
        ValueError::OutOfRange,
        ValueError::OffStep,
        ValueError::UnknownChoice,
        ValueError::BadFormat { format: "time" },
        ValueError::TooLong,
        ValueError::Duplicate,
    ];
    for error in all {
        assert!(error.message_key().starts_with("settingsd.error."));
        assert!(!error.to_string().is_empty());
    }
}
