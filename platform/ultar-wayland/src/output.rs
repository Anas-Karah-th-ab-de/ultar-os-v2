//! `wl_output`: one global per display of the compositor, with its scale
//! rounded up (the bridge scales a window down to fractional scales).

use crate::state::State;
use std::collections::HashMap;
use ultar_surface::proto::{OutputId, OutputInfo};
use wayland_server::backend::{GlobalId, ObjectId};
use wayland_server::protocol::{wl_output, wl_output::WlOutput};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

/// Version of `wl_output` offered (4: name and description).
pub const VERSION: u32 = 4;

/// Displays and their objects.
#[derive(Default)]
pub struct Outputs {
    known: HashMap<u32, (OutputInfo, GlobalId)>,
    bound: Vec<(u32, WlOutput)>,
}

impl Outputs {
    /// Scale (× 1000) of the primary display (1000 if none).
    pub fn primary_scale(&self) -> u32 {
        self.primary().map_or(1000, |o| o.scale_milli)
    }

    fn primary(&self) -> Option<&OutputInfo> {
        let mut all: Vec<&OutputInfo> = self.known.values().map(|(i, _)| i).collect();
        all.sort_by_key(|i| (!i.primary, i.id.0));
        all.first().copied()
    }
}

/// Integer Wayland scale of a native scale.
pub fn int_scale(scale_milli: u32) -> i32 {
    scale_milli.div_ceil(1000).clamp(1, 4) as i32
}

/// A display appeared or changed.
pub fn update(state: &mut State, info: OutputInfo) {
    let id = info.id.0;
    if let Some((old, _)) = state.outputs.known.get_mut(&id) {
        *old = info.clone();
        for (_, o) in state
            .outputs
            .bound
            .iter()
            .filter(|(i, o)| *i == id && o.is_alive())
        {
            describe(o, &info);
        }
        return;
    }
    let global = state.dh.create_global::<State, WlOutput, u32>(VERSION, id);
    state.outputs.known.insert(id, (info, global));
}

/// A display went away.
pub fn remove(state: &mut State, id: OutputId) {
    if let Some((_, global)) = state.outputs.known.remove(&id.0) {
        state.dh.remove_global::<State>(global);
    }
    state.outputs.bound.retain(|(i, _)| *i != id.0);
}

fn describe(o: &WlOutput, info: &OutputInfo) {
    let scale = int_scale(info.scale_milli);
    let logical =
        |px: u32| (f64::from(px) * 1000.0 / f64::from(info.scale_milli.max(1000))).round() as i32;
    o.geometry(
        info.x,
        info.y,
        0,
        0,
        wl_output::Subpixel::Unknown,
        "Ultar".to_owned(),
        info.name.clone(),
        wl_output::Transform::Normal,
    );
    o.mode(
        wl_output::Mode::Current | wl_output::Mode::Preferred,
        info.width as i32,
        info.height as i32,
        info.refresh_millihertz as i32,
    );
    if o.version() >= 2 {
        o.scale(scale);
    }
    if o.version() >= 4 {
        o.name(info.name.clone());
        o.description(format!(
            "{} × {}",
            logical(info.width),
            logical(info.height)
        ));
    }
    if o.version() >= 2 {
        o.done();
    }
}

impl GlobalDispatch<WlOutput, u32> for State {
    fn bind(
        state: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlOutput>,
        id: &u32,
        data_init: &mut DataInit<'_, State>,
    ) {
        let o = data_init.init(resource, ());
        if let Some((info, _)) = state.outputs.known.get(id) {
            describe(&o, info);
        }
        state.outputs.bound.retain(|(_, o)| o.is_alive());
        state.outputs.bound.push((*id, o));
    }
}

impl Dispatch<WlOutput, ()> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &WlOutput,
        _: wl_output::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
    }
}

/// Tells a window it is on the primary display (scale choice of the app).
pub fn enter_primary(state: &mut State, id: &ObjectId) {
    let Some(primary) = state.outputs.primary().map(|i| (i.id.0, i.scale_milli)) else {
        return;
    };
    let Some(s) = state.objects.surfaces.get_mut(id) else {
        return;
    };
    if std::mem::replace(&mut s.xdg.entered, true) {
        return;
    }
    let wl = s.wl.clone();
    let owner = wl.client().map(|c| c.id());
    for (_, o) in state
        .outputs
        .bound
        .iter()
        .filter(|(i, o)| *i == primary.0 && o.client().map(|c| c.id()) == owner)
    {
        wl.enter(o);
    }
    if wl.version() >= 6 {
        wl.preferred_buffer_scale(int_scale(primary.1));
    }
}
