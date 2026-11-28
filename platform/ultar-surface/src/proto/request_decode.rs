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
        44..=47 => decode_input(opcode, r)?,
        56 | 57 => super::visual::decode(opcode, r)?,
        58 => Request::ConfigureLook {
            lang: super::LookLang::decode(r.u8()?)?,
            light: r.bool()?,
        },
        48 => Request::ImeKey {
            key: super::ImeKey::decode(r)?,
        },
        41 => Request::SetPhoneMode { on: r.bool()? },
        42 => Request::ClientHeader {
            surface: surface(r)?,
            on: match r.u8()? {
                0 => false,
                1 => true,
                _ => return Err(WireError::BadValue),
            },
        },
        43 => decode_layouts(r)?,
        _ => return Err(WireError::BadValue),
    })
}

/// The input languages of the shell (43): one to [`super::MAX_LAYOUTS`] names.
fn decode_layouts(r: &mut Reader<'_>) -> Result<Request, WireError> {
    let n = usize::from(r.u8()?);
    if n == 0 || n > super::MAX_LAYOUTS {
        return Err(WireError::BadValue);
    }
    let mut names = Vec::with_capacity(n);
    for _ in 0..n {
        names.push(r.str(super::MAX_LAYOUT_NAME)?);
    }
    Ok(Request::SetKeyboardLayouts { names })
}

/// The keyboard helpers (46) and the language-switch configuration (47).
fn decode_keys(opcode: u16, r: &mut Reader<'_>) -> Result<Request, WireError> {
    if opcode == 47 {
        let switch_shortcut = super::SwitchShortcut::decode(r)?;
        let per_window = r.bool()?;
        return Ok(Request::ConfigureKeyboard {
            switch_shortcut,
            per_window,
        });
    }
    let (sticky_keys, slow_keys) = (r.bool()?, r.bool()?);
    let (bounce_keys, mouse_keys) = (r.bool()?, r.bool()?);
    let compose_key = super::ComposeKey::decode(r)?;
    Ok(Request::ConfigureKeyHelpers {
        sticky_keys,
        slow_keys,
        bounce_keys,
        mouse_keys,
        compose_key,
    })
}

/// The pointer, key-repeat, keyboard-helpers and keyboard configuration of the display
/// service.
fn decode_input(opcode: u16, r: &mut Reader<'_>) -> Result<Request, WireError> {
    if opcode >= 46 {
        return decode_keys(opcode, r);
    }
    if opcode == 44 {
        let speed = i8::from_le_bytes([r.u8()?]);
        if !(super::MIN_POINTER_SPEED..=super::MAX_POINTER_SPEED).contains(&speed) {
            return Err(WireError::BadValue);
        }
        let natural_scroll = r.bool()?;
        // The old form ends here; the fields added later carry the schema's defaults.
        if r.remaining() == 0 {
            return Ok(Request::ConfigurePointer {
                speed,
                natural_scroll,
                swap_buttons: false,
                acceleration: true,
                size: super::PointerSize::Normal,
                double_click_ms: super::DEFAULT_DOUBLE_CLICK_MS,
            });
        }
        let (swap_buttons, acceleration) = (r.bool()?, r.bool()?);
        let size = super::PointerSize::decode(r)?;
        let double_click_ms = r.u16()?;
        if !(super::MIN_DOUBLE_CLICK_MS..=super::MAX_DOUBLE_CLICK_MS).contains(&double_click_ms) {
            return Err(WireError::BadValue);
        }
        return Ok(Request::ConfigurePointer {
            speed,
            natural_scroll,
            swap_buttons,
            acceleration,
            size,
            double_click_ms,
        });
    }
    let (enabled, delay_ms, per_second) = (r.bool()?, r.u16()?, r.u8()?);
    if delay_ms > super::MAX_KEY_REPEAT_DELAY_MS || per_second > super::MAX_KEY_REPEATS_PER_SECOND {
        return Err(WireError::BadValue);
    }
    Ok(Request::ConfigureKeyRepeat {
        enabled,
        delay_ms,
        per_second,
    })
}

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
