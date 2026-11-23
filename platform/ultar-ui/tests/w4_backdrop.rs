//! Review round 4 (second reviewer): the dimmed backdrop under a dialog (wave-4 fix of M1).
//! A frame painted incrementally while a dialog is open must equal the first frame of a
//! fresh `Ui` in the same state, for every change behind the dialog; a spinner behind the
//! dialog must not turn every frame into a full repaint.
//! Target path: platform/ultar-ui/tests/w4_backdrop.rs

mod common;

use std::time::Instant;
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::tokens::{ColorRole, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{
    Button, Dialog, Label, List, ListItem, Progress, Scroll, Switch, Text, Toast, Tone,
};
use ultar_ui::{Direction, Lang, Size};

const SIZE: Size = Size::new(480.0, 360.0);

/// CI asserts medians (`ULTAR_PERF_STAT=median`, see `r3_perf.rs`), a quiet machine p99.
fn median_only() -> bool {
    std::env::var("ULTAR_PERF_STAT").is_ok_and(|v| v == "median")
}

fn stat_name() -> &'static str {
    if median_only() {
        "median"
    } else {
        "p99"
    }
}

struct Scene {
    label: NodeId,
    switch: NodeId,
    progress: NodeId,
    scroll: NodeId,
    button: NodeId,
    dialog: NodeId,
}

fn k(key: &str) -> Label {
    Label::key(key)
}

fn scene(ui: &mut Ui) -> Scene {
    let root = ui.root();
    ui.set_root_flex(Flex::column());
    let label = ui.add(root, Text::new(k("demo.settings"))).unwrap();
    let switch = ui.add(root, Switch::new(k("demo.night"), false)).unwrap();
    let progress = ui
        .add(root, Progress::new(k("demo.progress")).value(0.25))
        .unwrap();
    let button = ui.add(root, Button::new(k("demo.apply"))).unwrap();
    let scroll = ui.add_sized(root, Scroll::new(), Sizing::grow(1)).unwrap();
    let list = ui.add(scroll, List::new()).unwrap();
    for i in 0..30 {
        ui.add(list, ListItem::new(Label::data(&format!("row {i}"))))
            .unwrap();
    }
    let dialog = ui.add(root, Dialog::new(k("demo.dialog.title"))).unwrap();
    ui.add(dialog, Button::new(k("demo.cancel"))).unwrap();
    Scene {
        label,
        switch,
        progress,
        scroll,
        button,
        dialog,
    }
}

type Change = fn(&mut Ui, &Scene);

fn changes() -> Vec<(&'static str, Change)> {
    vec![
        ("label", |ui, s| {
            ui.set_label(s.label, Label::data("a much longer title than before"))
                .unwrap()
        }),
        ("switch", |ui, s| ui.set_on(s.switch, true).unwrap()),
        ("progress", |ui, s| {
            ui.set_progress(s.progress, Some(0.9)).unwrap()
        }),
        ("scroll", |ui, s| {
            ui.set_scroll_offset(s.scroll, 120.0).unwrap()
        }),
        ("loading", |ui, s| ui.set_loading(s.button, true).unwrap()),
        ("disabled", |ui, s| ui.set_disabled(s.button, true).unwrap()),
        ("theme", |ui, _| ui.set_theme(ThemeKind::Light)),
        ("lang", |ui, _| ui.set_lang(Lang::En)),
        ("direction", |ui, _| ui.set_direction(Direction::Ltr)),
        ("size", |ui, _| {
            ui.set_size(Size::new(400.0, 300.0)).unwrap()
        }),
        ("background", |ui, _| ui.set_background(ColorRole::Surface)),
        ("toast", |ui, _| {
            ui.show_toast(Toast::new(1, Tone::Info, k("demo.bar.success")))
                .unwrap()
        }),
    ]
}

fn canvas(ui: &Ui) -> Canvas {
    let size = ui.size();
    Canvas::new(size.w as u32, size.h as u32, PixelFormat::Bgrx8).unwrap()
}

/// Pixels that differ between two canvases of the same size.
fn differing(a: &Canvas, b: &Canvas) -> usize {
    a.data()
        .chunks(4)
        .zip(b.data().chunks(4))
        .filter(|(x, y)| x != y)
        .count()
}

#[test]
fn bd01_incremental_frames_under_a_dialog_equal_a_fresh_frame() {
    let mut failures = Vec::new();
    for (name, change) in changes() {
        let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SIZE);
        let s = scene(&mut ui);
        let mut frame = canvas(&ui);
        ui.paint(&mut frame);
        ui.open_dialog(s.dialog).unwrap();
        ui.paint(&mut frame);
        change(&mut ui, &s);
        if ui.size() != SIZE {
            frame = canvas(&ui);
        }
        ui.paint(&mut frame);
        let mut fresh = common::ui(Lang::Ar, ThemeKind::Dark, SIZE);
        let t = scene(&mut fresh);
        fresh.open_dialog(t.dialog).unwrap();
        change(&mut fresh, &t);
        let mut reference = canvas(&fresh);
        fresh.paint(&mut reference);
        let diff = differing(&frame, &reference);
        if diff != 0 {
            failures.push(format!("{name}: {diff} pixels"));
        }
    }
    assert!(failures.is_empty(), "stale pixels: {failures:?}");
}

/// A spinner (loading button, indeterminate bar) behind an open dialog is part of the
/// dimmed backdrop. Every clock step re-dims the whole surface today: the damage of one
/// animation frame is the whole surface instead of the spinner.
#[test]
fn bd02_a_spinner_behind_a_dialog_damages_only_the_spinner() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SIZE);
    let s = scene(&mut ui);
    ui.set_loading(s.button, true).unwrap();
    ui.open_dialog(s.dialog).unwrap();
    let mut frame = canvas(&ui);
    ui.advance_to(1000);
    ui.paint(&mut frame);
    frame.take_damage();
    ui.advance_to(1016);
    ui.paint(&mut frame);
    let area = frame.damage().area();
    let surface = i64::from(frame.width()) * i64::from(frame.height());
    assert!(
        area * 4 < surface,
        "one animation frame behind a dialog repainted {area} of {surface} pixels"
    );
}

/// The cost of bd02 at full HD: an animation frame behind a dialog (release only).
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "wall time: cargo test --release --test w4_backdrop"
)]
fn bd03_spinner_behind_a_dialog_at_full_hd_costs_little() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(1920.0, 1080.0));
    let g = common::gallery::gallery(&mut ui).unwrap();
    ui.set_progress(g.progress, None).unwrap();
    ui.open_dialog(g.modal.dialog).unwrap();
    let mut frame = Canvas::new(1920, 1080, PixelFormat::Bgrx8).unwrap();
    let mut now = 1000;
    ui.advance_to(now);
    ui.paint(&mut frame);
    let mut best = f64::MAX;
    for _ in 0..3 {
        let mut times: Vec<f64> = (0..200)
            .map(|_| {
                now += 16;
                let start = Instant::now();
                ui.advance_to(now);
                ui.paint(&mut frame);
                start.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        times.sort_by(f64::total_cmp);
        println!(
            "spinner behind dialog: median {:.2} ms, p99 {:.2} ms",
            times[100], times[198]
        );
        best = best.min(if median_only() {
            times[100]
        } else {
            times[198]
        });
    }
    assert!(
        best <= 8.0 * 0.5,
        "{} {best:.2} ms for a 2% spinner change",
        stat_name()
    );
}

/// Opening a dialog and a sheet on a 4K desktop panel at scale 2 (a PC: plan §3.6, 8 ms).
/// `r3_perf` p02 measures only the repaint under an already open sheet, against 16 ms.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "wall time: cargo test --release --test w4_backdrop"
)]
fn bd04_opening_a_dialog_on_a_4k_panel_within_the_pc_budget() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(1920.0, 1080.0));
    ui.set_scale(2.0).unwrap();
    let g = common::gallery::gallery(&mut ui).unwrap();
    let mut frame = Canvas::new(3840, 2160, PixelFormat::Bgrx8).unwrap();
    ui.paint(&mut frame);
    for (name, dialog) in [("dialog", g.modal.dialog), ("sheet", g.modal.sheet)] {
        let mut best = f64::MAX;
        for _ in 0..3 {
            let mut times: Vec<f64> = (0..40)
                .map(|_| {
                    let start = Instant::now();
                    ui.open_dialog(dialog).unwrap();
                    ui.paint(&mut frame);
                    let took = start.elapsed().as_secs_f64() * 1000.0;
                    ui.close_dialog(dialog).unwrap();
                    ui.paint(&mut frame);
                    took
                })
                .collect();
            times.sort_by(f64::total_cmp);
            println!(
                "4K {name} opens: median {:.2} ms, p99 {:.2} ms",
                times[20], times[39]
            );
            best = best.min(if median_only() { times[20] } else { times[39] });
        }
        let stat = stat_name();
        assert!(
            best <= 8.0,
            "4K {name} opens: {stat} {best:.2} ms over 8 ms"
        );
    }
}
