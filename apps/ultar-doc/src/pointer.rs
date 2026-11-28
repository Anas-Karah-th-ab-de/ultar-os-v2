//! Mouse and touch on the document: a click puts the caret, dragging selects, a double click
//! selects the word, a triple click the paragraph, the wheel scrolls (Shift: sideways, Ctrl:
//! zoom). A finger scrolls by dragging, a tap puts the caret, a double tap selects the word,
//! and holding the finger for a moment before dragging selects.

use crate::app::App;
use crate::paint::paper_x; // physical
use std::time::{Duration, Instant};
use ultar_ui::event::{Event, Modifiers, PointerButton};
use ultar_ui::geom::Point;
use ultar_ui::ui::UiError;

/// Two clicks closer in time and place than this belong together.
const MULTI: Duration = Duration::from_millis(450);
/// How long a finger rests before dragging selects instead of scrolling.
const HOLD: Duration = Duration::from_millis(450);
/// Distance (points) a finger may move and still count as a tap or a hold.
const SLOP: f32 = 8.0;
/// Pixels one wheel line scrolls.
const LINE_PX: f32 = 48.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Pending,
    Scroll,
    Select,
}

#[derive(Clone, Copy)]
struct Touch {
    id: u64,
    start: Point,
    last: Point,
    since: Instant,
    mode: Mode,
}

/// The state between pointer events.
#[derive(Default)]
pub struct PointerState {
    selecting: bool,
    last_click: Option<(Instant, Point, u8)>,
    touch: Option<Touch>,
    mods: Modifiers,
}

fn near(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() <= SLOP && (a.y - b.y).abs() <= SLOP
}

impl App {
    /// The point in document coordinates (device pixels), `None` outside the text area.
    fn doc_point(&self, p: Point, strict: bool) -> Option<(f32, f32)> {
        let rect = self.ui.rect(self.nodes.text?)?;
        if strict && !rect.contains(p) {
            return None;
        }
        let s = self.ui.env().scale;
        let (vx, vy) = ((p.x - rect.x) * s, (p.y - rect.y) * s);
        let lay = self.lay.as_ref()?;
        Some((
            vx - paper_x(lay, rect.w * s, self.scroll.0), // physical
            vy + self.scroll.1,
        ))
    }

    fn put_caret(&mut self, p: Point, extend: bool, strict: bool) -> bool {
        self.ensure_layout();
        let Some((x, y)) = self.doc_point(p, strict) else {
            return false;
        };
        let Some(c) = self.lay.as_ref().and_then(|l| l.hit(x, y)) else {
            return false;
        };
        self.doc.set_pos(c, extend);
        self.inp.goal_x = None;
        true
    }

    /// A pointer or touch event (the widgets saw it too).
    pub fn pointer(&mut self, event: Event) -> Result<(), UiError> {
        if let Event::Modifiers { modifiers } = event {
            self.inp.pointer_state.mods = modifiers;
            return Ok(());
        }
        if self.ask.is_some() {
            return Ok(());
        }
        match event {
            Event::PointerDown {
                pos,
                button: PointerButton::Primary,
            } => self.press(pos),
            Event::PointerMove { pos } if self.inp.pointer_state.selecting => {
                if self.put_caret(pos, true, false) {
                    return self.moved();
                }
                Ok(())
            }
            Event::PointerUp { .. } => {
                self.inp.pointer_state.selecting = false;
                Ok(())
            }
            Event::Scroll { pos, lines } => self.wheel(pos, lines),
            Event::TouchDown { id, pos } => self.touch_down(id, pos),
            Event::TouchMove { id, pos } => self.touch_move(id, pos),
            Event::TouchUp { id, pos } => self.touch_up(id, pos),
            Event::TouchCancel { .. } => {
                self.inp.pointer_state.touch = None;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn press(&mut self, pos: Point) -> Result<(), UiError> {
        let now = Instant::now();
        let clicks = match self.inp.pointer_state.last_click {
            Some((t, p, n)) if now.duration_since(t) <= MULTI && near(p, pos) => (n % 3) + 1,
            _ => 1,
        };
        if !self.put_caret(pos, self.inp.pointer_state.mods.shift, true) {
            return Ok(());
        }
        self.inp.pointer_state.last_click = Some((now, pos, clicks));
        self.inp.pointer_state.selecting = clicks == 1;
        match clicks {
            2 => self.doc.select_word(),
            3 => self.doc.select_item(),
            _ => {}
        }
        if let Some(t) = self.nodes.text {
            let _ = self.ui.focus(t);
        }
        self.moved()
    }

    fn wheel(&mut self, pos: Point, lines: f32) -> Result<(), UiError> {
        let inside = self
            .nodes
            .text
            .and_then(|n| self.ui.rect(n))
            .is_some_and(|r| r.contains(pos));
        if !inside {
            return Ok(());
        }
        let m = self.inp.pointer_state.mods;
        if m.ctrl {
            let factor = if lines < 0.0 { 1.1 } else { 1.0 / 1.1 };
            self.zoom(factor);
            return self.changed();
        }
        if m.shift {
            self.scroll.0 += lines * LINE_PX;
        } else {
            self.scroll.1 += lines * LINE_PX;
        }
        self.clamp_scroll();
        self.draw()
    }

    fn touch_down(&mut self, id: u64, pos: Point) -> Result<(), UiError> {
        let inside = self
            .nodes
            .text
            .and_then(|n| self.ui.rect(n))
            .is_some_and(|r| r.contains(pos));
        if inside {
            self.inp.pointer_state.touch = Some(Touch {
                id,
                start: pos,
                last: pos,
                since: Instant::now(),
                mode: Mode::Pending,
            });
        }
        Ok(())
    }

    fn touch_move(&mut self, id: u64, pos: Point) -> Result<(), UiError> {
        let Some(mut t) = self.inp.pointer_state.touch.filter(|t| t.id == id) else {
            return Ok(());
        };
        let moved = !near(t.start, pos);
        if t.mode == Mode::Pending && moved {
            t.mode = if t.since.elapsed() >= HOLD {
                Mode::Select
            } else {
                Mode::Scroll
            };
            if t.mode == Mode::Select {
                self.put_caret(t.start, false, true);
            }
        }
        let s = self.ui.env().scale;
        let (dx, dy) = ((t.last.x - pos.x) * s, (t.last.y - pos.y) * s);
        t.last = pos;
        self.inp.pointer_state.touch = Some(t);
        match t.mode {
            Mode::Scroll => {
                self.scroll.0 += dx;
                self.scroll.1 += dy;
                self.clamp_scroll();
                self.draw()
            }
            Mode::Select => {
                self.put_caret(pos, true, false);
                self.moved()
            }
            Mode::Pending => Ok(()),
        }
    }

    fn touch_up(&mut self, id: u64, pos: Point) -> Result<(), UiError> {
        let Some(t) = self.inp.pointer_state.touch.take().filter(|t| t.id == id) else {
            return Ok(());
        };
        if t.mode != Mode::Pending {
            return Ok(());
        }
        let now = Instant::now();
        let double = self
            .inp
            .pointer_state
            .last_click
            .is_some_and(|(at, p, _)| now.duration_since(at) <= MULTI && near(p, pos));
        let held = t.since.elapsed() >= HOLD;
        if !self.put_caret(pos, false, true) {
            return Ok(());
        }
        self.inp.pointer_state.last_click = Some((now, pos, 1));
        if double || held {
            self.doc.select_word();
        }
        if let Some(n) = self.nodes.text {
            let _ = self.ui.focus(n);
        }
        self.moved()
    }
}
