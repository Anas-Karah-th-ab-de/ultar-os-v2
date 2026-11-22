//! Contract of the shell: which experience is currently shown.

use crate::message::{Message, Topic};
use serde::{Deserialize, Serialize};

/// The two experiences that run on the one compositor.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// Touch-first single-window experience.
    #[default]
    Mobile,
    /// Windowed experience with keyboard and pointer.
    Desktop,
}

/// `shell.mode` – the active experience (retained, so applications that
/// start later lay themselves out correctly right away).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ShellMode {
    /// Experience now in effect.
    pub mode: Mode,
}

impl Message for ShellMode {
    const VERSION: u16 = 1;
}

impl Topic for ShellMode {
    const TOPIC: &'static str = "shell.mode";
    const RETAINED: bool = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn shell_mode_round_trips() {
        for mode in [Mode::Mobile, Mode::Desktop] {
            let message = ShellMode { mode };
            assert_eq!(
                decode::<ShellMode>(&encode(&message).unwrap()).unwrap(),
                message
            );
        }
        assert_eq!(ShellMode::default().mode, Mode::Mobile);
    }
}
