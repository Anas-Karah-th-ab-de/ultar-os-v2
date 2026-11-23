//! Accessibility nodes: what a screen reader needs to announce and operate the tree.

use super::NodeId;
use crate::geom::Rect;
use crate::widgets::Label;
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
    /// A warning or error message that interrupts.
    Alert,
    /// A message that does not interrupt (info, success, empty state, toast).
    Status,
    /// A list.
    List,
    /// A list item.
    ListItem,
    /// A scroll container.
    ScrollArea,
    /// A dialog, sheet or panel.
    Dialog,
    /// A search field.
    SearchField,
    /// An open menu level.
    Menu,
    /// A command of an open menu.
    MenuItem,
    /// One option of a radio group.
    Radio,
    /// One tab of a tab bar.
    Tab,
    /// One option of an open select.
    Option,
    /// A password field (its value is never announced).
    PasswordField,
    /// A dialog about something serious.
    AlertDialog,
    /// A table.
    Table,
    /// The header of a table column.
    ColumnHeader,
    /// A row of a table.
    TableRow,
    /// A tree.
    Tree,
    /// An item of a tree.
    TreeItem,
    /// The navigation list of a sidebar.
    Navigation,
    /// A header bar.
    Banner,
    /// A cell of a table row (named by its column header).
    Cell,
}

/// Parts of the accessibility tree are numbered per node; these bases keep the numbers of
/// different kinds apart, so that `(id, part)` is unique in the whole tree.
/// A table cell is `CELL_PARTS + row * MAX_TABLE_COLUMNS + column`.
pub const CELL_PARTS: usize = 1 << 24;
/// An open menu level `d` is `MENU_PARTS + d * MENU_LEVEL_PARTS`, its item `i` that plus
/// `1 + i` (the owner of a menu may have parts of its own).
pub const MENU_PARTS: usize = 1 << 28;
/// Part numbers one menu level spans.
pub const MENU_LEVEL_PARTS: usize = 1 << 8;
/// Toast `i` is `TOAST_PARTS + 3 * i`, its action and close buttons the next two.
pub const TOAST_PARTS: usize = 1 << 29;

impl Role {
    /// All roles.
    pub const ALL: [Role; 36] = [
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
        Role::Radio,
        Role::Tab,
        Role::Option,
        Role::PasswordField,
        Role::AlertDialog,
        Role::Table,
        Role::ColumnHeader,
        Role::TableRow,
        Role::Tree,
        Role::TreeItem,
        Role::Navigation,
        Role::Banner,
        Role::Cell,
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
            Role::Radio => "ui.role.radio",
            Role::Tab => "ui.role.tab",
            Role::Option => "ui.role.option",
            Role::PasswordField => "ui.role.password_field",
            Role::AlertDialog => "ui.role.alert_dialog",
            Role::Table => "ui.role.table",
            Role::ColumnHeader => "ui.role.column_header",
            Role::TableRow => "ui.role.table_row",
            Role::Tree => "ui.role.tree",
            Role::TreeItem => "ui.role.tree_item",
            Role::Navigation => "ui.role.navigation",
            Role::Banner => "ui.role.banner",
            Role::Cell => "ui.role.cell",
        }
    }
}

/// The state of a node as announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct A11yState {
    /// Cannot be operated.
    pub disabled: bool,
    /// Has keyboard focus (for parts: the part the keyboard is on).
    pub focused: bool,
    /// Is held down.
    pub pressed: bool,
    /// Waits for a result.
    pub busy: bool,
    /// Holds an invalid value.
    pub invalid: bool,
    /// On/off for switches, checked or not for check boxes and radio options.
    pub checked: Option<bool>,
    /// Neither checked nor unchecked (a check box standing for several things).
    pub mixed: bool,
    /// Open/closed for selects, menu items with a submenu and tree items with children.
    pub expanded: Option<bool>,
    /// Selected or not, for list items, tabs, options, table rows, tree items.
    pub selected: Option<bool>,
    /// Level of a tree item (1 = top).
    pub level: Option<usize>,
}

/// One node of the accessibility tree.
#[derive(Debug, Clone, PartialEq)]
pub struct A11yNode {
    /// The UI node (for parts and menus: the node they belong to).
    pub id: NodeId,
    /// For a part of a widget (radio option, tab, select option, clear button, table row,
    /// tree item, toast): its index; `None` for the widget itself.
    pub part: Option<usize>,
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
    /// The nearest announced ancestor (for parts: their widget).
    pub parent: Option<NodeId>,
    /// Position among its siblings and their count, 1-based ("2 of 3").
    pub position: Option<(usize, usize)>,
    /// The node whose text names it (the settings row or list item around a control).
    pub labelled_by: Option<NodeId>,
    /// The part of `labelled_by` that names this node (a cell: its column header).
    pub labelled_by_part: Option<usize>,
    /// Smallest value, largest value and step of a slider.
    pub range: Option<(f32, f32, f32)>,
}

impl A11yNode {
    /// A node of `role` for `id` with nothing else set.
    pub fn new(id: NodeId, role: Role, rect: Rect) -> A11yNode {
        A11yNode {
            id,
            part: None,
            role,
            name: None,
            value: None,
            error: None,
            description: None,
            state: A11yState::default(),
            rect,
            focusable: false,
            parent: None,
            position: None,
            labelled_by: None,
            labelled_by_part: None,
            range: None,
        }
    }

    /// The state words of this node: checked words for check boxes and radios, on/off for
    /// switches, then open, selected, disabled, busy, invalid.
    fn state_keys(&self) -> Vec<&'static str> {
        let state = &self.state;
        let checked = match (state.checked, state.mixed, self.role) {
            (_, true, _) => Some("ui.state.mixed"),
            (Some(true), _, Role::Switch) => Some("ui.state.on"),
            (Some(false), _, Role::Switch) => Some("ui.state.off"),
            (Some(true), _, _) => Some("ui.state.checked"),
            (Some(false), _, _) => Some("ui.state.unchecked"),
            (None, ..) => None,
        };
        let expanded = state.expanded.map(|e| {
            if e {
                "ui.state.expanded"
            } else {
                "ui.state.collapsed"
            }
        });
        [
            checked,
            expanded,
            state
                .selected
                .and_then(|s| s.then_some("ui.state.selected")),
            state.disabled.then_some("ui.state.disabled"),
            state.busy.then_some("ui.state.busy"),
            state.invalid.then_some("ui.state.invalid"),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// The sentence a screen reader speaks: name, role, value, level, states, position,
    /// error, description.
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
        if let Some(level) = self.state.level {
            let level = level.to_string();
            push(
                Label::key_with("ui.a11y.level", &[("level", &level)])
                    .resolve(tr)
                    .into_owned()
                    .into(),
            );
        }
        for key in self.state_keys() {
            push(tr.t(key));
        }
        if let Some((pos, count)) = self.position {
            let (pos, count) = (pos.to_string(), count.to_string());
            let args = [("pos", pos.as_str()), ("count", count.as_str())];
            push(
                Label::key_with("ui.a11y.position", &args)
                    .resolve(tr)
                    .into_owned()
                    .into(),
            );
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
