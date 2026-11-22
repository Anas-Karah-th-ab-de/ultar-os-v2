//! Ultar-Desktop ohne DRM-Treiber: Ausgabe direkt in den Bildspeicher (fbdev) oder über Samsungs DECON
//! (Exynos, z. B. Galaxy A30 – ION-Puffer + `S3CFB_WIN_CONFIG`, derselbe Weg wie in ultar-shell).
//!
//! - Darstellung: Pixman (CPU, SIMD), nur geänderte Bereiche werden neu gezeichnet und kopiert.
//! - Eingabe: libinput im Pfad-Modus (`/dev/input/event*` direkt, ohne logind/seatd; die Geräte müssen von udevd
//!   im Ubuntu-Bereich erfasst sein – das erledigt desktop-session.sh), Touch, Maus, Tastatur.
//! - Bildschirmwahl: `ULTAR_WM_FB=decon:/dev/fb0 | fbdev:/dev/fb0 | file:/pfad/BREITExHÖHE`, sonst der von der
//!   Ultar-Oberfläche gemerkte Fall (`/data/ultar/display.case`), sonst DECON (wenn /dev/ion), dann fbdev.
//! - Maßstab: `ULTAR_WM_SCALE`, sonst Handy-Maßstab (kurze Seite ≈ 400 Punkte).

use std::{
    fs::{File, OpenOptions},
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::fs::{FileExt, OpenOptionsExt},
    },
    path::Path,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use smithay::{
    backend::{
        allocator::Fourcc,
        input::InputEvent,
        libinput::LibinputInputBackend,
        renderer::{
            damage::OutputDamageTracker,
            element::AsRenderElements,
            pixman::PixmanRenderer,
            Bind, ImportMemWl, Offscreen,
        },
    },
    input::{
        keyboard::LedState,
        pointer::{CursorImageAttributes, CursorImageStatus},
    },
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::EventLoop,
        input::{Libinput, LibinputInterface},
        pixman,
        wayland_protocols::wp::presentation_time::server::wp_presentation_feedback,
        wayland_server::{protocol::wl_surface, Display},
    },
    utils::{IsAlive, Physical, Rectangle, Transform},
    wayland::{compositor, presentation::Refresh},
};
use tracing::{error, info, warn};

use crate::{
    decon::Decon,
    drawing::{PointerElement, CLEAR_COLOR},
    render::{render_output, CustomRenderElements},
    state::{take_presentation_feedback, AnvilState, Backend},
};

pub const OUTPUT_NAME: &str = "fb0";

/// Wohin das fertige Bild geht.
enum Sink {
    Fbdev { f: File, line: usize, swap_rb: bool, var: [u32; 40] },
    Decon(Decon),
    /// Prüfweg ohne Bildschirm: Rohbild (BGRX) in eine Datei
    File { path: String },
}

pub struct FbData {
    full_redraw: u8,
    pub has_pointer: bool,
}

impl Backend for FbData {
    fn seat_name(&self) -> String {
        "seat0".into()
    }
    fn reset_buffers(&mut self, _output: &Output) {
        self.full_redraw = 2;
    }
    fn early_import(&mut self, _surface: &wl_surface::WlSurface) {}
    fn update_led_state(&mut self, _led_state: LedState) {}
}

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

fn open_fbdev(path: &str) -> Result<(Sink, u32, u32), String> {
    let f = OpenOptions::new().read(true).write(true).open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut var = [0u32; 40];
    let mut fix = [0u8; 80];
    unsafe {
        if ioctl(f.as_raw_fd(), 0x4600, var.as_mut_ptr()) != 0 || ioctl(f.as_raw_fd(), 0x4602, fix.as_mut_ptr()) != 0 {
            return Err("fbdev-ioctl fehlgeschlagen".into());
        }
    }
    if var[6] != 32 {
        return Err(format!("{} Bit Farbtiefe nicht unterstützt", var[6]));
    }
    let line = u32::from_ne_bytes([fix[48], fix[49], fix[50], fix[51]]) as usize;
    let swap_rb = var[8] == 0 && var[14] == 16;
    unsafe {
        // wie ultar-shell: erst POWERDOWN, dann UNBLANK (Exynos-Panels brauchen die volle Einschaltsequenz)
        ioctl(f.as_raw_fd(), 0x4611, 4);
        std::thread::sleep(Duration::from_millis(60));
        ioctl(f.as_raw_fd(), 0x4611, 0);
    }
    let (w, h) = (var[0], var[1]);
    Ok((Sink::Fbdev { f, line, swap_rb, var }, w, h))
}

fn open_case(case: &str) -> Result<(Sink, u32, u32), String> {
    match case.split_once(':') {
        Some(("decon", p)) => {
            let d = Decon::open(p)?;
            let (w, h) = (d.w, d.h);
            Ok((Sink::Decon(d), w, h))
        }
        Some(("fbdev", p)) => open_fbdev(p),
        Some(("file", spec)) => {
            // file:/pfad/1080x2340
            let (path, size) = spec.rsplit_once('/').map(|(a, b)| (a.to_string(), b)).ok_or("file:/pfad/BxH")?;
            let (w, h) = size.split_once('x').ok_or("Größe BxH")?;
            let (w, h) = (w.parse::<u32>().map_err(|e| e.to_string())?, h.parse::<u32>().map_err(|e| e.to_string())?);
            Ok((Sink::File { path: format!("{path}/ultar-wm-{w}x{h}.bgrx") }, w, h))
        }
        _ => Err(format!("unbekannter Bildschirm-Fall {case}")),
    }
}

fn open_sink() -> Result<(Sink, u32, u32, String), String> {
    if let Ok(c) = std::env::var("ULTAR_WM_FB") {
        return open_case(&c).map(|(s, w, h)| (s, w, h, c));
    }
    let mut cases = Vec::new();
    if let Ok(c) = std::fs::read_to_string("/data/ultar/display.case") {
        let c = c.trim().to_string();
        if !c.starts_with("drm:") && !c.is_empty() {
            cases.push(c);
        }
    }
    if Path::new("/dev/ion").exists() {
        cases.extend(["decon:/dev/fb0", "decon:/dev/graphics/fb0"].map(String::from));
    }
    cases.extend(["fbdev:/dev/fb0", "fbdev:/dev/graphics/fb0", "fbdev:/dev/fb1"].map(String::from));
    let mut errs = Vec::new();
    for c in cases {
        let p = c.split_once(':').map(|x| x.1).unwrap_or("");
        if !Path::new(p).exists() {
            continue;
        }
        match open_case(&c) {
            Ok((s, w, h)) => return Ok((s, w, h, c)),
            Err(e) => errs.push(format!("{c}: {e}")),
        }
    }
    Err(if errs.is_empty() { "kein Bildspeicher gefunden (/dev/fb*)".into() } else { errs.join("; ") })
}

impl Sink {
    /// Geänderte Bereiche aus dem gezeichneten Bild (x8r8g8b8, `stride` Bytes je Zeile) übernehmen.
    fn present(&mut self, src: &[u8], stride: usize, w: u32, h: u32, damage: &[Rectangle<i32, Physical>]) {
        let full = [Rectangle::<i32, Physical>::from_size((w as i32, h as i32).into())];
        let rects: &[Rectangle<i32, Physical>] = if damage.is_empty() { &full } else { damage };
        match self {
            Sink::Decon(d) => {
                let line = d.line_bytes();
                let buf = d.buffer();
                let dst = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, buf.len() * 4) };
                copy_rects(dst, line, src, stride, w, h, rects, false);
                d.flip();
            }
            Sink::Fbdev { f, line, swap_rb, var } => {
                for r in rects {
                    let r = clip(r, w, h);
                    if r.size.w <= 0 {
                        continue;
                    }
                    let mut row = vec![0u8; r.size.w as usize * 4];
                    for y in r.loc.y..r.loc.y + r.size.h {
                        let so = y as usize * stride + r.loc.x as usize * 4;
                        let n = row.len();
                        row.copy_from_slice(&src[so..so + n]);
                        if *swap_rb {
                            for px in row.chunks_exact_mut(4) {
                                px.swap(0, 2);
                            }
                        }
                        let _ = f.write_all_at(&row, (y as usize * *line + r.loc.x as usize * 4) as u64);
                    }
                }
                let mut v = *var;
                v[4] = 0;
                v[5] = 0;
                unsafe { ioctl(f.as_raw_fd(), 0x4606 /* FBIOPAN_DISPLAY */, v.as_mut_ptr()) };
            }
            Sink::File { path } => {
                let mut out = vec![0u8; (w * h * 4) as usize];
                copy_rects(&mut out, w as usize * 4, src, stride, w, h, &full, false);
                let tmp = format!("{path}.tmp");
                if std::fs::write(&tmp, &out).is_ok() {
                    let _ = std::fs::rename(&tmp, path.as_str());
                }
            }
        }
    }
}

fn clip(r: &Rectangle<i32, Physical>, w: u32, h: u32) -> Rectangle<i32, Physical> {
    let x0 = r.loc.x.clamp(0, w as i32);
    let y0 = r.loc.y.clamp(0, h as i32);
    let x1 = (r.loc.x + r.size.w).clamp(0, w as i32);
    let y1 = (r.loc.y + r.size.h).clamp(0, h as i32);
    Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into())
}

#[allow(clippy::too_many_arguments)]
fn copy_rects(dst: &mut [u8], dline: usize, src: &[u8], sline: usize, w: u32, h: u32, rects: &[Rectangle<i32, Physical>], swap: bool) {
    for r in rects {
        let r = clip(r, w, h);
        if r.size.w <= 0 || r.size.h <= 0 {
            continue;
        }
        let n = r.size.w as usize * 4;
        for y in r.loc.y..r.loc.y + r.size.h {
            let so = y as usize * sline + r.loc.x as usize * 4;
            let d0 = y as usize * dline + r.loc.x as usize * 4;
            if d0 + n > dst.len() || so + n > src.len() {
                break;
            }
            dst[d0..d0 + n].copy_from_slice(&src[so..so + n]);
            if swap {
                for px in dst[d0..d0 + n].chunks_exact_mut(4) {
                    px.swap(0, 2);
                }
            }
        }
    }
}

/// Eingabegeräte direkt öffnen (läuft als root; ohne logind/seatd).
struct DirectInput;

impl LibinputInterface for DirectInput {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        let acc = flags & 3;
        OpenOptions::new()
            .read(acc == 0 || acc == 2)
            .write(acc == 1 || acc == 2)
            .custom_flags(flags & !3)
            .open(path)
            .map(Into::into)
            .map_err(|e| -e.raw_os_error().unwrap_or(1))
    }
    fn close_restricted(&mut self, fd: OwnedFd) {
        drop(fd);
    }
}

fn auto_scale(w: u32, h: u32) -> f64 {
    if let Some(s) = std::env::var("ULTAR_WM_SCALE").ok().and_then(|s| s.parse::<f64>().ok()) {
        return s.clamp(1.0, 4.0);
    }
    let short = w.min(h) as f64;
    // Handy/Tablet: kurze Seite ~400 Punkte; Monitore (kurze Seite ≥ 1000 px bei Querformat): 1
    if h > w || short < 900.0 {
        (short / 400.0).round().clamp(1.0, 4.0)
    } else {
        1.0
    }
}

pub fn run_fb() {
    let mut event_loop = EventLoop::try_new().unwrap();
    let display = Display::new().unwrap();
    let mut display_handle = display.handle();

    let (mut sink, w, h, case) = match open_sink() {
        Ok(x) => x,
        Err(e) => {
            error!("Kein Bildschirm für den Bildspeicher-Weg: {e}");
            return;
        }
    };
    info!("Bildschirm: {case} ({w}x{h})");

    let mut renderer = match PixmanRenderer::new() {
        Ok(r) => r,
        Err(e) => {
            error!("Pixman nicht verfügbar: {e}");
            return;
        }
    };
    let mut image: pixman::Image<'static, 'static> = match renderer.create_buffer(Fourcc::Xrgb8888, (w as i32, h as i32).into()) {
        Ok(i) => i,
        Err(e) => {
            error!("Bildpuffer: {e}");
            return;
        }
    };

    let mode = Mode { size: (w as i32, h as i32).into(), refresh: 60_000 };
    let output = Output::new(
        OUTPUT_NAME.to_string(),
        PhysicalProperties { size: (0, 0).into(), subpixel: Subpixel::Unknown, make: "Ultar".into(), model: case.clone() },
    );
    let _global = output.create_global::<AnvilState<FbData>>(&display.handle());
    let scale = auto_scale(w, h);
    let transform = match std::env::var("ULTAR_WM_ROTATE").ok().as_deref() {
        Some("90") => Transform::_90,
        Some("180") => Transform::_180,
        Some("270") => Transform::_270,
        _ => Transform::Normal,
    };
    output.change_current_state(Some(mode), Some(transform), Some(Scale::Fractional(scale)), Some((0, 0).into()));
    output.set_preferred(mode);

    let data = FbData { full_redraw: 2, has_pointer: false };
    let mut state = AnvilState::init(display, event_loop.handle(), data, true);
    state.shm_state.update_formats(renderer.shm_formats());
    state.space.map_output(&output, (0, 0));
    let logical_w = (transform.transform_size(mode.size).w as f64 / scale) as i32;
    state.ui.output_added(logical_w);

    // Eingabe: libinput im Pfad-Modus
    let mut li = Libinput::new_from_path(DirectInput);
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir("/dev/input") {
        let mut paths: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().map(|f| f.to_string_lossy().starts_with("event")).unwrap_or(false))
            .collect();
        paths.sort();
        for p in paths {
            if let Some(dev) = li.path_add_device(&p.to_string_lossy()) {
                n += 1;
                let _ = dev;
            }
        }
    }
    info!("{n} Eingabegeräte über libinput");
    let dh = display_handle.clone();
    if let Err(e) = event_loop
        .handle()
        .insert_source(LibinputInputBackend::new(li), move |event, _, data: &mut AnvilState<FbData>| {
            // Zeiger erst zeigen, wenn wirklich eine Maus/ein Touchpad bewegt wird (Handys melden oft
            // Tasten-/Sensorgeräte mit Zeiger-Fähigkeit); Berührung blendet ihn wieder aus.
            match &event {
                InputEvent::PointerMotion { .. } | InputEvent::PointerMotionAbsolute { .. } => data.backend_data.has_pointer = true,
                InputEvent::TouchDown { .. } => data.backend_data.has_pointer = false,
                _ => {}
            }
            data.process_input_event_fb(&dh, event);
        })
    {
        error!("libinput: {e}");
    }

    #[cfg(feature = "xwayland")]
    state.start_xwayland();
    #[cfg(not(feature = "xwayland"))]
    state.run_autostart_once();

    let mut damage_tracker = OutputDamageTracker::from_output(&output);
    let mut pointer_element = PointerElement::default();
    let pointer_image = crate::cursor::Cursor::load();
    let mut pointer_buf: Option<smithay::backend::renderer::element::memory::MemoryRenderBuffer> = None;
    info!("Ultar-Desktop läuft (Bildspeicher-Weg, Pixman), Maßstab {scale}");

    let frame = Duration::from_millis(16);
    // Steuerung für Tests und Diagnose: Befehle zeilenweise in $XDG_RUNTIME_DIR/ultar-wm.ctl
    //   shot <pfad.png> | launcher | spawn <befehl> | bench <bilder> | stats | quit
    let ctl_path = std::env::var("XDG_RUNTIME_DIR").map(|d| format!("{d}/ultar-wm.ctl")).unwrap_or_else(|_| "/tmp/ultar-wm.ctl".into());
    let mut ctl = Ctl::open(&ctl_path);
    let mut stats = Stats::new();
    let mut shot_req: Option<String> = None;
    let mut bench_left = 0u32;
    while state.running.load(Ordering::SeqCst) {
        for line in ctl.lines() {
            let (cmd, arg) = line.split_once(' ').map(|(a, b)| (a.to_string(), b.trim().to_string())).unwrap_or((line.clone(), String::new()));
            match cmd.as_str() {
                "shot" => shot_req = Some(if arg.is_empty() { "/tmp/ultar-wm.png".into() } else { arg }),
                "launcher" => state.ui.toggle_launcher(),
                "spawn" => state.spawn_shell(&arg),
                "bench" => {
                    bench_left = arg.parse().unwrap_or(60);
                    stats = Stats::new();
                }
                "stats" => stats.log(true),
                "quit" => state.running.store(false, Ordering::SeqCst),
                _ => warn!("Steuerbefehl unbekannt: {line}"),
            }
        }
        if bench_left > 0 {
            // Prüflauf: jedes Bild komplett neu zeichnen (schlechtester Fall)
            state.backend_data.full_redraw = 1;
            bench_left -= 1;
            if bench_left == 0 {
                stats.log(true);
            }
        }
        let t0 = Instant::now();
        let now = state.clock.now();
        let frame_target = now + frame;
        state.pre_repaint(&output, frame_target);
        let ui_layers = state.ui_layers(&output);

        // Zeiger (nur wenn eine Maus/ein Touchpad da ist)
        let mut elements: Vec<CustomRenderElements<PixmanRenderer>> = Vec::new();
        if state.backend_data.has_pointer {
            if let CursorImageStatus::Surface(ref s) = state.cursor_status {
                if !s.alive() {
                    state.cursor_status = CursorImageStatus::default_named();
                }
            }
            let hotspot = if let CursorImageStatus::Surface(ref s) = state.cursor_status {
                compositor::with_states(s, |st| {
                    st.data_map.get::<std::sync::Mutex<CursorImageAttributes>>().unwrap().lock().unwrap().hotspot
                })
            } else {
                (0, 0).into()
            };
            // Zeigerbild nur einmal erzeugen – ein neuer Puffer je Bild gälte als Änderung (ständiges Neuzeichnen)
            let buf = pointer_buf
                .get_or_insert_with(|| {
                    let img = pointer_image.get_image(scale.ceil() as u32, Duration::ZERO);
                    smithay::backend::renderer::element::memory::MemoryRenderBuffer::from_slice(
                        &img.pixels_rgba,
                        Fourcc::Abgr8888,
                        (img.width as i32, img.height as i32),
                        scale.ceil() as i32,
                        Transform::Normal,
                        None,
                    )
                })
                .clone();
            pointer_element.set_buffer(buf);
            pointer_element.set_status(state.cursor_status.clone());
            let pos = state.pointer.current_location() - hotspot.to_f64();
            let s = smithay::utils::Scale::from(scale);
            elements.extend(pointer_element.render_elements(&mut renderer, pos.to_physical(s).to_i32_round(), s, 1.0));
        }

        let age = if state.backend_data.full_redraw > 0 {
            state.backend_data.full_redraw -= 1;
            0
        } else {
            1
        };
        let result = match renderer.bind(&mut image) {
            Ok(mut fb) => render_output(
                &output,
                &state.space,
                elements,
                &mut renderer,
                &mut fb,
                &mut damage_tracker,
                age,
                state.show_window_preview,
                &ui_layers,
            )
            .map(|r| (r.damage.cloned(), r.states)),
            Err(e) => {
                warn!("Pixman-Bindung: {e}");
                state.running.store(false, Ordering::SeqCst);
                break;
            }
        };
        match result {
            Ok((damage, states)) => {
                if let Some(damage) = damage {
                    let stride = image.stride();
                    let len = stride * h as usize;
                    let src = unsafe { std::slice::from_raw_parts(image.data() as *const u8, len) };
                    let t_render = t0.elapsed();
                    sink.present(src, stride, w, h, &damage);
                    let px: i64 = damage.iter().map(|r| r.size.w as i64 * r.size.h as i64).sum();
                    stats.add(t_render, t0.elapsed(), px);
                    if let Some(p) = shot_req.take() {
                        save_png(src, stride, w, h, &p);
                    }
                    let mut fb = take_presentation_feedback(&output, &state.space, &states);
                    fb.presented(
                        frame_target,
                        Refresh::fixed(frame),
                        0,
                        wp_presentation_feedback::Kind::empty(),
                    );
                }
                state.post_repaint(&output, frame_target, None, &states);
            }
            Err(e) => warn!("Darstellung: {e:?}"),
        }
        let _ = CLEAR_COLOR;

        stats.log(false);
        let wait = frame.saturating_sub(t0.elapsed());
        if event_loop.dispatch(Some(wait), &mut state).is_err() {
            state.running.store(false, Ordering::SeqCst);
        } else {
            state.space.refresh();
            state.popups.cleanup();
            let _ = display_handle.flush_clients();
        }
    }
    info!("Ultar-Desktop beendet");
}

/// Bildzeiten messen (Zeichnen und Ausgeben) und alle 10 s ins Protokoll schreiben.
struct Stats {
    since: Instant,
    frames: u32,
    render: Duration,
    total: Duration,
    worst: Duration,
    pixels: i64,
}

impl Stats {
    fn new() -> Self {
        Self { since: Instant::now(), frames: 0, render: Duration::ZERO, total: Duration::ZERO, worst: Duration::ZERO, pixels: 0 }
    }
    fn add(&mut self, render: Duration, total: Duration, px: i64) {
        self.frames += 1;
        self.render += render;
        self.total += total;
        self.worst = self.worst.max(total);
        self.pixels += px;
    }
    fn log(&mut self, force: bool) {
        if !force && self.since.elapsed() < Duration::from_secs(10) {
            return;
        }
        if self.frames > 0 {
            let n = self.frames as f64;
            let ms = |d: Duration| d.as_secs_f64() * 1000.0;
            info!(
                "Bilder: {} in {:.1} s · Zeichnen {:.1} ms · gesamt {:.1} ms (schlechtestes {:.1} ms) · {:.0} kPixel/Bild",
                self.frames,
                self.since.elapsed().as_secs_f64(),
                ms(self.render) / n,
                ms(self.total) / n,
                ms(self.worst),
                self.pixels as f64 / n / 1000.0
            );
        }
        *self = Self::new();
    }
}

/// Bildschirmfoto des aktuellen Bildes (x8r8g8b8) als PNG.
fn save_png(src: &[u8], stride: usize, w: u32, h: u32, path: &str) {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h as usize {
        for px in src[y * stride..y * stride + w as usize * 4].chunks_exact(4) {
            rgb.extend_from_slice(&[px[2], px[1], px[0]]);
        }
    }
    match image::RgbImage::from_raw(w, h, rgb).map(|i| i.save(path)) {
        Some(Ok(())) => info!("Bildschirmfoto: {path}"),
        other => warn!("Bildschirmfoto fehlgeschlagen: {other:?}"),
    }
}

/// Steuerkanal (benannte Pipe), nicht blockierend gelesen.
struct Ctl {
    f: Option<File>,
    buf: String,
}

impl Ctl {
    fn open(path: &str) -> Self {
        extern "C" {
            fn mkfifo(path: *const std::os::raw::c_char, mode: u32) -> i32;
        }
        let _ = std::fs::remove_file(path);
        if let Ok(c) = std::ffi::CString::new(path) {
            unsafe { mkfifo(c.as_ptr(), 0o600) };
        }
        // O_RDWR hält die Pipe offen (kein EOF, wenn ein Schreiber geht), O_NONBLOCK
        let f = OpenOptions::new().read(true).write(true).custom_flags(0o4000).open(path).ok();
        if f.is_some() {
            info!("Steuerkanal: {path}");
        }
        Self { f, buf: String::new() }
    }
    fn lines(&mut self) -> Vec<String> {
        use std::io::Read;
        let Some(f) = self.f.as_mut() else { return Vec::new() };
        let mut tmp = [0u8; 1024];
        while let Ok(n) = f.read(&mut tmp) {
            if n == 0 {
                break;
            }
            self.buf.push_str(&String::from_utf8_lossy(&tmp[..n]));
        }
        let mut out = Vec::new();
        while let Some(i) = self.buf.find('\n') {
            let l = self.buf[..i].trim().to_string();
            self.buf.drain(..=i);
            if !l.is_empty() {
                out.push(l);
            }
        }
        out
    }
}
