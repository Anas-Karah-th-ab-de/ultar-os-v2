//! Messages from a client to the compositor.

use super::types::*;
use crate::wire::{Reader, WireError, Writer};

/// A request. Requests that act on behalf of the user (moving a window,
/// clipboard, drag) carry the serial of the input event that caused them;
/// the compositor refuses them without a matching recent user action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// First message: protocol version, application id (a hint for the
    /// dock and logs, not an identity) and whether the client can send
    /// shared-memory buffers.
    Hello {
        /// Protocol version of the client.
        version: u16,
        /// Application id such as `org.ultar.files`.
        app_id: String,
        /// The client can create sealed shared memory.
        shared_memory: bool,
    },
    /// Creates a surface with a role.
    CreateSurface {
        /// New id.
        surface: SurfaceId,
        /// What it is for.
        role: Role,
    },
    /// Destroys a surface (closing a window).
    DestroySurface {
        /// The surface.
        surface: SurfaceId,
    },
    /// Sets the title shown in the title bar and the dock.
    SetTitle {
        /// The surface.
        surface: SurfaceId,
        /// Title (at most [`MAX_TITLE`] bytes).
        title: String,
    },
    /// Smallest and largest content size in logical points (0 = no limit).
    SetSizeLimits {
        /// The surface.
        surface: SurfaceId,
        /// Smallest width and height.
        min: (u32, u32),
        /// Largest width and height.
        max: (u32, u32),
    },
    /// Registers a shared-memory buffer; the descriptor travels with this
    /// message (`SCM_RIGHTS`).
    CreateShmBuffer {
        /// New id.
        buffer: BufferId,
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// Bytes per row.
        stride: u32,
        /// Pixel layout.
        format: PixelFormat,
    },
    /// Registers a buffer whose pixels arrive in [`Request::WritePixels`].
    CreateInlineBuffer {
        /// New id.
        buffer: BufferId,
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// Pixel layout.
        format: PixelFormat,
    },
    /// Writes pixels into an inline buffer (`rect.width * rect.height * 4`
    /// bytes, rows without padding).
    WritePixels {
        /// The buffer.
        buffer: BufferId,
        /// Where in the buffer.
        rect: Rect,
        /// The pixels.
        data: Vec<u8>,
    },
    /// Forgets a buffer.
    DestroyBuffer {
        /// The buffer.
        buffer: BufferId,
    },
    /// Selects the buffer the next commit takes its pixels from
    /// (`BufferId(0)` hides the surface).
    Attach {
        /// The surface.
        surface: SurfaceId,
        /// The buffer.
        buffer: BufferId,
    },
    /// Marks parts of the attached buffer as changed (buffer pixels).
    Damage {
        /// The surface.
        surface: SurfaceId,
        /// Changed rectangles (at most [`MAX_DAMAGE`]).
        rects: Vec<Rect>,
    },
    /// Applies attach and damage atomically; acknowledges a configure.
    Commit {
        /// The surface.
        surface: SurfaceId,
        /// Serial of the last configure the content was drawn for.
        configure: u32,
    },
    /// Asks for a `FrameDone` when the next frame that shows the surface is
    /// on the screen (paces drawing to the display).
    Frame {
        /// The surface.
        surface: SurfaceId,
    },
    /// Asks for a window state (the compositor decides).
    SetState {
        /// The surface.
        surface: SurfaceId,
        /// Wanted state.
        state: WindowState,
    },
    /// Starts moving the window with the pointer (from a press in the app's
    /// own header area).
    Move {
        /// The surface.
        surface: SurfaceId,
        /// Serial of the press.
        serial: u32,
    },
    /// Starts resizing the window with the pointer.
    Resize {
        /// The surface.
        surface: SurfaceId,
        /// Serial of the press.
        serial: u32,
        /// Edges to drag.
        edges: ResizeEdges,
    },
    /// Answers a `Ping`.
    Pong {
        /// The ping's serial.
        serial: u32,
    },
    /// Offers clipboard content (copy), on a key or button press.
    SetSelection {
        /// Serial of the user action.
        serial: u32,
        /// Offered types.
        mimes: Vec<String>,
    },
    /// Asks for the clipboard content (paste), on a key or button press.
    ReadSelection {
        /// Serial of the user action.
        serial: u32,
        /// Wanted type.
        mime: String,
    },
    /// Data for a transfer the compositor asked this client for.
    SendData {
        /// The transfer.
        transfer: TransferId,
        /// A chunk (at most [`MAX_CHUNK`]).
        data: Vec<u8>,
        /// Last chunk.
        last: bool,
    },
    /// Starts dragging data from a pointer press.
    StartDrag {
        /// Serial of the press.
        serial: u32,
        /// Offered types.
        mimes: Vec<String>,
    },
    /// Asks for the data of the last drop on this client.
    ReadDrop {
        /// Wanted type.
        mime: String,
    },
    /// A text field got or lost the keyboard focus (shows the on-screen
    /// keyboard).
    TextInput {
        /// The surface.
        surface: SurfaceId,
        /// Field focused.
        enabled: bool,
        /// Kind of field (text, number, password …; see `ultar-ime`).
        purpose: u8,
    },
    /// Text typed on the on-screen keyboard (input method only).
    ImeCommit {
        /// The text.
        text: String,
    },
    /// Reads the pixels of an output (needs [`Rights::CAPTURE`]).
    Capture {
        /// The output.
        output: OutputId,
    },
    /// Ends the session lock after the lock screen authenticated the user
    /// (needs [`Rights::LOCK_SCREEN`] and a lock surface of this client).
    Unlock {
        /// The client's lock surface.
        surface: SurfaceId,
    },
    /// Asks for an activation token for a user action (a launcher passes it
    /// to the app it starts, which then may take the focus with it).
    RequestActivation {
        /// Serial of the user action.
        serial: u32,
    },
    /// Takes the keyboard focus with a token from `ActivationToken`.
    Activate {
        /// The surface.
        surface: SurfaceId,
        /// The token.
        token: u64,
    },
    /// Asks for the window list (needs [`Rights::CAPTURE`]); answered by
    /// `WindowList`.
    ListWindows,
    /// Sets the whole output arrangement (needs [`Rights::OUTPUT_CONFIG`]);
    /// answered by `OutputSetupDone`.
    ConfigureOutputs {
        /// The arrangement.
        setup: super::outputs::Setup,
    },
    /// Makes the provisional setup final (needs [`Rights::OUTPUT_CONFIG`]).
    ConfirmOutputs {
        /// Serial of the setup.
        serial: u32,
    },
    /// Shows each output's number large on it (needs [`Rights::OUTPUT_CONFIG`]).
    IdentifyOutputs {
        /// Output and number (1–99).
        numbers: Vec<(OutputId, u8)>,
        /// How long, in milliseconds (at most `MAX_IDENTIFY_MS`).
        ms: u32,
    },
    /// Looks for displays again and sends `OutputDetails` for each (needs
    /// [`Rights::OUTPUT_CONFIG`]).
    DetectOutputs,
}

impl Request {
    /// Opcode on the wire.
    pub fn opcode(&self) -> u16 {
        match self {
            Request::Hello { .. } => 1,
            Request::CreateSurface { .. } => 2,
            Request::DestroySurface { .. } => 3,
            Request::SetTitle { .. } => 4,
            Request::SetSizeLimits { .. } => 5,
            Request::CreateShmBuffer { .. } => 6,
            Request::CreateInlineBuffer { .. } => 7,
            Request::WritePixels { .. } => 8,
            Request::DestroyBuffer { .. } => 9,
            Request::Attach { .. } => 10,
            Request::Damage { .. } => 11,
            Request::Commit { .. } => 12,
            Request::Frame { .. } => 13,
            Request::SetState { .. } => 14,
            Request::Move { .. } => 15,
            Request::Resize { .. } => 16,
            Request::Pong { .. } => 17,
            Request::SetSelection { .. } => 18,
            Request::ReadSelection { .. } => 19,
            Request::SendData { .. } => 20,
            Request::StartDrag { .. } => 21,
            Request::ReadDrop { .. } => 22,
            Request::TextInput { .. } => 23,
            Request::ImeCommit { .. } => 24,
            Request::Capture { .. } => 25,
            Request::Unlock { .. } => 26,
            Request::RequestActivation { .. } => 27,
            Request::Activate { .. } => 28,
            Request::ListWindows => 29,
            Request::ConfigureOutputs { .. } => 34,
            Request::ConfirmOutputs { .. } => 35,
            Request::IdentifyOutputs { .. } => 36,
            Request::DetectOutputs => 37,
        }
    }

    /// Encodes the request as one frame.
    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        let mut w = Writer::new(self.opcode());
        self.encode_body(&mut w);
        w.finish()
    }

    fn encode_body(&self, w: &mut Writer) {
        match self {
            Request::Hello {
                version,
                app_id,
                shared_memory,
            } => {
                w.u16(*version).str(app_id, MAX_APP_ID).bool(*shared_memory);
            }
            Request::CreateSurface { surface, role } => {
                w.u32(surface.0);
                role.encode(w);
            }
            Request::DestroySurface { surface } | Request::Frame { surface } => {
                w.u32(surface.0);
            }
            Request::SetTitle { surface, title } => {
                w.u32(surface.0).str(title, MAX_TITLE);
            }
            Request::SetSizeLimits { surface, min, max } => {
                w.u32(surface.0).u32(min.0).u32(min.1).u32(max.0).u32(max.1);
            }
            Request::CreateShmBuffer {
                buffer,
                width,
                height,
                stride,
                format,
            } => {
                w.u32(buffer.0).u32(*width).u32(*height).u32(*stride);
                format.encode(w);
            }
            Request::CreateInlineBuffer {
                buffer,
                width,
                height,
                format,
            } => {
                w.u32(buffer.0).u32(*width).u32(*height);
                format.encode(w);
            }
            Request::WritePixels { buffer, rect, data } => {
                w.u32(buffer.0);
                rect.encode(w);
                w.bytes(data);
            }
            Request::DestroyBuffer { buffer } => {
                w.u32(buffer.0);
            }
            Request::Attach { surface, buffer } => {
                w.u32(surface.0).u32(buffer.0);
            }
            Request::Damage { surface, rects } => {
                let n = rects.len().min(MAX_DAMAGE);
                w.u32(surface.0).u8(n as u8);
                rects[..n].iter().for_each(|r| r.encode(w));
            }
            other => other.encode_more(w),
        }
    }

    fn encode_more(&self, w: &mut Writer) {
        match self {
            Request::Commit { surface, configure } => {
                w.u32(surface.0).u32(*configure);
            }
            Request::SetState { surface, state } => {
                w.u32(surface.0);
                state.encode(w);
            }
            Request::Move { surface, serial } => {
                w.u32(surface.0).u32(*serial);
            }
            Request::Resize {
                surface,
                serial,
                edges,
            } => {
                w.u32(surface.0).u32(*serial).u8(edges.0);
            }
            Request::Pong { serial } => {
                w.u32(*serial);
            }
            Request::SetSelection { serial, mimes } | Request::StartDrag { serial, mimes } => {
                w.u32(*serial);
                encode_mimes(w, mimes);
            }
            Request::ReadSelection { serial, mime } => {
                w.u32(*serial).str(mime, MAX_MIME);
            }
            Request::SendData {
                transfer,
                data,
                last,
            } => {
                w.u32(transfer.0).bytes(data).bool(*last);
            }
            Request::ReadDrop { mime } => {
                w.str(mime, MAX_MIME);
            }
            Request::TextInput {
                surface,
                enabled,
                purpose,
            } => {
                w.u32(surface.0).bool(*enabled).u8(*purpose);
            }
            other => other.encode_session(w),
        }
    }

    fn encode_session(&self, w: &mut Writer) {
        match self {
            Request::ImeCommit { text } => {
                w.str(text, MAX_IME_TEXT);
            }
            Request::Capture { output } => {
                w.u32(output.0);
            }
            Request::Unlock { surface } => {
                w.u32(surface.0);
            }
            Request::RequestActivation { serial } => {
                w.u32(*serial);
            }
            Request::Activate { surface, token } => {
                w.u32(surface.0).u64(*token);
            }
            Request::ConfigureOutputs { setup } => setup.encode(w),
            Request::ConfirmOutputs { serial } => {
                w.u32(*serial);
            }
            Request::IdentifyOutputs { numbers, ms } => {
                super::outputs::encode_numbers(w, numbers);
                w.u32(*ms);
            }
            _ => {}
        }
    }

    /// Decodes one request body. Every field is checked; unknown opcodes,
    /// trailing bytes and out-of-range values are errors.
    pub fn decode(opcode: u16, body: &[u8]) -> Result<Request, WireError> {
        let mut r = Reader::new(body);
        let request = super::request_decode::decode_body(opcode, &mut r)?;
        r.end()?;
        Ok(request)
    }
}
