//! The stroke icon set (design spec §4): one stroke width, outline style, drawn from SVG path
//! data in a 24-unit box. Icons with a direction are mirrored in right-to-left layouts.

pub mod path;
mod raster;

pub use path::{PathError, Subpath};
pub use raster::{IconCache, QuarterTurns, ICON_CACHE_BYTES};

/// Edge length of the box icon paths are drawn in.
pub const VIEW_BOX: f32 = 24.0;

macro_rules! icons {
    ($( $variant:ident, $name:literal, $mirror:literal, $doc:literal, $path:literal; )*) => {
        /// The icons of the system set. Each has exactly one meaning (design spec §4).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum IconId {
            $( #[doc = $doc] $variant, )*
        }

        impl IconId {
            /// Every icon of the set.
            pub const ALL: &'static [IconId] = &[ $( IconId::$variant, )* ];

            /// The icon's name in the design sources.
            pub const fn name(self) -> &'static str {
                match self { $( IconId::$variant => $name, )* }
            }

            /// The SVG path data in the 24-unit box.
            pub const fn path_data(self) -> &'static str {
                match self { $( IconId::$variant => $path, )* }
            }

            /// `true` when the icon has a direction and is mirrored in right-to-left layouts.
            pub const fn mirrored(self) -> bool {
                match self { $( IconId::$variant => $mirror, )* }
            }
        }
    };
}

icons! {
    Grid, "grid", false, "Nine dots: all apps.",
        "M5 5h.01M12 5h.01M19 5h.01M5 12h.01M12 12h.01M19 12h.01M5 19h.01M12 19h.01M19 19h.01";
    Search, "search", false, "Magnifier: search.",
        "M11 4a7 7 0 1 0 0 14a7 7 0 0 0 0-14zM20 20l-3.5-3.5";
    Wifi, "wifi", false, "Fan: Wi-Fi.",
        "M2.5 9a14 14 0 0 1 19 0M5.5 12.5a9.5 9.5 0 0 1 13 0M8.5 16a5 5 0 0 1 7 0M12 19.5h.01";
    Volume, "volume", true, "Speaker: sound.",
        "M4 9.5v5h3.5l4.5 4v-13l-4.5 4zM15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11";
    Battery, "battery", false, "Battery: energy.",
        "M3 8h15v8H3zM21 11v2M5.5 10.5h8v3h-8z";
    Bluetooth, "bluetooth", false, "Rune: Bluetooth.",
        "M7 7l10 10-5 4V3l5 4L7 17";
    Moon, "moon", false, "Moon: night light / dark.",
        "M20 14.5A8.5 8.5 0 1 1 9.5 4a7 7 0 0 0 10.5 10.5z";
    Sun, "sun", false, "Sun: brightness.",
        "M12 8a4 4 0 1 0 0 8a4 4 0 0 0 0-8zM12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4";
    Gear, "gear", false, "Gear: settings (only that).",
        "M12 9a3 3 0 1 0 0 6a3 3 0 0 0 0-6zM19.4 13.5a7.6 7.6 0 0 0 0-3l2-1.5-2-3.5-2.4 1a7.6 7.6 0 0 0-2.6-1.5L14 2.5h-4l-.4 2.5A7.6 7.6 0 0 0 7 6.5l-2.4-1-2 3.5 2 1.5a7.6 7.6 0 0 0 0 3l-2 1.5 2 3.5 2.4-1a7.6 7.6 0 0 0 2.6 1.5l.4 2.5h4l.4-2.5a7.6 7.6 0 0 0 2.6-1.5l2.4 1 2-3.5z";
    Power, "power", false, "Power ring: power menu.",
        "M12 3v8M6.3 6.3a8 8 0 1 0 11.4 0";
    Folder, "folder", false, "Folder: files.",
        "M3 6.5h6l2 2.5h10v9.5H3z";
    Globe, "globe", false, "Globe: browser / network.",
        "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18zM3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18";
    Terminal, "terminal", false, "Prompt window: terminal.",
        "M3 5h18v14H3zM7 10l3 2.5L7 15M12.5 15H17";
    Bag, "bag", false, "Shopping bag: Ultar Store.",
        "M5 8h14l-1 12H6zM9 8V6.5a3 3 0 0 1 6 0V8";
    File, "file", false, "Sheet: text document.",
        "M6 3h8l4 4v14H6zM14 3v4h4M9 12h6M9 16h6";
    Trash, "trash", false, "Bin: delete.",
        "M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13";
    Monitor, "monitor", false, "Screen: display.",
        "M3 5h18v11H3zM9 20h6M12 16v4";
    Chip, "chip", false, "Chip: drivers and hardware.",
        "M7 7h10v10H7zM10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4";
    Shield, "shield", false, "Shield: security.",
        "M12 3l8 3v6c0 4.5-3.3 7.8-8 9c-4.7-1.2-8-4.5-8-9V6z";
    Bell, "bell", false, "Bell: notifications.",
        "M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15zM10 20a2 2 0 0 0 4 0";
    Mesh, "mesh", false, "Two linked circles: device group.",
        "M7 9.5a2.5 2.5 0 1 0 0 5a2.5 2.5 0 0 0 0-5zM17 9.5a2.5 2.5 0 1 0 0 5a2.5 2.5 0 0 0 0-5zM9.5 12h5";
    Plane, "plane", false, "Plane: flight mode.",
        "M3 13l18-8-6 16-3-6z";
    Minimize, "min", false, "Dash: minimise window.",
        "M6 12h12";
    Maximize, "max", false, "Square: maximise window.",
        "M6 6h12v12H6z";
    Close, "close", false, "Cross: close window or dialog.",
        "M6 6l12 12M18 6L6 18";
    ChevronEnd, "chev", true, "Chevron towards the end edge: next / open.",
        "M9 6l6 6-6 6";
    Check, "check", false, "Check mark: done / selected.",
        "M5 12.5l4.5 4.5L19 7.5";
    Image, "image", false, "Picture: image.",
        "M3 5h18v14H3zM3 16l5-5 4 4 3-3 6 6M15.5 9.5h.01";
    Download, "download", false, "Arrow onto line: download.",
        "M12 4v11M7.5 11l4.5 4.5 4.5-4.5M5 20h14";
    User, "user", false, "Person: account.",
        "M12 4a4 4 0 1 0 0 8a4 4 0 0 0 0-8zM4.5 20a7.5 7.5 0 0 1 15 0";
    Keyboard, "keyboard", false, "Keyboard: keyboard.",
        "M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M8 14h8";
    Lock, "lock", false, "Closed lock: locked.",
        "M6 11h12v9H6zM8.5 11V8a3.5 3.5 0 0 1 7 0v3";
    Screenshot, "shot", false, "Corner marks: screenshot.",
        "M4 8V5h3M17 5h3v3M20 16v3h-3M7 19H4v-3";
    Palette, "palette", false, "Palette: appearance.",
        "M12 3a9 9 0 1 0 0 18c1.5 0 2-1 1.5-2.2c-.6-1.4.3-2.8 2-2.8H18a3 3 0 0 0 3-3c0-5.5-4-10-9-10zM7.5 12h.01M9.5 8h.01M14 7.5h.01";
    Window, "window", false, "Window: desktop and windows.",
        "M4 5h16v14H4zM4 9h16";
    Mouse, "mouse", false, "Mouse: mouse / touchpad.",
        "M8 3h8a3 3 0 0 1 3 3v9a7 7 0 0 1-14 0V6a3 3 0 0 1 3-3zM12 3v6";
    Update, "update", false, "Circular arrows: updates / reload.",
        "M4 12a8 8 0 0 1 14-5.3L20 9M20 4v5h-5M20 12a8 8 0 0 1-14 5.3L4 15M4 20v-5h5";
    Accessibility, "access", false, "Figure: accessibility.",
        "M12 3.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 0 0 0-3zM5 9l7 1 7-1M12 10v5M12 15l-3 6M12 15l3 6";
    Cable, "cable", false, "Plug: wired network.",
        "M9 3v5M15 3v5M7 8h10v4a5 5 0 0 1-10 0zM12 17v4";
    Home, "home", false, "House: home folder.",
        "M4 11l8-7 8 7v9h-5v-6H9v6H4z";
    Code, "code", false, "Angle brackets: code.",
        "M8 7l-5 5 5 5M16 7l5 5-5 5M13.5 5l-3 14";
    Music, "music", false, "Notes: music.",
        "M9 18V5l11-2v13M9 18a3 3 0 1 1-6 0a3 3 0 0 1 6 0zM20 16a3 3 0 1 1-6 0a3 3 0 0 1 6 0z";
    Assistant, "spark", true, "Speech bubble with spark: assistant.",
        "M5 5h14v10H10l-5 4zM12 7.5l.9 1.9 1.9.9-1.9.9-.9 1.9-.9-1.9-1.9-.9 1.9-.9z";
    Package, "box", false, "Box: package.",
        "M4 8l8-4 8 4v8l-8 4-8-4zM4 8l8 4 8-4M12 12v8";
    Plus, "plus", false, "Plus: new / add.",
        "M12 5v14M5 12h14";
    Star, "star", false, "Star: favourite.",
        "M12 4l2.5 5 5.5.8-4 3.9 1 5.5-5-2.7-5 2.7 1-5.5-4-3.9 5.5-.8z";
    Usb, "usb", false, "Trident: USB.",
        "M12 3v14M9 6l3-3 3 3M12 17a2 2 0 1 0 0 4a2 2 0 0 0 0-4zM7 9v3l5 3M17 8v4l-5 3";
    Info, "info", false, "Circle with i: information.",
        "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18zM12 11v6M12 7.5h.01";
    Success, "success", false, "Circle with check mark: it worked.",
        "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18zM8 12.5l3 3 5-6";
    Warning, "warning", false, "Triangle with exclamation mark: attention.",
        "M12 3.5l9.5 16.5h-19zM12 10v4.5M12 17.5h.01";
    Error, "error", false, "Circle with cross: it failed.",
        "M12 3a9 9 0 1 0 0 18a9 9 0 0 0 0-18zM9 9l6 6M15 9l-6 6";
}

impl IconId {
    /// The flattened outline of the icon.
    pub fn outline(self) -> Result<Vec<Subpath>, PathError> {
        path::parse(self.path_data())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_parses_and_stays_in_the_view_box() {
        for icon in IconId::ALL {
            let outline = icon
                .outline()
                .unwrap_or_else(|e| panic!("{}: {e}", icon.name()));
            assert!(!outline.is_empty(), "{}", icon.name());
            for sub in &outline {
                for p in &sub.points {
                    let inside = |v: f32| (0.0..=VIEW_BOX).contains(&v);
                    assert!(inside(p.x) && inside(p.y), "{} {p:?}", icon.name());
                }
            }
        }
    }

    #[test]
    fn names_are_unique_and_directional_icons_are_flagged() {
        let mut names: Vec<_> = IconId::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count);
        assert!(IconId::ChevronEnd.mirrored());
        assert!(IconId::Volume.mirrored());
        assert!(!IconId::Check.mirrored() && !IconId::Gear.mirrored() && !IconId::Close.mirrored());
    }
}
