//! Eigene WPEPlatform-Plattform „ultar“ – vollständig in Rust.
//! Drei GObject-Unterklassen (Display, Toplevel, View) plus Bildschirm und Eingabemethode.
//! WebKit liefert fertig gezeichnete Bilder als Shared-Memory-Puffer (Skia auf der CPU); die View gibt sie
//! an den Compositor-Thread weiter. Kein Wayland, kein Weston, kein DRM-Zwang.

use crate::ffi::*;
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_uint};
use std::ptr::null_mut;
use std::sync::OnceLock;

/// Ein fertiges Bild von WebKit (Zeiger bleiben gültig, solange `buffer` referenziert ist).
pub struct Frame {
    pub view: usize,
    pub buffer: usize,
    pub data: usize,
    pub len: usize,
    pub stride: u32,
    pub w: i32,
    pub h: i32,
}
// Die Zeiger werden nur gelesen; Freigabe erfolgt immer auf dem Hauptthread.
unsafe impl Send for Frame {}

/// Ereignisse der Plattform an den Browser (werden gesammelt und im Hauptthread abgearbeitet).
#[derive(Clone, Debug)]
pub enum PlatEv {
    /// Eingabefeld der Seite hat den Fokus: Bildschirmtastatur zeigen (außer die Seite verbietet es)
    ImFocusIn { ctx: usize, view: usize, purpose: i32, osk: bool },
    ImFocusOut { ctx: usize },
    Fullscreen { view: usize, on: bool },
}

thread_local! {
    static PENDING: RefCell<Vec<PlatEv>> = const { RefCell::new(Vec::new()) };
    static SINK: RefCell<Option<Box<dyn Fn(Frame)>>> = const { RefCell::new(None) };
    static WAKE: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

pub fn set_frame_sink(f: impl Fn(Frame) + 'static) {
    SINK.with(|s| *s.borrow_mut() = Some(Box::new(f)));
}
pub fn set_waker(f: impl Fn() + 'static) {
    WAKE.with(|s| *s.borrow_mut() = Some(Box::new(f)));
}
pub fn take_events() -> Vec<PlatEv> {
    PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()))
}
fn push(ev: PlatEv) {
    PENDING.with(|p| p.borrow_mut().push(ev));
    WAKE.with(|w| {
        if let Some(f) = w.borrow().as_ref() {
            f()
        }
    });
}

static TYPES: OnceLock<Types> = OnceLock::new();
struct Types {
    display: GType,
    view: GType,
    toplevel: GType,
    screen: GType,
    im: GType,
}

/// Geräteeigenschaften des Bildschirms, für den WebKit zeichnet.
static SCREEN_CFG: OnceLock<(i32, i32, f64)> = OnceLock::new();
static mut SCREEN: usize = 0;

unsafe fn register(parent: GType, name: &str, expect_class: usize, init: unsafe extern "C" fn(gpointer, gpointer)) -> GType {
    let mut q: GTypeQuery = std::mem::zeroed();
    g_type_query(parent, &mut q);
    // Sicherheitsprüfung: Klassenaufbau muss zur Bibliothek passen, sonst würden falsche Funktionszeiger gesetzt
    assert_eq!(q.class_size as usize, expect_class, "Klassengröße von {name} passt nicht zur WPE-Bibliothek");
    let info = GTypeInfo {
        class_size: q.class_size as u16,
        base_init: None,
        base_finalize: None,
        class_init: Some(init),
        class_finalize: None,
        class_data: std::ptr::null(),
        instance_size: q.instance_size as u16,
        n_preallocs: 0,
        instance_init: None,
        value_table: std::ptr::null(),
    };
    let n = cs(name);
    g_type_register_static(parent, n.as_ptr(), &info, 0)
}

fn types() -> &'static Types {
    TYPES.get_or_init(|| unsafe {
        assert_eq!(std::mem::size_of::<GObjectClass>(), 136);
        Types {
            display: register(wpe_display_get_type(), "UltarDisplay", std::mem::size_of::<WPEDisplayClass>(), display_class_init),
            view: register(wpe_view_get_type(), "UltarView", std::mem::size_of::<WPEViewClass>(), view_class_init),
            toplevel: register(wpe_toplevel_get_type(), "UltarToplevel", std::mem::size_of::<WPEToplevelClass>(), toplevel_class_init),
            screen: register(wpe_screen_get_type(), "UltarScreen", std::mem::size_of::<WPEScreenClass>(), screen_class_init),
            im: register(wpe_input_method_context_get_type(), "UltarInputMethod", std::mem::size_of::<WPEInputMethodContextClass>(), im_class_init),
        }
    })
}

/// Plattform anlegen und als primäre Anzeige für WebKit setzen.
/// `w`,`h`: Größe in physischen Pixeln, `scale`: Pixel je CSS-Pixel.
pub fn create_display(w: i32, h: i32, scale: f64, touch: bool, keyboard: bool) -> gpointer {
    let _ = SCREEN_CFG.set((w, h, scale));
    let t = types();
    unsafe {
        let d = g_object_new(t.display, std::ptr::null::<c_char>());
        let mut err: *mut GError = null_mut();
        if wpe_display_connect(d, &mut err) == FALSE {
            eprintln!("ultar-web: Anzeige konnte nicht verbunden werden: {}", if err.is_null() { String::new() } else { rs((*err).message) });
        }
        let mut dev = WPE_AVAILABLE_INPUT_DEVICE_MOUSE;
        if touch {
            dev |= WPE_AVAILABLE_INPUT_DEVICE_TOUCHSCREEN;
        }
        if keyboard {
            dev |= WPE_AVAILABLE_INPUT_DEVICE_KEYBOARD;
        }
        wpe_display_set_available_input_devices(d, dev);
        // Overlay-Bildlaufleisten wie auf Handys
        let st = wpe_display_get_settings(d);
        if !st.is_null() {
            let k = cs("/wpe-platform/overlay-scrollbars");
            wpe_settings_set_boolean(st, k.as_ptr(), TRUE, WPE_SETTINGS_SOURCE_APPLICATION, null_mut());
        }
        wpe_display_set_primary(d);
        d
    }
}

pub fn toplevel_type() -> GType {
    types().toplevel
}

// ---------------- Display ----------------
unsafe extern "C" fn display_class_init(klass: gpointer, _d: gpointer) {
    let k = &mut *(klass as *mut WPEDisplayClass);
    k.connect = Some(d_connect);
    k.create_view = Some(d_create_view);
    k.create_toplevel = Some(d_create_toplevel);
    k.get_n_screens = Some(d_n_screens);
    k.get_screen = Some(d_get_screen);
    k.create_input_method_context = Some(d_create_im);
    // get_egl_display bleibt leer: keine GPU – WebKit zeichnet mit Skia auf der CPU in Shared Memory.
}

unsafe extern "C" fn d_connect(display: gpointer, _e: *mut *mut GError) -> gboolean {
    let (w, h, scale) = *SCREEN_CFG.get().unwrap_or(&(720, 1280, 2.0));
    let s = g_object_new(types().screen, cs("id").as_ptr(), 1u32, std::ptr::null::<c_char>());
    wpe_screen_set_size(s, (w as f64 / scale).round() as c_int, (h as f64 / scale).round() as c_int);
    // physische Größe grob aus typischer Handydichte (~400 dpi); nur für CSS-Medienabfragen
    wpe_screen_set_physical_size(s, (w as f64 * 25.4 / 400.0) as c_int, (h as f64 * 25.4 / 400.0) as c_int);
    wpe_screen_set_scale(s, scale);
    wpe_screen_set_refresh_rate(s, 60_000);
    SCREEN = s as usize;
    wpe_display_screen_added(display, s);
    TRUE
}

unsafe extern "C" fn d_create_view(display: gpointer) -> gpointer {
    g_object_new(types().view, cs("display").as_ptr(), display, std::ptr::null::<c_char>())
}

unsafe extern "C" fn d_create_toplevel(display: gpointer, _max: c_uint) -> gpointer {
    g_object_new(types().toplevel, cs("display").as_ptr(), display, std::ptr::null::<c_char>())
}

unsafe extern "C" fn d_n_screens(_d: gpointer) -> c_uint {
    (SCREEN != 0) as c_uint
}

unsafe extern "C" fn d_get_screen(_d: gpointer, i: c_uint) -> gpointer {
    if i == 0 { SCREEN as gpointer } else { null_mut() }
}

unsafe extern "C" fn d_create_im(_d: gpointer, view: gpointer) -> gpointer {
    g_object_new(types().im, cs("view").as_ptr(), view, std::ptr::null::<c_char>())
}

// ---------------- Toplevel ----------------
unsafe extern "C" fn toplevel_class_init(klass: gpointer, _d: gpointer) {
    let k = &mut *(klass as *mut WPEToplevelClass);
    k.get_screen = Some(t_get_screen);
    k.resize = Some(t_resize);
    k.set_fullscreen = Some(t_fullscreen);
    k.set_maximized = Some(t_maximized);
}

unsafe extern "C" fn t_get_screen(_t: gpointer) -> gpointer {
    SCREEN as gpointer
}
unsafe extern "C" fn t_resize(t: gpointer, w: c_int, h: c_int) -> gboolean {
    // Größe bestimmt der Browser (Inhaltsbereich); Wünsche der Seite werden nur bestätigt
    let _ = (t, w, h);
    FALSE
}
unsafe extern "C" fn t_maximized(_t: gpointer, _m: gboolean) -> gboolean {
    TRUE
}
unsafe extern "C" fn t_fullscreen(t: gpointer, on: gboolean) -> gboolean {
    // Vollbild (z. B. Video): Browser blendet seine Leisten aus und meldet die neue Größe
    let mut view = 0usize;
    unsafe extern "C" fn first(_t: gpointer, v: gpointer, data: gpointer) -> gboolean {
        *(data as *mut usize) = v as usize;
        FALSE
    }
    wpe_toplevel_foreach_view(t, Some(first), &mut view as *mut usize as gpointer);
    push(PlatEv::Fullscreen { view, on: on != FALSE });
    let st = WPE_TOPLEVEL_STATE_ACTIVE | if on != FALSE { WPE_TOPLEVEL_STATE_FULLSCREEN } else { 0 };
    wpe_toplevel_state_changed(t, st);
    TRUE
}

// ---------------- Screen ----------------
unsafe extern "C" fn screen_class_init(_klass: gpointer, _d: gpointer) {}

// ---------------- View ----------------
unsafe extern "C" fn view_class_init(klass: gpointer, _d: gpointer) {
    let k = &mut *(klass as *mut WPEViewClass);
    k.render_buffer = Some(v_render_buffer);
}

pub fn debug() -> bool {
    static D: OnceLock<bool> = OnceLock::new();
    *D.get_or_init(|| std::env::var("ULTAR_WEB_DEBUG").is_ok())
}

unsafe extern "C" fn v_render_buffer(view: gpointer, buffer: gpointer, _dmg: *const WPERectangle, _n: c_uint, err: *mut *mut GError) -> gboolean {
    if debug() {
        eprintln!("ultar-web[dbg]: render_buffer {}x{} shm={}", wpe_buffer_get_width(buffer), wpe_buffer_get_height(buffer), g_type_check_instance_is_a(buffer, wpe_buffer_shm_get_type()));
    }
    if g_type_check_instance_is_a(buffer, wpe_buffer_shm_get_type()) == FALSE {
        // DMA-BUF (GPU) wird auf diesem Weg nicht genutzt
        let _ = err;
        wpe_view_buffer_rendered(view, buffer);
        wpe_view_buffer_released(view, buffer);
        return TRUE;
    }
    let bytes = wpe_buffer_shm_get_data(buffer);
    let mut len = 0usize;
    let data = g_bytes_get_data(bytes, &mut len);
    g_object_ref(buffer);
    g_object_ref(view);
    let f = Frame {
        view: view as usize,
        buffer: buffer as usize,
        data: data as usize,
        len,
        stride: wpe_buffer_shm_get_stride(buffer),
        w: wpe_buffer_get_width(buffer),
        h: wpe_buffer_get_height(buffer),
    };
    let mut f = Some(f);
    SINK.with(|s| {
        if let Some(sink) = s.borrow().as_ref() {
            sink(f.take().unwrap());
        }
    });
    if let Some(f) = f {
        // niemand nimmt das Bild ab: sofort freigeben
        frame_done(f.view, f.buffer);
    }
    TRUE
}

/// Bild ist auf dem Schirm: WebKit darf den Puffer wieder verwenden (nur im Hauptthread aufrufen).
pub fn frame_done(view: usize, buffer: usize) {
    unsafe {
        wpe_view_buffer_rendered(view as gpointer, buffer as gpointer);
        wpe_view_buffer_released(view as gpointer, buffer as gpointer);
        g_object_unref(buffer as gpointer);
        g_object_unref(view as gpointer);
    }
}

// ---------------- Eingabemethode (Bildschirmtastatur) ----------------
unsafe extern "C" fn im_class_init(klass: gpointer, _d: gpointer) {
    let k = &mut *(klass as *mut WPEInputMethodContextClass);
    k.focus_in = Some(im_focus_in);
    k.focus_out = Some(im_focus_out);
    k.get_preedit_string = Some(im_preedit);
    k.filter_key_event = Some(im_filter);
    k.reset = Some(im_reset);
}

unsafe extern "C" fn im_focus_in(ctx: gpointer) {
    let hints = wpe_input_method_context_get_input_hints(ctx);
    push(PlatEv::ImFocusIn {
        ctx: ctx as usize,
        view: wpe_input_method_context_get_view(ctx) as usize,
        purpose: wpe_input_method_context_get_input_purpose(ctx),
        osk: hints & WPE_INPUT_HINT_INHIBIT_OSK == 0,
    });
}
unsafe extern "C" fn im_focus_out(ctx: gpointer) {
    push(PlatEv::ImFocusOut { ctx: ctx as usize });
}
unsafe extern "C" fn im_preedit(_ctx: gpointer, text: *mut *mut c_char, underlines: *mut gpointer, cursor: *mut c_uint) {
    if !text.is_null() {
        *text = g_strdup(c"".as_ptr());
    }
    if !underlines.is_null() {
        *underlines = null_mut();
    }
    if !cursor.is_null() {
        *cursor = 0;
    }
}
unsafe extern "C" fn im_filter(_ctx: gpointer, _ev: gpointer) -> gboolean {
    FALSE
}
unsafe extern "C" fn im_reset(_ctx: gpointer) {}

/// Text aus der Bildschirmtastatur an das fokussierte Feld der Seite übergeben.
pub fn im_commit(ctx: usize, text: &str) {
    if ctx == 0 {
        return;
    }
    let t = cs(text);
    let sig = cs("committed");
    unsafe { g_signal_emit_by_name(ctx as gpointer, sig.as_ptr(), t.as_ptr()) };
}
