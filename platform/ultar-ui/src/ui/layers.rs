//! State of everything that is not the plain widget tree: the paint order, open dialogs,
//! the open menu, the tooltip and drags in progress.

use super::{NodeId, Pointer};
use crate::geom::{Point, Rect};
use crate::widgets::{Menu, MenuPanel};

/// One step of painting a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintStep {
    /// Paint the node itself.
    Node(NodeId),
    /// Paint the scrollbar of a scroll container, after everything inside it.
    ScrollBar(NodeId),
}

/// Where an open menu hangs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MenuOrigin {
    /// Below (or above) the owner node.
    Below,
    /// At a point (context menu).
    At(Point),
}

/// The open menu.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MenuState {
    /// The node the menu belongs to; its actions are reported for this node.
    pub(crate) owner: NodeId,
    pub(crate) menu: Menu,
    pub(crate) origin: MenuOrigin,
    /// Items whose submenus are open, from the top level down.
    pub(crate) path: Vec<usize>,
    /// The highlighted item of the deepest open level.
    pub(crate) highlight: Option<usize>,
    /// Geometry of every open level (`path.len() + 1` panels).
    pub(crate) panels: Vec<MenuPanel>,
    /// Scroll offset of every open level (kept when the levels are placed again).
    pub(crate) scrolls: Vec<f32>,
}

/// The visible tooltip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Tip {
    pub(crate) node: NodeId,
    pub(crate) rect: Rect,
}

/// What a drag in progress moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DragKind {
    /// The scrollbar thumb, held `grab` points below its top.
    Thumb {
        /// Distance from the thumb's top to the pointer.
        grab: f32,
    },
    /// The content under a finger.
    Finger {
        /// Where the finger went down (physical y).
        start_y: f32,
        /// The scroll offset at that moment.
        start_offset: f32,
        /// `true` once the finger moved far enough to mean "scroll" and not "tap".
        active: bool,
    },
}

/// A drag in progress on a scroll container.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Drag {
    pub(crate) scroller: NodeId,
    pub(crate) kind: DragKind,
    /// The pointer that drags; events of the other one do not move or end it.
    pub(crate) pointer: Pointer,
}

/// See the module documentation.
#[derive(Default)]
pub(crate) struct Layers {
    /// What to paint, in order; rebuilt with the layout.
    pub(crate) steps: Vec<PaintStep>,
    /// All dialogs and sheets of the tree, open or not.
    pub(crate) dialogs: Vec<NodeId>,
    /// The open ones, the topmost last.
    pub(crate) open: Vec<NodeId>,
    /// Index in `Ui::order` where the nodes of the topmost open dialog start.
    pub(crate) modal_start: usize,
    pub(crate) menu: Option<MenuState>,
    pub(crate) tip: Option<Tip>,
    /// Since when the pointer rests on a node that has a tooltip.
    pub(crate) hover_since: Option<u64>,
    pub(crate) drag: Option<Drag>,
    /// Long press, double click, held buttons, dragged nodes.
    pub(crate) gestures: crate::event::Gestures,
    /// Index in `steps` where the topmost dialog with a scrim starts; everything before it
    /// lies under the scrim.
    pub(crate) scrim_step: Option<usize>,
    /// Everything under the scrim, painted once and dimmed (see `Ui::paint`).
    pub(crate) backdrop: Option<crate::canvas::Canvas>,
    /// Something under the scrim changed: the backdrop is painted again (into the same
    /// buffer) before the next frame.
    pub(crate) backdrop_stale: bool,
    /// Visible toasts, oldest first.
    pub(crate) toasts: Vec<super::toasts::Shown>,
}
