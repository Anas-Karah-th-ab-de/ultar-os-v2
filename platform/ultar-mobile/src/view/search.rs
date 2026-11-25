//! The search over the apps: back and the search field at the top, the matching apps in the
//! grid's four columns, or a hint while nothing is typed.

use super::home::{cell_rows, small};
use super::{app_name, bars, Builder};
use crate::click::Click;
use crate::model::Mobile;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{IconButton, Label, SearchField};

/// Builds the search.
pub fn build(b: &mut Builder<'_>, root: NodeId, m: &Mobile) -> Result<(), UiError> {
    bars::status(b, root, m, true)?;
    let main = Flex::column()
        .gap(Space::S16)
        .padding(Padding::symmetric(Space::S16, Space::S8));
    let main = b.flex(root, main, Sizing::grow(1))?;
    let row = b.flex(
        main,
        Flex::row().gap(Space::S8).cross(CrossAlign::Center),
        Sizing::AUTO,
    )?;
    let back = IconButton::new(IconId::Back, Label::key("mobile.nav.back"));
    b.click(row, back, Sizing::AUTO, Click::Back)?;
    let field = SearchField::new(Label::key("mobile.search"))
        .placeholder(Label::key("mobile.search"))
        .text(&m.query);
    let id = b.add(row, field, Sizing::grow(1))?;
    b.built.search = Some(id);
    if m.query.trim().is_empty() {
        b.add(main, small(Label::key("mobile.search.hint")), Sizing::AUTO)?;
    } else {
        let ui: &ultar_ui::ui::Ui = b.ui;
        let hits: Vec<_> = m.hits(|a| app_name(a, ui)).into_iter().cloned().collect();
        if hits.is_empty() {
            let q = m.query.trim().to_owned();
            b.add(
                main,
                small(Label::key_with("mobile.search.none", &[("query", &q)])),
                Sizing::AUTO,
            )?;
        }
        cell_rows(b, main, &hits)?;
    }
    b.spacer(main)?;
    bars::nav(b, root)
}
