//! Review round 3: frame times against the budget of plan §3.6 ("Bildzeit Oberfläche
//! (99 %)": 8 ms on a PC, 16 ms on the phone). Unlike `perf.rs` this takes the 99th
//! percentile of many frames and asserts it: the dialog and sheet frames at full HD, scale
//! 2 on a 4K panel, a phone surface and a list of 2000 rows.
//!
//! Wall time only means something in an optimised build, so the tests are ignored in debug
//! builds and run (and assert) with `cargo test --release --test r3_perf -- --test-threads=1`.
//! A busy machine makes single frames slow; a budget that is missed is measured again (at
//! most `ROUNDS` times) before the test fails, the best 99th percentile counts.

mod common;

use common::gallery::{file_menu, gallery, Gallery};
use std::time::Instant;
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Event, Key, Modifiers};
use ultar_ui::layout::Sizing;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::ui::Ui;
use ultar_ui::widgets::{Label, List, ListItem, Scroll};
use ultar_ui::{Lang, Point, Size};

const FULL_HD: Size = Size::new(1920.0, 1080.0);
const FRAMES: usize = 200;
const ROUNDS: usize = 3;
/// Budget of plan §3.6 for a PC (99 % of the frames), in milliseconds.
const BUDGET_PC_MS: f64 = 8.0;
/// Budget of plan §3.6 for the phone, in milliseconds.
const BUDGET_PHONE_MS: f64 = 16.0;
/// A 4K panel at scale 2 has four times the pixels of full HD; the plan names no budget for
/// it, the phone budget (two PC budgets) is asserted.
const BUDGET_4K_MS: f64 = 16.0;

/// The 99th percentile of `FRAMES` runs of `frame`, in milliseconds; asserted against
/// `budget` (best of up to `ROUNDS` measurements).
fn measure(name: &str, budget: f64, mut frame: impl FnMut()) -> f64 {
    frame();
    let mut best = f64::MAX;
    for _ in 0..ROUNDS {
        let mut times: Vec<f64> = (0..FRAMES)
            .map(|_| {
                let start = Instant::now();
                frame();
                start.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        times.sort_by(f64::total_cmp);
        let (median, p99) = (times[FRAMES / 2], times[FRAMES * 99 / 100]);
        println!(
            "{name}: median {median:.2} ms, p99 {p99:.2} ms, max {:.2} ms (budget {budget} ms)",
            times[FRAMES - 1]
        );
        best = best.min(p99);
        if best <= budget {
            break;
        }
    }
    assert!(best <= budget, "{name}: p99 {best:.2} ms over {budget} ms");
    best
}

fn canvas() -> Canvas {
    Canvas::new(1920, 1080, PixelFormat::Bgrx8).expect("canvas")
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "wall time: cargo test --release --test r3_perf"
)]
fn p01_full_hd_frames_of_the_gallery() {
    for (lang, theme) in [(Lang::Ar, ThemeKind::Dark), (Lang::En, ThemeKind::Light)] {
        let mut ui = common::ui(lang, theme, FULL_HD);
        let g = gallery(&mut ui).expect("gallery");
        let mut frame = canvas();
        let tag = lang.code();
        measure(&format!("{tag} full repaint"), BUDGET_PC_MS, || {
            ui.paint_all(&mut frame)
        });
        measure(
            &format!("{tag} layout + full repaint"),
            BUDGET_PC_MS,
            || {
                ui.invalidate_layout();
                ui.paint(&mut frame);
            },
        );
        let mut down = false;
        measure(
            &format!("{tag} wheel step (damage only)"),
            BUDGET_PC_MS,
            || {
                down = !down;
                let lines = if down { 2.0 } else { -2.0 };
                let pos = Point::new(900.0, 700.0);
                ui.handle(Event::Scroll { pos, lines });
                ui.paint(&mut frame);
            },
        );
        measure(&format!("{tag} menu open + close"), BUDGET_PC_MS, || {
            ui.open_menu(g.menu_button, file_menu()).expect("menu");
            ui.paint(&mut frame);
            ui.close_menu();
            ui.paint(&mut frame);
        });
        dialog_frames(&mut ui, &g, &mut frame, tag);
    }
}

/// The frames of the gallery's dialog: opening it (backdrop dimmed once), a full repaint
/// under it and a focus step inside it.
fn dialog_frames(ui: &mut Ui, g: &Gallery, frame: &mut Canvas, tag: &str) {
    measure(
        &format!("{tag} dialog opens (scrim over full HD)"),
        BUDGET_PC_MS,
        || {
            ui.open_dialog(g.modal.dialog).expect("open");
            ui.paint(frame);
            ui.close_dialog(g.modal.dialog).expect("close");
        },
    );
    ui.open_dialog(g.modal.dialog).expect("open");
    measure(
        &format!("{tag} full repaint under a dialog"),
        BUDGET_PC_MS,
        || ui.paint_all(frame),
    );
    measure(
        &format!("{tag} Tab inside the dialog (damage only)"),
        BUDGET_PC_MS,
        || {
            let modifiers = Modifiers::NONE;
            ui.handle(Event::Key {
                key: Key::Tab,
                modifiers,
            });
            ui.paint(frame);
        },
    );
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "wall time: cargo test --release --test r3_perf"
)]
fn p02_scale_two_on_a_4k_panel() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, FULL_HD);
    ui.set_scale(2.0).expect("scale");
    let g = gallery(&mut ui).expect("gallery");
    let mut frame = Canvas::new(3840, 2160, PixelFormat::Bgrx8).expect("canvas");
    measure(
        "ar 3840x2160 at scale 2: full repaint",
        BUDGET_4K_MS,
        || ui.paint_all(&mut frame),
    );
    ui.open_dialog(g.modal.sheet).expect("open");
    measure(
        "ar 3840x2160 at scale 2: full repaint under a sheet",
        BUDGET_4K_MS,
        || ui.paint_all(&mut frame),
    );
}

fn long_list(ui: &mut Ui, rows: usize) -> ultar_ui::ui::NodeId {
    let root = ui.root();
    let scroll = ui
        .add_sized(root, Scroll::new(), Sizing::grow(1))
        .expect("scroll");
    let list = ui.add(scroll, List::new().selectable()).expect("list");
    for i in 0..rows {
        let item = ListItem::new(Label::data(&format!("الملف رقم {i}")))
            .sublabel(Label::data(&format!("{i} MB · مستند")))
            .separator();
        ui.add(list, item).expect("item");
    }
    scroll
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "wall time: cargo test --release --test r3_perf"
)]
fn p03_a_long_list_at_full_hd() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, FULL_HD);
    let start = Instant::now();
    let scroll = long_list(&mut ui, 2000);
    let mut frame = canvas();
    ui.paint(&mut frame);
    let first = start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "2000 rows, {} nodes: build + first frame {first:.1} ms",
        ui.node_count()
    );
    // Building 2000 rows and the first frame is a one-off; it gets four PC frames.
    assert!(
        first <= 4.0 * BUDGET_PC_MS,
        "build + first frame {first:.1} ms"
    );
    let mut offset = 0.0;
    measure("2000 rows: wheel step", BUDGET_PC_MS, || {
        offset += 96.0;
        ui.set_scroll_offset(scroll, offset).expect("offset");
        ui.paint(&mut frame);
    });
    measure("2000 rows: layout + full repaint", BUDGET_PC_MS, || {
        ui.invalidate_layout();
        ui.paint(&mut frame);
    });
    let modifiers = Modifiers::NONE;
    ui.handle(Event::Key {
        key: Key::Tab,
        modifiers,
    });
    ui.handle(Event::Key {
        key: Key::Tab,
        modifiers,
    });
    measure("2000 rows: arrow key step", BUDGET_PC_MS, || {
        ui.handle(Event::Key {
            key: Key::ArrowDown,
            modifiers,
        });
        ui.paint(&mut frame);
    });
    measure("2000 rows: accessibility tree", BUDGET_PC_MS, || {
        assert!(ui.a11y_tree().len() > 2000);
    });
}

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "wall time: cargo test --release --test r3_perf"
)]
fn p04_a_phone_surface() {
    // 1080 × 2400 pixels at scale 3 (360 × 800 points), both directions.
    for (lang, theme) in [(Lang::Ar, ThemeKind::Dark), (Lang::En, ThemeKind::Light)] {
        let mut ui = common::ui(lang, theme, Size::new(360.0, 800.0));
        ui.set_scale(3.0).expect("scale");
        let g = gallery(&mut ui).expect("gallery");
        let mut frame = Canvas::new(1080, 2400, PixelFormat::Bgrx8).expect("canvas");
        let tag = lang.code();
        measure(
            &format!("{tag} phone: full repaint"),
            BUDGET_PHONE_MS,
            || ui.paint_all(&mut frame),
        );
        measure(
            &format!("{tag} phone: sheet opens"),
            BUDGET_PHONE_MS,
            || {
                ui.open_dialog(g.modal.sheet).expect("open");
                ui.paint(&mut frame);
                ui.close_dialog(g.modal.sheet).expect("close");
            },
        );
        ui.open_dialog(g.modal.dialog).expect("open");
        measure(
            &format!("{tag} phone: full repaint under a dialog"),
            BUDGET_PHONE_MS,
            || ui.paint_all(&mut frame),
        );
    }
}
