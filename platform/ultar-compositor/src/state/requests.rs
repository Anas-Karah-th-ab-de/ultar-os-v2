//! Handling of client requests. Every request is checked against the
//! client's own objects, its rights and its limits; anything wrong ends
//! the client with an error code – never the compositor.

use super::{ClientId, State, SurfaceKey};
use crate::geom::Rect;
use crate::pixels::Image;
use crate::surface::{Buffer, Storage, Surface};
use ultar_hal_gfx::shm::SealedMapping;
use ultar_surface::proto::{
    BufferId, ErrorCode, Event, PixelFormat, Request, Rights, Role, SurfaceId, MAX_DAMAGE, MAX_SIDE,
};

type Outcome = Result<(), ErrorCode>;

fn valid_size(width: u32, height: u32) -> bool {
    (1..=MAX_SIDE).contains(&width) && (1..=MAX_SIDE).contains(&height)
}

impl State {
    /// Decodes and applies one request of `client`.
    pub fn handle_frame(&mut self, client: ClientId, opcode: u16, body: &[u8]) {
        let request = match Request::decode(opcode, body) {
            Ok(r) => r,
            Err(_) => return self.fail(client, ErrorCode::Malformed),
        };
        let hello = self.clients.get(&client).is_some_and(|c| c.hello);
        let is_hello = matches!(request, Request::Hello { .. });
        let outcome = if is_hello == hello {
            // A second Hello, or anything before the first.
            Err(ErrorCode::Malformed)
        } else {
            self.apply(client, request)
        };
        if let Err(code) = outcome {
            self.fail(client, code);
        }
    }

    fn apply(&mut self, client: ClientId, request: Request) -> Outcome {
        match request {
            r @ (Request::ConfigureOutputs { .. }
            | Request::ConfirmOutputs { .. }
            | Request::IdentifyOutputs { .. }
            | Request::DetectOutputs) => self.output_request(client, r),
            Request::Hello { app_id, .. } => {
                if let Some(c) = self.clients.get_mut(&client) {
                    c.hello = true;
                    c.app_id = app_id;
                }
                self.welcome(client);
                Ok(())
            }
            Request::CreateSurface { surface, role } => self.create_surface(client, surface, role),
            Request::DestroySurface { surface } => {
                let key = self.own_surface(client, surface)?;
                self.destroy_surface(key);
                Ok(())
            }
            Request::SetTitle { surface, title } => {
                let key = self.own_surface(client, surface)?;
                if let Some(s) = self.surface_mut(key) {
                    s.title = super::text::clean_title(&title);
                }
                if let Some(i) = self.window_index(key) {
                    let f = self.windows[i].frame;
                    self.damage_logical(Rect::new(f.x, f.y, f.w, crate::decor::TITLEBAR));
                }
                Ok(())
            }
            Request::SetSizeLimits { surface, min, max } => {
                let key = self.own_surface(client, surface)?;
                let clamp = |(w, h): (u32, u32)| (w.min(MAX_SIDE), h.min(MAX_SIDE));
                if let Some(s) = self.surface_mut(key) {
                    s.limits.min = clamp(min);
                    s.limits.max = clamp(max);
                }
                Ok(())
            }
            other => self.apply_buffers(client, other),
        }
    }

    fn apply_buffers(&mut self, client: ClientId, request: Request) -> Outcome {
        match request {
            Request::CreateShmBuffer {
                buffer,
                width,
                height,
                stride,
                format,
            } => self.create_shm_buffer(client, buffer, (width, height, stride), format),
            Request::CreateInlineBuffer {
                buffer,
                width,
                height,
                format,
            } => {
                self.check_new_buffer(client, buffer, width, height)?;
                let image = Image::new(width, height, 0).ok_or(ErrorCode::Limit)?;
                self.add_buffer(
                    client,
                    buffer,
                    Buffer {
                        width,
                        height,
                        format,
                        storage: Storage::Inline(image),
                    },
                );
                Ok(())
            }
            Request::WritePixels { buffer, rect, data } => {
                let c = self
                    .clients
                    .get_mut(&client)
                    .ok_or(ErrorCode::UnknownObject)?;
                let b = c.buffers.get_mut(&buffer).ok_or(ErrorCode::UnknownObject)?;
                if b.write(&rect, &data) {
                    Ok(())
                } else {
                    Err(ErrorCode::BadBuffer)
                }
            }
            Request::DestroyBuffer { buffer } => {
                let c = self
                    .clients
                    .get_mut(&client)
                    .ok_or(ErrorCode::UnknownObject)?;
                let b = c.buffers.remove(&buffer).ok_or(ErrorCode::UnknownObject)?;
                c.bytes = c.bytes.saturating_sub(b.charge());
                for s in c.surfaces.values_mut() {
                    if s.pending_buffer == Some(buffer.0) {
                        s.pending_buffer = None;
                    }
                }
                Ok(())
            }
            other => self.apply_more(client, other),
        }
    }

    fn apply_more(&mut self, client: ClientId, request: Request) -> Outcome {
        match request {
            Request::Attach { surface, buffer } => {
                let key = self.own_surface(client, surface)?;
                if buffer.0 != 0
                    && !self
                        .clients
                        .get(&client)
                        .is_some_and(|c| c.buffers.contains_key(&buffer))
                {
                    return Err(ErrorCode::UnknownObject);
                }
                if let Some(s) = self.surface_mut(key) {
                    s.pending_buffer = Some(buffer.0);
                }
                Ok(())
            }
            Request::Damage { surface, rects } => {
                let key = self.own_surface(client, surface)?;
                if let Some(s) = self.surface_mut(key) {
                    for r in rects {
                        let w = r.width.min(MAX_SIDE) as i32;
                        let h = r.height.min(MAX_SIDE) as i32;
                        s.pending_damage.push(Rect::new(r.x, r.y, w, h));
                    }
                    if s.pending_damage.len() > MAX_DAMAGE {
                        let all = Rect::new(0, 0, MAX_SIDE as i32, MAX_SIDE as i32);
                        s.pending_damage = vec![all];
                    }
                }
                Ok(())
            }
            Request::Commit { surface, configure } => self.commit(client, surface, configure),
            Request::Frame { surface } => {
                let key = self.own_surface(client, surface)?;
                if let Some(s) = self.surface_mut(key) {
                    s.frame_requested = true;
                }
                Ok(())
            }
            Request::SetState { surface, state } => {
                let key = self.own_surface(client, surface)?;
                let index = self.window_index(key).ok_or(ErrorCode::UnknownObject)?;
                self.set_state_by(index, state, None, false);
                Ok(())
            }
            other => self.apply_input(client, other),
        }
    }

    fn apply_input(&mut self, client: ClientId, request: Request) -> Outcome {
        match request {
            Request::Move { surface, serial } => {
                self.start_move_request(client, surface, serial, None)
            }
            Request::Resize {
                surface,
                serial,
                edges,
            } => self.start_move_request(client, surface, serial, Some(edges.0)),
            Request::Pong { serial } => self.pong(client, serial),
            Request::TextInput {
                surface,
                enabled,
                purpose,
            } => {
                let key = self.own_surface(client, surface)?;
                if let Some(s) = self.surface_mut(key) {
                    s.text_input = enabled.then_some(purpose);
                }
                self.update_ime();
                Ok(())
            }
            Request::ImeCommit { text } => self.ime_commit(client, text),
            Request::Unlock { surface } => self.unlock(client, surface),
            Request::RequestActivation { serial } => {
                self.request_activation(client, serial);
                Ok(())
            }
            Request::Activate { surface, token } => self.activate(client, surface, token),
            Request::ListWindows => self.list_windows(client),
            Request::ShellFocus { surface } => self.shell_focus(client, surface),
            Request::WindowAction { window, action } => self.window_action(client, window, action),
            Request::SetDisplayMode { mode } => self.set_display_mode(client, mode),
            Request::SetKeyboardLayout { index } => self.set_keyboard_layout(client, index),
            Request::SetPhoneMode { on } => self.set_phone_mode(client, on),
            Request::WindowThumbnail { .. }
            | Request::SwitchWorkspace { .. }
            | Request::MoveToWorkspace { .. } => self.apply_overview(client, request),
            Request::Capture { output } => {
                let c = self
                    .clients
                    .get_mut(&client)
                    .ok_or(ErrorCode::UnknownObject)?;
                if !c.rights.contains(Rights::CAPTURE) {
                    return Err(ErrorCode::Permission);
                }
                if self.layout.get(output).is_none() {
                    return Err(ErrorCode::UnknownObject);
                }
                if let Some(c) = self.clients.get_mut(&client) {
                    c.capture = Some((output, 0));
                }
                Ok(())
            }
            other => self.apply_transfer(client, other),
        }
    }

    /// The client answered a ping: it is responsive again.
    fn pong(&mut self, client: ClientId, serial: u32) -> Outcome {
        let c = self
            .clients
            .get_mut(&client)
            .ok_or(ErrorCode::UnknownObject)?;
        if c.ping.is_some_and(|(s, _)| s == serial) {
            c.ping = None;
            self.set_unresponsive(client, false);
        }
        Ok(())
    }

    fn own_surface(&self, client: ClientId, surface: SurfaceId) -> Result<SurfaceKey, ErrorCode> {
        let c = self.clients.get(&client).ok_or(ErrorCode::UnknownObject)?;
        if c.surfaces.contains_key(&surface) {
            Ok(SurfaceKey { client, surface })
        } else {
            Err(ErrorCode::UnknownObject)
        }
    }

    fn create_surface(&mut self, client: ClientId, surface: SurfaceId, role: Role) -> Outcome {
        let limit = self.config.limits.surfaces;
        let c = self.clients.get(&client).ok_or(ErrorCode::UnknownObject)?;
        if surface.0 == 0 || c.surfaces.contains_key(&surface) {
            return Err(ErrorCode::UnknownObject);
        }
        if c.surfaces.len() >= limit {
            return Err(ErrorCode::Limit);
        }
        if !c.rights.contains(role.required_rights()) {
            return Err(ErrorCode::Permission);
        }
        if let Role::Popup { parent, .. } = role {
            // A popup belongs to an own window, or (shell only) to an own
            // panel: sheets, menus and the overview of the bar.
            let ok = c.surfaces.get(&parent).is_some_and(|p| {
                p.role == Role::Toplevel
                    || (matches!(p.role, Role::Panel { .. }) && c.rights.contains(Rights::SHELL))
            });
            if !ok {
                return Err(ErrorCode::UnknownObject);
            }
            let siblings = c
                .surfaces
                .values()
                .filter(|s| matches!(s.role, Role::Popup { parent: p, .. } if p == parent))
                .count();
            if siblings >= self.config.limits.popups {
                return Err(ErrorCode::Limit);
            }
        }
        if let Some(c) = self.clients.get_mut(&client) {
            c.surfaces.insert(surface, Surface::new(surface, role));
        }
        let key = SurfaceKey { client, surface };
        if matches!(role, Role::Toplevel | Role::SystemDialog) {
            self.create_toplevel(key);
        } else {
            self.layers.push(key);
            self.configure_layer(key);
        }
        Ok(())
    }

    fn check_new_buffer(
        &self,
        client: ClientId,
        buffer: BufferId,
        width: u32,
        height: u32,
    ) -> Outcome {
        let c = self.clients.get(&client).ok_or(ErrorCode::UnknownObject)?;
        if buffer.0 == 0 || c.buffers.contains_key(&buffer) {
            return Err(ErrorCode::UnknownObject);
        }
        if !valid_size(width, height) {
            return Err(ErrorCode::BadBuffer);
        }
        let bytes = u64::from(width) * u64::from(height) * 4;
        if c.buffers.len() >= self.config.limits.buffers || bytes > self.memory_room(client) {
            return Err(ErrorCode::Limit);
        }
        Ok(())
    }

    fn add_buffer(&mut self, client: ClientId, id: BufferId, buffer: Buffer) {
        if let Some(c) = self.clients.get_mut(&client) {
            c.bytes += buffer.charge();
            c.buffers.insert(id, buffer);
        }
    }

    fn create_shm_buffer(
        &mut self,
        client: ClientId,
        id: BufferId,
        dims: (u32, u32, u32),
        format: PixelFormat,
    ) -> Outcome {
        let fd = self.clients.get_mut(&client).and_then(|c| c.conn.take_fd());
        if !self.shm {
            return Err(ErrorCode::Permission);
        }
        let (width, height, stride) = dims;
        self.check_new_buffer(client, id, width, height)?;
        let row = u64::from(width) * 4;
        if u64::from(stride) < row || u64::from(stride) > row + 4096 {
            return Err(ErrorCode::BadBuffer);
        }
        let fd = fd.ok_or(ErrorCode::BadBuffer)?;
        let needed = u64::from(stride) * u64::from(height - 1) + row;
        let mapping = SealedMapping::map(fd, needed).map_err(|_| ErrorCode::BadBuffer)?;
        self.add_buffer(
            client,
            id,
            Buffer {
                width,
                height,
                format,
                storage: Storage::Shared { mapping, stride },
            },
        );
        Ok(())
    }

    fn commit(&mut self, client: ClientId, surface: SurfaceId, configure: u32) -> Outcome {
        let key = self.own_surface(client, surface)?;
        // A layer that shrinks or moves must repaint where it was.
        if let Some(old) = self.layer_rect(key) {
            self.damage_logical(old);
        }
        let room = self.memory_room(client);
        let c = self
            .clients
            .get_mut(&client)
            .ok_or(ErrorCode::UnknownObject)?;
        let s = c
            .surfaces
            .get_mut(&surface)
            .ok_or(ErrorCode::UnknownObject)?;
        s.ack(configure);
        let before = s.charge();
        let mut released = None;
        let damage = match s.pending_buffer.take() {
            Some(0) => {
                s.image = None;
                s.pending_damage.clear();
                Some(Vec::new())
            }
            Some(id) => {
                let buffer = c
                    .buffers
                    .get(&BufferId(id))
                    .ok_or(ErrorCode::UnknownObject)?;
                let grow =
                    (u64::from(buffer.width) * u64::from(buffer.height) * 4).saturating_sub(before);
                if grow > room {
                    return Err(ErrorCode::Limit);
                }
                let rects = s.apply(Some(buffer)).ok_or(ErrorCode::BadBuffer)?;
                s.shared = matches!(buffer.storage, crate::surface::Storage::Shared { .. });
                released = Some(BufferId(id));
                Some(rects)
            }
            None => s.apply(None),
        };
        let after = s.charge();
        c.bytes = (c.bytes + after).saturating_sub(before);
        if let Some(buffer) = released {
            self.send(client, Event::BufferReleased { buffer });
        }
        let rects = damage.unwrap_or_default();
        self.surface_damaged(key, &rects);
        if self
            .surface(key)
            .is_some_and(|s| s.role == Role::Lock && s.image.is_some())
        {
            // A lock surface locks; a new one (after a crash) takes input.
            if self.locked() {
                self.update_ime();
            }
            self.lock();
        }
        Ok(())
    }

    /// Turns surface-pixel damage of a commit into output damage.
    fn surface_damaged(&mut self, key: SurfaceKey, rects: &[Rect]) {
        let scale = self
            .surface(key)
            .map_or(1.0, |s| f64::from(s.scale_milli) / 1000.0);
        let content = if self.window_index(key).is_some() {
            self.window_committed(key)
        } else {
            let old = self.layer_rect(key);
            if let Some(r) = old {
                // Layers can change size with every commit: damage all.
                self.damage_logical(r);
            }
            if self.surface(key).is_some_and(|s| s.image.is_none()) {
                return;
            }
            None
        };
        let Some(content) = content else {
            return;
        };
        for r in rects {
            let logical = Rect::new(
                (f64::from(r.x) / scale).floor() as i32,
                (f64::from(r.y) / scale).floor() as i32,
                (f64::from(r.w) / scale).ceil() as i32 + 1,
                (f64::from(r.h) / scale).ceil() as i32 + 1,
            );
            if let Some(visible) = logical.translate(content.x, content.y).intersect(&content) {
                self.damage_logical(visible);
            }
        }
    }
}
