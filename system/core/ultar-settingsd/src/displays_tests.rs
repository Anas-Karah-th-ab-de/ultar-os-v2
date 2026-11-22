use super::*;
use crate::schema::{Panel, Scope};

fn descriptor(mfg: &str, product: &str, serial: &str, connector: &str) -> DisplayDescriptor {
    DisplayDescriptor {
        manufacturer: mfg.into(),
        product: product.into(),
        serial: serial.into(),
        connector: connector.into(),
        ..DisplayDescriptor::default()
    }
}

fn laptop_panel() -> DisplayDescriptor {
    DisplayDescriptor {
        internal: true,
        width_px: 2880,
        height_px: 1800,
        dpi: 192,
        refresh_mhz: 90_000,
        ..descriptor("BOE", "NE135", "", "eDP-1")
    }
}

fn dock_monitor() -> DisplayDescriptor {
    DisplayDescriptor {
        width_px: 3840,
        height_px: 2160,
        dpi: 163,
        refresh_mhz: 60_000,
        ..descriptor("DEL", "U2720Q", "7XK2", "DP-3")
    }
}

fn projector() -> DisplayDescriptor {
    DisplayDescriptor {
        width_px: 1920,
        height_px: 1080,
        refresh_mhz: 59_940,
        ..descriptor("EPS", "EB-U42", "", "HDMI-A-1")
    }
}

fn ids(descriptors: &[DisplayDescriptor]) -> Vec<String> {
    identify(descriptors)
        .unwrap()
        .into_iter()
        .map(|display| display.id)
        .collect()
}

#[test]
fn identifier_comes_from_the_display_not_from_the_port() {
    assert_eq!(base_display_id(&dock_monitor()), "del-u2720q-7xk2");
    let other_port = DisplayDescriptor {
        connector: "HDMI-A-2".into(),
        ..dock_monitor()
    };
    assert_eq!(base_display_id(&other_port), "del-u2720q-7xk2");
    assert_eq!(
        base_display_id(&descriptor("", "", "", "HDMI-A-1")),
        "hdmi-a-1"
    );
    assert_eq!(base_display_id(&descriptor("", "", "", "")), "display");
    assert_eq!(base_display_id(&descriptor("شركة", "", "", "")), "display");
    let long = base_display_id(&descriptor(&"m".repeat(60), &"p".repeat(60), "s", "c"));
    assert!(crate::schema::is_valid_segment(&long));
}

#[test]
fn identical_displays_are_told_apart() {
    let twins = [
        descriptor("DEL", "P2419H", "", "DP-1"),
        descriptor("DEL", "P2419H", "", "DP-2"),
        descriptor("BOE", "X", "", "eDP-1"),
    ];
    assert_eq!(ids(&twins), ["del-p2419h-dp-1", "del-p2419h-dp-2", "boe-x"]);
    let no_data = [
        descriptor("", "", "", ""),
        descriptor("", "", "", ""),
        descriptor("", "", "", ""),
    ];
    assert_eq!(ids(&no_data), ["display", "display-2", "display-3"]);
    let long = "x".repeat(80);
    let long_twins = [
        descriptor(&long, "", "", "a"),
        descriptor(&long, "", "", "b"),
    ];
    let found = ids(&long_twins);
    assert_ne!(found[0], found[1]);
    assert!(found.iter().all(|id| crate::schema::is_valid_segment(id)));
}

#[test]
fn too_many_displays_are_refused() {
    let many = vec![projector(); MAX_DISPLAYS + 1];
    assert_eq!(identify(&many), Err(DisplayError::TooMany));
    assert_eq!(
        identify(&vec![projector(); MAX_DISPLAYS]).unwrap().len(),
        MAX_DISPLAYS
    );
    assert!(identify(&[]).unwrap().is_empty());
    assert!(!DisplayError::TooMany.to_string().is_empty());
}

#[test]
fn combination_is_a_set_not_a_sequence() {
    let home = combination_id(&ids(&[laptop_panel(), dock_monitor()]));
    let home_reversed = combination_id(&ids(&[dock_monitor(), laptop_panel()]));
    let office = combination_id(&ids(&[laptop_panel(), projector()]));
    let alone = combination_id(&ids(&[laptop_panel()]));
    assert_eq!(home, home_reversed);
    assert_ne!(home, office);
    assert_ne!(home, alone);
    assert_ne!(office, alone);
    assert!(crate::schema::is_valid_segment(&home));
    assert_eq!(home.len(), 18);
    // Stable across runs and builds: the value is part of the file format.
    assert_eq!(combination_id::<&str>(&[]), "c-cbf29ce484222325");
    assert_eq!(
        combination_id(&["a", "b"]),
        combination_id(&["b", "a", "a"])
    );
    assert_ne!(combination_id(&["ab"]), combination_id(&["a", "b"]));
}

#[test]
fn scale_recommendation_follows_density_in_steps() {
    let cases = [
        (0, 100),
        (72, 100),
        (96, 100),
        (108, 100),
        (110, 125),
        (120, 125),
        (144, 150),
        (163, 175),
        (192, 200),
        (288, 300),
        (600, 300),
    ];
    for (dpi, percent) in cases {
        assert_eq!(recommended_scale(dpi), percent, "dpi {dpi}");
    }
}

#[test]
fn first_time_arrangement_extends_from_the_built_in_display() {
    let displays = identify(&[dock_monitor(), laptop_panel()]).unwrap();
    assert_eq!(default_primary(&displays), Some("boe-ne135"));
    // Built-in panel: 2880 px at 200 % is 1440 wide; the monitor follows.
    assert_eq!(default_position(&displays, "boe-ne135"), Some((0, 0)));
    assert_eq!(
        default_position(&displays, "del-u2720q-7xk2"),
        Some((1440, 0))
    );
    assert_eq!(default_position(&displays, "nope"), None);

    let external_only = identify(&[projector(), dock_monitor()]).unwrap();
    assert_eq!(default_primary(&external_only), Some("del-u2720q-7xk2"));
    assert_eq!(default_primary(&[]), None);
    let unknown = [DisplayInfo::bare("a"), DisplayInfo::bare("b")];
    assert_eq!(default_position(&unknown, "b"), Some((1920, 0)));
}

#[test]
fn computed_defaults_of_display_settings() {
    let displays = identify(&[laptop_panel(), projector()]).unwrap();
    let provider = DisplayDefaults(&displays);
    let def = |key: &'static str| {
        SettingDef::flag(key, Panel::Display, false).scope(Scope::DisplayCombination)
    };
    let ask = |key: &'static str, id: Option<&str>| provider.computed_default(&def(key), id);
    assert_eq!(ask("display.primary", None), Some(Value::text("boe-ne135")));
    assert_eq!(
        ask("display.single_display", None),
        Some(Value::text("boe-ne135"))
    );
    assert_eq!(ask("display.mode", None), None);
    let panel = Some("boe-ne135");
    let beamer = Some("eps-eb-u42");
    assert_eq!(ask("display.<id>.scale", panel), Some(Value::Int(200)));
    assert_eq!(ask("display.<id>.scale", beamer), None);
    assert_eq!(
        ask("display.<id>.resolution", beamer),
        Some(Value::text("1920x1080"))
    );
    assert_eq!(
        ask("display.<id>.refresh_rate", beamer),
        Some(Value::Float(59.94))
    );
    assert_eq!(
        ask("display.<id>.position_x", beamer),
        Some(Value::Int(1440))
    );
    assert_eq!(ask("display.<id>.position_y", beamer), Some(Value::Int(0)));
    assert_eq!(ask("display.<id>.hdr", panel), None);
    assert_eq!(ask("display.<id>.scale", Some("unplugged")), None);
    assert_eq!(
        DisplayDefaults(&[]).computed_default(&def("display.primary"), None),
        None
    );
}
