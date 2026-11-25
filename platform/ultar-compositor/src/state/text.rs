//! Text: window titles and the input method.

use super::{ClientId, State};
use ultar_surface::proto::{ErrorCode, Event, ImeKey, Modifiers, Rights, Role};

/// Removes invisible format characters (bidi marks, overrides, isolates,
/// zero-width and other format characters, tags) so a title cannot
/// disguise itself; the title bar isolates it anyway.
pub(super) fn clean_title(title: &str) -> String {
    title
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '\u{00AD}'
                    | '\u{061C}'
                    | '\u{180E}'
                    | '\u{200B}'..='\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2060}'..='\u{2064}'
                    | '\u{2066}'..='\u{206F}'
                    | '\u{FEFF}'
                    | '\u{FFF9}'..='\u{FFFB}'
                    | '\u{E0001}'
                    | '\u{E0020}'..='\u{E007F}'
            )
        })
        .collect()
}

impl State {
    /// Tells the input methods whether the text target has a text field: every client with
    /// the right `ui.input-method` (an on-screen keyboard shows its panel only when a field
    /// wants it, so it must hear about the field before it has a panel).
    pub(super) fn update_ime(&mut self) {
        let wanted = self
            .text_target()
            .and_then(|k| self.surface(k))
            .and_then(|s| s.text_input);
        let panels: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, c)| c.rights.contains(Rights::INPUT_METHOD))
            .map(|(id, _)| *id)
            .collect();
        for client in panels {
            let event = Event::ImeState {
                active: wanted.is_some(),
                purpose: wanted.unwrap_or(0),
            };
            self.send(client, event);
        }
    }

    /// The input method may type only with the right and while its panel is shown.
    fn may_type(&self, client: ClientId) -> Result<(), ErrorCode> {
        let c = self.clients.get(&client).ok_or(ErrorCode::UnknownObject)?;
        let has_panel = c.surfaces.values().any(|s| s.role == Role::InputPanel);
        if !c.rights.contains(Rights::INPUT_METHOD) || !has_panel {
            return Err(ErrorCode::Permission);
        }
        Ok(())
    }

    /// An editing key of the input method reaches the focused field as a press and a
    /// release (serial 0: it proves no user action of the app's own).
    pub(super) fn ime_key(&mut self, client: ClientId, key: ImeKey) -> Result<(), ErrorCode> {
        self.may_type(client)?;
        if let Some(focus) = self.text_target() {
            for state in [1, 0] {
                self.send(
                    focus.client,
                    Event::Key {
                        surface: focus.surface,
                        code: key.code(),
                        state,
                        modifiers: Modifiers::default(),
                        character: 0,
                        serial: 0,
                    },
                );
            }
        }
        Ok(())
    }

    pub(super) fn ime_commit(&mut self, client: ClientId, text: String) -> Result<(), ErrorCode> {
        self.may_type(client)?;
        if let Some(focus) = self.text_target() {
            self.send(
                focus.client,
                Event::Text {
                    surface: focus.surface,
                    text,
                },
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_lose_format_characters() {
        let t = "a\u{202E}b\u{2067}c\u{200D}d\u{061C}e\u{E0041}f\u{00AD}g";
        assert_eq!(clean_title(t), "abcdefg");
        assert_eq!(clean_title("نص عربي"), "نص عربي");
    }
}
