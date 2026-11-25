//! Messages from a client to the compositor.

use super::shell::{DisplayModeRequest, WindowAction};
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
    /// Smallest and largest content size in logical points (0 = no limit). Stated before
    /// the first frame, the smallest size is also the least the window opens with.
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
    /// Gives the keyboard to one of the shell's own panels or panel popups
    /// (an open sheet or menu); `SurfaceId(0)` gives it back to the focused
    /// window. Needs [`Rights::SHELL`]. The compositor takes it away again
    /// (`ShellFocusLost`) when the user clicks a window or the session locks.
    ShellFocus {
        /// The shell surface, or 0.
        surface: SurfaceId,
    },
    /// Acts on any toplevel window by the id of `WindowsChanged` (needs
    /// [`Rights::SHELL`]).
    WindowAction {
        /// Window id.
        window: u32,
        /// What to do.
        action: WindowAction,
    },
    /// Switches the display mode (needs [`Rights::SHELL`]).
    SetDisplayMode {
        /// The mode.
        mode: DisplayModeRequest,
    },
    /// Selects the keyboard layout by its index in `KeyboardLayouts`
    /// (needs [`Rights::SHELL`] or [`Rights::LOCK_SCREEN`]).
    SetKeyboardLayout {
        /// Index.
        index: u8,
    },
    /// Sets the keyboard layouts the user switches between, in order (needs
    /// [`Rights::SHELL`] or [`Rights::LOCK_SCREEN`]; 1 ..= `MAX_LAYOUTS` names of at most `MAX_LAYOUT_NAME` bytes;
    /// unknown names are left out by the compositor). The active one stays active when it
    /// is in the new list, else the first becomes active. Answered by `KeyboardLayouts`.
    SetKeyboardLayouts {
        /// Layout names (`ar`, `en`, `de`).
        names: Vec<String>,
    },
    /// Asks for a scaled copy of a window's content for the overview (needs
    /// [`Rights::SHELL`]; refused while the session is locked; system
    /// dialogs and unknown ids get no answer). Answered by
    /// `Event::WindowThumbnail`, at most [`MAX_THUMBNAIL_SIDE`](super::MAX_THUMBNAIL_SIDE)
    /// on each side, at most [`MAX_THUMBNAILS_PER_SECOND`](super::MAX_THUMBNAILS_PER_SECOND).
    WindowThumbnail {
        /// Window id of `WindowsChanged`.
        window: u32,
        /// Largest width of the copy in pixels (1 ..= 512).
        max_width: u16,
        /// Largest height of the copy in pixels (1 ..= 512).
        max_height: u16,
    },
    /// Shows workspace `index` (needs [`Rights::SHELL`]); `index` equal to
    /// the number of workspaces adds one (at most
    /// [`MAX_WORKSPACES`](super::MAX_WORKSPACES)).
    SwitchWorkspace {
        /// Workspace, from 0.
        index: u8,
    },
    /// Moves a window to workspace `index`, or onto all of them with
    /// [`ALL_WORKSPACES`](super::ALL_WORKSPACES) (needs [`Rights::SHELL`]).
    MoveToWorkspace {
        /// Window id of `WindowsChanged`.
        window: u32,
        /// Workspace, or `ALL_WORKSPACES`.
        index: u8,
    },
    /// Phone mode on or off (needs [`Rights::SHELL`]): every window without a title bar
    /// over the whole work area, and the edge gestures (`Event::EdgeSwipe`) for the shell.
    SetPhoneMode {
        /// On.
        on: bool,
    },
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
    /// How relative pointer devices behave (needs [`Rights::OUTPUT_CONFIG`]: the display
    /// service passes the settings `pointer.*` and the pointer parts of `accessibility.*`
    /// on). The body grew with the settings: a decoder accepts the old two-field form and
    /// fills the rest with the schema's defaults.
    ConfigurePointer {
        /// Pointer speed, [`super::MIN_POINTER_SPEED`] (slowest) to
        /// [`super::MAX_POINTER_SPEED`]; 0 moves the pointer as far as the device reports.
        speed: i8,
        /// The content follows the wheel or the fingers (scroll deltas are reversed).
        natural_scroll: bool,
        /// The right button is the primary one (`pointer.primary_button` = `right`):
        /// primary and secondary clicks are swapped.
        swap_buttons: bool,
        /// Faster movements travel farther (`pointer.acceleration`).
        acceleration: bool,
        /// Edge of the pointer image (`pointer.size`, `accessibility.large_pointer`).
        size: super::PointerSize,
        /// Milliseconds between two clicks of a double click
        /// (`accessibility.double_click_ms`), [`super::MIN_DOUBLE_CLICK_MS`] to
        /// [`super::MAX_DOUBLE_CLICK_MS`].
        double_click_ms: u16,
    },
    /// How a held key repeats (needs [`Rights::OUTPUT_CONFIG`]: the display service passes
    /// the settings `keyboard.repeat.*` on). The compositor thins out the repeats the keyboard
    /// driver reports; it never repeats earlier or faster than the driver does.
    ConfigureKeyRepeat {
        /// `false`: a held key gives one character only.
        enabled: bool,
        /// Time a key is held before it repeats, in milliseconds, at most
        /// [`super::MAX_KEY_REPEAT_DELAY_MS`]; 0 = as the driver reports.
        delay_ms: u16,
        /// Repeats per second, at most [`super::MAX_KEY_REPEATS_PER_SECOND`]; 0 = as the
        /// driver reports.
        per_second: u8,
    },
    /// Which keys switch the input language and whether every window keeps its own
    /// (needs [`Rights::OUTPUT_CONFIG`]: the display service passes the settings
    /// `keyboard.switch_shortcut` and `keyboard.per_window` on).
    ConfigureKeyboard {
        /// The combination that makes the next layout of the list active.
        switch_shortcut: super::SwitchShortcut,
        /// Every window remembers its own input language and gets it back on focus.
        per_window: bool,
    },
    /// The app draws the header of this toplevel itself (needs
    /// [`Rights::CLIENT_HEADER`]): no compositor title bar. Move, resize, snap, Alt+F4 and
    /// the "not responding" close stay with the compositor; system dialogs keep theirs.
    ClientHeader {
        /// The toplevel.
        surface: SurfaceId,
        /// Draws its own header.
        on: bool,
    },
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
            Request::ShellFocus { .. } => 30,
            Request::WindowAction { .. } => 31,
            Request::SetDisplayMode { .. } => 32,
            Request::SetKeyboardLayout { .. } => 33,
            Request::WindowThumbnail { .. } => 38,
            Request::SwitchWorkspace { .. } => 39,
            Request::MoveToWorkspace { .. } => 40,
            Request::SetPhoneMode { .. } => 41,
            Request::ConfigureOutputs { .. } => 34,
            Request::ConfirmOutputs { .. } => 35,
            Request::IdentifyOutputs { .. } => 36,
            Request::DetectOutputs => 37,
            Request::ClientHeader { .. } => 42,
            Request::SetKeyboardLayouts { .. } => 43,
            Request::ConfigurePointer { .. } => 44,
            Request::ConfigureKeyRepeat { .. } => 45,
            Request::ConfigureKeyboard { .. } => 46,
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
            Request::ShellFocus { surface } => {
                w.u32(surface.0);
            }
            Request::WindowAction { window, action } => {
                w.u32(*window);
                action.encode(w);
            }
            Request::SetDisplayMode { mode } => mode.encode(w),
            Request::SetKeyboardLayout { index } | Request::SwitchWorkspace { index } => {
                w.u8(*index);
            }
            Request::WindowThumbnail {
                window,
                max_width,
                max_height,
            } => {
                w.u32(*window).u16(*max_width).u16(*max_height);
            }
            Request::MoveToWorkspace { window, index } => {
                w.u32(*window).u8(*index);
            }
            Request::SetPhoneMode { on } => {
                w.bool(*on);
            }
            Request::ConfigureOutputs { setup } => setup.encode(w),
            Request::ConfirmOutputs { serial } => {
                w.u32(*serial);
            }
            Request::IdentifyOutputs { numbers, ms } => {
                super::outputs::encode_numbers(w, numbers);
                w.u32(*ms);
            }
            Request::ClientHeader { surface, on } => {
                w.u32(surface.0).u8(u8::from(*on));
            }
            Request::SetKeyboardLayouts { names } => {
                let names = &names[..names.len().min(super::MAX_LAYOUTS)];
                w.u8(names.len() as u8);
                for name in names {
                    w.str(name, super::MAX_LAYOUT_NAME);
                }
            }
            other => other.encode_input(w),
        }
    }

    fn encode_input(&self, w: &mut Writer) {
        match self {
            Request::ConfigurePointer {
                speed,
                natural_scroll,
                swap_buttons,
                acceleration,
                size,
                double_click_ms,
            } => {
                w.u8(speed.to_le_bytes()[0])
                    .bool(*natural_scroll)
                    .bool(*swap_buttons)
                    .bool(*acceleration);
                size.encode(w);
                w.u16(*double_click_ms);
            }
            Request::ConfigureKeyRepeat {
                enabled,
                delay_ms,
                per_second,
            } => {
                w.bool(*enabled).u16(*delay_ms).u8(*per_second);
            }
            Request::ConfigureKeyboard {
                switch_shortcut,
                per_window,
            } => {
                switch_shortcut.encode(w);
                w.bool(*per_window);
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
