//! Contract of the removable-drive service: mounted drives of the person in front and
//! ejecting them.

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "volumes";

/// One mounted drive.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Mounted {
    /// Id (kernel name of the partition, e.g. `sdb1`).
    pub id: String,
    /// Label, or the id when the drive has none (data).
    pub name: String,
    /// Where the files are (`file:///…`).
    pub uri: String,
}

/// `volumes.state` – the drives mounted for the person in front (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct VolumesState {
    /// Mounted drives.
    pub mounted: Vec<Mounted>,
}

/// `volumes.eject` – unmount a drive so it can be pulled out.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Eject {
    /// Its id.
    pub id: String,
}

/// How an eject went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum EjectOutcome {
    /// Unmounted: it can be pulled out.
    Done,
    /// Files on it are still open.
    Busy,
    /// No such mounted drive.
    Unknown,
    /// The kernel refused.
    Failed,
}

/// Answer to [`Eject`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct EjectReply {
    /// The result.
    pub outcome: EjectOutcome,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(Mounted, VolumesState, Eject, EjectReply);

impl Topic for VolumesState {
    const TOPIC: &'static str = "volumes.state";
    const RETAINED: bool = true;
}

impl Call for Eject {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "eject";
    type Reply = EjectReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn messages_round_trip() {
        let s = VolumesState {
            mounted: vec![Mounted {
                id: "sdb1".into(),
                name: "STICK".into(),
                uri: "file:///run/ultar/media/amal/STICK".into(),
            }],
        };
        assert_eq!(decode::<VolumesState>(&encode(&s).unwrap()).unwrap(), s);
        let r = EjectReply {
            outcome: EjectOutcome::Busy,
        };
        assert_eq!(decode::<EjectReply>(&encode(&r).unwrap()).unwrap(), r);
    }
}
