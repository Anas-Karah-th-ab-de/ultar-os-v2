//! Contract of the local helper (`ultar-agentd`): the person asks in words, the helper
//! answers and does what was asked through the system bus – with its own narrow rights,
//! and only after the person confirmed anything dangerous in the system dialog.
//!
//! Where the helper thinks depends on the device: with its own model, with the model of a
//! paired device that lends it, or – when neither exists – with the smallest help: fixed
//! commands it understands without a model.

use crate::message::{Call, Message, Text, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "agent";

/// Longest request in bytes.
pub const MAX_REQUEST: usize = 4096;

/// Where the helper thinks.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub enum Brain {
    /// No model anywhere: fixed commands only.
    #[default]
    Minimal,
    /// The model of this device.
    Local,
    /// The model of a paired device (its id).
    Mesh(String),
}

/// `agent.state` – what the helper can do right now (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct AgentState {
    /// Where it thinks.
    pub brain: Brain,
    /// Name of the lending device while [`Brain::Mesh`] (data).
    pub lender: String,
}

/// `agent.ask` – one request of the person.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Ask {
    /// What the person wrote or said.
    pub text: String,
}

/// How a step went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepOutcome {
    /// Done.
    Done,
    /// The system refused or failed.
    Failed,
    /// The person did not confirm it.
    NotConfirmed,
    /// The helper may not do this.
    NotAllowed,
}

/// One thing the helper did for a request.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Step {
    /// The tool (stable name, e.g. `open_app`).
    pub tool: String,
    /// How it went.
    pub outcome: StepOutcome,
}

/// Answer to [`Ask`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Answer {
    /// What the helper says: a text of its catalogue, or the model's words as its one
    /// verbatim argument.
    pub text: Text,
    /// Where it thought.
    pub brain: Brain,
    /// What it did, in order.
    pub steps: Vec<Step>,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(AgentState, Ask, Step, Answer);

impl Topic for AgentState {
    const TOPIC: &'static str = "agent.state";
    const RETAINED: bool = true;
}

impl Call for Ask {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "ask";
    type Reply = Answer;
}

/// The helper's texts (keys `agent.*`, in [`Answer::text`]) by language tag, for the
/// surface that shows its answers and for the helper itself.
pub const LOCALES: [(&str, &str); 3] = [
    ("ar", include_str!("../../locales/agent/ar.ftl")),
    ("en", include_str!("../../locales/agent/en.ftl")),
    ("de", include_str!("../../locales/agent/de.ftl")),
];
