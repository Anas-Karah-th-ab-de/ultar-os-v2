//! `wl_subcompositor`, `wl_subsurface`: parts of a window that the bridge
//! draws into the window's picture (positions only; stacking follows the
//! order of creation, synchronised and desynchronised behave alike).

use crate::limits::MAX_CHILDREN;
use crate::state::{Role, State};
use wayland_server::backend::ClientId;
use wayland_server::protocol::{
    wl_subcompositor, wl_subcompositor::WlSubcompositor, wl_subsurface,
    wl_subsurface::WlSubsurface, wl_surface::WlSurface,
};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

impl GlobalDispatch<WlSubcompositor, ()> for State {
    fn bind(
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlSubcompositor>,
        _: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WlSubcompositor, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        sc: &WlSubcompositor,
        request: wl_subcompositor::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let wl_subcompositor::Request::GetSubsurface {
            id,
            surface,
            parent,
        } = request
        else {
            return;
        };
        let (sid, pid) = (surface.id(), parent.id());
        let free = state
            .objects
            .surfaces
            .get(&sid)
            .is_some_and(|s| matches!(s.role, Role::None));
        let room = state
            .objects
            .surfaces
            .get(&pid)
            .is_some_and(|p| p.children.len() < MAX_CHILDREN);
        if !free || sid == pid || !room {
            sc.post_error(
                wl_subcompositor::Error::BadSurface,
                "surface cannot be a subsurface of that parent",
            );
            return;
        }
        data_init.init(id, surface.clone());
        if let Some(s) = state.objects.surfaces.get_mut(&sid) {
            s.role = Role::Sub {
                parent: pid.clone(),
                pos: (0, 0),
            };
        }
        if let Some(p) = state.objects.surfaces.get_mut(&pid) {
            p.children.push(sid);
        }
    }
}

impl Dispatch<WlSubsurface, WlSurface> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &WlSubsurface,
        request: wl_subsurface::Request,
        surface: &WlSurface,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        if let wl_subsurface::Request::SetPosition { x, y } = request {
            if let Some(Role::Sub { pos, .. }) = state
                .objects
                .surfaces
                .get_mut(&surface.id())
                .map(|s| &mut s.role)
            {
                *pos = (x.clamp(-8192, 8192), y.clamp(-8192, 8192));
            }
        }
    }

    fn destroyed(state: &mut State, _: ClientId, _: &WlSubsurface, surface: &WlSurface) {
        let id = surface.id();
        let parent = match state.objects.surfaces.get(&id).map(|s| &s.role) {
            Some(Role::Sub { parent, .. }) => parent.clone(),
            _ => return,
        };
        if let Some(p) = state.objects.surfaces.get_mut(&parent) {
            p.children.retain(|c| *c != id);
        }
        if let Some(s) = state.objects.surfaces.get_mut(&id) {
            s.role = Role::None;
        }
    }
}
