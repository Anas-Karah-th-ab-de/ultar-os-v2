//! The browser window on the compositor: one toplevel surface (`ultar-surface`), the
//! window tree painted by `ultar-ui` (page pixels included, as the page picture) into a
//! BGRX canvas and presented through the compositor's shared buffers. One thread waits on
//! the compositor socket and the engine socket together.

use crate::app::{AppError, Browser};
use crate::engine::{KeyInput, WebEngine};
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use std::fmt;
use std::time::{Duration, Instant};
use ultar_surface::client::{Client, ClientError, Window, WindowConfig};
use ultar_surface::proto::{Configure, Event, Modifiers as WireModifiers};
use ultar_surface::ui::{device_size, translate};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::geom::Point;
use ultar_ui::{Direction, Size};

/// Default compositor socket.
pub const DEFAULT_SOCKET: &str = "/run/ultar/ui/compositor.sock";
/// The application identifier at the compositor: the desktop shell's name of the browser, so the
/// window belongs to the browser's bar button (found in VMware: a second, nameless button appeared).
pub const APP_ID: &str = "org.ultar.browser";
/// Longest wait without any event (the clock of the window is advanced at least this often
/// while something animates; idle it waits for events only).
const IDLE: Duration = Duration::from_secs(60);

/// Why the window ended.
#[derive(Debug)]
pub enum WindowError {
    /// The compositor connection failed.
    Client(ClientError),
    /// The browser failed.
    App(AppError),
    /// Waiting for events failed.
    Poll(std::io::Error),
}

impl fmt::Display for WindowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WindowError::Client(e) => write!(f, "compositor: {e}"),
            WindowError::App(e) => write!(f, "browser: {e}"),
            WindowError::Poll(e) => write!(f, "poll: {e}"),
        }
    }
}

impl std::error::Error for WindowError {}

impl From<ClientError> for WindowError {
    fn from(e: ClientError) -> WindowError {
        WindowError::Client(e)
    }
}

impl From<AppError> for WindowError {
    fn from(e: AppError) -> WindowError {
        WindowError::App(e)
    }
}

/// Modifier bits of the engine from the compositor's modifiers.
pub fn modifier_bits(m: WireModifiers) -> u8 {
    let mut bits = 0;
    for (wire, bit) in [
        (WireModifiers::SHIFT, 1),
        (WireModifiers::CTRL, 2),
        (WireModifiers::ALT, 4),
        (WireModifiers::SUPER, 8),
    ] {
        if m.has(wire) {
            bits |= bit;
        }
    }
    bits
}

struct Surface {
    client: Client,
    window: Window,
    canvas: Option<Canvas>,
    pointer: Point,
    started: Instant,
}

impl Surface {
    fn configure<E: WebEngine>(
        &mut self,
        browser: &mut Browser<E>,
        c: &Configure,
    ) -> Result<(), WindowError> {
        let ui = browser.ui_mut();
        let scale = c.scale_milli as f32 / 1000.0;
        let bad = |_| WindowError::Client(ClientError::InvalidSize);
        ui.set_scale(scale).map_err(bad)?;
        ui.set_size(Size::new(c.width as f32, c.height as f32))
            .map_err(bad)?;
        ui.set_direction(if c.rtl {
            Direction::Rtl
        } else {
            Direction::Ltr
        });
        let (w, h) = device_size(c);
        if self
            .canvas
            .as_ref()
            .is_none_or(|k| k.width() != w || k.height() != h)
        {
            self.canvas =
                Some(Canvas::new(w, h, PixelFormat::Bgrx8).map_err(|_| ClientError::InvalidSize)?);
            ui.invalidate_layout();
        }
        browser.refresh()?;
        Ok(())
    }

    fn event<E: WebEngine>(
        &mut self,
        browser: &mut Browser<E>,
        event: Event,
    ) -> Result<(), WindowError> {
        if self.window.handle(&event) {
            if let Event::Configure(c) = &event {
                self.configure(browser, c)?;
            }
            return Ok(());
        }
        if let Event::PointerEnter { .. } | Event::PointerMotion { .. } = &event {
            if let Some(ultar_ui::event::Event::PointerMove { pos }) =
                translate(&event, self.pointer)
            {
                self.pointer = pos;
            }
        }
        if let Event::Key {
            code,
            state,
            modifiers,
            character,
            ..
        } = event
        {
            let key = KeyInput {
                code,
                character,
                modifiers: modifier_bits(modifiers),
                pressed: state != 0,
            };
            browser.key(key, translate(&event, self.pointer))?;
            return Ok(());
        }
        if let Some(ui_event) = translate(&event, self.pointer) {
            browser.pointer(ui_event)?;
        }
        Ok(())
    }

    fn draw<E: WebEngine>(&mut self, browser: &mut Browser<E>) -> Result<(), WindowError> {
        let Some(canvas) = self.canvas.as_mut() else {
            return Ok(());
        };
        let ui = browser.ui_mut();
        if !self.window.can_draw() || !ui.needs_paint() {
            return Ok(());
        }
        ui.paint(canvas);
        let rects: Vec<ultar_surface::proto::Rect> = canvas
            .take_damage()
            .rects()
            .iter()
            .filter(|r| r.x1 > r.x0 && r.y1 > r.y0)
            .map(|r| {
                ultar_surface::proto::Rect::new(
                    r.x0,
                    r.y0,
                    (r.x1 - r.x0) as u32,
                    (r.y1 - r.y0) as u32,
                )
            })
            .collect();
        let size = (canvas.width(), canvas.height());
        self.window
            .present(&mut self.client, canvas.data(), size, &rects)?;
        Ok(())
    }

    fn wait<E: WebEngine>(&self, browser: &Browser<E>) -> Result<(), WindowError> {
        let now = self.started.elapsed().as_millis() as u64;
        let wake = browser
            .ui()
            .next_wake()
            .map(|w| Duration::from_millis(w.saturating_sub(now)));
        let timeout = wake.map_or(IDLE, |w| w.min(IDLE));
        let spec = Timespec {
            tv_sec: timeout.as_secs() as i64,
            tv_nsec: i64::from(timeout.subsec_nanos()),
        };
        let client_fd = self.client.fd();
        let mut fds = vec![PollFd::new(&client_fd, PollFlags::IN)];
        let engine_fd = browser.engine().wake_fd();
        if let Some(fd) = &engine_fd {
            fds.push(PollFd::new(fd, PollFlags::IN));
        }
        match poll(&mut fds, Some(&spec)) {
            Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
            Err(e) => Err(WindowError::Poll(e.into())),
        }
    }
}

/// Shows `browser` in a new toplevel window of `client` until the window is closed or the
/// last tab closes. Settings, history and permissions are written at the end.
pub fn run<E: WebEngine>(browser: &mut Browser<E>, mut client: Client) -> Result<(), WindowError> {
    let title = browser.ui().translator().t("web.title").into_owned();
    let window = Window::create(&mut client, &WindowConfig::toplevel(&title))?;
    let mut s = Surface {
        client,
        window,
        canvas: None,
        pointer: Point::new(0.0, 0.0),
        started: Instant::now(),
    };
    while !browser.is_closed() && !s.window.close_requested() {
        s.wait(browser)?;
        while let Some(event) = s.client.next_event(Some(Duration::ZERO))? {
            s.event(browser, event)?;
            if browser.is_closed() || s.window.close_requested() {
                break;
            }
        }
        browser.pump(Some(Duration::ZERO))?;
        browser.tick(s.started.elapsed().as_millis() as u64)?;
        s.draw(browser)?;
    }
    browser.save()?;
    if !browser.is_closed() {
        let _ = s.window.destroy(&mut s.client);
    }
    Ok(())
}
