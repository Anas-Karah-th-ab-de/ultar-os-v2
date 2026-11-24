//! Every clickable element of the phone and its action name in `docs/ui/CLICKS.md`.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use ultar_bus::messages::call::Output;

/// A window operation the platform carries out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowOp {
    /// `win.min`.
    Minimize,
    /// `win.max`.
    Maximize,
    /// `win.close`.
    Close,
}

/// What a click on an element of the phone means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Click {
    /// `account.copy`.
    CopyNumber,
    /// `win.min` / `win.max` / `win.close`.
    Window(WindowOp),
    /// `phone.way` (0 = Ultar number, 1 = mobile network).
    Way,
    /// `phone.number`.
    Number,
    /// `phone.backspace`.
    Backspace,
    /// `phone.suggest`.
    Suggest,
    /// `phone.digit` (the key).
    Digit(char),
    /// `phone.call.video`.
    CallVideo,
    /// `phone.call`.
    Call,
    /// `phone.filter`.
    Filter,
    /// `phone.settings`.
    Settings,
    /// `phone.callback` (history entry id).
    Callback(u32),
    /// `phone.entry.more` (history entry id).
    EntryMore(u32),
    /// `phone.accept` (answer; another call is held).
    Accept(u32),
    /// `phone.decline`.
    Decline(u32),
    /// `phone.decline.msg`.
    DeclineMessage(u32),
    /// Answer a waiting call and end the current one (second incoming call).
    AcceptEnd(u32),
    /// `phone.mute`.
    Mute,
    /// `phone.output`.
    Output,
    /// `phone.keypad`.
    Keypad,
    /// `phone.hold`.
    Hold(u32),
    /// Swap the active and the held call (resume the held one).
    Swap(u32),
    /// `phone.video`.
    Video(u32),
    /// `phone.camera`.
    Camera(u32),
    /// `phone.share`.
    Share(u32),
    /// `phone.hangup`.
    Hangup(u32),
    /// "Call again" on the notice of an ended call.
    Again,
    /// Compact page tabs (dial pad / history).
    Page,
    /// A ready message of the "decline with message" dialog (index).
    Text(usize),
    /// Confirms screen sharing.
    ShareConfirm(u32),
    /// Closes the open dialog or sheet.
    CloseDialog,
}

impl Click {
    /// Action name of `docs/ui/CLICKS.md` (`None` for elements the boards do not show).
    pub fn action(self) -> Option<&'static str> {
        Some(match self {
            Click::CopyNumber => "account.copy",
            Click::Window(WindowOp::Minimize) => "win.min",
            Click::Window(WindowOp::Maximize) => "win.max",
            Click::Window(WindowOp::Close) => "win.close",
            Click::Way => "phone.way",
            Click::Number => "phone.number",
            Click::Backspace => "phone.backspace",
            Click::Suggest => "phone.suggest",
            Click::Digit(_) => "phone.digit",
            Click::CallVideo => "phone.call.video",
            Click::Call => "phone.call",
            Click::Filter => "phone.filter",
            Click::Settings => "phone.settings",
            Click::Callback(_) => "phone.callback",
            Click::EntryMore(_) => "phone.entry.more",
            Click::Accept(_) => "phone.accept",
            Click::Decline(_) => "phone.decline",
            Click::DeclineMessage(_) => "phone.decline.msg",
            Click::Mute => "phone.mute",
            Click::Output => "phone.output",
            Click::Keypad => "phone.keypad",
            Click::Hold(_) => "phone.hold",
            Click::Video(_) => "phone.video",
            Click::Camera(_) => "phone.camera",
            Click::Share(_) => "phone.share",
            Click::Hangup(_) => "phone.hangup",
            _ => return None,
        })
    }
}

/// Menu item identifiers of the history entry menu.
pub mod menu {
    /// Create contact.
    pub const CONTACT: u32 = 1;
    /// Block number.
    pub const BLOCK: u32 = 2;
    /// Delete entry.
    pub const DELETE: u32 = 3;
    /// Delete the whole history.
    pub const CLEAR: u32 = 4;
    /// First identifier of the output menu (plus the output's index in [`super::OUTPUTS`]).
    pub const OUTPUT: u32 = 10;
}

/// Outputs in menu order.
pub const OUTPUTS: [Output; 4] = [
    Output::Earpiece,
    Output::Speaker,
    Output::Headset,
    Output::Bluetooth,
];

/// The keys of the dial pad in board order.
pub const KEYS: [char; 12] = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '*', '0', '#'];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_board_action_has_a_click() {
        let names: Vec<_> = [
            Click::CopyNumber,
            Click::Window(WindowOp::Minimize),
            Click::Window(WindowOp::Maximize),
            Click::Window(WindowOp::Close),
            Click::Way,
            Click::Number,
            Click::Backspace,
            Click::Suggest,
            Click::Digit('1'),
            Click::CallVideo,
            Click::Call,
            Click::Filter,
            Click::Settings,
            Click::Callback(1),
            Click::EntryMore(1),
            Click::Accept(1),
            Click::Decline(1),
            Click::DeclineMessage(1),
            Click::Mute,
            Click::Output,
            Click::Keypad,
            Click::Hold(1),
            Click::Video(1),
            Click::Camera(1),
            Click::Share(1),
            Click::Hangup(1),
        ]
        .iter()
        .filter_map(|c| c.action())
        .collect();
        assert_eq!(names.len(), 26);
        assert_eq!(Click::Again.action(), None);
        assert_eq!(Click::Swap(1).action(), None);
        assert_eq!(KEYS.len(), 12);
    }
}
