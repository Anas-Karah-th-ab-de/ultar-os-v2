//! The Displays page (boards *SettingsDisplay*, *SettingsDisplayLight*, *CompactDisplay*).

use super::{k, metrics, muted, section, spec, title, Cx};
use crate::action::Act;
use crate::state::display::{DisplayEntry, DisplayMode, ORIENTATIONS, SCALES};
use crate::view::shell::compact;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    ArrangedItem, Arrangement, Badge, BarEdge, Button, Card, Checkbox, IconButton, InfoBar, Label,
    Pictogram, RadioGroup, RadioLayout, Row, Select, Switch, Text, Tone,
};

/// `15.6″` from tenths of an inch.
pub fn inches(tenths: u16) -> String {
    if tenths.is_multiple_of(10) {
        format!("{}″", tenths / 10)
    } else {
        format!("{}.{}″", tenths / 10, tenths % 10)
    }
}

/// Short name of a display (tiles, choices): "Built-in 15.6″" / "Dell 24″".
pub fn short_name(d: &DisplayEntry) -> Label {
    let size = if d.diagonal_tenths > 0 {
        inches(d.diagonal_tenths)
    } else {
        String::new()
    };
    if d.internal {
        return Label::key_with("app.settings.display.builtin_short", &[("size", &size)]);
    }
    let name = if d.name.is_empty() { &d.id } else { &d.name };
    Label::data(format!("{name} {size}").trim())
}

/// Long name (card title): "Built-in display 15.6″" / "Dell 24″".
pub fn long_name(d: &DisplayEntry) -> Label {
    if d.internal {
        let size = inches(d.diagonal_tenths);
        return Label::key_with("app.settings.display.builtin_long", &[("size", &size)]);
    }
    short_name(d)
}

/// Accessible name of a tile: "Display 1: Built-in 15.6″ (main)".
fn tile_name(cx: &Cx<'_>, d: &DisplayEntry, primary: bool) -> Label {
    let name = short_name(d).resolve(cx.tr).into_owned();
    let number = d.number.to_string();
    let key = if primary {
        "app.settings.display.tile_main"
    } else {
        "app.settings.display.tile"
    };
    Label::key_with(key, &[("n", &number), ("name", &name)])
}

/// Builds the page.
pub fn build(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    title(cx, content, k("app.settings.area.displays"))?;
    let st = &cx.state.display;
    if !st.settings_known {
        let error = cx
            .state
            .panels_error
            .clone()
            .unwrap_or_else(|| ultar_bus::Text::key("app.settings.error.unavailable"));
        let bar = InfoBar::new(Tone::Danger, k("app.settings.display.no_settings"))
            .message(crate::i18n::label(&error, cx.tr));
        cx.ui.add(content, bar)?;
        return Ok(());
    }
    if !st.outputs_known {
        let bar = InfoBar::new(Tone::Warning, k("app.settings.display.no_outputs"))
            .message(k("app.settings.display.no_outputs.sub"));
        cx.ui.add(content, bar)?;
    }
    if compact(cx.ui) {
        return build_compact(cx, content);
    }
    // the details beside the main column only when both fit; else below it, and the main
    // column never wider than the page
    let side_by_side = side_by_side(cx.ui);
    let flex = if side_by_side {
        Flex::row().gap(Space::S16).cross(CrossAlign::Start)
    } else {
        Flex::column().gap(Space::S16)
    };
    let columns = cx.ui.add(content, flex)?;
    let main_sizing = if side_by_side {
        Sizing::width(metrics::display_main_column())
    } else {
        Sizing::AUTO
    };
    let main = cx
        .ui
        .add_sized(columns, Flex::column().gap(Space::S16), main_sizing)?;
    arrangement(cx, main)?;
    if cx.state.display.displays.len() > 1 {
        mode(cx, main)?;
    }
    general(cx, main)?;
    let side_sizing = if side_by_side {
        Sizing::grow(1)
    } else {
        Sizing::AUTO
    };
    let side = cx.ui.add_sized(columns, Flex::column(), side_sizing)?;
    details(cx, side)
}

/// Narrowest the details column may be beside the main column.
const SIDE_MIN: f32 = 280.0;

/// `true` when the page is wide enough for the main column and the details beside it.
fn side_by_side(ui: &ultar_ui::ui::Ui) -> bool {
    super::content_width(ui) >= metrics::display_main_column() + Space::S16.points() + SIDE_MIN
}

/// The width of the main column: the board's beside the details, else the whole page.
fn main_width(ui: &ultar_ui::ui::Ui) -> f32 {
    if side_by_side(ui) {
        metrics::display_main_column()
    } else {
        super::content_width(ui)
    }
}

/// Compact surface (board *CompactDisplay*): one card with the rows a phone needs;
/// arrangement and mode only with an external display.
fn build_compact(cx: &mut Cx<'_>, content: NodeId) -> Result<(), UiError> {
    let several = cx.state.display.displays.len() > 1;
    if several {
        arrangement(cx, content)?;
        mode(cx, content)?;
    }
    let Some(d) = cx.state.display.current().cloned() else {
        return Ok(());
    };
    let card = cx.ui.add(content, Card::new())?;
    card_header(cx, card, &d)?;
    if d.caps.brightness {
        compact_brightness(cx, card, &d)?;
    }
    let st = cx.state.display.clone();
    if let Some(auto) = st.auto_brightness {
        let row = Row::new(k("app.settings.display.auto_brightness"))
            .sublabel(k("app.settings.display.auto_brightness.sub"));
        let row = cx.ui.add(card, row)?;
        let switch = Switch::new(k("app.settings.display.auto_brightness"), auto);
        cx.act(row, switch, Act::DisplayAutoBrightness)?;
    }
    night_row(cx, card, st.night)?;
    compact_choices(cx, card, &d)?;
    super::display_rows::hdr_row(cx, card, &d)?;
    if !several {
        let note = cx.ui.add(
            content,
            Flex::row().gap(Space::S8).cross(CrossAlign::Center),
        )?;
        cx.ui.add(
            note,
            ultar_ui::widgets::Icon::new(IconId::Info)
                .small()
                .color(ultar_ui::tokens::ColorRole::TextMuted),
        )?;
        muted(cx.ui, note, k("app.settings.display.compact_note"))?;
    }
    Ok(())
}

/// Size of elements, refresh rate and orientation on a phone.
fn compact_choices(cx: &mut Cx<'_>, card: NodeId, d: &DisplayEntry) -> Result<(), UiError> {
    use super::display_rows::choice_row;
    choice_row(
        cx,
        card,
        "app.settings.display.scale",
        None,
        scale_choices(d),
        Act::DisplayScale,
    )?;
    choice_row(
        cx,
        card,
        "app.settings.display.refresh",
        None,
        refresh_choices(d),
        Act::DisplayRefresh,
    )?;
    choice_row(
        cx,
        card,
        "app.settings.display.orientation",
        None,
        orientation_choices(d),
        Act::DisplayOrientation,
    )?;
    Ok(())
}

/// Brightness on a phone (board *CompactDisplay*): the label above, the slider with the sun
/// before it; padding 8 / 16 / 12, a hairline under it.
fn compact_brightness(cx: &mut Cx<'_>, card: NodeId, d: &DisplayEntry) -> Result<(), UiError> {
    let block = cx.ui.add(
        card,
        Card::rule(BarEdge::Bottom).flex(
            Flex::column()
                .padding(Padding {
                    top: Some(Space::S8),
                    bottom: Some(Space::S12),
                    ..Padding::symmetric(Space::S16, Space::S8)
                })
                .gap(Space::S4),
        ),
    )?;
    cx.ui.add(
        block,
        Text::new(k("app.settings.display.brightness")).spec(spec::label()),
    )?;
    let line = cx
        .ui
        .add(block, Flex::row().gap(Space::S12).cross(CrossAlign::Center))?;
    cx.ui.add(
        line,
        ultar_ui::widgets::Icon::new(IconId::Sun).color(ultar_ui::tokens::ColorRole::TextMuted),
    )?;
    let slider =
        ultar_ui::widgets::Slider::new(k("app.settings.display.brightness"), 0.0, 100.0, 1.0)
            .value(d.brightness as f32);
    cx.act_sized(line, slider, Sizing::grow(1), Act::DisplayBrightness)?;
    Ok(())
}

fn night_row(cx: &mut Cx<'_>, card: NodeId, on: bool) -> Result<(), UiError> {
    let row =
        Row::new(k("app.settings.display.night")).sublabel(k("app.settings.display.night.sub"));
    let row = cx.ui.add(card, row)?;
    let pair = cx.ui.add(row, Flex::row().gap(Space::S8))?;
    cx.act(
        pair,
        Switch::new(k("app.settings.display.night"), on),
        Act::DisplayNightlight,
    )?;
    let more = IconButton::new(IconId::Forward, k("app.settings.display.night.more"));
    cx.act(pair, more, Act::DisplayNightlightMore)?;
    Ok(())
}

fn arrangement(cx: &mut Cx<'_>, parent: NodeId) -> Result<(), UiError> {
    let (card, header) = section(
        cx.ui,
        parent,
        Sizing::AUTO,
        k("app.settings.display.arrangement"),
        Some(k("app.settings.display.arrangement.sub")),
    )?;
    let identify = Button::new(k("app.settings.display.identify"));
    cx.act(header, identify, Act::DisplayIdentify)?;
    let detect = cx.act(
        header,
        Button::new(k("app.settings.display.detect")),
        Act::DisplayDetect,
    )?;
    cx.view.live.push((Act::DisplayDetect, detect));
    let body = cx
        .ui
        .add(card, Flex::column().padding(Padding::all(Space::S16)))?;
    let (items, selected) = arranged_items(cx);
    let canvas_h = metrics::arrangement_height();
    let canvas = Arrangement::new(k("app.settings.display.selected"), items).selected(selected);
    // Boards: the 144-point stage has its one-point edge outside (CSS content box).
    let sizing = Sizing::AUTO.with_height(canvas_h + 2.0 * ultar_ui::tokens::size::BORDER);
    cx.act_sized(body, canvas, sizing, Act::DisplaySelect)?;
    Ok(())
}

/// The tiles of the arrangement canvas in physical order, and the chosen one.
fn arranged_items(cx: &Cx<'_>) -> (Vec<ArrangedItem>, usize) {
    let st = &cx.state.display;
    let order = st.arrangement();
    let primary = st.primary_id().map(str::to_owned);
    // Boards: the main display 176 × 112, the others 144 × 96, 8 apart, centred under the
    // card head, vertically centred on the 144-high canvas (inside its one-point edge).
    let sizes: Vec<(f32, f32)> = order
        .iter()
        .map(|&i| {
            if primary.as_deref() == Some(st.displays[i].id.as_str()) {
                (176.0, 112.0)
            } else {
                (144.0, 96.0)
            }
        })
        .collect();
    let mut gap = Space::S8.points();
    let width = |sizes: &[(f32, f32)], gap: f32| {
        sizes.iter().map(|s| s.0).sum::<f32>() + gap * (sizes.len().max(1) - 1) as f32
    };
    let stage = main_width(cx.ui);
    // a page narrower than the board: the tiles shrink together until all of them are on the
    // stage (inside the card's padding and the stage's edge), none is cut off at the side
    let room = (stage - 2.0 * Space::S16.points() - 4.0 * ultar_ui::tokens::size::BORDER).max(1.0);
    let total = width(&sizes, gap);
    let fits = total <= room;
    let mut sizes = sizes;
    if !fits {
        let factor = room / total;
        gap *= factor;
        for s in &mut sizes {
            *s = (s.0 * factor, s.1 * factor);
        }
    }
    let total = width(&sizes, gap);
    let mut x = if fits {
        ((stage - total) / 2.0).max(0.0) + 1.0
    } else {
        1.0
    };
    let canvas_h = metrics::arrangement_height();
    let mut items = Vec::new();
    for (n, &i) in order.iter().enumerate() {
        let d = &st.displays[i];
        let is_primary = primary.as_deref() == Some(d.id.as_str());
        let (w, h) = sizes[n];
        let rect = ultar_ui::Rect::new(x, (canvas_h - h) / 2.0 + 1.0, w, h);
        x += w + gap;
        let mut item = ArrangedItem::new(
            tile_name(cx, d, is_primary),
            Label::data(&d.number.to_string()),
            rect,
        )
        .caption(short_name(d));
        if is_primary {
            item = item.tag(k("app.settings.display.main"));
        }
        items.push(item);
    }
    let selected = order.iter().position(|&i| i == st.selected).unwrap_or(0);
    (items, selected)
}

fn mode(cx: &mut Cx<'_>, parent: NodeId) -> Result<(), UiError> {
    let (card, _) = section(
        cx.ui,
        parent,
        Sizing::AUTO,
        k("app.settings.display.multi"),
        None,
    )?;
    let body = cx
        .ui
        .add(card, Flex::column().padding(Padding::all(Space::S16)))?;
    let st = &cx.state.display;
    let options = DisplayMode::ALL
        .iter()
        .map(|m| k(&format!("app.settings.display.mode.{}", m.id())))
        .collect();
    let index = DisplayMode::ALL
        .iter()
        .position(|m| *m == st.mode)
        .unwrap_or(0);
    let mut group = RadioGroup::new(k("app.settings.display.mode"), options)
        .selected(index)
        .layout(RadioLayout::Tiles);
    for (i, m) in DisplayMode::ALL.iter().enumerate() {
        let picture = match m {
            DisplayMode::Extend => Pictogram::Extend,
            DisplayMode::Mirror => Pictogram::Mirror,
            DisplayMode::Single => Pictogram::First,
        };
        group = group
            .detail(i, k(&format!("app.settings.display.mode.{}.sub", m.id())))
            .pictogram(i, picture);
    }
    cx.act(body, group, Act::DisplayMode)?;
    match st.mode {
        DisplayMode::Mirror if st.displays.len() >= 3 => mirror_rows(cx, card),
        DisplayMode::Single => single_row(cx, card),
        _ => Ok(()),
    }
}

fn mirror_rows(cx: &mut Cx<'_>, card: NodeId) -> Result<(), UiError> {
    let row = cx
        .ui
        .add(card, Row::new(k("app.settings.display.mirrored")).last())?;
    let boxes = cx.ui.add(row, Flex::row().gap(Space::S12))?;
    let st = &cx.state.display;
    for (i, d) in st.displays.iter().enumerate() {
        let on = st.mirror.is_empty() || st.mirror.contains(&d.id);
        let name = short_name(d);
        cx.act(boxes, Checkbox::new(name, on), Act::DisplayMirrorPick(i))?;
    }
    Ok(())
}

fn single_row(cx: &mut Cx<'_>, card: NodeId) -> Result<(), UiError> {
    let st = &cx.state.display;
    let options: Vec<Label> = st.displays.iter().map(short_name).collect();
    let index = st
        .displays
        .iter()
        .position(|d| d.id == st.single)
        .unwrap_or(0);
    let row = cx
        .ui
        .add(card, Row::new(k("app.settings.display.single")).last())?;
    let select = Select::new(k("app.settings.display.single"), options).selected(index);
    cx.act(row, select, Act::DisplaySingle)?;
    Ok(())
}

fn general(cx: &mut Cx<'_>, parent: NodeId) -> Result<(), UiError> {
    let card = cx.ui.add(parent, Card::new())?;
    let st = cx.state.display.clone();
    night_row(cx, card, st.night)?;
    if let Some(auto) = st.auto_brightness {
        let row = Row::new(k("app.settings.display.auto_brightness"))
            .sublabel(k("app.settings.display.auto_brightness.sub"));
        let row = cx.ui.add(card, row)?;
        let switch = Switch::new(k("app.settings.display.auto_brightness"), auto);
        cx.act(row, switch, Act::DisplayAutoBrightness)?;
    }
    let row = Row::new(k("app.settings.display.remember"))
        .sublabel(k("app.settings.display.remember.sub"));
    let row = cx.ui.add(card, row)?;
    let switch = Switch::new(k("app.settings.display.remember"), st.remember);
    cx.act(row, switch, Act::DisplayRemember)?;
    let bar = [
        k("app.settings.display.bar.primary"),
        k("app.settings.display.bar.all"),
    ];
    let has_lid = st.displays.iter().any(|d| d.internal);
    let mut row = Row::new(k("app.settings.display.bar"));
    if !has_lid {
        row = row.last();
    }
    let row = cx.ui.add(card, row)?;
    let select = Select::new(k("app.settings.display.bar"), bar.to_vec())
        .selected(usize::from(st.bar_all))
        .fit();
    cx.act(row, select, Act::DisplayBar)?;
    if has_lid {
        let lid = vec![
            k("app.settings.display.lid.stay_on"),
            k("app.settings.display.lid.suspend"),
        ];
        let row = cx
            .ui
            .add(card, Row::new(k("app.settings.display.lid")).last())?;
        let select = Select::new(k("app.settings.display.lid"), lid)
            .selected(usize::from(!st.lid_stay))
            .fit();
        cx.act(row, select, Act::DisplayLid)?;
    }
    Ok(())
}

fn details(cx: &mut Cx<'_>, parent: NodeId) -> Result<(), UiError> {
    let Some(d) = cx.state.display.current().cloned() else {
        let bar = InfoBar::new(Tone::Info, k("app.settings.display.none"));
        cx.ui.add(parent, bar)?;
        return Ok(());
    };
    let card = cx.ui.add(parent, Card::new())?;
    card_header(cx, card, &d)?;
    primary_row(cx, card, &d)?;
    super::display_rows::rows(cx, card, &d)
}

fn card_header(cx: &mut Cx<'_>, card: NodeId, d: &DisplayEntry) -> Result<(), UiError> {
    let header = cx.ui.add(
        card,
        Card::rule(BarEdge::Bottom).flex(
            Flex::row()
                .padding(Padding::all(Space::S16))
                .gap(Space::S12)
                .cross(CrossAlign::Center),
        ),
    )?;
    if !compact(cx.ui) {
        cx.ui
            .add(header, Badge::new(Label::data(&d.number.to_string())))?;
    }
    let texts = cx.ui.add_sized(header, Flex::column(), Sizing::grow(1))?;
    cx.ui
        .add(texts, Text::new(long_name(d)).spec(spec::card_title()))?;
    let res = d
        .resolution_px()
        .map_or_else(String::new, |(w, h)| format!("{w} × {h}"));
    let kind = if d.internal { "internal" } else { "external" };
    let sub = Label::key_with(
        format!("app.settings.display.summary.{kind}"),
        &[("res", &res)],
    );
    muted(cx.ui, texts, sub)?;
    Ok(())
}

fn primary_row(cx: &mut Cx<'_>, card: NodeId, d: &DisplayEntry) -> Result<(), UiError> {
    let row =
        Row::new(k("app.settings.display.primary")).sublabel(k("app.settings.display.primary.sub"));
    let row = cx.ui.add(card, row)?;
    let is_primary = cx.state.display.primary_id() == Some(d.id.as_str());
    if is_primary {
        let badge = Badge::new(k("app.settings.display.is_primary"))
            .tone(Tone::Success)
            .with_icon();
        cx.ui.add(row, badge)?;
    } else {
        let button = Button::new(k("app.settings.display.make_primary"));
        cx.act(row, button, Act::DisplayMakePrimary)?;
    }
    Ok(())
}

/// Option labels and the selected index of the per-display choices.
pub struct Choices {
    /// Labels.
    pub labels: Vec<Label>,
    /// Selected index.
    pub selected: usize,
}

/// Resolutions offered; the preferred one marked "recommended".
pub fn resolution_choices(d: &DisplayEntry) -> Choices {
    let list = d.resolutions();
    let current = d.resolution_px();
    let preferred = d.caps.preferred.map(|m| (m.width, m.height));
    let labels = list
        .iter()
        .map(|&(w, h)| {
            let text = format!("{w} × {h}");
            if Some((w, h)) == preferred {
                Label::key_with("app.settings.display.recommended_f", &[("value", &text)])
            } else {
                Label::data(&text)
            }
        })
        .collect();
    let selected = list.iter().position(|r| Some(*r) == current).unwrap_or(0);
    Choices { labels, selected }
}

/// Element sizes 100–300 %; the computed default marked.
pub fn scale_choices(d: &DisplayEntry) -> Choices {
    let labels = SCALES
        .iter()
        .map(|&s| {
            let text = format!("{s}%");
            if s == d.scale_default {
                Label::key_with("app.settings.display.recommended_m", &[("value", &text)])
            } else {
                Label::data(&text)
            }
        })
        .collect();
    let selected = SCALES.iter().position(|&s| s == d.scale).unwrap_or(0);
    Choices { labels, selected }
}

/// Refresh rates at the current resolution.
pub fn refresh_choices(d: &DisplayEntry) -> Choices {
    let rates = d.refresh_rates();
    let labels = rates
        .iter()
        .map(|&mhz| Label::data(&format!("{} Hz", format_hz(mhz))))
        .collect();
    let current = (d.refresh * 1000.0).round() as u32;
    let selected = rates.iter().position(|&r| r == current).unwrap_or(0);
    Choices { labels, selected }
}

/// `59.94` / `60` from millihertz.
pub fn format_hz(mhz: u32) -> String {
    if mhz.is_multiple_of(1000) {
        (mhz / 1000).to_string()
    } else {
        format!("{:.2}", f64::from(mhz) / 1000.0)
    }
}

/// Orientations.
pub fn orientation_choices(d: &DisplayEntry) -> Choices {
    let labels = ORIENTATIONS
        .iter()
        .map(|o| k(&format!("app.settings.display.orientation.{o}")))
        .collect();
    let selected = ORIENTATIONS
        .iter()
        .position(|o| *o == d.orientation)
        .unwrap_or(0);
    Choices { labels, selected }
}

/// Colour profiles: automatic first.
pub fn profile_choices(d: &DisplayEntry) -> Choices {
    let mut labels = vec![k("app.settings.display.profile.auto")];
    labels.extend(d.caps.profiles.iter().map(|p| Label::data(p)));
    let selected = d
        .caps
        .profiles
        .iter()
        .position(|p| *p == d.profile)
        .map_or(0, |i| i + 1);
    Choices { labels, selected }
}
