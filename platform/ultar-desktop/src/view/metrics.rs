//! The system display on the desktop background (`desktop.metrics.enabled`): a quiet card in
//! the top corner opposite the icons with the processor, memory, GPU and network traffic –
//! each its icon, name and value and a minute of history as a bar chart (the network: the
//! download as bars, the upload as a line). The values change in place every two seconds
//! ([`value`], [`meter`]); the card is built again only when it appears or moves.

use super::{Target, View};
use crate::model::metrics::{Gauge, Metrics};
use crate::model::Desktop;
use ultar_i18n::units::{format_percent, format_size};
use ultar_i18n::{Arg, Translator};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Brick, BrickKind, Card, Icon, Label, Text};

/// Width of the card (`g(64)`).
pub const WIDTH: f32 = 64.0 * Space::S4.points();
/// Distance of the card from the edges of the work area (the icons' distance).
const EDGE: Space = Space::S24;

/// Icon and name of a gauge.
fn look(gauge: Gauge) -> (IconId, &'static str) {
    match gauge {
        Gauge::Cpu => (IconId::Chip, "desk.metrics.cpu"),
        Gauge::Ram => (IconId::Server, "desk.metrics.ram"),
        Gauge::Gpu => (IconId::Monitor, "desk.metrics.gpu"),
        Gauge::Net => (IconId::Network, "desk.metrics.net"),
    }
}

/// The text of a gauge's value: `23%`, `5.2 GB of 15.6 GB`, `↓ 1.2 MB/s ↑ 85 KB/s`, or a dash
/// where the device does not offer it.
pub fn value(m: &Metrics, gauge: Gauge, tr: &Translator) -> Label {
    let text = match gauge {
        Gauge::Cpu => m.cpu.map(|v| format_percent(i64::from(v), tr)),
        Gauge::Gpu => m.gpu.map(|v| format_percent(i64::from(v), tr)),
        Gauge::Ram => m.ram.map(|(used, total)| {
            let (used, total) = (format_size(used, tr), format_size(total, tr));
            tr.t_args(
                "desk.metrics.memory",
                &[("used", Arg::Str(&used)), ("total", Arg::Str(&total))],
            )
        }),
        Gauge::Net => m.net.map(|(down, up)| {
            let rate = |b: u64| {
                let size = format_size(b, tr);
                tr.t_args("desk.metrics.rate", &[("value", Arg::Str(&size))])
            };
            let (down, up) = (rate(down), rate(up));
            tr.t_args(
                "desk.metrics.rates",
                &[("down", Arg::Str(&down)), ("up", Arg::Str(&up))],
            )
        }),
    };
    text.map_or_else(|| Label::key("desk.metrics.none"), |t| Label::data(&t))
}

/// The history chart of a gauge.
pub fn meter(m: &Metrics, gauge: Gauge) -> BrickKind {
    BrickKind::Meter {
        values: m.history[gauge.index()].clone(),
        second: if gauge == Gauge::Net {
            m.upload.clone()
        } else {
            Vec::new()
        },
    }
}

/// Puts the card into `area` (the background's overlay; its alignment places the card at
/// the end opposite the icons).
pub fn add(view: &mut View, area: NodeId, d: &Desktop, tr: &Translator) -> Result<(), UiError> {
    if !d.prefs.desk.metrics {
        return Ok(());
    }
    // The card keeps its width at the side opposite the icons.
    let side = if d.prefs.desk.corner.end() {
        CrossAlign::Start
    } else {
        CrossAlign::End
    };
    let holder = view.ui.add_sized(
        area,
        Flex::column().padding(Padding::all(EDGE)).cross(side),
        Sizing::AUTO,
    )?;
    let flex = Flex::column()
        .padding(Padding::all(Space::S16))
        .gap(Space::S12);
    let card = view
        .ui
        .add_sized(holder, Card::new().flex(flex), Sizing::width(WIDTH))?;
    let title = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);
    view.ui.add(
        card,
        Text::new(Label::key("desk.metrics.title")).spec(title),
    )?;
    for gauge in Gauge::ALL {
        row(view, card, &d.places.metrics, gauge, tr)?;
    }
    Ok(())
}

fn row(
    view: &mut View,
    card: NodeId,
    m: &Metrics,
    gauge: Gauge,
    tr: &Translator,
) -> Result<(), UiError> {
    let (icon, name) = look(gauge);
    let block = view.ui.add(card, Flex::column().gap(Space::S4))?;
    let head = view
        .ui
        .add(block, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    view.ui.add(head, Icon::new(icon).small())?;
    let small = TextSpec::new(TypeSize::S12);
    view.ui.add_sized(
        head,
        Text::new(Label::key(name))
            .spec(small)
            .color(ColorRole::TextMuted),
        Sizing::grow(1),
    )?;
    let strong = TextSpec::new(TypeSize::S12).weight(Weight::Semibold);
    // The traffic's two rates are long: they get a line of their own below the name.
    let line = if gauge == Gauge::Net {
        view.ui.add(block, Flex::row().main(MainAlign::End))?
    } else {
        head
    };
    let text = view
        .ui
        .add(line, Text::new(value(m, gauge, tr)).spec(strong))?;
    view.mark(text, Target::MetricValue(gauge));
    let chart = Brick::new(meter(m, gauge), Label::key(name));
    let chart = view
        .ui
        .add_sized(block, chart, Sizing::AUTO.with_height(Space::S24.points()))?;
    view.mark(chart, Target::MetricGraph(gauge));
    Ok(())
}
