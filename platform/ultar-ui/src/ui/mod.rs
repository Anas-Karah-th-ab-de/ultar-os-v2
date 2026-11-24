//! The widget tree with its interaction state: build it, feed it events, paint it.

mod access;
mod access_describe;
mod access_tree;
mod clock;
mod edit;
mod error;
mod frame;
mod layers;
mod measure;
mod menus;
mod modal;
mod programmatic;
mod render;
mod scrolling;
mod settings;
mod state;
mod toasts;
mod tree;

pub use access::{
    A11yNode, A11yState, Role, CELL_PARTS, MENU_LEVEL_PARTS, MENU_PARTS, TOAST_PARTS,
};
pub use error::UiError;

pub(crate) use layers::{Drag, DragKind, Layers, MenuOrigin, MenuState, PaintStep, Tip};

use crate::geom::{Rect, Size};
use crate::layout::{Flex, LogicalRect, Sizing};
use crate::paint::{PaintEnv, Resources};
use crate::text::TextEngine;
use crate::tokens::{ColorRole, InputMode, SizeClass, Space};
use crate::widgets::{Label, Menu, Widget};
use std::cell::RefCell;
use ultar_i18n::{Lang, Translator};

/// Most nodes one tree may hold.
pub const MAX_NODES: usize = 4096;
/// Deepest nesting of nodes.
pub const MAX_DEPTH: usize = 64;
/// Most separate dirty rectangles before the whole surface is repainted.
const MAX_DIRTY: usize = 16;
/// Most actions that wait for the next event.
const MAX_PENDING: usize = 32;

/// Handle of a node in a [`Ui`]: the slot (low 16 bits) and the 32-bit generation of the
/// slot (above). A removed node's slot is reused with the next generation – a slot whose
/// generation would wrap is retired instead – so an old
/// handle never reaches the new node (it answers `UiError::UnknownNode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u64);

impl NodeId {
    pub(crate) fn new(index: usize, generation: u32) -> NodeId {
        NodeId((u64::from(generation) << 16) | (index as u64 & 0xFFFF))
    }

    pub(crate) fn index(self) -> usize {
        (self.0 & 0xFFFF) as usize
    }

    pub(crate) fn generation(self) -> u32 {
        (self.0 >> 16) as u32
    }
}

/// Generation of a node slot and whether the node in it was removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Slot {
    pub(crate) generation: u32,
    pub(crate) removed: bool,
}

pub(crate) struct Node {
    pub(crate) widget: Widget,
    pub(crate) sizing: Sizing,
    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
    pub(crate) frame: LogicalRect,
    pub(crate) disabled: bool,
    pub(crate) loading: bool,
    pub(crate) error: Option<Label>,
    /// Last measurement: available width and resulting size.
    pub(crate) measured: Option<(f32, Size)>,
    /// Explanation shown on hover or keyboard focus.
    pub(crate) tooltip: Option<Label>,
    /// Commands offered by a secondary click or the menu key.
    pub(crate) context_menu: Option<Box<Menu>>,
    /// The node can be dragged (`Ui::set_draggable`).
    pub(crate) draggable: bool,
    /// A drag may end on this node (`Ui::set_drop_target`).
    pub(crate) drop_target: bool,
    /// Generation of the slot; removed by `Ui::remove` (the slot is reused).
    pub(crate) slot: Slot,
    /// The nearest scroll container or dialog above this node (set by the layout): the
    /// node is drawn and hit only inside that ancestor's rectangle.
    pub(crate) clip_parent: Option<NodeId>,
}

/// The open options list of a select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Popup {
    pub(crate) select: NodeId,
    pub(crate) highlight: usize,
    pub(crate) scroll: usize,
}

/// A widget held down by one pointer, and which of its parts (tab, radio option).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Press {
    pub(crate) node: NodeId,
    pub(crate) part: Option<usize>,
}

/// Which pointing device an event came from; each has its own press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pointer {
    Mouse,
    Touch,
}

/// Which nodes the user currently interacts with.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct InputState {
    pub(crate) hover: Option<NodeId>,
    /// The part of the hovered widget under the pointer.
    pub(crate) hover_part: Option<usize>,
    /// The widget held down by the mouse button.
    pub(crate) mouse: Option<Press>,
    /// The widget held down by the finger.
    pub(crate) finger: Option<Press>,
    pub(crate) focus: Option<NodeId>,
    pub(crate) focus_visible: bool,
    pub(crate) touch: Option<u64>,
    pub(crate) popup: Option<Popup>,
    /// Last position of the mouse pointer on the surface (for hit-testing again after the
    /// content moved under it).
    pub(crate) pointer: Option<crate::geom::Point>,
    /// Where focus was before it was lost (disabled, Escape in a search field): the next
    /// Tab continues from there.
    pub(crate) focus_hint: Option<NodeId>,
    /// Modifier keys held.
    pub(crate) modifiers: crate::event::Modifiers,
    /// The node a middle press landed on (a middle click is reported on release over it).
    pub(crate) middle: Option<NodeId>,
}

impl InputState {
    /// The press of a pointer kind.
    pub(crate) fn press(&self, pointer: Pointer) -> Option<Press> {
        match pointer {
            Pointer::Mouse => self.mouse,
            Pointer::Touch => self.finger,
        }
    }

    /// The part of `id` held down by any pointer.
    pub(crate) fn pressed_part(&self, id: NodeId) -> Option<usize> {
        [self.mouse, self.finger]
            .into_iter()
            .flatten()
            .find(|press| press.node == id)
            .and_then(|press| press.part)
    }

    /// `true` while any pointer holds `id` down.
    pub(crate) fn is_pressed(&self, id: NodeId) -> bool {
        [self.mouse, self.finger]
            .into_iter()
            .flatten()
            .any(|press| press.node == id)
    }
}

#[derive(Default)]
struct Dirty {
    layout: bool,
    /// Every cached measurement is stale (language, scale, input mode, catalogs).
    remeasure: bool,
    full: bool,
    rects: Vec<Rect>,
}

/// A widget tree, its layout, its interaction state and its drawing resources.
pub struct Ui {
    pub(crate) nodes: Vec<Node>,
    /// Nodes in logical (depth-first) order; rebuilt with the layout.
    pub(crate) order: Vec<NodeId>,
    pub(crate) env: PaintEnv,
    pub(crate) mode: InputMode,
    reduce_motion: bool,
    size: Size,
    background: ColorRole,
    pub(crate) tr: Translator,
    pub(crate) res: RefCell<Resources>,
    pub(crate) input: InputState,
    /// Everything above the plain tree: paint order, dialogs, menu, tooltip, drags.
    pub(crate) layers: Layers,
    /// Actions caused by the clock, returned by the next `handle`.
    pub(crate) pending: Vec<crate::event::Action>,
    dirty: Dirty,
    pub(crate) now_ms: u64,
}

const TOOLKIT_CATALOGS: [(Lang, &str); 3] = [
    (Lang::Ar, include_str!("../../locales/ar.ftl")),
    (Lang::En, include_str!("../../locales/en.ftl")),
    (Lang::De, include_str!("../../locales/de.ftl")),
];

impl Ui {
    /// A UI of `size` logical points whose root is a column. `translator` carries the app's
    /// catalogs; the toolkit's own keys (`ui.*`) are added to it.
    pub fn new(
        env: PaintEnv,
        mode: InputMode,
        size: Size,
        mut translator: Translator,
    ) -> Result<Ui, UiError> {
        if !(env.scale.is_finite() && (0.5..=8.0).contains(&env.scale)) {
            return Err(UiError::InvalidScale);
        }
        let valid = |v: f32| v.is_finite() && v >= 0.0;
        if !(valid(size.w) && valid(size.h)) {
            return Err(UiError::InvalidSize);
        }
        for (lang, source) in TOOLKIT_CATALOGS {
            translator.add_source(lang, source)?;
        }
        translator.set_lang(env.lang);
        let res = Resources {
            text: TextEngine::new()?,
            icons: crate::icon::IconCache::new()?,
        };
        let mut ui = Ui {
            nodes: Vec::new(),
            order: Vec::new(),
            env,
            mode,
            reduce_motion: false,
            size,
            background: ColorRole::Ground,
            tr: translator,
            res: RefCell::new(res),
            input: InputState::default(),
            layers: Layers::default(),
            pending: Vec::new(),
            dirty: Dirty::default(),
            now_ms: 0,
        };
        ui.clear();
        Ok(ui)
    }

    /// The root node (a flex container).
    pub fn root(&self) -> NodeId {
        NodeId::new(0, 0)
    }

    /// Removes every node and interaction state; the root becomes an empty column.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.nodes.push(tree::new_node(
            Widget::Flex(Flex::column()),
            Sizing::AUTO,
            None,
        ));
        self.input = InputState::default();
        self.layers = Layers::default();
        self.invalidate_layout();
    }

    /// Replaces the layout of the root container.
    pub fn set_root_flex(&mut self, flex: Flex) {
        self.nodes[0].widget = Widget::Flex(flex);
        self.invalidate_layout();
    }

    /// The paint environment (language, direction, theme, scale).
    pub fn env(&self) -> &PaintEnv {
        &self.env
    }

    /// The translator with the app's and the toolkit's catalogs.
    pub fn translator(&self) -> &Translator {
        &self.tr
    }

    /// Mutable access to the translator, to add catalogs. Call [`Ui::invalidate_layout`]
    /// afterwards if visible texts may have changed.
    pub fn translator_mut(&mut self) -> &mut Translator {
        &mut self.tr
    }

    /// The surface size in logical points.
    pub fn size(&self) -> Size {
        self.size
    }

    /// The size class of the surface (compact, medium, wide) by its logical width.
    pub fn size_class(&self) -> SizeClass {
        SizeClass::for_width(self.size.w)
    }

    /// The input mode that sizes the targets.
    pub fn input_mode(&self) -> InputMode {
        self.mode
    }

    /// Forces a new layout with every node measured again, and a full repaint (after
    /// catalog changes).
    pub fn invalidate_layout(&mut self) {
        self.dirty.layout = true;
        self.dirty.remeasure = true;
        self.dirty.full = true;
        self.layers.backdrop.ready = false;
    }

    /// A node changed its size (text, state, children): it and its ancestors are measured
    /// again at the next layout; every other node keeps its measurement.
    pub(crate) fn invalidate_node(&mut self, id: NodeId) {
        let mut current = Some(id);
        while let Some(node) = current.and_then(|c| self.slot_mut(c)) {
            node.measured = None;
            current = node.parent;
        }
        self.dirty.layout = true;
        self.dirty.full = true;
        if !self.inside_open_dialog(id) {
            self.layers.backdrop.ready = false;
        }
    }

    /// Arranges again (an opened or closed dialog) without measuring anything again.
    pub(crate) fn relayout(&mut self) {
        self.dirty.layout = true;
        self.dirty.full = true;
        self.layers.backdrop.ready = false;
    }

    /// `true` once after a full re-measure was requested; clears the request.
    pub(crate) fn take_remeasure_request(&mut self) -> bool {
        std::mem::take(&mut self.dirty.remeasure)
    }

    /// Queues actions caused by the clock (bounded: the oldest are dropped past 32).
    pub(crate) fn queue(&mut self, actions: Vec<crate::event::Action>) {
        self.pending.extend(actions);
        let excess = self.pending.len().saturating_sub(MAX_PENDING);
        self.pending.drain(..excess);
    }

    /// `true` once after a layout was requested; clears the request.
    pub(crate) fn take_layout_request(&mut self) -> bool {
        std::mem::take(&mut self.dirty.layout)
    }

    /// Requests a repaint of the whole surface, keeping the backdrop under a dialog.
    pub(crate) fn mark_full_repaint(&mut self) {
        self.dirty.full = true;
    }

    /// Requests a repaint of the whole surface without a new layout (the backdrop under a
    /// dialog is painted again too).
    pub(crate) fn repaint_all(&mut self) {
        self.dirty.full = true;
        self.layers.backdrop.ready = false;
    }

    /// `true` when [`Ui::paint`] has something to draw.
    pub fn needs_paint(&self) -> bool {
        self.dirty.full || !self.dirty.rects.is_empty() || self.layers.backdrop.fresh
    }

    /// Marks a logical-point rectangle for repaint.
    pub(crate) fn damage(&mut self, rect: Rect) {
        if self.dirty.full || rect.is_empty() {
            return;
        }
        if self.dirty.rects.len() >= MAX_DIRTY {
            self.dirty.full = true;
            self.dirty.rects.clear();
        } else {
            self.dirty.rects.push(rect);
        }
    }

    /// Marks a node (with its focus ring and halo) for repaint. A change under the scrim of
    /// an open dialog is also recorded for the dimmed backdrop (only that rectangle is
    /// painted and dimmed again).
    pub(crate) fn touch(&mut self, id: NodeId) {
        let Some(rect) = self.rect(id).map(|r| r.inflate(Space::S8.points())) else {
            return;
        };
        let behind = self.layers.backdrop.ready && !self.inside_open_dialog(id);
        if behind && self.layers.backdrop.damage.len() < MAX_DIRTY {
            self.layers.backdrop.damage.push(rect);
        } else if behind {
            self.layers.backdrop.ready = false;
            self.dirty.full = true;
        }
        self.damage(rect);
    }

    /// The physical rectangle of a node in logical points, for the current direction and
    /// the current scroll offsets. Meaningful after [`Ui::layout`].
    pub fn rect(&self, id: NodeId) -> Option<Rect> {
        let node = self.slot(id)?;
        let mut rect = node.frame.resolve(self.env.direction, self.size.w);
        let mut above = node.clip_parent;
        while let Some(scroller) = above.and_then(|s| self.slot(s)) {
            if let Widget::Scroll(scroll) = &scroller.widget {
                rect.y -= scroll.offset;
            }
            above = scroller.clip_parent;
        }
        Some(rect)
    }

    /// The logical rectangle of a node (start-relative), independent of the direction and
    /// of scroll offsets.
    pub fn logical_rect(&self, id: NodeId) -> Option<LogicalRect> {
        self.slot(id).map(|n| n.frame)
    }
}
