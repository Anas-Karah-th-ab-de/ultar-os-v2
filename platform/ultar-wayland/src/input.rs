//! Input from the compositor to the app: pointer, keyboard, touch. Native
//! positions are relative to the window's content (its window geometry);
//! Wayland positions are relative to the surface, so the geometry's origin
//! is added. A popup with a grab borrows its window's keyboard and closes
//! when the window itself is clicked.

use crate::keymap;
use crate::seat::of_client;
use crate::state::{Role, State};
use ultar_surface::proto::{Modifiers, SurfaceId};
use wayland_server::backend::ObjectId;
use wayland_server::protocol::{
    wl_keyboard, wl_keyboard::WlKeyboard, wl_pointer, wl_surface::WlSurface,
};
use wayland_server::Resource;

/// The wl_surface of a native surface and its geometry origin (logical).
fn target(state: &State, native: SurfaceId) -> Option<(ObjectId, WlSurface, (f64, f64))> {
    let id = state.objects.by_native.get(&native)?.clone();
    let s = state.objects.surfaces.get(&id)?;
    let origin = s
        .xdg
        .geometry
        .map_or((0.0, 0.0), |g| (f64::from(g.x), f64::from(g.y)));
    Some((id, s.wl.clone(), origin))
}

fn pos(fixed: i32, origin: f64) -> f64 {
    f64::from(fixed) / 256.0 + origin
}

/// The pointer entered a window.
pub fn pointer_enter(state: &mut State, native: SurfaceId, x: i32, y: i32) {
    let Some((id, wl, o)) = target(state, native) else {
        return;
    };
    pointer_leave_current(state);
    let serial = state.serials.next(0);
    for p in of_client(&state.seat.pointers, &wl) {
        p.enter(serial, &wl, pos(x, o.0), pos(y, o.1));
        frame(&p);
    }
    state.seat.pointer_focus = Some(id);
}

fn pointer_leave_current(state: &mut State) {
    let Some(old) = state.seat.pointer_focus.take() else {
        return;
    };
    let Some(wl) = state.objects.surfaces.get(&old).map(|s| s.wl.clone()) else {
        return;
    };
    let serial = state.serials.next(0);
    for p in of_client(&state.seat.pointers, &wl) {
        p.leave(serial, &wl);
        frame(&p);
    }
}

/// The pointer left a window.
pub fn pointer_leave(state: &mut State, native: SurfaceId) {
    if target(state, native).map(|t| t.0) == state.seat.pointer_focus {
        pointer_leave_current(state);
    }
}

/// The pointer moved.
pub fn motion(state: &mut State, native: SurfaceId, x: i32, y: i32) {
    let Some((id, wl, o)) = target(state, native) else {
        return;
    };
    if state.seat.pointer_focus.as_ref() != Some(&id) {
        pointer_enter(state, native, x, y);
        return;
    }
    let time = state.now_ms();
    for p in of_client(&state.seat.pointers, &wl) {
        p.motion(time, pos(x, o.0), pos(y, o.1));
        frame(&p);
    }
}

/// A button went down or up.
pub fn button(
    state: &mut State,
    native: SurfaceId,
    button: u16,
    pressed: bool,
    native_serial: u32,
) {
    let Some((id, wl, _)) = target(state, native) else {
        return;
    };
    if pressed && dismiss_popups(state, &id) {
        return;
    }
    let serial = if pressed {
        state.serials.user(native_serial)
    } else {
        state.serials.next(native_serial)
    };
    let time = state.now_ms();
    let st = if pressed {
        wl_pointer::ButtonState::Pressed
    } else {
        wl_pointer::ButtonState::Released
    };
    for p in of_client(&state.seat.pointers, &wl) {
        p.button(serial, time, u32::from(button), st);
        frame(&p);
    }
}

/// Scrolling (logical points, fixed point).
pub fn axis(state: &mut State, native: SurfaceId, dx: i32, dy: i32) {
    let Some((_, wl, _)) = target(state, native) else {
        return;
    };
    let time = state.now_ms();
    for p in of_client(&state.seat.pointers, &wl) {
        if p.version() >= 5 {
            p.axis_source(wl_pointer::AxisSource::Wheel);
        }
        if dy != 0 {
            p.axis(
                time,
                wl_pointer::Axis::VerticalScroll,
                f64::from(dy) / 256.0,
            );
        }
        if dx != 0 {
            p.axis(
                time,
                wl_pointer::Axis::HorizontalScroll,
                f64::from(dx) / 256.0,
            );
        }
        frame(&p);
    }
}

fn frame(p: &wayland_server::protocol::wl_pointer::WlPointer) {
    if p.version() >= 5 {
        p.frame();
    }
}

/// A press on `id` closes the grabbing popups that are not `id` or its parents.
fn dismiss_popups(state: &mut State, id: &ObjectId) -> bool {
    let keep = chain(state, id);
    let grabbed: Vec<ObjectId> = state
        .objects
        .surfaces
        .iter()
        .filter(|(sid, s)| matches!(&s.role, Role::Popup(p) if p.grab) && !keep.contains(sid))
        .map(|(sid, _)| sid.clone())
        .collect();
    for g in &grabbed {
        if let Some(Role::Popup(p)) = state.objects.surfaces.get(g).map(|s| &s.role) {
            p.res.popup_done();
        }
    }
    !grabbed.is_empty()
        && !matches!(
            state.objects.surfaces.get(id).map(|s| &s.role),
            Some(Role::Popup(_))
        )
}

/// `id` and the popups/window it hangs on.
fn chain(state: &State, id: &ObjectId) -> Vec<ObjectId> {
    let mut out = vec![id.clone()];
    let mut cur = id.clone();
    for _ in 0..16 {
        match state.objects.surfaces.get(&cur).map(|s| &s.role) {
            Some(Role::Popup(p)) => {
                cur = p.parent.clone();
                out.push(cur.clone());
            }
            _ => break,
        }
    }
    out
}

/// The compositor gave or took the keyboard.
pub fn keyboard_focus(state: &mut State, native: SurfaceId, focused: bool) {
    let Some((id, _, _)) = target(state, native) else {
        return;
    };
    if focused {
        state.seat.window_focus = Some(id.clone());
        let to = topmost_grab(state, &id).unwrap_or(id);
        move_keyboard(state, Some(to));
    } else if state.seat.window_focus.as_ref() == Some(&id) {
        state.seat.window_focus = None;
        move_keyboard(state, None);
    }
}

/// The newest grabbing popup of a window.
fn topmost_grab(state: &State, window: &ObjectId) -> Option<ObjectId> {
    let mut best = None;
    let mut depth = 0;
    for (sid, s) in &state.objects.surfaces {
        if let Role::Popup(p) = &s.role {
            let d = chain(state, sid).len();
            if p.grab && &p.root == window && d > depth {
                best = Some(sid.clone());
                depth = d;
            }
        }
    }
    best
}

/// Gives the keyboard to `to` (leave the old surface first).
fn move_keyboard(state: &mut State, to: Option<ObjectId>) {
    if state.seat.key_focus == to {
        return;
    }
    if let Some(old) = state.seat.key_focus.take() {
        if let Some(wl) = state.objects.surfaces.get(&old).map(|s| s.wl.clone()) {
            let serial = state.serials.next(0);
            for k in of_client(&state.seat.keyboards, &wl) {
                k.leave(serial, &wl);
            }
        }
    }
    let Some(new) = to else {
        return;
    };
    let Some(wl) = state.objects.surfaces.get(&new).map(|s| s.wl.clone()) else {
        return;
    };
    crate::data::focus_changed(state, &wl);
    for k in of_client(&state.seat.keyboards, &wl) {
        enter(state, &k, &wl);
    }
    state.seat.key_focus = Some(new);
}

fn enter(state: &mut State, k: &WlKeyboard, wl: &WlSurface) {
    let serial = state.serials.next(0);
    k.enter(serial, wl, Vec::new());
    let (d, l, g) = state.seat.mods;
    k.modifiers(serial, d, 0, l, g);
}

/// A keyboard object appeared: it gets the focus if its connection has it.
pub fn keyboard_bound(state: &mut State, k: &WlKeyboard) {
    let Some(focus) = state.seat.key_focus.clone() else {
        return;
    };
    let Some(wl) = state.objects.surfaces.get(&focus).map(|s| s.wl.clone()) else {
        return;
    };
    if wl.client().map(|c| c.id()) == k.client().map(|c| c.id()) {
        enter(state, k, &wl);
    }
}

/// A key changed (`state`: 0 up, 1 down, 2 repeat).
pub fn key(
    state: &mut State,
    code: u16,
    key_state: u8,
    m: Modifiers,
    character: u32,
    native_serial: u32,
) {
    let Some(focus) = state.seat.key_focus.clone() else {
        return;
    };
    let Some(wl) = state.objects.surfaces.get(&focus).map(|s| s.wl.clone()) else {
        return;
    };
    let group = if key_state == 0 {
        state.seat.group
    } else {
        keymap::group_for(code, m, character, state.seat.group)
    };
    let (depressed, locked) = keymap::masks(m);
    let serial = if key_state == 0 {
        state.serials.next(native_serial)
    } else {
        state.serials.user(native_serial)
    };
    let keyboards = of_client(&state.seat.keyboards, &wl);
    if (depressed, locked, group) != state.seat.mods {
        state.seat.mods = (depressed, locked, group);
        state.seat.group = group;
        for k in &keyboards {
            k.modifiers(serial, depressed, 0, locked, group);
        }
    }
    let time = state.now_ms();
    let st = if key_state == 0 {
        wl_keyboard::KeyState::Released
    } else {
        wl_keyboard::KeyState::Pressed
    };
    for k in &keyboards {
        k.key(serial, time, u32::from(code), st);
    }
}

/// A popup took the grab: it gets the keyboard if its window has it.
pub fn popup_grabbed(state: &mut State, id: &ObjectId) {
    let root = match state.objects.surfaces.get(id).map(|s| &s.role) {
        Some(Role::Popup(p)) => p.root.clone(),
        _ => return,
    };
    if state.seat.window_focus.as_ref() == Some(&root) {
        move_keyboard(state, Some(id.clone()));
    }
}

/// A popup closed: the keyboard goes back to its parent.
pub fn popup_gone(state: &mut State, id: &ObjectId, parent: &ObjectId) {
    state.seat.forget_pointer(id);
    if state.seat.key_focus.as_ref() == Some(id) {
        state.seat.key_focus = None;
        let back = state.seat.window_focus.as_ref().map(|_| parent.clone());
        move_keyboard(state, back);
    }
}

/// Touch: a finger went down.
pub fn touch_down(
    state: &mut State,
    native: SurfaceId,
    id: u32,
    x: i32,
    y: i32,
    native_serial: u32,
) {
    let Some((_, wl, o)) = target(state, native) else {
        return;
    };
    let serial = state.serials.user(native_serial);
    let time = state.now_ms();
    for t in of_client(&state.seat.touches, &wl) {
        t.down(serial, time, &wl, id as i32, pos(x, o.0), pos(y, o.1));
        t.frame();
    }
}

/// Touch: a finger moved.
pub fn touch_move(state: &mut State, native: SurfaceId, id: u32, x: i32, y: i32) {
    let Some((_, wl, o)) = target(state, native) else {
        return;
    };
    let time = state.now_ms();
    for t in of_client(&state.seat.touches, &wl) {
        t.motion(time, id as i32, pos(x, o.0), pos(y, o.1));
        t.frame();
    }
}

/// Touch: a finger lifted (or all were taken: `id` = None).
pub fn touch_up(state: &mut State, native: SurfaceId, id: Option<u32>) {
    let Some((_, wl, _)) = target(state, native) else {
        return;
    };
    let serial = state.serials.next(0);
    let time = state.now_ms();
    for t in of_client(&state.seat.touches, &wl) {
        match id {
            Some(id) => {
                t.up(serial, time, id as i32);
                t.frame();
            }
            None => t.cancel(),
        }
    }
}
