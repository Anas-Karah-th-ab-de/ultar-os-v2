//! Fensterköpfe im Ultar-Stil (vom Fenstersystem gezeichnet): Titel, Minimieren, Maximieren, Schließen.
//! Doppelklick auf den Titel maximiert bzw. stellt wieder her.
//! Basis: Anvil (Smithay, MIT), Darstellung neu.

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                AsRenderElements, Kind,
            },
            ImportMem, Renderer,
        },
    },
    desktop::WindowSurface,
    input::Seat,
    utils::{Logical, Point, Serial, Transform},
    wayland::shell::xdg::XdgShellHandler,
};

use std::{
    cell::{RefCell, RefMut},
    time::Instant,
};

use crate::{
    state::Backend,
    ultar::{
        canvas::{Canvas, Rgba},
        text::Text,
    },
    AnvilState,
};

use super::WindowElement;

pub struct WindowState {
    pub is_ssd: bool,
    pub header_bar: HeaderBar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Btn {
    Min,
    Max,
    Close,
}

#[derive(Debug, Clone)]
pub struct HeaderBar {
    pub pointer_loc: Option<Point<f64, Logical>>,
    pub width: u32,
    buffer: Option<MemoryRenderBuffer>,
    key: u64,
    last_click: Option<Instant>,
}

pub const HEADER_BAR_HEIGHT: i32 = 34;
const BUTTON_WIDTH: u32 = 42;

thread_local! {
    static TEXT: RefCell<Text> = RefCell::new(Text::new());
}

impl HeaderBar {
    fn button_at(&self, loc: Option<&Point<f64, Logical>>) -> Option<Btn> {
        let loc = loc?;
        let w = self.width as f64;
        let bw = BUTTON_WIDTH as f64;
        if loc.x >= w - bw {
            Some(Btn::Close)
        } else if loc.x >= w - bw * 2.0 {
            Some(Btn::Max)
        } else if loc.x >= w - bw * 3.0 {
            Some(Btn::Min)
        } else {
            None
        }
    }

    pub fn pointer_enter(&mut self, loc: Point<f64, Logical>) {
        self.pointer_loc = Some(loc);
    }

    pub fn pointer_leave(&mut self) {
        self.pointer_loc = None;
    }

    fn act<BackendData: Backend>(&mut self, btn: Btn, state: &mut AnvilState<BackendData>, window: &WindowElement) {
        match btn {
            Btn::Close => match window.0.underlying_surface() {
                WindowSurface::Wayland(w) => w.send_close(),
                #[cfg(feature = "xwayland")]
                WindowSurface::X11(w) => {
                    let _ = w.close();
                }
            },
            Btn::Max => toggle_maximize(state, window),
            Btn::Min => {
                let w = window.clone();
                state.handle.insert_idle(move |data| data.minimize_window(&w));
            }
        }
    }

    pub fn clicked<BackendData: Backend>(
        &mut self,
        seat: &Seat<AnvilState<BackendData>>,
        state: &mut AnvilState<BackendData>,
        window: &WindowElement,
        serial: Serial,
    ) {
        if self.pointer_loc.is_none() {
            return;
        }
        if let Some(btn) = self.button_at(self.pointer_loc.as_ref()) {
            self.act(btn, state, window);
            return;
        }
        // Doppelklick auf den Titel: maximieren / wiederherstellen
        let now = Instant::now();
        if self.last_click.map(|t| now.duration_since(t).as_millis() < 400).unwrap_or(false) {
            self.last_click = None;
            toggle_maximize(state, window);
            return;
        }
        self.last_click = Some(now);
        start_move(seat, state, window, serial);
    }

    pub fn touch_down<BackendData: Backend>(
        &mut self,
        seat: &Seat<AnvilState<BackendData>>,
        state: &mut AnvilState<BackendData>,
        window: &WindowElement,
        serial: Serial,
    ) {
        if self.pointer_loc.is_some() && self.button_at(self.pointer_loc.as_ref()).is_none() {
            start_move(seat, state, window, serial);
        }
    }

    pub fn touch_up<BackendData: Backend>(
        &mut self,
        _seat: &Seat<AnvilState<BackendData>>,
        state: &mut AnvilState<BackendData>,
        window: &WindowElement,
        _serial: Serial,
    ) {
        if let Some(btn) = self.button_at(self.pointer_loc.as_ref()) {
            self.act(btn, state, window);
        }
    }

    /// Kopf neu zeichnen, wenn sich Breite, Titel, Zustand oder Zeiger geändert haben.
    pub fn redraw(&mut self, width: u32, title: &str, active: bool, maximized: bool, scale: i32) {
        self.width = width;
        if width == 0 {
            self.buffer = None;
            return;
        }
        let hover = self.button_at(self.pointer_loc.as_ref());
        let key = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (width, title, active, maximized, hover, scale).hash(&mut h);
            h.finish()
        };
        if key == self.key && self.buffer.is_some() {
            return;
        }
        self.key = key;
        let s = scale.max(1);
        let (w, h) = (width as i32 * s, HEADER_BAR_HEIGHT * s);
        let kf = |v: f32| v * s as f32;
        let mut cv = Canvas::new(w, h);
        let bg = if active { Rgba::hex(0x1B1E27) } else { Rgba::hex(0x14161D) };
        // oben abgerundet, unten gerade
        cv.rrect(0, 0, w, h + kf(12.0) as i32, kf(10.0), bg);
        cv.fill(0, h - s, w, s, Rgba::hex(0x2C303C));
        if active {
            cv.rrect(kf(12.0) as i32, 0, kf(56.0) as i32, kf(2.5) as i32, kf(1.2), Rgba::hex(0xC9A227));
        }
        // Knöpfe
        let bw = BUTTON_WIDTH as i32 * s;
        let fg = if active { Rgba::hex(0xECE8DF) } else { Rgba::hex(0x8C92A0) };
        for (i, b) in [Btn::Min, Btn::Max, Btn::Close].into_iter().enumerate() {
            let x0 = w - bw * (3 - i as i32);
            let hov = hover == Some(b);
            if hov {
                let c = if b == Btn::Close { Rgba::hex(0xE5484D) } else { Rgba::hex(0x2A2E3A) };
                cv.rrect(x0 + kf(5.0) as i32, kf(5.0) as i32, bw - kf(10.0) as i32, h - kf(10.0) as i32, kf(7.0), c);
            }
            let (cx, cy) = (x0 as f32 + bw as f32 / 2.0, h as f32 / 2.0);
            let r = kf(5.0);
            let th = kf(1.5);
            let col = if hov && b == Btn::Close { Rgba::hex(0xFFFFFF) } else { fg };
            match b {
                Btn::Min => cv.line(cx - r, cy + r * 0.6, cx + r, cy + r * 0.6, th, col),
                Btn::Max => {
                    if maximized {
                        // zwei versetzte Rahmen
                        let o = kf(2.0);
                        for (a, bb, c, d) in [
                            (cx - r + o, cy - r, cx + r, cy - r),
                            (cx + r, cy - r, cx + r, cy + r - o),
                        ] {
                            cv.line(a, bb, c, d, th, col);
                        }
                        let (x0, y0, x1, y1) = (cx - r, cy - r + o, cx + r - o, cy + r);
                        for (a, bb, c, d) in [(x0, y0, x1, y0), (x1, y0, x1, y1), (x1, y1, x0, y1), (x0, y1, x0, y0)] {
                            cv.line(a, bb, c, d, th, col);
                        }
                    } else {
                        let (x0, y0, x1, y1) = (cx - r, cy - r, cx + r, cy + r);
                        for (a, bb, c, d) in [(x0, y0, x1, y0), (x1, y0, x1, y1), (x1, y1, x0, y1), (x0, y1, x0, y0)] {
                            cv.line(a, bb, c, d, th, col);
                        }
                    }
                }
                Btn::Close => {
                    cv.line(cx - r, cy - r, cx + r, cy + r, th, col);
                    cv.line(cx + r, cy - r, cx - r, cy + r, th, col);
                }
            }
        }
        // Titel
        let fsz = kf(13.0);
        let maxw = (w - bw * 3 - kf(28.0) as i32) as f32;
        if maxw > kf(20.0) {
            TEXT.with(|t| {
                let mut t = t.borrow_mut();
                let s = t.fit(title, fsz, active, maxw);
                let ty = h as f32 / 2.0 - Text::line_h(fsz) / 2.0;
                t.draw(&mut cv, &s, kf(14.0), ty, fsz, fg, active);
            });
        }
        self.buffer = Some(MemoryRenderBuffer::from_slice(
            &cv.px,
            Fourcc::Argb8888,
            (w, h),
            s,
            Transform::Normal,
            None,
        ));
    }
}

fn toggle_maximize<BackendData: Backend>(state: &mut AnvilState<BackendData>, window: &WindowElement) {
    match window.0.underlying_surface() {
        WindowSurface::Wayland(w) => {
            use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
            if w.current_state().states.contains(xdg_toplevel::State::Maximized) {
                let win = window.clone();
                state.handle.insert_idle(move |data| data.snap_window(&win, 2));
            } else {
                state.maximize_request(w.clone())
            }
        }
        #[cfg(feature = "xwayland")]
        WindowSurface::X11(w) => {
            let surface = w.clone();
            if w.is_maximized() {
                let win = window.clone();
                state.handle.insert_idle(move |data| data.snap_window(&win, 2));
            } else {
                state
                    .handle
                    .insert_idle(move |data| data.maximize_request_x11(&surface));
            }
        }
    };
}

fn start_move<BackendData: Backend>(
    seat: &Seat<AnvilState<BackendData>>,
    state: &mut AnvilState<BackendData>,
    window: &WindowElement,
    serial: Serial,
) {
    match window.0.underlying_surface() {
        WindowSurface::Wayland(w) => {
            let seat = seat.clone();
            let toplevel = w.clone();
            state
                .handle
                .insert_idle(move |data| data.move_request_xdg(&toplevel, &seat, serial));
        }
        #[cfg(feature = "xwayland")]
        WindowSurface::X11(w) => {
            let window = w.clone();
            state
                .handle
                .insert_idle(move |data| data.move_request_x11(&window));
        }
    };
}

impl<R> AsRenderElements<R> for HeaderBar
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    type RenderElement = MemoryRenderBufferRenderElement<R>;

    fn render_elements<C: From<Self::RenderElement>>(
        &self,
        renderer: &mut R,
        location: Point<i32, smithay::utils::Physical>,
        _scale: smithay::utils::Scale<f64>,
        alpha: f32,
    ) -> Vec<C> {
        let Some(buf) = &self.buffer else { return vec![] };
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            location.to_f64(),
            buf,
            Some(alpha),
            None,
            None,
            Kind::Unspecified,
        )
        .ok()
        .map(|e| vec![C::from(e)])
        .unwrap_or_default()
    }
}

impl WindowElement {
    pub fn decoration_state(&self) -> RefMut<'_, WindowState> {
        self.user_data().insert_if_missing(|| {
            RefCell::new(WindowState {
                is_ssd: false,
                header_bar: HeaderBar {
                    pointer_loc: None,
                    width: 0,
                    buffer: None,
                    key: 0,
                    last_click: None,
                },
            })
        });

        self.user_data()
            .get::<RefCell<WindowState>>()
            .unwrap()
            .borrow_mut()
    }

    pub fn set_ssd(&self, ssd: bool) {
        self.decoration_state().is_ssd = ssd;
    }
}
