//! `xdg_positioner` and `xdg_popup`: menus and tooltips. A native popup
//! belongs to a toplevel, so nested menus are placed relative to their
//! toplevel; the compositor keeps every popup inside its window.

use crate::limits::MAX_POSITIONERS;
use crate::state::{Popup, Role, State};
use ultar_surface::proto::{Rect, Role as NativeRole};
use wayland_protocols::xdg::shell::server::{
    xdg_popup, xdg_popup::XdgPopup, xdg_positioner, xdg_positioner::XdgPositioner, xdg_wm_base,
};
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::wl_surface::WlSurface;
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, Resource, WEnum};

/// What a positioner says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Positioner {
    /// Popup size.
    pub size: (i32, i32),
    /// Anchor rectangle in the parent's window geometry.
    pub anchor_rect: Rect,
    /// Anchor edge/corner (xdg enum value).
    pub anchor: u32,
    /// Gravity (xdg enum value).
    pub gravity: u32,
    /// Extra offset.
    pub offset: (i32, i32),
}

impl Positioner {
    /// The popup rectangle relative to the parent's window geometry.
    pub fn place(&self) -> Rect {
        let r = &self.anchor_rect;
        let (w, h) = (self.size.0.max(1), self.size.1.max(1));
        let (left, right) = (r.x, r.x + r.width as i32); // physical: xdg anchors are
        let (top, bottom) = (r.y, r.y + r.height as i32);
        let (cx, cy) = ((left + right) / 2, (top + bottom) / 2); // physical
                                                                 // anchor: 1 top, 2 bottom, 3 left, 4 right, 5 tl, 6 bl, 7 tr, 8 br
        let ax = match self.anchor {
            3 | 5 | 6 => left,  // physical
            4 | 7 | 8 => right, // physical
            _ => cx,
        };
        let ay = match self.anchor {
            1 | 5 | 7 => top,
            2 | 6 | 8 => bottom,
            _ => cy,
        };
        // gravity: the direction the popup grows from the anchor point
        let x = match self.gravity {
            3 | 5 | 6 => ax - w,
            4 | 7 | 8 => ax,
            _ => ax - w / 2,
        };
        let y = match self.gravity {
            1 | 5 | 7 => ay - h,
            2 | 6 | 8 => ay,
            _ => ay - h / 2,
        };
        Rect::new(x + self.offset.0, y + self.offset.1, w as u32, h as u32)
    }
}

impl Dispatch<XdgPositioner, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        pos: &XdgPositioner,
        request: xdg_positioner::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        let Some(p) = state.objects.positioners.get_mut(&pos.id()) else {
            return;
        };
        match request {
            xdg_positioner::Request::SetSize { width, height } if width > 0 && height > 0 => {
                p.size = (width, height)
            }
            xdg_positioner::Request::SetAnchorRect {
                x,
                y,
                width,
                height,
            } if width >= 0 && height >= 0 => {
                p.anchor_rect = Rect::new(x, y, width as u32, height as u32)
            }
            xdg_positioner::Request::SetAnchor { anchor } => {
                p.anchor = match anchor {
                    WEnum::Value(a) => a as u32,
                    WEnum::Unknown(u) => u,
                }
            }
            xdg_positioner::Request::SetGravity { gravity } => {
                p.gravity = match gravity {
                    WEnum::Value(g) => g as u32,
                    WEnum::Unknown(u) => u,
                }
            }
            xdg_positioner::Request::SetOffset { x, y } => p.offset = (x, y),
            xdg_positioner::Request::SetSize { .. }
            | xdg_positioner::Request::SetAnchorRect { .. } => {
                pos.post_error(xdg_positioner::Error::InvalidInput, "negative or zero size")
            }
            _ => {}
        }
    }

    fn destroyed(state: &mut State, client: ClientId, resource: &XdgPositioner, _: &()) {
        if state.objects.positioners.remove(&resource.id()).is_some() {
            let c = state.counts(&client);
            c.positioners = c.positioners.saturating_sub(1);
        }
    }
}

/// `xdg_wm_base.create_positioner`.
pub fn create_positioner(
    state: &mut State,
    client: &Client,
    id: wayland_server::New<XdgPositioner>,
    data_init: &mut DataInit<'_, State>,
) {
    if state.counts(&client.id()).positioners >= MAX_POSITIONERS {
        crate::state::refuse(
            &state.dh,
            client,
            crate::state::NO_MEMORY,
            "too many positioners",
        );
        return;
    }
    let p = data_init.init(id, ());
    state.counts(&client.id()).positioners += 1;
    state
        .objects
        .positioners
        .insert(p.id(), Positioner::default());
}

/// `xdg_surface.get_popup`: the role and where it goes (shown on its first commit).
pub fn get_popup(
    state: &mut State,
    surface: &WlSurface,
    parent: Option<ObjectId>,
    positioner: &XdgPositioner,
    res: XdgPopup,
) {
    let Some(parent) = parent else {
        res.post_error(
            xdg_wm_base::Error::InvalidPopupParent,
            "a popup needs a parent",
        );
        return;
    };
    let pos = state
        .objects
        .positioners
        .get(&positioner.id())
        .copied()
        .unwrap_or_default();
    if pos.size == (0, 0) {
        res.post_error(
            xdg_wm_base::Error::InvalidPositioner,
            "positioner without size",
        );
        return;
    }
    let Some((root, base)) = root_and_offset(state, &parent) else {
        res.post_error(
            xdg_wm_base::Error::InvalidPopupParent,
            "the parent is no window",
        );
        return;
    };
    let rect = pos.place();
    let offset = (base.0 + rect.x, base.1 + rect.y);
    if let Some(s) = state.objects.surfaces.get_mut(&surface.id()) {
        s.role = Role::Popup(Box::new(Popup {
            res,
            native: None,
            root,
            parent,
            rect,
            offset,
            grab: false,
        }));
    }
}

/// The toplevel a parent belongs to and the parent's offset in it.
fn root_and_offset(state: &State, parent: &ObjectId) -> Option<(ObjectId, (i32, i32))> {
    match &state.objects.surfaces.get(parent)?.role {
        Role::Toplevel(_) => Some((parent.clone(), (0, 0))),
        Role::Popup(p) => Some((p.root.clone(), p.offset)),
        _ => None,
    }
}

/// First commit of a popup: the native popup and the configure.
pub fn map(state: &mut State, id: &ObjectId) {
    let Some(s) = state.objects.surfaces.get(id) else {
        return;
    };
    let Role::Popup(p) = &s.role else {
        return;
    };
    let Some(root_native) = state.objects.surfaces.get(&p.root).and_then(|r| r.native()) else {
        return;
    };
    let (rect, offset, res) = (p.rect, p.offset, p.res.clone());
    let role = NativeRole::Popup {
        parent: root_native,
        x: offset.0,
        y: offset.1,
    };
    if !state.native.room(role) {
        // Too many menus open: this one closes at once (allowed by xdg-shell).
        res.popup_done();
        return;
    }
    let r = state.native.create(role, "");
    let Some(native) = state.native_result(r) else {
        return;
    };
    state.objects.by_native.insert(native, id.clone());
    if let Some(Role::Popup(p)) = state.objects.surfaces.get_mut(id).map(|s| &mut s.role) {
        p.native = Some(native);
    }
    res.configure(rect.x, rect.y, rect.width as i32, rect.height as i32);
    crate::xdg::send_surface_configure(state, id, 0);
}

impl Dispatch<XdgPopup, WlSurface> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &XdgPopup,
        request: xdg_popup::Request,
        surface: &WlSurface,
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        if let xdg_popup::Request::Grab { .. } = request {
            if let Some(Role::Popup(p)) = state
                .objects
                .surfaces
                .get_mut(&surface.id())
                .map(|s| &mut s.role)
            {
                p.grab = true;
            }
            crate::input::popup_grabbed(state, &surface.id());
        }
    }

    fn destroyed(state: &mut State, _: ClientId, _: &XdgPopup, surface: &WlSurface) {
        unmap(state, &surface.id());
    }
}

/// Ends a popup's native surface and grab.
pub fn unmap(state: &mut State, id: &ObjectId) {
    let Some(s) = state.objects.surfaces.get_mut(id) else {
        return;
    };
    let Role::Popup(p) = std::mem::take(&mut s.role) else {
        return;
    };
    if let Some(native) = p.native {
        state.objects.by_native.remove(&native);
        let r = state.native.destroy(native);
        state.native_result(r);
    }
    crate::input::popup_gone(state, id, &p.parent);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_below_a_button() {
        let p = Positioner {
            size: (100, 50),
            anchor_rect: Rect::new(10, 10, 40, 20),
            anchor: 6,
            gravity: 8,
            offset: (0, 2),
        };
        assert_eq!(p.place(), Rect::new(10, 32, 100, 50));
        let centred = Positioner {
            size: (10, 10),
            anchor_rect: Rect::new(0, 0, 20, 20),
            ..Positioner::default()
        };
        assert_eq!(centred.place(), Rect::new(5, 5, 10, 10));
        let above_left = Positioner {
            size: (10, 10),
            anchor_rect: Rect::new(50, 50, 0, 0),
            anchor: 5,
            gravity: 5,
            offset: (0, 0),
        };
        assert_eq!(above_left.place(), Rect::new(40, 40, 10, 10));
    }
}
