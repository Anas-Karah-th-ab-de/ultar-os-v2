//! State of the page Settings › Mesh.

use ultar_bus::messages::mesh::{MeshState, PairCode, Peer, Right};
use ultar_bus::Text;

/// The rights of a paired device in the order of the board *MeshDevice*.
pub const RIGHTS: [Right; 8] = [
    Right::Handoff,
    Right::Files,
    Right::Clipboard,
    Right::View,
    Right::Control,
    Right::Audio,
    Right::Compute,
    Right::Model,
];

/// What the page knows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MeshView {
    /// The mesh (`None`: the service does not answer).
    pub state: Option<MeshState>,
    /// Why it does not.
    pub error: Option<Text>,
    /// The paired device whose page is open.
    pub device: Option<String>,
    /// The code this device shows.
    pub code: Option<PairCode>,
    /// The nearby device whose code is being typed.
    pub join: Option<String>,
}

impl MeshView {
    fn peers(&self, paired: bool) -> Vec<&Peer> {
        let peers = self.state.iter().flat_map(|s| s.peers.iter());
        peers
            .filter(|p| p.paired == paired && (paired || p.reachable))
            .collect()
    }

    /// The paired devices in the order shown.
    pub fn paired(&self) -> Vec<&Peer> {
        self.peers(true)
    }

    /// The devices in reach that are not paired.
    pub fn near(&self) -> Vec<&Peer> {
        self.peers(false)
    }

    /// The device whose page is open, while it is still paired.
    pub fn open(&self) -> Option<&Peer> {
        let id = self.device.as_deref()?;
        self.paired().into_iter().find(|p| p.id == id)
    }
}
