//! Contract of the cloud model (`llmcloud`): a language model of a provider in the
//! internet, as the helper's **last** way to think – used only when this device has no
//! model of its own and no paired device lends one, and only after the person entered a
//! key of their own. What is sent leaves the device: the request in the person's words and
//! the list of the helper's tools.
//!
//! The key is written once and never given out again: no call returns it.

use crate::message::{Call, Message, Topic};
use crate::messages::llm::Completion;
use serde::{Deserialize, Serialize};

/// Service name on the bus.
pub const SERVICE: &str = "llmcloud";
/// Longest key accepted (bytes).
pub const MAX_KEY: usize = 512;

/// Topic `llmcloud.state` (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct CloudState {
    /// The person entered a key.
    pub configured: bool,
    /// The provider's name (data, e.g. `Claude`).
    pub provider: String,
    /// The model asked for.
    pub model: String,
}

/// `llmcloud.set_key` – store the person's key (replaces an earlier one).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SetKey {
    /// The key.
    pub key: String,
}

// Never printed: a key must not reach a log through a debug line.
impl std::fmt::Debug for SetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SetKey")
    }
}

/// `llmcloud.clear_key` – forget the key.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ClearKey {}

/// Answer to [`SetKey`] and [`ClearKey`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Saved {
    /// Done.
    pub ok: bool,
}

/// `llmcloud.complete` – let the cloud model answer once.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct CloudComplete {
    /// The rules for the model.
    pub system: String,
    /// The request.
    pub prompt: String,
    /// Longest answer in tokens.
    pub max_tokens: u32,
}

impl Message for CloudState {
    const VERSION: u16 = 1;
}
impl Message for SetKey {
    const VERSION: u16 = 1;
}
impl Message for ClearKey {
    const VERSION: u16 = 1;
}
impl Message for Saved {
    const VERSION: u16 = 1;
}
impl Message for CloudComplete {
    const VERSION: u16 = 1;
}

impl Topic for CloudState {
    const TOPIC: &'static str = "llmcloud.state";
    const RETAINED: bool = true;
}

impl Call for SetKey {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "set_key";
    type Reply = Saved;
}

impl Call for ClearKey {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "clear_key";
    type Reply = Saved;
}

impl Call for CloudComplete {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "complete";
    type Reply = Completion;
}
