//! The clock: animations (switch knob, progress) and the tooltip delay.

use super::{NodeId, Tip, Ui};
use crate::tokens::{motion, Space};
use crate::widgets::{shadow_reach, tooltip_rect, MeasureCx, Switch, Widget};

/// Duration of one revolution of the progress ring and one pass of an indeterminate bar.
const SPIN_MS: u64 = motion::SPINNER_PERIOD as u64;

/// `true` for nodes that change with the clock alone.
fn moves(node: &super::Node) -> bool {
    node.loading
        || match &node.widget {
            Widget::Switch(switch) => switch.toggled_at.is_some(),
            Widget::Progress(progress) => progress.value.is_none(),
            _ => false,
        }
}

impl Ui {
    /// Sets the clock (milliseconds, monotonic, supplied by the caller) that drives
    /// animations and the tooltip delay, and marks animating widgets for repaint.
    pub fn advance_to(&mut self, now_ms: u64) {
        if now_ms == self.now_ms {
            return;
        }
        let was_animating = self.animating();
        self.now_ms = now_ms;
        self.show_due_tooltip();
        self.gesture_tick();
        self.expire_toasts();
        if !(was_animating || self.animating()) {
            return;
        }
        for index in 0..self.nodes.len() {
            if moves(&self.nodes[index]) {
                self.touch(NodeId(index as u32));
            }
        }
        for node in &mut self.nodes {
            if let Widget::Switch(switch) = &mut node.widget {
                if switch.settled(now_ms) {
                    switch.toggled_at = None;
                }
            }
        }
    }

    /// Actions the clock caused since the last event (a long press opening a context menu,
    /// `Repeated` of a held button, a toast that timed out). Call it after
    /// [`Ui::advance_to`]; whatever is not taken here comes first in the next
    /// [`Response`](crate::event::Response).
    pub fn take_actions(&mut self) -> Vec<crate::event::Action> {
        std::mem::take(&mut self.pending)
    }

    /// `true` while something moves (a knob in transit, a progress ring or bar), i.e. the
    /// caller should keep calling [`Ui::advance_to`] and [`Ui::paint`] every frame.
    pub fn animating(&self) -> bool {
        !self.reduce_motion() && self.nodes.iter().any(moves)
    }

    /// The clock value at which [`Ui::advance_to`] must be called even though nothing
    /// animates: a tooltip, a long press, a held button or a toast is due. `None` when
    /// nothing waits.
    pub fn next_wake(&self) -> Option<u64> {
        let tooltip = self
            .layers
            .hover_since
            .filter(|_| self.layers.tip.is_none());
        let tooltip = tooltip.map(|since| since.saturating_add(u64::from(motion::TOOLTIP_DELAY)));
        let toast = self.toast_wake();
        [tooltip, self.gesture_wake(), toast]
            .into_iter()
            .flatten()
            .min()
    }

    /// Rotation of progress rings for the current clock, 0..1.
    pub(crate) fn spin(&self) -> f32 {
        if self.reduce_motion() {
            0.0
        } else {
            (self.now_ms % SPIN_MS.max(1)) as f32 / SPIN_MS.max(1) as f32
        }
    }

    /// Knob position of a switch for the current clock, 0 (off) to 1 (on).
    pub(crate) fn knob_position(&self, switch: &Switch) -> f32 {
        if self.reduce_motion() {
            switch.target()
        } else {
            switch.position(self.now_ms)
        }
    }

    /// Starts the tooltip delay for the hovered node, or stops it.
    pub(crate) fn watch_hover(&mut self, hover: Option<NodeId>) {
        let has_tip = hover.is_some_and(|id| self.nodes[id.index()].tooltip.is_some());
        self.layers.hover_since = has_tip.then_some(self.now_ms);
    }

    fn show_due_tooltip(&mut self) {
        let due = self.next_wake().is_some_and(|at| self.now_ms >= at);
        if let (true, Some(node)) = (due, self.input.hover) {
            self.show_tooltip(node);
        }
    }

    /// Shows the tooltip of `id`, if it has one.
    pub(crate) fn show_tooltip(&mut self, id: NodeId) {
        self.layout();
        let Some(anchor) = self.rect(id) else {
            return;
        };
        let size = self.size();
        let Some(label) = self.nodes[id.index()].tooltip.as_ref() else {
            return;
        };
        let mut cx = MeasureCx {
            res: self.res.get_mut(),
            env: &self.env,
            tr: &self.tr,
            mode: self.mode,
        };
        let rect = tooltip_rect(&mut cx, label, anchor, size);
        self.hide_tooltip();
        self.layers.tip = Some(Tip { node: id, rect });
        self.damage_floating(rect);
    }

    /// Hides the tooltip, if one shows.
    pub(crate) fn hide_tooltip(&mut self) {
        if let Some(tip) = self.layers.tip.take() {
            self.damage_floating(tip.rect);
        }
    }

    /// Marks a floating rectangle with its shadow for repaint.
    pub(crate) fn damage_floating(&mut self, rect: crate::geom::Rect) {
        let reach = shadow_reach();
        let grown = rect.inflate(reach.x.max(Space::S8.points()));
        self.damage(crate::geom::Rect::new(
            grown.x,
            grown.y,
            grown.w,
            grown.h + reach.y,
        ));
    }
}
