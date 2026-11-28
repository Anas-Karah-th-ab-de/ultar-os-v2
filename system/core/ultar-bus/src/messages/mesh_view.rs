//! Contract of the mesh for remote view (mesh item C1): see and operate the screen of a
//! paired device.
//!
//! Three parties talk to the mesh service:
//! * the **viewer app** (`ultar-remote`): `mesh.view_start`, `mesh.view_input`,
//!   `mesh.view_stop`, and it follows the topic `mesh.view` (waiting, running, picture
//!   pieces, end);
//! * the **screen service** of the viewed device (`ultar-screend`, the only caller of
//!   `mesh.view_ready` and `mesh.view_picture`): follows `mesh.view_serve` (a session
//!   begins, the viewer's input, the end) and sends the damaged rectangles of its screen;
//! * the **shell**: follows the retained `mesh.view_state` for its indicator and ends a
//!   session with `mesh.view_stop` (the person on the source can always end it).
//!
//! The viewed device decides: only a paired device with the right "control" may ask, and
//! the person on the viewed device says yes in the system dialog first.

use crate::message::{Call, Message, Topic};
use crate::messages::mesh::SERVICE;
use serde::{Deserialize, Serialize};

/// Longest side of a picture (8K), as the screenshot service.
pub const MAX_SIDE: u32 = 8192;
/// Longest word of one input event.
pub const MAX_INPUT_WORD: usize = 64;

/// A mouse button.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    /// Main button.
    Left,
    /// Secondary button.
    Right,
    /// Wheel button.
    Middle,
}

/// One event of the viewer for the viewed device, in pixels of the viewed screen.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemoteInput {
    /// The pointer moved.
    Move {
        /// Column.
        x: u32,
        /// Row.
        y: u32,
    },
    /// A button went down or up.
    Button {
        /// Column.
        x: u32,
        /// Row.
        y: u32,
        /// Which.
        button: Button,
        /// Down (`true`) or up.
        down: bool,
    },
    /// The wheel turned (positive: further down).
    Wheel {
        /// Column.
        x: u32,
        /// Row.
        y: u32,
        /// Notches.
        lines: i16,
    },
    /// A key (Linux key code 1..=255) went down or up.
    Key {
        /// The code.
        code: u16,
        /// Down (`true`) or up.
        down: bool,
    },
}

fn letter(b: Button) -> char {
    match b {
        Button::Left => 'l',
        Button::Right => 'r',
        Button::Middle => 'm',
    }
}

fn button(word: &str) -> Option<Button> {
    match word {
        "l" => Some(Button::Left),
        "r" => Some(Button::Right),
        "m" => Some(Button::Middle),
        _ => None,
    }
}

fn pos(x: &str, y: &str) -> Option<(u32, u32)> {
    let (x, y) = (x.parse::<u32>().ok()?, y.parse::<u32>().ok()?);
    (x < MAX_SIDE && y < MAX_SIDE).then_some((x, y))
}

impl RemoteInput {
    /// The short word that travels over the mesh: `m X Y`, `d X Y B`, `u X Y B`,
    /// `w X Y N`, `k CODE down|up` (B is `l`, `r` or `m`).
    pub fn word(self) -> String {
        match self {
            RemoteInput::Move { x, y } => format!("m {x} {y}"),
            RemoteInput::Button { x, y, button, down } => {
                format!(
                    "{} {x} {y} {}",
                    if down { 'd' } else { 'u' },
                    letter(button)
                )
            }
            RemoteInput::Wheel { x, y, lines } => format!("w {x} {y} {lines}"),
            RemoteInput::Key { code, down } => {
                format!("k {code} {}", if down { "down" } else { "up" })
            }
        }
    }

    /// Reads a word back; `None` for anything that is not exactly one valid event (the
    /// source never acts on a word it does not understand).
    pub fn parse(word: &str) -> Option<RemoteInput> {
        if word.len() > MAX_INPUT_WORD {
            return None;
        }
        let parts: Vec<&str> = word.split(' ').collect();
        match parts.as_slice() {
            ["m", x, y] => pos(x, y).map(|(x, y)| RemoteInput::Move { x, y }),
            [d @ ("d" | "u"), x, y, b] => {
                let (x, y) = pos(x, y)?;
                Some(RemoteInput::Button {
                    x,
                    y,
                    button: button(b)?,
                    down: *d == "d",
                })
            }
            ["w", x, y, n] => {
                let (x, y) = pos(x, y)?;
                let lines = n.parse::<i16>().ok().filter(|l| l.abs() <= 100)?;
                Some(RemoteInput::Wheel { x, y, lines })
            }
            ["k", code, state @ ("down" | "up")] => {
                let code = code.parse::<u16>().ok().filter(|c| (1..=255).contains(c))?;
                Some(RemoteInput::Key {
                    code,
                    down: *state == "down",
                })
            }
            _ => None,
        }
    }
}

/// Why a session ended (what the viewer app shows).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EndReason {
    /// The other device (or its person) said no, or it has no screen service.
    Refused,
    /// One side stopped it.
    Stopped,
    /// The connection went away.
    Lost,
    /// The viewed device locked its screen.
    Locked,
}

/// `mesh.view` - what the viewer app follows.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum ViewEvent {
    /// The request is sent; the person on the other device has to say yes.
    Waiting {
        /// Name of the viewed device (data).
        name: String,
    },
    /// The session runs; the picture is `width` x `height` pixels.
    Running {
        /// Width.
        width: u32,
        /// Height.
        height: u32,
    },
    /// A rectangle of the picture, raw pixels `B, G, R, X` (four bytes each).
    Picture {
        /// Column of the rectangle.
        x: u32,
        /// Row of the rectangle.
        y: u32,
        /// Width of the rectangle.
        w: u32,
        /// Height of the rectangle.
        h: u32,
        /// `w * h * 4` bytes.
        data: Vec<u8>,
    },
    /// The session is over.
    Ended {
        /// Why.
        reason: EndReason,
    },
}

/// `mesh.view_serve` - what the screen service of the viewed device follows.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum ServeEvent {
    /// The person said yes: capture the screen and call `mesh.view_ready`.
    Begin {
        /// The session.
        id: u64,
        /// Name of the viewer (data).
        viewer: String,
    },
    /// An event of the viewer to inject.
    Input {
        /// The session.
        id: u64,
        /// The event.
        input: RemoteInput,
    },
    /// The session is over: stop capturing, release keys and buttons.
    End {
        /// The session.
        id: u64,
    },
}

/// `mesh.view_state` - who views and who is viewed (retained; the shell draws its coloured
/// frame and Stop button from it).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ViewState {
    /// This device is watched by the device of this name.
    pub watched_by: Option<String>,
    /// This device watches the device of this name.
    pub watching: Option<String>,
}

/// `mesh.view_start` - the viewer asks a paired device to show its screen.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ViewStart {
    /// Id of the device.
    pub device: String,
}

/// Answer to [`ViewStart`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ViewStarted {
    /// The request was sent (the device is paired, reachable and nothing else runs).
    pub ok: bool,
}

/// `mesh.view_input` - the viewer's event for the viewed device.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewInput {
    /// The event.
    pub input: RemoteInput,
}

/// `mesh.view_stop` - ends the session this device watches or serves (the viewer's Stop,
/// and the Stop of the person on the viewed device).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ViewStop {}

/// `mesh.view_ready` - the screen service captured the first picture: the stream may
/// begin. Only the screen service may call it; `0 x 0` refuses.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewReady {
    /// The session of [`ServeEvent::Begin`].
    pub id: u64,
    /// Width of the screen.
    pub width: u32,
    /// Height of the screen.
    pub height: u32,
}

/// `mesh.view_picture` - a damaged rectangle of the viewed screen (screen service only).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ViewPicture {
    /// Column.
    pub x: u32,
    /// Row.
    pub y: u32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
    /// `w * h * 4` bytes, `B, G, R, X`.
    pub data: Vec<u8>,
}

/// Answer to [`ViewPicture`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PictureTaken {
    /// The rectangle was sent. `false`: the link is slow (window full) or no session runs;
    /// the screen service keeps the damage and sends it again later.
    pub taken: bool,
}

/// `mesh.view_locked` - the viewed screen locked: the session ends at once and the viewer
/// is told why (screen service only).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ViewLocked {}

/// Plain answer of the calls without data.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ViewDone {
    /// It was done.
    pub ok: bool,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    ViewEvent,
    ServeEvent,
    ViewState,
    ViewStart,
    ViewStarted,
    ViewInput,
    ViewStop,
    ViewLocked,
    ViewReady,
    ViewPicture,
    PictureTaken,
    ViewDone
);

impl Topic for ViewEvent {
    const TOPIC: &'static str = "mesh.view";
    const RETAINED: bool = false;
}
impl Topic for ServeEvent {
    const TOPIC: &'static str = "mesh.view_serve";
    const RETAINED: bool = false;
}
impl Topic for ViewState {
    const TOPIC: &'static str = "mesh.view_state";
    const RETAINED: bool = true;
}

macro_rules! call {
    ($t:ty, $m:expr, $r:ty) => {
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    };
}
call!(ViewStart, "view_start", ViewStarted);
call!(ViewInput, "view_input", ViewDone);
call!(ViewStop, "view_stop", ViewDone);
call!(ViewLocked, "view_locked", ViewDone);
call!(ViewReady, "view_ready", ViewDone);
call!(ViewPicture, "view_picture", PictureTaken);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn input_words_round_trip() {
        let all = [
            RemoteInput::Move { x: 0, y: 8191 },
            RemoteInput::Button {
                x: 10,
                y: 20,
                button: Button::Left,
                down: true,
            },
            RemoteInput::Button {
                x: 1,
                y: 2,
                button: Button::Right,
                down: false,
            },
            RemoteInput::Button {
                x: 1,
                y: 2,
                button: Button::Middle,
                down: true,
            },
            RemoteInput::Wheel {
                x: 5,
                y: 6,
                lines: -3,
            },
            RemoteInput::Key {
                code: 42,
                down: true,
            },
            RemoteInput::Key {
                code: 255,
                down: false,
            },
        ];
        for input in all {
            assert_eq!(RemoteInput::parse(&input.word()), Some(input), "{input:?}");
        }
        assert_eq!(RemoteInput::Move { x: 3, y: 4 }.word(), "m 3 4");
        assert_eq!(
            RemoteInput::Key {
                code: 42,
                down: true
            }
            .word(),
            "k 42 down"
        );
    }

    #[test]
    fn malformed_words_are_never_understood() {
        for bad in [
            "",
            "m",
            "m 1",
            "m 1 2 3",
            "m -1 2",
            "m 8192 0",
            "m a b",
            "x 1 2",
            "d 1 2 x",
            "d 1 2",
            "w 1 2 101",
            "w 1 2 x",
            "k 0 down",
            "k 256 down",
            "k 5 sideways",
            "k 5",
            " m 1 2",
            "m  1 2",
            "m 1 2 ",
            "M 1 2",
            "k 1e1 down",
        ] {
            assert_eq!(RemoteInput::parse(bad), None, "{bad:?}");
        }
        assert_eq!(
            RemoteInput::parse(&format!("m 1 {}", "9".repeat(100))),
            None
        );
    }

    #[test]
    fn messages_round_trip() {
        let e = ViewEvent::Picture {
            x: 1,
            y: 2,
            w: 1,
            h: 1,
            data: vec![1, 2, 3, 4],
        };
        assert_eq!(decode::<ViewEvent>(&encode(&e).unwrap()).unwrap(), e);
        let s = ServeEvent::Input {
            id: 4,
            input: RemoteInput::Move { x: 1, y: 1 },
        };
        assert_eq!(decode::<ServeEvent>(&encode(&s).unwrap()).unwrap(), s);
        let p = ViewPicture {
            x: 0,
            y: 0,
            w: 2,
            h: 1,
            data: vec![0; 8],
        };
        assert_eq!(decode::<ViewPicture>(&encode(&p).unwrap()).unwrap(), p);
        assert_eq!(
            (ViewPicture::SERVICE, ViewPicture::METHOD),
            ("mesh", "view_picture")
        );
        assert_eq!(ViewState::TOPIC, "mesh.view_state");
        const { assert!(ViewState::RETAINED) };
    }
}
