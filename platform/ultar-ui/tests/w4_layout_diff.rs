//! Review round 4 (second reviewer): differential fuzzer for the incremental layout of wave 4
//! (measured sizes cached per node and width, `invalidate_node` re-measures one branch).
//! Two identical `Ui`s get the same random tree and the same random operations; `inc` lays
//! out and paints incrementally, `full` re-measures everything and repaints the whole
//! surface every step. Rectangles, accessibility nodes and pixels must be equal.
//! Seeds: `W4_SEEDS` (default 12), steps per seed `W4_STEPS` (default 50).
//! Target path: platform/ultar-ui/tests/w4_layout_diff.rs

mod common;

use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Sizing};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{
    Button, Card, Checkbox, Dialog, EmptyState, InfoBar, Label, List, ListItem, Progress,
    RadioGroup, Row, Scroll, Select, Slider, Switch, Table, Tabs, Text, TextField, Tone, Tree,
    TreeItem, Widget,
};
use ultar_ui::{Direction, Lang, Point, Size};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

fn data(rng: &mut Rng) -> Label {
    let words = [
        "ملف",
        "file",
        "إعدادات الشاشة",
        "x",
        "a longer label with words",
        "٣٤",
    ];
    let n = 1 + rng.below(4);
    let text: Vec<&str> = (0..n).map(|_| rng.pick(&words)).collect();
    Label::data(&text.join(" "))
}

fn labels(rng: &mut Rng, n: usize) -> Vec<Label> {
    (0..n).map(|_| data(rng)).collect()
}

/// A random widget; `true` when it is a container that takes children.
fn widget(rng: &mut Rng) -> (Widget, bool) {
    let l = data(rng);
    match rng.below(20) {
        0 => (Flex::row().into(), true),
        1 => (Flex::column().into(), true),
        2 => (Card::padded().into(), true),
        3 => (List::new().selectable().into(), true),
        4 => (Button::new(l).into(), false),
        5 => (Text::new(l).into(), false),
        6 => (Checkbox::new(l, rng.below(2) == 0).into(), false),
        7 => (Switch::new(l, false).into(), false),
        8 => (Select::new(l, labels(rng, 3)).into(), false),
        9 => (Slider::new(l, 0.0, 10.0, 1.0).into(), false),
        10 => (TextField::new(l).into(), false),
        11 => (Progress::new(l).into(), false),
        12 => (RadioGroup::new(l, labels(rng, 3)).into(), false),
        13 => (Tabs::new(l, labels(rng, 3)).into(), false),
        14 => (ListItem::new(l).sublabel(data(rng)).into(), false),
        15 => (Row::new(l).into(), false),
        16 => (InfoBar::new(Tone::Info, l).into(), false),
        17 => (EmptyState::new(IconId::Gear, l).into(), false),
        18 => {
            let t = Table::new(l, labels(rng, 3)).rows(vec![labels(rng, 3), labels(rng, 3)]);
            (t.into(), false)
        }
        _ => {
            let items = vec![
                TreeItem::new(data(rng), 0),
                TreeItem::new(data(rng), 1),
                TreeItem::new(data(rng), 1),
                TreeItem::new(data(rng), 0),
            ];
            (Tree::new(l, items).into(), false)
        }
    }
}

struct Pair {
    inc: Ui,
    full: Ui,
    nodes: Vec<NodeId>,
    containers: Vec<NodeId>,
    dialog: NodeId,
}

fn both(p: &mut Pair, f: &dyn Fn(&mut Ui)) {
    f(&mut p.inc);
    f(&mut p.full);
}

fn build(seed: u64) -> (Pair, Rng) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let size = Size::new(360.0, 320.0);
    let mut inc = common::ui(Lang::Ar, ThemeKind::Dark, size);
    let mut full = common::ui(Lang::Ar, ThemeKind::Dark, size);
    let root = inc.root();
    let scroll = inc.add_sized(root, Scroll::new(), Sizing::grow(1)).unwrap();
    full.add_sized(root, Scroll::new(), Sizing::grow(1))
        .unwrap();
    let dialog = inc.add(root, Dialog::new(Label::data("d"))).unwrap();
    full.add(root, Dialog::new(Label::data("d"))).unwrap();
    let mut p = Pair {
        inc,
        full,
        nodes: vec![root, scroll],
        containers: vec![scroll, dialog],
        dialog,
    };
    for _ in 0..12 {
        add_random(&mut p, &mut rng);
    }
    (p, rng)
}

fn add_random(p: &mut Pair, rng: &mut Rng) {
    let (w, container) = widget(rng);
    let parent = rng.pick(&p.containers);
    let host = rng.pick(&p.nodes);
    let hosts =
        p.inc.widget(host).is_some_and(|w| w.hosts_one_child()) && p.inc.children(host).is_empty();
    let parent = if hosts && !container && rng.below(3) == 0 {
        host
    } else {
        parent
    };
    let (a, b) = (p.inc.add(parent, w.clone()), p.full.add(parent, w));
    assert_eq!(a, b, "add diverged");
    if let Ok(id) = a {
        p.nodes.push(id);
        if container {
            p.containers.push(id);
        }
    }
}

fn key(ui: &mut Ui, key: Key) {
    ui.handle(Event::Key {
        key,
        modifiers: Modifiers::NONE,
    });
}

fn click(ui: &mut Ui, pos: Point) {
    let button = PointerButton::Primary;
    ui.handle(Event::PointerMove { pos });
    ui.handle(Event::PointerDown { pos, button });
    let r = ui.handle(Event::PointerUp { pos, button });
    if std::env::var("W4_TRACE").is_ok() {
        eprintln!(
            "click {pos:?}: hover {:?} actions {:?}",
            ui.hovered(),
            r.actions
        );
    }
}

/// One random operation applied to both `Ui`s; returns its name.
fn operate(p: &mut Pair, rng: &mut Rng) -> String {
    let id = rng.pick(&p.nodes);
    let label = data(rng);
    let flag = rng.below(2) == 0;
    let value = rng.below(12) as f32 / 10.0;
    let op = rng.below(19);
    let f: Box<dyn Fn(&mut Ui)> = match op {
        0 => Box::new(move |ui| drop(ui.set_label(id, label.clone()))),
        1 => Box::new(move |ui| drop(ui.set_error(id, flag.then(|| label.clone())))),
        2 => Box::new(move |ui| drop(ui.set_loading(id, flag))),
        3 => Box::new(move |ui| drop(ui.set_disabled(id, flag))),
        4 => Box::new(move |ui| drop(ui.set_progress(id, flag.then_some(value)))),
        5 => Box::new(move |ui| drop(ui.set_selected(id, (value * 3.0) as usize))),
        6 => Box::new(move |ui| drop(ui.set_checked(id, flag))),
        7 => Box::new(move |ui| drop(ui.set_text(id, "نص 12 text"))),
        8 => {
            let w = [200.0, 320.0, 360.0, 500.0][rng.below(4)];
            Box::new(move |ui| drop(ui.set_size(Size::new(w, 320.0))))
        }
        9 => Box::new(move |ui| ui.set_lang(if flag { Lang::En } else { Lang::Ar })),
        10 => {
            Box::new(move |ui| ui.set_direction(if flag { Direction::Ltr } else { Direction::Rtl }))
        }
        11 => Box::new(move |ui| {
            ui.set_input_mode(if flag {
                InputMode::Touch
            } else {
                InputMode::Pointer
            })
        }),
        12 => Box::new(move |ui| drop(ui.set_scale(if flag { 1.0 } else { 1.5 }))),
        13..=17 => input_op(p, rng, op, id, (flag, value)),
        _ => {
            add_random(p, rng);
            return "add".into();
        }
    };
    both(p, &*f);
    // Hover that was dropped (an open list or menu, a disabled node, a flipped direction)
    // comes back only with the next pointer event, while any later layout brings it back
    // too (finding w4-m3). The pointer is moved after every step so that this fuzzer
    // compares layouts, not hover.
    let pos = Point::new(rng.below(360) as f32, rng.below(320) as f32);
    both(p, &move |ui: &mut Ui| {
        drop(ui.handle(Event::PointerMove { pos }))
    });
    let name = format!("op {op} on {id:?} pos {pos:?}");
    if std::env::var("W4_TRACE").is_ok() {
        let hov = |ui: &Ui| ui.hovered();
        eprintln!(
            "{name}: inc hover {:?} full hover {:?}",
            hov(&p.inc),
            hov(&p.full)
        );
    }
    name
}

/// Keyboard, pointer, dialog and scroll operations of [`operate`].
fn input_op(
    p: &Pair,
    rng: &mut Rng,
    op: usize,
    id: NodeId,
    (flag, value): (bool, f32),
) -> Box<dyn Fn(&mut Ui)> {
    match op {
        13 => {
            let k = rng.pick(&[Key::Tab, Key::ArrowDown, Key::ArrowRight, Key::ArrowLeft]);
            Box::new(move |ui| key(ui, k))
        }
        14 => {
            let k = rng.pick(&[Key::Space, Key::Enter, Key::Escape, Key::Char('a')]);
            Box::new(move |ui| key(ui, k))
        }
        15 => {
            let pos = Point::new(rng.below(360) as f32, rng.below(320) as f32);
            Box::new(move |ui| click(ui, pos))
        }
        16 => {
            let d = p.dialog;
            Box::new(move |ui| {
                drop(if flag {
                    ui.open_dialog(d)
                } else {
                    ui.close_dialog(d)
                })
            })
        }
        _ => Box::new(move |ui| drop(ui.set_scroll_offset(id, value * 200.0))),
    }
}

fn rects(ui: &mut Ui, nodes: &[NodeId]) -> Vec<String> {
    ui.layout();
    nodes
        .iter()
        .map(|n| format!("{n:?} {:?}", ui.rect(*n)))
        .collect()
}

fn a11y(ui: &mut Ui) -> Vec<String> {
    let tr = ui.translator().clone();
    let nodes = ui.a11y_tree();
    nodes
        .iter()
        .map(|n| format!("{:?}/{:?} {:?} {}", n.id, n.part, n.rect, n.describe(&tr)))
        .collect()
}

fn frame(ui: &Ui) -> Canvas {
    let (w, h) = (ui.size().w * ui.env().scale, ui.size().h * ui.env().scale);
    Canvas::new(w.ceil() as u32, h.ceil() as u32, PixelFormat::Bgrx8).unwrap()
}

/// Pixels that differ between two canvases of the same size.
fn differing(a: &Canvas, b: &Canvas) -> usize {
    a.data()
        .chunks(4)
        .zip(b.data().chunks(4))
        .filter(|(x, y)| x != y)
        .count()
}

/// With `W4_DUMP=<dir>` both pictures are written as PPM files.
fn dump(canvas: &Canvas, reference: &Canvas) {
    if let Ok(dir) = std::env::var("W4_DUMP") {
        for (name, c) in [("inc", canvas), ("full", reference)] {
            let mut out = format!("P6 {} {} 255\n", c.width(), c.height()).into_bytes();
            for px in c.data().chunks(4) {
                out.extend_from_slice(&[px[2], px[1], px[0]]);
            }
            std::fs::write(format!("{dir}/{name}.ppm"), out).unwrap();
        }
    }
}

/// First difference between the two `Ui`s after a step, if any.
fn compare(p: &mut Pair, canvas: &mut Canvas) -> Option<String> {
    p.full.invalidate_layout();
    let nodes = p.nodes.clone();
    let (a, b) = (rects(&mut p.inc, &nodes), rects(&mut p.full, &nodes));
    if let Some((x, y)) = a.iter().zip(&b).find(|(x, y)| x != y) {
        return Some(format!("rect: incremental {x} / full {y}"));
    }
    let (a, b) = (a11y(&mut p.inc), a11y(&mut p.full));
    if a != b {
        let first = a.iter().zip(&b).find(|(x, y)| x != y);
        return Some(format!("a11y ({} vs {}): {first:?}", a.len(), b.len()));
    }
    let reference_size = frame(&p.full);
    if (canvas.width(), canvas.height()) != (reference_size.width(), reference_size.height()) {
        *canvas = frame(&p.inc);
        p.inc.paint_all(canvas);
    }
    p.inc.paint(canvas);
    let mut reference = reference_size;
    p.full.paint_all(&mut reference);
    let diff = differing(canvas, &reference);
    if diff == 0 {
        return None;
    }
    let (w, mut b) = (canvas.width() as usize, [usize::MAX, usize::MAX, 0, 0]);
    for (i, (x, y)) in canvas
        .data()
        .chunks(4)
        .zip(reference.data().chunks(4))
        .enumerate()
    {
        if x != y {
            let (px, py) = (i % w, i / w);
            b = [b[0].min(px), b[1].min(py), b[2].max(px), b[3].max(py)];
        }
    }
    dump(canvas, &reference);
    let nodes = p.nodes.clone();
    let state = |ui: &Ui| {
        let open: Vec<_> = nodes.iter().filter(|n| ui.is_open(**n)).collect();
        let w = ui
            .hovered()
            .and_then(|h| ui.widget(h))
            .map(|w| format!("{w:?}").chars().take(20).collect::<String>());
        format!(
            "hover {:?} {w:?} focus {:?} open {open:?} menu {:?} dialog {:?}",
            ui.hovered(),
            ui.focused(),
            ui.menu_owner(),
            ui.open_dialog_node()
        )
    };
    Some(format!(
        "{diff} pixels differ in {b:?}; inc {} / full {}",
        state(&p.inc),
        state(&p.full)
    ))
}

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[test]
fn ld01_incremental_layout_equals_full_layout() {
    let mut failures = Vec::new();
    let first = env("W4_FIRST", 1);
    for seed in first..first + env("W4_SEEDS", 12) {
        let (mut p, mut rng) = build(seed);
        let mut canvas = frame(&p.inc);
        p.inc.paint(&mut canvas);
        let mut log = Vec::new();
        for step in 0..env("W4_STEPS", 50) {
            log.push(operate(&mut p, &mut rng));
            if let Some(problem) = compare(&mut p, &mut canvas) {
                let tail = log[log.len().saturating_sub(4)..].join(", ");
                failures.push(format!("seed {seed} step {step}: {problem} after [{tail}]"));
                break;
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
