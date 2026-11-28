//! `wl_seat`, `wl_pointer`, `wl_keyboard`, `wl_touch`: the input objects of
//! the app. Which surface has the pointer and the keyboard follows the
//! compositor's events (`input.rs`).

use crate::limits::MAX_INPUT_OBJECTS;
use crate::state::{Role, State};
use std::os::fd::{AsFd, OwnedFd};
use wayland_server::backend::ObjectId;
use wayland_server::protocol::{
    wl_keyboard, wl_keyboard::WlKeyboard, wl_pointer, wl_pointer::WlPointer, wl_seat,
    wl_seat::WlSeat, wl_touch::WlTouch,
};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

/// Version of `wl_seat` offered (7: keymap must be mapped privately).
pub const VERSION: u32 = 7;

/// The keymap in a sealed memory file.
pub struct Keymap {
    fd: OwnedFd,
    size: u32,
}

impl Keymap {
    /// Writes the keymap text (with its terminating zero) into a sealed memory file.
    pub fn new(text: &str) -> std::io::Result<Keymap> {
        use rustix::fs::{fcntl_add_seals, memfd_create, MemfdFlags, SealFlags};
        let fd = memfd_create(
            "ultar-keymap",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )?;
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        // pwrite keeps the file position at 0 for clients that read() instead of mmap().
        let mut done = 0;
        while done < bytes.len() {
            done += rustix::io::pwrite(&fd, &bytes[done..], done as u64)?;
        }
        fcntl_add_seals(
            &fd,
            SealFlags::SHRINK | SealFlags::GROW | SealFlags::WRITE | SealFlags::SEAL,
        )?;
        Ok(Keymap {
            fd,
            size: bytes.len() as u32,
        })
    }
}

/// Input state.
pub struct Seat {
    /// The keymap.
    pub keymap: Keymap,
    /// Pointer objects of all connections.
    pub pointers: Vec<WlPointer>,
    /// Keyboard objects.
    pub keyboards: Vec<WlKeyboard>,
    /// Touch objects.
    pub touches: Vec<WlTouch>,
    /// Surface under the pointer.
    pub pointer_focus: Option<ObjectId>,
    /// Surface with the keyboard.
    pub key_focus: Option<ObjectId>,
    /// The window the compositor gave the keyboard to (popups may borrow it).
    pub window_focus: Option<ObjectId>,
    /// The XKB group in effect.
    pub group: u32,
    /// The last modifier state sent (depressed, locked, group).
    pub mods: (u32, u32, u32),
}

impl Seat {
    /// Forgets dead input objects; whether a kind is at its limit.
    pub fn full(&mut self) -> bool {
        self.pointers.retain(Resource::is_alive);
        self.keyboards.retain(Resource::is_alive);
        self.touches.retain(Resource::is_alive);
        [
            self.pointers.len(),
            self.keyboards.len(),
            self.touches.len(),
        ]
        .iter()
        .any(|n| *n >= MAX_INPUT_OBJECTS)
    }

    /// A seat with the keymap.
    pub fn new(keymap: Keymap) -> Seat {
        Seat {
            keymap,
            pointers: Vec::new(),
            keyboards: Vec::new(),
            touches: Vec::new(),
            pointer_focus: None,
            key_focus: None,
            window_focus: None,
            group: 0,
            mods: (0, 0, 0),
        }
    }

    /// A surface no longer has the pointer.
    pub fn forget_pointer(&mut self, id: &ObjectId) {
        if self.pointer_focus.as_ref() == Some(id) {
            self.pointer_focus = None;
        }
    }

    /// A surface went away: it holds nothing anymore.
    pub fn forget(&mut self, id: &ObjectId) {
        for f in [
            &mut self.pointer_focus,
            &mut self.key_focus,
            &mut self.window_focus,
        ] {
            if f.as_ref() == Some(id) {
                *f = None;
            }
        }
    }
}

/// The objects of type `T` of the connection that owns `surface`.
pub fn of_client<T: Resource + Clone>(list: &[T], surface: &impl Resource) -> Vec<T> {
    let owner = surface.client().map(|c| c.id());
    list.iter()
        .filter(|r| r.client().map(|c| c.id()) == owner && r.is_alive())
        .cloned()
        .collect()
}

impl GlobalDispatch<WlSeat, ()> for State {
    fn bind(
        _: &mut State,
        _: &DisplayHandle,
        _: &Client,
        resource: New<WlSeat>,
        _: &(),
        data_init: &mut DataInit<'_, State>,
    ) {
        let seat = data_init.init(resource, ());
        seat.capabilities(
            wl_seat::Capability::Pointer
                | wl_seat::Capability::Keyboard
                | wl_seat::Capability::Touch,
        );
        if seat.version() >= 2 {
            seat.name("seat0".to_owned());
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn request(
        state: &mut State,
        client: &Client,
        _: &WlSeat,
        request: wl_seat::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, State>,
    ) {
        let get = matches!(
            request,
            wl_seat::Request::GetPointer { .. }
                | wl_seat::Request::GetKeyboard { .. }
                | wl_seat::Request::GetTouch { .. }
        );
        match request {
            _ if get && state.seat.full() => crate::state::refuse(
                &state.dh,
                client,
                crate::state::NO_MEMORY,
                "too many input objects",
            ),
            wl_seat::Request::GetPointer { id } => {
                let p = data_init.init(id, ());
                state.seat.pointers.push(p);
            }
            wl_seat::Request::GetKeyboard { id } => {
                let k = data_init.init(id, ());
                k.keymap(
                    wl_keyboard::KeymapFormat::XkbV1,
                    state.seat.keymap.fd.as_fd(),
                    state.seat.keymap.size,
                );
                if k.version() >= 4 {
                    // The compositor repeats keys itself.
                    k.repeat_info(0, 0);
                }
                state.seat.keyboards.push(k.clone());
                crate::input::keyboard_bound(state, &k);
            }
            wl_seat::Request::GetTouch { id } => {
                let t = data_init.init(id, ());
                state.seat.touches.push(t);
            }
            _ => {}
        }
    }
}

impl Dispatch<WlPointer, ()> for State {
    fn request(
        state: &mut State,
        _: &Client,
        _: &WlPointer,
        request: wl_pointer::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
        if let wl_pointer::Request::SetCursor {
            surface: Some(surface),
            ..
        } = request
        {
            // The compositor draws its own pointer; the picture is accepted and not shown.
            if let Some(s) = state.objects.surfaces.get_mut(&surface.id()) {
                if matches!(s.role, Role::None) {
                    s.role = Role::Cursor;
                }
            }
        }
    }
}

impl Dispatch<WlKeyboard, ()> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &WlKeyboard,
        _: wl_keyboard::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
    }
}

impl Dispatch<WlTouch, ()> for State {
    fn request(
        _: &mut State,
        _: &Client,
        _: &WlTouch,
        _: wayland_server::protocol::wl_touch::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, State>,
    ) {
    }
}
