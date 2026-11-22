//! Handgeschriebene C-Schnittstelle zu GLib/GObject, WPEPlatform und WPE WebKit 2.0 (nur was Ultar Web braucht).
//! Bewusst ohne bindgen: die Laufzeit kommt aus dem Buildroot-Bündel, die Typen sind hier festgelegt und
//! werden beim Start gegen die Bibliothek geprüft (Klassengrößen über g_type_query).
#![allow(non_camel_case_types, dead_code, clippy::upper_case_acronyms)]

use std::ffi::{c_char, c_double, c_int, c_uint, c_void};

pub type gboolean = c_int;
pub type GType = usize;
pub type gpointer = *mut c_void;
pub type GCallback = Option<unsafe extern "C" fn()>;
pub type GDestroyNotify = Option<unsafe extern "C" fn(gpointer)>;
pub type GSourceFunc = Option<unsafe extern "C" fn(gpointer) -> gboolean>;
pub type GUnixFDSourceFunc = Option<unsafe extern "C" fn(c_int, c_uint, gpointer) -> gboolean>;
pub type GAsyncReadyCallback = Option<unsafe extern "C" fn(gpointer, gpointer, gpointer)>;

pub const TRUE: gboolean = 1;
pub const FALSE: gboolean = 0;

#[repr(C)]
pub struct GError {
    pub domain: u32,
    pub code: c_int,
    pub message: *mut c_char,
}

#[repr(C)]
pub struct GTypeInfo {
    pub class_size: u16,
    pub base_init: Option<unsafe extern "C" fn(gpointer)>,
    pub base_finalize: Option<unsafe extern "C" fn(gpointer)>,
    pub class_init: Option<unsafe extern "C" fn(gpointer, gpointer)>,
    pub class_finalize: Option<unsafe extern "C" fn(gpointer, gpointer)>,
    pub class_data: *const c_void,
    pub instance_size: u16,
    pub n_preallocs: u16,
    pub instance_init: Option<unsafe extern "C" fn(gpointer, gpointer)>,
    pub value_table: *const c_void,
}

#[repr(C)]
pub struct GTypeQuery {
    pub type_: GType,
    pub type_name: *const c_char,
    pub class_size: c_uint,
    pub instance_size: c_uint,
}

/// GObjectClass: 136 Byte auf 64-Bit (wird beim Start geprüft).
#[repr(C)]
pub struct GObjectClass {
    pub _p: [usize; 17],
}

// ---- WPEPlatform-Klassen (Aufbau exakt wie in den Headern von WPE WebKit 2.54) ----
#[repr(C)]
pub struct WPEDisplayClass {
    pub parent: GObjectClass,
    pub connect: Option<unsafe extern "C" fn(gpointer, *mut *mut GError) -> gboolean>,
    pub create_view: Option<unsafe extern "C" fn(gpointer) -> gpointer>,
    pub get_egl_display: Option<unsafe extern "C" fn(gpointer, *mut *mut GError) -> gpointer>,
    pub get_keymap: Option<unsafe extern "C" fn(gpointer) -> gpointer>,
    pub get_clipboard: Option<unsafe extern "C" fn(gpointer) -> gpointer>,
    pub get_preferred_buffer_formats: Option<unsafe extern "C" fn(gpointer) -> gpointer>,
    pub get_n_screens: Option<unsafe extern "C" fn(gpointer) -> c_uint>,
    pub get_screen: Option<unsafe extern "C" fn(gpointer, c_uint) -> gpointer>,
    pub get_drm_device: Option<unsafe extern "C" fn(gpointer) -> gpointer>,
    pub use_explicit_sync: Option<unsafe extern "C" fn(gpointer) -> gboolean>,
    pub create_input_method_context: Option<unsafe extern "C" fn(gpointer, gpointer) -> gpointer>,
    pub create_toplevel: Option<unsafe extern "C" fn(gpointer, c_uint) -> gpointer>,
    pub create_gamepad_manager: Option<unsafe extern "C" fn(gpointer) -> gpointer>,
    pub padding: [usize; 32],
}

#[repr(C)]
pub struct WPEViewClass {
    pub parent: GObjectClass,
    pub buffers_changed: Option<unsafe extern "C" fn(gpointer, *mut gpointer, c_uint)>,
    pub render_buffer: Option<unsafe extern "C" fn(gpointer, gpointer, *const WPERectangle, c_uint, *mut *mut GError) -> gboolean>,
    pub lock_pointer: Option<unsafe extern "C" fn(gpointer) -> gboolean>,
    pub unlock_pointer: Option<unsafe extern "C" fn(gpointer) -> gboolean>,
    pub set_cursor_from_name: Option<unsafe extern "C" fn(gpointer, *const c_char)>,
    pub set_cursor_from_bytes: Option<unsafe extern "C" fn(gpointer, gpointer, c_uint, c_uint, c_uint, c_uint, c_uint)>,
    pub set_opaque_rectangles: Option<unsafe extern "C" fn(gpointer, *mut WPERectangle, c_uint)>,
    pub can_be_mapped: Option<unsafe extern "C" fn(gpointer) -> gboolean>,
    pub get_accessible: Option<unsafe extern "C" fn(gpointer) -> gpointer>,
    pub padding: [usize; 32],
}

#[repr(C)]
pub struct WPEToplevelClass {
    pub parent: GObjectClass,
    pub set_title: Option<unsafe extern "C" fn(gpointer, *const c_char)>,
    pub get_screen: Option<unsafe extern "C" fn(gpointer) -> gpointer>,
    pub resize: Option<unsafe extern "C" fn(gpointer, c_int, c_int) -> gboolean>,
    pub set_fullscreen: Option<unsafe extern "C" fn(gpointer, gboolean) -> gboolean>,
    pub set_maximized: Option<unsafe extern "C" fn(gpointer, gboolean) -> gboolean>,
    pub set_minimized: Option<unsafe extern "C" fn(gpointer) -> gboolean>,
    pub get_preferred_buffer_formats: Option<unsafe extern "C" fn(gpointer) -> gpointer>,
    pub padding: [usize; 32],
}

#[repr(C)]
pub struct WPEScreenClass {
    pub parent: GObjectClass,
    pub invalidate: Option<unsafe extern "C" fn(gpointer)>,
    pub get_sync_observer: Option<unsafe extern "C" fn(gpointer) -> gpointer>,
    pub padding: [usize; 8],
}

#[repr(C)]
pub struct WPEInputMethodContextClass {
    pub parent: GObjectClass,
    pub get_preedit_string: Option<unsafe extern "C" fn(gpointer, *mut *mut c_char, *mut gpointer, *mut c_uint)>,
    pub filter_key_event: Option<unsafe extern "C" fn(gpointer, gpointer) -> gboolean>,
    pub focus_in: Option<unsafe extern "C" fn(gpointer)>,
    pub focus_out: Option<unsafe extern "C" fn(gpointer)>,
    pub set_cursor_area: Option<unsafe extern "C" fn(gpointer, c_int, c_int, c_int, c_int)>,
    pub set_surrounding: Option<unsafe extern "C" fn(gpointer, *const c_char, c_uint, c_uint, c_uint)>,
    pub reset: Option<unsafe extern "C" fn(gpointer)>,
    pub padding: [usize; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct WPERectangle {
    pub x: c_int,
    pub y: c_int,
    pub width: c_int,
    pub height: c_int,
}

// Ereignisse (WPEEventType)
pub const WPE_EVENT_POINTER_DOWN: c_int = 1;
pub const WPE_EVENT_POINTER_UP: c_int = 2;
pub const WPE_EVENT_POINTER_MOVE: c_int = 3;
pub const WPE_EVENT_KEYBOARD_KEY_DOWN: c_int = 7;
pub const WPE_EVENT_KEYBOARD_KEY_UP: c_int = 8;
pub const WPE_EVENT_TOUCH_DOWN: c_int = 9;
pub const WPE_EVENT_TOUCH_UP: c_int = 10;
pub const WPE_EVENT_TOUCH_MOVE: c_int = 11;
pub const WPE_INPUT_SOURCE_MOUSE: c_int = 0;
pub const WPE_INPUT_SOURCE_KEYBOARD: c_int = 2;
pub const WPE_INPUT_SOURCE_TOUCHSCREEN: c_int = 3;
pub const WPE_TOPLEVEL_STATE_FULLSCREEN: c_uint = 1;
pub const WPE_TOPLEVEL_STATE_ACTIVE: c_uint = 4;
pub const WPE_AVAILABLE_INPUT_DEVICE_MOUSE: c_uint = 1;
pub const WPE_AVAILABLE_INPUT_DEVICE_KEYBOARD: c_uint = 2;
pub const WPE_AVAILABLE_INPUT_DEVICE_TOUCHSCREEN: c_uint = 4;
pub const WPE_INPUT_HINT_INHIBIT_OSK: c_uint = 1 << 7;
pub const WPE_SETTINGS_SOURCE_APPLICATION: c_int = 1;

// WebKit-Aufzählungen
pub const WEBKIT_LOAD_STARTED: c_int = 0;
pub const WEBKIT_LOAD_COMMITTED: c_int = 2;
pub const WEBKIT_LOAD_FINISHED: c_int = 3;
pub const WEBKIT_HARDWARE_ACCELERATION_POLICY_NEVER: c_int = 1;
pub const WEBKIT_CACHE_MODEL_WEB_BROWSER: c_int = 2;
pub const WEBKIT_COOKIE_POLICY_ACCEPT_NO_THIRD_PARTY: c_int = 2;
pub const WEBKIT_TLS_ERRORS_POLICY_FAIL: c_int = 1;
pub const WEBKIT_SCRIPT_DIALOG_ALERT: c_int = 0;
pub const WEBKIT_SCRIPT_DIALOG_CONFIRM: c_int = 1;
pub const WEBKIT_SCRIPT_DIALOG_PROMPT: c_int = 2;
pub const WEBKIT_SCRIPT_DIALOG_BEFORE_UNLOAD_CONFIRM: c_int = 3;

extern "C" {
    // GLib
    pub fn g_free(p: gpointer);
    pub fn g_error_free(e: *mut GError);
    pub fn g_main_loop_new(ctx: gpointer, running: gboolean) -> gpointer;
    pub fn g_main_loop_run(l: gpointer);
    pub fn g_main_loop_quit(l: gpointer);
    pub fn g_idle_add(f: GSourceFunc, data: gpointer) -> c_uint;
    pub fn g_timeout_add(ms: c_uint, f: GSourceFunc, data: gpointer) -> c_uint;
    pub fn g_unix_fd_add(fd: c_int, cond: c_uint, f: GUnixFDSourceFunc, data: gpointer) -> c_uint;
    pub fn g_bytes_get_data(b: gpointer, size: *mut usize) -> *const c_void;
    pub fn g_bytes_new(data: *const c_void, size: usize) -> gpointer;
    pub fn g_bytes_unref(b: gpointer);
    pub fn g_bytes_get_size(b: gpointer) -> usize;
    pub fn g_variant_new_boolean(b: gboolean) -> gpointer;

    // GObject
    pub fn g_type_register_static(parent: GType, name: *const c_char, info: *const GTypeInfo, flags: c_uint) -> GType;
    pub fn g_type_query(t: GType, q: *mut GTypeQuery);
    pub fn g_type_check_instance_is_a(inst: gpointer, t: GType) -> gboolean;
    pub fn g_object_new(t: GType, first: *const c_char, ...) -> gpointer;
    pub fn g_object_ref(o: gpointer) -> gpointer;
    pub fn g_object_unref(o: gpointer);
    pub fn g_signal_connect_data(inst: gpointer, sig: *const c_char, cb: GCallback, data: gpointer, destroy: gpointer, flags: c_uint) -> u64;
    pub fn g_signal_emit_by_name(inst: gpointer, sig: *const c_char, ...);

    // WPEPlatform
    pub fn wpe_display_get_type() -> GType;
    pub fn wpe_view_get_type() -> GType;
    pub fn wpe_toplevel_get_type() -> GType;
    pub fn wpe_screen_get_type() -> GType;
    pub fn wpe_input_method_context_get_type() -> GType;
    pub fn wpe_buffer_shm_get_type() -> GType;
    pub fn wpe_display_set_primary(d: gpointer);
    pub fn wpe_display_connect(d: gpointer, e: *mut *mut GError) -> gboolean;
    pub fn wpe_display_screen_added(d: gpointer, s: gpointer);
    pub fn wpe_display_set_available_input_devices(d: gpointer, dev: c_uint);
    pub fn wpe_display_get_settings(d: gpointer) -> gpointer;
    pub fn wpe_settings_set_boolean(s: gpointer, key: *const c_char, v: gboolean, src: c_int, e: *mut *mut GError) -> gboolean;
    pub fn wpe_screen_set_size(s: gpointer, w: c_int, h: c_int);
    pub fn wpe_screen_set_physical_size(s: gpointer, w: c_int, h: c_int);
    pub fn wpe_screen_set_scale(s: gpointer, scale: c_double);
    pub fn wpe_screen_set_refresh_rate(s: gpointer, mhz: c_int);
    pub fn wpe_view_get_toplevel(v: gpointer) -> gpointer;
    pub fn wpe_view_resized(v: gpointer, w: c_int, h: c_int);
    pub fn wpe_view_map(v: gpointer);
    pub fn wpe_view_unmap(v: gpointer);
    pub fn wpe_view_set_visible(v: gpointer, vis: gboolean);
    pub fn wpe_view_get_width(v: gpointer) -> c_int;
    pub fn wpe_view_get_height(v: gpointer) -> c_int;
    pub fn wpe_view_buffer_rendered(v: gpointer, b: gpointer);
    pub fn wpe_view_buffer_released(v: gpointer, b: gpointer);
    pub fn wpe_view_event(v: gpointer, ev: gpointer);
    pub fn wpe_view_focus_in(v: gpointer);
    pub fn wpe_view_focus_out(v: gpointer);
    pub fn wpe_toplevel_resized(t: gpointer, w: c_int, h: c_int);
    pub fn wpe_toplevel_state_changed(t: gpointer, st: c_uint);
    pub fn wpe_toplevel_scale_changed(t: gpointer, scale: c_double);
    pub fn wpe_buffer_get_width(b: gpointer) -> c_int;
    pub fn wpe_buffer_get_height(b: gpointer) -> c_int;
    pub fn wpe_buffer_shm_get_data(b: gpointer) -> gpointer;
    pub fn wpe_buffer_shm_get_stride(b: gpointer) -> c_uint;
    pub fn wpe_event_touch_new(t: c_int, view: gpointer, src: c_int, time: u32, mods: c_uint, seq: u32, x: c_double, y: c_double) -> gpointer;
    pub fn wpe_event_pointer_button_new(t: c_int, view: gpointer, src: c_int, time: u32, mods: c_uint, button: c_uint, x: c_double, y: c_double, count: c_uint) -> gpointer;
    pub fn wpe_event_pointer_move_new(t: c_int, view: gpointer, src: c_int, time: u32, mods: c_uint, x: c_double, y: c_double, dx: c_double, dy: c_double) -> gpointer;
    pub fn wpe_event_scroll_new(view: gpointer, src: c_int, time: u32, mods: c_uint, dx: c_double, dy: c_double, precise: gboolean, stop: gboolean, x: c_double, y: c_double) -> gpointer;
    pub fn wpe_event_keyboard_new(t: c_int, view: gpointer, src: c_int, time: u32, mods: c_uint, keycode: c_uint, keyval: c_uint) -> gpointer;
    pub fn wpe_event_unref(e: gpointer);
    pub fn wpe_input_method_context_get_view(c: gpointer) -> gpointer;
    pub fn wpe_input_method_context_get_input_purpose(c: gpointer) -> c_int;
    pub fn wpe_input_method_context_get_input_hints(c: gpointer) -> c_uint;

    // WebKit
    pub fn webkit_web_view_get_type() -> GType;
    pub fn webkit_web_view_get_wpe_view(v: gpointer) -> gpointer;
    pub fn webkit_web_view_load_uri(v: gpointer, uri: *const c_char);
    pub fn webkit_web_view_load_html(v: gpointer, html: *const c_char, base: *const c_char);
    pub fn webkit_web_view_reload(v: gpointer);
    pub fn webkit_web_view_stop_loading(v: gpointer);
    pub fn webkit_web_view_go_back(v: gpointer);
    pub fn webkit_web_view_go_forward(v: gpointer);
    pub fn webkit_web_view_can_go_back(v: gpointer) -> gboolean;
    pub fn webkit_web_view_can_go_forward(v: gpointer) -> gboolean;
    pub fn webkit_web_view_get_uri(v: gpointer) -> *const c_char;
    pub fn webkit_web_view_get_title(v: gpointer) -> *const c_char;
    pub fn webkit_web_view_get_estimated_load_progress(v: gpointer) -> c_double;
    pub fn webkit_web_view_is_loading(v: gpointer) -> gboolean;
    pub fn webkit_web_view_get_settings(v: gpointer) -> gpointer;
    pub fn webkit_web_view_try_close(v: gpointer);
    pub fn webkit_web_view_get_session_state(v: gpointer) -> gpointer;
    pub fn webkit_web_view_restore_session_state(v: gpointer, s: gpointer);
    pub fn webkit_web_view_session_state_unref(s: gpointer);
    pub fn webkit_web_view_get_tls_info(v: gpointer, cert: *mut gpointer, errors: *mut c_uint) -> gboolean;
    pub fn webkit_settings_new() -> gpointer;
    pub fn webkit_settings_set_hardware_acceleration_policy(s: gpointer, p: c_int);
    pub fn webkit_settings_set_user_agent(s: gpointer, ua: *const c_char);
    pub fn webkit_settings_set_enable_dns_prefetching(s: gpointer, b: gboolean);
    pub fn webkit_settings_set_enable_page_cache(s: gpointer, b: gboolean);
    pub fn webkit_settings_set_enable_smooth_scrolling(s: gpointer, b: gboolean);
    pub fn webkit_settings_set_enable_hyperlink_auditing(s: gpointer, b: gboolean);
    pub fn webkit_settings_set_enable_javascript(s: gpointer, b: gboolean);
    pub fn webkit_settings_set_enable_webgl(s: gpointer, b: gboolean);
    pub fn webkit_settings_set_media_playback_requires_user_gesture(s: gpointer, b: gboolean);
    pub fn webkit_settings_set_javascript_can_open_windows_automatically(s: gpointer, b: gboolean);
    pub fn webkit_settings_set_sans_serif_font_family(s: gpointer, f: *const c_char);
    pub fn webkit_settings_set_default_font_family(s: gpointer, f: *const c_char);
    pub fn webkit_settings_set_enable_write_console_messages_to_stdout(s: gpointer, b: gboolean);
    pub fn webkit_web_context_set_cache_model(c: gpointer, m: c_int);
    pub fn webkit_web_context_set_preferred_languages(c: gpointer, langs: *const *const c_char);
    pub fn webkit_web_context_register_uri_scheme(c: gpointer, scheme: *const c_char, cb: Option<unsafe extern "C" fn(gpointer, gpointer)>, data: gpointer, destroy: GDestroyNotify);
    pub fn webkit_web_context_get_type() -> GType;
    pub fn webkit_uri_scheme_request_get_path(r: gpointer) -> *const c_char;
    pub fn webkit_uri_scheme_request_finish(r: gpointer, stream: gpointer, len: i64, mime: *const c_char);
    pub fn g_memory_input_stream_new_from_data(data: *const c_void, len: isize, destroy: GDestroyNotify) -> gpointer;
    pub fn webkit_network_session_new(data_dir: *const c_char, cache_dir: *const c_char) -> gpointer;
    pub fn webkit_network_session_new_ephemeral() -> gpointer;
    pub fn webkit_network_session_set_itp_enabled(s: gpointer, b: gboolean);
    pub fn webkit_network_session_get_cookie_manager(s: gpointer) -> gpointer;
    pub fn webkit_network_session_prefetch_dns(s: gpointer, host: *const c_char);
    pub fn webkit_network_session_set_tls_errors_policy(s: gpointer, p: c_int);
    pub fn webkit_network_session_set_memory_pressure_settings(m: gpointer);
    pub fn webkit_cookie_manager_set_accept_policy(m: gpointer, p: c_int);
    pub fn webkit_memory_pressure_settings_new() -> gpointer;
    pub fn webkit_memory_pressure_settings_set_memory_limit(m: gpointer, mb: c_uint);
    pub fn webkit_memory_pressure_settings_set_conservative_threshold(m: gpointer, v: c_double);
    pub fn webkit_memory_pressure_settings_set_strict_threshold(m: gpointer, v: c_double);
    pub fn webkit_memory_pressure_settings_set_poll_interval(m: gpointer, s: c_double);
    pub fn webkit_memory_pressure_settings_free(m: gpointer);
    pub fn webkit_user_content_manager_new() -> gpointer;
    pub fn webkit_user_content_manager_add_filter(m: gpointer, f: gpointer);
    pub fn webkit_user_content_filter_store_new(path: *const c_char) -> gpointer;
    pub fn webkit_user_content_filter_store_save_from_file(st: gpointer, id: *const c_char, file: gpointer, cancel: gpointer, cb: GAsyncReadyCallback, data: gpointer);
    pub fn webkit_user_content_filter_store_save_from_file_finish(st: gpointer, res: gpointer, e: *mut *mut GError) -> gpointer;
    pub fn webkit_user_content_filter_store_load(st: gpointer, id: *const c_char, cancel: gpointer, cb: GAsyncReadyCallback, data: gpointer);
    pub fn webkit_user_content_filter_store_load_finish(st: gpointer, res: gpointer, e: *mut *mut GError) -> gpointer;
    pub fn g_file_new_for_path(p: *const c_char) -> gpointer;
    pub fn webkit_permission_request_allow(r: gpointer);
    pub fn webkit_permission_request_deny(r: gpointer);
    pub fn webkit_script_dialog_get_dialog_type(d: gpointer) -> c_int;
    pub fn webkit_script_dialog_get_message(d: gpointer) -> *const c_char;
    pub fn webkit_script_dialog_confirm_set_confirmed(d: gpointer, b: gboolean);
    pub fn webkit_script_dialog_prompt_get_default_text(d: gpointer) -> *const c_char;
    pub fn webkit_script_dialog_prompt_set_text(d: gpointer, t: *const c_char);
    pub fn webkit_script_dialog_ref(d: gpointer) -> gpointer;
    pub fn webkit_script_dialog_unref(d: gpointer);
    pub fn webkit_script_dialog_close(d: gpointer);
    pub fn webkit_option_menu_get_n_items(m: gpointer) -> c_uint;
    pub fn webkit_option_menu_get_item(m: gpointer, i: c_uint) -> gpointer;
    pub fn webkit_option_menu_item_get_label(i: gpointer) -> *const c_char;
    pub fn webkit_option_menu_item_is_enabled(i: gpointer) -> gboolean;
    pub fn webkit_option_menu_item_is_selected(i: gpointer) -> gboolean;
    pub fn webkit_option_menu_item_is_group_label(i: gpointer) -> gboolean;
    pub fn webkit_option_menu_activate_item(m: gpointer, i: c_uint);
    pub fn webkit_option_menu_close(m: gpointer);
    pub fn webkit_navigation_policy_decision_get_navigation_action(d: gpointer) -> gpointer;
    pub fn webkit_navigation_action_get_request(a: gpointer) -> gpointer;
    pub fn webkit_uri_request_get_uri(r: gpointer) -> *const c_char;
}

/// Rust-&str als NUL-terminierte Zeichenkette.
pub fn cs(s: &str) -> std::ffi::CString {
    std::ffi::CString::new(s.replace('\0', "")).unwrap_or_default()
}

/// C-Zeichenkette (darf NULL sein) nach String.
pub unsafe fn rs(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

/// Signal verbinden; `cb` ist ein extern "C"-Funktionszeiger beliebiger Signatur.
pub unsafe fn connect(inst: gpointer, sig: &str, cb: *const (), data: gpointer) -> u64 {
    let s = cs(sig);
    g_signal_connect_data(inst, s.as_ptr(), Some(std::mem::transmute::<*const (), unsafe extern "C" fn()>(cb)), data, std::ptr::null_mut(), 0)
}

extern "C" {
    pub fn g_strdup(s: *const c_char) -> *mut c_char;
    pub fn g_type_name_from_instance(inst: gpointer) -> *const c_char;
    pub fn g_memory_input_stream_new_from_bytes(b: gpointer) -> gpointer;
    pub fn wpe_toplevel_foreach_view(t: gpointer, f: Option<unsafe extern "C" fn(gpointer, gpointer, gpointer) -> gboolean>, data: gpointer);
    pub fn wpe_view_set_toplevel(v: gpointer, t: gpointer);
    pub fn webkit_web_view_get_back_forward_list(v: gpointer) -> gpointer;
    pub fn webkit_back_forward_list_get_current_item(l: gpointer) -> gpointer;
    pub fn webkit_web_view_go_to_back_forward_list_item(v: gpointer, item: gpointer);
    pub fn webkit_web_view_is_playing_audio(v: gpointer) -> gboolean;
    pub fn webkit_web_view_get_context(v: gpointer) -> gpointer;
    pub fn webkit_file_chooser_request_cancel(r: gpointer);
}

extern "C" {
    pub fn wpe_view_get_display(v: gpointer) -> gpointer;
}

extern "C" {
    pub fn webkit_settings_get_all_features() -> gpointer;
    pub fn webkit_feature_list_get_length(l: gpointer) -> usize;
    pub fn webkit_feature_list_get(l: gpointer, i: usize) -> gpointer;
    pub fn webkit_feature_list_unref(l: gpointer);
    pub fn webkit_feature_get_identifier(f: gpointer) -> *const c_char;
    pub fn webkit_settings_set_feature_enabled(s: gpointer, f: gpointer, on: gboolean);
}

extern "C" {
    pub fn webkit_uri_scheme_request_get_uri(r: gpointer) -> *const c_char;
}

extern "C" {
    pub fn g_unix_signal_add(signum: c_int, f: GSourceFunc, data: gpointer) -> c_uint;
}

extern "C" {
    pub fn webkit_web_view_set_settings(v: gpointer, s: gpointer);
}
