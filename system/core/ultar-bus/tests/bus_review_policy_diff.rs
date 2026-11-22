mod common;
use common::XorShift;
use std::collections::BTreeMap;
use ultar_bus::Policy;

#[test]
fn differential_fuzz_subset_reader_vs_toml() {
    let pieces: &[&str] = &[
        "[[rule]]",
        "[[rule]]\n",
        "[rule]",
        "[[ rule ]]",
        "[ [rule] ]",
        "[[rule]",
        "]",
        "[",
        "\n",
        "\n",
        "\n",
        " ",
        "\t",
        "\r",
        "\r\n",
        "identity",
        "call",
        "publish",
        "subscribe",
        "register",
        "=",
        " = ",
        ",",
        "\"a\"",
        "\"*\"",
        "'app.*'",
        "\"a.b\"",
        "\"settings.admin\"",
        "[\"*\"]",
        "[]",
        "# c",
        "#",
        "''",
        "\"\"",
        "\"\"\"",
        "'''",
        "\"a#b\"",
        "'a\"b'",
        "\"a'b\"",
        "a.b",
        "\"identity\"",
        "'call'",
        "{",
        "}",
        "true",
        "1",
        "\u{feff}",
        "\u{a0}",
        "\"a\\u0061\"",
        "\\",
        "\"a\" ",
        ".",
        "identity.x",
        "\x0c",
        "\x00",
        "\u{7f}",
        "\"a\tb\"",
        "'a\tb'",
    ];
    let mut rng = XorShift(0x1234_5678_9abc_def1);
    let mut lenient: BTreeMap<String, String> = BTreeMap::new();
    let mut differ: Vec<String> = Vec::new();
    let mut strict = 0usize;
    for _ in 0..400_000 {
        let n = 1 + rng.below(14) as usize;
        let mut text = String::new();
        for _ in 0..n {
            text.push_str(pieces[rng.below(pieces.len() as u64) as usize]);
        }
        let ours = Policy::from_toml_str(&text);
        let reference = toml::from_str::<Policy>(&text);
        match (ours, reference) {
            (Ok(a), Ok(b)) => {
                if a != b && differ.len() < 10 {
                    differ.push(format!("{text:?}\n   ours={a:?}\n   toml={b:?}"));
                }
            }
            (Ok(a), Err(e)) => {
                if !a.rules.is_empty() && !text.contains('#') {
                    let msg = format!(
                        "{}|{}",
                        e.message(),
                        text.chars()
                            .filter(|c| c.is_control() && *c != '\n' && *c != '\t')
                            .map(|c| format!("{:x}", c as u32))
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    lenient
                        .entry(msg)
                        .or_insert(format!("{text:?} -> rules={}", a.rules.len()));
                }
            }
            (Err(_), Ok(_)) => strict += 1,
            (Err(_), Err(_)) => {}
        }
    }
    eprintln!("stricter than TOML in {strict} samples (harmless)");
    eprintln!(
        "ACCEPTED BY SUBSET, REJECTED BY TOML ({} distinct reasons):",
        lenient.len()
    );
    for (why, sample) in &lenient {
        eprintln!("  {why}: {sample}");
    }
    eprintln!("BOTH ACCEPT, DIFFERENT POLICY ({}):", differ.len());
    for d in &differ {
        eprintln!("  {d}");
    }
    assert!(differ.is_empty() && lenient.is_empty());
}
