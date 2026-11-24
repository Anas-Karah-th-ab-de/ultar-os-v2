//! Contract of the mesh for acting on another device: the helper here asks the helper of
//! a paired device to do something there. That device decides per device whether it takes
//! such requests ([`crate::messages::mesh::Right::Control`], off after pairing), acts with
//! its own helper's narrow rights, and its person confirms anything dangerous there.

use crate::message::{Call, Message};
use crate::messages::agent::Step;
use crate::messages::mesh::SERVICE;
use serde::{Deserialize, Serialize};

/// Most steps reported back from the other device.
pub const MAX_STEPS: usize = 8;

/// `mesh.act` – ask the helper of a paired device to do something there.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Act {
    /// The device.
    pub device: String,
    /// The request, in the person's words.
    pub text: String,
}

/// Answer to [`Act`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Acted {
    /// The device took the request (it is reachable and allows it).
    pub ok: bool,
    /// What its helper did.
    pub steps: Vec<Step>,
    /// The device.
    pub by: String,
}

impl Message for Act {
    const VERSION: u16 = 1;
}

impl Message for Acted {
    const VERSION: u16 = 1;
}

impl Call for Act {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "act";
    type Reply = Acted;
}
