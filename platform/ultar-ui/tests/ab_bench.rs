//! A/B frame benchmark: the minimum of N frames for the paths that matter, printed as
//! `AB <name> <ms>` lines. `tools/ui-bench/ab.sh <ref>` builds this file in the current
//! tree and in an older commit and runs them interleaved, so a regression (round 5: a full
//! repaint 2.7× slower) shows as a ratio. Ignored by default; release builds only.

mod common;

use std::time::Instant;
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::{Lang, Size};

/// Frames per measurement; the minimum is the least noisy statistic on a shared machine.
fn rounds() -> usize {
    std::env::var("AB_FRAMES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60)
}

fn min_ms(mut frame: impl FnMut()) -> f64 {
    frame();
    (0..rounds())
        .map(|_| {
            let start = Instant::now();
            frame();
            start.elapsed().as_secs_f64() * 1e3
        })
        .fold(f64::MAX, f64::min)
}

#[test]
#[ignore = "A/B benchmark: tools/ui-bench/ab.sh"]
fn ab_frames() {
    for (scale, w, h) in [(1.0, 1920, 1080), (2.0, 3840, 2160)] {
        let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(1920.0, 1080.0));
        ui.set_scale(scale).expect("scale");
        let g = common::gallery::gallery(&mut ui).expect("gallery");
        let mut c = Canvas::new(w, h, PixelFormat::Bgrx8).expect("canvas");
        println!("AB full_repaint_{w} {:.3}", min_ms(|| ui.paint_all(&mut c)));
        println!(
            "AB layout_repaint_{w} {:.3}",
            min_ms(|| {
                ui.invalidate_layout();
                ui.paint(&mut c);
            })
        );
        println!(
            "AB dialog_open_{w} {:.3}",
            min_ms(|| {
                ui.open_dialog(g.modal.dialog).expect("open");
                ui.paint(&mut c);
                ui.close_dialog(g.modal.dialog).expect("close");
                ui.paint(&mut c);
            })
        );
        ui.open_dialog(g.modal.sheet).expect("open");
        println!("AB under_sheet_{w} {:.3}", min_ms(|| ui.paint_all(&mut c)));
        ui.close_dialog(g.modal.sheet).expect("close");
    }
}
