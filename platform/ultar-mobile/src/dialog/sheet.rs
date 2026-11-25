//! The frame of every phone dialog (board CompactMeshIncoming): the dimmed upper part and
//! the sheet at the bottom edge (raised fill, padding 16 · 16 · 24, gap 12), its title
//! 20/600, its text 12 muted, and full-width touch-size buttons.

use super::{DialogBuilt, DialogClick};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{size, ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Brick, BrickKind, Button, ButtonVariant, Card, Label, Text, Widget};

/// Small text (the sheet's explanation, the detail lines).
pub const SMALL: TextSpec = TextSpec::new(TypeSize::S12).overflow(Overflow::Wrap);

/// The sheet's title: 20/600, wrapping.
pub fn heading() -> TextSpec {
    TextSpec::title(TypeSize::S20)
        .weight(Weight::Semibold)
        .overflow(Overflow::Wrap)
}

/// A dialog being built.
pub struct Sheet<'a> {
    /// The tree.
    pub ui: &'a mut Ui,
    /// What was collected.
    pub built: DialogBuilt,
    /// The sheet's column.
    pub body: NodeId,
}

impl Sheet<'_> {
    /// Clears `ui` and lays out the dimmed part and the empty sheet.
    pub fn start(ui: &mut Ui) -> Result<Sheet<'_>, UiError> {
        ui.clear();
        ui.set_root_flex(Flex::column());
        let root = ui.root();
        // The dimmed screen above the sheet; a tap on it answers nothing.
        let scrim = Brick::new(BrickKind::Scrim, Label::key("mobile.dialog.scrim"));
        ui.add_sized(root, scrim, Sizing::grow(1))?;
        let padding = Padding {
            start: Some(Space::S16),
            end: Some(Space::S16),
            top: Some(Space::S16),
            bottom: Some(Space::S24),
        };
        let column = Flex::column().padding(padding).gap(Space::S12);
        let body = ui.add_sized(root, Card::new().raised().flex(column), Sizing::AUTO)?;
        Ok(Sheet {
            ui,
            built: DialogBuilt::default(),
            body,
        })
    }

    /// Adds a node.
    pub fn add(
        &mut self,
        parent: NodeId,
        widget: impl Into<Widget>,
        sizing: Sizing,
    ) -> Result<NodeId, UiError> {
        self.ui.add_sized(parent, widget, sizing)
    }

    /// The title (20/600) and the explanation (12, muted).
    pub fn texts(&mut self, title: Label, text: Label) -> Result<(), UiError> {
        let body = self.body;
        let heading = heading();
        self.add(body, Text::new(title).spec(heading), Sizing::AUTO)?;
        let text = Text::new(text).spec(SMALL).color(ColorRole::TextMuted);
        self.add(body, text, Sizing::AUTO)?;
        Ok(())
    }

    /// A full-width button of the sheet; the first primary one comes first.
    pub fn button(
        &mut self,
        key: &str,
        variant: ButtonVariant,
        icon: Option<IconId>,
        click: DialogClick,
    ) -> Result<NodeId, UiError> {
        let mut button = Button::new(Label::key(key)).variant(variant);
        if let Some(icon) = icon {
            button = button.icon(icon);
        }
        let body = self.body;
        let touch = Sizing::AUTO.with_height(size::TARGET_TOUCH_MIN);
        let id = self.add(body, button, touch)?;
        self.built.clicks.push((id, click));
        Ok(id)
    }

    /// The primary action above the safe one; the safe one has the focus (Enter by habit
    /// answers safely).
    pub fn yes_no(
        &mut self,
        yes: Option<(&str, DialogClick)>,
        safe: &str,
    ) -> Result<NodeId, UiError> {
        if let Some((key, click)) = yes {
            self.button(key, ButtonVariant::Primary, Some(IconId::Check), click)?;
        }
        let no = self.button(safe, ButtonVariant::Plain, None, DialogClick::Safe)?;
        self.built.focus = Some(no);
        Ok(no)
    }
}
