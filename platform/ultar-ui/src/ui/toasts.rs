//! Toasts on the surface: showing, timing out, closing, hit-testing, keyboard focus.
//!
//! Timing (DESKTOP-SPEC §3, decision of round 5): a toast without action closes after
//! `motion.toast` (4 s). A toast with an action stays `motion.toast_action` (10 s); its time
//! stands still while the pointer is over it or one of its buttons has the keyboard focus
//! (WCAG 2.2.1), and it can always be closed at once with Esc or its close button.

use super::{NodeId, Ui, UiError};
use crate::event::{Action, ActionKind, Key};
use crate::geom::Point;
use crate::tokens::motion;
use crate::widgets::{MeasureCx, Toast, ToastPart, ToastView, MAX_TOASTS};

/// A toast on the surface and since when.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Shown {
    pub(crate) toast: Toast,
    pub(crate) since: u64,
    /// Since when its time stands still (pointer over it, keyboard focus on it).
    paused_at: Option<u64>,
    /// Milliseconds it stood still before.
    paused_ms: u64,
}

impl Shown {
    /// How long the toast is shown while its time runs.
    fn life(&self) -> u64 {
        if self.toast.action.is_some() {
            u64::from(motion::TOAST_ACTION)
        } else {
            u64::from(motion::TOAST)
        }
    }

    /// When the toast times out; `None` while its time stands still.
    fn due(&self) -> Option<u64> {
        match self.paused_at {
            Some(_) => None,
            None => Some(
                self.since
                    .saturating_add(self.life())
                    .saturating_add(self.paused_ms),
            ),
        }
    }
}

/// Pointer and keyboard state of the toasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ToastInput {
    /// The toast under the pointer.
    hover: Option<u32>,
    /// The button a press went down on: it acts on release over the same button.
    press: Option<(u32, ToastPart)>,
    /// The button with the keyboard focus.
    pub(crate) focus: Option<(u32, ToastPart)>,
    /// Where the keyboard focus goes back to when the focused toast closes.
    focus_return: Option<NodeId>,
}

impl Ui {
    /// Shows a toast at the bottom of the surface. A toast with the same identifier is
    /// replaced; beyond three the oldest goes (it reports `ToastClosed` with the next event).
    pub fn show_toast(&mut self, toast: Toast) -> Result<(), UiError> {
        let mut closed = Vec::new();
        self.damage_toasts();
        self.layers.toasts.retain(|t| t.toast.id != toast.id);
        while self.layers.toasts.len() >= MAX_TOASTS {
            let old = self.layers.toasts.remove(0);
            closed.push(self.toast_action(ActionKind::ToastClosed(old.toast.id)));
        }
        let since = self.now_ms;
        self.layers.toasts.push(Shown {
            toast,
            since,
            paused_at: None,
            paused_ms: 0,
        });
        self.damage_toasts();
        self.sync_toast_pause();
        self.queue(closed);
        Ok(())
    }

    /// Removes a toast without reporting it. When it had the keyboard focus, the focus goes
    /// back to where it was before.
    pub fn hide_toast(&mut self, id: u32) {
        self.damage_toasts();
        self.layers.toasts.retain(|t| t.toast.id != id);
        let input = &mut self.layers.toast_input;
        input.press = input.press.filter(|p| p.0 != id);
        input.hover = input.hover.filter(|h| *h != id);
        if input.focus.is_some_and(|f| f.0 == id) {
            input.focus = None;
            let back = input.focus_return.take();
            if let Some(node) = back.filter(|n| self.order.contains(n) && !self.is_disabled(*n)) {
                self.set_focus(Some(node), true);
            }
        }
    }

    /// Identifiers of the visible toasts, oldest first.
    pub fn toasts(&self) -> Vec<u32> {
        self.layers.toasts.iter().map(|t| t.toast.id).collect()
    }

    /// The toast and button that have the keyboard focus (`true` = the action button).
    pub fn toast_focus(&self) -> Option<(u32, bool)> {
        self.layers
            .toast_input
            .focus
            .map(|(id, part)| (id, part == ToastPart::Action))
    }

    /// Toast actions are reported for the root node.
    fn toast_action(&self, kind: ActionKind) -> Action {
        Action {
            node: self.root(),
            kind,
        }
    }

    /// Geometry of every visible toast, newest at the bottom.
    pub(crate) fn toast_views(&mut self) -> Vec<(usize, ToastView)> {
        let size = self.size();
        let count = self.layers.toasts.len();
        let mut cx = MeasureCx {
            res: self.res.get_mut(),
            env: &self.env,
            tr: &self.tr,
            mode: self.mode,
        };
        let mut views = Vec::with_capacity(count);
        for (index, shown) in self.layers.toasts.iter().enumerate() {
            let place = count - 1 - index;
            views.push((index, shown.toast.view(&mut cx, size, place)));
        }
        views
    }

    fn damage_toasts(&mut self) {
        for (_, view) in self.toast_views() {
            self.damage_floating(view.rect);
        }
    }

    /// The toast under `pos` and the button there, if any.
    fn toast_hit(&mut self, pos: Point) -> Option<(u32, Option<ToastPart>)> {
        let (index, view) = self
            .toast_views()
            .into_iter()
            .rev()
            .find(|(_, view)| view.rect.contains(pos))?;
        let id = self.layers.toasts.get(index)?.toast.id;
        Some((id, view.part_at(pos)))
    }

    /// A press on a toast. Its buttons act on release (a press can be taken back by moving
    /// away, as on every button). Returns `true` when the press landed on a toast.
    pub(crate) fn toast_press(&mut self, pos: Point) -> bool {
        let Some((id, part)) = self.toast_hit(pos) else {
            return false;
        };
        self.layers.toast_input.press = part.map(|p| (id, p));
        true
    }

    /// A release after a press on a toast button: over the same button it runs it.
    /// Returns `true` when a toast press was pending.
    pub(crate) fn toast_release(&mut self, pos: Point, out: &mut Vec<Action>) -> bool {
        let Some((id, part)) = self.layers.toast_input.press.take() else {
            return false;
        };
        if self.toast_hit(pos) == Some((id, Some(part))) {
            self.run_toast_part(id, part, out);
        }
        true
    }

    fn run_toast_part(&mut self, id: u32, part: ToastPart, out: &mut Vec<Action>) {
        if part == ToastPart::Action {
            out.push(self.toast_action(ActionKind::ToastAction(id)));
        }
        self.close_toast(id, out);
    }

    /// Pointer movement: the toast under the pointer stops its time.
    pub(crate) fn toast_hover(&mut self, pos: Option<Point>) {
        let hover = pos.and_then(|p| self.toast_hit(p)).map(|(id, _)| id);
        if hover != self.layers.toast_input.hover {
            self.layers.toast_input.hover = hover;
            self.sync_toast_pause();
        }
    }

    /// Closes a toast and reports it.
    pub(crate) fn close_toast(&mut self, id: u32, out: &mut Vec<Action>) {
        self.hide_toast(id);
        out.push(self.toast_action(ActionKind::ToastClosed(id)));
    }

    /// The newest toast (Escape closes it, Ctrl+Enter runs its action).
    pub(crate) fn newest_toast(&self) -> Option<&Toast> {
        self.layers.toasts.last().map(|t| &t.toast)
    }

    /// Stops or restarts the time of every toast after the pointer or the focus moved.
    fn sync_toast_pause(&mut self) {
        let (now, input) = (self.now_ms, self.layers.toast_input);
        for shown in &mut self.layers.toasts {
            let id = shown.toast.id;
            let held = input.hover == Some(id) || input.focus.is_some_and(|f| f.0 == id);
            let stops = shown.toast.action.is_some() && held;
            match (stops, shown.paused_at) {
                (true, None) => shown.paused_at = Some(now),
                (false, Some(at)) => {
                    shown.paused_ms = shown.paused_ms.saturating_add(now.saturating_sub(at));
                    shown.paused_at = None;
                }
                _ => {}
            }
        }
    }

    /// Toasts whose time ran out go away.
    pub(crate) fn expire_toasts(&mut self) {
        let now = self.now_ms;
        let expired: Vec<u32> = self
            .layers
            .toasts
            .iter()
            .filter(|t| t.due().is_some_and(|due| now >= due))
            .map(|t| t.toast.id)
            .collect();
        let mut out = Vec::new();
        for id in expired {
            self.close_toast(id, &mut out);
        }
        self.queue(out);
    }

    /// When the next toast times out.
    pub(crate) fn toast_wake(&self) -> Option<u64> {
        self.layers.toasts.iter().filter_map(Shown::due).min()
    }

    /// Tab with the keyboard focus on a toast moves between its buttons and leaves it after
    /// the close button (forward) or before the action (backward). Tab past the last control
    /// of the surface enters the newest toast with an action. Returns `true` when handled.
    pub(crate) fn toast_tab(&mut self, forward: bool, at_edge: bool) -> bool {
        if let Some((id, part)) = self.layers.toast_input.focus {
            let next = match (part, forward) {
                (ToastPart::Action, true) => Some(ToastPart::Close),
                (ToastPart::Close, false) => Some(ToastPart::Action),
                _ => None,
            };
            match next {
                Some(part) => self.layers.toast_input.focus = Some((id, part)),
                None => {
                    self.layers.toast_input.focus = None;
                    self.layers.toast_input.focus_return = None;
                    self.input.focus_hint = None;
                    self.sync_toast_pause();
                    return false;
                }
            }
            self.damage_toasts();
            return true;
        }
        let target = self
            .layers
            .toasts
            .iter()
            .rev()
            .find(|t| t.toast.action.is_some())
            .map(|t| t.toast.id);
        let (Some(id), true) = (target, at_edge) else {
            return false;
        };
        let part = if forward {
            ToastPart::Action
        } else {
            ToastPart::Close
        };
        let back = self.input.focus;
        self.set_focus(None, true);
        self.layers.toast_input.focus_return = back;
        self.layers.toast_input.focus = Some((id, part));
        self.sync_toast_pause();
        self.damage_toasts();
        true
    }

    /// Enter, Space and Escape on a focused toast button. Returns `true` when handled.
    pub(crate) fn toast_focus_key(&mut self, key: Key, out: &mut Vec<Action>) -> bool {
        let Some((id, part)) = self.layers.toast_input.focus else {
            return false;
        };
        match key {
            Key::Enter | Key::Space => self.run_toast_part(id, part, out),
            Key::Escape => self.close_toast(id, out),
            _ => return false,
        }
        true
    }

    /// Something else took the keyboard focus: the toast keeps running.
    pub(crate) fn drop_toast_focus(&mut self) {
        if self.layers.toast_input.focus.take().is_some() {
            self.layers.toast_input.focus_return = None;
            self.sync_toast_pause();
            self.damage_toasts();
        }
    }
}
