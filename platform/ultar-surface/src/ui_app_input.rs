//! What an app asks of its window for itself, not for a widget: the clipboard text for its
//! own text (the editor), and the pointer over pictures it draws itself.

use super::{UiWindow, MAX_UNHANDLED, TEXT_MIME};
use crate::client::ClientError;
use crate::proto::Request;
use ultar_ui::event::Event as UiEvent;

/// The app's own input.
#[derive(Default)]
pub(super) struct AppInput {
    /// The paste in progress was asked for by the app ([`UiWindow::request_paste`]).
    pub(super) paste: bool,
    /// Clipboard text the app asked for, once it arrived.
    pub(super) pasted: Option<String>,
    /// Pointer presses, releases, scrolls and moves with a button held, oldest first.
    pub(super) pointer_events: Vec<UiEvent>,
    /// A pointer button is held.
    pub(super) held: bool,
}

impl UiWindow {
    /// Pointer presses, releases, scrolls and moves with a button held since the last call,
    /// oldest first (the widget tree handled them too). An app that draws a picture itself
    /// (the editor's text) places its caret and selection with them.
    pub fn take_pointer_events(&mut self) -> Vec<UiEvent> {
        std::mem::take(&mut self.app.pointer_events)
    }

    /// Asks the compositor for the clipboard text for the app itself (not for a field);
    /// it arrives with [`UiWindow::take_pasted`].
    pub fn request_paste(&mut self) -> Result<(), ClientError> {
        self.pasting = Some(Vec::new());
        self.app.paste = true;
        self.client.send(&Request::ReadSelection {
            serial: self.serial,
            mime: TEXT_MIME.to_owned(),
        })
    }

    /// The clipboard text asked for with [`UiWindow::request_paste`], once it arrived.
    pub fn take_pasted(&mut self) -> Option<String> {
        self.app.pasted.take()
    }

    pub(super) fn note_pointer(&mut self, event: &UiEvent) {
        let keep = match event {
            UiEvent::PointerDown { .. } => {
                self.app.held = true;
                true
            }
            UiEvent::PointerUp { .. } => {
                self.app.held = false;
                true
            }
            UiEvent::Scroll { .. } => true,
            UiEvent::PointerMove { .. } => self.app.held,
            _ => false,
        };
        if keep {
            if self.app.pointer_events.len() >= MAX_UNHANDLED {
                self.app.pointer_events.remove(0);
            }
            self.app.pointer_events.push(*event);
        }
    }
}
