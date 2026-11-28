//! The formula engine of the spreadsheet app: every function the package asks for, the
//! references, the error values and the cycle.

use ultar_office::sheet::{Value, Workbook};

/// 2024-03-15 18:00 as an Excel serial number.
const NOW: f64 = 45_366.75;

fn book() -> Workbook {
    let mut w = Workbook::new();
    w.sheets[0].name = "Daten".into();
    for (cell, raw) in [
        ("A1", "1"),
        ("A2", "2"),
        ("A3", "3"),
        ("A4", "text"),
        ("A5", "10"),
        ("B1", "a"),
        ("B2", "b"),
        ("B3", "c"),
        ("B4", "d"),
        ("B5", "e"),
        ("C1", "TRUE"),
        ("D1", "-4.5"),
    ] {
        let _ = w.set_ref(0, cell, raw);
    }
    let _ = w.set_ref(0, "Zwei!A1", "100");
    let _ = w.set_ref(0, "Zwei!A2", "200");
    w
}

fn ev(f: &str) -> Value {
    book().eval_formula_at(0, f, NOW)
}

fn num(f: &str) -> f64 {
    match ev(f) {
        Value::Num(n) => n,
        other => panic!("{f} = {other:?}"),
    }
}

fn text(f: &str) -> String {
    match ev(f) {
        Value::Text(t) => t,
        other => panic!("{f} = {other:?}"),
    }
}

fn err(f: &str) -> String {
    match ev(f) {
        Value::Err(e) => e,
        other => panic!("{f} = {other:?}"),
    }
}

#[test]
fn sums_averages_and_counts() {
    assert_eq!(num("=SUM(1,2,3)"), 6.0);
    assert_eq!(num("=SUM(A1:A3)"), 6.0);
    // text in a range is skipped, empty cells too
    assert_eq!(num("=SUM(A1:A5)"), 16.0);
    assert_eq!(num("=SUM(A1:A3;A5)"), 16.0);
    assert_eq!(num("=AVERAGE(A1:A3)"), 2.0);
    assert_eq!(num("=MIN(A1:A5)"), 1.0);
    assert_eq!(num("=MAX(A1:A5,D1)"), 10.0);
    assert_eq!(num("=MIN(D1,A1)"), -4.5);
    assert_eq!(num("=COUNT(A1:A5)"), 4.0);
    assert_eq!(num("=COUNTA(A1:A5)"), 5.0);
    assert_eq!(num("=COUNTA(A1:A7)"), 5.0);
    assert_eq!(num("=PRODUCT(A1:A3)"), 6.0);
    assert_eq!(num("=MEDIAN(A1,A2,A3,A5)"), 2.5);
    assert_eq!(err("=AVERAGE(B1:B3)"), "#DIV/0!");
}

#[test]
fn logic() {
    assert_eq!(text("=IF(A1>0,\"yes\",\"no\")"), "yes");
    assert_eq!(text("=IF(A1>5,\"yes\",\"no\")"), "no");
    assert_eq!(ev("=IF(A1>5,\"yes\")"), Value::Bool(false));
    assert_eq!(ev("=AND(A1>0,A2>1)"), Value::Bool(true));
    assert_eq!(ev("=AND(A1>0,A2>5)"), Value::Bool(false));
    assert_eq!(ev("=OR(A1>5,A2>1)"), Value::Bool(true));
    assert_eq!(ev("=OR(A1>5,A2>5)"), Value::Bool(false));
    assert_eq!(ev("=NOT(A1>5)"), Value::Bool(true));
    assert_eq!(ev("=NOT(C1)"), Value::Bool(false));
    assert_eq!(num("=IFERROR(1/0,7)"), 7.0);
    assert_eq!(num("=IFERROR(A1+1,7)"), 2.0);
    assert_eq!(ev("=ISERROR(1/0)"), Value::Bool(true));
    assert_eq!(ev("=ISERROR(1)"), Value::Bool(false));
    assert_eq!(ev("=1<2"), Value::Bool(true));
    assert_eq!(ev("=\"a\"=\"A\""), Value::Bool(true));
    assert_eq!(ev("=1<>1"), Value::Bool(false));
}

#[test]
fn numbers() {
    assert_eq!(num("=ROUND(2.567,2)"), 2.57);
    assert_eq!(num("=ROUND(2.5,0)"), 3.0);
    assert_eq!(num("=ROUND(-2.5,0)"), -3.0);
    assert_eq!(num("=ROUND(1234.567,-2)"), 1200.0);
    assert_eq!(num("=ROUNDUP(2.01,1)"), 2.1);
    assert_eq!(num("=ROUNDDOWN(2.99,1)"), 2.9);
    assert_eq!(num("=ABS(-3)"), 3.0);
    assert_eq!(num("=ABS(D1)"), 4.5);
    assert_eq!(num("=INT(-1.5)"), -2.0);
    assert_eq!(num("=SQRT(16)"), 4.0);
    assert_eq!(num("=POWER(2,10)"), 1024.0);
    assert_eq!(num("=2^3^2"), 512.0);
    assert_eq!(num("=MOD(-7,3)"), 2.0);
    assert_eq!(num("=50%"), 0.5);
    assert_eq!(num("=2+3*4-(1+1)"), 12.0);
    assert_eq!(num("=-A2+A3"), 1.0);
    assert!((num("=PI()") - std::f64::consts::PI).abs() < 1e-12);
}

#[test]
fn text_functions() {
    assert_eq!(text("=CONCAT(\"a\",\"b\",A1)"), "ab1");
    assert_eq!(text("=CONCATENATE(\"x\",B1)"), "xa");
    assert_eq!(text("=\"a\"&\"b\"&A2"), "ab2");
    assert_eq!(text("=B1&B2&B3"), "abc");
    assert_eq!(num("=LEN(\"مرحبا\")"), 5.0);
    assert_eq!(num("=LEN(A4)"), 4.0);
    assert_eq!(text("=LEFT(\"abcdef\",2)"), "ab");
    assert_eq!(text("=LEFT(\"abcdef\")"), "a");
    assert_eq!(text("=RIGHT(\"abcdef\",3)"), "def");
    assert_eq!(text("=MID(\"abcdef\",2,3)"), "bcd");
    assert_eq!(text("=MID(\"مرحبا\",2,2)"), "رح");
    assert_eq!(text("=UPPER(\"abc\")"), "ABC");
    assert_eq!(text("=LOWER(\"ABC\")"), "abc");
    assert_eq!(text("=TRIM(\"  a   b \")"), "a b");
    assert_eq!(text("=TEXTJOIN(\"-\",TRUE,B1:B3)"), "a-b-c");
    assert_eq!(text("=TEXT(0.256,\"0.0%\")"), "25.6%");
    assert_eq!(num("=VALUE(\"12.5\")"), 12.5);
}

#[test]
fn dates_and_the_clock() {
    assert_eq!(num("=TODAY()"), 45_366.0);
    assert_eq!(num("=NOW()"), NOW);
    assert_eq!(num("=DATE(2024,1,1)"), 45_292.0);
    assert_eq!(num("=DATE(2024,14,1)"), num("=DATE(2025,2,1)"));
    assert_eq!(num("=YEAR(45292)"), 2024.0);
    assert_eq!(num("=MONTH(45366)"), 3.0);
    assert_eq!(num("=DAY(TODAY())"), 15.0);
    assert_eq!(num("=TODAY()-DATE(2024,3,1)"), 14.0);
}

#[test]
fn lookups_and_conditional_sums() {
    let mut w = Workbook::new();
    for (c, v) in [
        ("A1", "1"), ("B1", "one"),
        ("A2", "3"), ("B2", "three"),
        ("A3", "5"), ("B3", "five"),
        ("D1", "apple"), ("E1", "10"),
        ("D2", "banana"), ("E2", "20"),
        ("D3", "apple"), ("E3", "30"),
    ] {
        let _ = w.set_ref(0, c, v);
    }
    let at = |f: &str| w.eval_formula_at(0, f, NOW);
    assert_eq!(at("=VLOOKUP(3,A1:B3,2,FALSE)"), Value::Text("three".into()));
    assert_eq!(at("=VLOOKUP(4,A1:B3,2,TRUE)"), Value::Text("three".into()));
    assert_eq!(at("=VLOOKUP(4,A1:B3,2,FALSE)"), Value::Err("#N/A".into()));
    assert_eq!(at("=VLOOKUP(3,A1:B3,3,FALSE)"), Value::Err("#REF!".into()));
    assert_eq!(at("=INDEX(A1:B3,2,2)"), Value::Text("three".into()));
    assert_eq!(at("=MATCH(5,A1:A3,0)"), Value::Num(3.0));
    assert_eq!(at("=SUMIF(D1:D3,\"apple\",E1:E3)"), Value::Num(40.0));
    assert_eq!(at("=SUMIF(E1:E3,\">15\")"), Value::Num(50.0));
    assert_eq!(at("=COUNTIF(D1:D3,\"apple\")"), Value::Num(2.0));
    assert_eq!(at("=COUNTIF(D1:D3,\"a*\")"), Value::Num(2.0));
    assert_eq!(at("=COUNTIF(E1:E3,\"<>20\")"), Value::Num(2.0));
    assert_eq!(at("=AVERAGEIF(D1:D3,\"apple\",E1:E3)"), Value::Num(20.0));
}

#[test]
fn references_between_cells_and_sheets() {
    let mut w = book();
    let _ = w.set_ref(0, "F1", "=A1+A2");
    let _ = w.set_ref(0, "F2", "=$A$1*10");
    let _ = w.set_ref(0, "F3", "=Zwei!A1+Zwei!A2");
    let _ = w.set_ref(0, "F4", "=SUM(Zwei!A1:A2)");
    let _ = w.set_ref(0, "F5", "=F1*2");
    let _ = w.set_ref(0, "Mein Blatt!A1", "7");
    let _ = w.set_ref(0, "F6", "='Mein Blatt'!A1+1");
    let v = |c: u32| w.value_at(0, c, 5, NOW);
    assert_eq!(v(0), Value::Num(3.0));
    assert_eq!(v(1), Value::Num(10.0));
    assert_eq!(v(2), Value::Num(300.0));
    assert_eq!(v(3), Value::Num(300.0));
    assert_eq!(v(4), Value::Num(6.0));
    assert_eq!(v(5), Value::Num(8.0));
}

#[test]
fn errors_look_like_excels() {
    assert_eq!(err("=1/0"), "#DIV/0!");
    assert_eq!(err("=A1/(A2-2)"), "#DIV/0!");
    assert_eq!(err("=NOSUCH(1)"), "#NAME?");
    assert_eq!(err("=\"abc\"+1"), "#VALUE!");
    assert_eq!(err("=A4*2"), "#VALUE!");
    assert_eq!(err("=#REF!+1"), "#REF!");
    assert_eq!(err("=Fehlt!A1"), "#REF!");
    assert_eq!(err("=SQRT(-1)"), "#NUM!");
    assert_eq!(err("=VLOOKUP(9,A1:B3,2,FALSE)"), "#N/A");
    // an error travels through the formulas that read it
    assert_eq!(err("=SUM(1,1/0)"), "#DIV/0!");
    assert_eq!(err("=IF(1/0,1,2)"), "#DIV/0!");
    // a broken formula is an error, never a crash
    assert!(matches!(ev("=1+"), Value::Err(_)));
    assert!(matches!(ev("=SUM(1,"), Value::Err(_)));
    assert!(matches!(ev("=(1"), Value::Err(_)));
    assert!(matches!(ev("=@@"), Value::Err(_)));
}

#[test]
fn a_cycle_is_detected_in_every_shape() {
    let mut w = Workbook::new();
    let _ = w.set_ref(0, "A1", "=A1+1");
    let _ = w.set_ref(0, "B1", "=C1");
    let _ = w.set_ref(0, "C1", "=D1");
    let _ = w.set_ref(0, "D1", "=B1");
    let _ = w.set_ref(0, "E1", "=SUM(E1:E3)");
    let _ = w.set_ref(0, "F1", "=B1+1");
    for c in 0..6 {
        assert_eq!(w.value_at(0, 0, c, NOW), Value::Err("#CIRC!".into()), "column {c}");
    }
}

#[test]
fn the_list_of_functions_is_complete() {
    for f in [
        "SUM", "AVERAGE", "MIN", "MAX", "COUNT", "COUNTA", "IF", "AND", "OR", "NOT", "ROUND", "ABS",
        "CONCAT", "LEN", "LEFT", "RIGHT", "MID", "UPPER", "LOWER", "TODAY", "NOW", "VLOOKUP",
        "IFERROR", "SUMIF", "COUNTIF",
    ] {
        assert!(ultar_office::sheet::FUNCTIONS.contains(&f), "{f}");
    }
}
