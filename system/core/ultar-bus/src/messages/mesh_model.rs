//! Contract of the mesh for lending the language model: a device without a model (or with
//! a small one) lets a paired device think for it. The lending device decides per device
//! whether it lends ([`crate::messages::mesh::Right::Model`], off after pairing).

use crate::message::{Call, Message};
use crate::messages::llm::ModelClass;
use crate::messages::mesh::SERVICE;
use serde::{Deserialize, Serialize};

/// `mesh.think` – let the model of a paired device answer one prompt.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Think {
    /// The device (`None`: the reachable paired device with the best model).
    pub device: Option<String>,
    /// What the model is and may do.
    pub system: String,
    /// What it is asked.
    pub prompt: String,
    /// Longest answer in tokens.
    pub max_tokens: u32,
}

/// Answer to [`Think`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Thought {
    /// A device answered.
    pub ok: bool,
    /// The answer.
    pub text: String,
    /// The device that answered.
    pub by: String,
}

/// `mesh.models` – the paired devices that lend their model right now.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Models;

/// One device that lends its model.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Lender {
    /// Its id.
    pub device: String,
    /// Its name (data).
    pub name: String,
    /// The class of its model.
    pub class: ModelClass,
}

/// Answer to [`Models`]: best model first.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ModelList {
    /// The devices.
    pub lenders: Vec<Lender>,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(Think, Thought, Models, Lender, ModelList);

impl Call for Think {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "think";
    type Reply = Thought;
}

impl Call for Models {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "models";
    type Reply = ModelList;
}
