//! Reading the screen from the compositor (right `ui.capture`), one connection for the
//! whole session. While the session is locked the compositor sends nothing: that is how a
//! lock is noticed, and the gate (injecting the viewer's input) opens only after the first
//! piece of a fresh picture arrived, so nothing is ever typed into a lock screen.

use std::path::{Path, PathBuf};
use std::time::Duration;
use ultar_surface::client::Client;
use ultar_surface::proto::{Event, OutputId, Request};

/// Longest wait for the first piece of a picture; longer means the screen is locked.
pub const FIRST_PIECE: Duration = Duration::from_secs(2);
/// Longest wait for the next piece of a picture that has begun.
pub const NEXT_PIECE: Duration = Duration::from_secs(5);
/// Largest picture accepted (8K).
pub const MAX_SIDE: u32 = 8192;

/// A picture of the whole screen: `width * height` pixels `B, G, R, X`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
    /// The pixels.
    pub data: Vec<u8>,
}

/// Why no picture came.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenError {
    /// The compositor sent nothing in time: the session is locked.
    Locked,
    /// No compositor, no output, or something malformed.
    Gone,
}

/// A screen that can be looked at.
pub trait Screen: Send {
    /// Takes a picture. `gate` is called once the picture has begun to arrive (so the
    /// screen is not locked) and before it is complete: the caller injects input there.
    fn grab(&mut self, gate: &mut dyn FnMut()) -> Result<Frame, ScreenError>;
}

/// Puts the chunks of one picture together, checking every one.
#[derive(Default)]
pub struct Assembler {
    frame: Option<Frame>,
}

impl Assembler {
    /// Adds the chunk starting at `row`; `Err` when it does not belong to a sane picture.
    pub fn add(
        &mut self,
        width: u32,
        height: u32,
        row: u32,
        data: &[u8],
    ) -> Result<(), ScreenError> {
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(ScreenError::Gone);
        }
        let f = self.frame.get_or_insert_with(|| Frame {
            width,
            height,
            data: Vec::new(),
        });
        let stride = width as usize * 4;
        let expected = (f.data.len() / stride) as u32;
        let rows_ok = data.len().is_multiple_of(stride);
        let fits = f.data.len() + data.len() <= stride * height as usize;
        if f.width != width || f.height != height || row != expected || !rows_ok || !fits {
            return Err(ScreenError::Gone);
        }
        f.data.extend_from_slice(data);
        Ok(())
    }

    /// The finished picture; `Err` when rows are missing.
    pub fn finish(self) -> Result<Frame, ScreenError> {
        let f = self.frame.ok_or(ScreenError::Gone)?;
        if f.data.len() == f.width as usize * f.height as usize * 4 {
            Ok(f)
        } else {
            Err(ScreenError::Gone)
        }
    }
}

/// The compositor's primary output.
pub struct Compositor {
    client: Client,
    output: OutputId,
}

impl Compositor {
    /// Connects to the compositor at `socket` and picks the primary output.
    pub fn open(socket: &Path) -> Result<Self, ScreenError> {
        let mut client = Client::connect(socket, "ultar-screend").map_err(|_| ScreenError::Gone)?;
        let mut outputs = Vec::new();
        // The outputs arrive right after the connection.
        while let Ok(Some(event)) = client.next_event(Some(Duration::from_millis(300))) {
            if let Event::Output(info) = event {
                outputs.push(info);
            }
        }
        let chosen = outputs.iter().find(|o| o.primary).or(outputs.first());
        let output = chosen.ok_or(ScreenError::Gone)?.id;
        Ok(Self { client, output })
    }
}

impl Screen for Compositor {
    fn grab(&mut self, gate: &mut dyn FnMut()) -> Result<Frame, ScreenError> {
        self.client
            .send(&Request::Capture {
                output: self.output,
            })
            .map_err(|_| ScreenError::Gone)?;
        let mut picture = Assembler::default();
        let mut first = true;
        loop {
            let wait = if first { FIRST_PIECE } else { NEXT_PIECE };
            match self.client.next_event(Some(wait)) {
                Ok(Some(Event::CaptureData {
                    output: OutputId(o),
                    width,
                    height,
                    row,
                    data,
                    last,
                })) if o == self.output.0 => {
                    picture.add(width, height, row, &data)?;
                    if first {
                        first = false;
                        gate();
                    }
                    if last {
                        return picture.finish();
                    }
                }
                Ok(Some(_)) => {}
                Ok(None) if first => return Err(ScreenError::Locked),
                Ok(None) | Err(_) => return Err(ScreenError::Gone),
            }
        }
    }
}

/// Where the compositor listens (`--compositor`).
pub fn socket(arg: Option<PathBuf>) -> PathBuf {
    arg.or_else(|| std::env::var_os("ULTAR_COMPOSITOR").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/run/ultar/ui/compositor.sock"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn chunks_are_checked() {
        let mut a = Assembler::default();
        assert_eq!(a.add(0, 1, 0, &[]), Err(ScreenError::Gone));
        assert_eq!(a.add(MAX_SIDE + 1, 1, 0, &[]), Err(ScreenError::Gone));
        a.add(2, 2, 0, &[1; 8]).unwrap();
        assert_eq!(a.add(2, 2, 0, &[1; 8]), Err(ScreenError::Gone), "row again");
        assert_eq!(
            a.add(3, 2, 1, &[1; 12]),
            Err(ScreenError::Gone),
            "other size"
        );
        assert_eq!(
            a.add(2, 2, 1, &[1; 7]),
            Err(ScreenError::Gone),
            "half a row"
        );
        assert_eq!(
            a.add(2, 2, 1, &[1; 16]),
            Err(ScreenError::Gone),
            "too many rows"
        );
        a.add(2, 2, 1, &[2; 8]).unwrap();
        assert_eq!(a.finish().unwrap().data.len(), 16);
    }

    #[test]
    fn a_picture_with_rows_missing_is_not_finished() {
        let mut a = Assembler::default();
        a.add(2, 2, 0, &[1; 8]).unwrap();
        assert_eq!(a.finish(), Err(ScreenError::Gone));
        assert_eq!(Assembler::default().finish(), Err(ScreenError::Gone));
    }

    #[test]
    fn no_compositor_is_gone() {
        assert!(matches!(
            Compositor::open(Path::new("/nonexistent/ultar.sock")),
            Err(ScreenError::Gone)
        ));
    }
}
