//! Building the accessibility tree: one node per announced widget, plus nodes for the parts
//! a screen reader must reach one by one (radio options, tabs, open select options, the
//! button of a search or password field, table headers and rows, tree items, toasts).

use super::{A11yNode, A11yState, NodeId, Role, Ui};
use crate::geom::Rect;
use crate::widgets::{DialogKind, Label, TableCursor, TablePart, Tone, Widget, MAX_TOASTS};

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
        Widget::TextField(w) if w.is_password() => (Role::PasswordField, named(&w.name), None),
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
        Widget::InfoBar(w) => {
            let role = match w.tone {
                Tone::Warning | Tone::Danger => Role::Alert,
                _ => Role::Status,
            };
            (role, named(&w.title), w.message.clone())
        }
        Widget::EmptyState(w) => (Role::Status, named(&w.title), w.message.clone()),
        Widget::List(w) if w.navigation => (Role::Navigation, w.name.clone(), None),
        Widget::List(w) => (Role::List, w.name.clone(), None),
        Widget::ListItem(w) => (Role::ListItem, named(&w.label), w.sublabel.clone()),
        Widget::Scroll(w) => (Role::ScrollArea, w.name.clone(), None),
        Widget::Dialog(w) if w.alert && w.kind == DialogKind::Dialog => {
            (Role::AlertDialog, named(&w.title), None)
        }
        Widget::Dialog(w) => (Role::Dialog, named(&w.title), None),
        Widget::Table(w) => (Role::Table, named(&w.name), None),
        Widget::Tree(w) => (Role::Tree, named(&w.name), None),
        Widget::HeaderBar(w) => (Role::Banner, named(&w.title), None),
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
    /// The nearest ancestor that has an accessibility node of its own.
    fn announced_parent(&self, id: NodeId) -> Option<NodeId> {
        let mut current = self.parent(id);
        while let Some(node) = current {
            if self.widget(node).and_then(describe_widget).is_some() {
                return Some(node);
            }
            current = self.parent(node);
        }
        None
    }

    /// The accessibility node of one UI node; `None` for pure layout containers and
    /// unnamed (decorative) icons.
    pub fn a11y_node(&self, id: NodeId) -> Option<A11yNode> {
        let node = self.nodes.get(id.index())?;
        let (role, name, value) = describe_widget(&node.widget)?;
        let visual = self.visual(id);
        let mut out = A11yNode::new(id, role, self.rect(id)?);
        out.name = name;
        out.value = value;
        out.error = node.error.clone();
        out.description = node.tooltip.clone().or_else(|| tone_of(&node.widget));
        out.focusable = self.interactive(id) && !visual.disabled;
        out.parent = self.announced_parent(id);
        out.labelled_by = self
            .parent(id)
            .filter(|p| matches!(self.widget(*p), Some(w) if w.hosts_one_child()));
        out.state = A11yState {
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
            mixed: matches!(&node.widget, Widget::Checkbox(w) if w.mixed),
            expanded: matches!(node.widget, Widget::Select(_)).then(|| self.is_open(id)),
            selected: match &node.widget {
                Widget::ListItem(w) => Some(w.selected),
                _ => None,
            },
            level: None,
        };
        if let Widget::Slider(w) = &node.widget {
            out.range = Some((w.min, w.max, w.step));
        }
        if let (Some(list), Widget::ListItem(_)) = (self.parent(id), &node.widget) {
            let count = self.children(list).len();
            let at = self.children(list).iter().position(|c| *c == id);
            out.position = at.map(|a| (a + 1, count));
        }
        Some(out)
    }

    /// A part node of `owner` (its state follows the owner's disabled state).
    fn part(&self, owner: &A11yNode, part: usize, role: Role, rect: Rect) -> A11yNode {
        let mut node = A11yNode::new(owner.id, role, rect);
        node.part = Some(part);
        node.parent = Some(owner.id);
        node.state.disabled = owner.state.disabled;
        node
    }

    /// The parts of a widget that are announced one by one.
    fn parts_of(&self, owner: &A11yNode, out: &mut Vec<A11yNode>) {
        let id = owner.id;
        let (Some(rect), Some(widget)) = (self.rect(id), self.widget(id)) else {
            return;
        };
        // While a menu is open its highlighted command is where the keyboard is.
        let focused = owner.state.focused && self.layers.menu.is_none();
        let direction = self.env.direction;
        match widget {
            Widget::RadioGroup(group) => {
                let count = group.options.len();
                for (i, option) in group.options.iter().enumerate() {
                    let mut node = self.part(owner, i, Role::Radio, group.row(rect, i, self.mode));
                    node.name = Some(option.clone());
                    node.state.checked = Some(group.selected == Some(i));
                    node.state.focused = focused && group.current() == i;
                    node.position = Some((i + 1, count));
                    out.push(node);
                }
            }
            Widget::Tabs(tabs) => {
                let count = tabs.tabs.len();
                for (i, label) in tabs.tabs.iter().enumerate() {
                    let mut node = self.part(owner, i, Role::Tab, tabs.tab(rect, i, direction));
                    node.name = Some(label.clone());
                    node.state.selected = Some(tabs.selected == i);
                    node.state.focused = focused && tabs.selected == i;
                    node.position = Some((i + 1, count));
                    out.push(node);
                }
            }
            Widget::Select(_) => self.option_parts(owner, out),
            Widget::TextField(field) => {
                if let (Some(part), Some(button)) = (
                    field.end_button(),
                    field.end_button_rect(field.field(rect, self.mode), direction),
                ) {
                    let mut node = self.part(owner, part, Role::Button, button);
                    let key = if part == 0 {
                        "ui.search.clear"
                    } else {
                        "ui.password.show"
                    };
                    node.name = Some(Label::key(key));
                    node.state.pressed = field.is_revealed();
                    out.push(node);
                }
            }
            Widget::Table(_) => self.table_parts(owner, rect, out),
            Widget::Tree(_) => self.tree_parts(owner, rect, out),
            _ => {}
        }
    }

    /// The options of an open select; the highlighted one has the focus.
    fn option_parts(&self, owner: &A11yNode, out: &mut Vec<A11yNode>) {
        let (Some(popup), Some((_, view))) = (self.input.popup, self.popup_view()) else {
            return;
        };
        let Some(select) = self
            .select(owner.id)
            .ok()
            .filter(|_| popup.select == owner.id)
        else {
            return;
        };
        let count = select.options.len();
        for (i, option) in select.options.iter().enumerate() {
            let rect = view.row(i).unwrap_or_default();
            let mut node = self.part(owner, i, Role::Option, rect);
            node.name = Some(option.clone());
            node.state.selected = Some(select.selected == i);
            node.state.focused = popup.highlight == i;
            node.position = Some((i + 1, count));
            out.push(node);
        }
    }

    fn table_parts(&self, owner: &A11yNode, rect: Rect, out: &mut Vec<A11yNode>) {
        let Some(Widget::Table(table)) = self.widget(owner.id) else {
            return;
        };
        let header = table.band(rect, None, self.mode);
        for (c, label) in table.columns.iter().enumerate() {
            let cell = table.cell(header, c, self.env.direction);
            let mut node = self.part(
                owner,
                TablePart::Header(c).encode(),
                Role::ColumnHeader,
                cell,
            );
            node.name = Some(label.clone());
            node.value = table.sort.filter(|(s, _)| *s == c).map(|(_, up)| {
                Label::key(if up {
                    "ui.state.ascending"
                } else {
                    "ui.state.descending"
                })
            });
            node.state.focused = owner.state.focused && table.cursor == TableCursor::Header(c);
            node.position = Some((c + 1, table.columns.len()));
            out.push(node);
        }
        let count = table.rows.len();
        for (r, cells) in table.rows.iter().enumerate() {
            let band = table.band(rect, Some(r), self.mode);
            let mut node = self.part(owner, TablePart::Row(r).encode(), Role::TableRow, band);
            node.name = cells.first().cloned();
            node.value = cells.get(1).cloned();
            node.state.selected = Some(table.selected == Some(r));
            node.state.focused = owner.state.focused
                && table.cursor == TableCursor::Rows
                && table.selected == Some(r);
            node.position = Some((r + 1, count));
            out.push(node);
        }
    }

    fn tree_parts(&self, owner: &A11yNode, rect: Rect, out: &mut Vec<A11yNode>) {
        let Some(Widget::Tree(tree)) = self.widget(owner.id) else {
            return;
        };
        for (slot, index) in tree.visible().into_iter().enumerate() {
            let Some(item) = tree.items.get(index) else {
                continue;
            };
            let mut node = self.part(
                owner,
                index,
                Role::TreeItem,
                tree.row(rect, slot, self.mode),
            );
            node.name = Some(item.label.clone());
            node.state.level = Some(usize::from(item.depth) + 1);
            node.state.expanded = tree.has_children(index).then_some(item.expanded);
            node.state.selected = Some(tree.selected == Some(index));
            node.state.focused = owner.state.focused && tree.selected == Some(index);
            node.position = tree.position(index);
            out.push(node);
        }
    }

    /// The nodes of the open menu: one per level and one per command of each level. They
    /// carry the id of the menu's owner; the highlighted command is the focused one.
    fn menu_nodes(&self) -> Vec<A11yNode> {
        let Some(state) = self.layers.menu.as_ref() else {
            return Vec::new();
        };
        let mut nodes = Vec::new();
        for (depth, panel) in state.panels.iter().enumerate() {
            let Some(level) = state.menu.level(&state.path[..depth.min(state.path.len())]) else {
                continue;
            };
            let mut menu = A11yNode::new(state.owner, Role::Menu, panel.rect);
            menu.parent = Some(state.owner);
            nodes.push(menu);
            let deepest = depth == state.path.len();
            let count = level.items.len();
            for (index, item) in level.items.iter().enumerate() {
                let Some(row) = panel.row(index) else {
                    continue;
                };
                let mut node = A11yNode::new(state.owner, Role::MenuItem, row);
                node.part = Some(index);
                node.parent = Some(state.owner);
                node.name = Some(item.label.clone());
                node.value = item.shortcut.clone();
                node.focusable = !item.disabled;
                node.state.disabled = item.disabled;
                node.state.focused = deepest && state.highlight == Some(index);
                node.state.checked = item.checked;
                let open = state.path.get(depth) == Some(&index);
                node.state.expanded = item.submenu.as_ref().map(|_| open);
                node.position = Some((index + 1, count));
                nodes.push(node);
            }
        }
        nodes
    }

    /// The visible toasts, as polite status messages.
    /// Each toast: a status node with its message and tone, then its action and close
    /// buttons (parts after `MAX_TOASTS`, two per toast) so a screen reader reaches them.
    fn toast_nodes(&mut self) -> Vec<A11yNode> {
        let root = self.root();
        let views = self.toast_views();
        let mut nodes = Vec::with_capacity(3 * views.len());
        for (i, view) in views {
            let Some(shown) = self.layers.toasts.get(i) else {
                continue;
            };
            let mut node = A11yNode::new(root, Role::Status, view.rect);
            node.part = Some(i);
            node.name = Some(shown.toast.message.clone());
            node.description = shown.toast.tone.name_key().map(Label::key);
            nodes.push(node);
            let buttons = [
                (view.action, shown.toast.action.clone()),
                (Some(view.close), Some(Label::key("ui.toast.close"))),
            ];
            for (k, (rect, name)) in buttons.into_iter().enumerate() {
                if let (Some(rect), Some(name)) = (rect, name) {
                    let mut button = A11yNode::new(root, Role::Button, rect);
                    button.part = Some(MAX_TOASTS + 2 * i + k);
                    button.name = Some(name);
                    nodes.push(button);
                }
            }
        }
        nodes
    }

    /// All accessibility nodes in logical (reading) order: the tree (only the topmost
    /// dialog while one is open – nothing behind it can be reached) with the parts after
    /// their widget, then the open menu, then toasts.
    pub fn a11y_tree(&mut self) -> Vec<A11yNode> {
        self.layout();
        let mut nodes = Vec::with_capacity(self.reachable().len());
        for id in self.reachable() {
            if let Some(node) = self.a11y_node(*id) {
                let mut parts = Vec::new();
                self.parts_of(&node, &mut parts);
                nodes.push(node);
                nodes.extend(parts);
            }
        }
        nodes.extend(self.menu_nodes());
        nodes.extend(self.toast_nodes());
        nodes
    }
}
