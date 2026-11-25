//! The bus side: `llmcloud.state` (retained), the key, and one answer per call.

use crate::key::KeyStore;
use crate::provider::Provider;
use std::sync::Arc;
use ultar_bus::messages::llm::{Completion, MAX_PROMPT};
use ultar_bus::messages::llm_cloud::{ClearKey, CloudComplete, CloudState, Saved, SetKey, SERVICE};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_svckit::{Bus, Service};

/// The cloud model on the bus.
pub struct LlmCloud {
    keys: KeyStore,
    provider: Arc<dyn Provider>,
    next: u64,
    waiting: Vec<(u64, IncomingRequest)>,
}

impl LlmCloud {
    /// The service with its key store and provider.
    pub fn new(keys: KeyStore, provider: Arc<dyn Provider>) -> Self {
        Self {
            keys,
            provider,
            next: 0,
            waiting: Vec::new(),
        }
    }

    fn publish(&self, bus: &Bus<'_, Self>) {
        bus.retain(&CloudState {
            configured: self.keys.read().is_some(),
            provider: self.provider.name().to_owned(),
            model: self.provider.model().to_owned(),
        });
    }

    fn answer(&mut self, id: u64, text: Option<String>) {
        if let Some(at) = self.waiting.iter().position(|(i, _)| *i == id) {
            let (_, request) = self.waiting.remove(at);
            let reply = Completion {
                ok: text.is_some(),
                text: text.unwrap_or_default(),
            };
            let _ = request.reply(&reply);
        }
    }

    fn complete(&mut self, request: IncomingRequest, ask: CloudComplete, bus: &Bus<'_, Self>) {
        // Without the person's key nothing leaves the device.
        let Some(key) = self.keys.read() else {
            let _ = request.reply(&Completion::default());
            return;
        };
        self.next += 1;
        let id = self.next;
        let provider = self.provider.clone();
        let started = bus.later(move |_| {
            let said = provider.complete(&key, &ask.system, &ask.prompt, ask.max_tokens);
            Box::new(move |service: &mut Self, _: &Bus<'_, Self>| service.answer(id, said.ok()))
        });
        if started {
            self.waiting.push((id, request));
        } else {
            let _ = request.fail(ErrorCode::ServiceBusy);
        }
    }

    fn saved(&mut self, request: IncomingRequest, ok: bool, bus: &Bus<'_, Self>) {
        let _ = request.reply(&Saved { ok });
        self.publish(bus);
    }
}

impl Service for LlmCloud {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        self.publish(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        match request.method() {
            SetKey::METHOD => match request.decode::<SetKey>() {
                Ok(set) => {
                    let ok = self.keys.write(&set.key);
                    self.saved(request, ok, bus);
                }
                Err(_) => {
                    let _ = request.fail(ErrorCode::BadPayload);
                }
            },
            ClearKey::METHOD => {
                let ok = self.keys.clear();
                self.saved(request, ok, bus);
            }
            CloudComplete::METHOD => match request.decode::<CloudComplete>() {
                Ok(ask) if ask.system.len() + ask.prompt.len() <= MAX_PROMPT => {
                    self.complete(request, ask, bus);
                }
                _ => {
                    let _ = request.fail(ErrorCode::BadPayload);
                }
            },
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
            }
        }
    }
}
