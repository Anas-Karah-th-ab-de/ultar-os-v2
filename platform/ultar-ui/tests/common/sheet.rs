//! The snapshot sheet: renders one widget case in Arabic and English, dark and light,
//! compares each picture with its reference and checks that the states of a widget differ.

use super::{center, key, LANGS};
use std::collections::BTreeMap;
use ultar_ui::event::{Event, Key, PointerButton};
use ultar_ui::layout::{CrossAlign, Flex, Padding};
use ultar_ui::snapshot::{render, Snapshots};
use ultar_ui::tokens::{Space, ThemeKind};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::Label;
use ultar_ui::{Lang, Size};

/// Offset of the "scrolled" state of a scroll container.
const SCROLLED: f32 = 90.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Normal,
    Hovered,
    Pressed,
    Focused,
    Disabled,
    Loading,
    Error,
    Open,
    Scrolled,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Normal => "normal",
            State::Hovered => "hovered",
            State::Pressed => "pressed",
            State::Focused => "focused",
            State::Disabled => "disabled",
            State::Loading => "loading",
            State::Error => "error",
            State::Open => "open",
            State::Scrolled => "scrolled",
        }
    }
}

pub const ACTION_STATES: [State; 6] = [
    State::Normal,
    State::Hovered,
    State::Pressed,
    State::Focused,
    State::Disabled,
    State::Loading,
];

pub fn apply(ui: &mut Ui, id: NodeId, state: State) -> Result<(), UiError> {
    let pos = center(ui, id);
    match state {
        State::Normal => {}
        State::Hovered => {
            ui.handle(Event::PointerMove { pos });
        }
        State::Pressed => {
            ui.handle(Event::PointerMove { pos });
            ui.handle(Event::PointerDown {
                pos,
                button: PointerButton::Primary,
            });
        }
        State::Focused => {
            while ui.focused() != Some(id) {
                assert!(key(ui, Key::Tab).consumed, "widget is reachable by Tab");
            }
        }
        State::Disabled => ui.set_disabled(id, true)?,
        State::Scrolled => ui.set_scroll_offset(id, SCROLLED)?,
        State::Loading => {
            ui.set_loading(id, true)?;
            ui.advance_to(150);
        }
        State::Error => ui.set_error(id, Some(Label::key("demo.orientation.error")))?,
        State::Open => {
            while ui.focused() != Some(id) {
                key(ui, Key::Tab);
            }
            key(ui, Key::Enter);
            key(ui, Key::ArrowDown);
        }
    }
    Ok(())
}

/// Runs the cases, collects every mismatch and checks that the states of a widget differ.
pub struct Sheet {
    pub set: Snapshots,
    pub problems: Vec<String>,
    /// Rendered pixels per case name (Arabic, dark) to prove that states look different.
    pub pictures: BTreeMap<String, Vec<u8>>,
    pub cases: usize,
}

impl Sheet {
    pub fn shoot(
        &mut self,
        name: &str,
        size: Size,
        build: &dyn Fn(&mut Ui) -> Result<NodeId, UiError>,
        state: State,
    ) {
        self.cases += 1;
        for lang in LANGS {
            for theme in ThemeKind::ALL {
                let result = self.set.ui(size, lang, theme).and_then(|mut ui| {
                    ui.set_root_flex(
                        Flex::column()
                            .padding(Padding::all(Space::S16))
                            .gap(Space::S8)
                            .cross(CrossAlign::Start),
                    );
                    let id = build(&mut ui)?;
                    apply(&mut ui, id, state)?;
                    if (lang, theme) == (Lang::Ar, ThemeKind::Dark) {
                        self.pictures
                            .insert(name.to_string(), render(&mut ui)?.to_rgba());
                    }
                    self.set.assert_ui(name, &mut ui)
                });
                if let Err(e) = result {
                    self.problems
                        .push(format!("{name} {} {}: {e}", lang.code(), theme.name()));
                }
            }
        }
    }

    pub fn states(
        &mut self,
        widget: &str,
        size: Size,
        states: &[State],
        build: &dyn Fn(&mut Ui) -> Result<NodeId, UiError>,
    ) {
        for state in states {
            self.shoot(&format!("{widget}-{}", state.name()), size, build, *state);
        }
        let own: Vec<(&String, &Vec<u8>)> = self
            .pictures
            .iter()
            .filter(|(k, _)| k.starts_with(&format!("{widget}-")))
            .collect();
        for (i, (a, pa)) in own.iter().enumerate() {
            for (b, pb) in &own[i + 1..] {
                if pa == pb {
                    self.problems.push(format!("{a} and {b} look identical"));
                }
            }
        }
    }
}
