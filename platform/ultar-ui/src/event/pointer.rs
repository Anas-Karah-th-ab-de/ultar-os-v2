//! Pointer and touch handling, activation and the select popup.

use super::{Action, ActionKind};
use crate::geom::Point;
use crate::ui::{NodeId, Pointer, Popup, Press, Ui};
use crate::widgets::{Select, Widget};

impl Ui {
    /// The topmost operable widget under `pos`. While a dialog is open only its widgets
    /// count; widgets inside a scroll container count only where they are visible.
    pub(crate) fn hit(&self, pos: Point) -> Option<NodeId> {
        self.reachable()
            .iter()
            .rev()
            .copied()
            .find(|id| self.interactive(*id) && !self.is_disabled(*id) && self.visible_at(*id, pos))
    }

    /// The part of `id` under `pos`: a tab, a radio option, the clear button of a search
    /// field. `None` for widgets without parts.
    pub(crate) fn part_at(&self, id: NodeId, pos: Point) -> Option<usize> {
        let rect = self.rect(id)?;
        let direction = self.env.direction;
        match &self.nodes[id.index()].widget {
            Widget::Tabs(tabs) => tabs.part_at(rect, pos, direction),
            Widget::RadioGroup(group) => group.part_at(rect, pos, self.mode),
            Widget::Table(table) => table.part_at(rect, pos, self.mode, direction),
            Widget::Tree(tree) => tree.part_at(rect, pos, self.mode, direction),
            Widget::TextField(field) => field.part_at(field.field(rect, self.mode), pos, direction),
            _ => None,
        }
    }

    pub(crate) fn set_hover(&mut self, hover: Option<NodeId>) {
        if self.input.hover != hover {
            let previous = std::mem::replace(&mut self.input.hover, hover);
            self.input.hover_part = None;
            for id in [previous, hover].into_iter().flatten() {
                self.touch(id);
            }
            self.hide_tooltip();
            self.watch_hover(hover);
        }
    }

    /// Hit-tests the resting mouse pointer again after the content moved under it (scroll,
    /// layout), so that hover and tooltip follow what is drawn there now.
    pub(crate) fn rehover(&mut self) {
        let Some(pos) = self.input.pointer else {
            return;
        };
        if self.input.popup.is_some() || self.layers.menu.is_some() {
            return;
        }
        let hit = self.hit(pos);
        self.set_hover(hit);
        let part = hit.and_then(|id| self.part_at(id, pos));
        self.set_hover_part(part);
    }

    fn set_hover_part(&mut self, part: Option<usize>) {
        if self.input.hover_part != part {
            self.input.hover_part = part;
            if let Some(id) = self.input.hover {
                self.touch(id);
            }
        }
    }

    /// Sets what `pointer` holds down.
    pub(crate) fn set_press(&mut self, pointer: Pointer, press: Option<Press>) {
        let slot = match pointer {
            Pointer::Mouse => &mut self.input.mouse,
            Pointer::Touch => &mut self.input.finger,
        };
        if *slot != press {
            let previous = std::mem::replace(slot, press);
            for id in [previous, press].into_iter().flatten().map(|p| p.node) {
                self.touch(id);
            }
        }
    }

    /// Moves focus; `visible` shows the focus ring (keyboard focus). Keyboard focus scrolls
    /// the widget into view and shows its tooltip at once; a press never moves the content
    /// under the pointer.
    pub(crate) fn set_focus(&mut self, focus: Option<NodeId>, visible: bool) {
        if self.input.focus != focus || self.input.focus_visible != visible {
            let previous = std::mem::replace(&mut self.input.focus, focus);
            self.input.focus_visible = visible;
            if let Some(left) = previous.filter(|p| Some(*p) != focus) {
                // A password shown by keyboard is hidden when its field loses the focus.
                if self
                    .layers
                    .gestures
                    .reveal
                    .is_some_and(|r| r.0 == left && r.1.is_none())
                {
                    self.hide_password();
                }
                self.set_eye_focus(left, false);
            }
            if focus.is_some() {
                self.input.focus_hint = None;
            }
            for id in [previous, focus].into_iter().flatten() {
                self.touch(id);
            }
            self.hide_tooltip();
            if let (Some(id), true) = (focus, visible) {
                self.reveal(id, &mut Vec::new());
                self.show_tooltip(id);
            }
        }
    }

    /// Number of options the open list shows at once.
    fn popup_rows(&self, id: NodeId) -> usize {
        let (Some(rect), Ok(select)) = (self.rect(id), self.select(id)) else {
            return 0;
        };
        let field = Select::field(rect, self.mode);
        select.popup(field, self.size(), self.mode, 0).count
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
        let rows = self.popup_rows(id);
        keep_visible(&mut popup, rows);
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
            self.rehover();
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
        let rows = self.popup_rows(popup.select);
        keep_visible(&mut popup, rows);
        if self.input.popup != Some(popup) {
            self.touch_popup();
            self.input.popup = Some(popup);
            self.touch_popup();
        }
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

    fn popup_move(&mut self, pos: Point) -> Option<bool> {
        let (_, view) = self.popup_view()?;
        // The open list is modal: nothing beneath it is hovered.
        self.set_hover(None);
        if let Some(index) = view.option_at(pos) {
            self.highlight(index);
        }
        Some(view.rect.contains(pos))
    }

    pub(crate) fn pointer_move(
        &mut self,
        pos: Point,
        pointer: Pointer,
        out: &mut Vec<Action>,
    ) -> bool {
        if self.layers.menu.is_some() {
            return self.menu_hover(pos);
        }
        if let Some(inside) = self.popup_move(pos) {
            return inside;
        }
        if self.drag_move(pos, pointer, out) || self.gesture_move(pos, pointer, out) {
            return true;
        }
        if pointer == Pointer::Mouse {
            let hit = self.hit(pos);
            self.set_hover(hit);
            let part = hit.and_then(|id| self.part_at(id, pos));
            self.set_hover_part(part);
        }
        match self.input.press(pointer) {
            Some(press) if matches!(self.nodes[press.node.index()].widget, Widget::Slider(_)) => {
                self.drag_slider(press.node, pos, out);
                true
            }
            press => press.is_some(),
        }
    }

    fn popup_down(&mut self, pos: Point, out: &mut Vec<Action>) -> bool {
        let Some((_, view)) = self.popup_view() else {
            return false;
        };
        match view.option_at(pos) {
            Some(index) => self.highlight(index),
            None if !view.rect.contains(pos) => self.close_popup(out),
            None => {}
        }
        true
    }

    pub(crate) fn pointer_down(
        &mut self,
        pos: Point,
        pointer: Pointer,
        out: &mut Vec<Action>,
    ) -> bool {
        if self.layers.menu.is_some() {
            return self.menu_press(pos, out);
        }
        if self.toast_press(pos, out) {
            return true;
        }
        if self.popup_down(pos, out) {
            return true;
        }
        self.hide_tooltip();
        self.layers.hover_since = None;
        if self.drag_start(pos, pointer) {
            return true;
        }
        let Some(id) = self.hit(pos) else {
            return self.modal_press(pos, out);
        };
        let part = self.part_at(id, pos);
        self.set_press(pointer, Some(Press { node: id, part }));
        self.set_focus(Some(id), false);
        self.gesture_down(id, (pos, part), pointer, out);
        match &self.nodes[id.index()].widget {
            Widget::Slider(_) => self.drag_slider(id, pos, out),
            Widget::TextField(_) if part.is_none() => self.place_caret(id, pos),
            _ => {}
        }
        true
    }

    pub(crate) fn pointer_up(
        &mut self,
        pos: Point,
        pointer: Pointer,
        out: &mut Vec<Action>,
    ) -> bool {
        if self.gesture_up(pointer, Some(pos), out) {
            self.set_press(pointer, None);
            return true;
        }
        if self.layers.menu.is_some() {
            self.set_press(pointer, None);
            return self.menu_release(pos, out);
        }
        if let Some((_, view)) = self.popup_view() {
            if let Some(index) = view.option_at(pos) {
                self.choose(index, out);
            }
            self.set_press(pointer, None);
            return true;
        }
        if self.drag_end(pointer) {
            return true;
        }
        let Some(press) = self.input.press(pointer) else {
            return self.open_dialog_node().is_some();
        };
        self.set_press(pointer, None);
        if self.hit(pos) == Some(press.node) && self.part_at(press.node, pos) == press.part {
            let before = out.len();
            self.activate(press.node, press.part, out);
            // Rows of a table or tree count even when the row was selected already: the
            // second click of a double click on it changes nothing else.
            let rows = matches!(
                self.widget(press.node),
                Some(Widget::Table(_) | Widget::Tree(_))
            );
            if out.len() > before || rows {
                self.detect_double(press.node, pos, out);
            }
        }
        true
    }

    /// The secondary button: opens the context menu of the node under the pointer.
    /// A second secondary click beside an open menu closes it and opens the menu of the
    /// new place at once.
    pub(crate) fn secondary_down(&mut self, pos: Point, out: &mut Vec<Action>) -> bool {
        if self.layers.menu.is_some() {
            if self.menu_contains(pos) {
                return self.menu_press(pos, out);
            }
            self.close_menu_with(out);
        }
        if self.input.popup.is_some() {
            self.close_popup(out);
            return true;
        }
        match self.node_at(pos) {
            Some(id) => self.open_registered_menu(id, Some(pos), out),
            None => false,
        }
    }

    pub(crate) fn touch_down(&mut self, id: u64, pos: Point, out: &mut Vec<Action>) -> bool {
        if self.input.touch.is_some() {
            return false;
        }
        self.input.touch = Some(id);
        self.set_hover(None);
        let used = self.pointer_down(pos, Pointer::Touch, out);
        self.watch_long_press(id, pos);
        used
    }

    pub(crate) fn touch_up(&mut self, id: u64, pos: Point, out: &mut Vec<Action>) -> bool {
        if self.input.touch != Some(id) {
            return false;
        }
        self.input.touch = None;
        self.pointer_up(pos, Pointer::Touch, out)
    }

    pub(crate) fn touch_cancel(&mut self, id: u64, out: &mut Vec<Action>) -> bool {
        if self.input.touch != Some(id) {
            return false;
        }
        self.input.touch = None;
        let gesture = self.gesture_up(Pointer::Touch, None, out);
        let had_press = self.input.finger.is_some() || self.drag_end(Pointer::Touch) || gesture;
        self.set_press(Pointer::Touch, None);
        had_press
    }
}

/// Adjusts the scroll offset so that the highlighted option is among the `rows` visible ones.
fn keep_visible(popup: &mut Popup, rows: usize) {
    let rows = rows.max(1);
    if popup.highlight < popup.scroll {
        popup.scroll = popup.highlight;
    } else if popup.highlight >= popup.scroll + rows {
        popup.scroll = popup.highlight + 1 - rows;
    }
}
