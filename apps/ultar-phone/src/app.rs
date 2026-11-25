//! The phone app: owns the toolkit tree, the [`Model`], the call-service port and the host.
//! The platform feeds it input events, `call.state` updates and clock ticks, and paints
//! [`PhoneApp::ui`] when it needs painting. Clicks are carried out in `app_click.rs`.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::click::Click;
use crate::model::{Model, Screen};
use crate::port::{CallPort, Host, PortError};
use crate::text::{failure_key, translator};
use crate::view::{self, Built};
use std::fmt;
use ultar_bus::messages::call::{CallError, CallState, Cellular, Phase, Ultar, Way};
use ultar_i18n::{Lang, ParseError, Translator};
use ultar_ui::canvas::Canvas;
use ultar_ui::event::{ActionKind, Event, Response};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{motion, InputMode, ThemeKind};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Label, Toast, Tone};
use ultar_ui::{Point, Size};

/// Failure while setting the app up or building its tree.
#[derive(Debug)]
pub enum AppError {
    /// The toolkit refused.
    Ui(UiError),
    /// A catalog of the phone does not parse.
    Catalog(ParseError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Ui(e) => write!(f, "toolkit: {e}"),
            AppError::Catalog(e) => write!(f, "catalog: {e}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<UiError> for AppError {
    fn from(e: UiError) -> Self {
        AppError::Ui(e)
    }
}

impl From<ParseError> for AppError {
    fn from(e: ParseError) -> Self {
        AppError::Catalog(e)
    }
}

/// How the app looks when it starts.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// Language.
    pub lang: Lang,
    /// Theme.
    pub theme: ThemeKind,
    /// Mouse or finger.
    pub mode: InputMode,
    /// Surface size in logical points.
    pub size: Size,
}

/// Width the window opens with on a PC: the dial pad's column and as much for the history.
pub const OPEN_WIDTH: f32 = 800.0;

/// Height of the surface the dial pad is measured on (higher than any pad).
const PROBE_HEIGHT: f32 = 4096.0;

/// Identifier of the one toast the phone shows.
const TOAST: u32 = 1;

/// The phone app.
pub struct PhoneApp<P: CallPort, H: Host> {
    pub(crate) ui: Ui,
    pub(crate) model: Model,
    pub(crate) port: P,
    pub(crate) host: H,
    pub(crate) built: Built,
    pub(crate) tr: Translator,
    pub(crate) menu: Option<Click>,
    pub(crate) toast: Option<(Tone, Label)>,
    pub(crate) pointer: Option<Point>,
    pub(crate) hold_start: Option<u64>,
    pub(crate) last_second: u64,
    /// The newest pictures of a video call: the other side's and the own camera's.
    pub(crate) remote: Option<Canvas>,
    pub(crate) own: Option<Canvas>,
}

impl<P: CallPort, H: Host> PhoneApp<P, H> {
    /// A phone app showing `state`.
    pub fn new(look: Look, state: CallState, port: P, host: H) -> Result<Self, AppError> {
        let tr = translator(look.lang)?;
        let ui = Ui::new(
            PaintEnv::new(look.lang, look.theme),
            look.mode,
            look.size,
            tr.clone(),
        )?;
        let mut app = PhoneApp {
            ui,
            model: Model::default(),
            port,
            host,
            built: Built::default(),
            tr,
            menu: None,
            toast: None,
            pointer: None,
            hold_start: None,
            last_second: 0,
            remote: None,
            own: None,
        };
        app.model.set_state(state, 0);
        app.reload_log();
        app.rebuild()?;
        Ok(app)
    }

    /// The toolkit tree (paint it, read its accessibility tree).
    pub fn ui(&mut self) -> &mut Ui {
        &mut self.ui
    }

    /// The toolkit tree, read-only.
    pub fn ui_ref(&self) -> &Ui {
        &self.ui
    }

    /// What the app shows.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// True while a call rings: the platform then shows the app as the incoming-call screen
    /// (compositor role `CallScreen`, right `ui.call-screen`), above the lock screen.
    pub fn wants_call_screen(&self) -> bool {
        matches!(self.model.screen(), crate::model::Screen::Incoming(_))
    }

    /// The clickable nodes of the current tree.
    pub fn clicks(&self) -> &[(NodeId, Click)] {
        &self.built.clicks
    }

    /// Key of the message of the shown toast.
    pub fn toast_key(&self) -> Option<&str> {
        self.toast.as_ref().and_then(|(_, l)| l.key_name())
    }

    /// Handles of the current tree.
    pub fn built(&self) -> &Built {
        &self.built
    }

    /// The call-service port (tests read what was sent).
    pub fn port(&mut self) -> &mut P {
        &mut self.port
    }

    /// The host (tests read what was asked).
    pub fn host(&mut self) -> &mut H {
        &mut self.host
    }

    /// Changes language, theme or size; the tree is rebuilt.
    pub fn set_look(&mut self, look: Look) -> Result<(), AppError> {
        self.tr = translator(look.lang)?;
        self.ui.set_lang(look.lang);
        self.ui.set_theme(look.theme);
        self.ui.set_input_mode(look.mode);
        self.ui.set_size(look.size)?;
        self.rebuild()
    }

    /// The window size (logical points) at which the dial screen shows whole in its two
    /// columns: [`OPEN_WIDTH`] wide, as high as the header and the dial pad down to the call
    /// buttons and the pad's edge. Measured from the layout in this language, with the rows
    /// that come and go above the keys counted in: the highest notice about a line (the
    /// window opens before the call service answered, and the line's state changes) and the
    /// suggestion shown while typing; so it does not depend on the state at the start.
    /// The window asks the compositor for this size when it opens; on anything lower the
    /// pad scrolls. `None` outside the dial screen (a call's screen fills whatever it gets).
    pub fn whole_size(&mut self) -> Option<(u32, u32)> {
        if self.model.screen() != Screen::Dial {
            return None;
        }
        let size = self.ui.size();
        self.ui.set_size(Size::new(OPEN_WIDTH, PROBE_HEIGHT)).ok()?;
        let mut probe = self.model.clone();
        probe.digits.clear();
        let cellular = probe.cellular_way();
        let lines = [
            (Way::Ultar, Ultar::SignedOut, Cellular::NoModem),
            (Way::Ultar, Ultar::Offline, Cellular::NoModem),
            (cellular, Ultar::SignedOut, Cellular::NoModem),
            (cellular, Ultar::SignedOut, Cellular::NoSim),
            (cellular, Ultar::SignedOut, Cellular::Airplane),
            (cellular, Ultar::SignedOut, Cellular::NoNetwork),
            (cellular, Ultar::SignedOut, Cellular::EmergencyOnly),
        ];
        let mut bottom: Option<f32> = None;
        for (way, ultar, cell) in lines {
            probe.way = Some(way);
            probe.state.lines.ultar = ultar;
            probe.state.lines.cellular = cell;
            let Ok(built) = view::build(&mut self.ui, &probe) else {
                continue;
            };
            self.built = built;
            self.ui.layout();
            if let Some(b) = self.pad_bottom() {
                bottom = Some(bottom.map_or(b, |a| a.max(b)));
            }
        }
        // Back to the surface's size and the real state.
        let restored = self.ui.set_size(size).is_ok() && self.rebuild().is_ok();
        let suggestion = view::metrics::key_height() + view::dial::PAD_GAP.points();
        let bottom = bottom.filter(|b| restored && b.is_finite() && *b > 0.0)? + suggestion;
        Some((OPEN_WIDTH as u32, bottom.ceil() as u32))
    }

    /// Lower end of the dial pad's content (the call buttons and the pad's edge below them).
    fn pad_bottom(&mut self) -> Option<f32> {
        self.built.pad?;
        let (call, _) = self.built.clicks.iter().find(|(_, c)| *c == Click::Call)?;
        let r = self.ui.rect(*call)?;
        let compact = self.ui.size_class() == ultar_ui::tokens::SizeClass::Compact;
        Some(r.y + r.h + view::dial::pad_edge(compact).points())
    }

    /// Lays the tree out again for a new surface size (window configure).
    pub fn relayout(&mut self) -> Result<(), AppError> {
        self.rebuild()
    }

    /// Sets the offset of the local time zone from UTC (history times).
    pub fn set_utc_offset(&mut self, offset_s: i32) -> Result<(), AppError> {
        self.model.utc_offset_s = offset_s;
        self.rebuild()
    }

    /// A picture of the call arrived (`call.picture`, a JPEG): the own camera's (`own`) or the
    /// other side's; false: not a picture, or no call to show it in.
    pub fn set_picture(&mut self, own: bool, jpeg: &[u8]) -> Result<bool, AppError> {
        if !matches!(self.model.screen(), Screen::InCall(_)) {
            return Ok(false);
        }
        let fit = if own {
            let (w, h) = view::metrics::own_picture();
            let s = f64::from(self.ui.env().scale);
            Some(((f64::from(w) * s) as usize, (f64::from(h) * s) as usize))
        } else {
            None
        };
        let Some(canvas) = crate::picture::canvas(jpeg, fit) else {
            return Ok(false);
        };
        let seen = if own {
            self.own = Some(canvas);
            std::mem::replace(&mut self.model.pictures.own, true)
        } else {
            self.remote = Some(canvas);
            std::mem::replace(&mut self.model.pictures.other, true)
        };
        if seen {
            self.show_pictures()?;
        } else {
            self.rebuild()?;
        }
        Ok(true)
    }

    /// Puts the newest pictures into their nodes.
    fn show_pictures(&mut self) -> Result<(), AppError> {
        if let (Some(id), Some(c)) = (self.built.remote, &self.remote) {
            self.ui.set_picture(id, c.clone())?;
        }
        if let (Some(id), Some(c)) = (self.built.own, &self.own) {
            self.ui.set_picture(id, c.clone())?;
        }
        Ok(())
    }

    /// A new `call.state` arrived at local time `now_ms`.
    pub fn set_state(&mut self, state: CallState, now_ms: u64) -> Result<(), AppError> {
        let ended_before = self.model.state.ended.as_ref().map(|c| c.id);
        let ended_now = state.ended.as_ref().map(|c| c.id);
        self.model.set_state(state, now_ms);
        if !self.model.pictures.other {
            self.remote = None;
        }
        if !self.model.pictures.own {
            self.own = None;
        }
        if ended_before != ended_now {
            self.reload_log();
        }
        self.rebuild()
    }

    /// The clock moved: wall time for the history, animations, toasts, held backspace and the
    /// call timer.
    pub fn tick(&mut self, now_ms: u64, wall_s: i64) -> Result<(), AppError> {
        self.model.now_ms = now_ms;
        let minute_changed = self.model.wall_s / 60 != wall_s / 60;
        self.model.wall_s = wall_s;
        if minute_changed {
            // History times ("today 16:20") depend on the wall clock.
            self.rebuild()?;
        }
        self.ui.advance_to(now_ms);
        let actions = self.ui.take_actions();
        self.apply(Response {
            consumed: true,
            actions,
            ..Response::default()
        })?;
        if let Some(start) = self.hold_start {
            if now_ms.saturating_sub(start) >= u64::from(motion::LONG_PRESS) {
                // CLICKS.md phone.backspace: held = delete all.
                self.hold_start = None;
                self.model.digits.clear();
                self.hold_start = Some(u64::MAX);
                self.rebuild()?;
            }
        }
        let second = now_ms / 1000;
        if second != self.last_second {
            self.last_second = second;
            self.update_timer()?;
        }
        Ok(())
    }

    /// One input event.
    pub fn handle(&mut self, event: Event) -> Result<Response, AppError> {
        self.track_hold(&event);
        let response = self.ui.handle(event);
        self.apply(response.clone())?;
        Ok(response)
    }

    fn track_hold(&mut self, event: &Event) {
        let back = self
            .built
            .number
            .and_then(|id| self.ui.end_action_rect(id).ok().flatten());
        match event {
            Event::PointerMove { pos } => self.pointer = Some(*pos),
            Event::PointerDown { pos, .. } | Event::TouchDown { pos, .. } => {
                self.pointer = Some(*pos);
                if back.is_some_and(|r| r.contains(*pos)) {
                    self.hold_start = Some(self.model.now_ms);
                }
            }
            Event::PointerUp { .. } | Event::TouchUp { .. } | Event::PointerLeave => {}
            _ => {}
        }
    }

    fn apply(&mut self, response: Response) -> Result<(), AppError> {
        let mut changed = false;
        for action in response.actions {
            let click = self
                .built
                .clicks
                .iter()
                .find(|(id, _)| *id == action.node)
                .map(|(_, c)| *c);
            changed |= match (click, action.kind) {
                (_, ActionKind::MenuChosen(item)) => self.menu_chosen(item),
                (_, ActionKind::ToastClosed(_)) => self.toast_closed(),
                (_, ActionKind::Closed) if self.built.dialog == Some(action.node) => {
                    self.close_dialog()
                }
                (Some(Click::Number), ActionKind::Activated)
                    if self.hold_start == Some(u64::MAX) =>
                {
                    // The hold already deleted everything; the release does nothing more.
                    self.hold_start = None;
                    false
                }
                (Some(click), kind) => {
                    if click == Click::Number && action.kind == ActionKind::Activated {
                        self.hold_start = None;
                    }
                    self.on_click(action.node, click, kind)
                }
                (None, _) => false,
            };
        }
        if changed {
            self.rebuild()?;
        }
        Ok(())
    }

    /// Takes a call history fetched elsewhere (the window's bus thread at start).
    pub fn set_log(
        &mut self,
        log: Vec<ultar_bus::messages::call::LogEntry>,
    ) -> Result<(), AppError> {
        self.model.log = log;
        self.rebuild()
    }

    /// Fetches the history again (the service owns it).
    pub(crate) fn reload_log(&mut self) {
        if let Ok(log) = self.port.log(false) {
            self.model.log = log;
        }
    }

    /// Shows a toast (kept across rebuilds until it closes).
    pub(crate) fn notify(&mut self, tone: Tone, label: Label) {
        self.toast = Some((tone, label));
    }

    /// Shows a danger toast with the text of `key` (platform errors of the window glue).
    pub fn notify_error(&mut self, key: &str) {
        self.notify(Tone::Danger, Label::key(key));
        // The toast is part of the tree: a failed rebuild keeps the last one on screen.
        let _ = self.rebuild();
    }

    /// Shows the error of a failed request.
    pub(crate) fn report(&mut self, e: PortError) {
        let key = match e {
            PortError::Call(CallError::Unavailable(f)) => failure_key(f),
            PortError::Call(CallError::Unsupported) => "phone.error.unsupported",
            PortError::Unreachable => "phone.error.service",
            PortError::Call(_) => "phone.error.generic",
        };
        self.notify(Tone::Danger, Label::key(key));
    }

    /// Rebuilds the tree from the model; focus and hover stay where they were.
    pub(crate) fn rebuild(&mut self) -> Result<(), AppError> {
        let had_focus = self.ui.focused();
        let focused = had_focus
            .and_then(|f| self.built.clicks.iter().find(|(id, _)| *id == f))
            .map(|(_, c)| *c);
        // What scrolls takes the focus itself (the dial pad in a small window, a long history)
        // and is no click target: its place in the Tab order finds it again.
        let place = match (had_focus, focused) {
            (Some(f), None) => self.ui.focus_order().iter().position(|id| *id == f),
            _ => None,
        };
        // The dial pad keeps its scroll position across rebuilds (every key press rebuilds).
        let pad = self.built.pad.and_then(|p| self.ui.scroll_offset(p).ok());
        self.built = view::build(&mut self.ui, &self.model)?;
        self.show_pictures()?;
        self.ui.layout();
        if let Some(c) = focused {
            if let Some((id, _)) = self.built.clicks.iter().find(|(_, x)| *x == c) {
                // A control that became disabled cannot keep the focus; that is fine.
                let _ = self.ui.focus(*id);
            }
        } else if let Some(id) = place.and_then(|i| self.ui.focus_order().get(i).copied()) {
            let _ = self.ui.focus(id);
        }
        // The pad stays where the user scrolled it: giving the focus back must not bring the
        // focused key into view again (it is a rebuild, not a step of the keyboard).
        if let (Some(offset), Some(id)) = (pad, self.built.pad) {
            if self.ui.set_scroll_offset(id, offset).is_ok() {
                self.ui.layout();
            }
        }
        if let Some(pos) = self.pointer {
            self.ui.handle(Event::PointerMove { pos });
        }
        if let Some((tone, label)) = self.toast.clone() {
            self.ui.show_toast(Toast::new(TOAST, tone, label))?;
        }
        Ok(())
    }

    /// Writes the duration of the shown call into its timer (no rebuild).
    fn update_timer(&mut self) -> Result<(), AppError> {
        let (Some(node), crate::model::Screen::InCall(id)) =
            (self.built.timer, self.model.screen())
        else {
            return Ok(());
        };
        if let Some(call) = self.model.call(id).filter(|c| c.phase == Phase::Active) {
            let text = crate::model::format_duration(self.model.duration_s(call));
            self.ui.set_label(node, Label::data(&text))?;
        }
        Ok(())
    }

    /// The toast closed by itself or by the user.
    pub(crate) fn toast_closed(&mut self) -> bool {
        self.toast = None;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_display() {
        let e = AppError::from(UiError::TooManyNodes);
        assert!(e.to_string().starts_with("toolkit"));
    }
}
