//! Review round 3, robustness: a fixed-seed fuzzer that is harsher than `fuzz3` – besides
//! input events it resizes the surface (also to nothing and to absurd sizes), changes scale,
//! language, theme, input mode and motion setting in the middle of a gesture, jumps the
//! clock, and passes hostile values (NaN, infinity, out-of-range indices, control and
//! bidi characters) to every setter. After every step: no panic, the incremental frame
//! equals a full repaint, and the invariants of focus and scrolling hold.
//! Target path: platform/ultar-ui/tests/r3_fuzz.rs
//! More seeds: `R3_SEEDS=200 cargo test --release --test r3_fuzz -- --ignored`.

mod common;

use common::gallery::{file_menu, gallery, Gallery};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::{Lang, Point, Size};

const CANVAS: (u32, u32) = (640, 480);

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

    fn pick<T: Copy>(&mut self, from: &[T]) -> T {
        from[self.below(from.len() as u32) as usize]
    }

    fn number(&mut self) -> f32 {
        const ODD: [f32; 8] = [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -0.0,
            f32::MAX,
            f32::MIN,
            1.0e-30,
            -1.0e9,
        ];
        if self.below(6) == 0 {
            self.pick(&ODD)
        } else {
            self.below(1400) as f32 / 2.0 - 60.0
        }
    }

    fn point(&mut self) -> Point {
        if self.below(25) == 0 {
            Point::new(self.number(), self.number())
        } else {
            Point::new(self.below(640) as f32, self.below(480) as f32)
        }
    }
}

const KEYS: [Key; 19] = [
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
    Key::Char('\u{202E}'),
    Key::Char('\u{0651}'),
    Key::Char('a'),
];

const TEXTS: [&str; 7] = [
    "",
    "بحث abc 123",
    "\u{202E}\u{2067}\u{200F}",
    "اللّٰه\u{0651}\u{0651}\u{0651}\u{0651}",
    "a\u{0}b\tc\nd",
    "👨‍👩‍👧‍👦🇸🇾",
    "ﷺ ﷽ ﷲ",
];

fn pointer(rng: &mut Rng, pos: Point) -> Event {
    let button = rng.pick(&[
        PointerButton::Primary,
        PointerButton::Primary,
        PointerButton::Secondary,
        PointerButton::Middle,
    ]);
    match rng.below(5) {
        0 | 1 => Event::PointerMove { pos },
        2 => Event::PointerDown { pos, button },
        3 => Event::PointerUp { pos, button },
        _ => Event::PointerLeave,
    }
}

fn input(ui: &mut Ui, rng: &mut Rng, last: &mut Point) -> String {
    let pos = if rng.below(3) == 0 {
        *last
    } else {
        rng.point()
    };
    *last = pos;
    let id = u64::from(rng.below(3));
    let event = match rng.below(12) {
        0..=3 => pointer(rng, pos),
        4 => Event::TouchDown { id, pos },
        5 => Event::TouchMove { id, pos },
        6 => Event::TouchUp { id, pos },
        7 => Event::TouchCancel { id },
        8 => Event::Scroll {
            pos,
            lines: rng.number() / 100.0,
        },
        _ => {
            let key = rng.pick(&KEYS);
            let modifiers = Modifiers {
                shift: rng.below(5) == 0,
                ctrl: rng.below(12) == 0,
                alt: rng.below(20) == 0,
                meta: false,
            };
            Event::Key { key, modifiers }
        }
    };
    ui.handle(event);
    format!("{event:?}")
}

fn environment(ui: &mut Ui, rng: &mut Rng, clock: &mut u64) -> String {
    let choice = rng.below(9);
    match choice {
        0 => {
            let sizes = [0.0, 1.0, 37.0, 200.0, 360.0, 599.0, 600.0, 640.0, 4000.0];
            let size = Size::new(rng.pick(&sizes), rng.pick(&sizes).min(480.0));
            let _ = ui.set_size(size);
        }
        1 => {
            let _ = ui.set_size(Size::new(rng.number(), rng.number()));
        }
        2 => {
            let _ = ui.set_scale(rng.pick(&[1.0, 1.25, 1.5, 2.0, 3.0, 0.0, f32::NAN, 1.0e9]));
        }
        3 => ui.set_lang(rng.pick(&[Lang::Ar, Lang::En, Lang::De])),
        4 => ui.set_theme(rng.pick(&ThemeKind::ALL)),
        5 => ui.set_input_mode(rng.pick(&[InputMode::Pointer, InputMode::Touch])),
        6 => ui.set_reduce_motion(rng.below(2) == 0),
        7 => *clock = clock.saturating_add(u64::from(rng.below(900))),
        _ => *clock = rng.pick(&[0, 1, *clock, u64::MAX / 2, u64::MAX]),
    }
    ui.advance_to(*clock);
    let (size, scale) = (ui.size(), ui.env().scale);
    format!("environment {choice}: size {size:?}, scale {scale}, clock {clock}")
}

fn application(ui: &mut Ui, g: &Gallery, rng: &mut Rng) -> String {
    let choice = rng.below(18);
    let flag = rng.below(2) == 0;
    let index = rng.pick(&[0usize, 1, 2, 3, 99, usize::MAX]);
    let item = g.items[rng.below(3) as usize];
    let any = [
        g.search,
        g.tabs,
        g.scroll,
        g.checkbox,
        g.radio,
        g.list,
        item,
        g.menu_button,
    ];
    let target = rng.pick(&any);
    let result = match choice {
        0 => ui.open_dialog(rng.pick(&[g.modal.dialog, g.modal.sheet, g.list])),
        1 => ui.close_dialog(rng.pick(&[g.modal.dialog, g.modal.sheet])),
        2 => ui.open_menu(target, file_menu()),
        3 => ui.open_context_menu(target, rng.point(), file_menu()),
        4 => ui.set_scroll_offset(g.scroll, rng.number()),
        5 => ui.set_checked(g.checkbox, flag),
        6 => ui.set_selected(rng.pick(&[g.tabs, g.radio]), index),
        7 => ui.set_progress(g.progress, flag.then_some(rng.number())),
        8 => ui.set_item_selected(item, flag),
        9 => ui.set_disabled(target, flag),
        10 => ui.set_text(g.search, rng.pick(&TEXTS)),
        11 => ui.scroll_into_view(target),
        12 => ui.set_loading(target, flag),
        13 => ui.set_tooltip(
            target,
            flag.then_some(ultar_ui::widgets::Label::data(rng.pick(&TEXTS))),
        ),
        14 => ui.set_context_menu(target, flag.then_some(file_menu())),
        15 => ui.set_label(target, ultar_ui::widgets::Label::data(rng.pick(&TEXTS))),
        16 => ui.set_on(g.item_switch, flag),
        _ => {
            ui.close_menu();
            Ok(())
        }
    };
    format!("application {choice} on {target:?} -> {}", result.is_ok())
}

/// `true` for a surface size a compositor can really send.
fn sane(size: Size) -> bool {
    let ok = |v: f32| v.is_finite() && (0.0..=16384.0).contains(&v);
    ok(size.w) && ok(size.h)
}

fn check_frame(ui: &mut Ui, frame: &mut Canvas, context: &dyn Fn() -> String) {
    ui.paint(frame);
    let mut full = Canvas::new(CANVAS.0, CANVAS.1, PixelFormat::Rgba8).unwrap();
    full.blit(frame, frame.bounds(), 0, 0);
    ui.paint_all(&mut full);
    if full.data() != frame.data() {
        let (w, h) = (CANVAS.0 as i32, CANVAS.1 as i32);
        let points = (0..h).flat_map(|y| (0..w).map(move |x| (x, y)));
        let stale: Vec<_> = points
            .filter(|(x, y)| full.pixel(*x, *y) != frame.pixel(*x, *y))
            .collect();
        let (first, last) = (stale[0], stale[stale.len() - 1]);
        let count = stale.len();
        let size = ui.size();
        panic!(
            "incremental frame differs from a full repaint: {count} pixels from {first:?} \
             to {last:?}, surface {size:?}; {}",
            context()
        );
    }
}

fn check(ui: &mut Ui, g: &Gallery, frame: &mut Canvas, trail: &[String], seed: u64) {
    let context = || format!("seed {seed}, last steps:\n{}", trail.join("\n"));
    if !sane(ui.size()) {
        // An absurd size is a finding of its own (`f03`); here it must only not panic.
        ui.paint_all(frame);
        let _ = ui.a11y_tree();
        return;
    }
    check_frame(ui, frame, &context);
    let offset = ui.scroll_offset(g.scroll).unwrap();
    assert!(
        offset.is_finite() && offset >= 0.0,
        "offset {offset}; {}",
        context()
    );
    if let Some(focus) = ui.focused() {
        assert!(
            !ui.is_disabled(focus),
            "focus on a disabled node; {}",
            context()
        );
        if let Some(dialog) = ui.open_dialog_node() {
            let mut node = Some(focus);
            while node.is_some() && node != Some(dialog) {
                node = node.and_then(|n| ui.parent(n));
            }
            assert!(node.is_some(), "focus behind the dialog; {}", context());
        }
    }
    let tree = ui.a11y_tree();
    let focused = tree.iter().filter(|n| n.state.focused).count();
    assert!(focused <= 2, "{focused} focused nodes; {}", context());
    for node in &tree {
        let r = node.rect;
        let finite = r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite();
        assert!(
            finite,
            "{:?} has the rectangle {r:?}; {}",
            node.role,
            context()
        );
    }
}

fn run(seed: u64, steps: u32) {
    let lang = if seed & 1 == 0 { Lang::Ar } else { Lang::En };
    let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(560.0, 420.0));
    let g = gallery(&mut ui).unwrap();
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut frame = Canvas::new(CANVAS.0, CANVAS.1, PixelFormat::Rgba8).unwrap();
    let (mut last, mut clock) = (Point::new(0.0, 0.0), 0u64);
    let mut trail: Vec<String> = Vec::new();
    for _ in 0..steps {
        let what = match rng.below(10) {
            0 => environment(&mut ui, &mut rng, &mut clock),
            1 | 2 => application(&mut ui, &g, &mut rng),
            _ => input(&mut ui, &mut rng, &mut last),
        };
        trail.push(what);
        if trail.len() > 12 {
            trail.remove(0);
        }
        check(&mut ui, &g, &mut frame, &trail, seed);
    }
}

/// Short campaign of the normal test run.
#[test]
fn f01_hostile_sequences_never_panic_or_leave_stale_pixels() {
    for seed in 1..=12 {
        run(seed, 250);
    }
}

/// Long campaign: `R3_SEEDS` seeds (default 60) of 600 steps.
#[test]
#[ignore = "long; run with --release -- --ignored"]
fn f02_long_campaign() {
    let number = |name: &str| std::env::var(name).ok().and_then(|s| s.parse::<u64>().ok());
    let first = number("R3_FIRST").unwrap_or(100);
    for seed in first..first + number("R3_SEEDS").unwrap_or(60) {
        run(seed, 600);
    }
}

/// `Ui::set_size` is fed by the platform (window size from the compositor): a size that is
/// not a number or negative must not reach the layout (`set_scale` already refuses such
/// values with `InvalidScale`).
#[test]
fn f03_a_surface_size_that_is_not_a_number_never_reaches_the_layout() {
    for bad in [f32::NAN, f32::INFINITY, -200.0] {
        let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(560.0, 420.0));
        let g = gallery(&mut ui).unwrap();
        ui.open_dialog(g.modal.dialog).unwrap();
        // Fixed in wave 4: the size is refused with a typed error.
        assert!(ui.set_size(Size::new(bad, 300.0)).is_err());
        for node in ui.a11y_tree() {
            let r = node.rect;
            assert!(
                r.x.is_finite() && r.w.is_finite() && r.w >= 0.0,
                "width {bad}: {:?} has the rectangle {r:?}",
                node.role
            );
        }
    }
}

/// Found by the campaign (seed 527): in a window too narrow for its content, moving the
/// focus leaves stale pixels – the incremental frame is not the frame a full repaint gives.
#[test]
fn f04_focus_moves_in_a_squeezed_window_leave_no_stale_pixels() {
    let mut stale = Vec::new();
    for (lang, width) in [
        (Lang::En, 181.0),
        (Lang::En, 240.0),
        (Lang::Ar, 240.0),
        (Lang::En, 300.0),
    ] {
        let mut ui = common::ui(lang, ThemeKind::Dark, Size::new(width, 110.0));
        gallery(&mut ui).unwrap();
        let mut frame = Canvas::new(CANVAS.0, CANVAS.1, PixelFormat::Rgba8).unwrap();
        ui.paint(&mut frame);
        let tab = Event::Key {
            key: Key::Tab,
            modifiers: Modifiers::NONE,
        };
        for step in 1..=8 {
            ui.handle(tab);
            ui.paint(&mut frame);
            let mut full = Canvas::new(CANVAS.0, CANVAS.1, PixelFormat::Rgba8).unwrap();
            full.blit(&frame, frame.bounds(), 0, 0);
            ui.paint_all(&mut full);
            if full.data() != frame.data() {
                let focus = ui.focused().and_then(|id| ui.rect(id));
                stale.push(format!(
                    "{lang:?} width {width}: Tab {step}, focus at {focus:?}"
                ));
                frame.blit(&full, full.bounds(), 0, 0);
            }
        }
    }
    assert!(stale.is_empty(), "stale pixels:\n{}", stale.join("\n"));
}
