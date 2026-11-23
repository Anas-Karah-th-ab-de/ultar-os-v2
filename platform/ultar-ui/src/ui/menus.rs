//! The open menu: placement of its levels, pointer and keyboard operation, painting.

use super::{MenuOrigin, MenuState, NodeId, Ui, UiError};
use crate::event::{Action, ActionKind, Key};
use crate::geom::Point;
use crate::paint::Painter;
use crate::widgets::{MeasureCx, Menu, MenuPanel, PaintCx};

impl Ui {
    /// Opens `menu` as a drop-down of `anchor` (below it, aligned with its start edge).
    /// Choosing an item emits `MenuChosen(id)` for `anchor`.
    pub fn open_menu(&mut self, anchor: NodeId, menu: Menu) -> Result<(), UiError> {
        self.start_menu(anchor, MenuOrigin::Below, menu)
    }

    /// Opens `menu` as a context menu at `pos` (logical points). Choosing an item emits
    /// `MenuChosen(id)` for `owner`.
    pub fn open_context_menu(
        &mut self,
        owner: NodeId,
        pos: Point,
        menu: Menu,
    ) -> Result<(), UiError> {
        self.start_menu(owner, MenuOrigin::At(pos), menu)
    }

    /// Sets or removes the context menu of a node: a secondary click on it (or on anything
    /// inside it that has none of its own) and the menu key while focus is inside open it.
    pub fn set_context_menu(&mut self, id: NodeId, menu: Option<Menu>) -> Result<(), UiError> {
        if menu.as_ref().is_some_and(|m| !m.within_limits()) {
            return Err(UiError::InvalidMenu);
        }
        self.node_mut(id)?.context_menu = menu.map(Box::new);
        Ok(())
    }

    /// Closes the open menu without emitting an action.
    pub fn close_menu(&mut self) {
        self.close_menu_with(&mut Vec::new());
    }

    /// The node the open menu belongs to.
    pub fn menu_owner(&self) -> Option<NodeId> {
        self.layers.menu.as_ref().map(|m| m.owner)
    }

    /// The open submenu chain (item indices from the top level) and the highlighted item of
    /// the deepest open level.
    pub fn menu_position(&self) -> Option<(&[usize], Option<usize>)> {
        self.layers
            .menu
            .as_ref()
            .map(|m| (m.path.as_slice(), m.highlight))
    }

    /// The rectangle of an open menu level (0 = top level) in logical points.
    pub fn menu_rect(&self, level: usize) -> Option<crate::geom::Rect> {
        let menu = self.layers.menu.as_ref()?;
        menu.panels.get(level).map(|panel| panel.rect)
    }

    fn start_menu(&mut self, owner: NodeId, origin: MenuOrigin, menu: Menu) -> Result<(), UiError> {
        self.node(owner)?;
        if !menu.within_limits() {
            return Err(UiError::InvalidMenu);
        }
        self.layout();
        let mut dropped = Vec::new();
        self.close_popup(&mut dropped);
        self.close_menu_with(&mut dropped);
        self.hide_tooltip();
        self.set_hover(None);
        // Opened from the keyboard, the first command is ready under the fingers.
        let highlight = if self.input.focus_visible {
            menu.step(None, true)
        } else {
            None
        };
        self.layers.menu = Some(MenuState {
            owner,
            menu,
            origin,
            path: Vec::new(),
            highlight,
            panels: Vec::new(),
        });
        self.place_menu();
        Ok(())
    }

    /// Opens the context menu registered for `id` or the nearest node above it.
    pub(crate) fn open_registered_menu(
        &mut self,
        id: NodeId,
        pos: Option<Point>,
        out: &mut Vec<Action>,
    ) -> bool {
        let mut current = Some(id);
        while let Some(node) = current {
            if let Some(menu) = self.nodes[node.index()].context_menu.as_deref().cloned() {
                let origin = pos.map_or(MenuOrigin::Below, MenuOrigin::At);
                if self.start_menu(node, origin, menu).is_ok() {
                    out.push(Action {
                        node,
                        kind: ActionKind::Opened,
                    });
                    return true;
                }
            }
            current = self.nodes[node.index()].parent;
        }
        false
    }

    /// The deepest visible node under `pos` (operable or not), for context menus.
    pub(crate) fn node_at(&self, pos: Point) -> Option<NodeId> {
        self.reachable()
            .iter()
            .rev()
            .copied()
            .find(|id| self.visible_at(*id, pos))
    }

    /// Places the open menu again after a layout (its owner may have moved, its texts may
    /// have another language). A menu whose owner is gone from the visible tree closes.
    pub(crate) fn replace_menu(&mut self) {
        let Some(owner) = self.menu_owner() else {
            return;
        };
        if self.order.contains(&owner) {
            self.place_menu();
        } else {
            self.layers.menu = None;
        }
    }

    /// Computes the panels of all open levels and marks them for repaint.
    fn place_menu(&mut self) {
        self.damage_menu();
        let size = self.size();
        let anchor = self.layers.menu.as_ref().and_then(|m| self.rect(m.owner));
        let Some(state) = self.layers.menu.as_mut() else {
            return;
        };
        let mut cx = MeasureCx {
            res: self.res.get_mut(),
            env: &self.env,
            tr: &self.tr,
            mode: self.mode,
        };
        let mut panels: Vec<MenuPanel> = Vec::with_capacity(state.path.len() + 1);
        let top = match (state.origin, anchor) {
            (MenuOrigin::At(pos), _) => state.menu.panel_at(&mut cx, size, pos),
            (MenuOrigin::Below, Some(rect)) => state.menu.panel_below(&mut cx, size, rect),
            (MenuOrigin::Below, None) => state.menu.panel_at(&mut cx, size, Point::default()),
        };
        panels.push(top);
        for depth in 0..state.path.len() {
            let Some(level) = state.menu.level(&state.path[..=depth]) else {
                state.path.truncate(depth);
                break;
            };
            let parent = &panels[depth];
            let Some(row) = parent.row(state.path[depth]) else {
                state.path.truncate(depth);
                break;
            };
            let panel = level.panel_beside(&mut cx, size, parent.rect, row);
            panels.push(panel);
        }
        state.panels = panels;
        self.damage_menu();
    }

    fn damage_menu(&mut self) {
        let rects: Vec<_> = self
            .layers
            .menu
            .as_ref()
            .map_or(Vec::new(), |m| m.panels.iter().map(|p| p.rect).collect());
        for rect in rects {
            self.damage_floating(rect);
        }
    }

    /// Closes the menu, if open, and reports `Closed` for its owner.
    pub(crate) fn close_menu_with(&mut self, out: &mut Vec<Action>) {
        self.damage_menu();
        if let Some(state) = self.layers.menu.take() {
            out.push(Action {
                node: state.owner,
                kind: ActionKind::Closed,
            });
        }
    }

    /// The level and item under `pos`: (level, item); the level alone when the pointer is on
    /// a panel but on no item.
    fn menu_hit(&self, pos: Point) -> Option<(usize, Option<usize>)> {
        let state = self.layers.menu.as_ref()?;
        state
            .panels
            .iter()
            .enumerate()
            .rev()
            .find(|(_, panel)| panel.rect.contains(pos))
            .map(|(level, panel)| (level, panel.item_at(pos)))
    }

    /// Highlights `item` of `level`, closing deeper levels, and opens its submenu if it has
    /// one and `open` is set.
    fn menu_select(&mut self, level: usize, item: Option<usize>, open: bool) {
        let Some(state) = self.layers.menu.as_mut() else {
            return;
        };
        let before = (state.path.clone(), state.highlight);
        let usable = item.filter(|i| {
            state
                .menu
                .level(&state.path[..level.min(state.path.len())])
                .and_then(|menu| menu.items.get(*i))
                .is_some_and(|entry| !entry.disabled)
        });
        let keeps_child = state.path.get(level) == usable.as_ref() && usable.is_some();
        if keeps_child {
            // The pointer went back to the parent item of the open submenu.
            state.path.truncate(level + 1);
            state.highlight = None;
        } else {
            state.path.truncate(level);
            state.highlight = usable;
            let has_submenu = usable.is_some_and(|i| {
                state
                    .menu
                    .level(&state.path)
                    .and_then(|menu| menu.items.get(i))
                    .is_some_and(|entry| entry.submenu.is_some())
            });
            if let (true, true, Some(i)) = (open, has_submenu, usable) {
                state.path.push(i);
                state.highlight = None;
            }
        }
        if before != (state.path.clone(), state.highlight) {
            self.place_menu();
        }
    }

    /// Pointer movement while a menu is open. Returns `true` (the menu is modal).
    pub(crate) fn menu_hover(&mut self, pos: Point) -> bool {
        if let Some((level, item)) = self.menu_hit(pos) {
            if item.is_some() {
                self.menu_select(level, item, true);
            }
        }
        true
    }

    /// A press while a menu is open: beside all panels it closes the menu.
    pub(crate) fn menu_press(&mut self, pos: Point, out: &mut Vec<Action>) -> bool {
        match self.menu_hit(pos) {
            Some((level, item)) => self.menu_select(level, item, true),
            None => self.close_menu_with(out),
        }
        true
    }

    /// A release while a menu is open: on a command it chooses it.
    pub(crate) fn menu_release(&mut self, pos: Point, out: &mut Vec<Action>) -> bool {
        if let Some((level, Some(item))) = self.menu_hit(pos) {
            self.menu_select(level, Some(item), true);
            let at_level = self
                .layers
                .menu
                .as_ref()
                .is_some_and(|m| m.path.len() == level && m.highlight == Some(item));
            if at_level {
                self.menu_choose(out);
            }
        }
        true
    }

    /// Chooses the highlighted command: opens its submenu, or reports it and closes.
    fn menu_choose(&mut self, out: &mut Vec<Action>) {
        let Some(state) = self.layers.menu.as_mut() else {
            return;
        };
        let item = state
            .highlight
            .and_then(|i| state.menu.level(&state.path).and_then(|m| m.items.get(i)));
        let Some(item) = item.filter(|item| !item.disabled) else {
            return;
        };
        if let (Some(submenu), Some(index)) = (&item.submenu, state.highlight) {
            let first = submenu.step(None, true);
            state.path.push(index);
            state.highlight = first;
            self.place_menu();
            return;
        }
        let action = Action {
            node: state.owner,
            kind: ActionKind::MenuChosen(item.id),
        };
        out.push(action);
        self.close_menu_with(out);
    }

    /// Leaves the deepest submenu; `false` at the top level.
    fn menu_back(&mut self) -> bool {
        let Some(state) = self.layers.menu.as_mut() else {
            return false;
        };
        match state.path.pop() {
            Some(parent) => {
                state.highlight = Some(parent);
                self.place_menu();
                true
            }
            None => false,
        }
    }

    fn menu_step(&mut self, from_current: bool, forward: bool) {
        let Some(state) = self.layers.menu.as_mut() else {
            return;
        };
        let from = state.highlight.filter(|_| from_current);
        let next = state
            .menu
            .level(&state.path)
            .and_then(|menu| menu.step(from, forward));
        if next != state.highlight {
            state.highlight = next;
            self.place_menu();
        }
    }

    /// A key while a menu is open. Every key is consumed: the menu is modal.
    pub(crate) fn menu_key(&mut self, key: Key, towards_end: bool, out: &mut Vec<Action>) -> bool {
        let opens = self.layers.menu.as_ref().is_some_and(|state| {
            state.highlight.is_some_and(|i| {
                state
                    .menu
                    .level(&state.path)
                    .and_then(|menu| menu.items.get(i))
                    .is_some_and(|entry| entry.submenu.is_some())
            })
        });
        match key {
            Key::ArrowDown => self.menu_step(true, true),
            Key::ArrowUp => self.menu_step(true, false),
            Key::Home => self.menu_step(false, true),
            Key::End => self.menu_step(false, false),
            Key::Enter | Key::Space => self.menu_choose(out),
            Key::ArrowLeft | Key::ArrowRight if towards_end && opens => self.menu_choose(out),
            Key::ArrowLeft | Key::ArrowRight if !towards_end => {
                self.menu_back();
            }
            Key::Escape => {
                if !self.menu_back() {
                    self.close_menu_with(out);
                }
            }
            Key::Tab => self.close_menu_with(out),
            _ => {}
        }
        true
    }

    /// Paints all open menu levels.
    pub(crate) fn paint_menu(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>) {
        let Some(state) = self.layers.menu.as_ref() else {
            return;
        };
        for (depth, panel) in state.panels.iter().enumerate() {
            let Some(level) = state.menu.level(&state.path[..depth.min(state.path.len())]) else {
                continue;
            };
            let deepest = depth == state.path.len();
            let highlight = state.highlight.filter(|_| deepest);
            level.paint(p, cx, panel, highlight, state.path.get(depth).copied());
        }
    }
}
