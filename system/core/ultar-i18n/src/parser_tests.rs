//! Tests of the catalog parser.

use super::*;

fn kind(value: &str) -> ParseErrorKind {
    parse_value(value).unwrap_err()
}

#[test]
fn keys_are_validated() {
    for good in ["a", "settings.display.title", "a_b-c.d1"] {
        assert!(is_valid_key(good), "{good}");
    }
    for bad in ["", "1a", ".a", "a.", "a..b", "a b", "مفتاح", "a=b", "_a"] {
        assert!(!is_valid_key(bad), "{bad}");
    }
}

#[test]
fn entries_and_continuations() {
    let source =
        "\u{feff}# head\na = one\n  two\n\nb = {n ->\n[one] x\n\n*[other] y\n}\n# end\nc=z\n";
    let entries = split_entries(source).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!((entries[0].line, entries[0].key.as_str()), (2, "a"));
    assert_eq!(entries[0].value, "one\ntwo");
    assert_eq!(entries[1].line, 5);
    assert_eq!(entries[1].value, "{n ->\n[one] x\n*[other] y\n}");
    assert_eq!(
        (entries[2].key.as_str(), entries[2].value.as_str()),
        ("c", "z")
    );
}

#[test]
fn line_level_errors() {
    let error = |source: &str| split_entries(source).unwrap_err();
    assert_eq!(error("  x").kind, ParseErrorKind::OrphanContinuation);
    assert_eq!(
        error("a = b\n\n  x").kind,
        ParseErrorKind::OrphanContinuation
    );
    assert_eq!(error("a = b\n\n  x").line, 3);
    assert_eq!(error("just text").kind, ParseErrorKind::MissingEquals);
    assert_eq!(error("bad key = x").kind, ParseErrorKind::InvalidKey);
    assert_eq!(error("bad key = x").key.as_deref(), Some("bad key"));
    assert_eq!(error(" = x\n").kind, ParseErrorKind::OrphanContinuation);
    assert_eq!(error("= x\n").kind, ParseErrorKind::InvalidKey);
}

#[test]
fn braces_in_literals_do_not_open_blocks() {
    assert_eq!(open_braces("a {\"{\"} b"), 0);
    assert_eq!(open_braces("{n ->"), 1);
    assert_eq!(open_braces("} }"), 0);
    assert_eq!(open_braces("{\"\\\"{\"}"), 0);
    assert_eq!(
        parse_value("a {\"{\"}b{ \"}\" } {\"\\\"q\\\"\"}").unwrap(),
        [Part::Text("a {b} \"q\"".to_string())]
    );
}

#[test]
fn placeholders_and_fluent_dollar() {
    let parts = parse_value("x { $name } y {_n-1}").unwrap();
    assert_eq!(parts.len(), 4);
    assert_eq!(parts[1], Part::Placeholder("name".to_string()));
    assert_eq!(parts[3], Part::Placeholder("_n-1".to_string()));
}

#[test]
fn value_level_errors() {
    assert_eq!(kind(""), ParseErrorKind::EmptyValue);
    assert_eq!(kind("a {b"), ParseErrorKind::UnclosedBrace);
    assert_eq!(kind("a {"), ParseErrorKind::UnclosedBrace);
    assert_eq!(kind("a }"), ParseErrorKind::UnexpectedClosingBrace);
    assert_eq!(kind("a {}"), ParseErrorKind::InvalidPlaceholder);
    assert_eq!(kind("a {1x}"), ParseErrorKind::InvalidPlaceholder);
    assert_eq!(kind("a {b c}"), ParseErrorKind::InvalidPlaceholder);
    assert_eq!(kind("a {\"b}"), ParseErrorKind::UnclosedLiteral);
    assert_eq!(kind("a {\"b\\"), ParseErrorKind::UnclosedLiteral);
    assert_eq!(kind("a {\"b\" x}"), ParseErrorKind::InvalidPlaceholder);
    assert_eq!(kind("a {\"b\""), ParseErrorKind::UnclosedBrace);
    assert_eq!(
        kind("{n -> [one] a}"),
        ParseErrorKind::MissingDefaultVariant
    );
    assert_eq!(kind("{n -> }"), ParseErrorKind::MissingDefaultVariant);
    assert_eq!(
        kind("{n -> *[one] a *[other] b}"),
        ParseErrorKind::MultipleDefaultVariants
    );
    assert_eq!(
        kind("{n -> [one] a *[one] b}"),
        ParseErrorKind::DuplicateVariant
    );
    assert_eq!(
        kind("{n -> text *[other] b}"),
        ParseErrorKind::InvalidVariant
    );
    assert_eq!(kind("{n -> [bad key] b}"), ParseErrorKind::InvalidVariant);
    assert_eq!(kind("{n -> *[other] b"), ParseErrorKind::UnclosedBrace);
}

#[test]
fn variants_are_trimmed_and_keep_inner_brackets() {
    let parts = parse_value("{n ->\n [one]  a[1] {n}  \n *[other] b }").unwrap();
    let Part::Select(select) = &parts[0] else {
        panic!("select expected")
    };
    assert_eq!(select.default, 1);
    assert_eq!(
        select.variants[0].pattern,
        [
            Part::Text("a[1] ".to_string()),
            Part::Placeholder("n".to_string())
        ]
    );
    assert_eq!(select.variants[1].pattern, [Part::Text("b".to_string())]);
}

#[test]
fn nesting_is_limited() {
    let nested = |levels: usize| {
        let mut text = String::from("x");
        for _ in 0..levels {
            text = alloc::format!("{{n -> *[other] {text}}}");
        }
        text
    };
    assert!(parse_value(&nested(MAX_DEPTH)).is_ok());
    assert_eq!(kind(&nested(MAX_DEPTH + 1)), ParseErrorKind::NestingTooDeep);
}
