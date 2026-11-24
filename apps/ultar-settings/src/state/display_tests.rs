use super::*;
use ultar_bus::messages::settings::{ItemKind, RiskInfo, ScopeInfo};
use ultar_bus::Text;

fn item(key: &str, value: SettingValue) -> ItemInfo {
    ItemInfo {
        key: key.into(),
        label: Text::key("x"),
        description: Text::key("x"),
        kind: ItemKind::Toggle,
        value: value.clone(),
        default: value,
        risk: RiskInfo::Safe,
        scope: ScopeInfo::System,
        writable: true,
        pending: false,
        template: false,
    }
}

fn panel(items: Vec<ItemInfo>) -> Vec<PanelInfo> {
    vec![PanelInfo {
        id: "display".into(),
        title: Text::key("x"),
        items,
    }]
}

#[test]
fn panels_and_outputs_join_by_id() {
    let mut st = DisplayState::default();
    st.apply_panels(&panel(vec![
        item("display.mode", SettingValue::Text("mirror".into())),
        item("display.primary", SettingValue::Text("b".into())),
        item(
            "display.b.resolution",
            SettingValue::Text("1920x1080".into()),
        ),
        item("display.a.scale", SettingValue::Integer(200)),
        item("display.a.position_x", SettingValue::Integer(1920)),
        item("display.b.position_x", SettingValue::Integer(0)),
        item("display.night_light.enabled", SettingValue::Flag(true)),
        item("bar.displays", SettingValue::Text("all".into())),
        item(
            "display.lid_close_with_external",
            SettingValue::Text("stay_on".into()),
        ),
    ]));
    assert_eq!(st.mode, DisplayMode::Mirror);
    assert!(st.night && st.bar_all && st.lid_stay);
    assert_eq!(st.displays.len(), 2);
    let mode = Mode {
        width: 3840,
        height: 2160,
        refresh_mhz: 60_000,
    };
    st.apply_outputs(&[Output {
        id: "b".into(),
        connector: "edp-1".into(),
        name: String::new(),
        internal: true,
        diagonal_tenths: 156,
        preferred: mode,
        current: mode,
        modes: vec![
            mode,
            Mode {
                width: 1920,
                height: 1080,
                refresh_mhz: 60_000,
            },
        ],
        hdr: false,
        brightness: true,
        profiles: vec![],
    }]);
    assert!(st.outputs_known);
    assert_eq!(st.displays[0].id, "b", "built-in display is number 1");
    assert_eq!(st.displays[0].number, 1);
    assert_eq!(st.displays[0].resolution_px(), Some((1920, 1080)));
    assert_eq!(
        st.displays[0].resolutions(),
        vec![(3840, 2160), (1920, 1080)]
    );
    assert_eq!(st.displays[0].refresh_rates(), vec![60_000]);
    assert_eq!(st.arrangement(), vec![0, 1], "physical order by position");
    assert_eq!(st.primary_id(), Some("b"));
    assert_eq!(st.current().map(|d| d.id.as_str()), Some("b"));
}

#[test]
fn unknown_values_fall_back() {
    assert_eq!(parse_resolution("auto"), None);
    assert_eq!(parse_resolution("10x"), None);
    assert_eq!(DisplayMode::from_id("x"), None);
    let st = DisplayState::default();
    assert_eq!(st.primary_id(), None);
    assert!(st.current().is_none());
    let d = DisplayEntry {
        refresh: 59.94,
        ..DisplayEntry::default()
    };
    assert_eq!(d.refresh_rates(), vec![59_940]);
}
