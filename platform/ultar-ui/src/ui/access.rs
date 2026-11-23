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
    /// A badge.
    Badge,
    /// A progress bar.
    Progress,
    /// A checkbox.
    Checkbox,
    /// A radio group.
    RadioGroup,
    /// A tab bar.
    TabList,
    /// An info bar.
    Alert,
    /// An empty state.
    Status,
    /// A list.
    List,
    /// A list item.
    ListItem,
    /// A scroll container.
    ScrollArea,
    /// A dialog or sheet.
    Dialog,
    /// A search field.
    SearchField,
    /// An open menu level.
    Menu,
    /// A command of an open menu.
    MenuItem,
}

impl Role {
    /// All roles.
    pub const ALL: [Role; 23] = [
        Role::Group,
        Role::Text,
        Role::Image,
        Role::Button,
        Role::Switch,
        Role::Select,
        Role::Slider,
        Role::TextField,
        Role::Row,
        Role::Badge,
        Role::Progress,
        Role::Checkbox,
        Role::RadioGroup,
        Role::TabList,
        Role::Alert,
        Role::Status,
        Role::List,
        Role::ListItem,
        Role::ScrollArea,
        Role::Dialog,
        Role::SearchField,
        Role::Menu,
        Role::MenuItem,
    ];

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
            Role::Badge => "ui.role.badge",
            Role::Progress => "ui.role.progress",
            Role::Checkbox => "ui.role.checkbox",
            Role::RadioGroup => "ui.role.radio_group",
            Role::TabList => "ui.role.tab_list",
            Role::Alert => "ui.role.alert",
            Role::Status => "ui.role.status",
            Role::List => "ui.role.list",
            Role::ListItem => "ui.role.list_item",
            Role::ScrollArea => "ui.role.scroll_area",
            Role::Dialog => "ui.role.dialog",
            Role::SearchField => "ui.role.search_field",
            Role::Menu => "ui.role.menu",
            Role::MenuItem => "ui.role.menu_item",
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
    /// Open/closed for selects and for menu items with a submenu.
    pub expanded: Option<bool>,
    /// Selected or not, for list items.
    pub selected: Option<bool>,
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
    /// Its description: the tooltip, or the tone of a badge or info bar.
    pub description: Option<Label>,
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
            self.state
                .selected
                .and_then(|s| s.then_some("ui.state.selected")),
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
        if let Some(description) = &self.description {
            push(description.resolve(tr));
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

fn percent(share: f32) -> Label {
    Label::data(&format!("{:.0}%", (share * 100.0).clamp(0.0, 100.0)))
}

/// Role, name and value of a widget; `None` for pure layout and unnamed icons.
fn describe_widget(widget: &Widget) -> Option<(Role, Option<Label>, Option<Label>)> {
    let named = |label: &Label| Some(label.clone());
    Some(match widget {
        Widget::Flex(_) => return None,
        Widget::Icon(w) => (Role::Image, Some(w.name.clone()?), None),
        Widget::Card(_) => (Role::Group, None, None),
        Widget::Text(w) => (Role::Text, named(&w.label), None),
        Widget::Row(w) => (Role::Row, named(&w.label), w.sublabel.clone()),
        Widget::Button(w) => (Role::Button, named(&w.label), None),
        Widget::IconButton(w) => (Role::Button, named(&w.name), None),
        Widget::Switch(w) => (Role::Switch, named(&w.name), None),
        Widget::Select(w) => (
            Role::Select,
            named(&w.name),
            w.options.get(w.selected).cloned(),
        ),
        Widget::Slider(w) => (Role::Slider, named(&w.name), Some(number(w.value))),
        Widget::TextField(w) => {
            let role = if w.is_search() {
                Role::SearchField
            } else {
                Role::TextField
            };
            (role, named(&w.name), Some(Label::data(w.content())))
        }
        Widget::Badge(w) => (Role::Badge, named(&w.label), None),
        Widget::Progress(w) => (Role::Progress, named(&w.name), w.value.map(percent)),
        Widget::Checkbox(w) => (Role::Checkbox, named(&w.label), None),
        Widget::RadioGroup(w) => {
            let chosen = w.selected.and_then(|i| w.options.get(i)).cloned();
            (Role::RadioGroup, named(&w.name), chosen)
        }
        Widget::Tabs(w) => (
            Role::TabList,
            named(&w.name),
            w.tabs.get(w.selected).cloned(),
        ),
        Widget::InfoBar(w) => (Role::Alert, named(&w.title), w.message.clone()),
        Widget::EmptyState(w) => (Role::Status, named(&w.title), w.message.clone()),
        Widget::List(w) => (Role::List, w.name.clone(), None),
        Widget::ListItem(w) => (Role::ListItem, named(&w.label), w.sublabel.clone()),
        Widget::Scroll(w) => (Role::ScrollArea, w.name.clone(), None),
        Widget::Dialog(w) => (Role::Dialog, named(&w.title), None),
    })
}

/// The tone of a badge or info bar as a spoken description.
fn tone_of(widget: &Widget) -> Option<Label> {
    let tone = match widget {
        Widget::Badge(badge) => badge.tone,
        Widget::InfoBar(bar) => bar.tone,
        _ => return None,
    };
    tone.name_key().map(Label::key)
}

impl Ui {
    /// The accessibility node of one UI node; `None` for pure layout containers and
    /// unnamed (decorative) icons.
    pub fn a11y_node(&self, id: NodeId) -> Option<A11yNode> {
        let node = self.nodes.get(id.index())?;
        let (role, name, value) = describe_widget(&node.widget)?;
        let visual = self.visual(id);
        let state = A11yState {
            disabled: visual.disabled,
            focused: self.input.focus == Some(id),
            pressed: visual.pressed,
            busy: node.loading || matches!(&node.widget, Widget::Progress(p) if p.value.is_none()),
            invalid: node.error.is_some(),
            checked: match &node.widget {
                Widget::Switch(w) => Some(w.on),
                Widget::Checkbox(w) => Some(w.checked),
                _ => None,
            },
            expanded: matches!(node.widget, Widget::Select(_)).then(|| self.is_open(id)),
            selected: match &node.widget {
                Widget::ListItem(w) => Some(w.selected),
                _ => None,
            },
        };
        Some(A11yNode {
            id,
            role,
            name,
            value,
            error: node.error.clone(),
            description: node.tooltip.clone().or_else(|| tone_of(&node.widget)),
            state,
            rect: self.rect(id)?,
            focusable: self.interactive(id) && !visual.disabled,
        })
    }

    /// The nodes of the open menu: one per level and one per command of each level. They
    /// carry the id of the menu's owner; the highlighted command is the focused one.
    fn menu_nodes(&self) -> Vec<A11yNode> {
        let Some(state) = self.layers.menu.as_ref() else {
            return Vec::new();
        };
        let mut nodes = Vec::new();
        let blank = |role: Role, rect: crate::geom::Rect| A11yNode {
            id: state.owner,
            role,
            name: None,
            value: None,
            error: None,
            description: None,
            state: A11yState::default(),
            rect,
            focusable: false,
        };
        for (depth, panel) in state.panels.iter().enumerate() {
            let Some(level) = state.menu.level(&state.path[..depth.min(state.path.len())]) else {
                continue;
            };
            nodes.push(blank(Role::Menu, panel.rect));
            let deepest = depth == state.path.len();
            for (index, item) in level.items.iter().enumerate() {
                let Some(row) = panel.row(index) else {
                    continue;
                };
                let mut node = blank(Role::MenuItem, row);
                node.name = Some(item.label.clone());
                node.value = item.shortcut.clone();
                node.focusable = !item.disabled;
                node.state.disabled = item.disabled;
                node.state.focused = deepest && state.highlight == Some(index);
                node.state.checked = item.checked;
                let open = state.path.get(depth) == Some(&index);
                node.state.expanded = item.submenu.as_ref().map(|_| open);
                nodes.push(node);
            }
        }
        nodes
    }

    /// All accessibility nodes in logical (reading) order: the tree (only the topmost
    /// dialog while one is open – nothing behind it can be reached), then the open menu.
    pub fn a11y_tree(&mut self) -> Vec<A11yNode> {
        self.layout();
        let mut nodes: Vec<A11yNode> = self
            .reachable()
            .iter()
            .filter_map(|id| self.a11y_node(*id))
            .collect();
        nodes.extend(self.menu_nodes());
        nodes
    }
}
