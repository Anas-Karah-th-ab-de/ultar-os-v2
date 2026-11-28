//! The questions of the window (unsaved changes, a name, the copy after a crash, the
//! assistant's request), saving, and the actions of the widgets.

use crate::app::{Out, SheetApp, State};
use crate::cmd::Cmd;
use crate::file::{self, DocError, Format};
use crate::recover::{Found, Recovery};
use std::path::PathBuf;
use ultar_ui::event::{Action, ActionKind};
use ultar_ui::layout::{Flex, MainAlign, Sizing};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::UiError;
use ultar_ui::widgets::{Button, ButtonVariant, Dialog, Label, Text, TextField};

/// What happens after a question is answered and the workbook is saved or given up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Then {
    /// Nothing.
    Nothing,
    /// The window closes.
    Close,
    /// A new, empty workbook.
    New,
    /// Another workbook is read.
    Load(PathBuf),
}

/// An open question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Unsaved changes.
    Unsaved(Then),
    /// The name to save under.
    Name(Then),
    /// The name of a workbook to open.
    Open,
    /// A new name for sheet `i`.
    Rename(usize),
    /// A copy of unsaved changes of an earlier run.
    Recover(Found),
    /// The assistant wants to change the workbook: what it wants.
    Agent(String),
}

/// What a question shows besides its buttons.
struct DialogParts {
    title: Label,
    ok_key: &'static str,
    /// A text field: its label key and the text it starts with.
    field: Option<(&'static str, String)>,
    message: Option<Label>,
}

impl SheetApp {
    // ---------------------------------------------------------------- dialogs

    /// Title, OK key, optional text field (key and start text) and message of a question.
    fn dialog_parts(&self, ask: &Ask) -> DialogParts {
        let name = self
            .core
            .book
            .name()
            .unwrap_or_else(|| self.t("sheet.untitled"));
        match ask {
            Ask::Unsaved(_) => DialogParts {
                title: Label::key_with("sheet.ask.title", &[("name", &name)]),
                ok_key: "sheet.ask.save",
                field: None,
                message: Some(Label::key("sheet.ask.message")),
            },
            Ask::Name(_) => DialogParts {
                title: Label::key("sheet.name.title"),
                ok_key: "sheet.name.ok",
                field: Some(("sheet.name.field", String::new())),
                message: None,
            },
            Ask::Open => DialogParts {
                title: Label::key("sheet.open.title"),
                ok_key: "sheet.open.ok",
                field: Some(("sheet.name.field", String::new())),
                message: None,
            },
            Ask::Rename(i) => {
                let cur = self.core.book.wb.sheets.get(*i).map(|s| s.name.clone());
                DialogParts {
                    title: Label::key("sheet.rename.title"),
                    ok_key: "sheet.rename.ok",
                    field: Some(("sheet.rename.field", cur.unwrap_or_default())),
                    message: None,
                }
            }
            Ask::Recover(_) => DialogParts {
                title: Label::key("sheet.recover.title"),
                ok_key: "sheet.recover.ok",
                field: None,
                message: Some(Label::key("sheet.recover.message")),
            },
            Ask::Agent(what) => DialogParts {
                title: Label::key("sheet.agent.title"),
                ok_key: "sheet.agent.allow",
                field: None,
                message: Some(Label::key_with("sheet.agent.message", &[("what", what)])),
            },
        }
    }

    pub(crate) fn build_dialog(&mut self) -> Result<(), UiError> {
        let Some(ask) = self.ask.clone() else {
            return Ok(());
        };
        let parts = self.dialog_parts(&ask);
        let root = self.ui.root();
        let dialog = self.ui.add(root, Dialog::new(parts.title))?;
        if let Some(m) = parts.message {
            self.ui
                .add(dialog, Text::new(m).color(ColorRole::TextMuted))?;
        }
        if let Some((key, text)) = parts.field {
            let f = TextField::new(Label::key(key)).labelled().text(&text);
            self.nodes.dialog_field = Some(self.ui.add_sized(dialog, f, Sizing::grow(1))?);
        }
        let row = self
            .ui
            .add(dialog, Flex::row().gap(Space::S8).main(MainAlign::End))?;
        let cancel_key = match ask {
            Ask::Recover(_) => "sheet.recover.discard",
            Ask::Agent(_) => "sheet.agent.deny",
            _ => "sheet.ask.cancel",
        };
        let cancel = self.ui.add(
            row,
            Button::new(Label::key(cancel_key)).variant(ButtonVariant::Plain),
        )?;
        self.nodes.dialog_cancel = Some(cancel);
        if matches!(ask, Ask::Unsaved(_)) {
            let d = self.ui.add(
                row,
                Button::new(Label::key("sheet.ask.discard")).variant(ButtonVariant::Danger),
            )?;
            self.nodes.dialog_discard = Some(d);
        }
        let ok = self.ui.add(row, Button::new(Label::key(parts.ok_key)))?;
        self.nodes.dialog_ok = Some(ok);
        self.ui.set_default_action(dialog, Some(ok))?;
        self.ui.open_dialog(dialog)?;
        if let Some(f) = self.nodes.dialog_field {
            self.ui.focus(f)?;
        }
        Ok(())
    }

    pub(crate) fn ask_name(&mut self, then: Then) -> Result<(), UiError> {
        self.ask = Some(Ask::Name(then));
        self.rebuild()
    }

    pub(crate) fn ask_open(&mut self) -> Result<(), UiError> {
        self.ask = Some(Ask::Open);
        self.rebuild()
    }

    pub(crate) fn ask_rename(&mut self) -> Result<(), UiError> {
        self.ask = Some(Ask::Rename(self.core.book.active));
        self.rebuild()
    }

    /// Offers the copy of an earlier run of this workbook, when there is one.
    pub fn offer_recovery(&mut self, path: &std::path::Path) {
        if let Some(found) = self.upkeep.recovery.find(Some(path)) {
            self.ask = Some(Ask::Recover(found));
        }
    }

    /// Offers the copy of a new workbook of an earlier run (start without a file).
    pub fn offer_recovery_new(&mut self) -> Result<(), UiError> {
        if let Some(found) = self.upkeep.recovery.find(None) {
            self.ask = Some(Ask::Recover(found));
            self.rebuild()?;
        }
        Ok(())
    }

    // ---------------------------------------------------------------- saving

    fn say_failed(&mut self, name: &str, e: DocError) {
        let why = self.t(e.key());
        self.core.message = Some(Label::key_with(
            "sheet.save_failed",
            &[("name", name), ("reason", &why)],
        ));
    }

    /// Saves to the workbook's file (asks for a name when it has none), then does `then`.
    pub(crate) fn save(&mut self, then: Then) -> Result<(), UiError> {
        self.finish_edit();
        let Some(path) = self.core.book.path.clone() else {
            return self.ask_name(then);
        };
        self.save_to(path, then)
    }

    fn save_to(&mut self, path: PathBuf, then: Then) -> Result<(), UiError> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut format = Format::of_name(&path);
        if let (Format::Csv { .. }, Format::Csv { .. }) = (format, self.core.book.format) {
            format = self.core.book.format;
        }
        match file::save(&path, &self.core.book.wb, format, self.core.book.active) {
            Ok(saved) => {
                self.core.book.path = Some(path.clone());
                self.core.book.format = match (format, saved.encoding_changed) {
                    (Format::Csv { sep, eol, .. }, Some(enc)) => Format::Csv { enc, sep, eol },
                    (f, _) => f,
                };
                self.core.book.mark_saved();
                self.upkeep.recovery.clear();
                self.upkeep.recovery = Recovery::new(&self.recovery_path(), Some(&path));
                let key = if saved.values_only {
                    "sheet.saved_values"
                } else if saved.encoding_changed.is_some() {
                    "sheet.saved_utf8"
                } else {
                    "sheet.saved"
                };
                self.core.message = Some(Label::key_with(key, &[("name", &name)]));
                self.upkeep.title_version = u64::MAX;
                self.after(then)?;
            }
            Err(e) => self.say_failed(&name, e),
        }
        self.rebuild()
    }

    fn recovery_path(&self) -> PathBuf {
        self.upkeep.recovery.dir().to_owned()
    }

    fn after(&mut self, then: Then) -> Result<(), UiError> {
        match then {
            Then::Nothing => {}
            Then::Close => self.out.push(Out::Close),
            Then::New => {
                self.ask = None;
                self.reset()?;
            }
            Then::Load(path) => self.start_load(path)?,
        }
        Ok(())
    }

    /// Reads another workbook in the background.
    pub(crate) fn start_load(&mut self, path: PathBuf) -> Result<(), UiError> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.ask = None;
        self.out.push(Out::Load(path));
        self.loading(&name)
    }

    /// Opens the workbook a `files://` URI names (also from the assistant).
    pub fn open_uri(&mut self, uri: &str) -> Result<(), UiError> {
        let (base, spot) = crate::place::split(uri);
        let Some(path) = crate::place::path_of(base, &self.areas) else {
            self.core.say("sheet.reason.missing");
            return self.refresh();
        };
        self.waiting.spot = Some(spot);
        if self.core.book.dirty() && self.state == State::Ready {
            self.ask = Some(Ask::Unsaved(Then::Load(path)));
            return self.rebuild();
        }
        self.start_load(path)
    }

    // ---------------------------------------------------------------- answers

    fn field(&self) -> String {
        self.nodes
            .dialog_field
            .and_then(|f| self.ui.text(f).ok())
            .unwrap_or_default()
            .trim()
            .to_owned()
    }

    fn answer_ok(&mut self) -> Result<(), UiError> {
        let Some(ask) = self.ask.take() else {
            return Ok(());
        };
        match ask {
            Ask::Unsaved(then) => {
                self.rebuild()?;
                return self.save(then);
            }
            Ask::Name(then) => {
                let name = self.field();
                match crate::place::new_path(&name, &self.areas) {
                    Some(p) => return self.save_to(p, then),
                    None => self.say_failed(&name, DocError::BadName),
                }
            }
            Ask::Open => {
                let name = self.field();
                match crate::place::new_path(&name, &self.areas) {
                    Some(p) if self.core.book.dirty() && self.state == State::Ready => {
                        self.ask = Some(Ask::Unsaved(Then::Load(p)));
                    }
                    Some(p) => return self.start_load(p),
                    None => self.say_failed(&name, DocError::BadName),
                }
            }
            Ask::Rename(i) => {
                let name = self.field();
                match self.core.rename_sheet(i, &name) {
                    Ok(()) => {}
                    Err(e) => {
                        let key = match e {
                            ultar_office::sheet_edit::NameError::Taken => "sheet.rename.taken",
                            _ => "sheet.rename.bad",
                        };
                        self.core.say(key);
                    }
                }
            }
            Ask::Recover(found) => match Recovery::load(&found) {
                Ok(wb) => {
                    self.core.book.adopt(wb);
                    self.core.say("sheet.recovered");
                }
                Err(e) => self.say_failed(&self.t("sheet.recover.title"), e),
            },
            Ask::Agent(_) => {
                let reply = self.core.agent_answer(true);
                self.waiting.agent_reply = Some(reply);
            }
        }
        self.rebuild()
    }

    fn answer_cancel(&mut self) -> Result<(), UiError> {
        match self.ask.take() {
            Some(Ask::Recover(_)) => self.upkeep.recovery.clear(),
            Some(Ask::Agent(_)) => self.waiting.agent_reply = Some(self.core.agent_answer(false)),
            _ => {}
        }
        self.rebuild()
    }

    fn answer_discard(&mut self) -> Result<(), UiError> {
        let then = match self.ask.take() {
            Some(Ask::Unsaved(t)) => t,
            _ => Then::Nothing,
        };
        self.core.book.mark_saved();
        self.upkeep.recovery.clear();
        match then {
            Then::Close => {
                self.out.push(Out::Close);
                self.rebuild()
            }
            other => {
                self.after(other)?;
                self.rebuild()
            }
        }
    }

    /// The actions of the widget tree.
    pub fn actions(&mut self, actions: Vec<Action>) -> Result<(), UiError> {
        for a in actions {
            let node = Some(a.node);
            match a.kind {
                ActionKind::Activated | ActionKind::Submitted => {
                    if self.ask.is_some() {
                        if node == self.nodes.dialog_cancel {
                            self.answer_cancel()?;
                        } else if node == self.nodes.dialog_discard {
                            self.answer_discard()?;
                        } else if node == self.nodes.dialog_ok || node == self.nodes.dialog_field {
                            self.answer_ok()?;
                        }
                    } else if node == self.nodes.formula && a.kind == ActionKind::Submitted {
                        self.finish_edit();
                        self.core.sel.advance(true, false);
                        self.focus_grid();
                        self.refresh()?;
                    } else if node == self.nodes.find && a.kind == ActionKind::Submitted {
                        self.run(Cmd::FindNext)?;
                    } else if let Some(&(_, cmd)) = self.cmds.iter().find(|(n, _)| Some(*n) == node)
                    {
                        self.run(cmd)?;
                    }
                }
                ActionKind::TextChanged => self.text_changed(a.node)?,
                ActionKind::Closed if self.tools_open => {
                    self.tools_open = false;
                    self.rebuild()?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn text_changed(&mut self, node: ultar_ui::ui::NodeId) -> Result<(), UiError> {
        let text = self.ui.text(node).unwrap_or_default().to_owned();
        if Some(node) == self.nodes.formula {
            // typing in the formula bar edits the active cell
            let at = self.core.sel.active;
            let caret = text.len();
            self.core.edit = Some(crate::draw::Editing { at, text, caret });
            self.draw_grid()?;
        } else if Some(node) == self.nodes.find {
            self.core.find.needle = text;
        } else if Some(node) == self.nodes.with {
            self.core.find.with = text;
        }
        Ok(())
    }

    pub(crate) fn focus_grid(&mut self) {
        if let Some(g) = self.nodes.grid {
            let _ = self.ui.focus(g);
        }
    }
}
