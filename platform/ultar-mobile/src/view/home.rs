//! The home screen: brand mark and dot-matrix clock, the date line with the Hijri date, the
//! cards "next" and "device", the search, the app grid in pages with dots, the dock and the
//! navigation bar.

use super::{app_label, bars, clock, metrics, per_page, Builder};
use crate::click::Click;
use crate::model::apps::{self, App};
use crate::model::Mobile;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Brick, BrickKind, Card, IconButton, Label, Progress, Text};

/// Height of the home screen's clock (points).
pub const CLOCK_HEIGHT: f32 = 48.0;

/// Builds the body of the home screen.
pub fn body(b: &mut Builder<'_>, root: NodeId, m: &Mobile) -> Result<(), UiError> {
    let main = Flex::column()
        .padding(Padding::symmetric(Space::S16, Space::S8))
        .gap(Space::S12);
    let main = b.flex(root, main, Sizing::grow(1))?;
    header(b, main, m)?;
    cards(b, main, m)?;
    search(b, main)?;
    grid(b, main, m)?;
    dock(b, main, m)?;
    Ok(())
}

/// Brand mark at the start, the clock and the date lines at the end.
fn header(b: &mut Builder<'_>, parent: NodeId, m: &Mobile) -> Result<(), UiError> {
    let row = b.flex(
        parent,
        Flex::row()
            .main(MainAlign::SpaceBetween)
            .cross(CrossAlign::Start),
        Sizing::AUTO,
    )?;
    let mark = Brick::new(BrickKind::Mark { points: 48 }, Label::key("mobile.brand"));
    b.add(row, mark, Sizing::AUTO)?;
    let Some(now) = m.now else {
        return Ok(());
    };
    let col = b.flex(
        row,
        Flex::column().cross(CrossAlign::End).gap(Space::S4),
        Sizing::AUTO,
    )?;
    let time = now.clock();
    let (area, width) = clock::area(&time, CLOCK_HEIGHT);
    let id = b.add(col, area, Sizing::width(width).with_height(CLOCK_HEIGHT))?;
    b.built.clocks.push((id, time));
    let tr = b.ui.translator().clone();
    let line = format!("{} · {}", now.date_line(&tr), now.hijri_line(&tr));
    let text = Text::new(Label::data(&line))
        .spec(TextSpec::new(TypeSize::S12))
        .color(ColorRole::TextMuted);
    b.add(col, text, Sizing::AUTO)?;
    Ok(())
}

/// The two cards: the next appointment and the device (processor, memory, battery).
fn cards(b: &mut Builder<'_>, parent: NodeId, m: &Mobile) -> Result<(), UiError> {
    let row = b.flex(parent, Flex::row().gap(Space::S8), Sizing::AUTO)?;
    let inner = Flex::column()
        .padding(Padding::all(Space::S12))
        .gap(Space::S4);
    let next = b.add(row, Card::new().flex(inner), Sizing::grow(1))?;
    b.add(next, small(Label::key("mobile.card.next")), Sizing::AUTO)?;
    let what = match &m.next {
        Some(n) => Label::data(n),
        None => Label::key("mobile.card.nothing"),
    };
    let spec = TextSpec::new(TypeSize::S14)
        .weight(Weight::Semibold)
        .overflow(Overflow::Ellipsis);
    b.add(next, Text::new(what).spec(spec), Sizing::AUTO)?;
    b.add(
        next,
        small(Label::key("mobile.card.calendar")),
        Sizing::AUTO,
    )?;

    let device = b.add(row, Card::new().flex(inner), Sizing::grow(1))?;
    b.add(
        device,
        small(Label::key("mobile.card.device")),
        Sizing::AUTO,
    )?;
    let mut lines = vec![
        ("mobile.card.cpu", Some(m.status.cpu)),
        ("mobile.card.ram", Some(m.status.ram)),
    ];
    if m.hardware.battery {
        lines.push(("mobile.card.battery", m.status.battery));
    }
    for (key, value) in lines {
        let line = b.flex(
            device,
            Flex::row().gap(Space::S8).cross(CrossAlign::Center),
            Sizing::AUTO,
        )?;
        let name = small(Label::key(key));
        b.add(line, name, Sizing::width(Space::S48.points()))?;
        let mut bar = Progress::new(Label::key(key));
        if let Some(v) = value {
            bar = bar.value(f32::from(v.min(100)) / 100.0);
        }
        b.add(line, bar, Sizing::grow(1))?;
    }
    Ok(())
}

/// The search entry (opens the search) and voice search.
fn search(b: &mut Builder<'_>, parent: NodeId) -> Result<(), UiError> {
    let row = b.flex(
        parent,
        Flex::row().gap(Space::S8).cross(CrossAlign::Center),
        Sizing::AUTO,
    )?;
    let width = b.screen().w - 2.0 * Space::S16.points() - super::TOUCH - Space::S8.points();
    let pill = Brick::new(BrickKind::SearchPill { width }, Label::key("mobile.search"));
    b.click(row, pill, Sizing::grow(1), Click::Search)?;
    let mic = IconButton::new(IconId::Mic, Label::key("mobile.voice"));
    b.click(row, mic, Sizing::AUTO, Click::Voice)?;
    Ok(())
}

/// One page of the app grid and its dots.
fn grid(b: &mut Builder<'_>, parent: NodeId, m: &Mobile) -> Result<(), UiError> {
    let per = per_page(b.screen().h);
    let all = m.grid_apps();
    let pages = apps::pages(&all, per);
    let page = m.launcher.page.min(pages.len().saturating_sub(1));
    let area = b.flex(parent, Flex::column().gap(Space::S8), Sizing::grow(1))?;
    b.built.grid = Some(area);
    let shown = pages.get(page).copied().unwrap_or(&[]);
    cell_rows(b, area, shown)?;
    b.spacer(area)?;
    bars::dots(b, parent, pages.len(), page)
}

/// Rows of app cells with four equal columns (short rows keep their columns).
pub fn cell_rows(b: &mut Builder<'_>, parent: NodeId, apps: &[App]) -> Result<(), UiError> {
    for chunk in apps.chunks(metrics::COLUMNS) {
        let row = b.flex(parent, Flex::row().gap(Space::S8), Sizing::AUTO)?;
        for app in chunk {
            let cell = Brick::new(
                BrickKind::PhoneCell {
                    icon: app.icon,
                    open: false,
                },
                super::grid_label(app),
            );
            b.click(row, cell, Sizing::grow(1), Click::App(app.id.clone()))?;
        }
        for _ in chunk.len()..metrics::COLUMNS {
            b.spacer(row)?;
        }
    }
    Ok(())
}

/// The dock: the four apps that stay on every page.
fn dock(b: &mut Builder<'_>, parent: NodeId, m: &Mobile) -> Result<(), UiError> {
    let flex = Flex::row()
        .main(MainAlign::SpaceBetween)
        .cross(CrossAlign::Center)
        .padding(Padding::symmetric(Space::S8, Space::S8));
    let card = b.add(parent, Card::new().dock().flex(flex), Sizing::AUTO)?;
    for app in m.dock_apps() {
        let tile = Brick::new(
            BrickKind::Tile {
                icon: app.icon,
                running: 0,
                active: false,
            },
            app_label(app),
        );
        b.click(card, tile, Sizing::AUTO, Click::App(app.id.clone()))?;
    }
    Ok(())
}

/// Small muted text.
pub fn small(label: Label) -> Text {
    Text::new(label)
        .spec(TextSpec::new(TypeSize::S12))
        .color(ColorRole::TextMuted)
}
