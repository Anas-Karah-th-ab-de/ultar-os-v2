//! Frame-time measurement (plan §3.6). Ignored by default because it measures wall time:
//! `cargo test --release --test perf -- --ignored --nocapture`.

mod common;

use common::{center, display_settings};
use std::time::Instant;
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::Event;
use ultar_ui::layout::{Flex, Padding, Sizing};
use ultar_ui::tokens::{Space, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{Card, Label, Row, Switch};
use ultar_ui::{Lang, Size};

const FULL_HD: Size = Size::new(1920.0, 1080.0);
const FRAMES: u32 = 60;

/// A typical full-screen settings surface: the display card plus two more columns of rows.
fn typical_screen(ui: &mut Ui) -> NodeId {
    let screen = display_settings(ui).expect("screen builds");
    let root = ui.root();
    let columns = ui
        .add(
            root,
            Flex::row().gap(Space::S24).padding(Padding::all(Space::S8)),
        )
        .expect("columns");
    for _ in 0..2 {
        let card = ui
            .add_sized(columns, Card::new(), Sizing::grow(1))
            .expect("card");
        for n in 0..4 {
            let row = Row::new(Label::key("demo.night")).sublabel(Label::key("demo.night.sub"));
            let row = ui.add(card, row).expect("row");
            ui.add(row, Switch::new(Label::key("demo.night"), n % 2 == 0))
                .expect("switch");
        }
    }
    screen.apply
}

fn average_ms(mut frame: impl FnMut()) -> f64 {
    frame();
    let start = Instant::now();
    for _ in 0..FRAMES {
        frame();
    }
    start.elapsed().as_secs_f64() * 1000.0 / f64::from(FRAMES)
}

#[test]
#[ignore = "measures wall time; run with --release --ignored --nocapture"]
fn full_hd_frame_time() {
    for lang in [Lang::Ar, Lang::En] {
        let mut ui = common::ui(lang, ThemeKind::Dark, FULL_HD);
        let button = typical_screen(&mut ui);
        let mut canvas = Canvas::new(1920, 1080, PixelFormat::Bgrx8).expect("canvas");
        let cold = Instant::now();
        ui.paint(&mut canvas);
        let first = cold.elapsed().as_secs_f64() * 1000.0;
        let full = average_ms(|| ui.paint_all(&mut canvas));
        let pos = center(&mut ui, button);
        let mut inside = false;
        let partial = average_ms(|| {
            inside = !inside;
            let event = if inside {
                Event::PointerMove { pos }
            } else {
                Event::PointerLeave
            };
            ui.handle(event);
            ui.paint(&mut canvas);
        });
        let relayout = average_ms(|| {
            ui.invalidate_layout();
            ui.paint(&mut canvas);
        });
        println!(
            "{} 1920x1080 BGRX, {} nodes: first frame {first:.2} ms, full repaint {full:.2} ms, \
             hover repaint {partial:.3} ms, layout + full repaint {relayout:.2} ms",
            lang.code(),
            ui.node_count()
        );
    }
}

#[test]
#[ignore = "measures wall time; run with --release --ignored --nocapture"]
fn gallery_frame_time() {
    use common::gallery::{file_menu, gallery};
    for lang in [Lang::Ar, Lang::En] {
        let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(1920.0, 420.0));
        let g = gallery(&mut ui).expect("gallery builds");
        let mut canvas = Canvas::new(1920, 420, PixelFormat::Bgrx8).expect("canvas");
        ui.paint(&mut canvas);
        let full = average_ms(|| ui.paint_all(&mut canvas));
        let mut down = false;
        let scroll = average_ms(|| {
            down = !down;
            let offset = if down { 96.0 } else { 0.0 };
            ui.set_scroll_offset(g.scroll, offset).expect("scroll");
            ui.paint(&mut canvas);
        });
        ui.open_dialog(g.modal.dialog).expect("dialog");
        let dialog = average_ms(|| ui.paint_all(&mut canvas));
        ui.close_dialog(g.modal.dialog).expect("dialog");
        let menu = average_ms(|| {
            ui.open_menu(g.menu_button, file_menu()).expect("menu");
            ui.paint(&mut canvas);
            ui.close_menu();
            ui.paint(&mut canvas);
        });
        println!(
            "{} gallery 1920x420 BGRX, {} nodes: full repaint {full:.2} ms, scroll step \
             {scroll:.2} ms, full repaint under a dialog {dialog:.2} ms, menu open + close \
             {menu:.2} ms",
            lang.code(),
            ui.node_count()
        );
    }
}
