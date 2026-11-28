//! The clipboard between the app and the compositor (text only):
//! `wl_data_device_manager`, `wl_data_source`, `wl_data_device`,
//! `wl_data_offer`. Data flows through pipes that the event loop drains in
//! chunks, never blocking on the app. Drag and drop is not offered.

use crate::limits::{MAX_CLIPBOARD, MAX_INPUT_OBJECTS, MAX_MIME_LEN, MAX_READS, MAX_SOURCE_MIMES};
use crate::seat::of_client;
use crate::state::State;
use std::collections::{HashMap, VecDeque};
use std::os::fd::OwnedFd;
use ultar_surface::proto::{Request, TransferId, CLIP_TEXT_MIME, MAX_CHUNK};
use wayland_server::backend::{ClientId, ObjectId};
use wayland_server::protocol::{
    wl_data_device, wl_data_device::WlDataDevice, wl_data_device_manager,
    wl_data_device_manager::WlDataDeviceManager, wl_data_offer, wl_data_offer::WlDataOffer,
    wl_data_source, wl_data_source::WlDataSource, wl_surface::WlSurface,
};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

/// Version of `wl_data_device_manager` offered.
pub const VERSION: u32 = 3;

/// Text types the bridge understands (the first is the compositor's).
pub const TEXT_MIMES: [&str; 4] = [CLIP_TEXT_MIME, "text/plain", "UTF8_STRING", "STRING"];

/// App → compositor: one transfer.
pub struct Outgoing {
    /// Read end of the pipe the app writes into.
    pub pipe: Option<OwnedFd>,
    /// Read and not yet sent.
    pub buf: Vec<u8>,
    /// The compositor asked for the next chunk.
    pub wanted: bool,
    /// Bytes sent so far.
    pub total: usize,
}

/// Compositor → app: one read.
pub struct Incoming {
    /// The compositor's transfer, once known.
    pub transfer: Option<u32>,
    /// Write end of the app's pipe.
    pub fd: OwnedFd,
    /// Received and not yet written.
    pub buf: Vec<u8>,
    /// The last chunk arrived.
    pub last: bool,
    /// Bytes received.
    pub total: usize,
}

/// Clipboard state.
#[derive(Default)]
pub struct Clipboard {
    /// Data devices of all connections.
    pub devices: Vec<WlDataDevice>,
    /// Types each data source offers.
    pub sources: HashMap<ObjectId, Vec<String>>,
    /// The app's current selection.
    pub selection: Option<WlDataSource>,
    /// Types the compositor's clipboard offers.
    pub native_mimes: Vec<String>,
    /// Transfers to the compositor.
    pub outgoing: HashMap<u32, Outgoing>,
    /// Reads from the compositor, in order.
    pub incoming: VecDeque<Incoming>,
}

fn is_text(m: &str) -> bool {
    TEXT_MIMES.contains(&m)
}

impl GlobalDispatch<WlDataDeviceManager, ()> for State {
    fn bind(
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlDataDeviceManager>,
        _: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<WlDataDeviceManager, ()> for State {
    fn request(
        state: &mut State,
        client: &Client,
        _: &WlDataDeviceManager,
        request: wl_data_device_manager::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        state.clipboard.devices.retain(Resource::is_alive);
        let full = state.clipboard.sources.len() >= MAX_INPUT_OBJECTS
            || state.clipboard.devices.len() >= MAX_INPUT_OBJECTS;
        match request {
            _ if full => crate::state::refuse(
                &state.dh,
                client,
                crate::state::NO_MEMORY,
                "too many data objects",
            ),
            wl_data_device_manager::Request::CreateDataSource { id } => {
                let s = data_init.init(id, ());
                state.clipboard.sources.insert(s.id(), Vec::new());
            }
            wl_data_device_manager::Request::GetDataDevice { id, .. } => {
                let d = data_init.init(id, ());
                state.clipboard.devices.push(d);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlDataSource, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        source: &WlDataSource,
        request: wl_data_source::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        if let wl_data_source::Request::Offer { mime_type } = request {
            let Some(list) = state.clipboard.sources.get_mut(&source.id()) else {
                return;
            };
            if list.len() < MAX_SOURCE_MIMES && mime_type.len() <= MAX_MIME_LEN {
                list.push(mime_type);
            }
        }
    }

    fn destroyed(state: &mut State, _: ClientId, resource: &WlDataSource, _: &()) {
        state.clipboard.sources.remove(&resource.id());
        if state.clipboard.selection.as_ref().map(Resource::id) == Some(resource.id()) {
            state.clipboard.selection = None;
        }
    }
}

impl Dispatch<WlDataDevice, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &WlDataDevice,
        request: wl_data_device::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        match request {
            wl_data_device::Request::SetSelection { source, .. } => set_selection(state, source),
            // Drag and drop is not offered: the drag ends at once.
            wl_data_device::Request::StartDrag {
                source: Some(s), ..
            } => s.cancelled(),
            _ => {}
        }
    }
}

fn set_selection(state: &mut State, source: Option<WlDataSource>) {
    let Some(source) = source else {
        return;
    };
    let offers_text = state
        .clipboard
        .sources
        .get(&source.id())
        .is_some_and(|m| m.iter().any(|m| is_text(m)));
    if !offers_text {
        source.cancelled();
        return;
    }
    // The compositor wants the focus and a fresh user action (else it would
    // end the connection): without them the copy is refused here. Apps
    // name the serial of any recent event, so the newest press is used.
    // Apps name the serial of any recent event (wl-copy: keyboard enter),
    // so the client's serial is not checked; the newest press is used.
    let (Some(native), true) = (
        state.serials.fresh_user(),
        state.seat.window_focus.is_some(),
    ) else {
        source.cancelled();
        return;
    };
    if let Some(old) = state.clipboard.selection.replace(source) {
        if old.is_alive() {
            old.cancelled();
        }
    }
    let r = state.native.client.send(&Request::SetSelection {
        serial: native,
        mimes: vec![CLIP_TEXT_MIME.to_owned()],
    });
    state.native_result(r);
}

impl Dispatch<WlDataOffer, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &WlDataOffer,
        request: wl_data_offer::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        if let wl_data_offer::Request::Receive { mime_type, fd } = request {
            receive(state, &mime_type, fd);
        }
    }
}

fn receive(state: &mut State, mime: &str, fd: OwnedFd) {
    let has_text = state
        .clipboard
        .native_mimes
        .iter()
        .any(|m| m == CLIP_TEXT_MIME);
    if !is_text(mime) || !has_text || state.clipboard.incoming.len() >= MAX_READS {
        return;
    }
    // Same rule as for copying: focus and a fresh press (Ctrl+V, a click).
    let (Some(serial), true) = (
        state.serials.fresh_user(),
        state.seat.window_focus.is_some(),
    ) else {
        return;
    };
    let _ = rustix::fs::fcntl_setfl(&fd, rustix::fs::OFlags::NONBLOCK);
    let r = state.native.client.send(&Request::ReadSelection {
        serial,
        mime: CLIP_TEXT_MIME.to_owned(),
    });
    if state.native_result(r).is_some() {
        state.clipboard.incoming.push_back(Incoming {
            transfer: None,
            fd,
            buf: Vec::new(),
            last: false,
            total: 0,
        });
    }
}

/// The compositor's clipboard changed.
pub fn native_offer(state: &mut State, mimes: Vec<String>) {
    state.clipboard.native_mimes = mimes;
    if let Some(focus) = state
        .seat
        .key_focus
        .clone()
        .and_then(|f| state.objects.surfaces.get(&f).map(|s| s.wl.clone()))
    {
        focus_changed(state, &focus);
    }
}

/// The keyboard went to `wl`: its connection learns the clipboard.
pub fn focus_changed(state: &mut State, wl: &WlSurface) {
    let text = state
        .clipboard
        .native_mimes
        .iter()
        .any(|m| m == CLIP_TEXT_MIME);
    for device in of_client(&state.clipboard.devices, wl) {
        let Some(client) = device.client() else {
            continue;
        };
        if !text {
            device.selection(None);
            continue;
        }
        let Ok(offer) =
            client.create_resource::<WlDataOffer, (), State>(&state.dh, device.version(), ())
        else {
            continue;
        };
        device.data_offer(&offer);
        for m in TEXT_MIMES {
            offer.offer(m.to_owned());
        }
        device.selection(Some(&offer));
    }
}

/// The compositor wants (the next chunk of) the app's selection.
pub fn send_request(state: &mut State, transfer: TransferId) {
    if let Some(o) = state.clipboard.outgoing.get_mut(&transfer.0) {
        o.wanted = true;
        return;
    }
    let Some(source) = state.clipboard.selection.clone().filter(Resource::is_alive) else {
        let r = state.native.client.send(&Request::SendData {
            transfer,
            data: Vec::new(),
            last: true,
        });
        state.native_result(r);
        return;
    };
    let mimes = state
        .clipboard
        .sources
        .get(&source.id())
        .cloned()
        .unwrap_or_default();
    let Some(mime) = TEXT_MIMES.iter().find(|t| mimes.iter().any(|m| m == *t)) else {
        return;
    };
    let Ok((read, write)) = rustix::pipe::pipe_with(
        rustix::pipe::PipeFlags::CLOEXEC | rustix::pipe::PipeFlags::NONBLOCK,
    ) else {
        return;
    };
    source.send((*mime).to_owned(), std::os::fd::AsFd::as_fd(&write));
    drop(write);
    state.clipboard.outgoing.insert(
        transfer.0,
        Outgoing {
            pipe: Some(read),
            buf: Vec::new(),
            wanted: true,
            total: 0,
        },
    );
}

/// A chunk of the compositor's clipboard arrived for the oldest read.
pub fn data(state: &mut State, transfer: TransferId, bytes: Vec<u8>, last: bool) {
    let cb = &mut state.clipboard;
    let index = cb
        .incoming
        .iter()
        .position(|i| i.transfer == Some(transfer.0))
        .or_else(|| cb.incoming.iter().position(|i| i.transfer.is_none()));
    let Some(i) = index.and_then(|n| cb.incoming.get_mut(n)) else {
        return;
    };
    i.transfer = Some(transfer.0);
    i.total += bytes.len();
    if i.total > MAX_CLIPBOARD {
        i.last = true;
        return;
    }
    i.buf.extend_from_slice(&bytes);
    i.last |= last;
}

/// A transfer ended without data.
pub fn cancelled(state: &mut State, transfer: TransferId) {
    state.clipboard.outgoing.remove(&transfer.0);
    state
        .clipboard
        .incoming
        .retain(|i| i.transfer != Some(transfer.0));
}

/// Moves data along (called by the event loop when pipes are ready).
pub fn pump(state: &mut State) {
    let mut sends = Vec::new();
    for (t, o) in state.clipboard.outgoing.iter_mut() {
        fill(o);
        let eof = o.pipe.is_none();
        if o.wanted && (o.buf.len() >= MAX_CHUNK || eof) {
            let n = o.buf.len().min(MAX_CHUNK);
            let chunk: Vec<u8> = o.buf.drain(..n).collect();
            o.total += n;
            let last = (eof && o.buf.is_empty()) || o.total >= MAX_CLIPBOARD;
            o.wanted = false;
            sends.push((*t, chunk, last));
        }
    }
    for (t, data, last) in sends {
        if last {
            state.clipboard.outgoing.remove(&t);
        }
        let r = state.native.client.send(&Request::SendData {
            transfer: TransferId(t),
            data,
            last,
        });
        state.native_result(r);
    }
    drain_incoming(state);
}

fn fill(o: &mut Outgoing) {
    let Some(pipe) = &o.pipe else {
        return;
    };
    let mut chunk = [0u8; 65536];
    while o.buf.len() < MAX_CHUNK {
        match rustix::io::read(pipe, &mut chunk) {
            Ok(0) => {
                o.pipe = None;
                return;
            }
            Ok(n) => o.buf.extend_from_slice(&chunk[..n]),
            Err(rustix::io::Errno::AGAIN) | Err(rustix::io::Errno::INTR) => return,
            Err(_) => {
                o.pipe = None;
                return;
            }
        }
    }
}

fn drain_incoming(state: &mut State) {
    for i in state.clipboard.incoming.iter_mut() {
        while !i.buf.is_empty() {
            match rustix::io::write(&i.fd, &i.buf) {
                Ok(n) => {
                    i.buf.drain(..n);
                }
                Err(rustix::io::Errno::AGAIN) | Err(rustix::io::Errno::INTR) => break,
                Err(_) => {
                    i.buf.clear();
                    i.last = true;
                }
            }
        }
    }
    state
        .clipboard
        .incoming
        .retain(|i| !(i.last && i.buf.is_empty()));
}

/// Descriptors the loop should wait on: (fd, want-write).
pub fn poll_fds(state: &State) -> Vec<(std::os::fd::BorrowedFd<'_>, bool)> {
    let mut out = Vec::new();
    for o in state.clipboard.outgoing.values() {
        if let Some(p) = &o.pipe {
            if o.buf.len() < MAX_CHUNK {
                out.push((std::os::fd::AsFd::as_fd(p), false));
            }
        }
    }
    for i in &state.clipboard.incoming {
        if !i.buf.is_empty() {
            out.push((std::os::fd::AsFd::as_fd(&i.fd), true));
        }
    }
    out
}
