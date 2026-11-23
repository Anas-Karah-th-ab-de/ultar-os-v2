//! Gestures built from presses and the clock: long press (= secondary click by touch),
//! double click, press-and-hold buttons with repeat, and dragging of draggable nodes.

use super::{Action, ActionKind};
use crate::geom::Point;
use crate::tokens::{motion, size};
use crate::ui::{NodeId, Pointer, Ui};
use crate::widgets::Widget;

/// A finger resting on the surface, waiting to become a long press.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LongPress {
    pub(crate) touch: u64,
    pub(crate) pos: Point,
    pub(crate) since: u64,
    pub(crate) fired: bool,
}

/// A held button that reports `Repeated` while it stays down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hold {
    pub(crate) node: NodeId,
    pub(crate) pointer: Pointer,
    pub(crate) next: u64,
}

/// A draggable node under a pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Grab {
    pub(crate) node: NodeId,
    pub(crate) pointer: Pointer,
    pub(crate) start: Point,
    pub(crate) active: bool,
}

/// The gesture state of a UI.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Gestures {
    pub(crate) long_press: Option<LongPress>,
    pub(crate) last_click: Option<(NodeId, u64, Point)>,
    pub(crate) hold: Option<Hold>,
    pub(crate) grab: Option<Grab>,
    /// A password field shown while its button is held (`Some(pointer)`) or after the
    /// keyboard toggled it (`None`).
    pub(crate) reveal: Option<(NodeId, Option<Pointer>)>,
}

/// Most `Repeated` actions one clock step may produce (a stalled clock must not burst).
const MAX_REPEATS: u32 = 8;

fn far(a: Point, b: Point) -> bool {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    // A position that is not a number counts as far (the gesture ends).
    let d = dx * dx + dy * dy;
    d.is_nan() || d > size::TOUCH_SLOP * size::TOUCH_SLOP
}

fn act(node: NodeId, kind: ActionKind) -> Action {
    Action { node, kind }
}

impl Ui {
    /// `true` for a button that reports press, repeat and release instead of activation.
    pub(crate) fn is_hold_button(&self, id: NodeId) -> bool {
        matches!(&self.nodes[id.index()].widget, Widget::Button(b) if b.hold)
    }

    /// A press landed on `id`: start hold, grab and long-press tracking.
    pub(crate) fn gesture_down(
        &mut self,
        id: NodeId,
        (pos, part): (Point, Option<usize>),
        pointer: Pointer,
        out: &mut Vec<Action>,
    ) {
        let now = self.now_ms;
        if let (Some(1), Ok(field)) = (part, self.text_field_mut(id)) {
            field.revealed = true;
            self.layers.gestures.reveal = Some((id, Some(pointer)));
            self.touch(id);
        }
        if self.is_hold_button(id) {
            let next = now.saturating_add(u64::from(motion::LONG_PRESS));
            self.layers.gestures.hold = Some(Hold {
                node: id,
                pointer,
                next,
            });
            out.push(act(id, ActionKind::Pressed));
        }
        if self.nodes[id.index()].draggable {
            let grab = Grab {
                node: id,
                pointer,
                start: pos,
                active: false,
            };
            self.layers.gestures.grab = Some(grab);
        }
    }

    /// A finger went down: it may become a long press.
    pub(crate) fn watch_long_press(&mut self, touch: u64, pos: Point) {
        let since = self.now_ms;
        let press = LongPress {
            touch,
            pos,
            since,
            fired: false,
        };
        self.layers.gestures.long_press = Some(press);
    }

    /// Pointer movement: cancels a long press that moved, moves a grabbed node. Returns
    /// `true` when a drag used the movement.
    pub(crate) fn gesture_move(
        &mut self,
        pos: Point,
        pointer: Pointer,
        out: &mut Vec<Action>,
    ) -> bool {
        if let Some(press) = self.layers.gestures.long_press {
            if pointer == Pointer::Touch && !press.fired && far(press.pos, pos) {
                self.layers.gestures.long_press = None;
            }
        }
        let Some(mut grab) = self.layers.gestures.grab.filter(|g| g.pointer == pointer) else {
            return false;
        };
        if !grab.active && !far(grab.start, pos) {
            return false;
        }
        if !grab.active {
            grab.active = true;
            self.set_press(pointer, None);
            out.push(act(grab.node, ActionKind::DragStarted));
        }
        let delta = Point::new(pos.x - grab.start.x, pos.y - grab.start.y);
        self.layers.gestures.grab = Some(grab);
        out.push(act(grab.node, ActionKind::Dragged(delta)));
        true
    }

    /// A release (or cancel when `pos` is `None`): ends holds, drags and swallows the release
    /// of a fired long press. Returns `true` when the release must not activate anything.
    pub(crate) fn gesture_up(
        &mut self,
        pointer: Pointer,
        pos: Option<Point>,
        out: &mut Vec<Action>,
    ) -> bool {
        let mut handled = false;
        if self
            .layers
            .gestures
            .reveal
            .is_some_and(|r| r.1 == Some(pointer))
        {
            self.hide_password();
            handled = true;
        }
        let dropped = self
            .layers
            .gestures
            .grab
            .filter(|g| g.pointer == pointer && g.active);
        let target = match (dropped, pos) {
            (Some(grab), Some(p)) => self.drop_target_at(p, grab.node),
            _ => None,
        };
        let gestures = &mut self.layers.gestures;
        if let Some(hold) = gestures.hold.filter(|h| h.pointer == pointer) {
            gestures.hold = None;
            out.push(act(hold.node, ActionKind::Released));
            handled = true;
        }
        if let Some(grab) = gestures.grab.filter(|g| g.pointer == pointer) {
            gestures.grab = None;
            if grab.active {
                let end = pos.unwrap_or(grab.start);
                let delta = Point::new(end.x - grab.start.x, end.y - grab.start.y);
                out.push(act(grab.node, ActionKind::DragEnded { delta, target }));
                handled = true;
            }
        }
        if pointer == Pointer::Touch {
            if let Some(press) = gestures.long_press.take() {
                handled |= press.fired;
            }
        }
        handled
    }

    /// After `id` was activated by a press: a second activation of the same node soon
    /// enough and close enough is a double click.
    pub(crate) fn detect_double(&mut self, id: NodeId, pos: Point, out: &mut Vec<Action>) {
        let now = self.now_ms;
        let window = u64::from(motion::DOUBLE_CLICK);
        match self.layers.gestures.last_click {
            Some((node, at, at_pos))
                if node == id && now.saturating_sub(at) <= window && !far(at_pos, pos) =>
            {
                self.layers.gestures.last_click = None;
                out.push(act(id, ActionKind::DoubleActivated));
            }
            _ => self.layers.gestures.last_click = Some((id, now, pos)),
        }
    }

    /// The clock moved: fire a due long press, repeat a held button. Actions go to the
    /// queue that the next `handle` returns.
    pub(crate) fn gesture_tick(&mut self) {
        let now = self.now_ms;
        let mut out = Vec::new();
        if let Some(mut hold) = self.layers.gestures.hold {
            let mut count = 0;
            while now >= hold.next && count < MAX_REPEATS {
                out.push(act(hold.node, ActionKind::Repeated));
                hold.next = hold.next.saturating_add(u64::from(motion::REPEAT.max(1)));
                count += 1;
            }
            if now >= hold.next {
                hold.next = now.saturating_add(u64::from(motion::REPEAT.max(1)));
            }
            self.layers.gestures.hold = Some(hold);
        }
        if let Some(mut press) = self.layers.gestures.long_press {
            let due = press.since.saturating_add(u64::from(motion::LONG_PRESS));
            if !press.fired && now >= due {
                press.fired = true;
                self.layers.gestures.long_press = Some(press);
                self.fire_long_press(press.pos, &mut out);
            }
        }
        self.queue(out);
    }

    fn fire_long_press(&mut self, pos: Point, out: &mut Vec<Action>) {
        // The finger no longer presses what is under it: the release activates nothing.
        self.set_press(Pointer::Touch, None);
        self.layers.gestures.grab = None;
        if let Some(id) = self.node_at(pos) {
            self.open_registered_menu(id, Some(pos), out);
        }
    }

    /// The next clock value a gesture needs (long press, repeat).
    pub(crate) fn gesture_wake(&self) -> Option<u64> {
        let gestures = &self.layers.gestures;
        let hold = gestures.hold.map(|h| h.next);
        let press = gestures
            .long_press
            .filter(|p| !p.fired)
            .map(|p| p.since.saturating_add(u64::from(motion::LONG_PRESS)));
        [hold, press].into_iter().flatten().min()
    }

    /// Masks a shown password again (its hold ended or was interrupted, the field lost the
    /// focus, was disabled or cleared, a dialog opened).
    pub(crate) fn hide_password(&mut self) {
        if let Some((id, _)) = self.layers.gestures.reveal.take() {
            if let Ok(field) = self.text_field_mut(id) {
                field.revealed = false;
            }
            self.touch(id);
        }
    }

    /// Ends every gesture without reporting it (a dialog appeared or went, the pointer
    /// left the surface): holds, drags and long presses stop, a shown password is masked.
    pub(crate) fn end_gestures(&mut self) {
        self.hide_password();
        let gestures = &mut self.layers.gestures;
        gestures.hold = None;
        gestures.grab = None;
        gestures.long_press = None;
    }

    /// A middle release over the node the middle press landed on: `MiddleClicked` (open in
    /// a new window, close a tab).
    pub(crate) fn middle_up(&mut self, pos: Point, out: &mut Vec<Action>) -> bool {
        let Some(id) = self.input.middle.take() else {
            return false;
        };
        if self.hit(pos) == Some(id) {
            out.push(act(id, ActionKind::MiddleClicked));
        }
        true
    }

    /// The pointer left the surface: no hover, no middle click, a password shown while the
    /// mouse held its button is masked again.
    pub(crate) fn pointer_leave(&mut self) -> bool {
        self.input.pointer = None;
        self.input.middle = None;
        if self
            .layers
            .gestures
            .reveal
            .is_some_and(|r| r.1 == Some(Pointer::Mouse))
        {
            self.hide_password();
        }
        self.set_hover(None);
        false
    }
}
