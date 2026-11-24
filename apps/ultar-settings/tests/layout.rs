//! Every Settings page at the owner's VMware screen (1024x768), in a small window (640x480) and at
//! the size the window opens with, in Arabic, English and German: nothing sticks out at the sides of
//! the window (no horizontal clipping: the content reflows to the width, cards shrink or stack), no
//! one-line text is cut off, no text is squeezed narrower than its longest word (text wraps between
//! words, never one character per line) and no text is an untranslated catalog key. `ULTAR_CONFORMANCE_OUT=<dir>` also writes
//! the renders (`layout-<w>x<h>-<lang>-<area>.png`).

mod common;

use common::*;
use ultar_i18n::Lang;
use ultar_settings::areas::AREAS;
use ultar_ui::layout::{CrossAlign, Flex};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{Label, Text, Widget};
use ultar_ui::Size;

/// How far a child may reach over its parent's edge (a card's 16-point padding: full-width rules).
const BLEED: f32 = 16.5;

/// A second, very wide surface on which texts get their natural size: what a text needs on one
/// line, and what its longest word needs.
struct Ruler {
    ui: Ui,
    asked: Vec<(String, NodeId, NodeId)>,
}

impl Ruler {
    fn new(lang: Lang) -> Ruler {
        let env = PaintEnv::new(lang, ThemeKind::Dark);
        let size = Size::new(20_000.0, 400.0);
        let mut ui = Ui::new(env, InputMode::Pointer, size, translator(lang)).expect("ruler");
        ui.set_root_flex(Flex::column().cross(CrossAlign::Start));
        Ruler {
            ui,
            asked: Vec::new(),
        }
    }

    /// Adds `text` and its longest word (same spec) to the ruler.
    fn ask(&mut self, text: &Text, shown: &str) {
        let longest = shown
            .split_whitespace()
            .max_by_key(|w| w.chars().count())
            .unwrap_or("");
        let mut word = text.clone();
        word.label = Label::data(longest);
        let root = self.ui.root();
        let whole = self.ui.add(root, text.clone()).expect("ruler text");
        let word = self.ui.add(root, word).expect("ruler word");
        self.asked.push((shown.to_owned(), whole, word));
    }

    /// (text, natural width, natural height, width of the longest word) of everything asked.
    fn answers(&mut self) -> Vec<(String, f32, f32, f32)> {
        self.ui.layout();
        let ui = &self.ui;
        self.asked
            .iter()
            .map(|(text, whole, word)| {
                let w = ui.logical_rect(*whole).expect("measured");
                let l = ui.logical_rect(*word).expect("measured");
                (text.clone(), w.width, w.height, l.width)
            })
            .collect()
    }
}

/// A text that is a catalog key nobody translated (`settings.power.mode.label`).
fn raw_key(text: &str) -> bool {
    let plain = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b);
    let parts: Vec<&str> = text.split('.').collect();
    let known = [
        "app", "settings", "accountd", "devd", "update", "brand", "desk",
    ];
    parts.len() >= 3
        && known.contains(&parts[0])
        && parts.iter().all(|p| !p.is_empty() && p.bytes().all(plain))
}

/// Every visible text of the page with its rectangle.
fn texts(ui: &Ui, id: NodeId, out: &mut Vec<(Text, String, f32, f32)>) {
    if let (Some(Widget::Text(t)), Some(r)) = (ui.widget(id), ui.logical_rect(id)) {
        let shown = t.label.resolve(ui.translator()).to_string();
        if r.height > 0.0 && !shown.trim().is_empty() {
            out.push((t.clone(), shown, r.width, r.height));
        }
    }
    for &child in ui.children(id) {
        texts(ui, child, out);
    }
}

/// Texts cut off at the side (one line that does not fit), texts squeezed narrower than their
/// longest word (they would break inside the word: letters one below the other), raw keys.
fn text_problems(ui: &Ui, lang: Lang, problems: &mut Vec<String>) {
    let mut found = Vec::new();
    texts(ui, ui.root(), &mut found);
    let mut ruler = Ruler::new(lang);
    for (text, shown, _, _) in &found {
        ruler.ask(text, shown);
    }
    for ((_, shown, width, height), (_, natural, line, word)) in found.iter().zip(ruler.answers()) {
        if raw_key(shown.trim()) {
            problems.push(format!("untranslated key {shown:?}"));
        }
        let lines = (height / line.max(1.0)).round();
        if lines <= 1.0 && width + 1.0 < natural {
            problems.push(format!(
                "text {shown:?} cut off: {width:.0} of the {natural:.0} points it needs"
            ));
        }
        if lines > 1.0 && width + 1.0 < word {
            problems.push(format!(
                "text {shown:?} squeezed to {width:.0} points, its longest word needs {word:.0}"
            ));
        }
    }
}

fn walk(ui: &Ui, id: NodeId, width: f32, problems: &mut Vec<String>) {
    let Some(r) = ui.logical_rect(id) else {
        return;
    };
    if r.width > 0.0 && r.height > 0.0 && (r.start < -0.5 || r.start + r.width > width + 0.5) {
        let what = ui.widget(id).map(widget_name).unwrap_or("?");
        problems.push(format!(
            "{what} {:?} outside the window ({:.0}..{:.0} of {width:.0})",
            id,
            r.start,
            r.start + r.width
        ));
    }
    // the display tiles of the arrangement stage all lie on the stage
    if let Some(Widget::Arrangement(stage)) = ui.widget(id) {
        for item in &stage.items {
            if item.rect.x < -0.5 || item.rect.x + item.rect.w > r.width + 0.5 {
                problems.push(format!(
                    "display tile at {:.0}..{:.0} outside its stage (0..{:.0})",
                    item.rect.x,
                    item.rect.x + item.rect.w,
                    r.width
                ));
            }
        }
    }
    // a child sticks out of its parent at the sides (cut off by the parent's card or column), except
    // where the parent scrolls
    let scrolls = matches!(ui.widget(id), Some(Widget::Scroll(_)));
    for &child in ui.children(id) {
        if let (false, Some(c)) = (scrolls, ui.logical_rect(child)) {
            let visible = c.width > 0.0 && c.height > 0.0 && r.width > 0.0;
            // up to BLEED: rules and highlights drawn over a card's padding
            if visible
                && (c.start < r.start - BLEED || c.start + c.width > r.start + r.width + BLEED)
            {
                let what = ui.widget(child).map(widget_name).unwrap_or("?");
                problems.push(format!(
                    "{what} {child:?} sticks out of its parent ({:.0}..{:.0} in {:.0}..{:.0})",
                    c.start,
                    c.start + c.width,
                    r.start,
                    r.start + r.width
                ));
            }
        }
        walk(ui, child, width, problems);
    }
}

fn widget_name(w: &Widget) -> &'static str {
    match w {
        Widget::Text(_) => "text",
        Widget::Card(_) => "card",
        Widget::Flex(_) => "flex",
        Widget::Row(_) => "row",
        Widget::Button(_) => "button",
        _ => "widget",
    }
}

#[test]
fn every_page_reflows_without_clipping_or_squeezed_text() {
    let out = std::env::var_os("ULTAR_CONFORMANCE_OUT").map(std::path::PathBuf::from);
    let mut problems = Vec::new();
    // the owner's VMware screen (a maximized window), a small window, and the size the window
    // opens with on that screen
    for (w, h) in [(1024.0, 768.0), (640.0, 480.0), (768.0, 462.0)] {
        for lang in Lang::ALL {
            for area in AREAS.iter() {
                for signed_in in [true, false] {
                    if !signed_in && area.id != "accounts" {
                        continue;
                    }
                    let world = board_world(2, signed_in);
                    {
                        // the network, Bluetooth and sound pages with their rows
                        let mut w = world.borrow_mut();
                        w.net = Some(Default::default());
                        w.bluetooth = Some(Default::default());
                        w.audio = Some(Default::default());
                    }
                    let size = content_size(w, h);
                    let mut app = app(&world, lang, ThemeKind::Dark, size, area.id);
                    app.ui.layout();
                    let mut found = Vec::new();
                    walk(&app.ui, app.ui.root(), size.w, &mut found);
                    text_problems(&app.ui, lang, &mut found);
                    for p in found {
                        problems.push(format!("{w}x{h} {lang:?} {} ({signed_in}): {p}", area.id));
                    }
                    if let Some(dir) = &out {
                        let name = format!("layout-{w}x{h}-{lang:?}-{}-{signed_in}.png", area.id);
                        write_png(&dir.join(name), &mut app.ui);
                    }
                }
            }
        }
    }
    problems.dedup();
    assert!(
        problems.is_empty(),
        "{} layout problems:\n{}",
        problems.len(),
        problems
            .iter()
            .take(80)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// A window that changes its width (made wider, narrower, maximized) lays its page out again for the
/// new width: the same result as a window opened at that size - no column of the old width stays.
#[test]
fn a_resized_window_reflows_like_a_new_one() {
    let sizes = [
        (768.0, 462.0),
        (1024.0, 768.0),
        (640.0, 480.0),
        (1440.0, 900.0),
    ];
    for area in ["displays", "accounts", "drivers", "notifications"] {
        for lang in [Lang::Ar, Lang::En] {
            let world = board_world(2, true);
            let mut resized = app(&world, lang, ThemeKind::Dark, content_size(1.0, 41.0), area);
            for (w, h) in sizes {
                let size = content_size(w, h);
                resized.ui.set_size(size).expect("size");
                resized.resized().expect("resized");
                resized.ui.layout();
                let mut fresh = app(&world, lang, ThemeKind::Dark, size, area);
                fresh.ui.layout();
                let mut problems = Vec::new();
                walk(&resized.ui, resized.ui.root(), size.w, &mut problems);
                text_problems(&resized.ui, lang, &mut problems);
                assert!(problems.is_empty(), "{area} {lang:?} {w}x{h}: {problems:?}");
                let rects = |ui: &Ui| {
                    let mut all = Vec::new();
                    let mut todo = vec![ui.root()];
                    while let Some(id) = todo.pop() {
                        if let Some(r) = ui.logical_rect(id) {
                            all.push((r.start.round(), r.width.round()));
                        }
                        todo.extend(ui.children(id).iter().copied());
                    }
                    all.sort_by(|a, b| a.partial_cmp(b).expect("numbers"));
                    all
                };
                assert_eq!(
                    rects(&resized.ui),
                    rects(&fresh.ui),
                    "{area} {lang:?}: resized to {w}x{h} differs from a window opened at that size"
                );
            }
        }
    }
}
