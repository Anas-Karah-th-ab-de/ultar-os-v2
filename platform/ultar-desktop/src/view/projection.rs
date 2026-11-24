//! The display mode panel of Super+P (board Projection): four modes as large cards –
//! this device only, mirror, extend, second display only; Super+P steps, Enter confirms,
//! Esc cancels, a click chooses at once.

use super::geometry::{self, Placement, PROJECTION_SIZE};
use super::{Look, Target, View};
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::UiError;
use ultar_ui::widgets::{Brick, BrickKind, Label, Picture, Text};

/// The four modes in board order: picture and catalog key.
pub const MODES: [(Picture, &str); 4] = [
    (Picture::DisplaysFirst, "desk.proj.first"),
    (Picture::DisplaysMirror, "desk.proj.mirror"),
    (Picture::DisplaysExtend, "desk.proj.extend"),
    (Picture::DisplaysSecond, "desk.proj.second"),
];

/// Builds the panel with mode `chosen` (0–3) highlighted.
pub fn build(
    chosen: usize,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<(View, Placement), UiError> {
    let width = PROJECTION_SIZE.w;
    super::card_sheet(look, screen, width, tr, geometry::centred, |view, card| {
        let head = Flex::row()
            .main(MainAlign::SpaceBetween)
            .cross(CrossAlign::Center);
        let head = view.ui.add(card, head)?;
        let title = Text::new(Label::key("desk.proj.title"))
            .spec(TextSpec::new(TypeSize::S16).weight(Weight::Semibold));
        view.ui.add(head, title)?;
        let keys = Text::new(Label::key("desk.proj.keys"))
            .spec(TextSpec::new(TypeSize::S12))
            .color(ColorRole::TextMuted);
        view.ui.add(head, keys)?;
        let row = view.ui.add(card, Flex::row().gap(Space::S12))?;
        for (i, (picture, key)) in MODES.iter().enumerate() {
            let brick = Brick::new(
                BrickKind::ModeCard {
                    picture: *picture,
                    chosen: i == chosen,
                },
                Label::key(*key),
            );
            let id = view.ui.add_sized(row, brick, Sizing::grow(1))?;
            view.mark(id, Target::Mode(i));
        }
        Ok(())
    })
}
