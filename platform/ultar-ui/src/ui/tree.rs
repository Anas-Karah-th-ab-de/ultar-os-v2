//! Building the tree and reading or changing widget state from application code.

use super::{Node, NodeId, Ui, UiError, MAX_DEPTH, MAX_NODES};
use crate::layout::{LogicalRect, Sizing};
use crate::widgets::{Label, Select, Slider, Switch, TextField, Visual, Widget};

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
    pub(crate) fn node(&self, id: NodeId) -> Result<&Node, UiError> {
        self.nodes.get(id.index()).ok_or(UiError::UnknownNode(id))
    }

    pub(crate) fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, UiError> {
        self.nodes
            .get_mut(id.index())
            .ok_or(UiError::UnknownNode(id))
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

    fn depth(&self, id: NodeId) -> usize {
        let mut depth = 0;
        let mut current = self.nodes.get(id.index()).and_then(|n| n.parent);
        while let Some(parent) = current {
            depth += 1;
            current = self.nodes.get(parent.index()).and_then(|n| n.parent);
        }
        depth
    }

    /// Adds a content-sized widget as the last child of `parent`.
    pub fn add(&mut self, parent: NodeId, widget: impl Into<Widget>) -> Result<NodeId, UiError> {
        self.add_sized(parent, widget, Sizing::AUTO)
    }

    /// Adds a widget with explicit sizing as the last child of `parent`.
    ///
    /// Flex containers and cards take any number of children, a settings row takes exactly
    /// one (its control); other widgets take none.
    pub fn add_sized(
        &mut self,
        parent: NodeId,
        widget: impl Into<Widget>,
        sizing: Sizing,
    ) -> Result<NodeId, UiError> {
        let host = self.node(parent)?;
        let accepts = match &host.widget {
            Widget::Flex(_) | Widget::Card(_) => true,
            Widget::Row(_) => host.children.is_empty(),
            _ => false,
        };
        if !accepts {
            return Err(UiError::NotAContainer(parent));
        }
        if self.nodes.len() >= MAX_NODES {
            return Err(UiError::TooManyNodes);
        }
        if self.depth(parent) + 1 >= MAX_DEPTH {
            return Err(UiError::TooDeep);
        }
        let id = NodeId(self.nodes.len() as u32);
        self.nodes
            .push(new_node(widget.into(), sizing, Some(parent)));
        self.node_mut(parent)?.children.push(id);
        self.invalidate_layout();
        Ok(id)
    }

    /// Number of nodes including the root.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The widget of a node.
    pub fn widget(&self, id: NodeId) -> Option<&Widget> {
        self.nodes.get(id.index()).map(|n| &n.widget)
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
        while let Some(node) = current.and_then(|c| self.nodes.get(c.index())) {
            if node.disabled {
                return true;
            }
            current = node.parent;
        }
        false
    }

    /// The interaction and status flags a node is painted with.
    pub fn visual(&self, id: NodeId) -> Visual {
        let Some(node) = self.nodes.get(id.index()) else {
            return Visual::default();
        };
        let disabled = self.is_disabled(id);
        let open = self.input.popup.is_some_and(|p| p.select == id);
        Visual {
            hovered: self.input.hover == Some(id) && !disabled,
            pressed: (self.input.pressed == Some(id) || open) && !disabled,
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
            let inside = |ui: &Ui, node: Option<NodeId>| node.is_some_and(|n| ui.is_disabled(n));
            if inside(self, self.input.focus) {
                self.input.focus = None;
            }
            if inside(self, self.input.pressed) {
                self.input.pressed = None;
            }
            if inside(self, self.input.hover) {
                self.input.hover = None;
            }
            if inside(self, self.input.popup.map(|p| p.select)) {
                self.input.popup = None;
            }
        }
        self.repaint_all();
        Ok(())
    }

    /// Shows or hides the loading state (buttons, icon buttons, switches, selects). A loading
    /// widget keeps focus but cannot be activated.
    pub fn set_loading(&mut self, id: NodeId, loading: bool) -> Result<(), UiError> {
        let node = self.node_mut(id)?;
        if !node.widget.can_load() {
            return Err(UiError::NotApplicable(id));
        }
        node.loading = loading;
        if loading && self.input.popup.is_some_and(|p| p.select == id) {
            self.input.popup = None;
        }
        self.invalidate_layout();
        Ok(())
    }

    /// Sets or clears the error message of a text field or select.
    pub fn set_error(&mut self, id: NodeId, error: Option<Label>) -> Result<(), UiError> {
        let node = self.node_mut(id)?;
        if !node.widget.can_fail() {
            return Err(UiError::NotApplicable(id));
        }
        node.error = error;
        self.invalidate_layout();
        Ok(())
    }

    /// The error message of a node, if it has one.
    pub fn error(&self, id: NodeId) -> Option<&Label> {
        self.nodes.get(id.index()).and_then(|n| n.error.as_ref())
    }

    /// State of a switch.
    pub fn is_on(&self, id: NodeId) -> Result<bool, UiError> {
        Ok(self.switch(id)?.on)
    }

    /// Sets a switch without animation and without emitting an action.
    pub fn set_on(&mut self, id: NodeId, on: bool) -> Result<(), UiError> {
        let switch = self.switch_mut(id)?;
        switch.on = on;
        switch.toggled_at = None;
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

    /// Chosen option of a select.
    pub fn selected(&self, id: NodeId) -> Result<usize, UiError> {
        Ok(self.select(id)?.selected)
    }

    /// Chooses an option of a select without emitting an action.
    pub fn set_selected(&mut self, id: NodeId, index: usize) -> Result<(), UiError> {
        let select = self.select_mut(id)?;
        if index >= select.options.len() {
            return Err(UiError::OptionOutOfRange { node: id, index });
        }
        select.selected = index;
        self.touch(id);
        Ok(())
    }

    /// `true` while the options list of this select is open.
    pub fn is_open(&self, id: NodeId) -> bool {
        self.input.popup.is_some_and(|p| p.select == id)
    }

    /// Content of a text field.
    pub fn text(&self, id: NodeId) -> Result<&str, UiError> {
        Ok(self.text_field(id)?.content())
    }

    /// Replaces the content of a text field without emitting an action.
    pub fn set_text(&mut self, id: NodeId, text: &str) -> Result<(), UiError> {
        self.text_field_mut(id)?.set_text(text);
        self.touch(id);
        Ok(())
    }

    /// Replaces the label of a button, text, or settings row.
    pub fn set_label(&mut self, id: NodeId, label: Label) -> Result<(), UiError> {
        match &mut self.node_mut(id)?.widget {
            Widget::Button(b) => b.label = label,
            Widget::Text(t) => t.label = label,
            Widget::Row(r) => r.label = label,
            _ => {
                return Err(UiError::WrongWidget {
                    node: id,
                    expected: "button, text or row",
                })
            }
        }
        self.invalidate_layout();
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
