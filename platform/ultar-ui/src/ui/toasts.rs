//! Toasts on the surface: showing, timing out, closing, hit-testing.

use super::{Ui, UiError};
use crate::event::{Action, ActionKind};
use crate::geom::Point;
use crate::tokens::motion;
use crate::widgets::{MeasureCx, Toast, ToastView, MAX_TOASTS};

/// A toast on the surface and since when.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Shown {
    pub(crate) toast: Toast,
    pub(crate) since: u64,
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
        self.layers.toasts.push(Shown { toast, since });
        self.damage_toasts();
        self.queue(closed);
        Ok(())
    }

    /// Removes a toast without reporting it.
    pub fn hide_toast(&mut self, id: u32) {
        self.damage_toasts();
        self.layers.toasts.retain(|t| t.toast.id != id);
    }

    /// Identifiers of the visible toasts, oldest first.
    pub fn toasts(&self) -> Vec<u32> {
        self.layers.toasts.iter().map(|t| t.toast.id).collect()
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

    /// A press on a toast: its action or its close button. Returns `true` when it hit one.
    pub(crate) fn toast_press(&mut self, pos: Point, out: &mut Vec<Action>) -> bool {
        let hit = self
            .toast_views()
            .into_iter()
            .rev()
            .find(|(_, view)| view.rect.contains(pos));
        let Some((index, view)) = hit else {
            return false;
        };
        let id = self.layers.toasts[index].toast.id;
        if view.action.is_some_and(|rect| rect.contains(pos)) {
            out.push(self.toast_action(ActionKind::ToastAction(id)));
            self.close_toast(id, out);
        } else if view.close.contains(pos) {
            self.close_toast(id, out);
        }
        true
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

    /// Toasts without action go away after `motion.toast`.
    pub(crate) fn expire_toasts(&mut self) {
        let now = self.now_ms;
        let due = |t: &Shown| {
            t.toast.action.is_none() && now >= t.since.saturating_add(u64::from(motion::TOAST))
        };
        let expired: Vec<u32> = self
            .layers
            .toasts
            .iter()
            .filter(|t| due(t))
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
        self.layers
            .toasts
            .iter()
            .filter(|t| t.toast.action.is_none())
            .map(|t| t.since.saturating_add(u64::from(motion::TOAST)))
            .min()
    }
}
