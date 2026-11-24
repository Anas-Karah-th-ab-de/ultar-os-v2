//! One surface with its pixel buffer: the part every client needs to show
//! pixels, independent of how they were drawn.

use super::{Client, ClientError};
use crate::proto::{
    BufferId, Configure, Event, PixelFormat, Rect, Request, Role, SurfaceId, Transport, MAX_CHUNK,
    MAX_DAMAGE, MAX_SIDE,
};
use std::time::{Duration, Instant};
use ultar_hal_gfx::shm::SharedBuffer;

/// Longest wait for the compositor to release a buffer before drawing.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(2);

/// What to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowConfig {
    /// Role of the surface.
    pub role: Role,
    /// Title (title bar, dock).
    pub title: String,
    /// Smallest content size in logical points.
    pub min_size: (u32, u32),
    /// Pixels carry premultiplied alpha (`Argb8888`): rounded sheets with
    /// their shadow. Opaque surfaces (`false`) are cheaper to compose.
    pub translucent: bool,
}

impl WindowConfig {
    /// An application window.
    pub fn toplevel(title: &str) -> WindowConfig {
        WindowConfig {
            role: Role::Toplevel,
            title: title.to_owned(),
            min_size: (0, 0),
            translucent: false,
        }
    }

    /// A surface of another role (shell panels, popups, lock screen,
    /// system dialogs).
    pub fn with_role(role: Role, title: &str, translucent: bool) -> WindowConfig {
        WindowConfig {
            role,
            title: title.to_owned(),
            min_size: (0, 0),
            translucent,
        }
    }
}

enum Storage {
    Shared(SharedBuffer),
    Inline,
}

struct Buffer {
    id: BufferId,
    width: u32,
    height: u32,
    storage: Storage,
    busy: bool,
}

/// A surface of this client.
pub struct Window {
    surface: SurfaceId,
    format: PixelFormat,
    configure: Option<Configure>,
    buffer: Option<Buffer>,
    frame_pending: bool,
    close_requested: bool,
}

impl std::fmt::Debug for Window {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Window")
            .field("surface", &self.surface)
            .field("configure", &self.configure)
            .finish()
    }
}

impl Window {
    /// Creates the surface. Its size comes with the first `Configure`.
    pub fn create(client: &mut Client, config: &WindowConfig) -> Result<Window, ClientError> {
        let surface = client.new_surface_id();
        client.send(&Request::CreateSurface {
            surface,
            role: config.role,
        })?;
        if !config.title.is_empty() {
            client.send(&Request::SetTitle {
                surface,
                title: config.title.clone(),
            })?;
        }
        if config.role == crate::proto::Role::Toplevel {
            if let Some(token) = client.take_activation_token() {
                client.send(&Request::Activate { surface, token })?;
            }
        }
        if config.min_size != (0, 0) {
            client.send(&Request::SetSizeLimits {
                surface,
                min: config.min_size,
                max: (0, 0),
            })?;
        }
        Ok(Window {
            surface,
            format: if config.translucent {
                PixelFormat::Argb8888
            } else {
                PixelFormat::Xrgb8888
            },
            configure: None,
            buffer: None,
            frame_pending: false,
            close_requested: false,
        })
    }

    /// The surface id.
    pub fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// The last configure, if any.
    pub fn configure(&self) -> Option<Configure> {
        self.configure
    }

    /// True after the user asked to close the window.
    pub fn close_requested(&self) -> bool {
        self.close_requested
    }

    /// False while a frame is on its way to the screen: drawing now would
    /// be wasted work (frame pacing).
    pub fn can_draw(&self) -> bool {
        !self.frame_pending
    }

    /// Takes the window's bookkeeping events (configure, release, frame
    /// done, close). Returns true if the event belonged to this window.
    pub fn handle(&mut self, event: &Event) -> bool {
        match event {
            Event::Configure(c) if c.surface == self.surface => self.configure = Some(*c),
            Event::BufferReleased { buffer } => match &mut self.buffer {
                Some(b) if b.id == *buffer => b.busy = false,
                _ => return false,
            },
            Event::FrameDone { surface, .. } if *surface == self.surface => {
                self.frame_pending = false
            }
            Event::CloseRequested { surface } if *surface == self.surface => {
                self.close_requested = true
            }
            _ => return false,
        }
        true
    }

    /// Shows `pixels` (`width × height`, rows of `width * 4` bytes,
    /// `0xXXRRGGBB` little-endian – the BGRX canvas of `ultar-ui`; for a
    /// translucent window `0xAARRGGBB` premultiplied). Only the
    /// `damage` rectangles are transferred; an empty list sends everything.
    pub fn present(
        &mut self,
        client: &mut Client,
        pixels: &[u8],
        size: (u32, u32),
        damage: &[Rect],
    ) -> Result<(), ClientError> {
        let (width, height) = size;
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(ClientError::InvalidSize);
        }
        if pixels.len() < width as usize * height as usize * 4 {
            return Err(ClientError::InvalidSize);
        }
        let fresh = self.ensure_buffer(client, width, height)?;
        self.wait_released(client)?;
        let whole = [Rect::new(0, 0, width, height)];
        let rects: Vec<Rect> = if damage.is_empty() || fresh || damage.len() > MAX_DAMAGE {
            whole.to_vec()
        } else {
            damage
                .iter()
                .filter_map(|r| clip(r, width, height))
                .collect()
        };
        let Some(buffer) = self.buffer.as_mut() else {
            return Err(ClientError::InvalidSize);
        };
        for rect in &rects {
            upload(client, buffer, pixels, rect)?;
        }
        buffer.busy = true;
        let id = buffer.id;
        let surface = self.surface;
        let configure = self.configure.map_or(0, |c| c.serial);
        client.send(&Request::Attach {
            surface,
            buffer: id,
        })?;
        client.send(&Request::Damage { surface, rects })?;
        client.send(&Request::Frame { surface })?;
        client.send(&Request::Commit { surface, configure })?;
        self.frame_pending = true;
        Ok(())
    }

    /// Destroys the surface and its buffer.
    pub fn destroy(mut self, client: &mut Client) -> Result<(), ClientError> {
        if let Some(buffer) = self.buffer.take() {
            client.send(&Request::DestroyBuffer { buffer: buffer.id })?;
        }
        client.send(&Request::DestroySurface {
            surface: self.surface,
        })
    }

    /// Makes sure a buffer of the size exists; true if it is new.
    fn ensure_buffer(
        &mut self,
        client: &mut Client,
        width: u32,
        height: u32,
    ) -> Result<bool, ClientError> {
        if let Some(b) = &self.buffer {
            if b.width == width && b.height == height {
                return Ok(false);
            }
        }
        if let Some(old) = self.buffer.take() {
            client.send(&Request::DestroyBuffer { buffer: old.id })?;
        }
        let id = client.new_buffer_id();
        let format = self.format;
        let storage = match client.transport() {
            Transport::SharedMemory => {
                let shared = SharedBuffer::create(width as usize * height as usize * 4)?;
                let request = Request::CreateShmBuffer {
                    buffer: id,
                    width,
                    height,
                    stride: width * 4,
                    format,
                };
                client.send_with_fd(&request, shared.fd())?;
                Storage::Shared(shared)
            }
            Transport::Inline => {
                client.send(&Request::CreateInlineBuffer {
                    buffer: id,
                    width,
                    height,
                    format,
                })?;
                Storage::Inline
            }
        };
        self.buffer = Some(Buffer {
            id,
            width,
            height,
            storage,
            busy: false,
        });
        Ok(true)
    }

    fn wait_released(&mut self, client: &mut Client) -> Result<(), ClientError> {
        let deadline = Instant::now() + RELEASE_TIMEOUT;
        let mut others = Vec::new();
        while self.buffer.as_ref().is_some_and(|b| b.busy) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match client.next_event(Some(remaining)) {
                Ok(Some(event)) => {
                    if !self.handle(&event) || matches!(event, Event::Configure(_)) {
                        others.push(event);
                    }
                }
                Ok(None) => {
                    client.requeue(others);
                    return Err(ClientError::Timeout);
                }
                Err(e) => {
                    client.requeue(others);
                    return Err(e);
                }
            }
        }
        client.requeue(others);
        Ok(())
    }
}

fn clip(rect: &Rect, width: u32, height: u32) -> Option<Rect> {
    let x0 = rect.x.clamp(0, width as i32);
    let y0 = rect.y.clamp(0, height as i32);
    let x1 = (i64::from(rect.x) + i64::from(rect.width)).clamp(0, i64::from(width)) as i32;
    let y1 = (i64::from(rect.y) + i64::from(rect.height)).clamp(0, i64::from(height)) as i32;
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
}

/// Moves the pixels of `rect` into the buffer (shared memory) or sends them
/// (inline, in chunks of at most [`MAX_CHUNK`] bytes).
fn upload(
    client: &mut Client,
    buffer: &mut Buffer,
    pixels: &[u8],
    rect: &Rect,
) -> Result<(), ClientError> {
    let row_bytes = buffer.width as usize * 4;
    let (x, w) = (rect.x as usize, rect.width as usize);
    match &mut buffer.storage {
        Storage::Shared(shared) => {
            let dst = shared.bytes_mut();
            for row in rect.y as usize..rect.y as usize + rect.height as usize {
                let start = row * row_bytes + x * 4;
                dst[start..start + w * 4].copy_from_slice(&pixels[start..start + w * 4]);
            }
        }
        Storage::Inline => {
            let rows_per_chunk = (MAX_CHUNK / (w * 4)).max(1);
            let mut row = rect.y as usize;
            let end = rect.y as usize + rect.height as usize;
            while row < end {
                let n = rows_per_chunk.min(end - row);
                let mut data = Vec::with_capacity(n * w * 4);
                for r in row..row + n {
                    let start = r * row_bytes + x * 4;
                    data.extend_from_slice(&pixels[start..start + w * 4]);
                }
                client.send(&Request::WritePixels {
                    buffer: buffer.id,
                    rect: Rect::new(rect.x, row as i32, rect.width, n as u32),
                    data,
                })?;
                row += n;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_of_damage() {
        assert_eq!(
            clip(&Rect::new(-5, -5, 10, 10), 8, 8),
            Some(Rect::new(0, 0, 5, 5))
        );
        assert_eq!(
            clip(&Rect::new(6, 6, 10, 10), 8, 8),
            Some(Rect::new(6, 6, 2, 2))
        );
        assert_eq!(clip(&Rect::new(8, 0, 1, 1), 8, 8), None);
        assert_eq!(clip(&Rect::new(i32::MAX, 0, u32::MAX, 1), 8, 8), None);
        let config = WindowConfig::toplevel("t");
        assert_eq!(config.role, Role::Toplevel);
        assert!(!config.translucent);
        let sheet = WindowConfig::with_role(Role::Overlay, "", true);
        assert!(sheet.translucent && sheet.title.is_empty());
    }
}
