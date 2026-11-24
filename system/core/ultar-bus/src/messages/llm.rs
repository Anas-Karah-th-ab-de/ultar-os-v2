//! Contract of the language-model service (`llm`): what the local model can do and one
//! call to let it think. The service loads the model on demand and unloads it under memory
//! pressure, on battery or when hot; how it computes (processor, graphics chip, neural
//! chip) is its own matter and no part of this contract.

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "llm";

/// Longest system text plus prompt of one call, in bytes.
pub const MAX_PROMPT: usize = 40 * 1024;
/// Longest answer of one call, in bytes.
pub const MAX_ANSWER: usize = 16 * 1024;

/// How much a model can be trusted with (ordered: a larger class thinks better).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum ModelClass {
    /// Short answers and simple commands (up to about 2 billion parameters).
    #[default]
    Small,
    /// Several steps in a row (up to about 8 billion parameters).
    Medium,
    /// Long tasks (above).
    Large,
}

/// `llm.state` – the model of this device (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct LlmState {
    /// A model is installed and the device can run it now.
    pub ready: bool,
    /// Its name (data).
    pub model: String,
    /// Its class.
    pub class: ModelClass,
}

/// `llm.complete` – let the model answer one prompt.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Complete {
    /// What the model is and may do (the caller's rules).
    pub system: String,
    /// What it is asked.
    pub prompt: String,
    /// Longest answer in tokens.
    pub max_tokens: u32,
}

/// Answer to [`Complete`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Completion {
    /// The model answered.
    pub ok: bool,
    /// Its answer (empty when it did not).
    pub text: String,
}

impl Message for LlmState {
    const VERSION: u16 = 1;
}
impl Message for Complete {
    const VERSION: u16 = 1;
}
impl Message for Completion {
    const VERSION: u16 = 1;
}

impl Topic for LlmState {
    const TOPIC: &'static str = "llm.state";
    const RETAINED: bool = true;
}

impl Call for Complete {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "complete";
    type Reply = Completion;
}
