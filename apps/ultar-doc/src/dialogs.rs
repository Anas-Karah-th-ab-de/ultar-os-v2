//! The questions the window asks: save changes, a file name (save as / PDF / picture), find and
//! replace, text colour and highlight, "more" tool bar commands, bringing back an autosaved
//! copy, the warning about parts of a `.docx` the app cannot keep, and the confirmation of an
//! assistant's edit. Each is a modal dialog built from the state in [`Ask`].

use crate::action::AgentAction;
use crate::app::App;
use crate::commands::{self, Class, Cmd};
use ultar_ui::event::{Action, ActionKind};
use ultar_ui::layout::{Flex, MainAlign};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Checkbox, Dialog, Label, Text, TextField};

/// What a file-name question is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Save,
    Pdf,
    Picture,
}

/// An open question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    Unsaved { then_close: bool },
    Name { then_close: bool, purpose: Purpose },
    Find,
    Color,
    Highlight,
    More,
    Recover { copy: std::path::PathBuf },
    Lossy,
    Agent(AgentAction),
}

/// One choice inside a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Text(Option<&'static str>),
    Mark(Option<&'static str>),
    Command(Cmd),
}

/// Text colours (key, hex).
pub const TEXT_COLORS: [(&str, Option<&str>); 8] = [
    ("doc.c.auto", None),
    ("doc.c.black", Some("000000")),
    ("doc.c.red", Some("C00000")),
    ("doc.c.orange", Some("ED7D31")),
    ("doc.c.green", Some("00803C")),
    ("doc.c.blue", Some("1F4E9F")),
    ("doc.c.purple", Some("7030A0")),
    ("doc.c.gray", Some("7F7F7F")),
];

/// Highlight colours (key, hex).
pub const MARKS: [(&str, Option<&str>); 6] = [
    ("doc.c.none", None),
    ("doc.c.yellow", Some("FFFF00")),
    ("doc.c.lime", Some("92D050")),
    ("doc.c.cyan", Some("00FFFF")),
    ("doc.c.pink", Some("FF99CC")),
    ("doc.c.amber", Some("FFC000")),
];

/// The nodes of the window that the app addresses.
#[derive(Default)]
pub struct Nodes {
    pub text: Option<NodeId>,
    pub status: Option<NodeId>,
    pub dialog: Option<NodeId>,
    pub name_field: Option<NodeId>,
    pub find_field: Option<NodeId>,
    pub replace_field: Option<NodeId>,
    pub case_box: Option<NodeId>,
    pub hits: Option<NodeId>,
    pub ok: Option<NodeId>,
    pub cancel: Option<NodeId>,
    pub discard: Option<NodeId>,
    pub prev: Option<NodeId>,
    pub replace: Option<NodeId>,
    pub replace_all: Option<NodeId>,
    pub picks: Vec<(NodeId, Pick)>,
}

fn preview(s: &str) -> String {
    let t: String = s.chars().take(80).collect();
    if s.chars().count() > 80 {
        format!("{t}...")
    } else {
        t
    }
}

impl App {
    /// Opens a question (the tree is rebuilt with the dialog on top).
    pub fn ask_for(&mut self, ask: Ask) -> Result<(), UiError> {
        self.ask = Some(ask);
        self.rebuild()
    }

    /// Closes the question.
    pub fn close_ask(&mut self) -> Result<(), UiError> {
        self.ask = None;
        self.find.hits.clear();
        self.rebuild()
    }

    pub(crate) fn build_dialog(&mut self) -> Result<(), UiError> {
        let Some(ask) = self.ask.clone() else {
            return Ok(());
        };
        let name = self
            .doc
            .name()
            .unwrap_or_else(|| self.ui.translator().t("doc.untitled").into_owned());
        let title = match &ask {
            Ask::Unsaved { .. } => Label::key_with("doc.ask.title", &[("name", &name)]),
            Ask::Name {
                purpose: Purpose::Save,
                ..
            } => Label::key("doc.name.title"),
            Ask::Name {
                purpose: Purpose::Pdf,
                ..
            } => Label::key("doc.pdf.title"),
            Ask::Name {
                purpose: Purpose::Picture,
                ..
            } => Label::key("doc.picture.title"),
            Ask::Find => Label::key("doc.find.title"),
            Ask::Color => Label::key("doc.color"),
            Ask::Highlight => Label::key("doc.highlight"),
            Ask::More => Label::key("doc.more"),
            Ask::Recover { .. } => Label::key("doc.recover.title"),
            Ask::Lossy => Label::key("doc.lossy.title"),
            Ask::Agent(_) => Label::key("doc.agent.title"),
        };
        let root = self.ui.root();
        let dialog = self.ui.add(root, Dialog::new(title))?;
        self.nodes.dialog = Some(dialog);
        self.dialog_body(dialog, &ask)?;
        self.dialog_buttons(dialog, &ask)?;
        self.ui.open_dialog(dialog)?;
        if let Some(field) = self.nodes.name_field.or(self.nodes.find_field) {
            self.ui.focus(field)?;
        }
        Ok(())
    }

    fn muted(&mut self, parent: NodeId, label: Label) -> Result<NodeId, UiError> {
        self.ui
            .add(parent, Text::new(label).color(ColorRole::TextMuted))
    }

    fn dialog_body(&mut self, dialog: NodeId, ask: &Ask) -> Result<(), UiError> {
        match ask {
            Ask::Unsaved { .. } => {
                self.muted(dialog, Label::key("doc.ask.message"))?;
            }
            Ask::Name { purpose, .. } => {
                let key = if *purpose == Purpose::Picture {
                    "doc.picture.field"
                } else {
                    "doc.name.field"
                };
                let field = self
                    .ui
                    .add(dialog, TextField::new(Label::key(key)).labelled())?;
                self.nodes.name_field = Some(field);
                let suggest = self
                    .doc
                    .name()
                    .map(|n| n.trim_end_matches(".docx").to_owned());
                if let (Purpose::Save | Purpose::Pdf, Some(s)) = (purpose, suggest) {
                    self.ui.set_text(field, &s)?;
                }
            }
            Ask::Find => self.find_body(dialog)?,
            Ask::Color => self.picks(dialog, &TEXT_COLORS, Pick::Text)?,
            Ask::Highlight => self.picks(dialog, &MARKS, Pick::Mark)?,
            Ask::More => self.more_body(dialog)?,
            Ask::Recover { .. } => {
                self.muted(dialog, Label::key("doc.recover.message"))?;
            }
            Ask::Lossy => {
                let list = self.lossy_names();
                self.muted(
                    dialog,
                    Label::key_with("doc.lossy.message", &[("parts", &list)]),
                )?;
            }
            Ask::Agent(a) => {
                let label = match a {
                    AgentAction::InsertText { text } => {
                        Label::key_with("doc.agent.insert", &[("text", &preview(text))])
                    }
                    AgentAction::Replace { find, repl } => Label::key_with(
                        "doc.agent.replace",
                        &[("find", &preview(find)), ("repl", &preview(repl))],
                    ),
                    _ => Label::key("doc.agent.other"),
                };
                self.muted(dialog, label)?;
            }
        }
        Ok(())
    }

    fn lossy_names(&self) -> String {
        let tr = self.ui.translator();
        let names: Vec<String> = self
            .doc
            .lossy
            .iter()
            .map(|k| tr.t(&format!("doc.lossy.{k}")).into_owned())
            .collect();
        names.join(", ")
    }

    fn find_body(&mut self, dialog: NodeId) -> Result<(), UiError> {
        let find = self.ui.add(
            dialog,
            TextField::new(Label::key("doc.find.field")).labelled(),
        )?;
        self.ui.set_text(find, &self.find.needle.clone())?;
        let repl = self.ui.add(
            dialog,
            TextField::new(Label::key("doc.replace.field")).labelled(),
        )?;
        self.ui.set_text(repl, &self.find.repl.clone())?;
        let case = self.ui.add(
            dialog,
            Checkbox::new(Label::key("doc.find.case"), self.find.match_case),
        )?;
        let hits = self.muted(
            dialog,
            Label::key_with(
                "doc.find.count",
                &[("n", &self.find.hits.len().to_string())],
            ),
        )?;
        self.nodes.find_field = Some(find);
        self.nodes.replace_field = Some(repl);
        self.nodes.case_box = Some(case);
        self.nodes.hits = Some(hits);
        Ok(())
    }

    fn picks(
        &mut self,
        dialog: NodeId,
        list: &[(&'static str, Option<&'static str>)],
        make: fn(Option<&'static str>) -> Pick,
    ) -> Result<(), UiError> {
        let row = self.ui.add(dialog, Flex::row().gap(Space::S8).wrap())?;
        for (key, hex) in list {
            let id = self.ui.add(
                row,
                Button::new(Label::key(*key)).variant(ButtonVariant::Plain),
            )?;
            self.nodes.picks.push((id, make(*hex)));
        }
        Ok(())
    }

    fn more_body(&mut self, dialog: NodeId) -> Result<(), UiError> {
        let row = self.ui.add(dialog, Flex::row().gap(Space::S8).wrap())?;
        for cmd in commands::more(Class::of(self.ui.size().w)) {
            let id = self.ui.add(
                row,
                Button::new(Label::key(commands::key(cmd))).variant(ButtonVariant::Plain),
            )?;
            self.nodes.picks.push((id, Pick::Command(cmd)));
        }
        Ok(())
    }

    fn dialog_buttons(&mut self, dialog: NodeId, ask: &Ask) -> Result<(), UiError> {
        let row = self.ui.add(
            dialog,
            Flex::row().gap(Space::S8).main(MainAlign::End).wrap(),
        )?;
        let button = |ui: &mut ultar_ui::ui::Ui, key: &str, variant| {
            ui.add(row, Button::new(Label::key(key)).variant(variant))
        };
        let plain = ButtonVariant::Plain;
        if !matches!(ask, Ask::Lossy) {
            let key = if matches!(ask, Ask::Find | Ask::More | Ask::Color | Ask::Highlight) {
                "doc.close"
            } else {
                "doc.ask.cancel"
            };
            self.nodes.cancel = Some(button(&mut self.ui, key, plain)?);
        }
        match ask {
            Ask::Unsaved { .. } => {
                self.nodes.discard = Some(button(
                    &mut self.ui,
                    "doc.ask.discard",
                    ButtonVariant::Danger,
                )?)
            }
            Ask::Recover { .. } => {
                self.nodes.discard = Some(button(
                    &mut self.ui,
                    "doc.recover.discard",
                    ButtonVariant::Danger,
                )?)
            }
            Ask::Find => {
                self.nodes.prev = Some(button(&mut self.ui, "doc.find.prev", plain)?);
                self.nodes.replace = Some(button(&mut self.ui, "doc.find.replace", plain)?);
                self.nodes.replace_all = Some(button(&mut self.ui, "doc.find.replace_all", plain)?);
            }
            _ => {}
        }
        let ok_key = match ask {
            Ask::Unsaved { .. } => "doc.ask.save",
            Ask::Name { .. } => "doc.name.ok",
            Ask::Find => "doc.find.next",
            Ask::Recover { .. } => "doc.recover.ok",
            Ask::Agent(_) => "doc.agent.allow",
            Ask::Lossy => "doc.ok",
            Ask::Color | Ask::Highlight | Ask::More => return Ok(()),
        };
        let ok = button(&mut self.ui, ok_key, ButtonVariant::Primary)?;
        self.nodes.ok = Some(ok);
        self.ui.set_default_action(dialog, Some(ok))
    }

    pub(crate) fn field(&self, node: Option<NodeId>) -> String {
        node.and_then(|n| self.ui.text(n).ok())
            .unwrap_or_default()
            .to_owned()
    }

    /// Reads the find fields into the search state and searches again.
    fn sync_find(&mut self) {
        self.find.needle = self.field(self.nodes.find_field);
        self.find.repl = self.field(self.nodes.replace_field);
        if let Some(c) = self.nodes.case_box {
            self.find.match_case = self.ui.is_checked(c).unwrap_or(false);
        }
        self.find.run(&self.doc);
    }

    fn show_hits(&mut self) -> Result<(), UiError> {
        if let Some(n) = self.nodes.hits {
            self.ui.set_label(
                n,
                Label::key_with(
                    "doc.find.count",
                    &[("n", &self.find.hits.len().to_string())],
                ),
            )?;
        }
        self.reveal();
        self.draw()
    }

    fn find_action(&mut self, node: NodeId) -> Result<bool, UiError> {
        let n = &self.nodes;
        let (next, prev, repl, all) = (
            n.ok == Some(node),
            n.prev == Some(node),
            n.replace == Some(node),
            n.replace_all == Some(node),
        );
        if !(next || prev || repl || all) {
            return Ok(false);
        }
        self.sync_find();
        let mut find = std::mem::take(&mut self.find);
        if repl {
            find.replace_current(&mut self.doc);
        } else if all {
            let n = find.replace_all(&mut self.doc);
            self.message = Some(Label::key_with(
                "doc.find.replaced",
                &[("n", &n.to_string())],
            ));
        } else {
            find.step(&mut self.doc, if prev { -1 } else { 1 });
        }
        self.find = find;
        self.touch_title();
        self.show_hits()?;
        Ok(true)
    }

    /// An action of the widgets while a question is open.
    pub(crate) fn dialog_action(&mut self, a: &Action) -> Result<(), UiError> {
        let node = a.node;
        if matches!(a.kind, ActionKind::TextChanged) && Some(node) == self.nodes.find_field {
            self.sync_find();
            return self.show_hits();
        }
        if !matches!(
            a.kind,
            ActionKind::Activated | ActionKind::Submitted | ActionKind::Toggled(_)
        ) {
            return Ok(());
        }
        if matches!(a.kind, ActionKind::Toggled(_)) && Some(node) == self.nodes.case_box {
            self.sync_find();
            return self.show_hits();
        }
        if matches!(a.kind, ActionKind::Toggled(_)) {
            return Ok(());
        }
        let field_submit = matches!(a.kind, ActionKind::Submitted)
            && (Some(node) == self.nodes.name_field || Some(node) == self.nodes.find_field);
        let ok = self.nodes.ok == Some(node) || field_submit;
        if self.ask == Some(Ask::Find)
            && (field_submit
                || self.nodes.ok == Some(node)
                || [self.nodes.prev, self.nodes.replace, self.nodes.replace_all]
                    .contains(&Some(node)))
        {
            let target = if field_submit {
                self.nodes.ok.unwrap_or(node)
            } else {
                node
            };
            self.find_action(target)?;
            return Ok(());
        }
        if let Some(&(_, pick)) = self.nodes.picks.iter().find(|(n, _)| *n == node) {
            return self.pick(pick);
        }
        if self.nodes.cancel == Some(node) {
            return self.cancel_ask();
        }
        if self.nodes.discard == Some(node) {
            return self.discard_ask();
        }
        if ok {
            return self.ok_ask();
        }
        Ok(())
    }

    fn pick(&mut self, pick: Pick) -> Result<(), UiError> {
        self.ask = None;
        self.rebuild()?;
        match pick {
            Pick::Text(hex) => self.doc.apply_run(&|r, _| r.color = hex.map(str::to_owned)),
            Pick::Mark(hex) => self
                .doc
                .apply_run(&|r, _| r.highlight = hex.map(str::to_owned)),
            Pick::Command(cmd) => return self.do_cmd(cmd),
        }
        self.doc.burst_break();
        self.changed()
    }

    fn cancel_ask(&mut self) -> Result<(), UiError> {
        if let Some(Ask::Agent(a)) = self.ask.clone() {
            self.agent_declined(&a);
        }
        self.close_ask()
    }

    fn discard_ask(&mut self) -> Result<(), UiError> {
        match self.ask.clone() {
            Some(Ask::Unsaved { then_close }) => {
                self.ask = None;
                self.doc.dirty = false;
                crate::recovery::remove(self.doc.path.as_deref(), &self.areas);
                if then_close {
                    self.io.out.push(crate::action::Out::Close);
                    return Ok(());
                }
                self.reset_doc()
            }
            Some(Ask::Recover { copy }) => {
                let _ = std::fs::remove_file(copy);
                self.close_ask()
            }
            _ => Ok(()),
        }
    }

    fn ok_ask(&mut self) -> Result<(), UiError> {
        match self.ask.clone() {
            Some(Ask::Unsaved { then_close }) => {
                self.ask = None;
                self.rebuild()?;
                self.save(then_close)
            }
            Some(Ask::Name {
                then_close,
                purpose,
            }) => self.submit_name(then_close, purpose),
            Some(Ask::Recover { copy }) => {
                let message = match crate::recovery::restore(&mut self.doc, &copy) {
                    Ok(()) => Label::key("doc.recover.done"),
                    Err(e) => Label::key_with(
                        "doc.open_failed",
                        &[("reason", &self.ui.translator().t(e.key()))],
                    ),
                };
                self.message = Some(message);
                self.ask = None;
                self.changed()?;
                self.rebuild()
            }
            Some(Ask::Agent(a)) => {
                self.ask = None;
                self.rebuild()?;
                self.agent_run(&a)
            }
            _ => self.close_ask(),
        }
    }
}
