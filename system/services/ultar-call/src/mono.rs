//! `accessibility.mono_audio`: both channels in every speaker. The Ultar kernel sums them in
//! the sound card's output path for every writer at once (browser, calls); its switch
//! `/dev/ultar/pcm-mono` takes the right `sound`, which this service already holds for the
//! streams. The service follows the setting (at start and on `settings.changed`) and writes
//! the wish to the switch; without the node (no sound card, a Linux kernel) nothing happens
//! and it is tried again later.

use std::io::Write;
use std::path::PathBuf;
use ultar_bus::messages::settings::SettingValue;

/// The setting key.
pub const KEY: &str = "accessibility.mono_audio";

/// The wish a settings value means; `None` for a value of the wrong type (it changes nothing).
pub fn of(value: &SettingValue) -> Option<bool> {
    match value {
        SettingValue::Flag(on) => Some(*on),
        _ => None,
    }
}

/// The kernel's mono switch: what the person wants and what is on the card.
#[derive(Debug)]
pub struct Switch {
    node: PathBuf,
    wanted: bool,
    applied: Option<bool>,
}

impl Switch {
    /// The switch at `node` (`seat_nodes::pcm_mono_node`); off until the setting is read.
    pub fn new(node: PathBuf) -> Self {
        Self {
            node,
            wanted: false,
            applied: None,
        }
    }

    /// The person's wish; written at once.
    pub fn want(&mut self, on: bool) {
        self.wanted = on;
        self.apply();
    }

    /// Writes the wish when the card does not have it yet (`1` on, `0` off).
    pub fn apply(&mut self) {
        if self.applied == Some(self.wanted) {
            return;
        }
        let byte: &[u8] = if self.wanted { b"1" } else { b"0" };
        let written = std::fs::OpenOptions::new()
            .write(true)
            .open(&self.node)
            .and_then(|mut f| f.write_all(byte));
        if written.is_ok() {
            self.applied = Some(self.wanted);
        }
    }

    /// What was last written (`None`: nothing yet).
    pub fn applied(&self) -> Option<bool> {
        self.applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wish_reaches_the_switch_and_a_missing_node_is_tried_again() {
        assert_eq!(of(&SettingValue::Flag(true)), Some(true));
        assert_eq!(of(&SettingValue::Text("1".into())), None);
        let dir = std::env::temp_dir().join(format!("call-mono-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let node = dir.join("pcm-mono");
        let mut s = Switch::new(node.clone());
        s.want(true);
        assert_eq!(s.applied(), None, "no node yet");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&node, b"").unwrap();
        s.apply();
        assert_eq!(std::fs::read(&node).unwrap(), b"1");
        s.want(false);
        assert_eq!(std::fs::read(&node).unwrap(), b"0");
        assert_eq!(s.applied(), Some(false));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
