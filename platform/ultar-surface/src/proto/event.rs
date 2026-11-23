//! Messages from the compositor to a client.
//!
//! Positions are logical points in fixed point ([`FIXED_ONE`] = 1 point),
//! relative to the surface's content.

use super::types::*;
use crate::wire::{Reader, WireError, Writer};

/// Why the compositor ends a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// A message could not be decoded.
    Malformed,
    /// A message named an object that does not exist or exists already.
    UnknownObject,
    /// A limit (count, size, memory) was exceeded.
    Limit,
    /// The client is not allowed to do this.
    Permission,
    /// A buffer is unusable (not sealed, too small, wrong size).
    BadBuffer,
    /// The client did not read its events and fell too far behind.
    TooSlow,
}

impl ErrorCode {
    fn to_u8(self) -> u8 {
        match self {
            ErrorCode::Malformed => 0,
            ErrorCode::UnknownObject => 1,
            ErrorCode::Limit => 2,
            ErrorCode::Permission => 3,
            ErrorCode::BadBuffer => 4,
            ErrorCode::TooSlow => 5,
        }
    }

    pub(crate) fn from_u8(v: u8) -> Result<ErrorCode, WireError> {
        Ok(match v {
            0 => ErrorCode::Malformed,
            1 => ErrorCode::UnknownObject,
            2 => ErrorCode::Limit,
            3 => ErrorCode::Permission,
            4 => ErrorCode::BadBuffer,
            5 => ErrorCode::TooSlow,
            _ => return Err(WireError::BadValue),
        })
    }
}

/// Description of one output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputInfo {
    /// Id.
    pub id: OutputId,
    /// Position in the global layout, logical points.
    pub x: i32,
    /// Position in the global layout, logical points.
    pub y: i32,
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
    /// Device pixels per logical point × 1000 (1000 – 3000).
    pub scale_milli: u32,
    /// Refresh rate in millihertz.
    pub refresh_millihertz: u32,
    /// The primary output (bar, new windows, login).
    pub primary: bool,
    /// Connector name (technical).
    pub name: String,
}

/// What the client should draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Configure {
    /// The surface.
    pub surface: SurfaceId,
    /// Serial to acknowledge with the commit of matching content.
    pub serial: u32,
    /// Content width in logical points.
    pub width: u32,
    /// Content height in logical points.
    pub height: u32,
    /// Draw at this scale (device pixels per point × 1000).
    pub scale_milli: u32,
    /// Window state.
    pub state: WindowState,
    /// The window is the active one (keyboard focus).
    pub activated: bool,
    /// The layout direction is right-to-left (Arabic).
    pub rtl: bool,
}

/// An event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Answer to `Hello`.
    Welcome {
        /// Protocol version of the compositor.
        version: u16,
        /// How to send pixels.
        transport: Transport,
        /// Granted rights.
        rights: Rights,
        /// Memory the client may use for buffers.
        max_buffer_bytes: u64,
    },
    /// An output appeared or changed.
    Output(OutputInfo),
    /// An output disappeared.
    OutputRemoved {
        /// The output.
        output: OutputId,
    },
    /// Size, scale and state to draw.
    Configure(Configure),
    /// The compositor is done reading a buffer.
    BufferReleased {
        /// The buffer.
        buffer: BufferId,
    },
    /// The frame asked for with `Frame` is on the screen.
    FrameDone {
        /// The surface.
        surface: SurfaceId,
        /// Presentation time in milliseconds (compositor clock).
        time_ms: u32,
    },
    /// The pointer entered the surface.
    PointerEnter {
        /// The surface.
        surface: SurfaceId,
        /// Position (fixed point).
        x: i32,
        /// Position (fixed point).
        y: i32,
    },
    /// The pointer left the surface.
    PointerLeave {
        /// The surface.
        surface: SurfaceId,
    },
    /// The pointer moved.
    PointerMotion {
        /// The surface.
        surface: SurfaceId,
        /// Position (fixed point).
        x: i32,
        /// Position (fixed point).
        y: i32,
    },
    /// A button went down or up.
    PointerButton {
        /// The surface.
        surface: SurfaceId,
        /// Linux button code (`0x110` = primary).
        button: u16,
        /// Pressed or released.
        pressed: bool,
        /// Serial for requests made in response.
        serial: u32,
    },
    /// Scrolling (fixed point, logical points; positive = down/end).
    PointerAxis {
        /// The surface.
        surface: SurfaceId,
        /// Horizontal amount.
        dx: i32,
        /// Vertical amount.
        dy: i32,
    },
    /// Keyboard focus came or went.
    KeyboardFocus {
        /// The surface.
        surface: SurfaceId,
        /// Focused now.
        focused: bool,
    },
    /// A key changed.
    Key {
        /// The surface.
        surface: SurfaceId,
        /// Linux key code.
        code: u16,
        /// 0 released, 1 pressed, 2 repeated.
        state: u8,
        /// Modifiers held.
        modifiers: Modifiers,
        /// Character of the active layout (0 = none).
        character: u32,
        /// Serial for requests made in response.
        serial: u32,
    },
    /// A finger touched the surface.
    TouchDown {
        /// The surface.
        surface: SurfaceId,
        /// Contact id.
        id: u32,
        /// Position (fixed point).
        x: i32,
        /// Position (fixed point).
        y: i32,
        /// Serial.
        serial: u32,
    },
    /// A finger moved.
    TouchMove {
        /// The surface.
        surface: SurfaceId,
        /// Contact id.
        id: u32,
        /// Position (fixed point).
        x: i32,
        /// Position (fixed point).
        y: i32,
    },
    /// A finger lifted.
    TouchUp {
        /// The surface.
        surface: SurfaceId,
        /// Contact id.
        id: u32,
    },
    /// All touches of the surface were taken away (gesture).
    TouchCancel {
        /// The surface.
        surface: SurfaceId,
    },
    /// Text from the input method.
    Text {
        /// The surface.
        surface: SurfaceId,
        /// The text.
        text: String,
    },
    /// The user asked to close the window (title bar, Alt+F4).
    CloseRequested {
        /// The surface.
        surface: SurfaceId,
    },
    /// The clipboard offers these types (sent to the focused client).
    SelectionOffer {
        /// Offered types (empty = clipboard empty).
        mimes: Vec<String>,
    },
    /// Send the next chunk (one `SendData`, ≤ `MAX_CHUNK`) of the clipboard
    /// or drag data of this type for a transfer. Sent again for every next
    /// chunk when the reader has room (flow control); chunks sent unasked
    /// are held only up to a small bound, then the transfer is cancelled.
    SendRequest {
        /// The transfer.
        transfer: TransferId,
        /// Wanted type.
        mime: String,
    },
    /// Data of a transfer this client asked for.
    Data {
        /// The transfer.
        transfer: TransferId,
        /// A chunk.
        data: Vec<u8>,
        /// Last chunk.
        last: bool,
    },
    /// A transfer ended without data (source gone, limit).
    TransferCancelled {
        /// The transfer.
        transfer: TransferId,
    },
    /// Something was dropped on the surface.
    DropOffer {
        /// The surface.
        surface: SurfaceId,
        /// Position (fixed point).
        x: i32,
        /// Position (fixed point).
        y: i32,
        /// Offered types.
        mimes: Vec<String>,
    },
    /// Answer with `Pong` (the window is marked "not responding" after 5 s).
    Ping {
        /// Serial to echo.
        serial: u32,
    },
    /// Show or hide the on-screen keyboard (input method only).
    ImeState {
        /// A text field has the focus.
        active: bool,
        /// Kind of field.
        purpose: u8,
    },
    /// A piece of a captured output (capture right only).
    CaptureData {
        /// The output.
        output: OutputId,
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// First row in this chunk.
        row: u32,
        /// Rows of `width * 4` bytes, `0xXXRRGGBB` little-endian.
        data: Vec<u8>,
        /// Last chunk.
        last: bool,
    },
    /// The connection is closed after this.
    Error {
        /// Why.
        code: ErrorCode,
    },
    /// Answer to `RequestActivation` (single use, valid for a short time).
    ActivationToken {
        /// The token.
        token: u64,
    },
    /// The session was locked or unlocked (sent to shell and lock screen:
    /// start the lock screen when it is missing).
    SessionLocked {
        /// Locked now.
        locked: bool,
    },
}

impl Event {
    /// Opcode on the wire.
    pub fn opcode(&self) -> u16 {
        match self {
            Event::Welcome { .. } => 1,
            Event::Output(_) => 2,
            Event::OutputRemoved { .. } => 3,
            Event::Configure(_) => 4,
            Event::BufferReleased { .. } => 5,
            Event::FrameDone { .. } => 6,
            Event::PointerEnter { .. } => 7,
            Event::PointerLeave { .. } => 8,
            Event::PointerMotion { .. } => 9,
            Event::PointerButton { .. } => 10,
            Event::PointerAxis { .. } => 11,
            Event::KeyboardFocus { .. } => 12,
            Event::Key { .. } => 13,
            Event::TouchDown { .. } => 14,
            Event::TouchMove { .. } => 15,
            Event::TouchUp { .. } => 16,
            Event::TouchCancel { .. } => 17,
            Event::Text { .. } => 18,
            Event::CloseRequested { .. } => 19,
            Event::SelectionOffer { .. } => 20,
            Event::SendRequest { .. } => 21,
            Event::Data { .. } => 22,
            Event::TransferCancelled { .. } => 23,
            Event::DropOffer { .. } => 24,
            Event::Ping { .. } => 25,
            Event::ImeState { .. } => 26,
            Event::CaptureData { .. } => 27,
            Event::Error { .. } => 28,
            Event::ActivationToken { .. } => 29,
            Event::SessionLocked { .. } => 30,
        }
    }

    /// Encodes the event as one frame.
    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        let mut w = Writer::new(self.opcode());
        super::event_codec::encode_body(self, &mut w);
        w.finish()
    }

    /// Decodes one event body.
    pub fn decode(opcode: u16, body: &[u8]) -> Result<Event, WireError> {
        let mut r = Reader::new(body);
        let event = super::event_codec::decode_body(opcode, &mut r)?;
        r.end()?;
        Ok(event)
    }

    pub(crate) fn error_code(code: ErrorCode) -> u8 {
        code.to_u8()
    }
}
