//! Pointer and touch handling, activation and the select popup.

use super::{Action, ActionKind};
use crate::geom::Point;
use crate::ui::{NodeId, Popup, Ui};
use crate::widgets::{Widget, MAX_VISIBLE_OPTIONS};

impl Ui {
    /// The topmost operable widget under `pos`.
    pub(crate) fn hit(&self, pos: Point) -> Option<NodeId> {
        self.order.iter().rev().copied().find(|id| {
            let interactive = self.nodes[id.index()].widget.is_interactive();
            interactive && !self.is_disabled(*id) && self.rect(*id).is_some_and(|r| r.contains(pos))
        })
    }

    pub(crate) fn set_hover(&mut self, hover: Option<NodeId>) {
        if self.input.hover != hover {
            let previous = std::mem::replace(&mut self.input.hover, hover);
            for id in [previous, hover].into_iter().flatten() {
                self.touch(id);
            }
        }
    }

    pub(crate) fn set_pressed(&mut self, pressed: Option<NodeId>) {
        if self.input.pressed != pressed {
            let previous = std::mem::replace(&mut self.input.pressed, pressed);
            for id in [previous, pressed].into_iter().flatten() {
                self.touch(id);
            }
        }
    }

    /// Moves focus; `visible` shows the focus ring (keyboard focus).
    pub(crate) fn set_focus(&mut self, focus: Option<NodeId>, visible: bool) {
        if self.input.focus != focus || self.input.focus_visible != visible {
            let previous = std::mem::replace(&mut self.input.focus, focus);
            self.input.focus_visible = visible;
            for id in [previous, focus].into_iter().flatten() {
                self.touch(id);
            }
        }
    }

    /// Opens the options list of a select.
    pub(crate) fn open_popup(&mut self, id: NodeId, out: &mut Vec<Action>) {
        let Ok(select) = self.select(id) else {
            return;
        };
        if select.options.is_empty() {
            return;
        }
        let highlight = select.selected.min(select.options.len() - 1);
        let mut popup = Popup {
            select: id,
            highlight,
            scroll: 0,
        };
        keep_visible(&mut popup);
        self.input.popup = Some(popup);
        self.set_hover(None);
        self.touch_popup();
        self.touch(id);
        out.push(Action {
            node: id,
            kind: ActionKind::Opened,
        });
    }

    /// Closes the options list, if open.
    pub(crate) fn close_popup(&mut self, out: &mut Vec<Action>) {
        if let Some(popup) = self.input.popup {
            self.touch_popup();
            self.input.popup = None;
            self.touch(popup.select);
            out.push(Action {
                node: popup.select,
                kind: ActionKind::Closed,
            });
        }
    }

    /// Chooses option `index` of the open select and closes the list.
    pub(crate) fn choose(&mut self, index: usize, out: &mut Vec<Action>) {
        let Some(popup) = self.input.popup else {
            return;
        };
        if let Ok(select) = self.select_mut(popup.select) {
            if index < select.options.len() && select.selected != index {
                select.selected = index;
                out.push(Action {
                    node: popup.select,
                    kind: ActionKind::Selected(index),
                });
            }
        }
        self.close_popup(out);
    }

    /// Moves the popup highlight to `index` and scrolls it into view.
    pub(crate) fn highlight(&mut self, index: usize) {
        let Some(mut popup) = self.input.popup else {
            return;
        };
        let count = self.select(popup.select).map_or(0, |s| s.options.len());
        if count == 0 {
            return;
        }
        popup.highlight = index.min(count - 1);
        keep_visible(&mut popup);
        if self.input.popup != Some(popup) {
            self.input.popup = Some(popup);
            self.touch_popup();
        }
    }

    /// Performs the widget's main action (click, Enter, Space).
    pub(crate) fn activate(&mut self, id: NodeId, out: &mut Vec<Action>) {
        if self.is_disabled(id) || self.nodes[id.index()].loading {
            return;
        }
        let now = self.now_ms;
        let animate = !self.reduce_motion();
        let open = self.is_open(id);
        let kind = match &mut self.nodes[id.index()].widget {
            Widget::Button(_) | Widget::IconButton(_) => ActionKind::Activated,
            Widget::Switch(switch) => {
                switch.on = !switch.on;
                switch.toggled_at = animate.then_some(now);
                ActionKind::Toggled(switch.on)
            }
            Widget::Select(_) if open => return self.close_popup(out),
            Widget::Select(_) => return self.open_popup(id, out),
            _ => return,
        };
        out.push(Action { node: id, kind });
        self.touch(id);
    }

    /// Sets a slider from a pointer position.
    fn drag_slider(&mut self, id: NodeId, pos: Point, out: &mut Vec<Action>) {
        let (Some(rect), direction) = (self.rect(id), self.env.direction) else {
            return;
        };
        if let Ok(slider) = self.slider_mut(id) {
            let value = slider.value_at(rect, pos.x, direction);
            if value != slider.value {
                slider.value = value;
                out.push(Action {
                    node: id,
                    kind: ActionKind::ValueChanged(value),
                });
                self.touch(id);
            }
        }
    }

    fn place_caret(&mut self, id: NodeId, pos: Point) {
        let (Some(rect), env, mode) = (self.rect(id), self.env, self.mode) else {
            return;
        };
        let res = self.res.get_mut();
        if let Some(Widget::TextField(field)) =
            self.nodes.get_mut(id.index()).map(|n| &mut n.widget)
        {
            let caret = field.caret_at(res, &env, rect, mode, pos);
            field.set_caret(caret);
            self.touch(id);
        }
    }

    pub(crate) fn pointer_move(&mut self, pos: Point, hover: bool, out: &mut Vec<Action>) -> bool {
        if let Some((_, view)) = self.popup_view() {
            // The open list is modal: nothing beneath it is hovered.
            self.set_hover(None);
            if let Some(index) = view.option_at(pos) {
                self.highlight(index);
            }
            return view.rect.contains(pos);
        }
        if hover {
            let hit = self.hit(pos);
            self.set_hover(hit);
        }
        match self.input.pressed {
            Some(id) if matches!(self.nodes[id.index()].widget, Widget::Slider(_)) => {
                self.drag_slider(id, pos, out);
                true
            }
            pressed => pressed.is_some(),
        }
    }

    pub(crate) fn pointer_down(&mut self, pos: Point, out: &mut Vec<Action>) -> bool {
        if let Some((_, view)) = self.popup_view() {
            match view.option_at(pos) {
                Some(index) => self.highlight(index),
                None if !view.rect.contains(pos) => self.close_popup(out),
                None => {}
            }
            return true;
        }
        let Some(id) = self.hit(pos) else {
            return false;
        };
        self.set_pressed(Some(id));
        self.set_focus(Some(id), false);
        match self.nodes[id.index()].widget {
            Widget::Slider(_) => self.drag_slider(id, pos, out),
            Widget::TextField(_) => self.place_caret(id, pos),
            _ => {}
        }
        true
    }

    pub(crate) fn pointer_up(&mut self, pos: Point, out: &mut Vec<Action>) -> bool {
        if let Some((_, view)) = self.popup_view() {
            if let Some(index) = view.option_at(pos) {
                self.choose(index, out);
            }
            self.set_pressed(None);
            return true;
        }
        let Some(id) = self.input.pressed else {
            return false;
        };
        self.set_pressed(None);
        if self.hit(pos) == Some(id) {
            self.activate(id, out);
        }
        true
    }

    pub(crate) fn touch_down(&mut self, id: u64, pos: Point, out: &mut Vec<Action>) -> bool {
        if self.input.touch.is_some() {
            return false;
        }
        self.input.touch = Some(id);
        self.set_hover(None);
        self.pointer_down(pos, out)
    }

    pub(crate) fn touch_up(&mut self, id: u64, pos: Point, out: &mut Vec<Action>) -> bool {
        if self.input.touch != Some(id) {
            return false;
        }
        self.input.touch = None;
        self.pointer_up(pos, out)
    }

    pub(crate) fn touch_cancel(&mut self, id: u64) -> bool {
        if self.input.touch != Some(id) {
            return false;
        }
        self.input.touch = None;
        let had_press = self.input.pressed.is_some();
        self.set_pressed(None);
        had_press
    }

    pub(crate) fn scroll(&mut self, pos: Point, lines: f32) -> bool {
        let (Some(mut popup), Some((id, view))) = (self.input.popup, self.popup_view()) else {
            return false;
        };
        if !view.rect.contains(pos) || !lines.is_finite() {
            return false;
        }
        let count = self.select(id).map_or(0, |s| s.options.len());
        let last = count.saturating_sub(MAX_VISIBLE_OPTIONS) as f32;
        popup.scroll = (popup.scroll as f32 + lines.round()).clamp(0.0, last) as usize;
        if self.input.popup != Some(popup) {
            self.touch_popup();
            self.input.popup = Some(popup);
        }
        true
    }
}

/// Adjusts the scroll offset so that the highlighted option is among the visible ones.
fn keep_visible(popup: &mut Popup) {
    if popup.highlight < popup.scroll {
        popup.scroll = popup.highlight;
    } else if popup.highlight >= popup.scroll + MAX_VISIBLE_OPTIONS {
        popup.scroll = popup.highlight + 1 - MAX_VISIBLE_OPTIONS;
    }
}
