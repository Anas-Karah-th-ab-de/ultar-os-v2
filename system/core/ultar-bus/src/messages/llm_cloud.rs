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

/// Most models listed in the state.
pub const MAX_MODELS: usize = 40;
/// Longest address of a provider (bytes).
pub const MAX_ENDPOINT: usize = 200;

/// A provider this service can ask.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ProviderInfo {
    /// Stable id (`claude`, `compatible`).
    pub id: String,
    /// Its name, as the person reads it (data).
    pub name: String,
    /// The person must give its address (a server of their own choice).
    pub needs_endpoint: bool,
}

/// A model of the provider.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ModelInfo {
    /// The id asked for.
    pub id: String,
    /// Its name, as the person reads it (data; the id when the provider gives none).
    pub name: String,
}

/// Topic `llmcloud.state` (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct CloudState {
    /// The person entered a key.
    pub configured: bool,
    /// The provider accepted the key when it was last asked (`false`: not asked yet, or
    /// it could not be reached).
    pub verified: bool,
    /// Id of the chosen provider.
    pub provider: String,
    /// Address of the chosen provider when the person gave one.
    pub endpoint: String,
    /// The model asked for.
    pub model: String,
    /// The providers to choose from.
    pub providers: Vec<ProviderInfo>,
    /// The models the provider offered for the key (empty until it was asked).
    pub models: Vec<ModelInfo>,
}

/// `llmcloud.configure` – choose provider, address and model. Another provider or address
/// forgets the key: a key is never sent to a server it was not entered for.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Configure {
    /// Id of the provider.
    pub provider: String,
    /// Its address (only for a provider that needs one).
    pub endpoint: String,
    /// The model (empty: the provider's small default).
    pub model: String,
}

/// `llmcloud.set_key` – store the person's key (replaces an earlier one). The provider is
/// asked once with it: a key it refuses is not stored; when it cannot be reached the key
/// is stored unverified.
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

/// Why nothing was saved.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Refusal {
    /// Saved.
    #[default]
    None,
    /// Not a key, not an address, or an unknown provider or model.
    Invalid,
    /// The provider refused the key.
    KeyRefused,
    /// The device could not store it.
    Storage,
}

/// Answer to [`SetKey`], [`ClearKey`] and [`Configure`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Saved {
    /// Done.
    pub ok: bool,
    /// Why not.
    pub refusal: Refusal,
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

impl CloudState {
    /// The chosen provider.
    pub fn chosen(&self) -> Option<&ProviderInfo> {
        self.providers.iter().find(|p| p.id == self.provider)
    }

    /// The chosen provider's name, as the person reads it.
    pub fn provider_name(&self) -> &str {
        self.chosen()
            .map_or(self.provider.as_str(), |p| p.name.as_str())
    }
}

impl Saved {
    /// "Done".
    pub fn default_ok() -> Self {
        Self {
            ok: true,
            refusal: Refusal::None,
        }
    }
}

impl Message for CloudState {
    const VERSION: u16 = 1;
}
impl Message for SetKey {
    const VERSION: u16 = 1;
}
impl Message for Configure {
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

impl Call for Configure {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "configure";
    type Reply = Saved;
}

impl Call for CloudComplete {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "complete";
    type Reply = Completion;
}
