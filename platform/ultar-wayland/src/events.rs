//! Native events from the compositor, handed to the right part of the bridge.

use crate::state::{Role, State};
use ultar_surface::proto::Event;

/// Handles one event.
pub fn handle(state: &mut State, event: Event) {
    match event {
        Event::Configure(c) => {
            if let Some(id) = state.objects.by_native.get(&c.surface).cloned() {
                crate::xdg::native_configure(state, &id, c);
            }
        }
        Event::BufferReleased { buffer } => {
            let r = state.native.released(buffer);
            state.native_result(r);
        }
        Event::FrameDone { surface, time_ms } => {
            if let Some(id) = state.objects.by_native.get(&surface).cloned() {
                crate::compositor::done_tree(state, &id, time_ms);
            }
        }
        Event::CloseRequested { surface } => close(state, surface),
        Event::Ping { serial } => crate::xdg::ping(state, serial),
        Event::Output(info) => crate::output::update(state, info),
        Event::OutputRemoved { output } => crate::output::remove(state, output),
        Event::Error { .. } => state.native_failed = true,
        other => input(state, other),
    }
}

fn input(state: &mut State, event: Event) {
    use crate::input as i;
    match event {
        Event::PointerEnter { surface, x, y } => i::pointer_enter(state, surface, x, y),
        Event::PointerLeave { surface } => i::pointer_leave(state, surface),
        Event::PointerMotion { surface, x, y } => i::motion(state, surface, x, y),
        Event::PointerButton {
            surface,
            button,
            pressed,
            serial,
        } => i::button(state, surface, button, pressed, serial),
        Event::PointerAxis { surface, dx, dy } => i::axis(state, surface, dx, dy),
        Event::KeyboardFocus { surface, focused } => i::keyboard_focus(state, surface, focused),
        Event::Key {
            code,
            state: st,
            modifiers,
            character,
            serial,
            ..
        } => i::key(state, code, st, modifiers, character, serial),
        Event::TouchDown {
            surface,
            id,
            x,
            y,
            serial,
        } => i::touch_down(state, surface, id, x, y, serial),
        Event::TouchMove { surface, id, x, y } => i::touch_move(state, surface, id, x, y),
        Event::TouchUp { surface, id } => i::touch_up(state, surface, Some(id)),
        Event::TouchCancel { surface } => i::touch_up(state, surface, None),
        Event::SelectionOffer { mimes } => crate::data::native_offer(state, mimes),
        Event::SendRequest { transfer, .. } => crate::data::send_request(state, transfer),
        Event::Data {
            transfer,
            data,
            last,
        } => crate::data::data(state, transfer, data, last),
        Event::TransferCancelled { transfer } => crate::data::cancelled(state, transfer),
        _ => {}
    }
}

fn close(state: &mut State, native: ultar_surface::proto::SurfaceId) {
    let Some(id) = state.objects.by_native.get(&native).cloned() else {
        return;
    };
    match state.objects.surfaces.get(&id).map(|s| &s.role) {
        Some(Role::Toplevel(t)) => t.res.close(),
        Some(Role::Popup(p)) => p.res.popup_done(),
        _ => {}
    }
}
