//! The phone in a compositor window (`ultar-surface`): a toplevel whose header the app draws
//! itself (`ClientHeader`, right `ui.client-header`), the incoming-call screen above the lock
//! screen while a call rings (`Role::CallScreen`, right `ui.call-screen`), input translated
//! for the toolkit, painting with damage, and the platform requests of [`Host`]
//! (clipboard, window operations) carried out on the connection.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::app::PhoneApp;
use crate::click::WindowOp;
use crate::port::{CallPort, Host};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_surface::client::{Client, ClientError, Window, WindowConfig};
use ultar_surface::proto::{Configure, Event, Request, Role, WindowState};
use ultar_surface::ui::{device_size, translate};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::Point;

/// Time the launcher may take to start Settings (it answers after the app is ready).
const LAUNCH_WAIT: Duration = Duration::from_secs(10);

/// Smallest window (logical points): the compact layout's width, header plus a few rows.
/// The window opens larger ([`PhoneApp::whole_size`]: the whole dial pad); the user may make
/// it this small, the dial pad scrolls then.
pub const MIN_SIZE: (u32, u32) = (360, 320);

/// Events handled in one step before drawing (what has arrived already: a new window gets
/// its configures in a row, and only the last one is worth a frame).
const EVENTS_PER_STEP: usize = 32;

/// MIME type of a copied number.
const TEXT: &str = "text/plain;charset=utf-8";

/// A request of the app to the platform, carried out by [`PhoneWindow`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostOp {
    /// Offer this text on the clipboard.
    Copy(String),
    /// Window operation.
    Window(WindowOp),
    /// Open the phone settings (through the launcher, `launcher.start` v2).
    Settings,
    /// Create a contact for this number (no Contacts app yet: reported to the caller).
    Contact(String),
}

/// [`Host`] that queues its requests for the window.
#[derive(Clone, Default)]
pub struct QueuedHost {
    ops: Arc<Mutex<Vec<HostOp>>>,
}

impl QueuedHost {
    fn push(&self, op: HostOp) {
        let mut ops = self.ops.lock().unwrap_or_else(|e| e.into_inner());
        if ops.len() < 64 {
            ops.push(op);
        }
    }

    /// Takes the queued requests.
    pub fn take(&self) -> Vec<HostOp> {
        std::mem::take(&mut *self.ops.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl Host for QueuedHost {
    fn copy_text(&mut self, text: &str) -> bool {
        self.push(HostOp::Copy(text.to_owned()));
        true
    }
    fn window(&mut self, op: WindowOp) {
        self.push(HostOp::Window(op));
    }
    fn open_settings(&mut self) {
        self.push(HostOp::Settings);
    }
    fn create_contact(&mut self, number: &str) {
        self.push(HostOp::Contact(number.to_owned()));
    }
}

/// What one step of the window loop ended with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Keep going.
    Running,
    /// The user closed the window.
    Closed,
    /// Requests only another program can carry out (settings, contacts).
    Forward(Vec<HostOp>),
}

/// The phone app in a window.
pub struct PhoneWindow<P: CallPort> {
    client: Client,
    window: Window,
    call_screen: Option<Window>,
    canvas: Option<Canvas>,
    app: PhoneApp<P, QueuedHost>,
    host: QueuedHost,
    pointer: Point,
    serial: u32,
    copied: Option<String>,
    started: Instant,
    maximized: bool,
    launcher: Option<ultar_bus::Client>,
    /// Settings asked for; waiting for the activation token of that click.
    settings_pending: bool,
    /// Answer of the launcher for the last Settings request (asked on a thread of its own:
    /// the launcher answers only when Settings is ready, the window must not wait for it).
    launching: Option<Receiver<bool>>,
    /// The window asked for its opening size as its least size and has not yet told the
    /// compositor the real least size ([`MIN_SIZE`]); done after the first frame.
    opening: bool,
}

fn serial_of(event: &Event) -> Option<u32> {
    match event {
        Event::PointerButton { serial, .. } | Event::Key { serial, .. } => Some(*serial),
        Event::TouchDown { serial, .. } => Some(*serial),
        _ => None,
    }
}

/// The toolkit's event for a compositor event. `pointer` is where the pointer is, in the
/// toolkit's points: the compositor reports positions in fixed point with motion only, so
/// buttons and the wheel act where the last motion was. (Kept as the raw fixed-point number
/// it was 256 times too far out: no click and no wheel turn reached a key of the dial pad.)
fn toolkit_event(event: &Event, pointer: &mut Point) -> Option<ultar_ui::event::Event> {
    let translated = translate(event, *pointer)?;
    if let ultar_ui::event::Event::PointerMove { pos } = &translated {
        *pointer = *pos;
    }
    Some(translated)
}

impl<P: CallPort> PhoneWindow<P> {
    /// Opens the window on `client`; `host` must be the host given to `app`.
    pub fn open(
        mut client: Client,
        mut app: PhoneApp<P, QueuedHost>,
        host: QueuedHost,
        title: &str,
    ) -> Result<PhoneWindow<P>, ClientError> {
        // The window opens with the whole dial pad in it: the compositor gives a new window a
        // share of the screen but no less than the least size stated before its first frame
        // (and no more than the screen has). After the first frame the least size becomes
        // MIN_SIZE (`draw`): the user may make the window smaller, the dial pad scrolls then
        // (view/dial.rs), as it does where the screen is too small for the whole pad.
        let whole = app.whole_size();
        let (w, h) = whole.unwrap_or(MIN_SIZE);
        let config = WindowConfig {
            min_size: (w.max(MIN_SIZE.0), h.max(MIN_SIZE.1)),
            ..WindowConfig::toplevel(title)
        };
        let window = Window::create(&mut client, &config)?;
        client.send(&Request::ClientHeader {
            surface: window.surface(),
            on: true,
        })?;
        Ok(PhoneWindow {
            client,
            window,
            call_screen: None,
            canvas: None,
            app,
            host,
            pointer: Point::new(0.0, 0.0),
            serial: 0,
            copied: None,
            started: Instant::now(),
            maximized: false,
            launcher: None,
            settings_pending: false,
            launching: None,
            opening: whole.is_some(),
        })
    }

    /// Uses `bus` to reach the app launcher (settings page).
    pub fn with_launcher(mut self, bus: ultar_bus::Client) -> Self {
        self.launcher = Some(bus);
        self
    }

    /// Uses `bus` to reach the app launcher from now on (the bus may connect after the
    /// window opened).
    pub fn set_launcher(&mut self, bus: ultar_bus::Client) {
        self.launcher = Some(bus);
    }

    /// The app (feed it `call.state` with [`PhoneApp::set_state`]).
    pub fn app(&mut self) -> &mut PhoneApp<P, QueuedHost> {
        &mut self.app
    }

    /// Milliseconds since the window opened (the app's clock).
    pub fn now_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// Waits up to `timeout` for one event, handles it, carries out the app's requests,
    /// keeps the call screen in step with the app and draws.
    pub fn step(&mut self, timeout: Duration, wall_s: i64) -> Result<Step, ClientError> {
        let mut wait = Some(timeout);
        for _ in 0..EVENTS_PER_STEP {
            let Some(event) = self.client.next_event(wait)? else {
                break;
            };
            if let Some(step) = self.handle(&event)? {
                return Ok(step);
            }
            wait = Some(Duration::ZERO);
        }
        let now = self.now_ms();
        // Clock work of the app (toasts, timer) cannot fail in a way the window could act on.
        let _ = self.app.tick(now, wall_s);
        let forward = self.carry_out()?;
        self.launch_answer();
        self.sync_call_screen()?;
        self.draw()?;
        Ok(match forward {
            Some(step) => step,
            None => Step::Running,
        })
    }

    fn handle(&mut self, event: &Event) -> Result<Option<Step>, ClientError> {
        if let Some(s) = serial_of(event) {
            self.serial = s;
        }
        if let Event::ActivationToken { token } = event {
            if std::mem::take(&mut self.settings_pending) {
                self.open_settings(Some(*token));
            }
            return Ok(None);
        }
        if let Event::SendRequest { transfer, mime } = event {
            let data = match (&self.copied, mime.as_str()) {
                (Some(text), TEXT) => text.as_bytes().to_vec(),
                _ => Vec::new(),
            };
            self.client.send(&Request::SendData {
                transfer: *transfer,
                data,
                last: true,
            })?;
            return Ok(None);
        }
        let screen = self.call_screen.as_mut().is_some_and(|w| w.handle(event));
        if !screen && self.window.handle(event) {
            if self.window.close_requested() {
                return Ok(Some(Step::Closed));
            }
            if let Event::Configure(c) = event {
                self.configure(c)?;
            }
            return Ok(None);
        }
        if let Event::Configure(c) = event {
            self.configure(c)?;
            return Ok(None);
        }
        if let Some(ui_event) = toolkit_event(event, &mut self.pointer) {
            // A failed rebuild leaves the last tree on screen; the next event retries.
            let _ = self.app.handle(ui_event);
        }
        Ok(None)
    }

    fn configure(&mut self, c: &Configure) -> Result<(), ClientError> {
        let ui = self.app.ui();
        ui.set_scale(c.scale_milli as f32 / 1000.0)
            .map_err(|_| ClientError::InvalidSize)?;
        ui.set_size(ultar_ui::Size::new(c.width as f32, c.height as f32))
            .map_err(|_| ClientError::InvalidSize)?;
        let (w, h) = device_size(c);
        let keep = self
            .canvas
            .as_ref()
            .is_some_and(|k| k.width() == w && k.height() == h);
        if !keep {
            let canvas =
                Canvas::new(w, h, PixelFormat::Bgrx8).map_err(|_| ClientError::InvalidSize)?;
            self.canvas = Some(canvas);
        }
        self.maximized = c.state == WindowState::Maximized;
        // The new size needs a new layout of the tree.
        self.app.relayout().map_err(|_| ClientError::InvalidSize)
    }

    /// Carries out the app's platform requests; returns what only others can do.
    fn carry_out(&mut self) -> Result<Option<Step>, ClientError> {
        let mut forward = Vec::new();
        let surface = self.window.surface();
        for op in self.host.take() {
            match op {
                HostOp::Copy(text) => {
                    self.copied = Some(text);
                    let mimes = vec![TEXT.to_owned()];
                    self.client.send(&Request::SetSelection {
                        serial: self.serial,
                        mimes,
                    })?;
                }
                HostOp::Window(WindowOp::Close) => return Ok(Some(Step::Closed)),
                HostOp::Window(op) => {
                    let state = match op {
                        WindowOp::Minimize => WindowState::Minimized,
                        _ if self.maximized => WindowState::Normal,
                        _ => WindowState::Maximized,
                    };
                    self.client.send(&Request::SetState { surface, state })?;
                }
                // The click's activation token lets Settings take the focus; it comes back as
                // an event, then the launcher is asked (`open_settings`).
                // Without a user action of the compositor (serial 0) there is no token to ask for.
                HostOp::Settings if self.serial == 0 => self.open_settings(None),
                HostOp::Settings => {
                    self.settings_pending = true;
                    self.client.request_activation(self.serial)?;
                }
                other => forward.push(other),
            }
        }
        Ok((!forward.is_empty()).then_some(Step::Forward(forward)))
    }

    /// Opens the phone's page in Settings; without a launcher (or when it refuses) the app
    /// shows why. The launcher is asked on a thread of its own: it answers only once
    /// Settings is ready (up to [`LAUNCH_WAIT`]) and the window keeps answering meanwhile.
    fn open_settings(&mut self, token: Option<u64>) {
        let Some(bus) = self.launcher.clone() else {
            self.app.notify_error("phone.error.launch");
            return;
        };
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("phone-launch".into())
            .spawn(move || {
                let ok = crate::launch::open_phone_settings(&bus, token, LAUNCH_WAIT).is_ok();
                let _ = tx.send(ok);
            });
        match spawned {
            Ok(_) => self.launching = Some(rx),
            Err(_) => self.app.notify_error("phone.error.launch"),
        }
    }

    /// Shows the launcher's refusal once it answered.
    fn launch_answer(&mut self) {
        let Some(rx) = &self.launching else { return };
        let failed = match rx.try_recv() {
            Ok(ok) => !ok,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => true,
        };
        self.launching = None;
        if failed {
            self.app.notify_error("phone.error.launch");
        }
    }

    /// While a call rings the app is shown on a call-screen surface too (above the lock
    /// screen); it goes when the call stops ringing. Without the right the compositor
    /// refuses the role and the ordinary window stays the only one.
    fn sync_call_screen(&mut self) -> Result<(), ClientError> {
        let wanted = self.app.wants_call_screen();
        match (wanted, self.call_screen.take()) {
            (true, None) => {
                let config = WindowConfig {
                    role: Role::CallScreen,
                    ..WindowConfig::toplevel("")
                };
                self.call_screen = Window::create(&mut self.client, &config).ok();
            }
            (false, Some(w)) => w.destroy(&mut self.client)?,
            (_, keep) => self.call_screen = keep,
        }
        Ok(())
    }

    /// Paints and presents when needed. Nothing is painted while every surface still waits
    /// for its last frame (`FrameDone`): a paint then would clear `needs_paint` without
    /// reaching the screen, and the window would keep showing an old picture (it looked
    /// frozen) until the next change.
    fn draw(&mut self) -> Result<(), ClientError> {
        let Some(canvas) = self.canvas.as_mut() else {
            return Ok(());
        };
        let main = self.window.can_draw();
        let screen = self.call_screen.as_ref().is_some_and(|w| w.can_draw());
        let ui = self.app.ui();
        if !ui.needs_paint() || !(main || screen) {
            return Ok(());
        }
        ui.paint_all(canvas);
        let size = (canvas.width(), canvas.height());
        if main {
            self.window
                .present(&mut self.client, canvas.data(), size, &[])?;
            if std::mem::take(&mut self.opening) {
                // Shown at its opening size; from now on it may be made as small as MIN_SIZE.
                self.client.send(&Request::SetSizeLimits {
                    surface: self.window.surface(),
                    min: MIN_SIZE,
                    max: (0, 0),
                })?;
            }
        }
        if let Some(screen) = self.call_screen.as_mut().filter(|w| w.can_draw()) {
            screen.present(&mut self.client, canvas.data(), size, &[])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_surface::proto::{SurfaceId, FIXED_ONE};
    use ultar_ui::event::Event as UiEvent;

    /// A click and a wheel turn act where the pointer is on the window (in points), not at
    /// the fixed-point number of the compositor's motion event.
    #[test]
    fn buttons_and_the_wheel_act_where_the_pointer_is() {
        let surface = SurfaceId(1);
        let mut pointer = Point::new(0.0, 0.0);
        let at = Point::new(412.0, 274.5);
        let fixed = |v: f32| (v * FIXED_ONE as f32) as i32;
        for motion in [
            Event::PointerEnter {
                surface,
                x: fixed(at.x),
                y: fixed(at.y),
            },
            Event::PointerMotion {
                surface,
                x: fixed(at.x),
                y: fixed(at.y),
            },
        ] {
            pointer = Point::new(0.0, 0.0);
            assert_eq!(
                toolkit_event(&motion, &mut pointer),
                Some(UiEvent::PointerMove { pos: at })
            );
            assert_eq!(pointer, at);
        }
        let press = Event::PointerButton {
            surface,
            serial: 2,
            button: 0x110,
            pressed: true,
        };
        match toolkit_event(&press, &mut pointer) {
            Some(UiEvent::PointerDown { pos, .. }) => assert_eq!(pos, at),
            other => panic!("{other:?}"),
        }
        let wheel = Event::PointerAxis {
            surface,
            dx: 0,
            dy: 48 * FIXED_ONE,
        };
        match toolkit_event(&wheel, &mut pointer) {
            Some(UiEvent::Scroll { pos, lines }) => {
                assert_eq!(pos, at);
                assert!(lines > 0.0);
            }
            other => panic!("{other:?}"),
        }
    }
}
