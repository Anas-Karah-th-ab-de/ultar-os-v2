//! The messages of the engine-host protocol (kinds 1–16 commands, 101–114 events).

use super::codec::{Reader, Writer};
use super::{DecodeError, MAX_SIDE};
use crate::engine::{
    DownloadId, EngineEvent, FrameDamage, KeyInput, PermissionKind, PointerInput, RequestId, TabId,
};

/// Version the host announces in [`HostEvent::Ready`].
pub const PROTOCOL_VERSION: u16 = 1;

/// A command from the browser to the host.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Make a tab (`private`: ephemeral session).
    CreateTab(TabId, bool),
    /// Close a tab.
    CloseTab(TabId),
    /// Load an address.
    Load(TabId, String),
    /// History back.
    Back(TabId),
    /// History forward.
    Forward(TabId),
    /// Reload.
    Reload(TabId),
    /// Stop loading.
    Stop(TabId),
    /// Page size in device pixels and scale.
    Resize(TabId, u32, u32, f32),
    /// Show or hide.
    Visible(TabId, bool),
    /// Pointer input.
    Pointer(TabId, PointerInput),
    /// Key input.
    Key(TabId, KeyInput),
    /// Tracking protection on/off for the tab.
    Protection(TabId, bool),
    /// Answer to a permission request.
    Permission(RequestId, bool),
    /// Cancel a download.
    CancelDownload(DownloadId),
    /// The browser has read the tab's frame; the host may render the next one.
    FrameDone(TabId),
    /// End the host.
    Quit,
}

fn pointer_fields(p: PointerInput) -> (u8, u8, f32, f32, f32, f32) {
    match p {
        PointerInput::Move { x, y } => (0, 0, x, y, 0.0, 0.0),
        PointerInput::Down { x, y, button } => (1, button, x, y, 0.0, 0.0),
        PointerInput::Up { x, y, button } => (2, button, x, y, 0.0, 0.0),
        PointerInput::Scroll { x, y, dx, dy } => (3, 0, x, y, dx, dy),
    }
}

impl Command {
    /// The framed bytes.
    pub fn encode(&self) -> Vec<u8> {
        let tab = |k: u8, t: TabId| Writer::new(k).u32(t.0);
        match self {
            Command::CreateTab(t, p) => tab(1, *t).u8(u8::from(*p)),
            Command::CloseTab(t) => tab(2, *t),
            Command::Load(t, url) => tab(3, *t).str(url),
            Command::Back(t) => tab(4, *t),
            Command::Forward(t) => tab(5, *t),
            Command::Reload(t) => tab(6, *t),
            Command::Stop(t) => tab(7, *t),
            Command::Resize(t, w, h, s) => tab(8, *t).u32(*w).u32(*h).f32(*s),
            Command::Visible(t, v) => tab(9, *t).u8(u8::from(*v)),
            Command::Pointer(t, p) => {
                let (k, b, x, y, dx, dy) = pointer_fields(*p);
                tab(10, *t).u8(k).u8(b).f32(x).f32(y).f32(dx).f32(dy)
            }
            Command::Key(t, k) => tab(11, *t)
                .u16(k.code)
                .u32(k.character)
                .u8(k.modifiers)
                .u8(u8::from(k.pressed)),
            Command::Protection(t, on) => tab(12, *t).u8(u8::from(*on)),
            Command::Permission(r, a) => Writer::new(13).u32(r.0).u8(u8::from(*a)),
            Command::CancelDownload(d) => Writer::new(14).u32(d.0),
            Command::FrameDone(t) => tab(15, *t),
            Command::Quit => Writer::new(16),
        }
        .finish()
    }
}

/// An event from the host.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    /// The host is up and speaks this protocol version.
    Ready(u16),
    /// A new frame buffer for a tab (the descriptor comes with the message):
    /// width, height, stride in bytes.
    Buffer(TabId, u32, u32, u32),
    /// Everything else maps to an engine event.
    Engine(EngineEvent),
}

impl HostEvent {
    /// Decodes the payload of message `kind`.
    pub fn decode(kind: u8, payload: &[u8]) -> Result<HostEvent, DecodeError> {
        let mut r = Reader::new(payload);
        let event = match kind {
            101 => HostEvent::Ready(r.u16()?),
            106 => {
                let (tab, w, h, stride) = (TabId(r.u32()?), r.u32()?, r.u32()?, r.u32()?);
                let valid = (1..=MAX_SIDE).contains(&w)
                    && (1..=MAX_SIDE).contains(&h)
                    && u64::from(stride) >= u64::from(w) * 4
                    && stride <= MAX_SIDE * 4 + 64;
                if !valid {
                    return Err(DecodeError::BadValue);
                }
                HostEvent::Buffer(tab, w, h, stride)
            }
            _ => HostEvent::Engine(decode_engine(kind, &mut r)?),
        };
        r.end()?;
        Ok(event)
    }

    /// The framed bytes (used by tests and by in-process hosts).
    pub fn encode(&self) -> Vec<u8> {
        match self {
            HostEvent::Ready(v) => Writer::new(101).u16(*v).finish(),
            HostEvent::Buffer(t, w, h, s) => {
                Writer::new(106).u32(t.0).u32(*w).u32(*h).u32(*s).finish()
            }
            HostEvent::Engine(e) => encode_engine(e).finish(),
        }
    }
}

/// Kinds 107 (frame, checked damage) and 108 (permission request).
fn decode_frame_or_permission(kind: u8, r: &mut Reader<'_>) -> Result<EngineEvent, DecodeError> {
    Ok(match kind {
        107 => {
            let tab = TabId(r.u32()?);
            let damage = FrameDamage {
                x0: r.u32()?,
                y0: r.u32()?,
                x1: r.u32()?,
                y1: r.u32()?,
            };
            if damage.x0 >= damage.x1
                || damage.y0 >= damage.y1
                || damage.x1 > MAX_SIDE
                || damage.y1 > MAX_SIDE
            {
                return Err(DecodeError::BadValue);
            }
            EngineEvent::Frame { tab, damage }
        }
        108 => {
            let (tab, request) = (TabId(r.u32()?), RequestId(r.u32()?));
            let kind = PermissionKind::from_code(r.u8()?).ok_or(DecodeError::BadValue)?;
            EngineEvent::Permission {
                tab,
                request,
                kind,
                host: r.str()?,
            }
        }
        k => return Err(DecodeError::UnknownKind(k)),
    })
}

fn decode_engine(kind: u8, r: &mut Reader<'_>) -> Result<EngineEvent, DecodeError> {
    Ok(match kind {
        102 => {
            let tab = TabId(r.u32()?);
            let (can_back, can_forward) = (r.flag()?, r.flag()?);
            EngineEvent::Navigated {
                tab,
                url: r.str()?,
                can_back,
                can_forward,
            }
        }
        103 => EngineEvent::Title {
            tab: TabId(r.u32()?),
            title: r.str()?,
        },
        104 => {
            let (tab, loading, progress) = (TabId(r.u32()?), r.flag()?, r.f32()?);
            if !(0.0..=1.0).contains(&progress) {
                return Err(DecodeError::BadValue);
            }
            EngineEvent::Progress {
                tab,
                loading,
                progress,
            }
        }
        105 => EngineEvent::Security {
            tab: TabId(r.u32()?),
            secure: r.flag()?,
        },
        107 | 108 => decode_frame_or_permission(kind, r)?,
        109 => EngineEvent::Blocked {
            tab: TabId(r.u32()?),
            count: r.u32()?,
        },
        110 => EngineEvent::DownloadStarted {
            download: DownloadId(r.u32()?),
            url: r.str()?,
            name: r.str()?,
        },
        111 => EngineEvent::DownloadProgress {
            download: DownloadId(r.u32()?),
            received: r.u64()?,
            total: r.u64()?,
        },
        112 => EngineEvent::DownloadFinished {
            download: DownloadId(r.u32()?),
            ok: r.flag()?,
        },
        113 => EngineEvent::NewTab {
            opener: TabId(r.u32()?),
            url: r.str()?,
        },
        114 => EngineEvent::Crashed {
            tab: TabId(r.u32()?),
        },
        k => return Err(DecodeError::UnknownKind(k)),
    })
}

fn encode_engine(e: &EngineEvent) -> Writer {
    match e {
        EngineEvent::Navigated {
            tab,
            url,
            can_back,
            can_forward,
        } => Writer::new(102)
            .u32(tab.0)
            .u8(u8::from(*can_back))
            .u8(u8::from(*can_forward))
            .str(url),
        EngineEvent::Title { tab, title } => Writer::new(103).u32(tab.0).str(title),
        EngineEvent::Progress {
            tab,
            loading,
            progress,
        } => Writer::new(104)
            .u32(tab.0)
            .u8(u8::from(*loading))
            .f32(*progress),
        EngineEvent::Security { tab, secure } => Writer::new(105).u32(tab.0).u8(u8::from(*secure)),
        EngineEvent::Frame { tab, damage: d } => Writer::new(107)
            .u32(tab.0)
            .u32(d.x0)
            .u32(d.y0)
            .u32(d.x1)
            .u32(d.y1),
        EngineEvent::Permission {
            tab,
            request,
            host,
            kind,
        } => Writer::new(108)
            .u32(tab.0)
            .u32(request.0)
            .u8(kind.code())
            .str(host),
        EngineEvent::Blocked { tab, count } => Writer::new(109).u32(tab.0).u32(*count),
        EngineEvent::DownloadStarted {
            download,
            url,
            name,
        } => Writer::new(110).u32(download.0).str(url).str(name),
        EngineEvent::DownloadProgress {
            download,
            received,
            total,
        } => Writer::new(111).u32(download.0).u64(*received).u64(*total),
        EngineEvent::DownloadFinished { download, ok } => {
            Writer::new(112).u32(download.0).u8(u8::from(*ok))
        }
        EngineEvent::NewTab { opener, url } => Writer::new(113).u32(opener.0).str(url),
        EngineEvent::Crashed { tab } => Writer::new(114).u32(tab.0),
    }
}
