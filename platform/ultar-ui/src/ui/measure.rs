//! Layout of the tree: measure bottom-up, arrange top-down, all in logical coordinates.

use super::{Node, NodeId, PaintStep, Ui};
use crate::geom::Size;
use crate::layout::{distribute, Axis, CrossAlign, Flex, Length, LogicalRect, Slot};
use crate::widgets::{EmptyState, InfoBar, ListItem, MeasureCx, Row, Widget};

fn cross_offset(align: CrossAlign, free: f32) -> f32 {
    match align {
        CrossAlign::Start | CrossAlign::Stretch => 0.0,
        CrossAlign::Center => free / 2.0,
        CrossAlign::End => free,
    }
}

/// Size of a node for an available width.
fn measure(nodes: &mut [Node], cx: &mut MeasureCx<'_>, id: NodeId, avail: f32) -> Size {
    let i = id.index();
    let avail = match nodes[i].sizing.width {
        Length::Points(p) => p.min(avail),
        Length::Auto => avail,
    }
    .max(0.0);
    if let Some((cached_avail, size)) = nodes[i].measured {
        if cached_avail == avail {
            return size;
        }
    }
    let flex = nodes[i].widget.flex().copied();
    let mut size = match flex {
        Some(flex) => measure_flex(nodes, cx, id, &flex, avail),
        None => measure_leaf(nodes, cx, id, avail),
    };
    let grows = nodes[i].sizing.grow > 0;
    if let Widget::Scroll(scroll) = &mut nodes[i].widget {
        // The viewport takes what its sizing gives it: a fixed height, or (growing) the
        // parent's left-over space, starting from nothing. The content keeps its height.
        scroll.content = size.h;
        if grows {
            size.h = 0.0;
        }
    }
    if let Length::Points(p) = nodes[i].sizing.width {
        size.w = p;
    }
    if let Length::Points(p) = nodes[i].sizing.height {
        size.h = p;
    }
    nodes[i].measured = Some((avail, size));
    size
}

fn measure_leaf(nodes: &mut [Node], cx: &mut MeasureCx<'_>, id: NodeId, avail: f32) -> Size {
    let i = id.index();
    let hosted = nodes[i].children.first().copied();
    let control = match hosted.filter(|_| nodes[i].widget.hosts_one_child()) {
        Some(child) => measure(nodes, cx, child, avail),
        None => Size::default(),
    };
    let node = &nodes[i];
    let error = node.error.as_ref();
    match &node.widget {
        Widget::Flex(_)
        | Widget::Card(_)
        | Widget::List(_)
        | Widget::Scroll(_)
        | Widget::Dialog(_) => Size::default(),
        Widget::Text(w) => w.measure(cx, avail),
        Widget::Icon(w) => w.measure(),
        Widget::Button(w) => w.measure(cx, avail, node.loading),
        Widget::IconButton(w) => w.measure(cx),
        Widget::Switch(w) => w.measure(cx),
        Widget::Select(w) => w.measure(cx, avail, error),
        Widget::Slider(w) => w.measure(cx, avail),
        Widget::TextField(w) => w.measure(cx, avail, error),
        Widget::Row(w) => w.measure(cx, avail, control),
        Widget::Badge(w) => w.measure(cx, avail),
        Widget::Progress(w) => w.measure(cx, avail),
        Widget::Checkbox(w) => w.measure(cx, avail),
        Widget::RadioGroup(w) => w.measure(cx, avail),
        Widget::Tabs(w) => w.measure(cx, avail),
        Widget::InfoBar(w) => w.measure(cx, avail, control),
        Widget::EmptyState(w) => w.measure(cx, avail, control),
        Widget::ListItem(w) => w.measure(cx, avail, control),
    }
}

/// Where a widget that hosts one child puts it: (offset from its start, offset from its top).
fn hosted_offset(widget: &Widget, size: Size, control: Size) -> Option<(f32, f32)> {
    match widget {
        Widget::Row(_) => Some(Row::control_offset(size, control)),
        Widget::InfoBar(_) => Some(InfoBar::control_offset(size, control)),
        Widget::EmptyState(_) => Some(EmptyState::control_offset(size, control)),
        Widget::ListItem(_) => Some(ListItem::control_offset(size, control)),
        _ => None,
    }
}

/// Natural main-axis extents and grow factors of the children of an inline flex.
fn inline_items(
    nodes: &mut [Node],
    cx: &mut MeasureCx<'_>,
    id: NodeId,
    flex: &Flex,
    inner: f32,
) -> Vec<(f32, u8)> {
    let count = nodes[id.index()].children.len();
    let mut items = Vec::with_capacity(count);
    let mut used = 0.0f32;
    for n in 0..count {
        let child = nodes[id.index()].children[n];
        let grow = nodes[child.index()].sizing.grow;
        if grow > 0 {
            items.push((0.0, grow));
        } else {
            let width = measure(nodes, cx, child, (inner - used).max(0.0)).w;
            used += width + flex.gap_points();
            items.push((width, 0));
        }
    }
    items
}

fn measure_flex(
    nodes: &mut [Node],
    cx: &mut MeasureCx<'_>,
    id: NodeId,
    flex: &Flex,
    avail: f32,
) -> Size {
    let inner = (avail - flex.padding.inline_sum()).max(0.0);
    let count = nodes[id.index()].children.len();
    let gaps = flex.gap_points() * count.saturating_sub(1) as f32;
    let (mut main, mut cross) = (0.0f32, 0.0f32);
    match flex.axis {
        Axis::Inline => {
            let items = inline_items(nodes, cx, id, flex, inner);
            let mut slots: Vec<Slot> = Vec::with_capacity(count);
            distribute(inner, flex.gap_points(), flex.main, &items, &mut slots);
            let grows = items.iter().any(|(_, grow)| *grow > 0);
            for (n, slot) in slots.iter().enumerate() {
                let child = nodes[id.index()].children[n];
                cross = cross.max(measure(nodes, cx, child, slot.extent).h);
                main += slot.extent;
            }
            let width = if grows {
                avail
            } else {
                main + gaps + flex.padding.inline_sum()
            };
            Size::new(width, cross + flex.padding.block_sum())
        }
        Axis::Block => {
            for n in 0..count {
                let child = nodes[id.index()].children[n];
                let size = measure(nodes, cx, child, inner);
                main += size.h;
                cross = cross.max(size.w);
            }
            let width = if flex.cross == CrossAlign::Stretch {
                avail
            } else {
                cross + flex.padding.inline_sum()
            };
            Size::new(width, main + gaps + flex.padding.block_sum())
        }
    }
}

fn arrange(nodes: &mut [Node], cx: &mut MeasureCx<'_>, id: NodeId, frame: LogicalRect) {
    nodes[id.index()].frame = frame;
    if let Some(flex) = nodes[id.index()].widget.flex().copied() {
        let mut content = frame;
        if let Widget::Scroll(scroll) = &mut nodes[id.index()].widget {
            // The children keep their natural height; the viewport shows a part of it.
            scroll.offset = scroll.clamp(scroll.offset, frame.height);
            content.height = scroll.content.max(frame.height);
        }
        arrange_flex(nodes, cx, id, &flex, content);
    } else if let Some(child) = nodes[id.index()].children.first().copied() {
        let control = measure(nodes, cx, child, frame.width);
        let size = Size::new(frame.width, frame.height);
        if let Some((start, top)) = hosted_offset(&nodes[id.index()].widget, size, control) {
            let child_frame =
                LogicalRect::new(frame.start + start, frame.top + top, control.w, control.h);
            arrange(nodes, cx, child, child_frame);
        }
    }
}

/// The content box of a flex container in logical coordinates.
struct Inner {
    start: f32,
    top: f32,
    width: f32,
    height: f32,
}

fn arrange_flex(
    nodes: &mut [Node],
    cx: &mut MeasureCx<'_>,
    id: NodeId,
    flex: &Flex,
    frame: LogicalRect,
) {
    let inner = Inner {
        start: frame.start + flex.padding.start_points(),
        top: frame.top + flex.padding.top_points(),
        width: (frame.width - flex.padding.inline_sum()).max(0.0),
        height: (frame.height - flex.padding.block_sum()).max(0.0),
    };
    match flex.axis {
        Axis::Inline => arrange_inline(nodes, cx, id, flex, &inner),
        Axis::Block => arrange_block(nodes, cx, id, flex, &inner),
    }
}

fn arrange_inline(
    nodes: &mut [Node],
    cx: &mut MeasureCx<'_>,
    id: NodeId,
    flex: &Flex,
    inner: &Inner,
) {
    let items = inline_items(nodes, cx, id, flex, inner.width);
    let mut slots: Vec<Slot> = Vec::with_capacity(items.len());
    distribute(
        inner.width,
        flex.gap_points(),
        flex.main,
        &items,
        &mut slots,
    );
    for (n, slot) in slots.iter().enumerate() {
        let child = nodes[id.index()].children[n];
        let natural = measure(nodes, cx, child, slot.extent).h;
        let stretch = flex.cross == CrossAlign::Stretch;
        let height = if stretch { inner.height } else { natural };
        let top = inner.top + cross_offset(flex.cross, inner.height - height);
        let frame = LogicalRect::new(inner.start + slot.offset, top, slot.extent, height);
        arrange(nodes, cx, child, frame);
    }
}

fn arrange_block(
    nodes: &mut [Node],
    cx: &mut MeasureCx<'_>,
    id: NodeId,
    flex: &Flex,
    inner: &Inner,
) {
    let count = nodes[id.index()].children.len();
    let mut sizes = Vec::with_capacity(count);
    let mut items: Vec<(f32, u8)> = Vec::with_capacity(count);
    for n in 0..count {
        let child = nodes[id.index()].children[n];
        let size = measure(nodes, cx, child, inner.width);
        items.push((size.h, nodes[child.index()].sizing.grow));
        sizes.push(size);
    }
    let mut slots: Vec<Slot> = Vec::with_capacity(count);
    distribute(
        inner.height,
        flex.gap_points(),
        flex.main,
        &items,
        &mut slots,
    );
    for (n, slot) in slots.iter().enumerate() {
        let child = nodes[id.index()].children[n];
        let stretch = flex.cross == CrossAlign::Stretch;
        let width = if stretch {
            inner.width
        } else {
            sizes[n].w.min(inner.width)
        };
        let start = inner.start + cross_offset(flex.cross, inner.width - width);
        let frame = LogicalRect::new(start, inner.top + slot.offset, width, slot.extent);
        arrange(nodes, cx, child, frame);
    }
}

/// Appends the subtree of `id` to the logical order and the paint steps, and tells every
/// node which scroll container or dialog clips it.
fn collect(ui: &mut Ui, id: NodeId, clip: Option<NodeId>) {
    ui.nodes[id.index()].clip_parent = clip;
    ui.order.push(id);
    ui.layers.steps.push(PaintStep::Node(id));
    let clips = matches!(
        ui.nodes[id.index()].widget,
        Widget::Scroll(_) | Widget::Dialog(_)
    );
    let inner = if clips { Some(id) } else { clip };
    for n in 0..ui.nodes[id.index()].children.len() {
        let child = ui.nodes[id.index()].children[n];
        collect(ui, child, inner);
    }
    if matches!(ui.nodes[id.index()].widget, Widget::Scroll(_)) {
        ui.layers.steps.push(PaintStep::ScrollBar(id));
    }
}

impl Ui {
    /// Lays the tree out if anything changed since the last layout. Called by
    /// [`Ui::paint`] and [`Ui::handle`]; call it yourself before reading rectangles.
    pub fn layout(&mut self) {
        if !self.take_layout_request() {
            return;
        }
        for node in &mut self.nodes {
            node.measured = None;
        }
        let root = self.root();
        let size = self.size();
        let open = self.layers.open.clone();
        let mut cx = MeasureCx {
            res: self.res.get_mut(),
            env: &self.env,
            tr: &self.tr,
            mode: self.mode,
        };
        measure(&mut self.nodes, &mut cx, root, size.w);
        let surface = LogicalRect::new(0.0, 0.0, size.w, size.h);
        arrange(&mut self.nodes, &mut cx, root, surface);
        for dialog in &open {
            let Widget::Dialog(widget) = self.nodes[dialog.index()].widget.clone() else {
                continue;
            };
            let content = measure(&mut self.nodes, &mut cx, *dialog, widget.width(size));
            let frame = widget.frame(content, size);
            arrange(&mut self.nodes, &mut cx, *dialog, frame);
        }
        self.rebuild_order();
        // What floats hangs on rectangles and text sizes that may have changed.
        self.layers.tip = None;
        self.replace_menu();
    }

    /// Rebuilds the logical order and the paint steps: the main tree, then the open dialogs
    /// (the topmost last). Closed dialogs are in neither.
    fn rebuild_order(&mut self) {
        self.order.clear();
        self.layers.steps.clear();
        let root = self.root();
        collect(self, root, None);
        self.layers.modal_start = 0;
        for dialog in self.layers.open.clone() {
            self.layers.modal_start = self.order.len();
            collect(self, dialog, None);
        }
    }

    /// The content size the tree would like at the current width (after layout: the root's
    /// measured size).
    pub fn content_size(&mut self) -> Size {
        self.layout();
        self.nodes[0]
            .measured
            .map_or(Size::default(), |(_, size)| size)
    }
}
