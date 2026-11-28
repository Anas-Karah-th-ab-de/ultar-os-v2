//! Contract of the file that another device sent and that now lies in the inbox
//! (`mesh.received`): the shell tells the person ("beelink sent report.pdf") and offers to
//! open the file again, which the mesh does once by itself when it arrives.

use crate::message::{Message, Topic};
use serde::{Deserialize, Serialize};

/// `mesh.received` – a file arrived from a paired device (the person accepted it).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct FileReceived {
    /// The offer it came with (see [`crate::messages::mesh::HandoffProgress`]).
    pub offer: u64,
    /// Name of the device that sent it.
    pub from_name: String,
    /// The file's name.
    pub title: String,
    /// Where it lies (`file://` and an absolute path in the inbox).
    pub uri: String,
}

impl Message for FileReceived {
    const VERSION: u16 = 1;
}

impl Topic for FileReceived {
    const TOPIC: &'static str = "mesh.received";
    const RETAINED: bool = false;
}
