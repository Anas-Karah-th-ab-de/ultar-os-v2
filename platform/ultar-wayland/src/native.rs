//! The bridge's side of the compositor: one ordinary client connection
//! (identity and rights come from the app's uid, as for any app). Every
//! Wayland window becomes a native surface; pictures go up through two
//! buffers per surface so the app never waits for the compositor.

use crate::pixels::Image;
use std::collections::HashMap;
use ultar_hal_gfx::shm::SharedBuffer;
use ultar_surface::client::{Client, ClientError};
use ultar_surface::proto::{
    BufferId, PixelFormat, Rect, Request, Role, SurfaceId, Transport, MAX_CHUNK, MAX_DAMAGE,
    MAX_SIDE,
};

/// Buffers kept per surface.
const BUFFERS: usize = 2;
/// Native surfaces at most (two buffers each stay within the compositor's
/// 64 buffers and 64 surfaces per connection).
pub const MAX_NATIVE_SURFACES: usize = 30;
/// Native popups at most (the compositor allows 8 per connection).
pub const MAX_NATIVE_POPUPS: usize = 8;

enum Storage {
    Shared(SharedBuffer),
    Inline,
}

struct Buf {
    id: BufferId,
    width: u32,
    height: u32,
    format: PixelFormat,
    storage: Storage,
    busy: bool,
    /// Damage of frames this buffer has not received yet.
    missing: Vec<Rect>,
}

/// A frame waiting for a free buffer.
struct Waiting {
    image: Image,
    damage: Vec<Rect>,
    configure: u32,
    translucent: bool,
}

struct Surf {
    popup: bool,
    buffers: Vec<Buf>,
    waiting: Option<Waiting>,
}

/// The compositor connection and the native surfaces.
///
/// Every request stays inside the compositor's limits: a refused request
/// would end the connection, and with it every window of the app. What
/// does not fit is refused here instead (a window too many, a frame too big).
pub struct Native {
    /// The connection.
    pub client: Client,
    surfaces: HashMap<SurfaceId, Surf>,
    /// Bytes of all native buffers.
    bytes: u64,
}

impl Native {
    /// Wraps a connected client.
    pub fn new(client: Client) -> Native {
        Native {
            client,
            surfaces: HashMap::new(),
            bytes: 0,
        }
    }

    /// Whether another native surface of `role` fits.
    pub fn room(&self, role: Role) -> bool {
        let popups = self.surfaces.values().filter(|s| s.popup).count();
        let popup = matches!(role, Role::Popup { .. });
        self.surfaces.len() < MAX_NATIVE_SURFACES && (!popup || popups < MAX_NATIVE_POPUPS)
    }

    /// Bytes the buffers may use together (an eighth stays free).
    fn budget(&self) -> u64 {
        self.client.max_buffer_bytes() / 8 * 7
    }

    /// Creates a native surface (call [`Native::room`] first).
    pub fn create(&mut self, role: Role, title: &str) -> Result<SurfaceId, ClientError> {
        let surface = self.client.new_surface_id();
        self.client
            .send(&Request::CreateSurface { surface, role })?;
        if !title.is_empty() {
            self.set_title(surface, title)?;
        }
        if role == Role::Toplevel {
            if let Some(token) = self.client.take_activation_token() {
                self.client.send(&Request::Activate { surface, token })?;
            }
        }
        let popup = matches!(role, Role::Popup { .. });
        self.surfaces.insert(
            surface,
            Surf {
                popup,
                buffers: Vec::new(),
                waiting: None,
            },
        );
        Ok(surface)
    }

    /// Sets a title (see [`short_title`]).
    pub fn set_title(&mut self, surface: SurfaceId, title: &str) -> Result<(), ClientError> {
        self.client.send(&Request::SetTitle {
            surface,
            title: short_title(title),
        })
    }

    /// Destroys a native surface and its buffers.
    pub fn destroy(&mut self, surface: SurfaceId) -> Result<(), ClientError> {
        if let Some(s) = self.surfaces.remove(&surface) {
            for b in s.buffers {
                self.bytes = self.bytes.saturating_sub(b.bytes());
                self.client.send(&Request::DestroyBuffer { buffer: b.id })?;
            }
            self.client.send(&Request::DestroySurface { surface })?;
        }
        Ok(())
    }

    /// Shows `image` on `surface` (damage in its pixels; empty = all),
    /// acknowledging the native `configure` serial. Waits for a free buffer
    /// without blocking: the newest frame is kept and sent on release.
    pub fn present(
        &mut self,
        surface: SurfaceId,
        image: Image,
        damage: Vec<Rect>,
        configure: u32,
        translucent: bool,
    ) -> Result<(), ClientError> {
        let Some(s) = self.surfaces.get_mut(&surface) else {
            return Ok(());
        };
        let free = s.buffers.iter().any(|b| !b.busy) || s.buffers.len() < BUFFERS;
        if !free {
            let damage = match s.waiting.take() {
                Some(_) => Vec::new(),
                None => damage,
            };
            s.waiting = Some(Waiting {
                image,
                damage,
                configure,
                translucent,
            });
            return Ok(());
        }
        self.send_frame(surface, &image, &damage, configure, translucent)
    }

    /// A buffer came back; a waiting frame goes out now.
    pub fn released(&mut self, buffer: BufferId) -> Result<(), ClientError> {
        let mut owner = None;
        for (id, s) in &mut self.surfaces {
            if let Some(b) = s.buffers.iter_mut().find(|b| b.id == buffer) {
                b.busy = false;
                owner = Some(*id);
            }
        }
        let Some(surface) = owner else {
            return Ok(());
        };
        let waiting = self
            .surfaces
            .get_mut(&surface)
            .and_then(|s| s.waiting.take());
        match waiting {
            Some(w) => self.send_frame(surface, &w.image, &w.damage, w.configure, w.translucent),
            None => Ok(()),
        }
    }

    fn send_frame(
        &mut self,
        surface: SurfaceId,
        image: &Image,
        damage: &[Rect],
        configure: u32,
        translucent: bool,
    ) -> Result<(), ClientError> {
        let whole = [image.rect()];
        let damage: Vec<Rect> = if damage.is_empty() || damage.len() > MAX_DAMAGE {
            whole.to_vec()
        } else {
            damage.to_vec()
        };
        let format = if translucent {
            PixelFormat::Argb8888
        } else {
            PixelFormat::Xrgb8888
        };
        let Some(index) = self.pick_buffer(surface, image.width, image.height, format)? else {
            // Too big for what is left: the window keeps its last picture.
            return Ok(());
        };
        let transport = self.client.transport();
        let Some(s) = self.surfaces.get_mut(&surface) else {
            return Ok(());
        };
        for b in s.buffers.iter_mut() {
            b.missing.extend_from_slice(&damage);
        }
        let b = &mut s.buffers[index];
        let upload = if b.missing.len() > MAX_DAMAGE {
            whole.to_vec()
        } else {
            std::mem::take(&mut b.missing)
        };
        b.missing.clear();
        b.busy = true;
        let id = b.id;
        upload_rects(&mut self.client, transport, b, image, &upload)?;
        self.client.send(&Request::Attach {
            surface,
            buffer: id,
        })?;
        self.client.send(&Request::Damage {
            surface,
            rects: damage,
        })?;
        self.client.send(&Request::Frame { surface })?;
        self.client.send(&Request::Commit { surface, configure })
    }

    /// Index of a free buffer of the size, making or remaking one
    /// (`None`: it does not fit the limits).
    fn pick_buffer(
        &mut self,
        surface: SurfaceId,
        w: u32,
        h: u32,
        format: PixelFormat,
    ) -> Result<Option<usize>, ClientError> {
        let transport = self.client.transport();
        let need = u64::from(w) * u64::from(h) * 4;
        let side = (1..=MAX_SIDE).contains(&w) && (1..=MAX_SIDE).contains(&h);
        let Some(s) = self.surfaces.get_mut(&surface) else {
            return Ok(None);
        };
        if let Some(i) = s
            .buffers
            .iter()
            .position(|b| !b.busy && (b.width, b.height, b.format) == (w, h, format))
        {
            return Ok(Some(i));
        }
        if let Some(i) = s.buffers.iter().position(|b| !b.busy) {
            let old = s.buffers.remove(i);
            self.bytes = self.bytes.saturating_sub(old.bytes());
            self.client
                .send(&Request::DestroyBuffer { buffer: old.id })?;
        }
        if !side || self.bytes + need > self.budget() {
            return Ok(None);
        }
        let id = self.client.new_buffer_id();
        let storage = new_storage(&mut self.client, transport, id, (w, h), format)?;
        self.bytes += need;
        let whole = vec![Rect::new(0, 0, w, h)];
        let Some(s) = self.surfaces.get_mut(&surface) else {
            return Ok(None);
        };
        s.buffers.push(Buf {
            id,
            width: w,
            height: h,
            format,
            storage,
            busy: false,
            missing: whole,
        });
        Ok(Some(s.buffers.len() - 1))
    }
}

/// The memory of a new native buffer: shared (sealed memory file) or inline.
fn new_storage(
    client: &mut Client,
    transport: Transport,
    id: BufferId,
    (width, height): (u32, u32),
    format: PixelFormat,
) -> Result<Storage, ClientError> {
    match transport {
        Transport::SharedMemory => {
            let shared = SharedBuffer::create(width as usize * height as usize * 4)?;
            let stride = width * 4;
            let request = Request::CreateShmBuffer {
                buffer: id,
                width,
                height,
                stride,
                format,
            };
            client.send_with_fd(&request, shared.fd())?;
            Ok(Storage::Shared(shared))
        }
        Transport::Inline => {
            client.send(&Request::CreateInlineBuffer {
                buffer: id,
                width,
                height,
                format,
            })?;
            Ok(Storage::Inline)
        }
    }
}

/// A title cut at a character boundary to the compositor's limit, without
/// control characters.
pub fn short_title(title: &str) -> String {
    let mut end = title.len().min(crate::limits::MAX_TITLE);
    while !title.is_char_boundary(end) {
        end -= 1;
    }
    title[..end].chars().filter(|c| !c.is_control()).collect()
}

impl Buf {
    fn bytes(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * 4
    }
}

fn upload_rects(
    client: &mut Client,
    transport: Transport,
    b: &mut Buf,
    image: &Image,
    rects: &[Rect],
) -> Result<(), ClientError> {
    for r in rects {
        let Some(r) = crate::pixels::clip(r, image.width, image.height) else {
            continue;
        };
        match (&mut b.storage, transport) {
            (Storage::Shared(shared), _) => {
                let dst = shared.bytes_mut();
                for y in r.y as u32..r.y as u32 + r.height {
                    let row = (y * image.width) as usize;
                    for x in r.x as u32..r.x as u32 + r.width {
                        let at = (row + x as usize) * 4;
                        dst[at..at + 4]
                            .copy_from_slice(&image.data[row + x as usize].to_le_bytes());
                    }
                }
            }
            (Storage::Inline, _) => write_inline(client, b.id, image, &r)?,
        }
    }
    Ok(())
}

fn write_inline(
    client: &mut Client,
    buffer: BufferId,
    image: &Image,
    r: &Rect,
) -> Result<(), ClientError> {
    let rows_per_chunk = (MAX_CHUNK / (r.width as usize * 4)).max(1);
    let mut y = r.y as u32;
    let end = r.y as u32 + r.height;
    while y < end {
        let n = (rows_per_chunk as u32).min(end - y);
        let mut data = Vec::with_capacity(n as usize * r.width as usize * 4);
        for row in y..y + n {
            let at = (row * image.width + r.x as u32) as usize;
            for px in &image.data[at..at + r.width as usize] {
                data.extend_from_slice(&px.to_le_bytes());
            }
        }
        client.send(&Request::WritePixels {
            buffer,
            rect: Rect::new(r.x, y as i32, r.width, n),
            data,
        })?;
        y += n;
    }
    Ok(())
}
