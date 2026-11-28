//! `ultar-unlock` – the graphical unlock screen at start (see the crate documentation).
//! Exit status: 0 opened (or nothing to open), 1 failed, 3 no display (text console instead).

use std::fs::File;
use std::os::unix::fs::FileExt;
use std::process::ExitCode;
use std::time::Duration;
use ultar_compositor::keymap::Layout;
use ultar_hal::{Display, DisplayId, FrameView, Input, InputEventKind, KeyState, PixelFormat};
use ultar_hal_gfx::evdev::Evdev;
use ultar_hal_gfx::fbdev::FbDev;
use ultar_hal_system::crypt;
use ultar_unlock::keys::{layout_of, Action, Typed};
use ultar_unlock::{render, scale_for, Phase, Screen, NO_DISPLAY};

/// Pause between two looks at the input devices.
const TICK: Duration = Duration::from_millis(15);
/// How long a failure stays on the screen before the text console takes over.
const FAILURE_SHOWN: Duration = Duration::from_secs(5);

fn reader(f: &File) -> impl FnMut(u64, &mut [u8]) -> Result<(), ultar_luks::Error> + '_ {
    move |at, buf| f.read_exact_at(buf, at).map_err(ultar_luks::Error::from)
}

fn show(fb: &FbDev, screen: &Screen) -> Result<(), String> {
    let g = fb.geometry();
    let canvas = render(screen, (g.width, g.height), scale_for(g.width))?;
    let size = ultar_hal::Size {
        width: g.width,
        height: g.height,
    };
    let frame = FrameView::new(size, g.width * 4, PixelFormat::Xrgb8888, canvas.data())
        .map_err(|e| format!("{e:?}"))?;
    fb.present(DisplayId(0), &frame, &[])
        .map_err(|e| format!("{e:?}"))
}

fn hex(key: &[u8]) -> zeroize::Zeroizing<String> {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = zeroize::Zeroizing::new(String::with_capacity(key.len() * 2));
    for b in key {
        s.push(char::from(D[usize::from(b >> 4)]));
        s.push(char::from(D[usize::from(b & 15)]));
    }
    s
}

/// Tries every candidate text of the strokes; `Ok(true)` when one opened the data.
fn try_open(header: &File, typed: &Typed, first: Layout) -> Result<bool, String> {
    for pass in typed.candidates(first) {
        match ultar_luks::open(&mut reader(header), pass.as_bytes()) {
            Ok(o) => {
                crypt::hand_key(o.offset, &hex(&o.key[..])).map_err(|e| e.to_string())?;
                return Ok(true);
            }
            Err(ultar_luks::Error::Passphrase) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(false)
}

fn submit(
    fb: &FbDev,
    screen: &mut Screen,
    typed: &mut Typed,
    header: &File,
    first: Layout,
) -> Result<bool, String> {
    screen.phase = Phase::Checking;
    show(fb, screen)?;
    let opened = try_open(header, typed, first);
    typed.clear();
    screen.typed = 0;
    match opened {
        Ok(true) => {
            screen.phase = Phase::Done;
            show(fb, screen)?;
            Ok(true)
        }
        Ok(false) => {
            screen.phase = Phase::Wrong;
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

fn run(
    fb: &FbDev,
    input: &Evdev,
    header: &File,
    screen: &mut Screen,
    first: Layout,
) -> Result<(), String> {
    let mut typed = Typed::default();
    show(fb, screen)?;
    loop {
        std::thread::sleep(TICK);
        let mut changed = false;
        for ev in input.poll(0).map_err(|e| format!("{e:?}"))? {
            let InputEventKind::Key { code, state } = ev.kind else {
                continue;
            };
            match typed.key(code, state != KeyState::Released) {
                Action::None => {}
                Action::Changed => {
                    screen.typed = typed.len();
                    screen.caps = typed.caps();
                    screen.phase = Phase::Typing;
                    changed = true;
                }
                Action::Submit if !typed.is_empty() => {
                    if submit(fb, screen, &mut typed, header, first)? {
                        return Ok(());
                    }
                    changed = true;
                }
                Action::Submit => {}
            }
        }
        if changed {
            show(fb, screen)?;
        }
    }
}

fn main() -> ExitCode {
    if !crypt::locked() {
        return ExitCode::SUCCESS;
    }
    let header = match crypt::open_header() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("ultar-unlock: {}: {e}", crypt::DATA_NODE);
            return ExitCode::FAILURE;
        }
    };
    let info = ultar_luks::start_info(&mut reader(&header))
        .ok()
        .flatten()
        .unwrap_or_default();
    let lang = ultar_i18n::Lang::parse(&info.lang).unwrap_or_default();
    let first = layout_of(&info.keyboard);
    let (Ok(fb), Ok(input)) = (FbDev::open(None), Evdev::open_all()) else {
        eprintln!("ultar-unlock: no framebuffer or no input devices: the text console asks");
        return ExitCode::from(NO_DISPLAY);
    };
    let mut screen = Screen::new(lang);
    // while the input devices are open the kernel hands keys only to them, never to the
    // console's line buffer (kernel eingabe.rs: raw mode): the strokes stay in this process
    let result = run(&fb, &input, &header, &mut screen, first);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ultar-unlock: {e}");
            screen.phase = Phase::Failed(e);
            let _ = show(&fb, &screen);
            std::thread::sleep(FAILURE_SHOWN);
            ExitCode::FAILURE
        }
    }
}
