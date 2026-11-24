//! What the helper can do: a short list of tools, each one a call on the system bus.
//! A tool that is not in this list does not exist for the helper, whatever a model says.

use std::collections::BTreeMap;
use ultar_bus::messages::agent::StepOutcome;

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
    /// How much it can change.
    pub risk: Risk,
    /// Its arguments by name.
    pub args: &'static [&'static str],
    /// What it does, for the model.
    pub about: &'static str,
}

/// Every tool of the helper.
pub const TOOLS: [Tool; 18] = [
    tool("list_apps", Risk::Reads, &[], "lists the ids of the installed apps"),
    tool("open_app", Risk::Changes, &["app"], "opens the app with this id"),
    tool("set_volume", Risk::Changes, &["percent"], "sets the sound volume, 0 to 100"),
    tool("mute", Risk::Changes, &["on"], "mutes (true) or unmutes (false) the sound"),
    tool("wifi", Risk::Changes, &["on"], "switches Wi-Fi on (true) or off (false)"),
    tool("bluetooth", Risk::Changes, &["on"], "switches Bluetooth on (true) or off (false)"),
    tool("airplane", Risk::Changes, &["on"], "switches airplane mode on (true) or off (false)"),
    tool("media", Risk::Changes, &["button"], "presses a media button: play_pause, next or previous"),
    tool("screenshot", Risk::Changes, &[], "takes a screenshot and says where it was saved"),
    tool("lock", Risk::Changes, &[], "locks the screen"),
    tool("help", Risk::Reads, &["topic"], "explains this system; topics: pairing, continue, files, model, wifi, bluetooth, updates, helper"),
    tool("devices", Risk::Reads, &[], "lists the person's paired devices and whether they are connected"),
    tool("continue_on", Risk::Changes, &["device"], "offers what is open here to another paired device (device id, or empty for the only one); that device asks its person"),
    tool("send_file", Risk::Changes, &["device", "path"], "offers a file to a paired device; that device asks its person"),
    tool("ask_device", Risk::Changes, &["device", "request"], "asks the helper of a paired device (device id) to do the request there; works only when that device allows control"),
    tool("suspend", Risk::Dangerous, &[], "puts this device to sleep"),
    tool("restart", Risk::Dangerous, &[], "restarts this device"),
    tool("power_off", Risk::Dangerous, &[], "switches this device off"),
];

const fn tool(
    name: &'static str,
    risk: Risk,
    args: &'static [&'static str],
    about: &'static str,
) -> Tool {
    Tool {
        name,
        risk,
        args,
        about,
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
        assert_eq!(dangerous, ["suspend", "restart", "power_off"]);
        assert!(find("format_disk").is_none());
        assert_eq!(Call::new("wifi", &[("on", "true")]).arg("on"), "true");
        assert_eq!(Call::new("wifi", &[]).arg("on"), "");
    }
}
