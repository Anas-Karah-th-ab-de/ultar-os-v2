//! Building the tree and reading or changing widget state from application code.

use super::{Node, NodeId, Slot, Ui, UiError, MAX_DEPTH, MAX_NODES};
use crate::geom::Rect;
use crate::layout::{LogicalRect, Sizing};
use crate::paint::TextSpec;
use crate::tokens::TypeSize;
use crate::widgets::Label;
use crate::widgets::{
    Checkbox, Dialog, ListItem, Progress, RadioGroup, Scroll, Select, Slider, Switch, Text,
    TextField, Visual, Widget,
};

pub(super) fn new_node(widget: Widget, sizing: Sizing, parent: Option<NodeId>) -> Node {
    Node {
        widget,
        sizing,
        parent,
        children: Vec::new(),
        frame: LogicalRect::default(),
        disabled: false,
        loading: false,
        error: None,
        measured: None,
        tooltip: None,
        context_menu: None,
        clip_parent: None,
        draggable: false,
        drop_target: false,
        slot: Slot::default(),
    }
}

macro_rules! typed_access {
    ($get:ident, $get_mut:ident, $variant:ident, $ty:ty, $name:literal) => {
        pub(crate) fn $get(&self, id: NodeId) -> Result<&$ty, UiError> {
            match &self.node(id)?.widget {
                Widget::$variant(w) => Ok(w),
                _ => Err(UiError::WrongWidget {
                    node: id,
                    expected: $name,
                }),
            }
        }

        pub(crate) fn $get_mut(&mut self, id: NodeId) -> Result<&mut $ty, UiError> {
            match &mut self.node_mut(id)?.widget {
                Widget::$variant(w) => Ok(w),
                _ => Err(UiError::WrongWidget {
                    node: id,
                    expected: $name,
                }),
            }
        }
    };
}

impl Ui {
    /// The live node `id` refers to (right slot and generation, not removed).
    pub(crate) fn slot(&self, id: NodeId) -> Option<&Node> {
        self.nodes
            .get(id.index())
            .filter(|n| n.slot.generation == id.generation() && !n.slot.removed)
    }

    /// See [`Ui::slot`].
    pub(crate) fn slot_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes
            .get_mut(id.index())
            .filter(|n| n.slot.generation == id.generation() && !n.slot.removed)
    }

    /// The current handle of slot `index`.
    pub(crate) fn id_at(&self, index: usize) -> NodeId {
        NodeId::new(
            index,
            self.nodes.get(index).map_or(0, |n| n.slot.generation),
        )
    }

    /// Puts `node` into a free slot (reused with the next generation) or a new one.
    fn store(&mut self, mut node: Node) -> NodeId {
        match self.layers.free_slots.pop() {
            Some(index) if index < self.nodes.len() => {
                let generation = self.nodes[index].slot.generation.saturating_add(1);
                node.slot = Slot {
                    generation,
                    removed: false,
                };
                self.nodes[index] = node;
                NodeId::new(index, generation)
            }
            _ => {
                self.nodes.push(node);
                NodeId::new(self.nodes.len() - 1, 0)
            }
        }
    }

    pub(crate) fn node(&self, id: NodeId) -> Result<&Node, UiError> {
        self.slot(id).ok_or(UiError::UnknownNode(id))
    }

    pub(crate) fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, UiError> {
        self.slot_mut(id).ok_or(UiError::UnknownNode(id))
    }

    typed_access!(switch, switch_mut, Switch, Switch, "switch");
    typed_access!(select, select_mut, Select, Select, "select");
    typed_access!(slider, slider_mut, Slider, Slider, "slider");
    typed_access!(
        text_field,
        text_field_mut,
        TextField,
        TextField,
        "text field"
    );
    typed_access!(checkbox, checkbox_mut, Checkbox, Checkbox, "checkbox");

    pub(crate) fn radio(&self, id: NodeId) -> Result<&RadioGroup, UiError> {
        match &self.node(id)?.widget {
            Widget::RadioGroup(group) => Ok(group),
            _ => Err(UiError::WrongWidget {
                node: id,
                expected: "radio group",
            }),
        }
    }

    typed_access!(
        progress_bar,
        progress_bar_mut,
        Progress,
        Progress,
        "progress"
    );
    typed_access!(list_item, list_item_mut, ListItem, ListItem, "list item");
    typed_access!(scroller, scroller_mut, Scroll, Scroll, "scroll container");
    typed_access!(dialog, dialog_mut, Dialog, Dialog, "dialog");

    fn depth(&self, id: NodeId) -> usize {
        let mut depth = 0;
        let mut current = self.slot(id).and_then(|n| n.parent);
        while let Some(parent) = current {
            depth += 1;
            current = self.slot(parent).and_then(|n| n.parent);
        }
        depth
    }

    /// Adds a content-sized widget as the last child of `parent`.
    pub fn add(&mut self, parent: NodeId, widget: impl Into<Widget>) -> Result<NodeId, UiError> {
        self.add_sized(parent, widget, Sizing::AUTO)
    }

    /// Adds a widget with explicit sizing as the last child of `parent`.
    ///
    /// Flex containers, cards, lists, scroll containers and dialogs take any number of
    /// children; a settings row, list item, info bar and empty state take exactly one (their
    /// control or action); other widgets take none. A dialog or sheet may be added to any
    /// node: it is not laid out there but floats above the surface (closed at first), and
    /// its title becomes its first child.
    pub fn add_sized(
        &mut self,
        parent: NodeId,
        widget: impl Into<Widget>,
        sizing: Sizing,
    ) -> Result<NodeId, UiError> {
        let widget = widget.into();
        let host = self.node(parent)?;
        let title = match &widget {
            Widget::Dialog(dialog) => Some(dialog.title.clone()),
            _ => None,
        };
        let accepts = match &host.widget {
            w if w.flex().is_some() => true,
            w if w.hosts_one_child() => host.children.is_empty(),
            _ => false,
        };
        if !accepts && title.is_none() {
            return Err(UiError::NotAContainer(parent));
        }
        let needed = if title.is_some() { 2 } else { 1 };
        if self.node_count() + needed > MAX_NODES {
            return Err(UiError::TooManyNodes);
        }
        if title.is_none() && self.depth(parent) + 1 >= MAX_DEPTH {
            return Err(UiError::TooDeep);
        }
        let id = match title {
            Some(title) => {
                let mut widget = widget;
                if let Widget::Dialog(dialog) = &mut widget {
                    dialog.host = Some(parent);
                }
                let id = self.store(new_node(widget, sizing, None));
                self.layers.dialogs.push(id);
                let spec = TextSpec::title(TypeSize::S16).weight(crate::text::Weight::Semibold);
                let heading = Text::new(title).spec(spec);
                let child = self.store(new_node(heading.into(), Sizing::AUTO, Some(id)));
                self.node_mut(id)?.children.push(child);
                id
            }
            None => {
                let id = self.store(new_node(widget, sizing, Some(parent)));
                self.node_mut(parent)?.children.push(id);
                id
            }
        };
        self.invalidate_node(id);
        Ok(id)
    }

    /// Number of live nodes including the root.
    pub fn node_count(&self) -> usize {
        self.nodes.len() - self.layers.free_slots.len()
    }

    /// The widget of a node.
    pub fn widget(&self, id: NodeId) -> Option<&Widget> {
        self.slot(id).map(|n| &n.widget)
    }

    /// The parent of a node; `None` for the root and for dialogs (they hang in no layout).
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.slot(id).and_then(|n| n.parent)
    }

    /// The children of a node in logical order.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        self.nodes
            .get(id.index())
            .map_or(&[], |n| n.children.as_slice())
    }

    /// `true` when the node or one of its ancestors is disabled.
    pub fn is_disabled(&self, id: NodeId) -> bool {
        let mut current = Some(id);
        while let Some(node) = current.and_then(|c| self.slot(c)) {
            if node.disabled {
                return true;
            }
            current = node.parent;
        }
        false
    }

    /// The interaction and status flags a node is painted with.
    pub fn visual(&self, id: NodeId) -> Visual {
        let Some(node) = self.slot(id) else {
            return Visual::default();
        };
        let disabled = self.is_disabled(id);
        // An open options list or drop-down menu keeps its owner pressed (boards "Menus":
        // the menu button shows the pressed fill while its menu is open).
        let open = self.input.popup.is_some_and(|p| p.select == id) || self.drop_down_owner(id);
        Visual {
            hovered: self.input.hover == Some(id) && !disabled,
            pressed: (self.input.is_pressed(id) || open) && !disabled,
            focused: self.input.focus == Some(id) && self.input.focus_visible && !disabled,
            disabled,
            loading: node.loading,
            error: node.error.is_some(),
        }
    }

    /// Enables or disables a node (and with it everything below it).
    pub fn set_disabled(&mut self, id: NodeId, disabled: bool) -> Result<(), UiError> {
        self.node_mut(id)?.disabled = disabled;
        if disabled {
            self.drop_interaction_inside_disabled();
        }
        self.repaint_all();
        // A node enabled again under the resting pointer is hovered at once.
        self.rehover();
        Ok(())
    }

    /// Lets the user drag a node that takes presses (a button, a list item, a card with an
    /// action): a press that moves beyond the touch slop reports `DragStarted`, `Dragged`
    /// with the distance from the press and `DragEnded` instead of activating the node.
    pub fn set_draggable(&mut self, id: NodeId, draggable: bool) -> Result<(), UiError> {
        self.node_mut(id)?.draggable = draggable;
        Ok(())
    }

    /// Forgets focus, hover, presses, the open list and the open menu of nodes that are
    /// disabled now.
    fn drop_interaction_inside_disabled(&mut self) {
        let inside = |ui: &Ui, node: Option<NodeId>| node.is_some_and(|n| ui.is_disabled(n));
        if inside(self, self.input.focus) {
            // The next Tab continues from this place (not from the first widget).
            self.input.focus_hint = self.input.focus;
            self.input.focus = None;
        }
        if inside(self, self.input.mouse.map(|p| p.node)) {
            self.input.mouse = None;
        }
        if inside(self, self.input.finger.map(|p| p.node)) {
            self.input.finger = None;
        }
        if inside(self, self.input.hover) {
            self.input.hover = None;
            self.input.hover_part = None;
            self.layers.hover_since = None;
        }
        if inside(self, self.input.popup.map(|p| p.select)) {
            self.input.popup = None;
        }
        if inside(self, self.layers.menu.as_ref().map(|m| m.owner)) {
            self.layers.menu = None;
        }
        if inside(self, self.layers.tip.map(|t| t.node)) {
            self.layers.tip = None;
        }
        if inside(self, self.layers.gestures.reveal.map(|r| r.0)) {
            self.hide_password();
        }
        // A held button stops repeating and reports its release.
        if let Some(hold) = self
            .layers
            .gestures
            .hold
            .filter(|h| inside(self, Some(h.node)))
        {
            self.layers.gestures.hold = None;
            let released = crate::event::Action {
                node: hold.node,
                kind: crate::event::ActionKind::Released,
            };
            self.queue(vec![released]);
        }
    }

    /// Shows or hides the loading state (buttons, icon buttons, switches, selects, check
    /// boxes, list items, sliders, text fields). A loading widget keeps focus but cannot be
    /// activated; a loading text field (a value being checked) can still be typed into.
    /// Radio groups and tabs have no loading state (DESKTOP-SPEC §3 names the exception).
    pub fn set_loading(&mut self, id: NodeId, loading: bool) -> Result<(), UiError> {
        let node = self.node_mut(id)?;
        if !node.widget.can_load() {
            return Err(UiError::NotApplicable(id));
        }
        node.loading = loading;
        if loading && self.input.popup.is_some_and(|p| p.select == id) {
            self.input.popup = None;
        }
        self.invalidate_node(id);
        Ok(())
    }

    /// Sets or clears the error message of a text field, select, check box, slider or list
    /// item (shown below it with an icon; a list item shows it instead of its sublabel).
    pub fn set_error(&mut self, id: NodeId, error: Option<Label>) -> Result<(), UiError> {
        let node = self.node_mut(id)?;
        if !node.widget.can_fail() {
            return Err(UiError::NotApplicable(id));
        }
        node.error = error;
        self.invalidate_node(id);
        Ok(())
    }

    /// The error message of a node, if it has one.
    pub fn error(&self, id: NodeId) -> Option<&Label> {
        self.slot(id).and_then(|n| n.error.as_ref())
    }

    /// State of a switch.
    pub fn is_on(&self, id: NodeId) -> Result<bool, UiError> {
        Ok(self.switch(id)?.on)
    }

    /// Sets a switch without animation and without emitting an action.
    pub fn set_on(&mut self, id: NodeId, on: bool) -> Result<(), UiError> {
        self.switch_mut(id)?.set(on);
        self.touch(id);
        Ok(())
    }

    /// Value of a slider.
    pub fn value(&self, id: NodeId) -> Result<f32, UiError> {
        Ok(self.slider(id)?.value)
    }

    /// Sets a slider (clamped and snapped) without emitting an action.
    pub fn set_value(&mut self, id: NodeId, value: f32) -> Result<(), UiError> {
        let slider = self.slider_mut(id)?;
        slider.value = slider.snap(value);
        self.touch(id);
        Ok(())
    }

    /// Chosen option of a select, shown tab of a tab bar, or chosen option of a radio group
    /// (`OptionOutOfRange` with index 0 while a radio group has no choice yet; see
    /// [`Ui::radio_selected`]).
    pub fn selected(&self, id: NodeId) -> Result<usize, UiError> {
        match &self.node(id)?.widget {
            Widget::Select(select) => Ok(select.selected),
            Widget::Tabs(tabs) => Ok(tabs.selected),
            Widget::RadioGroup(group) => group
                .selected
                .ok_or(UiError::OptionOutOfRange { node: id, index: 0 }),
            _ => Err(UiError::WrongWidget {
                node: id,
                expected: "select, tabs or radio group",
            }),
        }
    }

    /// Chooses an option of a select, a tab or a radio option without emitting an action.
    pub fn set_selected(&mut self, id: NodeId, index: usize) -> Result<(), UiError> {
        let node = self.node_mut(id)?;
        let count = match &node.widget {
            Widget::Select(select) => select.options.len(),
            Widget::Tabs(tabs) => tabs.tabs.len(),
            Widget::RadioGroup(group) => group.options.len(),
            _ => {
                return Err(UiError::WrongWidget {
                    node: id,
                    expected: "select, tabs or radio group",
                })
            }
        };
        if index >= count {
            return Err(UiError::OptionOutOfRange { node: id, index });
        }
        if node.widget.option_unavailable(index) {
            return Err(UiError::OptionUnavailable { node: id, index });
        }
        match &mut node.widget {
            Widget::Select(select) => select.selected = index,
            Widget::Tabs(tabs) => tabs.selected = index,
            Widget::RadioGroup(group) => group.selected = Some(index),
            _ => {}
        }
        self.touch(id);
        self.remeasure_tabs(id);
        self.keep_tab_visible(id);
        if self.is_open(id) {
            // The open list shows the check mark and the bold option: repaint it too.
            self.touch_popup();
        }
        Ok(())
    }

    /// `true` while the options list of this select is open.
    pub fn is_open(&self, id: NodeId) -> bool {
        self.input.popup.is_some_and(|p| p.select == id)
    }

    /// Rectangle of the action button inside a text field ([`crate::widgets::TextField::end_action`]);
    /// `None` when the field has none or is not shown.
    pub fn end_action_rect(&mut self, id: NodeId) -> Result<Option<Rect>, UiError> {
        self.layout();
        let rect = self.rect(id);
        let (mode, direction) = (self.mode, self.env.direction);
        let field = self.text_field(id)?;
        Ok(rect.and_then(|r| {
            let button = field.end_button_rect(field.field(r, mode), direction);
            button.filter(|_| field.end_button() == Some(2))
        }))
    }

    /// Content of a text field.
    pub fn text(&self, id: NodeId) -> Result<&str, UiError> {
        Ok(self.text_field(id)?.content())
    }

    /// Replaces the content of a text field without emitting an action.
    pub fn set_text(&mut self, id: NodeId, text: &str) -> Result<(), UiError> {
        self.text_field_mut(id)?.set_text(text);
        if self.layers.gestures.reveal.is_some_and(|r| r.0 == id) {
            self.hide_password();
        }
        self.touch(id);
        Ok(())
    }

    /// Replaces the label of a button, text, settings row, checkbox, badge or list item, or
    /// the title of an info bar or empty state.
    pub fn set_label(&mut self, id: NodeId, label: Label) -> Result<(), UiError> {
        match &mut self.node_mut(id)?.widget {
            Widget::Button(b) => b.label = label,
            Widget::Text(t) => t.label = label,
            Widget::Row(r) => r.label = label,
            Widget::Checkbox(c) => c.label = label,
            Widget::Badge(b) => b.label = label,
            Widget::ListItem(i) => i.label = label,
            Widget::InfoBar(b) => b.title = label,
            Widget::EmptyState(e) => e.title = label,
            _ => {
                return Err(UiError::WrongWidget {
                    node: id,
                    expected: "a widget with a label",
                })
            }
        }
        self.invalidate_node(id);
        Ok(())
    }

    /// The node that has keyboard focus.
    pub fn focused(&self) -> Option<NodeId> {
        self.input.focus
    }

    /// The node under the pointer.
    pub fn hovered(&self) -> Option<NodeId> {
        self.input.hover
    }
}
