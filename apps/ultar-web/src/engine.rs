//! Engine-Schnittstelle: Der Browser-Kern spricht nur über `WebEngine` mit einer Engine.
//! Heute: WPE WebKit (`WpeTab`). Eine Servo-Engine kann dieselbe Schnittstelle umsetzen.

use crate::ffi::*;
use std::ffi::c_int;

pub trait WebEngine {
    fn load(&self, uri: &str);
    fn back(&self);
    fn forward(&self);
    fn reload(&self);
    fn stop(&self);
    fn can_back(&self) -> bool;
    fn can_forward(&self) -> bool;
    fn title(&self) -> String;
    fn uri(&self) -> String;
    fn progress(&self) -> f64;
    fn loading(&self) -> bool;
    fn secure(&self) -> bool;
    fn playing_audio(&self) -> bool;
    /// Berührung in CSS-Pixeln des Inhalts (kind: 0 down, 1 move, 2 up)
    fn touch(&self, kind: u8, x: f64, y: f64, time: u32);
    /// Taste (X11-Keysym)
    fn key(&self, keyval: u32, keycode: u32, time: u32);
    /// Größe in CSS-Pixeln, Maßstab Pixel je CSS-Pixel
    fn resize(&self, w: i32, h: i32, scale: f64);
    fn set_visible(&self, on: bool);
}

pub struct WpeTab {
    pub view: gpointer,
    pub wpe: gpointer,
    pub toplevel: gpointer,
}

impl WpeTab {
    pub fn new(view: gpointer) -> Self {
        unsafe {
            let wpe = webkit_web_view_get_wpe_view(view);
            let mut tl = wpe_view_get_toplevel(wpe);
            if tl.is_null() {
                tl = g_object_new(
                    crate::platform::toplevel_type(),
                    cs("display").as_ptr(),
                    wpe_view_get_display(wpe),
                    std::ptr::null::<std::ffi::c_char>(),
                );
                wpe_view_set_toplevel(wpe, tl);
            }
            Self {
                view,
                wpe,
                toplevel: tl,
            }
        }
    }
}

impl WebEngine for WpeTab {
    fn load(&self, uri: &str) {
        let u = cs(uri);
        unsafe { webkit_web_view_load_uri(self.view, u.as_ptr()) }
    }
    fn back(&self) {
        unsafe { webkit_web_view_go_back(self.view) }
    }
    fn forward(&self) {
        unsafe { webkit_web_view_go_forward(self.view) }
    }
    fn reload(&self) {
        unsafe { webkit_web_view_reload(self.view) }
    }
    fn stop(&self) {
        unsafe { webkit_web_view_stop_loading(self.view) }
    }
    fn can_back(&self) -> bool {
        unsafe { webkit_web_view_can_go_back(self.view) != 0 }
    }
    fn can_forward(&self) -> bool {
        unsafe { webkit_web_view_can_go_forward(self.view) != 0 }
    }
    fn title(&self) -> String {
        unsafe { rs(webkit_web_view_get_title(self.view)) }
    }
    fn uri(&self) -> String {
        unsafe { rs(webkit_web_view_get_uri(self.view)) }
    }
    fn progress(&self) -> f64 {
        unsafe { webkit_web_view_get_estimated_load_progress(self.view) }
    }
    fn loading(&self) -> bool {
        unsafe { webkit_web_view_is_loading(self.view) != 0 }
    }
    fn secure(&self) -> bool {
        let mut cert = std::ptr::null_mut();
        let mut errs = 0u32;
        unsafe { webkit_web_view_get_tls_info(self.view, &mut cert, &mut errs) != 0 && errs == 0 }
    }
    fn playing_audio(&self) -> bool {
        unsafe { webkit_web_view_is_playing_audio(self.view) != 0 }
    }
    fn touch(&self, kind: u8, x: f64, y: f64, time: u32) {
        let t: c_int = match kind {
            0 => WPE_EVENT_TOUCH_DOWN,
            1 => WPE_EVENT_TOUCH_MOVE,
            _ => WPE_EVENT_TOUCH_UP,
        };
        unsafe {
            let ev =
                wpe_event_touch_new(t, self.wpe, WPE_INPUT_SOURCE_TOUCHSCREEN, time, 0, 1, x, y);
            wpe_view_event(self.wpe, ev);
            wpe_event_unref(ev);
        }
    }
    fn key(&self, keyval: u32, keycode: u32, time: u32) {
        unsafe {
            for t in [WPE_EVENT_KEYBOARD_KEY_DOWN, WPE_EVENT_KEYBOARD_KEY_UP] {
                let ev = wpe_event_keyboard_new(
                    t,
                    self.wpe,
                    WPE_INPUT_SOURCE_KEYBOARD,
                    time,
                    0,
                    keycode,
                    keyval,
                );
                wpe_view_event(self.wpe, ev);
                wpe_event_unref(ev);
            }
        }
    }
    fn resize(&self, w: i32, h: i32, scale: f64) {
        if crate::platform::debug() {
            eprintln!(
                "ultar-web[dbg]: resize {w}x{h} scale {scale:.2} toplevel={:?}",
                self.toplevel
            );
        }
        unsafe {
            wpe_toplevel_scale_changed(self.toplevel, scale);
            wpe_toplevel_resized(self.toplevel, w, h);
            wpe_toplevel_state_changed(self.toplevel, WPE_TOPLEVEL_STATE_ACTIVE);
            wpe_view_resized(self.wpe, w, h);
        }
    }
    fn set_visible(&self, on: bool) {
        if crate::platform::debug() {
            eprintln!("ultar-web[dbg]: sichtbar {on}");
        }
        unsafe {
            wpe_view_set_visible(self.wpe, on as gboolean);
            if on {
                wpe_view_map(self.wpe);
                wpe_view_focus_in(self.wpe);
            } else {
                wpe_view_focus_out(self.wpe);
                wpe_view_unmap(self.wpe);
            }
        }
    }
}
