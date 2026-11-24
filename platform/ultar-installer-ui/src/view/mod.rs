//! Widget trees of the installer, from `ultar-ui` only (boards *Installer*, *InstallerDisk*,
//! *InstallerDrivers*, *InstallerSummary*).

mod disk;
mod drivers;
mod frame;
mod pages;
mod summary;

use crate::action::{Act, Registry};
use crate::model::Model;
use ultar_i18n::Translator;
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{ColorRole, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Label, Text, Widget};

pub use drivers::badge as drivers_badge;
pub use frame::{build, footer_state};

/// Handles of the shown tree.
#[derive(Debug, Default)]
pub struct View {
    /// Node → action.
    pub registry: Registry,
    /// The steps list.
    pub steps: Option<NodeId>,
    /// Steps in list order.
    pub step_items: Vec<crate::wizard::Step>,
    /// The cancel dialog.
    pub dialog: Option<NodeId>,
    /// Next button and the reason text beside it.
    pub next: Option<NodeId>,
    /// Text explaining why Next is disabled.
    pub reason: Option<NodeId>,
    /// Progress bar of step 15.
    pub progress: Option<NodeId>,
    /// Passphrase fields.
    pub passes: Vec<NodeId>,
    /// The passphrases are shown (the show button was pressed).
    pub shown: bool,
    /// Size field of a new manual partition.
    pub manual_size: Option<NodeId>,
}

pub(crate) struct Cx<'a> {
    pub(crate) ui: &'a mut Ui,
    pub(crate) view: &'a mut View,
    pub(crate) tr: &'a Translator,
    pub(crate) model: &'a Model,
}

impl Cx<'_> {
    pub(crate) fn act(
        &mut self,
        parent: NodeId,
        w: impl Into<Widget>,
        act: Act,
    ) -> Result<NodeId, UiError> {
        let id = self.ui.add(parent, w)?;
        self.view.registry.add(id, act);
        Ok(id)
    }

    pub(crate) fn act_sized(
        &mut self,
        parent: NodeId,
        w: impl Into<Widget>,
        sizing: ultar_ui::layout::Sizing,
        act: Act,
    ) -> Result<NodeId, UiError> {
        let id = self.ui.add_sized(parent, w, sizing)?;
        self.view.registry.add(id, act);
        Ok(id)
    }
}

pub(crate) fn k(key: &str) -> Label {
    Label::key(key)
}

pub(crate) fn small() -> TextSpec {
    TextSpec::new(TypeSize::S12).overflow(Overflow::Wrap)
}

pub(crate) fn muted(ui: &mut Ui, parent: NodeId, label: Label) -> Result<NodeId, UiError> {
    ui.add(
        parent,
        Text::new(label).spec(small()).color(ColorRole::TextMuted),
    )
}

pub(crate) fn body_muted(ui: &mut Ui, parent: NodeId, label: Label) -> Result<NodeId, UiError> {
    let spec = TextSpec::new(TypeSize::S14).overflow(Overflow::Wrap);
    ui.add(
        parent,
        Text::new(label).spec(spec).color(ColorRole::TextMuted),
    )
}

pub(crate) fn label_spec() -> TextSpec {
    TextSpec::new(TypeSize::S14).weight(Weight::Medium)
}

/// The exact height of `lines` lines of `size` text (CSS line boxes of the boards are not
/// rounded; rounding each text would let a long table drift from the board).
pub(crate) fn lines_height(ui: &Ui, size: TypeSize, lines: f32) -> f32 {
    let factor = if ui.env().lang == ultar_i18n::Lang::Ar {
        ultar_ui::tokens::typography::LINE_HEIGHT_ARABIC
    } else {
        ultar_ui::tokens::typography::LINE_HEIGHT_LATIN
    };
    size.points() * factor * lines
}
