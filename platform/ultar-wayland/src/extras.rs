//! Small protocols: `zxdg_decoration_manager_v1` (who draws the title bar)
//! and `xdg_activation_v1` (tokens; the first window already gets the
//! launcher's token from `Native::create`).
//!
//! Title bars: the compositor draws them unless the app draws its own and
//! the app holds the right `ui.client-header` – then the compositor leaves
//! the header to the app. Apps that never ask (GTK 4) draw their own.

use crate::state::{Role, State};
use std::sync::atomic::{AtomicU32, Ordering};
use ultar_surface::proto::{Request, Rights};
use wayland_protocols::xdg::activation::v1::server::{
    xdg_activation_token_v1, xdg_activation_token_v1::XdgActivationTokenV1, xdg_activation_v1,
    xdg_activation_v1::XdgActivationV1,
};
use wayland_protocols::xdg::decoration::zv1::server::{
    zxdg_decoration_manager_v1,
    zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
    zxdg_toplevel_decoration_v1,
    zxdg_toplevel_decoration_v1::{Mode, ZxdgToplevelDecorationV1},
};
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::wl_surface::WlSurface;
use wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
};

impl GlobalDispatch<ZxdgDecorationManagerV1, ()> for State {
    fn bind(
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZxdgDecorationManagerV1>,
        _: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZxdgDecorationManagerV1, ()> for State {
    fn request(
        state: &mut State,
        client: &Client,
        _: &ZxdgDecorationManagerV1,
        request: zxdg_decoration_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let zxdg_decoration_manager_v1::Request::GetToplevelDecoration { id, toplevel } = request
        else {
            return;
        };
        let Some(surface) = toplevel.data::<WlSurface>().cloned() else {
            crate::state::refuse(
                &state.dh,
                client,
                crate::state::IMPLEMENTATION,
                "toplevel without surface",
            );
            return;
        };
        let deco = data_init.init(id, surface.clone());
        if let Some(Role::Toplevel(t)) = state
            .objects
            .surfaces
            .get_mut(&surface.id())
            .map(|s| &mut s.role)
        {
            t.decoration = Some(deco);
        }
    }
}

impl Dispatch<ZxdgToplevelDecorationV1, WlSurface> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &ZxdgToplevelDecorationV1,
        request: zxdg_toplevel_decoration_v1::Request,
        surface: &WlSurface,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        let wants_own = match request {
            zxdg_toplevel_decoration_v1::Request::SetMode { mode } => {
                mode == WEnum::Value(Mode::ClientSide)
            }
            zxdg_toplevel_decoration_v1::Request::UnsetMode => false,
            _ => return,
        };
        apply(state, &surface.id(), wants_own);
        crate::xdg::send_configure(state, &surface.id());
    }

    fn destroyed(
        state: &mut State,
        _: ClientId,
        _: &ZxdgToplevelDecorationV1,
        surface: &WlSurface,
    ) {
        if let Some(Role::Toplevel(t)) = state
            .objects
            .surfaces
            .get_mut(&surface.id())
            .map(|s| &mut s.role)
        {
            t.decoration = None;
        }
    }
}

/// At the initial commit: apps without a decoration object draw their own header.
pub fn decide_decoration(state: &mut State, id: &ObjectId) {
    let has = matches!(state.objects.surfaces.get(id).map(|s| &s.role), Some(Role::Toplevel(t)) if t.decoration.is_some());
    if !has {
        apply(state, id, true);
    }
}

/// Header by the app (if it may) or by the compositor; tells both sides.
fn apply(state: &mut State, id: &ObjectId, wants_own: bool) {
    let own = wants_own && state.rights.contains(Rights::CLIENT_HEADER);
    let Some(Role::Toplevel(t)) = state.objects.surfaces.get_mut(id).map(|s| &mut s.role) else {
        return;
    };
    if let Some(d) = &t.decoration {
        d.configure(if own {
            Mode::ClientSide
        } else {
            Mode::ServerSide
        });
    }
    let changed = t.client_header != own;
    t.client_header = own;
    let native = t.native;
    if changed && state.rights.contains(Rights::CLIENT_HEADER) {
        let r = state.native.client.send(&Request::ClientHeader {
            surface: native,
            on: own,
        });
        state.native_result(r);
    }
}

static TOKENS: AtomicU32 = AtomicU32::new(1);

impl GlobalDispatch<XdgActivationV1, ()> for State {
    fn bind(
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<XdgActivationV1>,
        _: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<XdgActivationV1, ()> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &XdgActivationV1,
        request: xdg_activation_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        if let xdg_activation_v1::Request::GetActivationToken { id } = request {
            data_init.init(id, ());
        }
    }
}

impl Dispatch<XdgActivationTokenV1, ()> for State {
    fn request(
        _: &mut State,
        _: &Client,
        token: &XdgActivationTokenV1,
        request: xdg_activation_token_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        if let xdg_activation_token_v1::Request::Commit = request {
            let n = TOKENS.fetch_add(1, Ordering::Relaxed);
            token.done(format!("ultar-{n}"));
        }
    }
}
