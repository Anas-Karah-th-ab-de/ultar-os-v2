//! The globals the bridge offers to the app.

use crate::state::State;
use wayland_protocols::xdg::activation::v1::server::xdg_activation_v1::XdgActivationV1;
use wayland_protocols::xdg::decoration::zv1::server::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1;
use wayland_protocols::xdg::shell::server::xdg_wm_base::XdgWmBase;
use wayland_server::protocol::{
    wl_compositor::WlCompositor, wl_data_device_manager::WlDataDeviceManager, wl_seat::WlSeat,
    wl_shm::WlShm, wl_subcompositor::WlSubcompositor,
};
use wayland_server::DisplayHandle;

/// Creates every global (outputs come from the compositor's events).
pub fn create(dh: &DisplayHandle) {
    dh.create_global::<State, WlCompositor, ()>(crate::compositor::VERSION, ());
    dh.create_global::<State, WlSubcompositor, ()>(1, ());
    dh.create_global::<State, WlShm, ()>(1, ());
    dh.create_global::<State, WlSeat, ()>(crate::seat::VERSION, ());
    dh.create_global::<State, XdgWmBase, ()>(crate::xdg::VERSION, ());
    dh.create_global::<State, ZxdgDecorationManagerV1, ()>(1, ());
    dh.create_global::<State, XdgActivationV1, ()>(1, ());
    dh.create_global::<State, WlDataDeviceManager, ()>(crate::data::VERSION, ());
}
