//! # ultar-unlock – the graphical unlock screen of the encrypted data partition
//!
//! At start, before any service runs (ultar-init, `/usr/bin/ultar-unlock`), the kernel holds
//! the data partition locked (`kernel/ultar-kern/src/crypt.rs`). This program draws the
//! unlock screen with the UI toolkit straight onto the framebuffer, reads the keyboard from
//! the input devices, opens the LUKS2 header with `ultar-luks` and hands the master key to
//! the kernel, which mounts `/data`. Language and keyboard of the installation come from the
//! header's `ultar-start` token; the strokes are tried as text in every layout (`keys`),
//! the installation's layout first, so a passphrase typed in any layout at installation opens.
//! Without a framebuffer it ends with [`NO_DISPLAY`] and ultar-init asks on the text console
//! (`ultar-luks unlock`).

pub mod keys;
pub mod view;

/// Exit status: no framebuffer or no keyboard – the text console takes over.
pub const NO_DISPLAY: u8 = 3;

/// Where the screen is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for the passphrase.
    Typing,
    /// The key derivation runs.
    Checking,
    /// The last passphrase was wrong.
    Wrong,
    /// The data cannot be opened (reason, shown in the field's error line).
    Failed(String),
    /// Opened: `/data` is mounted.
    Done,
}

/// What the screen shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    /// Language of the installation.
    pub lang: ultar_i18n::Lang,
    /// Strokes typed (one dot each).
    pub typed: usize,
    /// Caps Lock is on.
    pub caps: bool,
    /// Phase.
    pub phase: Phase,
}

impl Screen {
    /// The first screen in `lang`.
    pub fn new(lang: ultar_i18n::Lang) -> Screen {
        Screen {
            lang,
            typed: 0,
            caps: false,
            phase: Phase::Typing,
        }
    }
}

/// Renders `screen` into a scan-out canvas of `size` pixels (blue first, as the framebuffer).
pub fn render(
    screen: &Screen,
    size: (u32, u32),
    scale: f32,
) -> Result<ultar_ui::canvas::Canvas, String> {
    let mut ui = view::build(screen, size, scale).map_err(|e| format!("{e:?}"))?;
    let mut canvas =
        ultar_ui::canvas::Canvas::new(size.0, size.1, ultar_ui::canvas::PixelFormat::Bgrx8)
            .map_err(|e| format!("{e:?}"))?;
    ui.paint_all(&mut canvas);
    Ok(canvas)
}

/// Device pixels per point for a screen `width` pixels wide.
pub fn scale_for(width: u32) -> f32 {
    if width >= 2560 {
        2.0
    } else {
        1.0
    }
}
