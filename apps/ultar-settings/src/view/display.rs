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
    Badge, Button, Card, Checkbox, IconButton, InfoBar, Label, RadioGroup, Row, Select, Switch,
    Text, Tone,
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
    let columns = cx.ui.add(
        content,
        Flex::row().gap(Space::S16).cross(CrossAlign::Start),
    )?;
    let main = cx.ui.add_sized(
        columns,
        Flex::column().gap(Space::S16),
        Sizing::width(metrics::display_main_column()),
    )?;
    arrangement(cx, main)?;
    if cx.state.display.displays.len() > 1 {
        mode(cx, main)?;
    }
    general(cx, main)?;
    let side = cx.ui.add_sized(columns, Flex::column(), Sizing::grow(1))?;
    details(cx, side)
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
        super::display_rows::brightness_row(cx, card, &d, false)?;
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
    use super::display_rows::choice_row;
    choice_row(
        cx,
        card,
        "app.settings.display.scale",
        None,
        scale_choices(&d),
        Act::DisplayScale,
    )?;
    choice_row(
        cx,
        card,
        "app.settings.display.refresh",
        None,
        refresh_choices(&d),
        Act::DisplayRefresh,
    )?;
    choice_row(
        cx,
        card,
        "app.settings.display.orientation",
        None,
        orientation_choices(&d),
        Act::DisplayOrientation,
    )?;
    super::display_rows::hdr_row(cx, card, &d)?;
    if !several {
        muted(cx.ui, content, k("app.settings.display.compact_note"))?;
    }
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
    let st = &cx.state.display;
    let order = st.arrangement();
    let primary = st.primary_id().map(str::to_owned);
    let options: Vec<Label> = order
        .iter()
        .map(|&i| {
            let d = &st.displays[i];
            tile_name(cx, d, primary.as_deref() == Some(d.id.as_str()))
        })
        .collect();
    let selected = order.iter().position(|&i| i == st.selected).unwrap_or(0);
    let group = RadioGroup::new(k("app.settings.display.selected"), options).selected(selected);
    cx.act(body, group, Act::DisplaySelect)?;
    Ok(())
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
    let group = RadioGroup::new(k("app.settings.display.mode"), options).selected(index);
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
    let select =
        Select::new(k("app.settings.display.bar"), bar.to_vec()).selected(usize::from(st.bar_all));
    cx.act(row, select, Act::DisplayBar)?;
    if has_lid {
        let lid = vec![
            k("app.settings.display.lid.stay_on"),
            k("app.settings.display.lid.suspend"),
        ];
        let row = cx
            .ui
            .add(card, Row::new(k("app.settings.display.lid")).last())?;
        let select =
            Select::new(k("app.settings.display.lid"), lid).selected(usize::from(!st.lid_stay));
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
        Flex::row()
            .padding(Padding::all(Space::S16))
            .gap(Space::S12),
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
