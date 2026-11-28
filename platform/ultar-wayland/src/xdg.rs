//! `xdg_wm_base`, `xdg_surface`, `xdg_toplevel`: windows. Each toplevel is
//! a native toplevel; native configures become xdg configures, and the
//! app's acknowledgement goes back with its next commit.

use crate::state::{Role, State, Toplevel};
use ultar_surface::proto::{Configure, Request, ResizeEdges, Role as NativeRole, WindowState};
use wayland_protocols::xdg::shell::server::{
    xdg_surface, xdg_surface::XdgSurface, xdg_toplevel, xdg_toplevel::XdgToplevel, xdg_wm_base,
    xdg_wm_base::XdgWmBase,
};
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::wl_surface::WlSurface;
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};

/// Version of `xdg_wm_base` offered.
pub const VERSION: u32 = 5;

impl GlobalDispatch<XdgWmBase, ()> for State {
    fn bind(
        state: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<XdgWmBase>,
        _: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        let base = data_init.init(resource, ());
        state.objects.wm_bases.retain(Resource::is_alive);
        if state.objects.wm_bases.len() < crate::limits::MAX_CLIENTS * 4 {
            state.objects.wm_bases.push(base);
        }
    }
}

/// The compositor checks that the app still answers: the app is asked in
/// turn, so a hung app shows as "not responding".
pub fn ping(state: &mut State, native: u32) {
    state.objects.wm_bases.retain(Resource::is_alive);
    if state.objects.wm_bases.is_empty() {
        let r = state.native.client.send(&Request::Pong { serial: native });
        state.native_result(r);
        return;
    }
    let serial = state.serials.next(0);
    state.serials.ping = Some((serial, native));
    for b in &state.objects.wm_bases {
        b.ping(serial);
    }
}

fn pong(state: &mut State, serial: u32) {
    if let Some((_, native)) = state.serials.ping.filter(|(wl, _)| *wl == serial) {
        state.serials.ping = None;
        let r = state.native.client.send(&Request::Pong { serial: native });
        state.native_result(r);
    }
}

impl Dispatch<XdgWmBase, ()> for State {
    fn request(
        state: &mut State,
        client: &Client,
        base: &XdgWmBase,
        request: xdg_wm_base::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        match request {
            xdg_wm_base::Request::CreatePositioner { id } => {
                crate::popup::create_positioner(state, client, id, data_init)
            }
            xdg_wm_base::Request::Pong { serial } => pong(state, serial),
            xdg_wm_base::Request::GetXdgSurface { id, surface } => {
                let Some(s) = state.objects.surfaces.get_mut(&surface.id()) else {
                    base.post_error(xdg_wm_base::Error::Role, "unknown surface");
                    return;
                };
                if !matches!(s.role, Role::None) || s.xdg.res.is_some() {
                    base.post_error(xdg_wm_base::Error::Role, "the surface has a role");
                    return;
                }
                if s.content.is_some() {
                    base.post_error(
                        xdg_wm_base::Error::InvalidSurfaceState,
                        "buffer before configure",
                    );
                    return;
                }
                s.xdg.res = Some(data_init.init(id, surface));
            }
            _ => {}
        }
    }
}

impl Dispatch<XdgSurface, WlSurface> for State {
    fn request(
        state: &mut State,
        client: &Client,
        xs: &XdgSurface,
        request: xdg_surface::Request,
        surface: &WlSurface,
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let id = surface.id();
        let free = state
            .objects
            .surfaces
            .get(&id)
            .is_some_and(|s| matches!(s.role, Role::None));
        match request {
            xdg_surface::Request::GetToplevel { .. } | xdg_surface::Request::GetPopup { .. }
                if !free =>
            {
                xs.post_error(
                    xdg_surface::Error::AlreadyConstructed,
                    "the surface has a role already",
                );
            }
            xdg_surface::Request::GetToplevel { id: new } => {
                let res = data_init.init(new, surface.clone());
                make_toplevel(state, client, &id, res);
            }
            xdg_surface::Request::GetPopup {
                id: new,
                parent,
                positioner,
            } => {
                let res = data_init.init(new, surface.clone());
                let parent = parent.and_then(|p| p.data::<WlSurface>().map(Resource::id));
                crate::popup::get_popup(state, surface, parent, &positioner, res);
            }
            xdg_surface::Request::SetWindowGeometry {
                x,
                y,
                width,
                height,
            } => {
                if width <= 0 || height <= 0 {
                    xs.post_error(xdg_surface::Error::InvalidSize, "geometry without size");
                } else if let Some(s) = state.objects.surfaces.get_mut(&id) {
                    s.xdg.pending_geometry = Some(ultar_surface::proto::Rect::new(
                        x,
                        y,
                        width as u32,
                        height as u32,
                    ));
                }
            }
            xdg_surface::Request::AckConfigure { serial } => {
                if let Some(s) = state.objects.surfaces.get_mut(&id) {
                    if let Some(&(_, native)) = s.xdg.serials.iter().find(|(w, _)| *w == serial) {
                        s.xdg.acked = native;
                        s.xdg.serials.retain(|(w, _)| *w > serial);
                    }
                }
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut State, _: ClientId, _: &XdgSurface, surface: &WlSurface) {
        if let Some(s) = state.objects.surfaces.get_mut(&surface.id()) {
            s.xdg = Default::default();
        }
    }
}

fn make_toplevel(state: &mut State, client: &Client, id: &ObjectId, res: XdgToplevel) {
    if !state.native.room(NativeRole::Toplevel) {
        crate::state::refuse(
            &state.dh,
            client,
            crate::state::NO_MEMORY,
            "too many windows",
        );
        return;
    }
    let r = state.native.create(NativeRole::Toplevel, "");
    let Some(native) = state.native_result(r) else {
        return;
    };
    state.objects.by_native.insert(native, id.clone());
    if let Some(s) = state.objects.surfaces.get_mut(id) {
        s.role = Role::Toplevel(Box::new(Toplevel {
            res,
            native,
            configure: None,
            decoration: None,
            client_header: false,
            title: String::new(),
        }));
    }
}

/// After every commit: the initial commit of a window starts its configure
/// sequence (and maps a popup).
pub fn after_commit(state: &mut State, id: &ObjectId) {
    let Some(s) = state.objects.surfaces.get_mut(id) else {
        return;
    };
    if s.xdg.res.is_none() || s.xdg.committed {
        return;
    }
    match &s.role {
        Role::Toplevel(_) => {
            s.xdg.committed = true;
            crate::extras::decide_decoration(state, id);
            send_configure(state, id);
        }
        Role::Popup(_) => {
            s.xdg.committed = true;
            crate::popup::map(state, id);
        }
        _ => {}
    }
}

/// A window lost its buffer: hidden until it commits anew.
pub fn unmapped(state: &mut State, id: &ObjectId) {
    let is_popup = matches!(
        state.objects.surfaces.get(id).map(|s| &s.role),
        Some(Role::Popup(_))
    );
    if is_popup {
        crate::popup::unmap(state, id);
        return;
    }
    let Some(s) = state.objects.surfaces.get_mut(id) else {
        return;
    };
    let Role::Toplevel(t) = &mut s.role else {
        return;
    };
    let (old, title) = (t.native, t.title.clone());
    s.xdg.committed = false;
    state.objects.by_native.remove(&old);
    let r = state
        .native
        .destroy(old)
        .and_then(|_| state.native.create(NativeRole::Toplevel, &title));
    let Some(native) = state.native_result(r) else {
        return;
    };
    state.objects.by_native.insert(native, id.clone());
    if let Some(Role::Toplevel(t)) = state.objects.surfaces.get_mut(id).map(|s| &mut s.role) {
        t.native = native;
        t.configure = None;
    }
}

/// A native configure arrived for a toplevel.
pub fn native_configure(state: &mut State, id: &ObjectId, c: Configure) {
    if let Some(Role::Toplevel(t)) = state.objects.surfaces.get_mut(id).map(|s| &mut s.role) {
        t.configure = Some(c);
        send_configure(state, id);
    } else if let Some(s) = state.objects.surfaces.get_mut(id) {
        // Popups: acknowledge the newest at once (their size is the app's).
        s.xdg.acked = c.serial;
    }
}

/// Sends the toplevel's newest configure (after its initial commit).
pub fn send_configure(state: &mut State, id: &ObjectId) {
    let Some(s) = state.objects.surfaces.get(id) else {
        return;
    };
    let Role::Toplevel(t) = &s.role else {
        return;
    };
    let (Some(c), true) = (t.configure, s.xdg.committed) else {
        return;
    };
    let mut states: Vec<u8> = Vec::new();
    let mut put = |v: xdg_toplevel::State| states.extend_from_slice(&(v as u32).to_ne_bytes());
    match c.state {
        WindowState::Maximized => put(xdg_toplevel::State::Maximized),
        WindowState::Fullscreen => put(xdg_toplevel::State::Fullscreen),
        _ => {}
    }
    if c.activated {
        put(xdg_toplevel::State::Activated);
    }
    t.res.configure(c.width as i32, c.height as i32, states);
    crate::output::enter_primary(state, id);
    send_surface_configure(state, id, c.serial);
}

/// `xdg_surface.configure` standing for a native serial.
pub fn send_surface_configure(state: &mut State, id: &ObjectId, native: u32) {
    let serial = state.serials.next(0);
    let Some(s) = state.objects.surfaces.get_mut(id) else {
        return;
    };
    s.xdg.serials.push((serial, native));
    if s.xdg.serials.len() > 32 {
        s.xdg.serials.remove(0);
    }
    if let Some(res) = &s.xdg.res {
        res.configure(serial);
    }
}

/// The native scale (× 1000) a window draws for.
pub fn native_scale(state: &State, id: &ObjectId) -> u32 {
    let of = |id: &ObjectId| match state.objects.surfaces.get(id).map(|s| &s.role) {
        Some(Role::Toplevel(t)) => t.configure.map(|c| c.scale_milli),
        _ => None,
    };
    let root = match state.objects.surfaces.get(id).map(|s| &s.role) {
        Some(Role::Popup(p)) => p.root.clone(),
        _ => id.clone(),
    };
    of(&root)
        .unwrap_or_else(|| state.outputs.primary_scale())
        .max(1000)
}

impl Dispatch<XdgToplevel, WlSurface> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &XdgToplevel,
        request: xdg_toplevel::Request,
        surface: &WlSurface,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        let Some(native) = state
            .objects
            .surfaces
            .get(&surface.id())
            .and_then(|s| s.native())
        else {
            return;
        };
        let req = match request {
            xdg_toplevel::Request::SetTitle { title } => {
                let r = state.native.set_title(native, &title);
                state.native_result(r);
                if let Some(Role::Toplevel(t)) = state
                    .objects
                    .surfaces
                    .get_mut(&surface.id())
                    .map(|s| &mut s.role)
                {
                    t.title = crate::native::short_title(&title);
                }
                return;
            }
            xdg_toplevel::Request::SetMinSize { width, height } => {
                size_limits(state, &surface.id(), Some((width, height)), None)
            }
            xdg_toplevel::Request::SetMaxSize { width, height } => {
                size_limits(state, &surface.id(), None, Some((width, height)))
            }
            xdg_toplevel::Request::SetMaximized => Some(Request::SetState {
                surface: native,
                state: WindowState::Maximized,
            }),
            xdg_toplevel::Request::SetFullscreen { .. } => Some(Request::SetState {
                surface: native,
                state: WindowState::Fullscreen,
            }),
            xdg_toplevel::Request::UnsetMaximized | xdg_toplevel::Request::UnsetFullscreen => {
                Some(Request::SetState {
                    surface: native,
                    state: WindowState::Normal,
                })
            }
            xdg_toplevel::Request::SetMinimized => Some(Request::SetState {
                surface: native,
                state: WindowState::Minimized,
            }),
            xdg_toplevel::Request::Move { serial, .. } => Some(Request::Move {
                surface: native,
                serial: state.serials.native(serial),
            }),
            xdg_toplevel::Request::Resize { serial, edges, .. } => {
                let bits = match edges {
                    WEnum::Value(e) => e as u32,
                    WEnum::Unknown(u) => u,
                };
                Some(Request::Resize {
                    surface: native,
                    serial: state.serials.native(serial),
                    edges: ResizeEdges((bits & 15) as u8),
                })
            }
            _ => None,
        };
        if let Some(req) = req {
            let r = state.native.client.send(&req);
            state.native_result(r);
        }
    }

    fn destroyed(state: &mut State, _: ClientId, _: &XdgToplevel, surface: &WlSurface) {
        let Some(s) = state.objects.surfaces.get_mut(&surface.id()) else {
            return;
        };
        if let Role::Toplevel(t) = std::mem::take(&mut s.role) {
            state.objects.by_native.remove(&t.native);
            let r = state.native.destroy(t.native);
            state.native_result(r);
        }
        state.seat.forget(&surface.id());
    }
}

/// Min/max size (0 = no limit); the compositor gets both at once.
fn size_limits(
    state: &mut State,
    id: &ObjectId,
    min: Option<(i32, i32)>,
    max: Option<(i32, i32)>,
) -> Option<Request> {
    let s = state.objects.surfaces.get_mut(id)?;
    let native = s.native()?;
    let clamp = |(w, h): (i32, i32)| (w.max(0) as u32, h.max(0) as u32);
    let limits = &mut s.xdg;
    if let Some(m) = min {
        limits.min_size = clamp(m);
    }
    if let Some(m) = max {
        limits.max_size = clamp(m);
    }
    Some(Request::SetSizeLimits {
        surface: native,
        min: limits.min_size,
        max: limits.max_size,
    })
}
