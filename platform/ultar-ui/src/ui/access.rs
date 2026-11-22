//! Accessibility nodes: what a screen reader needs to announce and operate the tree.

use super::{NodeId, Ui};
use crate::geom::Rect;
use crate::widgets::{Label, Widget};
use ultar_i18n::Translator;

/// What kind of thing a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// A card grouping other nodes.
    Group,
    /// Static text.
    Text,
    /// A named icon.
    Image,
    /// A button or icon button.
    Button,
    /// An on/off switch.
    Switch,
    /// A select.
    Select,
    /// A slider.
    Slider,
    /// A text field.
    TextField,
    /// A settings row.
    Row,
}

impl Role {
    /// Catalog key of the role's spoken name.
    pub const fn name_key(self) -> &'static str {
        match self {
            Role::Group => "ui.role.group",
            Role::Text => "ui.role.text",
            Role::Image => "ui.role.image",
            Role::Button => "ui.role.button",
            Role::Switch => "ui.role.switch",
            Role::Select => "ui.role.select",
            Role::Slider => "ui.role.slider",
            Role::TextField => "ui.role.text_field",
            Role::Row => "ui.role.row",
        }
    }
}

/// The state of a node as announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct A11yState {
    /// Cannot be operated.
    pub disabled: bool,
    /// Has keyboard focus.
    pub focused: bool,
    /// Is held down.
    pub pressed: bool,
    /// Waits for a result.
    pub busy: bool,
    /// Holds an invalid value.
    pub invalid: bool,
    /// On/off for switches.
    pub checked: Option<bool>,
    /// Open/closed for selects.
    pub expanded: Option<bool>,
}

/// One node of the accessibility tree.
#[derive(Debug, Clone, PartialEq)]
pub struct A11yNode {
    /// The UI node.
    pub id: NodeId,
    /// Its role.
    pub role: Role,
    /// Its name (a catalog key label), if it has one.
    pub name: Option<Label>,
    /// Its current value (chosen option, slider value, field content).
    pub value: Option<Label>,
    /// Its error message.
    pub error: Option<Label>,
    /// Its state.
    pub state: A11yState,
    /// Its physical rectangle in logical points.
    pub rect: Rect,
    /// `true` when keyboard focus can reach it.
    pub focusable: bool,
}

impl A11yNode {
    /// The sentence a screen reader speaks: name, role, value, states.
    pub fn describe(&self, tr: &Translator) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut push = |text: std::borrow::Cow<'_, str>| {
            if !text.is_empty() {
                parts.push(text.into_owned());
            }
        };
        if let Some(name) = &self.name {
            push(name.resolve(tr));
        }
        push(tr.t(self.role.name_key()));
        if let Some(value) = &self.value {
            push(value.resolve(tr));
        }
        let flag = |on: Option<bool>, yes: &'static str, no: &'static str| {
            on.map(|v| if v { yes } else { no })
        };
        let states = [
            flag(self.state.checked, "ui.state.on", "ui.state.off"),
            flag(
                self.state.expanded,
                "ui.state.expanded",
                "ui.state.collapsed",
            ),
            self.state.disabled.then_some("ui.state.disabled"),
            self.state.busy.then_some("ui.state.busy"),
            self.state.invalid.then_some("ui.state.invalid"),
        ];
        for key in states.into_iter().flatten() {
            push(tr.t(key));
        }
        if let Some(error) = &self.error {
            push(error.resolve(tr));
        }
        let mut joiner = tr.t("ui.a11y.joiner").into_owned();
        joiner.push(' ');
        parts.join(&joiner)
    }
}

fn number(value: f32) -> Label {
    let text = if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };
    Label::data(&text)
}

impl Ui {
    /// The accessibility node of one UI node; `None` for pure layout containers and
    /// unnamed (decorative) icons.
    pub fn a11y_node(&self, id: NodeId) -> Option<A11yNode> {
        let node = self.nodes.get(id.index())?;
        let (role, name, value) = match &node.widget {
            Widget::Flex(_) => return None,
            Widget::Icon(w) => (Role::Image, Some(w.name.clone()?), None),
            Widget::Card(_) => (Role::Group, None, None),
            Widget::Text(w) => (Role::Text, Some(w.label.clone()), None),
            Widget::Row(w) => (Role::Row, Some(w.label.clone()), w.sublabel.clone()),
            Widget::Button(w) => (Role::Button, Some(w.label.clone()), None),
            Widget::IconButton(w) => (Role::Button, Some(w.name.clone()), None),
            Widget::Switch(w) => (Role::Switch, Some(w.name.clone()), None),
            Widget::Select(w) => (
                Role::Select,
                Some(w.name.clone()),
                w.options.get(w.selected).cloned(),
            ),
            Widget::Slider(w) => (Role::Slider, Some(w.name.clone()), Some(number(w.value))),
            Widget::TextField(w) => (
                Role::TextField,
                Some(w.name.clone()),
                Some(Label::data(w.content())),
            ),
        };
        let visual = self.visual(id);
        let state = A11yState {
            disabled: visual.disabled,
            focused: self.input.focus == Some(id),
            pressed: visual.pressed,
            busy: node.loading,
            invalid: node.error.is_some(),
            checked: match &node.widget {
                Widget::Switch(w) => Some(w.on),
                _ => None,
            },
            expanded: matches!(node.widget, Widget::Select(_)).then(|| self.is_open(id)),
        };
        Some(A11yNode {
            id,
            role,
            name,
            value,
            error: node.error.clone(),
            state,
            rect: self.rect(id)?,
            focusable: node.widget.is_interactive() && !visual.disabled,
        })
    }

    /// All accessibility nodes in logical (reading) order.
    pub fn a11y_tree(&mut self) -> Vec<A11yNode> {
        self.layout();
        self.order
            .iter()
            .filter_map(|id| self.a11y_node(*id))
            .collect()
    }
}
