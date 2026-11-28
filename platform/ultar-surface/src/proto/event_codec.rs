//! Encoding and decoding of event bodies (split from `event.rs` for size).

use super::event::{Configure, ErrorCode, Event, OutputInfo};
use super::types::*;
use crate::wire::{Reader, WireError, Writer};

pub(super) fn encode_body(event: &Event, w: &mut Writer) {
    if !encode_window(event, w) && !encode_input(event, w) && !encode_outputs(event, w) {
        encode_data(event, w);
    }
}

/// Output management events; false if `event` is another kind.
fn encode_outputs(event: &Event, w: &mut Writer) -> bool {
    match event {
        Event::OutputDetails(d) => d.encode(w),
        Event::OutputSetupDone { serial, outcome } => {
            w.u32(*serial).u8(outcome.to_u8());
        }
        Event::OutputsDetected { count } => {
            w.u32(*count);
        }
        _ => return false,
    }
    true
}

/// Window, output and buffer events; false if `event` is another kind.
fn encode_window(event: &Event, w: &mut Writer) -> bool {
    match event {
        Event::Welcome {
            version,
            transport,
            rights,
            max_buffer_bytes,
        } => {
            let t = u8::from(*transport == Transport::Inline);
            w.u16(*version).u8(t).u32(rights.0).u64(*max_buffer_bytes);
        }
        Event::Output(o) => {
            w.u32(o.id.0).i32(o.x).i32(o.y).u32(o.width).u32(o.height);
            w.u32(o.scale_milli)
                .u32(o.refresh_millihertz)
                .bool(o.primary);
            w.str(&o.name, 32);
        }
        Event::OutputRemoved { output } => {
            w.u32(output.0);
        }
        Event::Configure(c) => {
            w.u32(c.surface.0).u32(c.serial).u32(c.width).u32(c.height);
            w.u32(c.scale_milli);
            c.state.encode(w);
            w.bool(c.activated).bool(c.rtl);
        }
        Event::BufferReleased { buffer } => {
            w.u32(buffer.0);
        }
        Event::FrameDone { surface, time_ms } => {
            w.u32(surface.0).u32(*time_ms);
        }
        Event::CloseRequested { surface }
        | Event::PointerLeave { surface }
        | Event::TouchCancel { surface } => {
            w.u32(surface.0);
        }
        Event::Ping { serial } => {
            w.u32(*serial);
        }
        Event::ImeState { active, purpose } => {
            w.bool(*active).u8(*purpose);
        }
        _ => return false,
    }
    true
}

fn encode_input(event: &Event, w: &mut Writer) -> bool {
    match event {
        Event::PointerEnter { surface, x, y } | Event::PointerMotion { surface, x, y } => {
            w.u32(surface.0).i32(*x).i32(*y);
        }
        Event::PointerButton {
            surface,
            button,
            pressed,
            serial,
        } => {
            w.u32(surface.0).u16(*button).bool(*pressed).u32(*serial);
        }
        Event::PointerAxis { surface, dx, dy } => {
            w.u32(surface.0).i32(*dx).i32(*dy);
        }
        Event::KeyboardFocus { surface, focused } => {
            w.u32(surface.0).bool(*focused);
        }
        Event::Key {
            surface,
            code,
            state,
            modifiers,
            character,
            serial,
        } => {
            w.u32(surface.0).u16(*code).u8(*state).u8(modifiers.0);
            w.u32(*character).u32(*serial);
        }
        Event::TouchDown {
            surface,
            id,
            x,
            y,
            serial,
        } => {
            w.u32(surface.0).u32(*id).i32(*x).i32(*y).u32(*serial);
        }
        Event::TouchMove { surface, id, x, y } => {
            w.u32(surface.0).u32(*id).i32(*x).i32(*y);
        }
        Event::TouchUp { surface, id } => {
            w.u32(surface.0).u32(*id);
        }
        _ => return false,
    }
    true
}

fn encode_data(event: &Event, w: &mut Writer) {
    match event {
        Event::Text { surface, text } => {
            w.u32(surface.0).str(text, MAX_IME_TEXT);
        }
        Event::SelectionOffer { mimes } => encode_mimes(w, mimes),
        Event::SelectionChange {
            mimes,
            confidential,
        } => {
            encode_mimes(w, mimes);
            w.bool(*confidential);
        }
        Event::SendRequest { transfer, mime } => {
            w.u32(transfer.0).str(mime, MAX_MIME);
        }
        Event::Data {
            transfer,
            data,
            last,
        } => {
            w.u32(transfer.0).bytes(data).bool(*last);
        }
        Event::TransferCancelled { transfer } => {
            w.u32(transfer.0);
        }
        Event::DropOffer {
            surface,
            x,
            y,
            mimes,
        } => {
            w.u32(surface.0).i32(*x).i32(*y);
            encode_mimes(w, mimes);
        }
        Event::CaptureData {
            output,
            width,
            height,
            row,
            data,
            last,
        } => {
            w.u32(output.0).u32(*width).u32(*height).u32(*row);
            w.bytes(data).bool(*last);
        }
        Event::Error { code } => {
            w.u8(Event::error_code(*code));
        }
        Event::ActivationToken { token } => {
            w.u64(*token);
        }
        Event::SessionLocked { locked } => {
            w.bool(*locked);
        }
        Event::WindowList { windows } => {
            super::windows::encode_list(w, windows);
        }
        other => encode_shell(other, w),
    }
}

/// The events of the desktop shell (ui.shell).
fn encode_shell(event: &Event, w: &mut Writer) {
    match event {
        Event::ShellKey {
            code,
            modifiers,
            serial,
        } => {
            w.u16(*code).u8(modifiers.0).u32(*serial);
        }
        Event::WindowsChanged { windows } => super::shell::encode_windows(w, windows),
        Event::KeyboardLayouts { names, active } => {
            super::shell::encode_layouts(w, names, *active);
        }
        Event::ShellFocusLost { surface } => {
            w.u32(surface.0);
        }
        Event::WindowThumbnail {
            window,
            width,
            height,
            data,
        } => {
            w.u32(*window).u16(*width).u16(*height).bytes(data);
        }
        Event::Workspaces { current, count } => {
            w.u8(*current).u8(*count);
        }
        Event::EdgeSwipe { edge, held } => {
            w.u8(edge.to_u8()).bool(*held);
        }
        _ => {}
    }
}

/// The display service's events: details (36), a setup's outcome (37), detection (38).
fn decode_outputs(opcode: u16, r: &mut Reader<'_>) -> Result<Event, WireError> {
    Ok(match opcode {
        36 => Event::OutputDetails(Box::new(super::outputs::OutputDetails::decode(r)?)),
        37 => Event::OutputSetupDone {
            serial: r.u32()?,
            outcome: super::outputs::SetupOutcome::from_u8(r.u8()?)?,
        },
        _ => Event::OutputsDetected { count: r.u32()? },
    })
}

/// The overview's events: a thumbnail (39) or the workspaces (40); any other opcode is refused.
fn decode_overview(opcode: u16, r: &mut Reader<'_>) -> Result<Event, WireError> {
    match opcode {
        39 => decode_thumbnail(r),
        40 => decode_workspaces(r),
        41 => Ok(Event::EdgeSwipe {
            edge: super::shell::SwipeEdge::from_u8(r.u8()?)?,
            held: r.bool()?,
        }),
        _ => Err(WireError::BadValue),
    }
}

/// Workspaces: at least one, at most `MAX_WORKSPACES`, the shown one among them.
fn decode_workspaces(r: &mut Reader<'_>) -> Result<Event, WireError> {
    let (current, count) = (r.u8()?, r.u8()?);
    let max = super::MAX_WORKSPACES as u8;
    if count == 0 || count > max || current >= count {
        return Err(WireError::BadValue);
    }
    Ok(Event::Workspaces { current, count })
}

/// A thumbnail whose pixels match its size exactly.
fn decode_thumbnail(r: &mut Reader<'_>) -> Result<Event, WireError> {
    let window = r.u32()?;
    let (width, height) = (r.u16()?, r.u16()?);
    let side = super::MAX_THUMBNAIL_SIDE;
    if width > side || height > side {
        return Err(WireError::BadValue);
    }
    let len = usize::from(width) * usize::from(height) * 4;
    let data = r.bytes(len)?.to_vec();
    if data.len() != len {
        return Err(WireError::BadValue);
    }
    Ok(Event::WindowThumbnail {
        window,
        width,
        height,
        data,
    })
}

fn sid(r: &mut Reader<'_>) -> Result<SurfaceId, WireError> {
    Ok(SurfaceId(r.u32()?))
}

pub(super) fn decode_body(opcode: u16, r: &mut Reader<'_>) -> Result<Event, WireError> {
    match opcode {
        1..=6 | 19 | 25 | 26 => decode_window(opcode, r),
        _ => decode_input(opcode, r),
    }
}

fn decode_window(opcode: u16, r: &mut Reader<'_>) -> Result<Event, WireError> {
    Ok(match opcode {
        1 => Event::Welcome {
            version: r.u16()?,
            transport: match r.u8()? {
                0 => Transport::SharedMemory,
                1 => Transport::Inline,
                _ => return Err(WireError::BadValue),
            },
            rights: Rights(r.u32()?),
            max_buffer_bytes: r.u64()?,
        },
        2 => Event::Output(OutputInfo {
            id: OutputId(r.u32()?),
            x: r.i32()?,
            y: r.i32()?,
            width: r.u32()?,
            height: r.u32()?,
            scale_milli: r.u32()?,
            refresh_millihertz: r.u32()?,
            primary: r.bool()?,
            name: r.str(32)?,
        }),
        3 => Event::OutputRemoved {
            output: OutputId(r.u32()?),
        },
        4 => Event::Configure(Configure {
            surface: sid(r)?,
            serial: r.u32()?,
            width: r.u32()?,
            height: r.u32()?,
            scale_milli: r.u32()?,
            state: WindowState::decode(r)?,
            activated: r.bool()?,
            rtl: r.bool()?,
        }),
        5 => Event::BufferReleased {
            buffer: BufferId(r.u32()?),
        },
        6 => Event::FrameDone {
            surface: sid(r)?,
            time_ms: r.u32()?,
        },
        19 => Event::CloseRequested { surface: sid(r)? },
        25 => Event::Ping { serial: r.u32()? },
        26 => Event::ImeState {
            active: r.bool()?,
            purpose: r.u8()?,
        },
        _ => return Err(WireError::BadValue),
    })
}

fn decode_input(opcode: u16, r: &mut Reader<'_>) -> Result<Event, WireError> {
    Ok(match opcode {
        7 | 9 => {
            let (surface, x, y) = (sid(r)?, r.i32()?, r.i32()?);
            if opcode == 7 {
                Event::PointerEnter { surface, x, y }
            } else {
                Event::PointerMotion { surface, x, y }
            }
        }
        8 => Event::PointerLeave { surface: sid(r)? },
        10 => Event::PointerButton {
            surface: sid(r)?,
            button: r.u16()?,
            pressed: r.bool()?,
            serial: r.u32()?,
        },
        11 => Event::PointerAxis {
            surface: sid(r)?,
            dx: r.i32()?,
            dy: r.i32()?,
        },
        12 => Event::KeyboardFocus {
            surface: sid(r)?,
            focused: r.bool()?,
        },
        13 => Event::Key {
            surface: sid(r)?,
            code: r.u16()?,
            state: r.u8()?,
            modifiers: Modifiers(r.u8()?),
            character: r.u32()?,
            serial: r.u32()?,
        },
        14 => Event::TouchDown {
            surface: sid(r)?,
            id: r.u32()?,
            x: r.i32()?,
            y: r.i32()?,
            serial: r.u32()?,
        },
        15 => Event::TouchMove {
            surface: sid(r)?,
            id: r.u32()?,
            x: r.i32()?,
            y: r.i32()?,
        },
        16 => Event::TouchUp {
            surface: sid(r)?,
            id: r.u32()?,
        },
        17 => Event::TouchCancel { surface: sid(r)? },
        _ => decode_data(opcode, r)?,
    })
}

fn decode_data(opcode: u16, r: &mut Reader<'_>) -> Result<Event, WireError> {
    Ok(match opcode {
        18 => Event::Text {
            surface: sid(r)?,
            text: r.text(MAX_IME_TEXT)?,
        },
        20 => Event::SelectionOffer {
            mimes: decode_mimes(r)?,
        },
        42 => Event::SelectionChange {
            mimes: decode_mimes(r)?,
            confidential: r.bool()?,
        },
        21 => Event::SendRequest {
            transfer: TransferId(r.u32()?),
            mime: r.str(MAX_MIME)?,
        },
        22 => Event::Data {
            transfer: TransferId(r.u32()?),
            data: r.bytes(MAX_CHUNK)?.to_vec(),
            last: r.bool()?,
        },
        23 => Event::TransferCancelled {
            transfer: TransferId(r.u32()?),
        },
        24 => Event::DropOffer {
            surface: sid(r)?,
            x: r.i32()?,
            y: r.i32()?,
            mimes: decode_mimes(r)?,
        },
        27 => Event::CaptureData {
            output: OutputId(r.u32()?),
            width: r.u32()?,
            height: r.u32()?,
            row: r.u32()?,
            data: r.bytes(MAX_CHUNK)?.to_vec(),
            last: r.bool()?,
        },
        28 => Event::Error {
            code: ErrorCode::from_u8(r.u8()?)?,
        },
        29 => Event::ActivationToken { token: r.u64()? },
        30 => Event::SessionLocked { locked: r.bool()? },
        31 => Event::WindowList {
            windows: super::windows::decode_list(r)?,
        },
        32 => Event::ShellKey {
            code: r.u16()?,
            modifiers: Modifiers(r.u8()?),
            serial: r.u32()?,
        },
        33 => Event::WindowsChanged {
            windows: super::shell::decode_windows(r)?,
        },
        34 => {
            let (names, active) = super::shell::decode_layouts(r)?;
            Event::KeyboardLayouts { names, active }
        }
        35 => Event::ShellFocusLost { surface: sid(r)? },
        36..=38 => decode_outputs(opcode, r)?,
        _ => decode_overview(opcode, r)?,
    })
}
