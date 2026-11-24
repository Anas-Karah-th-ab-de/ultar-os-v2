//! Decoding of request bodies (split from `request.rs` for size).

use super::request::Request;
use super::types::*;
use crate::wire::{Reader, WireError};

fn surface(r: &mut Reader<'_>) -> Result<SurfaceId, WireError> {
    Ok(SurfaceId(r.u32()?))
}

fn buffer(r: &mut Reader<'_>) -> Result<BufferId, WireError> {
    Ok(BufferId(r.u32()?))
}

fn pair(r: &mut Reader<'_>) -> Result<(u32, u32), WireError> {
    Ok((r.u32()?, r.u32()?))
}

pub(super) fn decode_body(opcode: u16, r: &mut Reader<'_>) -> Result<Request, WireError> {
    Ok(match opcode {
        1 => Request::Hello {
            version: r.u16()?,
            app_id: r.str(MAX_APP_ID)?,
            shared_memory: r.bool()?,
        },
        2 => Request::CreateSurface {
            surface: surface(r)?,
            role: Role::decode(r)?,
        },
        3 => Request::DestroySurface {
            surface: surface(r)?,
        },
        4 => Request::SetTitle {
            surface: surface(r)?,
            title: r.str(MAX_TITLE)?,
        },
        5 => Request::SetSizeLimits {
            surface: surface(r)?,
            min: pair(r)?,
            max: pair(r)?,
        },
        6 => Request::CreateShmBuffer {
            buffer: buffer(r)?,
            width: r.u32()?,
            height: r.u32()?,
            stride: r.u32()?,
            format: PixelFormat::decode(r)?,
        },
        7 => Request::CreateInlineBuffer {
            buffer: buffer(r)?,
            width: r.u32()?,
            height: r.u32()?,
            format: PixelFormat::decode(r)?,
        },
        8 => Request::WritePixels {
            buffer: buffer(r)?,
            rect: Rect::decode(r)?,
            data: r.bytes(MAX_CHUNK)?.to_vec(),
        },
        9 => Request::DestroyBuffer { buffer: buffer(r)? },
        10 => Request::Attach {
            surface: surface(r)?,
            buffer: buffer(r)?,
        },
        11 => decode_damage(r)?,
        12 => Request::Commit {
            surface: surface(r)?,
            configure: r.u32()?,
        },
        _ => decode_more(opcode, r)?,
    })
}

fn decode_more(opcode: u16, r: &mut Reader<'_>) -> Result<Request, WireError> {
    Ok(match opcode {
        13 => Request::Frame {
            surface: surface(r)?,
        },
        14 => Request::SetState {
            surface: surface(r)?,
            state: WindowState::decode(r)?,
        },
        15 => Request::Move {
            surface: surface(r)?,
            serial: r.u32()?,
        },
        16 => Request::Resize {
            surface: surface(r)?,
            serial: r.u32()?,
            edges: ResizeEdges(r.u8()?),
        },
        17 => Request::Pong { serial: r.u32()? },
        18 => Request::SetSelection {
            serial: r.u32()?,
            mimes: decode_mimes(r)?,
        },
        19 => Request::ReadSelection {
            serial: r.u32()?,
            mime: r.str(MAX_MIME)?,
        },
        20 => Request::SendData {
            transfer: TransferId(r.u32()?),
            data: r.bytes(MAX_CHUNK)?.to_vec(),
            last: r.bool()?,
        },
        21 => Request::StartDrag {
            serial: r.u32()?,
            mimes: decode_mimes(r)?,
        },
        22 => Request::ReadDrop {
            mime: r.str(MAX_MIME)?,
        },
        23 => Request::TextInput {
            surface: surface(r)?,
            enabled: r.bool()?,
            purpose: r.u8()?,
        },
        _ => decode_session(opcode, r)?,
    })
}

fn decode_session(opcode: u16, r: &mut Reader<'_>) -> Result<Request, WireError> {
    Ok(match opcode {
        24 => Request::ImeCommit {
            text: r.text(MAX_IME_TEXT)?,
        },
        25 => Request::Capture {
            output: OutputId(r.u32()?),
        },
        26 => Request::Unlock {
            surface: surface(r)?,
        },
        27 => Request::RequestActivation { serial: r.u32()? },
        28 => Request::Activate {
            surface: surface(r)?,
            token: r.u64()?,
        },
        29 => Request::ListWindows,
        30 => Request::ShellFocus {
            surface: SurfaceId(r.u32()?),
        },
        31 => Request::WindowAction {
            window: r.u32()?,
            action: super::shell::WindowAction::decode(r)?,
        },
        32 => Request::SetDisplayMode {
            mode: super::shell::DisplayModeRequest::decode(r)?,
        },
        33 => Request::SetKeyboardLayout { index: r.u8()? },
        38..=40 => decode_overview(opcode, r)?,
        34 => Request::ConfigureOutputs {
            setup: super::outputs::Setup::decode(r)?,
        },
        35 => Request::ConfirmOutputs { serial: r.u32()? },
        36 => {
            let numbers = super::outputs::decode_numbers(r)?;
            let ms = r.u32()?;
            if ms > super::outputs::MAX_IDENTIFY_MS {
                return Err(WireError::BadValue);
            }
            Request::IdentifyOutputs { numbers, ms }
        }
        37 => Request::DetectOutputs,
        41 => Request::SetPhoneMode { on: r.bool()? },
        42 => Request::ClientHeader {
            surface: surface(r)?,
            on: match r.u8()? {
                0 => false,
                1 => true,
                _ => return Err(WireError::BadValue),
            },
        },
        43 => {
            let n = usize::from(r.u8()?);
            if n == 0 || n > super::MAX_LAYOUTS {
                return Err(WireError::BadValue);
            }
            let mut names = Vec::with_capacity(n);
            for _ in 0..n {
                names.push(r.str(super::MAX_LAYOUT_NAME)?);
            }
            Request::SetKeyboardLayouts { names }
        }
        _ => return Err(WireError::BadValue),
    })
}

/// The overview's requests: a thumbnail (38), show (39) or move to (40) a workspace.
fn decode_overview(opcode: u16, r: &mut Reader<'_>) -> Result<Request, WireError> {
    Ok(match opcode {
        38 => {
            let window = r.u32()?;
            let (max_width, max_height) = (r.u16()?, r.u16()?);
            let side = 1..=super::MAX_THUMBNAIL_SIDE;
            if !side.contains(&max_width) || !side.contains(&max_height) {
                return Err(WireError::BadValue);
            }
            Request::WindowThumbnail {
                window,
                max_width,
                max_height,
            }
        }
        39 => Request::SwitchWorkspace { index: r.u8()? },
        _ => Request::MoveToWorkspace {
            window: r.u32()?,
            index: r.u8()?,
        },
    })
}

fn decode_damage(r: &mut Reader<'_>) -> Result<Request, WireError> {
    let surface = surface(r)?;
    let n = usize::from(r.u8()?);
    if n > MAX_DAMAGE {
        return Err(WireError::TooLong);
    }
    let rects = (0..n)
        .map(|_| Rect::decode(r))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Request::Damage { surface, rects })
}
