//! Review round 5 (second reviewer): cost of the accessibility tree of a long table, now
//! that every cell is a node (wave 5, W4). `List` docs send long data to `Table`
//! ("thousands of files"); `MAX_TABLE_ROWS` is 100 000 with up to 16 columns.
//! Target path: platform/ultar-ui/tests/w5_perf.rs

mod common;

use std::time::Instant;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::widgets::{Label, Table};
use ultar_ui::{Lang, Size};

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "wall time: cargo test --release --test w5_perf"
)]
fn pf01_accessibility_tree_of_a_folder_of_10000_files() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(1920.0, 1080.0));
    let root = ui.root();
    let columns = ["Name", "Size", "Kind", "Modified"]
        .map(Label::data)
        .to_vec();
    let rows = (0..10_000)
        .map(|i| {
            vec![
                Label::data(&format!("ملف {i}.odt")),
                Label::data(&format!("{i} KB")),
                Label::key("demo.kind.doc"),
                Label::data("2026-11-23"),
            ]
        })
        .collect();
    ui.add(root, Table::new(Label::data("Files"), columns).rows(rows))
        .unwrap();
    ui.layout();
    let mut times = Vec::new();
    let mut count = 0;
    for _ in 0..20 {
        let start = Instant::now();
        count = ui.a11y_tree().len();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    let bytes = count * std::mem::size_of::<ultar_ui::ui::A11yNode>();
    println!(
        "{count} nodes ({:.1} MB of A11yNode), median {:.1} ms, max {:.1} ms",
        bytes as f64 / 1e6,
        times[10],
        times[19]
    );
    assert!(
        times[10] <= 8.0,
        "a11y tree of one table: median {:.1} ms",
        times[10]
    );
}
