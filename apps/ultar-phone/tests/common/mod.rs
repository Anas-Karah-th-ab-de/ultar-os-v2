//! Shared parts of the phone tests: a recording call port, a recording host, the board data.
#![allow(dead_code)]

use ultar_bus::messages::call::{
    Audio, CallError, CallInfo, CallState, Cellular, Direction, Lines, LogEntry, Media, Output,
    Phase, Ultar, Way,
};
use ultar_phone::port::{CallPort, Host, PortError, Request};
use ultar_phone::{Look, PhoneApp, WindowOp};
use ultar_ui::event::{Event, PointerButton};
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::NodeId;
use ultar_ui::{Lang, Size};

/// Board size (1280 × 800) and phone size (360 × 720).
pub const WIDE: Size = Size {
    w: 1280.0,
    h: 800.0,
};
pub const PHONE: Size = Size { w: 360.0, h: 720.0 };

#[derive(Default)]
pub struct FakePort {
    pub sent: Vec<Request>,
    pub fail_next: Option<PortError>,
    pub log: Vec<LogEntry>,
    pub next_id: u32,
}

impl CallPort for FakePort {
    fn send(&mut self, request: Request) -> Result<Option<u32>, PortError> {
        if let Some(e) = self.fail_next.take() {
            return Err(e);
        }
        let dial = matches!(request, Request::Dial(_));
        self.sent.push(request);
        if dial {
            self.next_id += 1;
            return Ok(Some(self.next_id));
        }
        Ok(None)
    }

    fn log(&mut self, missed_only: bool) -> Result<Vec<LogEntry>, PortError> {
        Ok(self
            .log
            .iter()
            .filter(|e| !missed_only || e.missed)
            .cloned()
            .collect())
    }
}

#[derive(Default)]
pub struct FakeHost {
    pub copied: Vec<String>,
    pub window: Vec<WindowOp>,
    pub settings: usize,
    pub contacts: Vec<String>,
    pub refuse_copy: bool,
}

impl Host for FakeHost {
    fn copy_text(&mut self, text: &str) -> bool {
        self.copied.push(text.to_owned());
        !self.refuse_copy
    }
    fn window(&mut self, op: WindowOp) {
        self.window.push(op);
    }
    fn open_settings(&mut self) {
        self.settings += 1;
    }
    fn create_contact(&mut self, number: &str) {
        self.contacts.push(number.to_owned());
    }
}

pub type App = PhoneApp<FakePort, FakeHost>;

/// 2025-11-22 16:52 in Damascus (UTC+3), the boards' clock.
pub const NOW_S: i64 = 1_763_819_520;

pub fn entry(
    id: u32,
    name: Option<&str>,
    number: &str,
    way: Way,
    dir: Direction,
    missed: bool,
    ago_s: i64,
) -> LogEntry {
    LogEntry {
        id,
        way,
        direction: dir,
        missed,
        number: number.into(),
        name: name.map(str::to_owned),
        when_s: NOW_S - ago_s,
    }
}

/// The history of board PhoneDial.
pub fn board_log() -> Vec<LogEntry> {
    let h = 3600;
    vec![
        entry(
            5,
            Some("ليلى حداد"),
            "UOS-4821-7730",
            Way::Ultar,
            Direction::Incoming,
            true,
            h / 2,
        ),
        entry(
            4,
            Some("مكتب KM Code Craft"),
            "+963 11 555 0100",
            Way::Cellular(1),
            Direction::Outgoing,
            false,
            6 * h,
        ),
        entry(
            3,
            None,
            "UOS-2290-1184",
            Way::Ultar,
            Direction::Incoming,
            false,
            21 * h,
        ),
        entry(
            2,
            None,
            "+963 11 555 0142",
            Way::Cellular(1),
            Direction::Incoming,
            false,
            50 * h,
        ),
        entry(
            1,
            Some("سامر"),
            "UOS-1000-2000",
            Way::Ultar,
            Direction::Outgoing,
            false,
            74 * h,
        ),
    ]
}

pub fn ready() -> CallState {
    CallState {
        lines: Lines {
            cellular: Cellular::Ready,
            sims: vec![1],
            ultar: Ultar::Ready,
            own_number: Some("UOS-1111-2222".into()),
        },
        audio: Audio {
            muted: false,
            output: Output::Earpiece,
            outputs: vec![Output::Earpiece, Output::Speaker, Output::Bluetooth],
        },
        clock_ms: 200_000,
        ..CallState::default()
    }
}

pub fn call(id: u32, phase: Phase, video: bool) -> CallInfo {
    CallInfo {
        id,
        way: Way::Ultar,
        direction: Direction::Incoming,
        number: "UOS-4821-7730".into(),
        name: Some("ليلى حداد".into()),
        phase,
        media: Media {
            video_call: video,
            camera_on: video,
            camera: 0,
            cameras: 2,
            sharing: false,
        },
        encrypted: true,
        active_since_ms: Some(200_000 - 134_000),
    }
}

pub fn with_calls(calls: Vec<CallInfo>) -> CallState {
    CallState { calls, ..ready() }
}

pub fn app_with(lang: Lang, theme: ThemeKind, size: Size, state: CallState) -> App {
    let port = FakePort {
        log: board_log(),
        ..FakePort::default()
    };
    let look = Look {
        lang,
        theme,
        mode: InputMode::Pointer,
        size,
    };
    let mut app = PhoneApp::new(look, state.clone(), port, FakeHost::default()).unwrap();
    app.set_state(state, 200_000).unwrap();
    app.set_utc_offset(3 * 3600).unwrap();
    app.tick(200_000, NOW_S).unwrap();
    app
}

pub fn app(state: CallState) -> App {
    app_with(Lang::Ar, ThemeKind::Dark, WIDE, state)
}

/// Nodes of one action name, in tree order.
pub fn nodes(app: &App, action: &str) -> Vec<NodeId> {
    app.clicks()
        .iter()
        .filter(|(_, c)| c.action() == Some(action))
        .map(|(id, _)| *id)
        .collect()
}

pub fn node(app: &App, action: &str) -> NodeId {
    nodes(app, action)
        .first()
        .copied()
        .unwrap_or_else(|| panic!("no node for {action}"))
}

/// Pointer click on the middle of `id`.
pub fn click(app: &mut App, id: NodeId) {
    app.ui().layout();
    let pos = app.ui().rect(id).expect("shown").center();
    app.handle(Event::PointerMove { pos }).unwrap();
    app.handle(Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    })
    .unwrap();
    app.handle(Event::PointerUp {
        pos,
        button: PointerButton::Primary,
    })
    .unwrap();
}

pub fn click_action(app: &mut App, action: &str) {
    let id = node(app, action);
    click(app, id);
}

/// Accessible names of all nodes.
pub fn names(app: &mut App) -> Vec<String> {
    let tree = app.ui().a11y_tree();
    let tr = app.ui().translator();
    tree.iter().map(|n| n.describe(tr)).collect()
}

pub fn sent_error() -> PortError {
    PortError::Call(CallError::Unavailable(
        ultar_bus::messages::call::Failure::NoNetwork,
    ))
}

/// The dial pad key `c`.
pub fn digit(app: &App, c: char) -> NodeId {
    app.clicks()
        .iter()
        .find(|(_, k)| *k == ultar_phone::Click::Digit(c))
        .map(|(id, _)| *id)
        .expect("key shown")
}

/// Rectangle of the backspace inside the number field (`phone.backspace`).
pub fn backspace_rect(app: &mut App) -> ultar_ui::geom::Rect {
    let field = app.built().number.expect("number field");
    app.ui()
        .end_action_rect(field)
        .unwrap()
        .expect("backspace shown")
}

/// Clicks the backspace inside the number field.
pub fn click_backspace(app: &mut App) {
    let pos = backspace_rect(app).center();
    app.handle(Event::PointerMove { pos }).unwrap();
    app.handle(Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    })
    .unwrap();
    app.handle(Event::PointerUp {
        pos,
        button: PointerButton::Primary,
    })
    .unwrap();
}

/// True when the node is announced as pressed (toggle on).
pub fn pressed(app: &mut App, id: NodeId) -> bool {
    app.ui()
        .a11y_tree()
        .iter()
        .any(|n| n.id == id && n.part.is_none() && n.state.pressed)
}
