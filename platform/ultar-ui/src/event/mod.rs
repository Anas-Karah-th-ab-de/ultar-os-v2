//! Input events, the widget state machine and the actions it emits.
//!
//! Positions are logical points with a physical origin (top-left). The platform translates
//! device input into [`Event`]s; the application reacts to the returned [`Action`]s.

mod activate;
mod arrange;
mod data_keys;
mod gesture;
mod keys;
mod pointer;
mod scroll_input;
mod selection;

pub(crate) use gesture::Gestures;
pub(crate) use keys::{next_match, starts_with};

use crate::geom::Point;
use crate::ui::{NodeId, Pointer, Ui};

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// The main button (left on a right-handed mouse).
    Primary,
    /// The context-menu button.
    Secondary,
    /// The wheel button.
    Middle,
}

/// Modifier keys held during a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub ctrl: bool,
    /// Alt.
    pub alt: bool,
    /// Super / logo key.
    pub meta: bool,
}

impl Modifiers {
    /// No modifier held.
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };
    /// Only shift held.
    pub const SHIFT: Modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
        meta: false,
    };

    /// `true` when a shortcut modifier (control, alt, super) is held.
    pub const fn command(self) -> bool {
        self.ctrl || self.alt || self.meta
    }
}

/// A key press (repeats arrive as further presses).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Tab: next widget; with shift: previous.
    Tab,
    /// Enter / Return.
    Enter,
    /// Space bar.
    Space,
    /// Escape.
    Escape,
    /// Physical left arrow.
    ArrowLeft,
    /// Physical right arrow.
    ArrowRight,
    /// Up arrow.
    ArrowUp,
    /// Down arrow.
    ArrowDown,
    /// Home.
    Home,
    /// End.
    End,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Page Up: scroll back by a viewport.
    PageUp,
    /// Page Down: scroll on by a viewport.
    PageDown,
    /// The menu key (or Shift+F10): open the context menu of the focused widget.
    Menu,
    /// A function key, `F(1)` to `F(12)` (F5: reload, F2: rename …; apps decide).
    F(u8),
    /// A character produced by the keyboard layout or input method.
    Char(char),
}

/// One input event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// The pointer moved.
    PointerMove {
        /// Position.
        pos: Point,
    },
    /// A pointer button went down.
    PointerDown {
        /// Position.
        pos: Point,
        /// Which button.
        button: PointerButton,
    },
    /// A pointer button was released.
    PointerUp {
        /// Position.
        pos: Point,
        /// Which button.
        button: PointerButton,
    },
    /// The pointer left the surface.
    PointerLeave,
    /// A finger touched the surface.
    TouchDown {
        /// Contact identifier.
        id: u64,
        /// Position.
        pos: Point,
    },
    /// A finger moved.
    TouchMove {
        /// Contact identifier.
        id: u64,
        /// Position.
        pos: Point,
    },
    /// A finger was lifted.
    TouchUp {
        /// Contact identifier.
        id: u64,
        /// Position.
        pos: Point,
    },
    /// The system took the touch away (gesture, palm rejection).
    TouchCancel {
        /// Contact identifier.
        id: u64,
    },
    /// A key was pressed.
    Key {
        /// The key.
        key: Key,
        /// Modifiers held.
        modifiers: Modifiers,
    },
    /// The modifier keys held changed (pointer events use the last state reported here or
    /// by a key event).
    Modifiers {
        /// Modifiers held now.
        modifiers: Modifiers,
    },
    /// The wheel or a two-finger scroll moved.
    Scroll {
        /// Pointer position.
        pos: Point,
        /// Lines to scroll; positive moves further down the content.
        lines: f32,
    },
}

/// What a widget did in response to input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActionKind {
    /// A button or list item was activated.
    Activated,
    /// A switch or checkbox changed to the given state.
    Toggled(bool),
    /// An option of a select or radio group, a tab, or an item of a selectable list (reported
    /// for the list) was chosen.
    Selected(usize),
    /// A slider moved to the given value.
    ValueChanged(f32),
    /// A text field's content changed.
    TextChanged,
    /// Enter was pressed in a text field.
    Submitted,
    /// A select's options list, a menu or a context menu opened.
    Opened,
    /// A select's options list or a menu closed; a dialog or sheet was closed by the user
    /// (Escape, or a press beside a sheet).
    Closed,
    /// The menu item with this identifier was chosen (reported for the menu's owner).
    MenuChosen(u32),
    /// A scroll container moved to this offset in points.
    Scrolled(f32),
    /// A second activation of the same node within the double-click time.
    DoubleActivated,
    /// A middle click (press and release of the middle button over the same node).
    MiddleClicked,
    /// The selection of a multi-selection list or table changed (Ctrl or Shift with a
    /// click or the keyboard); read it with `Ui::selected_rows`.
    SelectionChanged,
    /// A hold button went down.
    Pressed,
    /// A hold button is still held (every `motion.repeat` after `motion.long_press`).
    Repeated,
    /// A hold button was released (it reports no `Activated`).
    Released,
    /// A draggable node started to move with the pointer.
    DragStarted,
    /// A dragged node moved: the offset from where the drag started (physical points).
    Dragged(Point),
    /// A drag ended: total offset from the press, and the drop target under the pointer
    /// (`Ui::set_drop_target`), if any.
    DragEnded {
        /// Offset from where the drag started.
        delta: Point,
        /// The node the drag was dropped on.
        target: Option<crate::ui::NodeId>,
    },
    /// A table column header was chosen: column and ascending order.
    Sorted(usize, bool),
    /// A row of a table or an item of a tree was activated (Enter, double click).
    RowActivated(usize),
    /// A tree item was expanded (`true`) or collapsed.
    Expanded(usize, bool),
    /// The action button of the toast with this identifier was pressed.
    ToastAction(u32),
    /// The toast with this identifier went away (time, close button or Escape).
    ToastClosed(u32),
    /// Item `index` of an arrangement canvas was dragged to a new place
    /// (`Ui::arranged_rect` tells where).
    Arranged(usize),
}

/// An action emitted by a node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Action {
    /// The node that acted.
    pub node: NodeId,
    /// What it did.
    pub kind: ActionKind,
}

/// The result of handling one event.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Response {
    /// `true` when the UI used the event (the platform should not pass it on).
    pub consumed: bool,
    /// Actions emitted, in order (at most a handful per event). Actions caused by the clock
    /// (long press, held button, toast timeout) since the last event come first.
    pub actions: Vec<Action>,
    /// Modifier keys held while the event was handled (Ctrl/Shift with a click).
    pub modifiers: Modifiers,
}

impl Ui {
    /// Feeds one input event through the widget state machine.
    pub fn handle(&mut self, event: Event) -> Response {
        self.layout();
        let mut actions = std::mem::take(&mut self.pending);
        let consumed = match event {
            Event::PointerMove { pos } => {
                self.input.pointer = Some(pos);
                self.pointer_move(pos, Pointer::Mouse, &mut actions)
            }
            Event::PointerDown { pos, button } => {
                self.input.pointer = Some(pos);
                match button {
                    PointerButton::Primary => self.pointer_down(pos, Pointer::Mouse, &mut actions),
                    PointerButton::Secondary => self.secondary_down(pos, &mut actions),
                    PointerButton::Middle => {
                        self.input.middle = self.hit(pos).filter(|_| self.layers.menu.is_none());
                        self.input.middle.is_some()
                    }
                }
            }
            Event::PointerUp { pos, button } => match button {
                PointerButton::Primary => self.pointer_up(pos, Pointer::Mouse, &mut actions),
                PointerButton::Middle => self.middle_up(pos, &mut actions),
                PointerButton::Secondary => false,
            },
            Event::PointerLeave => self.pointer_leave(),
            Event::TouchDown { id, pos } => self.touch_down(id, pos, &mut actions),
            Event::TouchMove { id, pos } => {
                self.input.touch == Some(id) && self.pointer_move(pos, Pointer::Touch, &mut actions)
            }
            Event::TouchUp { id, pos } => self.touch_up(id, pos, &mut actions),
            Event::TouchCancel { id } => self.touch_cancel(id, &mut actions),
            Event::Key { key, modifiers } => {
                self.input.modifiers = modifiers;
                self.key(key, modifiers, &mut actions)
            }
            Event::Modifiers { modifiers } => {
                self.input.modifiers = modifiers;
                false
            }
            Event::Scroll { pos, lines } => self.scroll(pos, lines, &mut actions),
        };
        let modifiers = self.input.modifiers;
        Response {
            consumed,
            actions,
            modifiers,
        }
    }
}
