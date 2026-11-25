//! The terminal window: the output as a picture (newest lines at the bottom, Arabic lines
//! from the right), the prompt with the command line, and Run / Stop. Up and Down walk the
//! history, Page Up/Down and the wheel scroll the output, Ctrl+C stops a running program,
//! Ctrl+L clears the screen.

use crate::run::{Event as RunEvent, Running};
use crate::shell::{Next, Shell};
use std::path::PathBuf;
use ultar_i18n::Arg;
use ultar_textview::{Marks, TextView, ViewError};
use ultar_ui::event::{Action, ActionKind, Event, Key};
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Label, PixelArea, Text, TextField};

/// Most output lines kept (older ones are dropped).
pub const MAX_LINES: usize = 10_000;

#[derive(Default)]
struct Nodes {
    output: Option<NodeId>,
    prompt: Option<NodeId>,
    field: Option<NodeId>,
    run: Option<NodeId>,
}

/// The terminal: shell, output, the running program.
pub struct Terminal {
    /// The widget tree (the window draws it).
    pub ui: Ui,
    /// The shell (folder, history).
    pub shell: Shell,
    /// The output lines.
    pub lines: Vec<String>,
    view: TextView,
    nodes: Nodes,
    running: Option<Running>,
    /// Place in the history while walking it with Up/Down.
    walk: Option<usize>,
    /// The view follows new output (not while the user scrolled up).
    follow: bool,
    closed: bool,
}

impl Terminal {
    /// A terminal over `ui`, starting in `home`.
    pub fn new(ui: Ui, home: PathBuf) -> Result<Terminal, ViewError> {
        let view = TextView::new(*ui.env(), true)?;
        let welcome = ui.translator().t("term.welcome").into_owned();
        let mut t = Terminal {
            ui,
            shell: Shell::new(home),
            lines: vec![welcome],
            view,
            nodes: Nodes::default(),
            running: None,
            walk: None,
            follow: true,
            closed: false,
        };
        t.rebuild().map_err(|e| ViewError(format!("{e:?}")))?;
        Ok(t)
    }

    /// `true` once `exit` was typed.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// `true` while a program runs.
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Builds the whole tree (also after a size change); the typed command is kept.
    pub fn rebuild(&mut self) -> Result<(), UiError> {
        let typed = self
            .nodes
            .field
            .and_then(|f| self.ui.text(f).ok())
            .unwrap_or_default()
            .to_owned();
        let ui = &mut self.ui;
        ui.clear();
        ui.set_root_flex(Flex::column().cross(CrossAlign::Stretch));
        let root = ui.root();
        self.nodes.output = Some(ui.add_sized(
            root,
            PixelArea::new(Label::key("term.output")),
            Sizing::grow(1),
        )?);
        let row = ui.add(
            root,
            Flex::row()
                .gap(Space::S8)
                .padding(Padding::all(Space::S8))
                .cross(CrossAlign::Center),
        )?;
        self.nodes.prompt = Some(ui.add(
            row,
            Text::new(Label::data(&self.shell.prompt())).color(ColorRole::AccentText),
        )?);
        let field = TextField::new(Label::key("term.command"))
            .direction(ultar_ui::widgets::TextDirection::Auto)
            .text(&typed);
        let field = ui.add_sized(row, field, Sizing::grow(1))?;
        self.nodes.field = Some(field);
        let (key, variant) = if self.running.is_some() {
            ("term.stop", ButtonVariant::Danger)
        } else {
            ("term.run", ButtonVariant::Plain)
        };
        self.nodes.run = Some(ui.add(row, Button::new(Label::key(key)).variant(variant))?);
        ui.focus(field)?;
        self.draw()
    }

    fn rows(&mut self) -> usize {
        self.size().map_or(1, |(_, h)| self.view.rows(h))
    }

    fn size(&mut self) -> Option<(u32, u32)> {
        self.ui.layout();
        let rect = self.ui.rect(self.nodes.output?)?;
        let s = self.ui.env().scale;
        let (w, h) = ((rect.w * s).round(), (rect.h * s).round());
        (w >= 1.0 && h >= 1.0).then_some((w as u32, h as u32))
    }

    /// Draws the visible output.
    pub fn draw(&mut self) -> Result<(), UiError> {
        let (Some(node), Some((w, h))) = (self.nodes.output, self.size()) else {
            return Ok(());
        };
        self.view.set_env(*self.ui.env());
        let rows = self.view.rows(h);
        if self.follow {
            self.view.first = self.lines.len().saturating_sub(rows);
        }
        let frame = self
            .view
            .render(w, h, &self.lines, &Marks::default())
            .map_err(|_| UiError::InvalidSize)?;
        self.ui.set_picture(node, frame)
    }

    /// Adds lines to the output (the oldest go beyond [`MAX_LINES`]).
    pub fn print(&mut self, lines: impl IntoIterator<Item = String>) {
        self.lines.extend(lines);
        if self.lines.len() > MAX_LINES {
            let extra = self.lines.len() - MAX_LINES;
            self.lines.drain(..extra);
        }
    }

    /// Runs `line` as typed into the command line.
    pub fn submit(&mut self, line: &str) -> Result<(), UiError> {
        if self.running.is_some() {
            let busy = self.ui.translator().t("term.busy").into_owned();
            self.print([busy]);
            return self.draw();
        }
        self.walk = None;
        self.follow = true;
        let echo = format!("{} {line}", self.shell.prompt());
        self.print([echo]);
        let mut out = Vec::new();
        let next = self.shell.run(line, self.ui.translator(), &mut out);
        self.print(out);
        match next {
            Next::Done => {}
            Next::Clear => self.lines.clear(),
            Next::Exit => self.closed = true,
            Next::Program(program, args) => {
                match Running::start(&program, &args, &self.shell.cwd) {
                    Ok(r) => self.running = Some(r),
                    Err(e) => {
                        let name = program.display().to_string();
                        let msg = self.ui.translator().t_args(
                            "term.start_failed",
                            &[
                                ("name", Arg::Str(&name)),
                                ("reason", Arg::Str(&e.to_string())),
                            ],
                        );
                        self.print([msg]);
                    }
                }
            }
        }
        if let Some(f) = self.nodes.field {
            self.ui.set_text(f, "")?;
        }
        self.rebuild()
    }

    /// Takes the output of the running program; `true` when something changed.
    pub fn poll(&mut self) -> Result<bool, UiError> {
        let Some(r) = self.running.as_mut() else {
            return Ok(false);
        };
        let events = r.poll();
        if events.is_empty() {
            return Ok(false);
        }
        let mut ended = None;
        let mut lines = Vec::new();
        for e in events {
            match e {
                RunEvent::Line(l) => lines.push(l),
                RunEvent::Ended(code) => ended = Some(code),
            }
        }
        self.print(lines);
        match ended {
            Some(code) => {
                self.running = None;
                let tr = self.ui.translator();
                let msg = match code {
                    Some(0) => None,
                    Some(c) => Some(tr.t_args("term.exit_code", &[("code", Arg::Int(c.into()))])),
                    None => Some(tr.t("term.stopped").into_owned()),
                };
                self.print(msg);
                self.rebuild()?;
            }
            None => self.draw()?,
        }
        Ok(true)
    }

    /// Stops the running program (Ctrl+C, Stop).
    pub fn stop(&mut self) {
        if let Some(r) = self.running.as_mut() {
            r.stop();
        }
    }

    /// A key that came to the terminal (history, scrolling, Ctrl+C, Ctrl+L).
    pub fn key(&mut self, event: Event) -> Result<(), UiError> {
        let Event::Key { key, modifiers: m } = event else {
            return Ok(());
        };
        match key {
            Key::Char('c' | 'C') if m.ctrl => self.stop(),
            Key::Char('l' | 'L') if m.ctrl => {
                self.lines.clear();
                return self.draw();
            }
            Key::ArrowUp | Key::ArrowDown if !m.command() => {
                return self.walk_history(key == Key::ArrowUp)
            }
            Key::PageUp | Key::PageDown => {
                let rows = self.rows();
                let step = rows.saturating_sub(1).max(1) as isize;
                self.scroll(if key == Key::PageUp { -step } else { step })?;
            }
            _ => {}
        }
        Ok(())
    }

    fn walk_history(&mut self, back: bool) -> Result<(), UiError> {
        let n = self.shell.history.len();
        if n == 0 {
            return Ok(());
        }
        let next = match (self.walk, back) {
            (None, true) => Some(n - 1),
            (None, false) => None,
            (Some(i), true) => Some(i.saturating_sub(1)),
            (Some(i), false) if i + 1 < n => Some(i + 1),
            (Some(_), false) => None,
        };
        self.walk = next;
        let text = next
            .map_or("", |i| self.shell.history[i].as_str())
            .to_owned();
        if let Some(f) = self.nodes.field {
            self.ui.set_text(f, &text)?;
        }
        Ok(())
    }

    fn scroll(&mut self, lines: isize) -> Result<(), UiError> {
        let rows = self.rows();
        let total = self.lines.len();
        self.view.scroll(lines, total, rows);
        self.follow = self.view.first + rows >= total;
        self.draw()
    }

    /// A pointer event: the wheel over the output scrolls it.
    pub fn pointer(&mut self, event: Event) -> Result<(), UiError> {
        if let Event::Scroll { pos, lines } = event {
            let over = self
                .nodes
                .output
                .and_then(|n| self.ui.rect(n))
                .is_some_and(|r| r.contains(pos));
            if over {
                self.scroll(lines.round() as isize * 3)?;
            }
        }
        Ok(())
    }

    /// The actions of the widget tree: Enter in the command line, Run / Stop.
    pub fn actions(&mut self, actions: Vec<Action>) -> Result<(), UiError> {
        for a in actions {
            let field = Some(a.node) == self.nodes.field;
            let run = Some(a.node) == self.nodes.run;
            match a.kind {
                ActionKind::Submitted if field => self.submit_field()?,
                ActionKind::Activated if run && self.running.is_some() => self.stop(),
                ActionKind::Activated if run => self.submit_field()?,
                _ => {}
            }
        }
        Ok(())
    }

    fn submit_field(&mut self) -> Result<(), UiError> {
        let line = self
            .nodes
            .field
            .and_then(|f| self.ui.text(f).ok())
            .unwrap_or_default()
            .to_owned();
        self.submit(&line)
    }
}
