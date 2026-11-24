//! The question the mesh asks the person through the system dialog: take what another
//! device offers? (Board MeshIncoming, docs/ui/mesh.md §2.4.)

use crate::message::{Call, Message};
use crate::messages::mesh::{DeviceKind, Stage};
use serde::{Deserialize, Serialize};

/// `sysdialog.handoff` – the mesh asks the person on this device (system dialog, trusted
/// frame) whether to take what another device offers. Called by `system.mesh` only;
/// unanswered within `sysdialog::ANSWER_WITHIN_MS` the dialog answers `accept: false`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct AskHandoff {
    /// Name of the offering device (data: shown as text, never interpreted).
    pub from_name: String,
    /// What it is.
    pub from_kind: DeviceKind,
    /// Title of the activity or the file's name (data).
    pub title: String,
    /// App id (empty for a plain file).
    pub app: String,
    /// Size of the file that would arrive, in bytes.
    pub size: Option<u64>,
    /// How it would arrive.
    pub stage: Stage,
}

/// Answer to [`AskHandoff`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoffDecision {
    /// The person accepted.
    pub accept: bool,
}

impl Call for AskHandoff {
    const SERVICE: &'static str = crate::messages::sysdialog::SERVICE;
    const METHOD: &'static str = "handoff";
    type Reply = HandoffDecision;
}

impl Message for AskHandoff {
    const VERSION: u16 = 1;
}
impl Message for HandoffDecision {
    const VERSION: u16 = 1;
}
