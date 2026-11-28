//! What the helper can do: a short list of tools, each one a call on the system bus.
//! A tool that is not in this list does not exist for the helper, whatever a model says.

use std::collections::BTreeMap;
use ultar_bus::messages::agent::{
    Area, AreaReach, Progress, Reach, StepOutcome, ToolInfo, ToolRisk,
};

/// How much a tool can change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Risk {
    /// It only looks.
    Reads,
    /// It changes something that is changed back as easily.
    Changes,
    /// It ends work or cannot be taken back: the person confirms it in the system dialog.
    Dangerous,
}

/// One tool.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tool {
    /// Its stable name.
    pub name: &'static str,
    /// The part of the system it reaches.
    pub area: Area,
    /// How much it can change.
    pub risk: Risk,
    /// Its arguments by name.
    pub args: &'static [&'static str],
    /// What it does, for the model.
    pub about: &'static str,
}

use Area::{Apps, Devices, Help, Network, Notifications, Power, Screen, Settings, Sound, Storage};
use Risk::{Changes, Dangerous, Reads};

/// Every tool of the helper.
pub const TOOLS: [Tool; 34] = [
    tool("list_apps", Apps, Reads, &[], "lists the ids of the installed apps"),
    tool("open_app", Apps, Changes, &["app"], "opens the app with this id"),
    tool("close_app", Apps, Dangerous, &["app"], "closes the app with this id; unsaved work in it is lost"),
    tool("settings_panels", Settings, Reads, &[], "lists the panels of the system settings"),
    tool("settings_list", Settings, Reads, &["panel"], "lists every setting of one panel with its key, value and accepted values"),
    tool("setting_get", Settings, Reads, &["key"], "reads one setting by its key"),
    tool("setting_set", Settings, Changes, &["key", "value"], "changes one setting; settings of privacy, accounts, users, sharing, updates and emergency are confirmed by the person first"),
    tool("network_status", Network, Reads, &[], "says whether Wi-Fi, a cable, Bluetooth and airplane mode are on and which network is used"),
    tool("wifi", Network, Changes, &["on"], "switches Wi-Fi on (true) or off (false)"),
    tool("bluetooth", Network, Changes, &["on"], "switches Bluetooth on (true) or off (false)"),
    tool("airplane", Network, Changes, &["on"], "switches airplane mode on (true) or off (false)"),
    tool("sound_status", Sound, Reads, &[], "says the volume, whether sound is muted and which output is used"),
    tool("set_volume", Sound, Changes, &["percent"], "sets the sound volume, 0 to 100"),
    tool("mute", Sound, Changes, &["on"], "mutes (true) or unmutes (false) the sound"),
    tool("media", Sound, Changes, &["button"], "presses a media button: play_pause, next or previous"),
    tool("screenshot", Screen, Changes, &[], "takes a screenshot and says where it was saved"),
    tool("see_screen", Screen, Reads, &[], "asks the person in the system dialog, then takes a screenshot of the screen and says where it was saved; only when the person allowed the helper to see the screen"),
    tool("lock", Screen, Changes, &[], "locks the screen"),
    tool("notifications", Notifications, Reads, &[], "lists the open notifications with their ids"),
    tool("dismiss_notification", Notifications, Changes, &["id"], "removes the notification with this id"),
    tool("storage", Storage, Reads, &[], "says how full the device and each connected drive is"),
    tool("eject", Storage, Changes, &["drive"], "unmounts the drive with this id so it can be pulled out"),
    tool("devices", Devices, Reads, &[], "lists the person's paired devices and whether they are connected"),
    tool("continue_on", Devices, Changes, &["device"], "offers what is open here to another paired device (device id, or empty for the only one); that device asks its person"),
    tool("send_file", Devices, Changes, &["device", "path"], "offers a file to a paired device; that device asks its person"),
    tool("ask_device", Devices, Changes, &["device", "request"], "asks the helper of a paired device (device id) to do the request there; works only when that device allows control"),
    tool("battery", Power, Reads, &[], "says the battery charge and whether the device is charging"),
    tool("suspend", Power, Dangerous, &[], "puts this device to sleep"),
    tool("sign_out", Power, Dangerous, &[], "ends the session of the person; open apps are closed"),
    tool("restart", Power, Dangerous, &[], "restarts this device"),
    tool("power_off", Power, Dangerous, &[], "switches this device off"),
    tool("help", Help, Reads, &["topic"], "explains this system; topics: pairing, continue, files, model, wifi, bluetooth, updates, helper"),
    tool("what_can_you_do", Help, Reads, &[], "lists the areas of the system the person lets you reach and those switched off"),
    tool("what_did_you_do", Help, Reads, &[], "lists what you did lately for the person"),
];

const fn tool(
    name: &'static str,
    area: Area,
    risk: Risk,
    args: &'static [&'static str],
    about: &'static str,
) -> Tool {
    Tool {
        name,
        area,
        risk,
        args,
        about,
    }
}

impl Tool {
    /// The risk as surfaces show it.
    pub const fn shown_risk(&self) -> ToolRisk {
        match self.risk {
            Reads => ToolRisk::Reads,
            Changes => ToolRisk::Changes,
            Dangerous => ToolRisk::Confirms,
        }
    }
}

/// What the person closed to the helper.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Limits {
    /// Areas switched off.
    pub off: Vec<Area>,
    /// The helper only looks and explains (`assistant.act` is off).
    pub read_only: bool,
    /// The helper may see the screen when asked (`assistant.see_screen`, off until then).
    pub see_screen: bool,
    /// Suspicious links in the model's answers are warned about (`assistant.check_links`).
    pub check_links: bool,
    /// The request was spoken while the screen was locked: only harmless looking is open.
    pub locked: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            off: Vec::new(),
            read_only: false,
            see_screen: false,
            check_links: true,
            locked: false,
        }
    }
}

/// Tools that only look but still show something private: closed while the screen is locked.
pub const LOCKED_CLOSED: [&str; 7] = [
    "see_screen",
    "notifications",
    "settings_list",
    "setting_get",
    "what_did_you_do",
    "devices",
    "storage",
];

impl Limits {
    /// Why `tool` is closed now, in words for the model (`None`: it is open).
    pub fn closes(&self, tool: &Tool) -> Option<&'static str> {
        if self.off.contains(&tool.area) {
            Some("the person switched this area off for the helper")
        } else if tool.name == "see_screen" && !self.see_screen {
            Some("the person has not allowed the helper to see the screen")
        } else if self.locked && (tool.risk != Reads || LOCKED_CLOSED.contains(&tool.name)) {
            Some("the screen is locked: only harmless looking is open; the person unlocks first")
        } else if self.read_only && tool.risk != Reads {
            Some("the person lets the helper only look, not act; they change that in Settings")
        } else {
            None
        }
    }
}

/// What the helper reaches under `limits`.
pub fn reach(limits: &Limits) -> Reach {
    let off = &limits.off;
    let areas = Area::ALL.into_iter().map(|area| AreaReach {
        area,
        allowed: !off.contains(&area),
        tools: TOOLS
            .iter()
            .filter(|t| t.area == area)
            .map(|t| ToolInfo {
                name: t.name.to_owned(),
                risk: t.shown_risk(),
            })
            .collect(),
    });
    Reach {
        acts: !limits.read_only,
        areas: areas.collect(),
    }
}

/// The tool called `name`.
pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// One use of a tool.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Call {
    /// The tool.
    pub tool: String,
    /// Its arguments.
    pub args: BTreeMap<String, String>,
}

impl Call {
    /// A call of `tool` with `args`.
    pub fn new(tool: &str, args: &[(&str, &str)]) -> Self {
        Self {
            tool: tool.to_owned(),
            args: args
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    /// The argument `name` (empty when missing).
    pub fn arg(&self, name: &str) -> &str {
        self.args.get(name).map_or("", String::as_str)
    }
}

/// What using a tool led to: how it went, and what the model is told about it.
pub type Outcome = (StepOutcome, String);

/// Something that carries out tool calls.
pub trait Toolbox {
    /// Uses one tool.
    fn run(&self, call: &Call) -> Outcome;

    /// Tells whoever watches that the request moved on (nobody, unless overridden).
    fn note(&self, _progress: &Progress) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_have_unique_names_and_the_dangerous_ones_are_marked() {
        let mut names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len());
        let dangerous: Vec<&str> = TOOLS
            .iter()
            .filter(|t| t.risk == Risk::Dangerous)
            .map(|t| t.name)
            .collect();
        assert_eq!(
            dangerous,
            ["close_app", "suspend", "sign_out", "restart", "power_off"]
        );
        // every area holds a tool, and a switched-off area is shown as such
        let limits = Limits {
            off: vec![Area::Power],
            read_only: true,
            ..Limits::default()
        };
        let all = reach(&limits);
        assert!(!all.acts);
        assert!(limits.closes(find("wifi").unwrap()).is_some());
        assert!(
            limits.closes(find("battery").unwrap()).is_some(),
            "area off"
        );
        assert!(limits.closes(find("network_status").unwrap()).is_none());
        assert!(all.areas.iter().all(|a| !a.tools.is_empty()));
        let off: Vec<Area> = all
            .areas
            .iter()
            .filter(|a| !a.allowed)
            .map(|a| a.area)
            .collect();
        assert_eq!(off, [Area::Power]);
        assert!(find("format_disk").is_none());
        assert_eq!(Call::new("wifi", &[("on", "true")]).arg("on"), "true");
        assert_eq!(Call::new("wifi", &[]).arg("on"), "");
    }

    #[test]
    fn a_locked_screen_leaves_only_harmless_looking_open() {
        let locked = Limits {
            locked: true,
            see_screen: true,
            ..Limits::default()
        };
        for tool in TOOLS.iter() {
            let open = locked.closes(tool).is_none();
            let harmless = tool.risk == Reads && !LOCKED_CLOSED.contains(&tool.name);
            assert_eq!(open, harmless, "{}", tool.name);
        }
        // the harmless ones the person may expect
        for name in ["battery", "network_status", "sound_status", "help"] {
            assert!(locked.closes(find(name).unwrap()).is_none(), "{name}");
        }
        // acting and ending work stay closed, also the dangerous ones
        for name in [
            "wifi",
            "set_volume",
            "lock",
            "power_off",
            "close_app",
            "open_app",
        ] {
            assert!(locked.closes(find(name).unwrap()).is_some(), "{name}");
        }
        // an unlocked screen is not affected
        assert!(Limits::default().closes(find("wifi").unwrap()).is_none());
    }

    #[test]
    fn seeing_the_screen_needs_the_persons_switch() {
        let tool = find("see_screen").unwrap();
        assert_eq!(tool.area, Area::Screen);
        assert_eq!(tool.risk, Risk::Reads);
        // Off (the default): closed even though looking is otherwise allowed.
        let limits = Limits::default();
        assert!(limits.closes(tool).is_some());
        // On: open, also while the helper may only look.
        let on = Limits {
            see_screen: true,
            read_only: true,
            ..Limits::default()
        };
        assert!(on.closes(tool).is_none());
        // A switched-off Screen area closes it still.
        let off = Limits {
            off: vec![Area::Screen],
            see_screen: true,
            ..Limits::default()
        };
        assert!(off.closes(tool).is_some());
    }
}
