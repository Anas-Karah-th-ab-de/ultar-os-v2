//! The engine interface: the browser core talks to a web engine only through [`WebEngine`].
//! Engines: [`remote::RemoteEngine`] (WPE WebKit in its own cell) and
//! [`test_engine::TestEngine`] (headless tests). A Servo engine (DESKTOP-SPEC §11) can
//! implement the same trait.

pub mod remote;
pub mod test_engine;

use std::fmt;
use std::os::fd::BorrowedFd;
use std::time::Duration;

/// A tab of the engine (one web view).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u32);

/// A pending permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub u32);

/// A download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DownloadId(pub u32);

/// What a site may ask for (DESKTOP-SPEC §8 "Apps": the rights of a page).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PermissionKind {
    /// The camera.
    Camera,
    /// The microphone.
    Microphone,
    /// The location.
    Location,
    /// Showing notifications.
    Notifications,
}

impl PermissionKind {
    /// Every kind, in the order of the page panel.
    pub const ALL: [PermissionKind; 4] = [
        PermissionKind::Camera,
        PermissionKind::Microphone,
        PermissionKind::Location,
        PermissionKind::Notifications,
    ];

    /// Wire and file code.
    pub fn code(self) -> u8 {
        match self {
            PermissionKind::Camera => 1,
            PermissionKind::Microphone => 2,
            PermissionKind::Location => 3,
            PermissionKind::Notifications => 4,
        }
    }

    /// The kind with wire code `code`.
    pub fn from_code(code: u8) -> Option<PermissionKind> {
        PermissionKind::ALL.into_iter().find(|k| k.code() == code)
    }

    /// Catalog key of its name.
    pub fn key(self) -> &'static str {
        match self {
            PermissionKind::Camera => "web.perm.camera",
            PermissionKind::Microphone => "web.perm.microphone",
            PermissionKind::Location => "web.perm.location",
            PermissionKind::Notifications => "web.perm.notifications",
        }
    }
}

/// Pointer input for a page, in device pixels of the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerInput {
    /// The pointer moved.
    Move {
        /// Horizontal position.
        x: f32,
        /// Vertical position.
        y: f32,
    },
    /// A button went down (1 primary, 2 middle, 3 secondary).
    Down {
        /// Horizontal position.
        x: f32,
        /// Vertical position.
        y: f32,
        /// Button.
        button: u8,
    },
    /// A button went up.
    Up {
        /// Horizontal position.
        x: f32,
        /// Vertical position.
        y: f32,
        /// Button.
        button: u8,
    },
    /// The wheel turned (pixels, positive = down / towards the end).
    Scroll {
        /// Horizontal position.
        x: f32,
        /// Vertical position.
        y: f32,
        /// Horizontal amount.
        dx: f32,
        /// Vertical amount.
        dy: f32,
    },
}

/// A key for a page: the Linux key code, the character it produces (0 for none) and the
/// modifiers (bits: 1 Shift, 2 Ctrl, 4 Alt, 8 Super).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyInput {
    /// Linux key code (evdev).
    pub code: u16,
    /// Unicode scalar the key produces, 0 for none.
    pub character: u32,
    /// Modifier bits.
    pub modifiers: u8,
    /// `true` for press, `false` for release.
    pub pressed: bool,
}

/// The newest pixels of a tab (BGRX/BGRA, `stride` bytes per row).
#[derive(Debug, Clone, Copy)]
pub struct FrameView<'a> {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes per row.
    pub stride: usize,
    /// The rows.
    pub pixels: &'a [u8],
}

/// A changed part of a frame, device pixels `[x0, y0, x1, y1)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameDamage {
    /// Left edge.
    pub x0: u32,
    /// Top edge.
    pub y0: u32,
    /// Right edge (exclusive).
    pub x1: u32,
    /// Bottom edge (exclusive).
    pub y1: u32,
}

/// What an engine reports.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// The tab shows another address (also after back/forward).
    Navigated {
        /// Tab.
        tab: TabId,
        /// The address.
        url: String,
        /// History behind.
        can_back: bool,
        /// History ahead.
        can_forward: bool,
    },
    /// The page title changed.
    Title {
        /// Tab.
        tab: TabId,
        /// The title (may be empty).
        title: String,
    },
    /// Loading started, progressed or ended.
    Progress {
        /// Tab.
        tab: TabId,
        /// `true` while loading.
        loading: bool,
        /// Share loaded, 0..1.
        progress: f32,
    },
    /// The connection state of the page (TLS without errors).
    Security {
        /// Tab.
        tab: TabId,
        /// `true` for a verified encrypted connection.
        secure: bool,
    },
    /// New pixels; read them with [`WebEngine::frame`].
    Frame {
        /// Tab.
        tab: TabId,
        /// What changed.
        damage: FrameDamage,
    },
    /// The page asks for a right; answer with [`WebEngine::answer_permission`].
    Permission {
        /// Tab.
        tab: TabId,
        /// Request.
        request: RequestId,
        /// Host of the page that asks.
        host: String,
        /// What it asks for.
        kind: PermissionKind,
    },
    /// Requests and elements were blocked by tracking protection (running count of the page).
    Blocked {
        /// Tab.
        tab: TabId,
        /// Blocked on the current page so far.
        count: u32,
    },
    /// A download started (the engine writes into the downloads folder only).
    DownloadStarted {
        /// Download.
        download: DownloadId,
        /// Source address.
        url: String,
        /// File name the engine chose (already made safe by the engine host).
        name: String,
    },
    /// Download progress.
    DownloadProgress {
        /// Download.
        download: DownloadId,
        /// Bytes received.
        received: u64,
        /// Size, 0 when unknown.
        total: u64,
    },
    /// A download ended.
    DownloadFinished {
        /// Download.
        download: DownloadId,
        /// `true` when complete.
        ok: bool,
    },
    /// The page wants a new tab (link with a target, `window.open` after a click).
    NewTab {
        /// The tab that asked.
        opener: TabId,
        /// The address.
        url: String,
    },
    /// The page process of a tab ended unexpectedly.
    Crashed {
        /// Tab.
        tab: TabId,
    },
}

/// Why an engine refused or failed.
#[derive(Debug)]
pub enum EngineError {
    /// No such tab (closed or never made).
    UnknownTab(TabId),
    /// A limit was reached (tabs, message size).
    Limit,
    /// The engine host is gone or speaks out of protocol.
    Disconnected,
    /// Starting the engine host failed.
    Start(std::io::Error),
    /// Input/output with the engine host failed.
    Io(std::io::Error),
    /// The engine host sent something invalid.
    Protocol(crate::ipc::DecodeError),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::UnknownTab(t) => write!(f, "unknown tab {}", t.0),
            EngineError::Limit => f.write_str("engine limit reached"),
            EngineError::Disconnected => f.write_str("engine host disconnected"),
            EngineError::Start(e) => write!(f, "engine host could not start: {e}"),
            EngineError::Io(e) => write!(f, "engine host i/o: {e}"),
            EngineError::Protocol(e) => write!(f, "engine host protocol: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// Most tabs one engine keeps.
pub const MAX_TABS: usize = 64;

/// A web engine as the browser core uses it.
pub trait WebEngine {
    /// Makes a tab; a private tab keeps nothing (cookies, cache, history) after it closes.
    fn create_tab(&mut self, private: bool) -> Result<TabId, EngineError>;
    /// Closes a tab and frees its page.
    fn close_tab(&mut self, tab: TabId) -> Result<(), EngineError>;
    /// Loads an address (already checked by [`crate::url::resolve`]).
    fn load(&mut self, tab: TabId, url: &str) -> Result<(), EngineError>;
    /// Back in the tab's history.
    fn back(&mut self, tab: TabId) -> Result<(), EngineError>;
    /// Forward in the tab's history.
    fn forward(&mut self, tab: TabId) -> Result<(), EngineError>;
    /// Loads the page again.
    fn reload(&mut self, tab: TabId) -> Result<(), EngineError>;
    /// Stops loading.
    fn stop(&mut self, tab: TabId) -> Result<(), EngineError>;
    /// Size of the page area in device pixels and the scale (device pixels per point).
    fn resize(
        &mut self,
        tab: TabId,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Result<(), EngineError>;
    /// Shows (renders) or hides a tab.
    fn set_visible(&mut self, tab: TabId, visible: bool) -> Result<(), EngineError>;
    /// Pointer input for the page.
    fn pointer(&mut self, tab: TabId, input: PointerInput) -> Result<(), EngineError>;
    /// Key input for the page.
    fn key(&mut self, tab: TabId, input: KeyInput) -> Result<(), EngineError>;
    /// Tracking protection on or off for the tab's current site (exception list).
    fn set_protection(&mut self, tab: TabId, on: bool) -> Result<(), EngineError>;
    /// Answers a permission request.
    fn answer_permission(&mut self, request: RequestId, allow: bool) -> Result<(), EngineError>;
    /// Cancels a running download (the partial file is removed).
    fn cancel_download(&mut self, download: DownloadId) -> Result<(), EngineError>;
    /// Waits up to `timeout` for events and returns them (empty on timeout).
    fn poll(&mut self, timeout: Option<Duration>) -> Result<Vec<EngineEvent>, EngineError>;
    /// The newest pixels of a tab.
    fn frame(&self, tab: TabId) -> Option<FrameView<'_>>;
    /// A descriptor that becomes readable when [`WebEngine::poll`] has events (the window
    /// waits on it together with the compositor); `None` for engines without one.
    fn wake_fd(&self) -> Option<BorrowedFd<'_>> {
        None
    }
}
