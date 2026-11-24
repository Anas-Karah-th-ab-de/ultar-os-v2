//! Draws a [`Model`] with the toolkit: header, then the surface of [`Model::screen`]
//! (boards PhoneDial, PhoneIncoming, PhoneInCall; compact size class: one column with pages).
//! Every clickable node is returned with its [`Click`]. The whole tree is rebuilt for a new
//! model (a few hundred nodes at most).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod call;
mod dial;
mod parts;

use crate::click::Click;
use crate::model::{Model, Screen};
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::tokens::{SizeClass, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Label, Widget};

/// Geometry derived from the tokens (base grid × n, as `g(n)` of the board generator).
pub mod metrics {
    use ultar_ui::tokens::Space;

    /// n units of the base grid.
    pub fn grid(n: f32) -> f32 {
        Space::S4.points() * n
    }
    /// Width of the dial pad column on medium and wide surfaces (`g(100)`).
    pub fn dial_width() -> f32 {
        grid(100.0)
    }
    /// Width of a call control tile (`g(26)`).
    pub fn control_width() -> f32 {
        grid(26.0)
    }
    /// Dial pad key, number field and suggestion height (`space.48`).
    pub fn key_height() -> f32 {
        Space::S48.points()
    }
    /// Own picture inside the video area (`g(48)` × `g(32)`).
    pub fn own_picture() -> (f32, f32) {
        (grid(48.0), grid(32.0))
    }
}

/// Handles of one built tree.
#[derive(Debug, Default)]
pub struct Built {
    /// Clickable nodes and what they mean.
    pub clicks: Vec<(NodeId, Click)>,
    /// The number field.
    pub number: Option<NodeId>,
    /// The dial pad's scroll container.
    pub pad: Option<NodeId>,
    /// The filter tabs.
    pub filter: Option<NodeId>,
    /// The way radio group.
    pub way: Option<NodeId>,
    /// The compact page tabs.
    pub page: Option<NodeId>,
    /// The open dialog or sheet.
    pub dialog: Option<NodeId>,
    /// The call timer text.
    pub timer: Option<NodeId>,
}

/// Collects nodes while the tree is built.
pub(crate) struct Builder<'a> {
    pub(crate) ui: &'a mut Ui,
    pub(crate) built: Built,
    pub(crate) class: SizeClass,
}

impl Builder<'_> {
    /// Adds a node.
    pub(crate) fn add(
        &mut self,
        parent: NodeId,
        widget: impl Into<Widget>,
        sizing: Sizing,
    ) -> Result<NodeId, UiError> {
        self.ui.add_sized(parent, widget, sizing)
    }

    /// Adds a clickable node.
    pub(crate) fn click(
        &mut self,
        parent: NodeId,
        widget: impl Into<Widget>,
        sizing: Sizing,
        click: Click,
    ) -> Result<NodeId, UiError> {
        let id = self.ui.add_sized(parent, widget, sizing)?;
        self.built.clicks.push((id, click));
        Ok(id)
    }

    /// Adds a flex container.
    pub(crate) fn flex(
        &mut self,
        parent: NodeId,
        flex: Flex,
        sizing: Sizing,
    ) -> Result<NodeId, UiError> {
        self.ui.add_sized(parent, flex, sizing)
    }

    /// Gives a node an accessible name other than its label.
    pub(crate) fn name(&mut self, id: NodeId, name: Label) -> Result<(), UiError> {
        self.ui.set_accessible_name(id, Some(name))
    }

    /// True on a phone-sized surface.
    pub(crate) fn compact(&self) -> bool {
        self.class == SizeClass::Compact
    }
}

/// Builds the tree of `model` into `ui` (which is cleared first).
pub fn build(ui: &mut Ui, model: &Model) -> Result<Built, UiError> {
    ui.clear();
    let class = ui.size_class();
    ui.set_root_flex(Flex::column());
    let root = ui.root();
    let mut b = Builder {
        ui,
        built: Built::default(),
        class,
    };
    parts::header(&mut b, root, model)?;
    let body = b.flex(
        root,
        Flex::row().cross(CrossAlign::Stretch),
        Sizing::grow(1),
    )?;
    match model.screen() {
        Screen::Dial => dial::surface(&mut b, body, model)?,
        Screen::Incoming(id) => call::incoming(&mut b, body, model, id)?,
        Screen::InCall(id) => call::in_call(&mut b, body, model, id)?,
    }
    parts::dialogs(&mut b, root, model)?;
    Ok(b.built)
}

/// A column that grows, with the usual surface padding of the boards (24 / 32).
pub(crate) fn section(b: &mut Builder<'_>, parent: NodeId, gap: Space) -> Result<NodeId, UiError> {
    use ultar_ui::layout::Padding;
    let pad = if b.compact() {
        Padding::all(Space::S16)
    } else {
        Padding::symmetric(Space::S32, Space::S24)
    };
    b.flex(
        parent,
        Flex::column().padding(pad).gap(gap),
        Sizing::grow(1),
    )
}
