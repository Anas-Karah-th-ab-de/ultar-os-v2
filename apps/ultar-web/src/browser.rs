//! Browser-Prozess von Ultar Web (Rolle wie der Browser-Prozess bei Chrome): Tabs, Leisten, Adresszeile,
//! Tab-Lebenszyklus, Speicherwächter, Datenschutz, Werbefilter und die interne Seite ultar://leistung.

use crate::compositor::{IRect, Msg, Out, Stats};
use crate::engine::{WebEngine, WpeTab};
use crate::ffi::*;
use crate::gfx::{rgb, Canvas, Color, Rect};
use crate::i18n::{t, Lang};
use crate::input::Ev;
use crate::platform::{self, PlatEv};
use crate::text::Text;
use crate::ui::{self, Al, Ic, Key, Ptr, Theme, Ui};
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_uint};
use std::ptr::null_mut;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const MOBILE_UA: &str = "Mozilla/5.0 (Linux; Android 14; Mobile) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Mobile Safari/605.1.15 UltarWeb/1.0";
pub const DESKTOP_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15 UltarWeb/1.0";
pub const SEARCH: &str = "https://duckduckgo.com/?q=";
pub const HOME: &str = "ultar://start";

pub struct Opts {
    pub url: String,
    pub profile: String,
    pub data_dir: String,
    pub bundle: String,
    pub desktop: bool,
    pub app: bool,
    pub lang: Lang,
    pub dark: bool,
    pub accent: usize,
    pub shot: Option<String>,
    pub shot_wait: u64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    Active,
    Hidden,
    Frozen,
    Discarded,
}

impl State {
    fn name(self, l: Lang) -> &'static str {
        match self {
            State::Active => t(l, "aktiv"),
            State::Hidden => t(l, "verborgen"),
            State::Frozen => t(l, "eingefroren"),
            State::Discarded => t(l, "verworfen"),
        }
    }
}

pub struct Tab {
    pub id: u64,
    pub eng: Option<WpeTab>,
    pub title: String,
    pub uri: String,
    pub progress: f64,
    pub loading: bool,
    pub secure: bool,
    pub private: bool,
    pub desktop: bool,
    pub state: State,
    pub hidden_at: Option<Instant>,
    pub used: Instant,
    pub thumb: Option<(i32, i32, Vec<u32>)>,
    /// gemerkter Verlauf eines verworfenen Tabs (WebKitWebViewSessionState)
    pub session: gpointer,
    pub crashed: bool,
}

enum Overlay {
    None,
    Menu,
    Tabs,
    Url { text: String, sugg: Vec<(String, String)> },
    Dialog { d: gpointer, kind: i32, msg: String, text: String },
    Options { m: gpointer, items: Vec<(String, bool, bool, bool)> },
    Perm { req: gpointer, what: String, host: String },
}

#[derive(Clone, Copy, PartialEq)]
enum Capture {
    None,
    Chrome,
    Content,
    Edge(f32),
}

enum Act {
    Back,
    Forward,
    Reload,
    Stop,
    Home,
    EditUrl,
    Submit(String),
    Menu,
    Tabs,
    NewTab(bool),
    Switch(usize),
    CloseTab(usize),
    Desktop,
    Perf,
    Exit,
    Close,
    Dialog(bool),
    Option(u32),
    Perm(bool),
    Osk(Key),
    UrlClear,
}

/// Ereignisse aus WebKit-Signalen (werden gesammelt, nie direkt im Signal verarbeitet)
enum Q {
    Changed(u64),
    Load(u64, c_int),
    Close(u64),
    Crashed(u64),
    Dialog(u64, gpointer),
    Options(u64, gpointer),
    Perm(u64, gpointer),
}

thread_local! {
    static BROWSER: RefCell<Option<Browser>> = const { RefCell::new(None) };
    static QUEUE: RefCell<Vec<Q>> = const { RefCell::new(Vec::new()) };
    static IDLE: RefCell<bool> = const { RefCell::new(false) };
}

pub struct Browser {
    o: Opts,
    w: i32,
    h: i32,
    k: f32,
    scale: f64,
    th: Theme,
    display: gpointer,
    ctx: gpointer,
    sess: gpointer,
    psess: gpointer,
    ucm: gpointer,
    settings_m: gpointer,
    settings_d: gpointer,
    tabs: Vec<Tab>,
    cur: usize,
    next_id: u64,
    ov: Overlay,
    p: Ptr,
    tx: Text,
    cv: Canvas,
    comp: Sender<Msg>,
    outs: Receiver<Out>,
    input: Receiver<Ev>,
    kb: u8,
    im: usize,
    osk: bool,
    full: bool,
    cap: Capture,
    down: (f32, f32),
    dirty: bool,
    content: IRect,
    history: Vec<(String, String)>,
    started: Instant,
    pub stats: Arc<Stats>,
    filters: String,
    saver: bool,
    cpu_prev: Vec<(u64, u64)>,
    cpu_load: Vec<f32>,
    proc_prev: Vec<(i32, u64)>,
    procs: Vec<(i32, String, u64, f32)>,
    lp: gpointer,
    shot_due: Option<Instant>,
    exit_code: i32,
    tscroll: f32,
}

fn now_ms() -> u32 {
    now_ms_impl()
}

/// Browser im Hauptthread ablegen und Signale anschließen.
pub fn install(b: Browser) {
    BROWSER.with(|x| *x.borrow_mut() = Some(b));
}

pub fn with<R>(f: impl FnOnce(&mut Browser) -> R) -> Option<R> {
    BROWSER.with(|x| x.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Verarbeitung im nächsten Durchlauf der Ereignisschleife anstoßen (aus dem Hauptthread).
pub fn schedule() {
    let already = IDLE.with(|i| std::mem::replace(&mut *i.borrow_mut(), true));
    if !already {
        unsafe extern "C" fn idle(_d: gpointer) -> gboolean {
            IDLE.with(|i| *i.borrow_mut() = false);
            pump();
            FALSE
        }
        unsafe { g_idle_add(Some(idle), null_mut()) };
    }
}

fn q(ev: Q) {
    QUEUE.with(|x| x.borrow_mut().push(ev));
    schedule();
}

/// Alles abarbeiten: Plattform-, WebKit-, Eingabe- und Compositor-Ereignisse; dann ggf. Leisten neu zeichnen.
pub fn pump() {
    with(|b| b.pump());
}

// ---------------- WebKit-Signale (nur sammeln) ----------------
unsafe extern "C" fn s_notify(_o: gpointer, _ps: gpointer, id: gpointer) {
    q(Q::Changed(id as u64));
}
unsafe extern "C" fn s_load(_v: gpointer, ev: c_int, id: gpointer) {
    q(Q::Load(id as u64, ev));
}
unsafe extern "C" fn s_close(_v: gpointer, id: gpointer) {
    q(Q::Close(id as u64));
}
unsafe extern "C" fn s_crash(_v: gpointer, _reason: c_int, id: gpointer) {
    q(Q::Crashed(id as u64));
}
unsafe extern "C" fn s_dialog(_v: gpointer, d: gpointer, id: gpointer) -> gboolean {
    webkit_script_dialog_ref(d);
    q(Q::Dialog(id as u64, d));
    TRUE
}
unsafe extern "C" fn s_options(_v: gpointer, m: gpointer, _r: gpointer, id: gpointer) -> gboolean {
    g_object_ref(m);
    q(Q::Options(id as u64, m));
    TRUE
}
unsafe extern "C" fn s_perm(_v: gpointer, req: gpointer, id: gpointer) -> gboolean {
    g_object_ref(req);
    q(Q::Perm(id as u64, req));
    TRUE
}
unsafe extern "C" fn s_file(_v: gpointer, req: gpointer, _id: gpointer) -> gboolean {
    // Dateiauswahl gibt es (noch) nicht – ablehnen statt die Seite hängen zu lassen
    webkit_file_chooser_request_cancel(req);
    TRUE
}
/// Neues Fenster (window.open / target=_blank): als neuer Tab im selben Prozess der Seite.
unsafe extern "C" fn s_create(v: gpointer, _action: gpointer, _id: gpointer) -> gpointer {
    with(|b| b.open_related(v)).unwrap_or(null_mut())
}

/// ultar://… – interne Seiten
unsafe extern "C" fn s_scheme(req: gpointer, _d: gpointer) {
    // ultar://leistung -> "leistung" (der Name steht an der Stelle des Hosts, nicht im Pfad)
    let uri = rs(webkit_uri_scheme_request_get_uri(req));
    let path = uri.trim_start_matches("ultar:").trim_start_matches('/').split(['/', '?', '#']).next().unwrap_or("").to_string();
    let html = with(|b| b.internal_page(&path)).unwrap_or_else(|| "<p>…</p>".into());
    let bytes = g_bytes_new(html.as_ptr() as *const _, html.len());
    let stream = g_memory_input_stream_new_from_bytes(bytes);
    let mime = cs("text/html");
    webkit_uri_scheme_request_finish(req, stream, html.len() as i64, mime.as_ptr());
    g_object_unref(stream);
    g_bytes_unref(bytes);
}

/// Werbe-/Tracking-Filter: kompilierte Regeln laden, sonst einmalig aus dem Bündel kompilieren.
unsafe extern "C" fn s_filter_loaded(store: gpointer, res: gpointer, data: gpointer) {
    let file = data as usize;
    let mut err: *mut GError = null_mut();
    let f = webkit_user_content_filter_store_load_finish(store, res, &mut err);
    if !f.is_null() {
        with(|b| b.filter_ready(f, "geladen"));
        return;
    }
    if !err.is_null() {
        g_error_free(err);
    }
    // noch nicht kompiliert: aus JSON übersetzen (einmalig, danach gespeichert)
    with(|b| {
        let id = cs(&b.filter_id());
        let g = g_file_new_for_path(cs(&format!("{}/share/ultar-web/filters.json", b.o.bundle)).as_ptr());
        webkit_user_content_filter_store_save_from_file(store, id.as_ptr(), g, null_mut(), Some(s_filter_saved), file as gpointer);
        g_object_unref(g);
        b.filters = "wird vorbereitet".into();
    });
}
unsafe extern "C" fn s_filter_saved(store: gpointer, res: gpointer, _d: gpointer) {
    let mut err: *mut GError = null_mut();
    let f = webkit_user_content_filter_store_save_from_file_finish(store, res, &mut err);
    if !f.is_null() {
        with(|b| b.filter_ready(f, "neu übersetzt"));
    } else {
        let m = if err.is_null() { String::new() } else { rs((*err).message) };
        eprintln!("ultar-web: Filter nicht übersetzbar: {m}");
        with(|b| b.filters = format!("Fehler: {m}"));
    }
}

impl Browser {
    #[allow(clippy::too_many_arguments)]
    pub fn new(o: Opts, w: i32, h: i32, touch: bool, keyboard: bool, comp: Sender<Msg>, outs: Receiver<Out>, input: Receiver<Ev>, stats: Arc<Stats>, lp: gpointer) -> Self {
        let k = ui::scale_for(w as f32, h as f32, 1.0);
        let scale = k as f64;
        let display = platform::create_display(w, h, scale, touch, keyboard);
        let th = ui::theme(o.dark, o.accent);
        let topo = crate::sched::topo();
        unsafe {
            // Speicherdruck: WebKit selbst räumt Caches stufenweise; Grenze je Seitenprozess nach Gerät
            let limit_mb = (topo.ram_mb / 3).clamp(256, 2048) as c_uint;
            let mps = webkit_memory_pressure_settings_new();
            webkit_memory_pressure_settings_set_memory_limit(mps, limit_mb);
            webkit_memory_pressure_settings_set_conservative_threshold(mps, 0.33);
            webkit_memory_pressure_settings_set_strict_threshold(mps, 0.5);
            webkit_memory_pressure_settings_set_poll_interval(mps, 10.0);
            webkit_network_session_set_memory_pressure_settings(mps);
            let ctx = g_object_new(webkit_web_context_get_type(), cs("memory-pressure-settings").as_ptr(), mps, std::ptr::null::<c_char>());
            webkit_memory_pressure_settings_free(mps);
            webkit_web_context_set_cache_model(ctx, WEBKIT_CACHE_MODEL_WEB_BROWSER);
            let langs: [&std::ffi::CStr; 3] = match o.lang {
                Lang::Ar => [c"ar", c"en", c"de"],
                Lang::De => [c"de", c"en", c"ar"],
                Lang::En => [c"en", c"de", c"ar"],
            };
            let lp2 = [langs[0].as_ptr(), langs[1].as_ptr(), langs[2].as_ptr(), std::ptr::null()];
            webkit_web_context_set_preferred_languages(ctx, lp2.as_ptr());
            webkit_web_context_register_uri_scheme(ctx, c"ultar".as_ptr(), Some(s_scheme), null_mut(), None);
            let data = cs(&format!("{}/daten", o.data_dir));
            let cache = cs(&format!("{}/cache", o.data_dir));
            let sess = webkit_network_session_new(data.as_ptr(), cache.as_ptr());
            Self::harden_session(sess);
            let ucm = webkit_user_content_manager_new();
            let settings_m = Self::make_settings(false);
            let settings_d = Self::make_settings(true);
            let mut b = Browser {
                w,
                h,
                k,
                scale,
                th,
                display,
                ctx,
                sess,
                psess: null_mut(),
                ucm,
                settings_m,
                settings_d,
                tabs: Vec::new(),
                cur: 0,
                next_id: 1,
                ov: Overlay::None,
                p: Ptr::default(),
                tx: Text::new(),
                cv: Canvas::new(w, h),
                comp,
                outs,
                input,
                kb: if o.lang == Lang::Ar { 3 } else { 0 },
                im: 0,
                osk: false,
                full: false,
                cap: Capture::None,
                down: (0.0, 0.0),
                dirty: true,
                content: IRect::default(),
                history: Vec::new(),
                started: Instant::now(),
                stats,
                filters: "lädt".into(),
                saver: false,
                cpu_prev: Vec::new(),
                cpu_load: Vec::new(),
                proc_prev: Vec::new(),
                procs: Vec::new(),
                lp,
                shot_due: None,
                exit_code: 0,
                tscroll: 0.0,
                o,
            };
            b.load_history();
            b.load_filters();
            b
        }
    }

    unsafe fn harden_session(sess: gpointer) {
        webkit_network_session_set_itp_enabled(sess, TRUE);
        webkit_network_session_set_tls_errors_policy(sess, WEBKIT_TLS_ERRORS_POLICY_FAIL);
        let cm = webkit_network_session_get_cookie_manager(sess);
        webkit_cookie_manager_set_accept_policy(cm, WEBKIT_COOKIE_POLICY_ACCEPT_NO_THIRD_PARTY);
    }

    unsafe fn make_settings(desktop: bool) -> gpointer {
        let s = webkit_settings_new();
        // Kein Grafikchip: Skia zeichnet auf der CPU in Shared Memory (alle Kerne, Kacheln parallel).
        // Abgeschaltet wird die Beschleunigung über WEBKIT_DISABLE_HARDWARE_ACCELERATION (main.rs, Patch 0002);
        // zusätzlich das Erzwingen von Compositing, falls die Fassung den Schalter anbietet.
        let _ = set_feature(s, "ForceCompositingMode", false);
        webkit_settings_set_enable_webgl(s, FALSE);
        webkit_settings_set_enable_smooth_scrolling(s, FALSE);
        webkit_settings_set_enable_page_cache(s, TRUE);
        webkit_settings_set_media_playback_requires_user_gesture(s, TRUE);
        webkit_settings_set_javascript_can_open_windows_automatically(s, FALSE);
        webkit_settings_set_default_font_family(s, c"Noto Sans".as_ptr());
        webkit_settings_set_sans_serif_font_family(s, c"Noto Sans".as_ptr());
        let ua = cs(if desktop { DESKTOP_UA } else { MOBILE_UA });
        webkit_settings_set_user_agent(s, ua.as_ptr());
        if std::env::var("ULTAR_WEB_CONSOLE").is_ok() {
            webkit_settings_set_enable_write_console_messages_to_stdout(s, TRUE);
        }
        s
    }

    fn filter_id(&self) -> String {
        let v = std::fs::read_to_string(format!("{}/share/ultar-web/filters.version", self.o.bundle)).unwrap_or_else(|_| "0".into());
        format!("ultar-schutz-{}", v.trim())
    }

    fn load_filters(&mut self) {
        unsafe {
            let dir = cs(&format!("{}/filter", self.o.data_dir));
            let store = webkit_user_content_filter_store_new(dir.as_ptr());
            let id = cs(&self.filter_id());
            webkit_user_content_filter_store_load(store, id.as_ptr(), null_mut(), Some(s_filter_loaded), null_mut());
        }
    }

    fn filter_ready(&mut self, f: gpointer, how: &str) {
        unsafe { webkit_user_content_manager_add_filter(self.ucm, f) };
        let n = std::fs::read_to_string(format!("{}/share/ultar-web/filters.count", self.o.bundle)).unwrap_or_default();
        self.filters = format!("{} Regeln aktiv ({how})", n.trim());
        self.dirty = true;
    }

    fn private_session(&mut self) -> gpointer {
        if self.psess.is_null() {
            unsafe {
                self.psess = webkit_network_session_new_ephemeral();
                Self::harden_session(self.psess);
            }
        }
        self.psess
    }

    fn connect_view(&self, v: gpointer, id: u64) {
        let d = id as gpointer;
        unsafe {
            for s in ["notify::title", "notify::uri", "notify::estimated-load-progress", "notify::is-loading", "notify::is-playing-audio"] {
                connect(v, s, s_notify as *const (), d);
            }
            connect(v, "load-changed", s_load as *const (), d);
            connect(v, "close", s_close as *const (), d);
            connect(v, "web-process-terminated", s_crash as *const (), d);
            connect(v, "script-dialog", s_dialog as *const (), d);
            connect(v, "show-option-menu", s_options as *const (), d);
            connect(v, "permission-request", s_perm as *const (), d);
            connect(v, "run-file-chooser", s_file as *const (), d);
            connect(v, "create", s_create as *const (), d);
        }
    }

    fn new_view(&mut self, private: bool, desktop: bool, related: gpointer) -> gpointer {
        let sess = if private { self.private_session() } else { self.sess };
        let st = if desktop { self.settings_d } else { self.settings_m };
        unsafe {
            if related.is_null() {
                g_object_new(
                    webkit_web_view_get_type(),
                    c"display".as_ptr(),
                    self.display,
                    c"web-context".as_ptr(),
                    self.ctx,
                    c"network-session".as_ptr(),
                    sess,
                    c"settings".as_ptr(),
                    st,
                    c"user-content-manager".as_ptr(),
                    self.ucm,
                    std::ptr::null::<c_char>(),
                )
            } else {
                g_object_new(
                    webkit_web_view_get_type(),
                    c"display".as_ptr(),
                    self.display,
                    c"related-view".as_ptr(),
                    related,
                    c"settings".as_ptr(),
                    st,
                    c"user-content-manager".as_ptr(),
                    self.ucm,
                    std::ptr::null::<c_char>(),
                )
            }
        }
    }

    fn add_tab(&mut self, view: gpointer, private: bool, desktop: bool) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.connect_view(view, id);
        let eng = WpeTab::new(view);
        self.tabs.push(Tab {
            id,
            eng: Some(eng),
            title: String::new(),
            uri: String::new(),
            progress: 0.0,
            loading: false,
            secure: false,
            private,
            desktop,
            state: State::Hidden,
            hidden_at: Some(Instant::now()),
            used: Instant::now(),
            thumb: None,
            session: null_mut(),
            crashed: false,
        });
        self.tabs.len() - 1
    }

    pub fn open(&mut self, url: &str, private: bool, desktop: bool) {
        let v = self.new_view(private, desktop, null_mut());
        let i = self.add_tab(v, private, desktop);
        self.switch(i);
        if let Some(e) = &self.tabs[i].eng {
            e.load(url);
        }
        self.tabs[i].uri = url.to_string();
    }

    fn open_related(&mut self, parent: gpointer) -> gpointer {
        let (private, desktop) = self.tabs.iter().find(|t| t.eng.as_ref().is_some_and(|e| e.view == parent)).map(|t| (t.private, t.desktop)).unwrap_or((false, false));
        let v = self.new_view(private, desktop, parent);
        let i = self.add_tab(v, private, desktop);
        self.switch(i);
        v
    }

    // ---------------- Tabs und Lebenszyklus ----------------
    fn tb(&self) -> i32 {
        if self.full { 0 } else { (56.0 * self.k).round() as i32 }
    }
    fn bb(&self) -> i32 {
        if self.full || self.osk { 0 } else { (54.0 * self.k).round() as i32 }
    }
    fn kh(&self) -> i32 {
        if self.osk { (236.0 * self.k).round() as i32 } else { 0 }
    }

    /// Inhaltsbereich neu berechnen und der aktiven Seite die Größe mitteilen (wie Chrome: Tastatur verkleinert die Seite).
    fn layout(&mut self) {
        let top = self.tb();
        let r = IRect { x: 0, y: top, w: self.w, h: (self.h - top - self.bb() - self.kh()).max(1) };
        if r != self.content {
            self.content = r;
            self.resize_active();
            self.dirty = true;
        }
    }

    fn css(&self) -> (i32, i32) {
        ((self.content.w as f64 / self.scale).round() as i32, (self.content.h as f64 / self.scale).round() as i32)
    }

    fn resize_active(&self) {
        let (cw, ch) = self.css();
        if let Some(e) = self.tabs.get(self.cur).and_then(|t| t.eng.as_ref()) {
            e.resize(cw, ch, self.scale);
        }
    }

    fn switch(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        if i != self.cur || self.tabs[i].state != State::Active {
            // Vorschaubild des bisherigen Tabs festhalten, dann verbergen
            if let Some(old) = self.tabs.get_mut(self.cur) {
                if old.state == State::Active {
                    let _ = self.comp.send(Msg::Thumb(old.id));
                    if let Some(e) = &old.eng {
                        e.set_visible(false);
                    }
                    old.state = State::Hidden;
                    old.hidden_at = Some(Instant::now());
                }
            }
        }
        self.cur = i;
        if self.tabs[i].state == State::Discarded {
            self.restore(i);
        }
        let t = &mut self.tabs[i];
        t.state = State::Active;
        t.hidden_at = None;
        t.used = Instant::now();
        self.im = 0;
        self.osk = false;
        self.content = IRect::default();
        self.layout();
        if let Some(e) = &self.tabs[i].eng {
            e.set_visible(true);
        }
        self.dirty = true;
    }

    /// Tab verwerfen: Prozess und Speicher frei, Verlauf + Vorschaubild bleiben (wie Tab Discarding bei Chrome).
    fn discard(&mut self, i: usize) {
        let t = &mut self.tabs[i];
        if let Some(e) = t.eng.take() {
            unsafe {
                t.session = webkit_web_view_get_session_state(e.view);
                g_object_unref(e.view);
            }
        }
        t.state = State::Discarded;
        eprintln!("ultar-web: Tab {} verworfen (Speicher knapp)", t.id);
    }

    fn restore(&mut self, i: usize) {
        let (private, desktop) = (self.tabs[i].private, self.tabs[i].desktop);
        let v = self.new_view(private, desktop, null_mut());
        let id = self.tabs[i].id;
        self.connect_view(v, id);
        let e = WpeTab::new(v);
        unsafe {
            let s = std::mem::replace(&mut self.tabs[i].session, null_mut());
            if !s.is_null() {
                webkit_web_view_restore_session_state(v, s);
                webkit_web_view_session_state_unref(s);
                let item = webkit_back_forward_list_get_current_item(webkit_web_view_get_back_forward_list(v));
                if !item.is_null() {
                    webkit_web_view_go_to_back_forward_list_item(v, item);
                } else {
                    e.load(&self.tabs[i].uri);
                }
            } else {
                e.load(&self.tabs[i].uri);
            }
        }
        self.tabs[i].eng = Some(e);
        self.tabs[i].crashed = false;
    }

    fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let t = self.tabs.remove(i);
        if let Some(e) = t.eng {
            unsafe { g_object_unref(e.view) };
        }
        if !t.session.is_null() {
            unsafe { webkit_web_view_session_state_unref(t.session) };
        }
        if self.tabs.is_empty() {
            if self.o.app {
                self.exit(0);
                return;
            }
            self.cur = 0;
            self.open(HOME, false, self.o.desktop);
            return;
        }
        if self.cur >= self.tabs.len() || self.cur == i {
            let n = self.cur.min(self.tabs.len() - 1);
            self.cur = usize::MAX;
            self.switch(n);
        } else if self.cur > i {
            self.cur -= 1;
        }
        self.dirty = true;
    }

    /// Alle 10 s: verborgene Tabs nach 5 min einfrieren; bei Speichernot den am längsten ungenutzten verwerfen;
    /// Netzwerkprozess auf sparsame Kerne; Energiesparen bei Akku < 15 % oder Hitze.
    pub fn tick(&mut self) {
        let saver = crate::sched::power_saver();
        // Energiesparen: kein Vorauflösen/Vorladen (prefetch() prüft self.saver), Seitenprozesse nur auf sparsamen Kernen
        self.saver = saver;
        for (pid, _) in crate::sched::place_children(saver) {
            let _ = pid;
        }
        for t in self.tabs.iter_mut() {
            if t.state == State::Hidden && t.hidden_at.is_some_and(|h| h.elapsed() > Duration::from_secs(300)) && !t.eng.as_ref().is_some_and(|e| e.playing_audio()) {
                t.state = State::Frozen;
            }
        }
        let free = crate::sched::mem_free_ratio();
        if free < 0.12 {
            let victim = self.tabs.iter().enumerate().filter(|(i, t)| *i != self.cur && t.eng.is_some() && t.state != State::Active).min_by_key(|(_, t)| t.used).map(|(i, _)| i);
            if let Some(v) = victim {
                self.discard(v);
            }
        }
        self.sample_usage();
        self.dirty = true;
        self.redraw_if_dirty();
    }

    fn sample_usage(&mut self) {
        let now = crate::sched::cpu_times();
        self.cpu_load = now
            .iter()
            .zip(self.cpu_prev.iter())
            .map(|(a, b)| {
                let (dt, di) = (a.0.saturating_sub(b.0), a.1.saturating_sub(b.1));
                if dt == 0 { 0.0 } else { 1.0 - di as f32 / dt as f32 }
            })
            .collect();
        self.cpu_prev = now;
        let mut list = vec![(std::process::id() as i32, "ultar-web".to_string())];
        list.extend(crate::sched::place_children(self.saver));
        let mut prev = Vec::new();
        self.procs = list
            .into_iter()
            .map(|(pid, name)| {
                let (rss, cpu) = crate::sched::proc_usage(pid);
                let old = self.proc_prev.iter().find(|x| x.0 == pid).map(|x| x.1).unwrap_or(cpu);
                prev.push((pid, cpu));
                (pid, name, rss, (cpu - old.min(cpu)) as f32 / 10.0 / 1.0)
            })
            .collect();
        self.proc_prev = prev;
    }

    fn exit(&mut self, code: i32) {
        self.exit_code = code;
        self.save_session();
        let _ = self.comp.send(Msg::Quit);
        unsafe { g_main_loop_quit(self.lp) };
    }

    /// Sauber beenden (SIGTERM): Sitzung sichern, Compositor stoppen
    pub fn quit(&mut self) {
        self.exit(0);
    }

    /// Bildschirmfoto im laufenden Betrieb (SIGUSR2) – liegt danach im Profilordner als bildschirm.bmp
    pub fn screenshot(&self) {
        let _ = self.comp.send(Msg::Shot(format!("{}/bildschirm.bmp", self.o.data_dir)));
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    // ---------------- Verlauf, Sitzung ----------------
    fn load_history(&mut self) {
        let s = std::fs::read_to_string(format!("{}/verlauf.tsv", self.o.data_dir)).unwrap_or_default();
        self.history = s.lines().filter_map(|l| l.split_once('\t')).map(|(u, t)| (u.to_string(), t.to_string())).collect();
    }

    fn remember(&mut self, uri: &str, title: &str) {
        if uri.starts_with("ultar:") || uri.is_empty() {
            return;
        }
        self.history.retain(|x| x.0 != uri);
        self.history.push((uri.to_string(), title.replace(['\t', '\n'], " ")));
        if self.history.len() > 2000 {
            self.history.drain(..self.history.len() - 2000);
        }
        let s: String = self.history.iter().map(|(u, t)| format!("{u}\t{t}\n")).collect();
        let _ = std::fs::write(format!("{}/verlauf.tsv", self.o.data_dir), s);
    }

    fn save_session(&self) {
        let s: String = self.tabs.iter().filter(|t| !t.private && !t.uri.is_empty()).map(|t| format!("{}\n", t.uri)).collect();
        let _ = std::fs::write(format!("{}/sitzung.txt", self.o.data_dir), s);
    }

    pub fn start(&mut self) {
        // erste Messung, damit ultar://leistung sofort Werte je Kern zeigt
        self.cpu_prev = crate::sched::cpu_times();
        let url = self.o.url.clone();
        let saved = std::fs::read_to_string(format!("{}/sitzung.txt", self.o.data_dir)).unwrap_or_default();
        let restore: Vec<String> = if url.is_empty() && !self.o.app { saved.lines().filter(|l| !l.is_empty()).map(String::from).collect() } else { Vec::new() };
        if restore.is_empty() {
            self.open(if url.is_empty() { HOME } else { &url }, false, self.o.desktop);
        } else {
            // Nur der aktive Tab lädt; die übrigen bleiben verworfen, bis sie geöffnet werden
            let last = restore.len() - 1;
            for (i, u) in restore.iter().enumerate() {
                if i == last {
                    self.open(u, false, self.o.desktop);
                } else {
                    let id = self.next_id;
                    self.next_id += 1;
                    self.tabs.push(Tab { id, eng: None, title: u.clone(), uri: u.clone(), progress: 0.0, loading: false, secure: false, private: false, desktop: self.o.desktop, state: State::Discarded, hidden_at: None, used: Instant::now(), thumb: None, session: null_mut(), crashed: false });
                }
            }
            self.cur = usize::MAX;
            let n = self.tabs.len() - 1;
            self.switch(n);
        }
        if self.o.shot.is_some() {
            self.shot_due = Some(Instant::now() + Duration::from_secs(self.o.shot_wait));
        }
    }

    // ---------------- Ereignisse ----------------
    fn idx(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn pump(&mut self) {
        for ev in platform::take_events() {
            self.plat(ev);
        }
        let qs = QUEUE.with(|x| std::mem::take(&mut *x.borrow_mut()));
        for ev in qs {
            self.webkit(ev);
        }
        while let Ok(o) = self.outs.try_recv() {
            match o {
                Out::Done { view, buffer } => platform::frame_done(view, buffer),
                Out::Thumb { tag, w, h, px } => {
                    if let Some(i) = self.idx(tag) {
                        self.tabs[i].thumb = Some((w, h, px));
                    }
                }
                Out::Shot(p) => {
                    println!("ultar-web: Bildschirmfoto {p}");
                    if self.o.shot.is_some() {
                        self.exit(0);
                        return;
                    }
                }
            }
        }
        let evs: Vec<Ev> = self.input.try_iter().collect();
        for e in evs {
            self.input_ev(e);
        }
        if let Some(due) = self.shot_due {
            let busy = self.tabs.get(self.cur).is_some_and(|t| t.loading);
            if Instant::now() >= due && (!busy || Instant::now() >= due + Duration::from_secs(20)) {
                self.shot_due = None;
                let _ = self.comp.send(Msg::Shot(self.o.shot.clone().unwrap_or_default()));
            }
        }
        self.redraw_if_dirty();
    }

    fn plat(&mut self, ev: PlatEv) {
        let active_wpe = self.tabs.get(self.cur).and_then(|t| t.eng.as_ref()).map(|e| e.wpe as usize).unwrap_or(0);
        match ev {
            PlatEv::ImFocusIn { ctx, view, osk, .. } => {
                if view == active_wpe {
                    self.im = ctx;
                    self.osk = osk;
                    self.layout();
                }
            }
            PlatEv::ImFocusOut { ctx } => {
                if ctx == self.im {
                    self.im = 0;
                    if !matches!(self.ov, Overlay::Url { .. } | Overlay::Dialog { .. }) {
                        self.osk = false;
                    }
                    self.layout();
                }
            }
            PlatEv::Fullscreen { on, .. } => {
                self.full = on;
                self.layout();
            }
        }
        self.dirty = true;
    }

    fn webkit(&mut self, ev: Q) {
        match ev {
            Q::Changed(id) | Q::Load(id, _) => {
                let Some(i) = self.idx(id) else { return };
                let t = &mut self.tabs[i];
                if let Some(e) = &t.eng {
                    t.title = e.title();
                    t.uri = e.uri();
                    t.progress = e.progress();
                    t.loading = e.loading();
                    t.secure = e.secure();
                }
                if let Q::Load(_, k) = ev {
                    if k == WEBKIT_LOAD_FINISHED && !self.tabs[i].private {
                        let (u, tt) = (self.tabs[i].uri.clone(), self.tabs[i].title.clone());
                        self.remember(&u, &tt);
                    }
                }
            }
            Q::Close(id) => {
                if let Some(i) = self.idx(id) {
                    self.close_tab(i);
                }
            }
            Q::Crashed(id) => {
                if let Some(i) = self.idx(id) {
                    self.tabs[i].crashed = true;
                    eprintln!("ultar-web: Seitenprozess von Tab {id} beendet");
                }
            }
            Q::Dialog(_, d) => unsafe {
                let kind = webkit_script_dialog_get_dialog_type(d);
                let msg = rs(webkit_script_dialog_get_message(d));
                let text = if kind == WEBKIT_SCRIPT_DIALOG_PROMPT { rs(webkit_script_dialog_prompt_get_default_text(d)) } else { String::new() };
                self.close_overlay();
                self.osk = kind == WEBKIT_SCRIPT_DIALOG_PROMPT;
                self.ov = Overlay::Dialog { d, kind, msg, text };
                self.layout();
            },
            Q::Options(_, m) => unsafe {
                let n = webkit_option_menu_get_n_items(m);
                let items = (0..n)
                    .map(|i| {
                        let it = webkit_option_menu_get_item(m, i);
                        (rs(webkit_option_menu_item_get_label(it)), webkit_option_menu_item_is_enabled(it) != 0, webkit_option_menu_item_is_selected(it) != 0, webkit_option_menu_item_is_group_label(it) != 0)
                    })
                    .collect();
                self.close_overlay();
                self.ov = Overlay::Options { m, items };
            },
            Q::Perm(id, req) => unsafe {
                let ty = rs(g_type_name_from_instance(req));
                let host = self.idx(id).map(|i| host_of(&self.tabs[i].uri)).unwrap_or_default();
                let (cap, what) = match ty.as_str() {
                    "WebKitGeolocationPermissionRequest" => ("loc", t(self.o.lang, "Standort")),
                    "WebKitUserMediaPermissionRequest" => ("cam", t(self.o.lang, "Kamera/Mikrofon")),
                    "WebKitNotificationPermissionRequest" => ("", t(self.o.lang, "Benachrichtigungen")),
                    "WebKitClipboardPermissionRequest" => ("", t(self.o.lang, "Zwischenablage")),
                    "WebKitPointerLockPermissionRequest" => {
                        webkit_permission_request_allow(req);
                        g_object_unref(req);
                        return;
                    }
                    _ => {
                        // Unbekannt oder nicht vorgesehen (z. B. Drittanbieter-Speicher, DRM): ablehnen
                        webkit_permission_request_deny(req);
                        g_object_unref(req);
                        return;
                    }
                };
                if !cap.is_empty() && !privacy_allows(cap) {
                    webkit_permission_request_deny(req);
                    g_object_unref(req);
                    return;
                }
                self.close_overlay();
                self.ov = Overlay::Perm { req, what: what.to_string(), host };
            },
        }
        self.dirty = true;
    }

    /// Offenes Overlay schließen; wartende WebKit-Anfragen werden dabei beantwortet (abgelehnt/abgebrochen).
    fn close_overlay(&mut self) {
        match std::mem::replace(&mut self.ov, Overlay::None) {
            Overlay::Dialog { d, .. } => unsafe {
                webkit_script_dialog_close(d);
                webkit_script_dialog_unref(d);
            },
            Overlay::Options { m, .. } => unsafe {
                webkit_option_menu_close(m);
                g_object_unref(m);
            },
            Overlay::Perm { req, .. } => unsafe {
                webkit_permission_request_deny(req);
                g_object_unref(req);
            },
            Overlay::Url { .. } => {}
            _ => {}
        }
        if self.im == 0 {
            self.osk = false;
        }
        self.layout();
        self.dirty = true;
    }

    fn eng(&self) -> Option<&WpeTab> {
        self.tabs.get(self.cur).and_then(|t| t.eng.as_ref())
    }

    fn chrome_hit(&self, x: f32, y: f32) -> bool {
        let c = self.content;
        !(x >= c.x as f32 && x < (c.x + c.w) as f32 && y >= c.y as f32 && y < (c.y + c.h) as f32)
    }

    fn modal(&self) -> bool {
        !matches!(self.ov, Overlay::None)
    }

    fn input_ev(&mut self, e: Ev) {
        let (w, h) = (self.w as f32, self.h as f32);
        let ms = now_ms();
        match e {
            Ev::Down(nx, ny) => {
                let (x, y) = (nx * w, ny * h);
                self.down = (x, y);
                self.p.x = x;
                self.p.y = y;
                let edge = 14.0 * self.k;
                self.cap = if self.modal() || self.chrome_hit(x, y) {
                    Capture::Chrome
                } else if x < edge || x > w - edge {
                    Capture::Edge(x)
                } else {
                    Capture::Content
                };
                match self.cap {
                    Capture::Chrome => {
                        self.p.down = true;
                        self.p.pressed = true;
                        self.p.x = x;
                        self.p.y = y;
                        self.p.px = x;
                        self.p.py = y;
                        self.p.moved = false;
                        self.dirty = true;
                    }
                    Capture::Content => {
                        let (cx, cy) = self.to_css(x, y);
                        if let Some(en) = self.eng() {
                            en.touch(0, cx, cy, ms);
                        }
                    }
                    _ => {}
                }
            }
            Ev::Move(nx, ny) => {
                let (x, y) = (nx * w, ny * h);
                match self.cap {
                    Capture::Chrome => {
                        self.p.dx += x - self.p.x;
                        self.p.dy += y - self.p.y;
                        self.p.x = x;
                        self.p.y = y;
                        if (x - self.p.px).abs() + (y - self.p.py).abs() > 10.0 * self.k {
                            self.p.moved = true;
                        }
                        self.dirty = true;
                    }
                    Capture::Content => {
                        let (cx, cy) = self.to_css(x, y);
                        if let Some(en) = self.eng() {
                            en.touch(1, cx, cy, ms);
                        }
                    }
                    _ => {}
                }
                self.p.x = x;
                self.p.y = y;
            }
            Ev::Up => {
                match self.cap {
                    Capture::Chrome => {
                        self.p.down = false;
                        self.p.released = true;
                        self.dirty = true;
                    }
                    Capture::Content => {
                        let (cx, cy) = self.to_css(self.p.x, self.p.y);
                        if let Some(en) = self.eng() {
                            en.touch(2, cx, cy, ms);
                        }
                    }
                    Capture::Edge(x0) => {
                        // Wisch vom Rand zur Mitte = zurück (wie im System)
                        if (self.p.x - x0).abs() > 60.0 * self.k {
                            self.act(Act::Back);
                        }
                    }
                    Capture::None => {}
                }
                self.cap = Capture::None;
            }
            Ev::Char(c) => self.typed(Key::Char(c)),
            Ev::Backspace => self.typed(Key::Backspace),
            Ev::Enter => self.typed(Key::Enter),
            Ev::Back => self.act(Act::Back),
            Ev::Power => self.act(Act::Exit),
            Ev::VolUp | Ev::VolDown => {}
        }
    }

    fn to_css(&self, x: f32, y: f32) -> (f64, f64) {
        (((x - self.content.x as f32) as f64 / self.scale).max(0.0), ((y - self.content.y as f32) as f64 / self.scale).max(0.0))
    }

    /// Tastendruck (Bildschirm- oder echte Tastatur) an das richtige Ziel
    fn typed(&mut self, k: Key) {
        match &mut self.ov {
            Overlay::Url { text, .. } => {
                match k {
                    Key::Char(c) => text.push(c),
                    Key::Backspace => {
                        text.pop();
                    }
                    Key::Enter => {
                        let s = text.clone();
                        self.act(Act::Submit(s));
                        return;
                    }
                }
                let s = text.clone();
                self.suggest(&s);
                self.prefetch(&s);
            }
            Overlay::Dialog { text, kind, .. } if *kind == WEBKIT_SCRIPT_DIALOG_PROMPT => match k {
                Key::Char(c) => text.push(c),
                Key::Backspace => {
                    text.pop();
                }
                Key::Enter => self.act(Act::Dialog(true)),
            },
            _ => {
                let ms = now_ms();
                match k {
                    Key::Char(c) if self.im != 0 => platform::im_commit(self.im, &c.to_string()),
                    Key::Char(c) => {
                        if let Some(e) = self.eng() {
                            e.key(keysym_of(c), 0, ms)
                        }
                    }
                    Key::Backspace => {
                        if let Some(e) = self.eng() {
                            e.key(0xff08, 22, ms)
                        }
                    }
                    Key::Enter => {
                        if let Some(e) = self.eng() {
                            e.key(0xff0d, 36, ms)
                        }
                    }
                }
            }
        }
        self.dirty = true;
    }

    /// Vorverbindung wie bei Chrome: schon beim Tippen die Adresse auflösen (nicht bei Energiesparen/mobilen Daten)
    fn prefetch(&self, s: &str) {
        if self.saver || crate::sched::metered() {
            return;
        }
        if let Some(h) = as_url(s).map(|u| host_of(&u)).filter(|h| h.contains('.')) {
            let c = cs(&h);
            unsafe { webkit_network_session_prefetch_dns(self.sess, c.as_ptr()) };
        }
    }

    fn suggest(&mut self, s: &str) {
        let l = s.to_lowercase();
        let mut out: Vec<(String, String)> = Vec::new();
        if !l.is_empty() {
            for (u, t) in self.history.iter().rev() {
                if out.len() >= 6 {
                    break;
                }
                if u.to_lowercase().contains(&l) || t.to_lowercase().contains(&l) {
                    out.push((u.clone(), t.clone()));
                }
            }
        }
        if let Overlay::Url { sugg, .. } = &mut self.ov {
            *sugg = out;
        }
    }

    fn act(&mut self, a: Act) {
        match a {
            Act::Back => {
                if self.modal() {
                    self.close_overlay();
                } else if self.osk {
                    self.osk = false;
                    self.layout();
                } else if self.eng().is_some_and(|e| e.can_back()) {
                    self.eng().unwrap().back();
                } else if self.o.app || self.tabs.len() <= 1 {
                    self.exit(0);
                } else {
                    self.close_tab(self.cur);
                }
            }
            Act::Forward => {
                if let Some(e) = self.eng() {
                    e.forward()
                }
            }
            Act::Reload => {
                let crashed = self.tabs.get(self.cur).is_some_and(|t| t.crashed);
                if let Some(e) = self.eng() {
                    e.reload()
                }
                if crashed {
                    self.tabs[self.cur].crashed = false;
                }
            }
            Act::Stop => {
                if let Some(e) = self.eng() {
                    e.stop()
                }
            }
            Act::Home => {
                if let Some(e) = self.eng() {
                    e.load(HOME)
                }
            }
            Act::EditUrl => {
                let u = self.tabs.get(self.cur).map(|t| t.uri.clone()).unwrap_or_default();
                let u = if u.starts_with("ultar:") { String::new() } else { u };
                self.ov = Overlay::Url { text: u, sugg: Vec::new() };
                self.osk = true;
                self.layout();
            }
            Act::Submit(s) => {
                let s = s.trim().to_string();
                self.ov = Overlay::None;
                self.osk = false;
                self.layout();
                if !s.is_empty() {
                    let url = as_url(&s).unwrap_or_else(|| format!("{SEARCH}{}", urlencode(&s)));
                    if let Some(e) = self.eng() {
                        e.load(&url);
                    }
                }
            }
            Act::Menu => self.ov = Overlay::Menu,
            Act::Tabs => {
                if let Some(t) = self.tabs.get(self.cur) {
                    let _ = self.comp.send(Msg::Thumb(t.id));
                }
                self.ov = Overlay::Tabs;
            }
            Act::NewTab(private) => {
                self.ov = Overlay::None;
                self.open(HOME, private, self.o.desktop);
            }
            Act::Switch(i) => {
                self.ov = Overlay::None;
                self.switch(i);
            }
            Act::CloseTab(i) => self.close_tab(i),
            Act::Desktop => {
                self.ov = Overlay::None;
                if let Some(t) = self.tabs.get_mut(self.cur) {
                    t.desktop = !t.desktop;
                    let (d, u) = (t.desktop, t.uri.clone());
                    if let Some(e) = &t.eng {
                        unsafe { webkit_web_view_set_settings(e.view, if d { self.settings_d } else { self.settings_m }) };
                        e.load(&u);
                    }
                }
            }
            Act::Perf => {
                self.ov = Overlay::None;
                self.open("ultar://leistung", false, false);
            }
            Act::Exit => self.exit(0),
            Act::Close => self.close_overlay(),
            Act::Dialog(ok) => {
                if let Overlay::Dialog { d, kind, text, .. } = &self.ov {
                    unsafe {
                        if *kind == WEBKIT_SCRIPT_DIALOG_CONFIRM || *kind == WEBKIT_SCRIPT_DIALOG_BEFORE_UNLOAD_CONFIRM {
                            webkit_script_dialog_confirm_set_confirmed(*d, ok as gboolean);
                        }
                        if *kind == WEBKIT_SCRIPT_DIALOG_PROMPT && ok {
                            let c = cs(text);
                            webkit_script_dialog_prompt_set_text(*d, c.as_ptr());
                        }
                    }
                }
                self.close_overlay();
            }
            Act::Option(i) => {
                if let Overlay::Options { m, .. } = std::mem::replace(&mut self.ov, Overlay::None) {
                    unsafe {
                        webkit_option_menu_activate_item(m, i);
                        webkit_option_menu_close(m);
                        g_object_unref(m);
                    }
                }
            }
            Act::Perm(ok) => {
                if let Overlay::Perm { req, .. } = std::mem::replace(&mut self.ov, Overlay::None) {
                    unsafe {
                        if ok {
                            webkit_permission_request_allow(req)
                        } else {
                            webkit_permission_request_deny(req)
                        }
                        g_object_unref(req);
                    }
                }
            }
            Act::Osk(k) => self.typed(k),
            Act::UrlClear => {
                if let Overlay::Url { text, sugg } = &mut self.ov {
                    text.clear();
                    sugg.clear();
                }
            }
        }
        self.dirty = true;
    }

    // ---------------- Zeichnen der Leisten und Overlays ----------------
    fn redraw_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        self.layout();
        let mut acts: Vec<Act> = Vec::new();
        let rects;
        let again;
        {
            let lang = self.o.lang;
            let app = self.o.app;
            let filters = self.filters.clone();
            let Browser { cv, tx, p, th, k, w, h, tabs, cur, ov, osk, full, kb, content, tscroll, .. } = self;
            let mut ui = Ui { cv, tx, p, th: *th, k: *k, w: *w as f32, h: *h as f32, rtl: false, enabled: true, focus: None, dirty: false, clip: None, links: Vec::new() };
            let d = Draw { lang, app, filters: &filters, tabs, cur: *cur, osk: *osk, full: *full, content: *content };
            rects = d.draw(&mut ui, ov, kb, tscroll, &mut acts);
            again = ui.dirty;
        }
        self.p.pressed = false;
        self.p.released = false;
        self.p.dx = 0.0;
        self.p.dy = 0.0;
        let pieces: Vec<(IRect, Vec<u32>)> = rects
            .iter()
            .map(|r| {
                let mut v = Vec::with_capacity((r.w * r.h).max(0) as usize);
                for y in r.y.max(0)..(r.y + r.h).min(self.h) {
                    let a = (y * self.w + r.x.max(0)) as usize;
                    let b = (y * self.w + (r.x + r.w).min(self.w)) as usize;
                    v.extend_from_slice(&self.cv.px[a..b]);
                }
                (*r, v)
            })
            .collect();
        let _ = self.comp.send(Msg::Chrome { pieces, content: self.content });
        for a in acts {
            self.act(a);
        }
        if again || self.dirty {
            self.dirty = true;
            schedule();
        }
    }

    // ---------------- Interne Seiten ----------------
    fn internal_page(&mut self, path: &str) -> String {
        let l = self.o.lang;
        let dir = if l == Lang::Ar { "rtl" } else { "ltr" };
        let head = format!(
            "<!doctype html><html dir='{dir}' lang='{}'><head><meta charset='utf-8'><title>{}</title><meta name='viewport' content='width=device-width,initial-scale=1'>\
             <style>body{{margin:0;font-family:'Noto Sans',sans-serif;background:#0B0D12;color:#F4F6F9}}main{{padding:20px;max-width:720px;margin:auto}}\
             h1{{font-size:24px;margin:28px 0 6px;color:#C9A227}}p,td{{color:#8A93A3;font-size:14px}}a{{color:#F4F6F9;text-decoration:none}}\
             form{{display:flex;margin:18px 0}}input{{flex:1;padding:14px 16px;border-radius:24px;border:1px solid #242A36;background:#161922;color:#F4F6F9;font-size:16px}}\
             .g{{display:grid;grid-template-columns:repeat(3,1fr);gap:12px;margin-top:18px}}.t{{background:#161922;border:1px solid #242A36;border-radius:16px;padding:16px 8px;text-align:center;font-size:14px}}\
             .t b{{display:block;font-size:22px;color:#C9A227;margin-bottom:6px}}table{{width:100%;border-collapse:collapse}}td,th{{padding:6px 4px;border-bottom:1px solid #242A36;text-align:start;font-size:13px}}\
             th{{color:#C9A227;font-weight:600}}.bar{{height:8px;background:#242A36;border-radius:4px}}.bar i{{display:block;height:8px;background:#C9A227;border-radius:4px}}</style></head><body><main>",
            l.code(),
            if path == "leistung" { t(l, "Leistung") } else { "Ultar Web" }
        );
        let body = match path {
            "leistung" => self.perf_html(),
            _ => {
                let tiles = [("WhatsApp", "https://web.whatsapp.com/", "W"), ("YouTube", "https://m.youtube.com/", "▶"), ("Telegram", "https://web.telegram.org/", "T"), ("Wikipedia", "https://www.wikipedia.org/", "W"), ("KM Code Craft", "https://code-craft.tech/", "KM"), (t(l, "Leistung"), "ultar://leistung", "⚡")];
                let tl: String = tiles.iter().map(|(n, u, i)| format!("<a class='t' href='{u}'><b>{i}</b>{n}</a>")).collect();
                format!(
                    "<h1>Ultar Web</h1><p>{}</p><form action='https://duckduckgo.com/' method='get'><input name='q' placeholder='{}' autocomplete='off'></form><div class='g'>{tl}</div><p style='margin-top:24px'>{} · {}</p>",
                    t(l, "Schnell, privat, ohne Werbung."),
                    t(l, "Suchen oder Adresse eingeben"),
                    t(l, "Werbe- und Trackingschutz"),
                    self.filters
                )
            }
        };
        format!("{head}{body}</main></body></html>")
    }

    fn perf_html(&mut self) -> String {
        self.sample_usage();
        let l = self.o.lang;
        let topo = crate::sched::topo();
        let st = &self.stats;
        let frames = st.frames.load(std::sync::atomic::Ordering::Relaxed);
        let avg = if frames > 0 { st.sum_us.load(std::sync::atomic::Ordering::Relaxed) as f64 / frames as f64 / 1000.0 } else { 0.0 };
        let mut s = format!("<h1>{}</h1>", t(l, "Leistung"));
        s += &format!(
            "<p>{}: {} · {}: {:?} · {}: {:?} · RAM: {} MB · {}: {:.0} %</p>",
            t(l, "Kerne"),
            topo.ncpu,
            t(l, "schnell"),
            topo.big,
            t(l, "sparsam"),
            topo.little,
            topo.ram_mb,
            t(l, "frei"),
            crate::sched::mem_free_ratio() * 100.0
        );
        s += &format!(
            "<p>{}: {} · {}: {:.1} ms ({}: {:.1} ms, max {:.1} ms) · {}: {} · {}: {} s</p>",
            t(l, "Bilder"),
            frames,
            t(l, "Bildzeit"),
            avg,
            t(l, "zuletzt"),
            st.last_us.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0,
            st.max_us.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1000.0,
            t(l, "übersprungen"),
            st.skipped.load(std::sync::atomic::Ordering::Relaxed),
            t(l, "Laufzeit"),
            self.started.elapsed().as_secs()
        );
        s += &format!("<p>{}: {} · {}: {}</p>", t(l, "Zeichen-Threads"), topo.ncpu, t(l, "Energiesparen"), if self.saver { t(l, "an") } else { t(l, "aus") });
        s += &format!("<h1>{}</h1><table>", t(l, "Kerne"));
        for (i, v) in self.cpu_load.iter().enumerate() {
            s += &format!("<tr><td>CPU {i}</td><td style='width:70%'><div class='bar'><i style='width:{:.0}%'></i></div></td><td>{:.0} %</td></tr>", v * 100.0, v * 100.0);
        }
        s += &format!("</table><h1>{}</h1><table><tr><th>PID</th><th>{}</th><th>RAM</th><th>CPU</th></tr>", t(l, "Prozesse"), t(l, "Name"));
        for (pid, name, rss, cpu) in &self.procs {
            s += &format!("<tr><td>{pid}</td><td>{name}</td><td>{} MB</td><td>{cpu:.0} %</td></tr>", rss / 1024);
        }
        s += &format!("</table><h1>Tabs</h1><table><tr><th>#</th><th>{}</th><th>{}</th></tr>", t(l, "Seite"), t(l, "Zustand"));
        for tb in &self.tabs {
            s += &format!("<tr><td>{}</td><td>{}</td><td>{}</td></tr>", tb.id, html_esc(&host_of(&tb.uri)), tb.state.name(l));
        }
        s += &format!("</table><p>{}: {}</p>", t(l, "Werbe- und Trackingschutz"), html_esc(&self.filters));
        s
    }
}

/// Zeichnen ohne Zugriff auf den restlichen Browser (getrennte Ausleihe)
struct Draw<'a> {
    lang: Lang,
    app: bool,
    filters: &'a str,
    tabs: &'a [Tab],
    cur: usize,
    osk: bool,
    full: bool,
    content: IRect,
}

fn ir(r: Rect) -> IRect {
    IRect { x: r.x.floor() as i32, y: r.y.floor() as i32, w: r.w.ceil() as i32 + 1, h: r.h.ceil() as i32 + 1 }
}

impl Draw<'_> {
    fn draw(&self, ui: &mut Ui, ov: &Overlay, kb: &mut u8, tscroll: &mut f32, acts: &mut Vec<Act>) -> Vec<IRect> {
        let (w, h) = (ui.w, ui.h);
        let l = self.lang;
        let mut rects = Vec::new();
        let tb = if self.full { 0.0 } else { ui.dp(56.0) };
        let kh = if self.osk { ui.dp(236.0) } else { 0.0 };
        let bb = if self.full || self.osk { 0.0 } else { ui.dp(54.0) };
        let tab = self.tabs.get(self.cur);

        if let Overlay::Tabs = ov {
            let r = Rect::new(0.0, 0.0, w, h);
            self.tabs_view(ui, r, tscroll, acts);
            rects.push(ir(r));
            return rects;
        }
        if let Overlay::Url { text, sugg } = ov {
            let r = Rect::new(0.0, 0.0, w, h - kh);
            self.url_view(ui, r, text, sugg, acts);
            rects.push(ir(r));
        } else if tb > 0.0 {
            let r = Rect::new(0.0, 0.0, w, tb);
            self.top_bar(ui, r, tab, acts);
            rects.push(ir(r));
        }
        if bb > 0.0 {
            let r = Rect::new(0.0, h - bb, w, bb);
            self.bottom_bar(ui, r, tab, acts);
            rects.push(ir(r));
        }
        if kh > 0.0 {
            let r = Rect::new(0.0, h - kh, w, kh);
            if let Some(k) = ui.keyboard(r, kb) {
                acts.push(Act::Osk(k));
            }
            rects.push(ir(r));
        }
        // abgestürzte Seite
        if tab.is_some_and(|t| t.crashed) && matches!(ov, Overlay::None) {
            let c = self.content;
            let r = Rect::new(c.x as f32 + ui.dp(24.0), c.y as f32 + c.h as f32 * 0.3, c.w as f32 - ui.dp(48.0), ui.dp(150.0));
            ui.card(r);
            ui.label(Rect::new(r.x, r.y + ui.dp(14.0), r.w, ui.dp(30.0)), t(l, "Die Seite wurde beendet."), 17.0, ui.th.text, true, Al::Center);
            if ui.button(Rect::new(r.x + ui.dp(24.0), r.bottom() - ui.dp(64.0), r.w - ui.dp(48.0), ui.dp(46.0)), t(l, "Neu laden"), ui::Btn::Primary) {
                acts.push(Act::Reload);
            }
            rects.push(ir(r));
        }
        match ov {
            Overlay::Menu => {
                let items: Vec<(Ic, &str, Option<Act>)> = vec![
                    (Ic::Plus, t(l, "Neuer Tab"), Some(Act::NewTab(false))),
                    (Ic::Lock, t(l, "Neuer privater Tab"), Some(Act::NewTab(true))),
                    (Ic::Globe, t(l, "Desktop-Version"), Some(Act::Desktop)),
                    (Ic::Rotate, t(l, "Neu laden"), Some(Act::Reload)),
                    (Ic::Chip, t(l, "Leistung"), Some(Act::Perf)),
                    (Ic::Shield, self.filters, None),
                    (Ic::Power, if self.app { t(l, "App schließen") } else { t(l, "Browser schließen") }, Some(Act::Exit)),
                ];
                let rh = ui.dp(50.0);
                let pw = ui.dp(270.0).min(w - ui.dp(16.0));
                let panel = Rect::new(w - pw - ui.dp(8.0), tb + ui.dp(4.0), pw, rh * items.len() as f32 + ui.dp(12.0));
                ui.cv.soft_shadow(panel, ui.dp(14.0), Color { r: 0, g: 0, b: 0, a: 120 });
                ui.card(panel);
                let desktop = tab.is_some_and(|t| t.desktop);
                let mut y = panel.y + ui.dp(6.0);
                for (ic, label, a) in items {
                    let r = Rect::new(panel.x + ui.dp(6.0), y, panel.w - ui.dp(12.0), rh);
                    if a.is_some() && ui.held(r) {
                        ui.cv.rrect(r, ui.dp(10.0), ui.th.press);
                    }
                    let c = if a.is_some() { ui.th.text } else { ui.th.sub };
                    ui.icon(ic, r.x + ui.dp(22.0), r.cy(), ui.dp(20.0), if a.is_some() { ui.th.accent } else { ui.th.sub });
                    let lw = r.w - ui.dp(84.0);
                    let s = ui.fit(label, 15.0, false, lw);
                    ui.label(Rect::new(r.x + ui.dp(46.0), r.y, lw, r.h), &s, 15.0, c, false, Al::Start);
                    if matches!(a, Some(Act::Desktop)) && desktop {
                        ui.icon(Ic::Check, r.right() - ui.dp(20.0), r.cy(), ui.dp(18.0), ui.th.accent);
                    }
                    if let Some(a) = a {
                        if ui.tap(r) {
                            acts.push(a);
                        }
                    }
                    y += rh;
                }
                if ui.p.released && !ui.p.moved && !panel.contains(ui.p.px, ui.p.py) {
                    acts.push(Act::Close);
                }
                rects.push(ir(Rect::new(panel.x - ui.dp(14.0), panel.y - ui.dp(14.0), panel.w + ui.dp(28.0), panel.h + ui.dp(28.0))));
            }
            Overlay::Dialog { kind, msg, text, .. } => {
                let cw = (w - ui.dp(40.0)).min(ui.dp(420.0));
                let mh = ui.tx.measure(msg, ui.dp(15.0), false, cw - ui.dp(40.0)).1.min(h * 0.4);
                let fh = if *kind == WEBKIT_SCRIPT_DIALOG_PROMPT { ui.dp(56.0) } else { 0.0 };
                let chh = mh + fh + ui.dp(120.0);
                let top = if self.osk { (h - kh - chh) / 2.0 } else { (h - chh) / 2.0 }.max(ui.dp(8.0));
                let r = Rect::new((w - cw) / 2.0, top, cw, chh);
                ui.cv.soft_shadow(r, ui.dp(16.0), Color { r: 0, g: 0, b: 0, a: 140 });
                ui.card(r);
                ui.set_clip(Some(Rect::new(r.x, r.y, r.w, mh + ui.dp(30.0))));
                ui.para(r.x + ui.dp(20.0), r.y + ui.dp(20.0), cw - ui.dp(40.0), msg, 15.0, ui.th.text);
                ui.set_clip(None);
                if fh > 0.0 {
                    let fr = Rect::new(r.x + ui.dp(20.0), r.y + mh + ui.dp(30.0), cw - ui.dp(40.0), ui.dp(44.0));
                    ui.cv.rrect(fr, ui.dp(10.0), ui.th.bg);
                    let shown = format!("{text}|");
                    ui.label(fr.inset(ui.dp(10.0)), &shown, 15.0, ui.th.text, false, Al::Start);
                }
                let by = r.bottom() - ui.dp(60.0);
                let bw = (cw - ui.dp(52.0)) / 2.0;
                if *kind == WEBKIT_SCRIPT_DIALOG_ALERT {
                    if ui.button(Rect::new(r.x + ui.dp(20.0), by, cw - ui.dp(40.0), ui.dp(44.0)), "OK", ui::Btn::Primary) {
                        acts.push(Act::Dialog(true));
                    }
                } else {
                    if ui.button(Rect::new(r.x + ui.dp(20.0), by, bw, ui.dp(44.0)), t(l, "Abbrechen"), ui::Btn::Plain) {
                        acts.push(Act::Dialog(false));
                    }
                    if ui.button(Rect::new(r.x + ui.dp(32.0) + bw, by, bw, ui.dp(44.0)), "OK", ui::Btn::Primary) {
                        acts.push(Act::Dialog(true));
                    }
                }
                rects.push(ir(Rect::new(r.x - ui.dp(16.0), r.y - ui.dp(16.0), r.w + ui.dp(32.0), r.h + ui.dp(32.0))));
            }
            Overlay::Options { items, .. } => {
                let rh = ui.dp(50.0);
                let maxh = h * 0.6;
                let sh = (rh * items.len() as f32 + ui.dp(20.0)).min(maxh);
                let r = Rect::new(0.0, h - sh, w, sh);
                ui.cv.rect(r, ui.th.card);
                ui.cv.rect(Rect::new(0.0, r.y, w, 1.0), ui.th.line);
                let total = rh * items.len() as f32 + ui.dp(20.0);
                if ui.p.down && ui.p.moved && r.contains(ui.p.px, ui.p.py) {
                    *tscroll = (*tscroll - ui.p.dy).clamp(0.0, (total - sh).max(0.0));
                }
                ui.set_clip(Some(r));
                let mut y = r.y + ui.dp(10.0) - *tscroll;
                for (i, (label, enabled, sel, group)) in items.iter().enumerate() {
                    let row = Rect::new(ui.dp(8.0), y, w - ui.dp(16.0), rh);
                    if *enabled && !*group && ui.held(row) {
                        ui.cv.rrect(row, ui.dp(10.0), ui.th.press);
                    }
                    let c = if *group { ui.th.sub } else if *enabled { ui.th.text } else { ui.th.sub };
                    ui.rtl = l == Lang::Ar;
                    ui.label(Rect::new(row.x + ui.dp(16.0), row.y, row.w - ui.dp(56.0), rh), label, 15.0, c, *group, Al::Start);
                    ui.rtl = false;
                    if *sel {
                        ui.icon(Ic::Check, row.right() - ui.dp(22.0), row.cy(), ui.dp(18.0), ui.th.accent);
                    }
                    if *enabled && !*group && ui.tap(row) {
                        acts.push(Act::Option(i as u32));
                    }
                    y += rh;
                }
                ui.set_clip(None);
                if ui.p.released && !ui.p.moved && !r.contains(ui.p.px, ui.p.py) {
                    acts.push(Act::Close);
                }
                rects.push(ir(r));
            }
            Overlay::Perm { what, host, .. } => {
                let sh = ui.dp(190.0);
                let r = Rect::new(0.0, h - sh, w, sh);
                ui.cv.rect(r, ui.th.card);
                ui.cv.rect(Rect::new(0.0, r.y, w, 1.0), ui.th.line);
                ui.icon(Ic::Shield, w / 2.0, r.y + ui.dp(30.0), ui.dp(26.0), ui.th.accent);
                let msg = format!("{host}: {} – {what}", t(l, "möchte zugreifen"));
                ui.rtl = l == Lang::Ar;
                ui.para(ui.dp(24.0), r.y + ui.dp(56.0), w - ui.dp(48.0), &msg, 15.0, ui.th.text);
                ui.rtl = false;
                let bw = (w - ui.dp(60.0)) / 2.0;
                let by = r.bottom() - ui.dp(64.0);
                if ui.button(Rect::new(ui.dp(24.0), by, bw, ui.dp(46.0)), t(l, "Ablehnen"), ui::Btn::Plain) {
                    acts.push(Act::Perm(false));
                }
                if ui.button(Rect::new(ui.dp(36.0) + bw, by, bw, ui.dp(46.0)), t(l, "Erlauben"), ui::Btn::Primary) {
                    acts.push(Act::Perm(true));
                }
                rects.push(ir(r));
            }
            _ => {}
        }
        rects
    }

    fn top_bar(&self, ui: &mut Ui, r: Rect, tab: Option<&Tab>, acts: &mut Vec<Act>) {
        let l = self.lang;
        ui.cv.rect(r, ui.th.card);
        ui.cv.rect(Rect::new(0.0, r.bottom() - 1.0, r.w, 1.0), ui.th.line);
        let btn = ui.dp(46.0);
        let pill = Rect::new(ui.dp(10.0), r.y + ui.dp(8.0), r.w - ui.dp(20.0) - btn * 2.0, r.h - ui.dp(16.0));
        let held = ui.held(pill);
        ui.cv.rrect(pill, pill.h / 2.0, if held { ui.th.press } else { ui.th.bg });
        let (ic, icc) = match tab {
            Some(t) if t.private => (Ic::Lock, ui.th.accent2),
            Some(t) if t.secure => (Ic::Lock, ui.th.ok),
            Some(t) if t.uri.starts_with("ultar:") => (Ic::Home, ui.th.accent),
            _ => (Ic::Info, ui.th.sub),
        };
        ui.icon(ic, pill.x + ui.dp(20.0), pill.cy(), ui.dp(16.0), icc);
        let text = match tab {
            Some(t) if t.uri.starts_with("ultar://start") || t.uri.is_empty() => t_s(l, "Suchen oder Adresse eingeben"),
            Some(t) if t.uri.starts_with("ultar:") => t.title.clone(),
            Some(t) => host_of(&t.uri),
            None => String::new(),
        };
        let tc = if tab.is_some_and(|t| t.uri.starts_with("ultar://start")) { ui.th.sub } else { ui.th.text };
        let tw = pill.w - ui.dp(48.0);
        let s = ui.fit(&text, 15.0, false, tw);
        ui.label(Rect::new(pill.x + ui.dp(38.0), pill.y, tw, pill.h), &s, 15.0, tc, false, Al::Start);
        if ui.tap(pill) {
            acts.push(Act::EditUrl);
        }
        // Tab-Zähler
        let tr = Rect::new(r.w - btn * 2.0 - ui.dp(4.0), r.y + (r.h - btn) / 2.0, btn, btn);
        if ui.held(tr) {
            ui.cv.circle(tr.cx(), tr.cy(), btn / 2.0, ui.th.press);
        }
        let sq = Rect::new(tr.cx() - ui.dp(11.0), tr.cy() - ui.dp(11.0), ui.dp(22.0), ui.dp(22.0));
        ui.cv.rrect(sq, ui.dp(5.0), ui.th.text);
        ui.cv.rrect(sq.inset(ui.dp(2.0)), ui.dp(4.0), ui.th.card);
        let n = self.tabs.len();
        ui.label(sq, &if n > 99 { ":D".into() } else { n.to_string() }, 11.0, ui.th.text, true, Al::Center);
        if ui.tap(tr) {
            acts.push(Act::Tabs);
        }
        // Menü (drei Punkte)
        let mr = Rect::new(r.w - btn - ui.dp(2.0), tr.y, btn, btn);
        if ui.held(mr) {
            ui.cv.circle(mr.cx(), mr.cy(), btn / 2.0, ui.th.press);
        }
        for d in [-1.0f32, 0.0, 1.0] {
            ui.cv.circle(mr.cx(), mr.cy() + d * ui.dp(6.0), ui.dp(2.2), ui.th.text);
        }
        if ui.tap(mr) {
            acts.push(Act::Menu);
        }
        // Ladefortschritt
        if let Some(t) = tab {
            if t.loading {
                let pw = (r.w * t.progress.clamp(0.05, 1.0) as f32).round();
                ui.cv.rect(Rect::new(0.0, r.bottom() - ui.dp(2.5), pw, ui.dp(2.5)), ui.th.accent);
            }
        }
    }

    fn bottom_bar(&self, ui: &mut Ui, r: Rect, tab: Option<&Tab>, acts: &mut Vec<Act>) {
        ui.cv.rect(r, ui.th.card);
        ui.cv.rect(Rect::new(0.0, r.y, r.w, 1.0), ui.th.line);
        let e = tab.and_then(|t| t.eng.as_ref());
        let (cb, cf) = (e.is_some_and(|e| e.can_back()), e.is_some_and(|e| e.can_forward()));
        let loading = tab.is_some_and(|t| t.loading);
        let items: [(Ic, bool, Act); 5] = [
            (Ic::Back, true, Act::Back),
            (Ic::Chevron, cf, Act::Forward),
            (Ic::Home, true, Act::Home),
            (if loading { Ic::Close } else { Ic::Rotate }, true, if loading { Act::Stop } else { Act::Reload }),
            (Ic::Plus, true, Act::NewTab(false)),
        ];
        let cw = r.w / items.len() as f32;
        for (i, (ic, en, a)) in items.into_iter().enumerate() {
            let cell = Rect::new(r.x + cw * i as f32, r.y, cw, r.h);
            let en = en && !(i == 0 && !cb && self.tabs.len() <= 1 && !self.app && false);
            if en && ui.held(cell) {
                ui.cv.circle(cell.cx(), cell.cy(), ui.dp(21.0), ui.th.press);
            }
            ui.icon(ic, cell.cx(), cell.cy(), ui.dp(22.0), if en { ui.th.text } else { ui.th.line });
            if en && ui.tap(cell) {
                acts.push(a);
            }
        }
    }

    fn url_view(&self, ui: &mut Ui, r: Rect, text: &str, sugg: &[(String, String)], acts: &mut Vec<Act>) {
        let l = self.lang;
        ui.cv.rect(r, ui.th.bg);
        let bar = Rect::new(0.0, 0.0, r.w, ui.dp(56.0));
        ui.cv.rect(bar, ui.th.card);
        let f = Rect::new(ui.dp(10.0), ui.dp(8.0), r.w - ui.dp(70.0), ui.dp(40.0));
        ui.cv.rrect(f, f.h / 2.0, ui.th.bg);
        ui.cv.rrect(Rect::new(f.x, f.bottom() - 2.0, f.w, 2.0), 1.0, ui.th.accent);
        let shown = format!("{text}|");
        let tw = f.w - ui.dp(54.0);
        // langen Text vom Ende her zeigen (Eingabestelle sichtbar)
        let mut s = shown.clone();
        while ui.text_w(&s, 15.0, false) > tw && s.chars().count() > 1 {
            s = s.chars().skip(1).collect();
        }
        ui.label(Rect::new(f.x + ui.dp(16.0), f.y, tw, f.h), &s, 15.0, ui.th.text, false, Al::Start);
        let cr = Rect::new(f.right() - ui.dp(40.0), f.y, ui.dp(40.0), f.h);
        ui.icon(Ic::Close, cr.cx(), cr.cy(), ui.dp(16.0), ui.th.sub);
        if ui.tap(cr) {
            acts.push(Act::UrlClear);
        }
        let back = Rect::new(r.w - ui.dp(56.0), 0.0, ui.dp(56.0), ui.dp(56.0));
        ui.icon(Ic::Back, back.cx(), back.cy(), ui.dp(20.0), ui.th.text);
        if ui.tap(back) {
            acts.push(Act::Close);
        }
        let mut y = bar.bottom() + ui.dp(6.0);
        let rh = ui.dp(58.0);
        if !text.trim().is_empty() {
            let is_url = as_url(text.trim()).is_some();
            let row = Rect::new(0.0, y, r.w, rh);
            if ui.held(row) {
                ui.cv.rect(row, ui.th.press);
            }
            ui.icon(if is_url { Ic::Globe } else { Ic::Search }, ui.dp(30.0), row.cy(), ui.dp(18.0), ui.th.accent);
            let lab = if is_url { format!("{} {}", t(l, "Öffnen:"), text.trim()) } else { format!("{} {}", t(l, "Suchen:"), text.trim()) };
            let s = ui.fit(&lab, 15.0, false, r.w - ui.dp(76.0));
            ui.label(Rect::new(ui.dp(60.0), row.y, r.w - ui.dp(76.0), rh), &s, 15.0, ui.th.text, false, Al::Start);
            if ui.tap(row) {
                acts.push(Act::Submit(text.to_string()));
            }
            y += rh;
        }
        for (u, tt) in sugg {
            if y + rh > r.bottom() {
                break;
            }
            let row = Rect::new(0.0, y, r.w, rh);
            if ui.held(row) {
                ui.cv.rect(row, ui.th.press);
            }
            ui.icon(Ic::Clock, ui.dp(30.0), row.cy(), ui.dp(18.0), ui.th.sub);
            let tw = r.w - ui.dp(76.0);
            let a = ui.fit(if tt.is_empty() { u } else { tt }, 15.0, false, tw);
            let b = ui.fit(u, 12.0, false, tw);
            ui.label(Rect::new(ui.dp(60.0), row.y + ui.dp(6.0), tw, ui.dp(24.0)), &a, 15.0, ui.th.text, false, Al::Start);
            ui.label(Rect::new(ui.dp(60.0), row.y + ui.dp(30.0), tw, ui.dp(20.0)), &b, 12.0, ui.th.sub, false, Al::Start);
            if ui.tap(row) {
                acts.push(Act::Submit(u.clone()));
            }
            y += rh;
        }
    }

    fn tabs_view(&self, ui: &mut Ui, r: Rect, tscroll: &mut f32, acts: &mut Vec<Act>) {
        let l = self.lang;
        ui.cv.vgrad(r, ui.th.bg, ui.th.bg2);
        let head = Rect::new(0.0, 0.0, r.w, ui.dp(56.0));
        ui.cv.rect(head, ui.th.card);
        ui.label(Rect::new(ui.dp(18.0), 0.0, r.w / 2.0, head.h), &format!("Tabs ({})", self.tabs.len()), 18.0, ui.th.text, true, Al::Start);
        let plus = Rect::new(r.w - ui.dp(112.0), ui.dp(5.0), ui.dp(46.0), ui.dp(46.0));
        ui.icon(Ic::Plus, plus.cx(), plus.cy(), ui.dp(22.0), ui.th.accent);
        if ui.tap(plus) {
            acts.push(Act::NewTab(false));
        }
        let done = Rect::new(r.w - ui.dp(60.0), ui.dp(5.0), ui.dp(50.0), ui.dp(46.0));
        ui.label(done, t(l, "Fertig"), 15.0, ui.th.accent, true, Al::Center);
        if ui.tap(done) {
            acts.push(Act::Close);
        }
        let cols = if r.w > r.h { 4 } else { 2 };
        let gap = ui.dp(12.0);
        let cw = (r.w - gap * (cols as f32 + 1.0)) / cols as f32;
        let ch = cw * 1.45;
        let area = Rect::new(0.0, head.bottom(), r.w, r.h - head.h);
        let rows = self.tabs.len().div_ceil(cols);
        let total = rows as f32 * (ch + gap) + gap;
        if ui.p.down && ui.p.moved && area.contains(ui.p.px, ui.p.py) {
            *tscroll = (*tscroll - ui.p.dy).clamp(0.0, (total - area.h).max(0.0));
        }
        ui.set_clip(Some(area));
        for (i, tb) in self.tabs.iter().enumerate() {
            let (cx, cy) = ((i % cols) as f32, (i / cols) as f32);
            let card = Rect::new(gap + cx * (cw + gap), area.y + gap + cy * (ch + gap) - *tscroll, cw, ch);
            if !ui.visible(card) {
                continue;
            }
            ui.cv.soft_shadow(card, ui.dp(10.0), Color { r: 0, g: 0, b: 0, a: 90 });
            ui.cv.rrect(card, ui.dp(14.0), if i == self.cur { ui.th.accent } else { ui.th.line });
            let inner = card.inset(ui.dp(2.0));
            ui.cv.rrect(inner, ui.dp(12.0), ui.th.card);
            let tr = Rect::new(inner.x, inner.y, inner.w, ui.dp(34.0));
            if tb.private {
                ui.icon(Ic::Lock, tr.x + ui.dp(16.0), tr.cy(), ui.dp(14.0), ui.th.accent2);
            }
            let title = if tb.title.is_empty() { host_of(&tb.uri) } else { tb.title.clone() };
            let s = ui.fit(&title, 13.0, false, tr.w - ui.dp(64.0));
            ui.label(Rect::new(tr.x + ui.dp(30.0), tr.y, tr.w - ui.dp(64.0), tr.h), &s, 13.0, ui.th.text, false, Al::Start);
            let xr = Rect::new(tr.right() - ui.dp(34.0), tr.y, ui.dp(34.0), tr.h);
            ui.icon(Ic::Close, xr.cx(), xr.cy(), ui.dp(13.0), ui.th.sub);
            let img = Rect::new(inner.x + ui.dp(4.0), tr.bottom(), inner.w - ui.dp(8.0), inner.bottom() - tr.bottom() - ui.dp(4.0));
            match &tb.thumb {
                Some((tw, th, px)) if *tw > 0 => {
                    let srch = (*th as f32).min(*tw as f32 * img.h / img.w);
                    ui.cv.blit_xrgb(px, *tw as u32, *th as u32, Rect::new(0.0, 0.0, *tw as f32, srch), img, false);
                }
                _ => {
                    ui.cv.rrect(img, ui.dp(8.0), ui.th.bg);
                    ui.icon(Ic::Globe, img.cx(), img.cy(), ui.dp(30.0), ui.th.line);
                }
            }
            if tb.state != State::Active {
                let s = tb.state.name(l);
                let (pw, _) = ui.tx.measure(s, ui.dp(11.0), false, 0.0);
                let pr = Rect::new(img.x + ui.dp(6.0), img.bottom() - ui.dp(26.0), pw + ui.dp(16.0), ui.dp(20.0));
                ui.cv.rrect(pr, ui.dp(10.0), Color { r: 0, g: 0, b: 0, a: 160 });
                ui.label(pr, s, 11.0, rgb(0xFFFFFF), false, Al::Center);
            }
            if ui.tap(xr) {
                acts.push(Act::CloseTab(i));
            } else if ui.tap(card) {
                acts.push(Act::Switch(i));
            }
        }
        ui.set_clip(None);
    }
}

// ---------------- Hilfen ----------------
/// WebKit-Feature (auch interne) per Kennung schalten; false, wenn es sie nicht gibt.
unsafe fn set_feature(s: gpointer, id: &str, on: bool) -> bool {
    let list = webkit_settings_get_all_features();
    let mut found = false;
    for i in 0..webkit_feature_list_get_length(list) {
        let f = webkit_feature_list_get(list, i);
        if rs(webkit_feature_get_identifier(f)) == id {
            webkit_settings_set_feature_enabled(s, f, on as gboolean);
            found = true;
            break;
        }
    }
    webkit_feature_list_unref(list);
    found
}

fn now_ms_impl() -> u32 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_millis() as u32
}

fn t_s(l: Lang, k: &'static str) -> String {
    t(l, k).to_string()
}

pub fn host_of(u: &str) -> String {
    let s = u.split_once("://").map(|x| x.1).unwrap_or(u);
    let h = s.split(['/', '?', '#']).next().unwrap_or("");
    h.strip_prefix("www.").unwrap_or(h).to_string()
}

/// Eingabe als Adresse deuten (sonst None = Suche)
pub fn as_url(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() || s.contains(' ') {
        return None;
    }
    if s.contains("://") || s.starts_with("about:") || s.starts_with("ultar:") {
        return Some(s.to_string());
    }
    let host = s.split(['/', '?', '#']).next().unwrap_or("");
    let ok = (host.contains('.') && !host.starts_with('.') && !host.ends_with('.') && host.rsplit('.').next().is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_alphabetic() || c.is_ascii_digit())))
        || host.starts_with("localhost");
    ok.then(|| format!("https://{s}"))
}

pub fn urlencode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            b' ' => o.push('+'),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

fn html_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\'', "&#39;")
}

/// Zeichen -> X11-Keysym (Latin-1 direkt, sonst Unicode-Keysym)
fn keysym_of(c: char) -> u32 {
    let u = c as u32;
    if (0x20..0x7f).contains(&u) || (0xa0..0x100).contains(&u) { u } else { 0x0100_0000 | u }
}

/// Datenschutzzentrum des Systems fragen (/run/ultar/privacy: all=1, cam=1 …). Fehlt die Datei: erlaubt.
fn privacy_allows(cap: &str) -> bool {
    let Ok(s) = std::fs::read_to_string("/run/ultar/privacy") else { return true };
    let get = |k: &str| s.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(|v| v.trim() == "1");
    if get("all") == Some(false) {
        return false;
    }
    match cap {
        "cam" => get("cam").unwrap_or(true) && get("mic").unwrap_or(true),
        c => get(c).unwrap_or(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adressen_und_suche() {
        assert_eq!(as_url("example.com").as_deref(), Some("https://example.com"));
        assert_eq!(as_url("https://a.b/c").as_deref(), Some("https://a.b/c"));
        assert_eq!(as_url("wetter berlin"), None);
        assert_eq!(as_url("hallo"), None);
        assert_eq!(as_url("ultar://leistung").as_deref(), Some("ultar://leistung"));
        assert_eq!(host_of("https://www.youtube.com/watch?v=1"), "youtube.com");
        assert_eq!(urlencode("ä b"), "%C3%A4+b");
        assert_eq!(keysym_of('a'), 'a' as u32);
        assert_eq!(keysym_of('ب'), 0x0100_0000 | 'ب' as u32);
    }
}
