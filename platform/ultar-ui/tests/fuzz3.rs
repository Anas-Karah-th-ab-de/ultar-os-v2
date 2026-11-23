//! Fixed-seed fuzzer over the wave-3 gallery: random pointer, touch, key, wheel and clock
//! events plus state changes from the application. After every step the incremental frame
//! must equal a full repaint, every changed pixel must lie inside the reported damage, and
//! the invariants of focus trap, scroll offsets and menus must hold. Runs alternate between
//! Arabic and English, pointer and touch targets, and scale 1, 1.25 and 1.5.

mod common;

use common::gallery::{file_menu, gallery, Gallery};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::{Direction, Lang, Point, Size};

const SURFACE: Size = Size::new(560.0, 420.0);

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }

    fn point(&mut self) -> Point {
        // Mostly inside, sometimes outside or absurd.
        match self.below(40) {
            0 => Point::new(-20.0, self.below(500) as f32),
            1 => Point::new(1.0e9, -1.0e9),
            2 => Point::new(f32::NAN, 10.0),
            _ => Point::new(self.below(560) as f32, self.below(420) as f32),
        }
    }
}

const KEYS: [Key; 17] = [
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
    Key::PageUp,
    Key::PageDown,
    Key::Menu,
    Key::Backspace,
    Key::Delete,
    Key::Char('س'),
    Key::Char('a'),
];

fn input(ui: &mut Ui, rng: &mut Rng, last: &mut Point) -> String {
    let pos = if rng.below(3) == 0 {
        *last
    } else {
        rng.point()
    };
    *last = pos;
    let event = match rng.below(13) {
        0 | 1 => Event::PointerMove { pos },
        2 => Event::PointerDown {
            pos,
            button: PointerButton::Primary,
        },
        3 => Event::PointerUp {
            pos,
            button: PointerButton::Primary,
        },
        4 => Event::PointerDown {
            pos,
            button: PointerButton::Secondary,
        },
        5 => Event::TouchDown {
            id: u64::from(rng.below(2)),
            pos,
        },
        6 => Event::TouchMove {
            id: u64::from(rng.below(2)),
            pos,
        },
        7 => Event::TouchUp {
            id: u64::from(rng.below(2)),
            pos,
        },
        8 => Event::TouchCancel {
            id: u64::from(rng.below(2)),
        },
        9 => Event::Scroll {
            pos,
            lines: rng.below(7) as f32 - 3.0,
        },
        10 => Event::PointerLeave,
        _ => {
            let key = KEYS[rng.below(KEYS.len() as u32) as usize];
            let modifiers = if rng.below(5) == 0 {
                Modifiers::SHIFT
            } else {
                Modifiers::NONE
            };
            Event::Key { key, modifiers }
        }
    };
    ui.handle(event);
    format!("{event:?}")
}

fn application(ui: &mut Ui, g: &Gallery, rng: &mut Rng, clock: &mut u64) -> String {
    let choice = rng.below(16);
    let flag = rng.below(2) == 0;
    let result = match choice {
        0 => ui.open_dialog(g.modal.dialog),
        1 => ui.close_dialog(g.modal.dialog),
        2 => ui.open_dialog(g.modal.sheet),
        3 => ui.close_dialog(g.modal.sheet),
        4 => ui.open_menu(g.menu_button, file_menu()),
        5 => ui.open_context_menu(g.list, rng.point(), file_menu()),
        6 => {
            ui.close_menu();
            Ok(())
        }
        7 => ui.set_scroll_offset(g.scroll, rng.below(900) as f32 - 100.0),
        8 => ui.set_checked(g.checkbox, flag),
        9 => ui.set_selected(g.tabs, rng.below(3) as usize),
        10 => ui.set_progress(g.progress, flag.then_some(rng.below(120) as f32 / 100.0)),
        11 => ui.set_item_selected(g.items[rng.below(3) as usize], flag),
        12 => ui.set_disabled(g.list, flag),
        13 => ui.set_text(g.search, if flag { "بحث abc" } else { "" }),
        14 => {
            ui.set_direction(if flag { Direction::Rtl } else { Direction::Ltr });
            Ok(())
        }
        _ => {
            *clock += u64::from(rng.below(700));
            ui.advance_to(*clock);
            Ok(())
        }
    };
    result.unwrap_or_else(|e| panic!("application call {choice}: {e}"));
    format!("app {choice} {flag}")
}

fn invariants(ui: &mut Ui, g: &Gallery) -> Result<(), String> {
    let offset = ui.scroll_offset(g.scroll).map_err(|e| e.to_string())?;
    if !(offset.is_finite() && offset >= 0.0) {
        return Err(format!("scroll offset {offset}"));
    }
    let order = ui.focus_order();
    if let Some(focus) = ui.focused() {
        if ui.open_dialog_node().is_some() && !order.contains(&focus) {
            return Err(format!("focus {focus:?} escaped the open dialog"));
        }
        if ui.is_disabled(focus) {
            return Err(format!("focus on disabled {focus:?}"));
        }
    }
    if let Some(top) = ui.open_dialog_node() {
        let mut inside = order.iter().map(|id| {
            let mut at = *id;
            while let Some(parent) = ui.parent(at) {
                at = parent;
            }
            at == top
        });
        if !inside.all(|ok| ok) {
            return Err("a widget outside the open dialog is focusable".into());
        }
    }
    if let Some((path, _)) = ui.menu_position() {
        if (0..=path.len()).any(|level| ui.menu_rect(level).is_none()) {
            return Err("an open menu level has no panel".into());
        }
    }
    for node in ui.a11y_tree() {
        if node.rect.w.is_nan() || node.rect.h.is_nan() {
            return Err(format!("{:?} has a NaN rectangle", node.id));
        }
    }
    Ok(())
}

/// Where two frames differ: pixel count and bounding box; `None` when they are equal.
fn difference(a: &Canvas, b: &Canvas) -> Option<String> {
    if a.data() == b.data() {
        return None;
    }
    let w = a.width();
    let wrong: Vec<(u32, u32)> = a
        .data()
        .chunks_exact(4)
        .zip(b.data().chunks_exact(4))
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .map(|(i, _)| (i as u32 % w, i as u32 / w))
        .collect();
    let xs = wrong.iter().map(|p| p.0);
    let ys = wrong.iter().map(|p| p.1);
    Some(format!(
        "{} px in ({:?},{:?})-({:?},{:?})",
        wrong.len(),
        xs.clone().min(),
        ys.clone().min(),
        xs.max(),
        ys.max()
    ))
}

fn run(seed: u64, lang: Lang, mode: InputMode, steps: usize) -> Result<(), String> {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let scale = [1.0, 1.25, 1.5][(seed % 5 % 3) as usize];
    let mut ui = common::ui(lang, ThemeKind::Dark, SURFACE);
    ui.set_input_mode(mode);
    ui.set_scale(scale).map_err(|e| e.to_string())?;
    let g = gallery(&mut ui).map_err(|e| e.to_string())?;
    let (w, h) = (
        (SURFACE.w * scale).ceil() as u32,
        (SURFACE.h * scale).ceil() as u32,
    );
    let mut canvas = Canvas::new(w, h, PixelFormat::Rgba8).map_err(|e| e.to_string())?;
    let mut full = Canvas::new(w, h, PixelFormat::Rgba8).map_err(|e| e.to_string())?;
    ui.paint(&mut canvas);
    let (mut last, mut clock) = (Point::new(0.0, 0.0), 0u64);
    let mut log: Vec<String> = Vec::new();
    for step in 0..steps {
        let op = if rng.below(4) == 0 {
            application(&mut ui, &g, &mut rng, &mut clock)
        } else {
            input(&mut ui, &mut rng, &mut last)
        };
        log.push(op);
        let tail = || log[log.len().saturating_sub(8)..].join(" | ");
        let before = canvas.to_rgba();
        canvas.take_damage();
        ui.paint(&mut canvas);
        let damage = canvas.take_damage().bounds();
        let after = canvas.to_rgba();
        for (i, (a, b)) in before
            .chunks_exact(4)
            .zip(after.chunks_exact(4))
            .enumerate()
        {
            let (x, y) = ((i as u32 % w) as i32, (i as u32 / w) as i32);
            let inside = x >= damage.x0 && x < damage.x1 && y >= damage.y0 && y < damage.y1;
            if a != b && !inside {
                return Err(format!("seed {seed} {lang:?} step {step}: pixel ({x},{y}) changed outside the damage {damage:?}; {}", tail()));
            }
        }
        ui.paint_all(&mut full);
        if let Some(diff) = difference(&canvas, &full) {
            return Err(format!(
                "seed {seed} {lang:?} {mode:?} x{scale} step {step}: incremental != full, {diff}; {}",
                tail()
            ));
        }
        canvas.take_damage();
        invariants(&mut ui, &g)
            .map_err(|e| format!("seed {seed} {lang:?} step {step}: {e}; {}", tail()))?;
    }
    Ok(())
}

fn campaign(seeds: std::ops::RangeInclusive<u64>, steps: usize) {
    let mut failures = Vec::new();
    for seed in seeds {
        let lang = if seed % 2 == 0 { Lang::Ar } else { Lang::En };
        let mode = if seed % 3 == 0 {
            InputMode::Touch
        } else {
            InputMode::Pointer
        };
        if let Err(e) = run(seed, lang, mode, steps) {
            failures.push(e);
        }
    }
    assert!(
        failures.is_empty(),
        "{} failing runs:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn fuzz_gallery_short() {
    campaign(1..=8, 60);
}

/// The long campaign: `cargo test --release --test fuzz3 -- --ignored`.
#[test]
#[ignore = "long: 300 runs of 250 steps"]
fn fuzz_gallery_long() {
    campaign(100..=399, 250);
}
