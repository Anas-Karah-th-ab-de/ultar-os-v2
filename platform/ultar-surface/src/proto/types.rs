//! Values shared by requests and events.

use crate::wire::{Reader, WireError, Writer};

/// Protocol version spoken by this crate.
pub const PROTOCOL_VERSION: u16 = 1;
/// Longest application id.
pub const MAX_APP_ID: usize = 64;
/// Longest window title in bytes (longer titles are cut by the sender).
pub const MAX_TITLE: usize = 512;
/// Longest MIME type.
pub const MAX_MIME: usize = 64;
/// Most MIME types offered at once.
pub const MAX_MIMES: usize = 8;
/// Most damage rectangles in one request.
pub const MAX_DAMAGE: usize = 32;
/// Largest chunk of clipboard/drag data or captured pixels in one message.
pub const MAX_CHUNK: usize = 1 << 20;
/// Longest text an input method commits at once.
pub const MAX_IME_TEXT: usize = 4096;
/// Largest surface side in pixels.
pub const MAX_SIDE: u32 = 8192;
/// Fixed-point factor of positions (1/256 of a logical point).
pub const FIXED_ONE: i32 = 256;

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);
    };
}

id_type!(
    /// A surface, chosen by the client (unique per connection, not 0).
    SurfaceId
);
id_type!(
    /// A buffer, chosen by the client (unique per connection, not 0).
    BufferId
);
id_type!(
    /// An output (display), chosen by the compositor.
    OutputId
);
id_type!(
    /// A data transfer (clipboard, drop), chosen by the compositor.
    TransferId
);

/// Pixel layout of a buffer; little-endian 32-bit words `0xAARRGGBB`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// Opaque; the top byte is ignored.
    Xrgb8888,
    /// Premultiplied alpha.
    Argb8888,
}

impl PixelFormat {
    pub(crate) fn encode(self, w: &mut Writer) {
        w.u8(match self {
            PixelFormat::Xrgb8888 => 0,
            PixelFormat::Argb8888 => 1,
        });
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<PixelFormat, WireError> {
        match r.u8()? {
            0 => Ok(PixelFormat::Xrgb8888),
            1 => Ok(PixelFormat::Argb8888),
            _ => Err(WireError::BadValue),
        }
    }
}

/// A rectangle in buffer pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    /// First column.
    pub x: i32,
    /// First row.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl Rect {
    /// A rectangle.
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    pub(crate) fn encode(&self, w: &mut Writer) {
        w.i32(self.x).i32(self.y).u32(self.width).u32(self.height);
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<Rect, WireError> {
        Ok(Rect {
            x: r.i32()?,
            y: r.i32()?,
            width: r.u32()?,
            height: r.u32()?,
        })
    }
}

/// The edge of an output a panel is attached to (logical: start/end follow
/// the reading direction).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edge {
    /// Top edge.
    Top,
    /// Bottom edge.
    Bottom,
    /// Start edge (right in Arabic).
    Start,
    /// End edge (left in Arabic).
    End,
}

/// What a surface is for. Everything but [`Role::Toplevel`] and
/// [`Role::Popup`] needs a right (see [`Rights`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// An application window with compositor decorations.
    Toplevel,
    /// A menu or tooltip attached to another surface of the same client,
    /// at a position relative to it (logical points).
    Popup {
        /// The surface it belongs to.
        parent: SurfaceId,
        /// Horizontal offset.
        x: i32,
        /// Vertical offset.
        y: i32,
    },
    /// A shell bar or dock along an edge, reserving `size` logical points.
    Panel {
        /// The edge.
        edge: Edge,
        /// Thickness in logical points.
        size: u32,
    },
    /// The on-screen keyboard.
    InputPanel,
    /// The lock screen: covers everything and takes all input.
    Lock,
    /// The desktop background.
    Background,
    /// Notifications and other surfaces above windows.
    Overlay,
    /// A system dialog (password, permission prompt): above every window,
    /// everything else dimmed, title bar with the compositor's lock mark
    /// that no app can get.
    SystemDialog,
}

impl Role {
    pub(crate) fn encode(&self, w: &mut Writer) {
        match *self {
            Role::Toplevel => {
                w.u8(0);
            }
            Role::Popup { parent, x, y } => {
                w.u8(1).u32(parent.0).i32(x).i32(y);
            }
            Role::Panel { edge, size } => {
                let e = match edge {
                    Edge::Top => 0,
                    Edge::Bottom => 1,
                    Edge::Start => 2,
                    Edge::End => 3,
                };
                w.u8(2).u8(e).u32(size);
            }
            Role::InputPanel => {
                w.u8(3);
            }
            Role::Lock => {
                w.u8(4);
            }
            Role::Background => {
                w.u8(5);
            }
            Role::Overlay => {
                w.u8(6);
            }
            Role::SystemDialog => {
                w.u8(7);
            }
        }
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<Role, WireError> {
        Ok(match r.u8()? {
            0 => Role::Toplevel,
            1 => Role::Popup {
                parent: SurfaceId(r.u32()?),
                x: r.i32()?,
                y: r.i32()?,
            },
            2 => {
                let edge = match r.u8()? {
                    0 => Edge::Top,
                    1 => Edge::Bottom,
                    2 => Edge::Start,
                    3 => Edge::End,
                    _ => return Err(WireError::BadValue),
                };
                Role::Panel {
                    edge,
                    size: r.u32()?,
                }
            }
            3 => Role::InputPanel,
            4 => Role::Lock,
            5 => Role::Background,
            6 => Role::Overlay,
            7 => Role::SystemDialog,
            _ => return Err(WireError::BadValue),
        })
    }

    /// The right a client needs to create a surface of this role.
    pub fn required_rights(&self) -> Rights {
        match self {
            Role::Toplevel | Role::Popup { .. } => Rights::NONE,
            Role::Panel { .. } | Role::Background => Rights::SHELL,
            Role::InputPanel => Rights::INPUT_METHOD,
            Role::Lock => Rights::LOCK_SCREEN,
            Role::Overlay => Rights::OVERLAY,
            Role::SystemDialog => Rights::SYSTEM_DIALOG,
        }
    }
}

/// State of a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowState {
    /// Free position and size.
    Normal,
    /// Fills the work area of its output.
    Maximized,
    /// Fills its output, no decorations.
    Fullscreen,
    /// Hidden; shown again from the dock.
    Minimized,
}

impl WindowState {
    pub(crate) fn encode(self, w: &mut Writer) {
        w.u8(match self {
            WindowState::Normal => 0,
            WindowState::Maximized => 1,
            WindowState::Fullscreen => 2,
            WindowState::Minimized => 3,
        });
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<WindowState, WireError> {
        match r.u8()? {
            0 => Ok(WindowState::Normal),
            1 => Ok(WindowState::Maximized),
            2 => Ok(WindowState::Fullscreen),
            3 => Ok(WindowState::Minimized),
            _ => Err(WireError::BadValue),
        }
    }
}

/// How pixels reach the compositor, chosen by the compositor in `Welcome`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Transport {
    /// Sealed shared memory passed as a descriptor (Linux).
    SharedMemory,
    /// Pixels inside the messages (kernels without memfd or `SCM_RIGHTS`).
    Inline,
}

/// Rights of a client, decided by the compositor from the app's manifest
/// (never from what the client claims).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rights(pub u32);

impl Rights {
    /// Ordinary app: windows and popups only.
    pub const NONE: Rights = Rights(0);
    /// Shell: panels, background, all windows in the overview.
    pub const SHELL: Rights = Rights(1);
    /// On-screen keyboard / input method.
    pub const INPUT_METHOD: Rights = Rights(2);
    /// Lock screen.
    pub const LOCK_SCREEN: Rights = Rights(4);
    /// Read the pixels of an output (screenshot, screen sharing).
    pub const CAPTURE: Rights = Rights(8);
    /// Surfaces above windows (notifications).
    pub const OVERLAY: Rights = Rights(16);
    /// System dialogs (password, permission prompts) with the trusted mark.
    pub const SYSTEM_DIALOG: Rights = Rights(32);
    /// Sets the output arrangement and learns each display's identity and
    /// modes (the display service).
    pub const OUTPUT_CONFIG: Rights = Rights(64);
    /// Every right.
    pub const ALL: Rights = Rights(127);

    /// True if every bit of `other` is set.
    pub const fn contains(self, other: Rights) -> bool {
        self.0 & other.0 == other.0
    }

    /// Both sets together.
    pub const fn union(self, other: Rights) -> Rights {
        Rights(self.0 | other.0)
    }
}

/// Modifier keys held (bit set).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(pub u8);

impl Modifiers {
    /// Shift.
    pub const SHIFT: u8 = 1;
    /// Control.
    pub const CTRL: u8 = 2;
    /// Alt.
    pub const ALT: u8 = 4;
    /// Super (the logo key).
    pub const SUPER: u8 = 8;
    /// Alt Gr.
    pub const ALT_GR: u8 = 16;
    /// Caps Lock is on (a state, not a held key: the lock screen shows a hint).
    pub const CAPS_LOCK: u8 = 32;

    /// True if the bit is held.
    pub const fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }
}

/// Edges a resize drags (bit set).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ResizeEdges(pub u8);

impl ResizeEdges {
    /// Top edge.
    pub const TOP: u8 = 1;
    /// Bottom edge.
    pub const BOTTOM: u8 = 2;
    /// Physical left edge.
    pub const LEFT: u8 = 4; // physical
    /// Physical right edge.
    pub const RIGHT: u8 = 8; // physical
}

pub(crate) fn encode_mimes(w: &mut Writer, mimes: &[String]) {
    let n = mimes.len().min(MAX_MIMES);
    w.u8(n as u8);
    for m in &mimes[..n] {
        w.str(m, MAX_MIME);
    }
}

pub(crate) fn decode_mimes(r: &mut Reader<'_>) -> Result<Vec<String>, WireError> {
    let n = usize::from(r.u8()?);
    if n > MAX_MIMES {
        return Err(WireError::TooLong);
    }
    (0..n).map(|_| r.str(MAX_MIME)).collect()
}
