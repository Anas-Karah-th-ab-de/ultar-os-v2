//! `ultar-wayland`: the Wayland front end for apps from other systems.
//!
//! One instance runs inside each app cell with the app's own uid: it offers
//! a Wayland socket to the app and shows the app's windows through the
//! compositor's own protocol (`ultar-surface`), so the compositor sees the
//! app like any other client. Pure Rust (no libwayland), software buffers
//! (`wl_shm`) only. See `docs/integration/ultar-wayland.md`.

pub mod args;
pub mod compositor;
pub mod data;
pub mod events;
pub mod extras;
pub mod globals;
pub mod input;
pub mod keymap;
pub mod limits;
pub mod msg;
pub mod native;
pub mod output;
pub mod pixels;
pub mod popup;
pub mod run;
pub mod seat;
pub mod shm;
pub mod state;
pub mod subsurface;
pub mod xdg;
