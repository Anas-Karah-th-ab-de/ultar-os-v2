//! Scrolling by wheel, touch drag, scrollbar and keyboard.

use super::{Action, Key};
use crate::geom::Point;
use crate::tokens::size;
use crate::ui::{Drag, DragKind, NodeId, Pointer, Ui};
use crate::widgets::{Scroll, Widget};

/// Points one wheel line scrolls.
const LINE: f32 = size::SCROLL_LINE;
/// Distance a finger travels before a touch means "scroll" instead of "tap".
const SLOP: f32 = size::TOUCH_SLOP;

impl Ui {
    /// Wheel or two-finger scroll at `pos`.
    pub(crate) fn scroll(&mut self, pos: Point, lines: f32, out: &mut Vec<Action>) -> bool {
        if !lines.is_finite() {
            return false;
        }
        if self.layers.menu.is_some() {
            self.menu_wheel(pos, lines * LINE);
            return true;
        }
        if let (Some(mut popup), Some((id, view))) = (self.input.popup, self.popup_view()) {
            if !view.rect.contains(pos) {
                return false;
            }
            let count = self.select(id).map_or(0, |s| s.options.len());
            let last = count.saturating_sub(view.count) as f32;
            popup.scroll = (popup.scroll as f32 + lines.round()).clamp(0.0, last) as usize;
            if self.input.popup != Some(popup) {
                self.touch_popup();
                self.input.popup = Some(popup);
            }
            return true;
        }
        // The innermost container that can still move in that direction takes the wheel.
        for id in self.scrollers_at(pos) {
            let offset = self.scroller(id).map_or(0.0, Scroll::offset);
            if self.scroll_to(id, offset + lines * LINE, out) {
                return true;
            }
        }
        false
    }

    /// A press on a scrollbar starts dragging its thumb (a press beside the thumb first
    /// brings the thumb under the pointer); a finger on scrollable content may become a
    /// scroll drag. Returns `true` only for the scrollbar: content presses go on to widgets.
    pub(crate) fn drag_start(&mut self, pos: Point, pointer: Pointer) -> bool {
        if self.layers.drag.is_some() {
            // One drag at a time: the other pointer goes on to the widgets.
            return false;
        }
        let direction = self.env.direction;
        for id in self.scrollers_at(pos) {
            let (Some(view), Ok(scroll)) = (self.rect(id), self.scroller(id)) else {
                continue;
            };
            let Some(thumb) = scroll.thumb(view, direction) else {
                continue;
            };
            if Scroll::grip(view, direction).contains(pos) {
                let on_thumb = pos.y >= thumb.y && pos.y < thumb.bottom();
                let grab = if on_thumb {
                    pos.y - thumb.y
                } else {
                    thumb.h / 2.0
                };
                let kind = DragKind::Thumb { grab };
                self.layers.drag = Some(Drag {
                    scroller: id,
                    kind,
                    pointer,
                });
                self.drag_move(pos, pointer, &mut Vec::new());
                self.touch(id);
                return true;
            }
            if pointer == Pointer::Touch {
                let kind = DragKind::Finger {
                    start_y: pos.y,
                    start_offset: scroll.offset(),
                    active: false,
                };
                self.layers.drag = Some(Drag {
                    scroller: id,
                    kind,
                    pointer,
                });
                return false;
            }
        }
        false
    }

    /// Moves a drag in progress. Returns `true` when the movement was a scroll.
    pub(crate) fn drag_move(
        &mut self,
        pos: Point,
        pointer: Pointer,
        out: &mut Vec<Action>,
    ) -> bool {
        let Some(drag) = self.layers.drag.filter(|drag| drag.pointer == pointer) else {
            return false;
        };
        let id = drag.scroller;
        match drag.kind {
            DragKind::Thumb { grab } => {
                let (Some(view), Ok(scroll)) = (self.rect(id), self.scroller(id)) else {
                    return false;
                };
                let offset = scroll.offset_for_thumb(view, pos.y - grab);
                self.scroll_to(id, offset, out);
                true
            }
            DragKind::Finger {
                start_y,
                start_offset,
                active,
            } => {
                let holds_slider = self.input.finger.is_some_and(|press| {
                    matches!(self.nodes[press.node.index()].widget, Widget::Slider(_))
                });
                let moved = (pos.y - start_y).abs() > SLOP;
                if !active && (!moved || holds_slider) {
                    return false;
                }
                if !active {
                    // From here on the touch scrolls: the widget under it is not tapped.
                    self.set_press(Pointer::Touch, None);
                    let kind = DragKind::Finger {
                        start_y,
                        start_offset,
                        active: true,
                    };
                    self.layers.drag = Some(Drag { kind, ..drag });
                    self.touch(id);
                }
                self.scroll_to(id, start_offset + (start_y - pos.y), out);
                true
            }
        }
    }

    /// Ends the drag of `pointer`. Returns `true` when a scroll drag was in progress (the
    /// release then activates nothing).
    pub(crate) fn drag_end(&mut self, pointer: Pointer) -> bool {
        let Some(drag) = self.layers.drag.filter(|drag| drag.pointer == pointer) else {
            return false;
        };
        let scrolled = match drag.kind {
            DragKind::Thumb { .. } => true,
            DragKind::Finger { active, .. } => active,
        };
        self.layers.drag = None;
        self.touch(drag.scroller);
        scrolled
    }

    /// Scrolls the container around `from` by keyboard: Page Up/Down by a viewport, Home/End
    /// to the ends, and – when `arrows` – Up/Down by a line.
    pub(crate) fn scroll_key(
        &mut self,
        from: NodeId,
        key: Key,
        arrows: bool,
        out: &mut Vec<Action>,
    ) -> bool {
        let Some(id) = self.scroller_of(from) else {
            return false;
        };
        let (Some(view), Ok(scroll)) = (self.rect(id), self.scroller(id)) else {
            return false;
        };
        let page = (view.h - LINE).max(LINE);
        let offset = scroll.offset();
        let target = match key {
            Key::PageDown => offset + page,
            Key::PageUp => offset - page,
            Key::Home => 0.0,
            Key::End => scroll.max_offset(view.h),
            Key::ArrowDown if arrows => offset + LINE,
            Key::ArrowUp if arrows => offset - LINE,
            _ => return false,
        };
        self.scroll_to(id, target, out)
    }
}
