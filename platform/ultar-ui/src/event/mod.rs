//! Input events, the widget state machine and the actions it emits.
//!
//! Positions are logical points with a physical origin (top-left). The platform translates
//! device input into [`Event`]s; the application reacts to the returned [`Action`]s.

mod activate;
mod keys;
mod pointer;
mod scroll_input;

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
    /// Actions emitted, in order (at most a handful per event).
    pub actions: Vec<Action>,
}

impl Ui {
    /// Feeds one input event through the widget state machine.
    pub fn handle(&mut self, event: Event) -> Response {
        self.layout();
        let mut actions = Vec::new();
        let consumed = match event {
            Event::PointerMove { pos } => self.pointer_move(pos, Pointer::Mouse, &mut actions),
            Event::PointerDown { pos, button } => match button {
                PointerButton::Primary => self.pointer_down(pos, Pointer::Mouse, &mut actions),
                PointerButton::Secondary => self.secondary_down(pos, &mut actions),
                PointerButton::Middle => false,
            },
            Event::PointerUp { pos, button } => {
                button == PointerButton::Primary
                    && self.pointer_up(pos, Pointer::Mouse, &mut actions)
            }
            Event::PointerLeave => {
                self.set_hover(None);
                false
            }
            Event::TouchDown { id, pos } => self.touch_down(id, pos, &mut actions),
            Event::TouchMove { id, pos } => {
                self.input.touch == Some(id) && self.pointer_move(pos, Pointer::Touch, &mut actions)
            }
            Event::TouchUp { id, pos } => self.touch_up(id, pos, &mut actions),
            Event::TouchCancel { id } => self.touch_cancel(id),
            Event::Key { key, modifiers } => self.key(key, modifiers, &mut actions),
            Event::Scroll { pos, lines } => self.scroll(pos, lines, &mut actions),
        };
        Response { consumed, actions }
    }
}
