//! `wl_compositor`, `wl_surface`, `wl_region`, `wl_callback`: surfaces and
//! their commits. A commit copies the client's pixels (and gives the buffer
//! back at once), then shows the window's whole tree on its native surface.

use crate::limits::{MAX_CALLBACKS, MAX_DAMAGE, MAX_SURFACES};
use crate::pixels::{self, Image};
use crate::state::{refuse, Content, Role, State, Surf, NO_MEMORY};
use ultar_surface::proto::Rect;
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::{
    wl_callback::WlCallback, wl_compositor, wl_compositor::WlCompositor, wl_region::WlRegion,
    wl_surface, wl_surface::WlSurface,
};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

/// Version of `wl_compositor` offered (6: preferred buffer scale).
pub const VERSION: u32 = 6;

impl GlobalDispatch<WlCompositor, ()> for State {
    fn bind(
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlCompositor>,
        _: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WlCompositor, ()> for State {
    fn request(
        state: &mut State,
        client: &Client,
        _: &WlCompositor,
        request: wl_compositor::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        match request {
            wl_compositor::Request::CreateSurface { id } => {
                if state.counts(&client.id()).surfaces >= MAX_SURFACES {
                    refuse(&state.dh, client, NO_MEMORY, "too many surfaces");
                    return;
                }
                let surface = data_init.init(id, ());
                state.counts(&client.id()).surfaces += 1;
                state
                    .objects
                    .surfaces
                    .insert(surface.id(), Surf::new(surface, client.id()));
            }
            wl_compositor::Request::CreateRegion { id } => {
                data_init.init(id, ());
            }
            _ => {}
        }
    }
}

impl Dispatch<WlRegion, ()> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &WlRegion,
        _: wayland_server::protocol::wl_region::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
    }
}

impl Dispatch<WlCallback, ()> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &WlCallback,
        _: wayland_server::protocol::wl_callback::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
    }
}

impl Dispatch<WlSurface, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        surface: &WlSurface,
        request: wl_surface::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let id = surface.id();
        if let wl_surface::Request::Frame { callback } = request {
            let cb = data_init.init(callback, ());
            frame(state, surface, &id, cb);
            return;
        }
        if let wl_surface::Request::Commit = request {
            commit(state, &id);
            return;
        }
        let Some(s) = state.objects.surfaces.get_mut(&id) else {
            return;
        };
        match request {
            wl_surface::Request::Attach { buffer, .. } => s.pending.buffer = Some(buffer),
            wl_surface::Request::Damage {
                x,
                y,
                width,
                height,
            } => push_damage(&mut s.pending.damage, x, y, width, height),
            wl_surface::Request::DamageBuffer {
                x,
                y,
                width,
                height,
            } => push_damage(&mut s.pending.damage_buffer, x, y, width, height),
            wl_surface::Request::SetBufferScale { scale } => {
                if scale < 1 {
                    surface.post_error(wl_surface::Error::InvalidScale, "scale below 1");
                } else {
                    s.pending.scale = Some(scale.min(4));
                }
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut State, client: ClientId, resource: &WlSurface, _: &()) {
        remove_surface(state, &client, &resource.id());
    }
}

fn frame(state: &mut State, surface: &WlSurface, id: &ObjectId, cb: WlCallback) {
    let Some(s) = state.objects.surfaces.get_mut(id) else {
        return;
    };
    if s.pending.callbacks.len() >= MAX_CALLBACKS {
        surface.post_error(0u32, "too many frame callbacks");
        return;
    }
    s.pending.callbacks.push(cb);
}

fn push_damage(list: &mut Vec<Rect>, x: i32, y: i32, w: i32, h: i32) {
    if w <= 0 || h <= 0 {
        return;
    }
    if list.len() > MAX_DAMAGE {
        // Too many: one rectangle that covers everything.
        list.clear();
        list.push(Rect::new(0, 0, u32::MAX >> 1, u32::MAX >> 1));
        return;
    }
    list.push(Rect::new(x, y, w as u32, h as u32));
}

/// Applies the pending state of `id`.
pub fn commit(state: &mut State, id: &ObjectId) {
    let Some(s) = state.objects.surfaces.get_mut(id) else {
        return;
    };
    let pending = std::mem::take(&mut s.pending);
    if let Some(geometry) = s.xdg.pending_geometry.take() {
        s.xdg.geometry = Some(geometry);
    }
    s.callbacks.extend(pending.callbacks);
    let scale = pending
        .scale
        .unwrap_or_else(|| s.content.as_ref().map_or(1, |c| c.scale));
    let damage = take_pixels(
        state,
        id,
        pending.buffer,
        scale,
        pending.damage,
        pending.damage_buffer,
    );
    crate::xdg::after_commit(state, id);
    let root = root_of(state, id);
    let shown = match (root, damage) {
        (Some(root), Some(damage)) => {
            present(state, &root, if root == *id { damage } else { Vec::new() })
        }
        _ => false,
    };
    if !shown {
        done_now(state, id);
    }
}

/// Copies the new buffer's damaged pixels; `None` if nothing changed on screen.
fn take_pixels(
    state: &mut State,
    id: &ObjectId,
    buffer: Option<Option<wayland_server::protocol::wl_buffer::WlBuffer>>,
    scale: i32,
    damage: Vec<Rect>,
    damage_buffer: Vec<Rect>,
) -> Option<Vec<Rect>> {
    let buffer = match buffer {
        None => {
            let s = state.objects.surfaces.get_mut(id)?;
            if let Some(c) = &mut s.content {
                c.scale = scale;
            }
            return None;
        }
        Some(None) => {
            if let Some(s) = state.objects.surfaces.get_mut(id) {
                s.content = None;
            }
            crate::xdg::unmapped(state, id);
            return None;
        }
        Some(Some(b)) => b,
    };
    let info = state.objects.buffers.get(&buffer.id())?;
    let (fd, layout) = (info.fd.clone(), info.layout);
    let s = state.objects.surfaces.get_mut(id)?;
    let same = s
        .content
        .as_ref()
        .is_some_and(|c| (c.image.width, c.image.height) == (layout.width, layout.height));
    let mut rects: Vec<Rect> = damage
        .iter()
        .map(|r| pixels::scale_rect(r, scale))
        .chain(damage_buffer)
        .collect();
    if !same || rects.is_empty() {
        rects = vec![Rect::new(0, 0, layout.width, layout.height)];
    }
    let content = s.content.get_or_insert_with(Content::default);
    if !same {
        content.image = Image::new(layout.width, layout.height);
    }
    content.scale = scale;
    content.translucent = layout.alpha;
    pixels::read_rects(
        std::os::fd::AsFd::as_fd(&*fd),
        &layout,
        &rects,
        &mut content.image,
    );
    buffer.release();
    Some(rects)
}

/// The window (toplevel or popup surface) a surface belongs to.
pub fn root_of(state: &State, id: &ObjectId) -> Option<ObjectId> {
    let mut cur = id.clone();
    for _ in 0..8 {
        let s = state.objects.surfaces.get(&cur)?;
        match &s.role {
            Role::Sub { parent, .. } => cur = parent.clone(),
            Role::Toplevel(_) | Role::Popup(_) => return Some(cur),
            _ => return None,
        }
    }
    None
}

/// Shows window `root` on its native surface; false if it is not mapped.
pub fn present(state: &mut State, root: &ObjectId, damage: Vec<Rect>) -> bool {
    let Some(s) = state.objects.surfaces.get(root) else {
        return false;
    };
    let (Some(native), Some(content)) = (s.native(), s.content.as_ref()) else {
        return false;
    };
    if !s.xdg.committed {
        return false;
    }
    let b = content.scale.max(1);
    let mut image = content.image.clone();
    let translucent = content.translucent;
    let composed = compose_children(state, root, &mut image, b);
    let Some(s) = state.objects.surfaces.get(root) else {
        return false;
    };
    let full = Rect::new(0, 0, image.width, image.height);
    let region = s.xdg.geometry.map_or(full, |g| {
        pixels::clip(&pixels::scale_rect(&g, b), image.width, image.height).unwrap_or(full)
    });
    let target = crate::xdg::native_scale(state, root);
    let (tw, th) = target_size(region, b, target);
    let mut damage = if composed || (tw, th) != (region.width, region.height) {
        Vec::new()
    } else {
        damage
    };
    for d in &mut damage {
        d.x -= region.x;
        d.y -= region.y;
    }
    let cropped = if region == full {
        image
    } else {
        pixels::crop(&image, region)
    };
    let out = pixels::scale(&cropped, tw, th);
    let configure = s.xdg.acked;
    let r = state
        .native
        .present(native, out, damage, configure, translucent);
    state.native_result(r).is_some()
}

/// Device pixels of a region of `b`-scaled buffer pixels at the native scale (× 1000).
fn target_size(region: Rect, b: i32, scale_milli: u32) -> (u32, u32) {
    let px = |v: u32| {
        ((f64::from(v) / f64::from(b)) * f64::from(scale_milli) / 1000.0)
            .round()
            .max(1.0) as u32
    };
    (px(region.width), px(region.height))
}

fn compose_children(state: &State, id: &ObjectId, image: &mut Image, b: i32) -> bool {
    let Some(s) = state.objects.surfaces.get(id) else {
        return false;
    };
    let mut any = false;
    for child in &s.children {
        let Some(c) = state.objects.surfaces.get(child) else {
            continue;
        };
        let (Role::Sub { pos, .. }, Some(content)) = (&c.role, &c.content) else {
            continue;
        };
        let img = if content.scale == b {
            content.image.clone()
        } else {
            pixels::scale(
                &content.image,
                content.image.width * b as u32 / content.scale.max(1) as u32,
                content.image.height * b as u32 / content.scale.max(1) as u32,
            )
        };
        pixels::blend(image, &img, pos.0 * b, pos.1 * b);
        compose_children(state, child, image, b);
        any = true;
    }
    any
}

/// Sends the frame callbacks of `id` now (nothing to wait for on screen).
pub fn done_now(state: &mut State, id: &ObjectId) {
    let time = state.now_ms();
    if let Some(s) = state.objects.surfaces.get_mut(id) {
        for cb in s.callbacks.drain(..) {
            cb.done(time);
        }
    }
}

/// Sends the frame callbacks of a window and its subsurfaces (its frame is on screen).
pub fn done_tree(state: &mut State, root: &ObjectId, time: u32) {
    let children = state
        .objects
        .surfaces
        .get(root)
        .map(|s| s.children.clone())
        .unwrap_or_default();
    if let Some(s) = state.objects.surfaces.get_mut(root) {
        for cb in s.callbacks.drain(..) {
            cb.done(time);
        }
    }
    for c in children {
        done_tree(state, &c, time);
    }
}

fn remove_surface(state: &mut State, client: &ClientId, id: &ObjectId) {
    let Some(s) = state.objects.surfaces.remove(id) else {
        return;
    };
    if let Some(native) = s.native() {
        state.objects.by_native.remove(&native);
        let r = state.native.destroy(native);
        state.native_result(r);
    }
    if let Role::Sub { parent, .. } = &s.role {
        if let Some(p) = state.objects.surfaces.get_mut(parent) {
            p.children.retain(|c| c != id);
        }
    }
    for child in s.children {
        if let Some(c) = state.objects.surfaces.get_mut(&child) {
            c.role = Role::None;
        }
    }
    state.seat.forget(id);
    let counts = state.counts(client);
    counts.surfaces = counts.surfaces.saturating_sub(1);
}
