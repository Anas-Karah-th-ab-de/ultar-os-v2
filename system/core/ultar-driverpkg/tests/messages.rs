//! Every message key used in the source has a text in all three languages, and no row is unused.

use std::collections::BTreeSet;
use ultar_driverpkg::msg::{keys, text, Lang};

fn keys_in_source() -> BTreeSet<String> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeSet::new();
    for item in std::fs::read_dir(src).unwrap() {
        let path = item.unwrap().path();
        if path.extension().is_some_and(|x| x == "rs")
            && path.file_name().is_some_and(|n| n != "msg.rs")
        {
            let code = std::fs::read_to_string(&path).unwrap();
            for piece in code.split('"').filter(|p| p.starts_with("driverpkg.")) {
                if piece
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
                {
                    found.insert(piece.to_string());
                }
            }
        }
    }
    found
}

#[test]
fn source_keys_and_tables_agree() {
    let used = keys_in_source();
    assert!(
        used.len() > 80,
        "source scan found only {} keys",
        used.len()
    );
    for key in &used {
        for lang in Lang::ALL {
            assert!(text(lang, key).is_some(), "no {lang:?} text for {key}");
        }
    }
    let table: BTreeSet<String> = keys().map(str::to_string).collect();
    let unused: Vec<&String> = table.difference(&used).collect();
    assert!(unused.is_empty(), "table rows without use: {unused:?}");
}
