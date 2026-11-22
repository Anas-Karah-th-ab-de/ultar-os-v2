//! Review: random event/state sequences; after every step the incremental frame must equal
//! a full repaint and every changed pixel must lie inside the canvas damage.
mod common;

use common::display_settings;
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::widgets::Label;
use ultar_ui::{Direction, Lang, Point, Size};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
    fn f(&mut self, max: f32) -> f32 {
        (self.next() % 100_000) as f32 / 100_000.0 * max
    }
}

fn run(
    seed: u64,
    lang: Lang,
    scale: f32,
    mode: InputMode,
    steps: usize,
    skip: &[u32],
) -> Result<(), String> {
    let size = Size::new(720.0, 760.0);
    let mut ui = common::ui(lang, ThemeKind::Dark, size);
    ui.set_scale(scale).unwrap();
    ui.set_input_mode(mode);
    let s = display_settings(&mut ui).unwrap();
    let (w, h) = (
        (size.w * scale).ceil() as u32,
        (size.h * scale).ceil() as u32,
    );
    let mut canvas = Canvas::new(w, h, PixelFormat::Rgba8).unwrap();
    let mut full = Canvas::new(w, h, PixelFormat::Rgba8).unwrap();
    ui.paint(&mut canvas);
    canvas.take_damage();
    let mut rng = Rng(seed);
    let nodes = [
        s.primary,
        s.resolution,
        s.scale,
        s.refresh,
        s.orientation,
        s.night,
        s.brightness,
        s.name,
        s.detect,
        s.revert,
        s.apply,
    ];
    let keys = [
        Key::Tab,
        Key::Enter,
        Key::Space,
        Key::Escape,
        Key::ArrowLeft,
        Key::ArrowRight,
        Key::ArrowUp,
        Key::ArrowDown,
        Key::Home,
        Key::End,
        Key::Backspace,
        Key::Delete,
        Key::Char('ل'),
        Key::Char('ا'),
        Key::Char('a'),
        Key::Char('ّ'),
    ];
    let mut now = 0u64;
    let mut log: Vec<String> = Vec::new();
    for step in 0..steps {
        let mut op = rng.below(22);
        while skip.contains(&op) {
            op = rng.below(22);
        }
        let pos = if rng.below(3) == 0 {
            Point::new(rng.f(size.w), rng.f(size.h))
        } else {
            let n = nodes[rng.below(nodes.len() as u32) as usize];
            let r = ui.rect(n).unwrap();
            Point::new(r.x + rng.f(r.w), r.y + rng.f(r.h))
        };
        let node = nodes[rng.below(nodes.len() as u32) as usize];
        let desc = match op {
            0 | 1 => {
                ui.handle(Event::PointerMove { pos });
                format!("move {pos:?}")
            }
            2 => {
                ui.handle(Event::PointerDown {
                    pos,
                    button: PointerButton::Primary,
                });
                format!("down {pos:?}")
            }
            3 => {
                ui.handle(Event::PointerUp {
                    pos,
                    button: PointerButton::Primary,
                });
                format!("up {pos:?}")
            }
            4 => {
                ui.handle(Event::PointerLeave);
                "leave".into()
            }
            5..=7 => {
                let key = keys[rng.below(keys.len() as u32) as usize];
                let modifiers = if rng.below(5) == 0 {
                    Modifiers::SHIFT
                } else {
                    Modifiers::NONE
                };
                ui.handle(Event::Key { key, modifiers });
                format!("key {key:?} {modifiers:?}")
            }
            8 => {
                ui.handle(Event::TouchDown { id: 1, pos });
                format!("tdown {pos:?}")
            }
            9 => {
                ui.handle(Event::TouchMove { id: 1, pos });
                format!("tmove {pos:?}")
            }
            10 => {
                ui.handle(Event::TouchUp { id: 1, pos });
                format!("tup {pos:?}")
            }
            11 => {
                ui.handle(Event::TouchCancel { id: 1 });
                "tcancel".into()
            }
            12 => {
                let l = rng.f(6.0) - 3.0;
                ui.handle(Event::Scroll { pos, lines: l });
                format!("scroll {l}")
            }
            13 => {
                now += u64::from(rng.below(90));
                ui.advance_to(now);
                format!("advance {now}")
            }
            14 => {
                let on = rng.below(2) == 0;
                let _ = ui.set_on(node, on);
                format!("set_on {node:?} {on}")
            }
            15 => {
                let v = rng.f(100.0);
                let _ = ui.set_value(node, v);
                format!("set_value {node:?} {v}")
            }
            16 => {
                let i = rng.below(3) as usize;
                let _ = ui.set_selected(node, i);
                format!("set_selected {node:?} {i}")
            }
            17 => {
                let t = ["", "مرحبا", "hello world", "لا إله"][rng.below(4) as usize];
                let _ = ui.set_text(node, t);
                format!("set_text {node:?} {t}")
            }
            18 => {
                let d = rng.below(2) == 0;
                let _ = ui.set_disabled(node, d);
                format!("set_disabled {node:?} {d}")
            }
            19 => {
                let l = rng.below(2) == 0;
                let _ = ui.set_loading(node, l);
                format!("set_loading {node:?} {l}")
            }
            20 => {
                let e = (rng.below(2) == 0).then(|| Label::key("demo.device.error"));
                let _ = ui.set_error(node, e);
                format!("set_error {node:?}")
            }
            _ => {
                let d = if rng.below(2) == 0 {
                    Direction::Rtl
                } else {
                    Direction::Ltr
                };
                ui.set_direction(d);
                format!("set_direction {d:?}")
            }
        };
        log.push(desc);
        let before = canvas.to_rgba();
        ui.paint(&mut canvas);
        let damage = canvas.take_damage();
        let after = canvas.to_rgba();
        // every changed pixel inside damage
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let i = (y as usize * w as usize + x as usize) * 4;
                if before[i..i + 4] != after[i..i + 4] {
                    let inside = damage
                        .rects()
                        .iter()
                        .any(|r| x >= r.x0 && x < r.x1 && y >= r.y0 && y < r.y1);
                    if !inside {
                        return Err(format!("seed {seed} step {step}: pixel ({x},{y}) changed outside damage; last ops {:?}", &log[log.len().saturating_sub(6)..]));
                    }
                }
            }
        }
        ui.paint_all(&mut full);
        full.take_damage();
        if canvas.data() != full.data() {
            let (mut n, mut bx0, mut by0, mut bx1, mut by1) = (0usize, i32::MAX, i32::MAX, 0, 0);
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    let i = (y as usize * w as usize + x as usize) * 4;
                    if canvas.data()[i..i + 4] != full.data()[i..i + 4] {
                        n += 1;
                        bx0 = bx0.min(x);
                        by0 = by0.min(y);
                        bx1 = bx1.max(x);
                        by1 = by1.max(y);
                    }
                }
            }
            return Err(format!("seed {seed} step {step} lang {lang:?} scale {scale}: incremental != full, {n} px in ({bx0},{by0})-({bx1},{by1}); last ops {:?}", &log[log.len().saturating_sub(8)..]));
        }
    }
    Ok(())
}

#[test]
#[ignore = "long pixel-diff fuzz run (minutes in debug builds): cargo test -p ultar-ui --test review_fuzz -- --ignored"]
fn fuzz_incremental_equals_full() {
    let mut failures = Vec::new();
    let skip: Vec<u32> = std::env::var("SKIP")
        .ok()
        .map(|s| s.split(',').filter_map(|v| v.parse().ok()).collect())
        .unwrap_or_default();
    let seeds: u64 = std::env::var("SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    for seed in 1..=seeds {
        for (lang, scale, mode) in [
            (Lang::Ar, 1.0, InputMode::Pointer),
            (Lang::En, 1.25, InputMode::Pointer),
            (Lang::Ar, 1.5, InputMode::Touch),
        ] {
            if let Err(e) = run(seed, lang, scale, mode, 120, &skip) {
                failures.push(e);
            }
        }
    }
    for f in &failures {
        eprintln!("FAIL: {f}");
    }
    assert!(failures.is_empty(), "{} failing runs", failures.len());
}
