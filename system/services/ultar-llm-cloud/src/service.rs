//! The bus side: `llmcloud.state` (retained), the choice of provider and model, the key,
//! and one answer per call.

use crate::config::{Config, ConfigFile};
use crate::key::{clean, KeyStore};
use crate::provider::{endpoint_ok, pick_model, Ask, Failure, Provider, Target};
use std::sync::Arc;
use ultar_bus::messages::llm::{Completion, MAX_PROMPT};
use ultar_bus::messages::llm_cloud::{
    ClearKey, CloudComplete, CloudState, Configure, ModelInfo, ProviderInfo, Refusal, Saved,
    SetKey, SERVICE,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_svckit::{Bus, Service};

type Checked = Result<Vec<ModelInfo>, Failure>;

/// The cloud model on the bus.
pub struct LlmCloud {
    keys: KeyStore,
    file: ConfigFile,
    config: Config,
    providers: Vec<Arc<dyn Provider>>,
    verified: bool,
    models: Vec<ModelInfo>,
    next: u64,
    waiting: Vec<(u64, IncomingRequest)>,
}

fn saved(refusal: Refusal) -> Saved {
    Saved {
        ok: refusal == Refusal::None,
        refusal,
    }
}

impl LlmCloud {
    /// The service with its key store, its stored choice and the providers it can ask
    /// (the first one is the default).
    pub fn new(keys: KeyStore, file: ConfigFile, providers: Vec<Arc<dyn Provider>>) -> Self {
        let config = file.read();
        Self {
            keys,
            file,
            config,
            providers,
            verified: false,
            models: Vec::new(),
            next: 0,
            waiting: Vec::new(),
        }
    }

    fn provider(&self) -> Option<Arc<dyn Provider>> {
        let chosen = self
            .providers
            .iter()
            .find(|p| p.id() == self.config.provider);
        chosen.or(self.providers.first()).cloned()
    }

    fn target(&self, provider: &dyn Provider) -> Target {
        let endpoint = provider.endpoint().unwrap_or(&self.config.endpoint);
        Target {
            endpoint: endpoint.to_owned(),
            model: pick_model(&self.models, &self.config.model, provider.small()),
        }
    }

    fn publish(&self, bus: &Bus<'_, Self>) {
        let provider = self.provider();
        let info = |p: &Arc<dyn Provider>| ProviderInfo {
            id: p.id().to_owned(),
            name: p.name().to_owned(),
            needs_endpoint: p.endpoint().is_none(),
        };
        bus.retain(&CloudState {
            configured: self.keys.read().is_some(),
            verified: self.verified,
            provider: provider.as_ref().map_or("", |p| p.id()).to_owned(),
            endpoint: self.config.endpoint.clone(),
            model: provider.map_or_else(String::new, |p| self.target(&*p).model),
            providers: self.providers.iter().map(info).collect(),
            models: self.models.clone(),
        });
    }

    fn take(&mut self, id: u64) -> Option<IncomingRequest> {
        let at = self.waiting.iter().position(|(i, _)| *i == id)?;
        Some(self.waiting.remove(at).1)
    }

    /// Asks the provider for the models of `key` in the background; `then` gets the
    /// outcome. `false`: nothing was started.
    fn check(
        &mut self,
        key: String,
        bus: &Bus<'_, Self>,
        then: impl FnOnce(&mut Self, Checked, &str, &Bus<'_, Self>) + Send + 'static,
    ) -> bool {
        let Some(provider) = self.provider() else {
            return false;
        };
        let target = self.target(&*provider);
        if !endpoint_ok(&target.endpoint) {
            return false;
        }
        bus.later(move |_| {
            let checked = provider.models(&target, &key);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                then(service, checked, &key, bus);
            })
        })
    }

    fn checked(&mut self, checked: &Checked) {
        self.verified = checked.is_ok();
        if let Ok(models) = checked {
            self.models.clone_from(models);
        }
    }

    /// A key the provider refuses is not stored; one it accepts, or cannot be asked
    /// about now, is.
    fn set_key(&mut self, request: IncomingRequest, key: &str, bus: &Bus<'_, Self>) {
        let Some(key) = clean(key).map(str::to_owned) else {
            let _ = request.reply(&saved(Refusal::Invalid));
            return;
        };
        self.next += 1;
        let id = self.next;
        let started = self.check(key, bus, move |service, checked, key, bus| {
            let refusal = match &checked {
                Err(Failure::Key) => Refusal::KeyRefused,
                _ if service.keys.write(key) => Refusal::None,
                _ => Refusal::Storage,
            };
            if refusal == Refusal::None {
                service.checked(&checked);
            }
            if let Some(request) = service.take(id) {
                let _ = request.reply(&saved(refusal));
            }
            service.publish(bus);
        });
        if started {
            self.waiting.push((id, request));
        } else {
            // no provider chosen or no address yet
            let _ = request.reply(&saved(Refusal::Invalid));
        }
    }

    fn configure(&mut self, want: Configure) -> Refusal {
        let Some(provider) = self.providers.iter().find(|p| p.id() == want.provider) else {
            return Refusal::Invalid;
        };
        let endpoint = if provider.endpoint().is_some() {
            String::new()
        } else {
            want.endpoint.trim().to_owned()
        };
        let model = want.model.trim();
        let plain = model.len() <= 128 && model.bytes().all(|b| b.is_ascii_graphic());
        if !plain || (provider.endpoint().is_none() && !endpoint_ok(&endpoint)) {
            return Refusal::Invalid;
        }
        let same = self.provider().is_some_and(|p| p.id() == want.provider)
            && endpoint == self.config.endpoint;
        let config = Config {
            provider: want.provider,
            endpoint,
            model: model.to_owned(),
        };
        if !self.file.write(&config) {
            return Refusal::Storage;
        }
        self.config = config;
        if !same {
            // a key is never sent to a server it was not entered for
            self.keys.clear();
            self.models.clear();
            self.verified = false;
        }
        Refusal::None
    }

    fn complete(&mut self, request: IncomingRequest, ask: CloudComplete, bus: &Bus<'_, Self>) {
        // Without the person's key nothing leaves the device.
        let (Some(key), Some(provider)) = (self.keys.read(), self.provider()) else {
            let _ = request.reply(&Completion::default());
            return;
        };
        let target = self.target(&*provider);
        self.next += 1;
        let id = self.next;
        let started = bus.later(move |_| {
            let asked = Ask {
                system: &ask.system,
                prompt: &ask.prompt,
                tokens: ask.max_tokens,
            };
            let said = provider.complete(&target, &key, &asked);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                if said == Err(Failure::Key) && service.verified {
                    service.verified = false;
                    service.publish(bus);
                }
                if let Some(request) = service.take(id) {
                    let text = said.ok();
                    let _ = request.reply(&Completion {
                        ok: text.is_some(),
                        text: text.unwrap_or_default(),
                    });
                }
            })
        });
        if started {
            self.waiting.push((id, request));
        } else {
            let _ = request.fail(ErrorCode::ServiceBusy);
        }
    }
}

impl Service for LlmCloud {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        self.publish(bus);
        // a stored key: ask once which models it may use (no answer changes nothing)
        if let Some(key) = self.keys.read() {
            self.check(key, bus, |service, checked, _, bus| {
                service.checked(&checked);
                service.publish(bus);
            });
        }
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        match request.method() {
            SetKey::METHOD => match request.decode::<SetKey>() {
                Ok(set) => self.set_key(request, &set.key, bus),
                Err(_) => {
                    let _ = request.fail(ErrorCode::BadPayload);
                }
            },
            ClearKey::METHOD => {
                let done = self.keys.clear();
                self.verified = false;
                self.models.clear();
                let refusal = if done {
                    Refusal::None
                } else {
                    Refusal::Storage
                };
                let _ = request.reply(&saved(refusal));
                self.publish(bus);
            }
            Configure::METHOD => match request.decode::<Configure>() {
                Ok(want) => {
                    let refusal = self.configure(want);
                    let _ = request.reply(&saved(refusal));
                    self.publish(bus);
                }
                Err(_) => {
                    let _ = request.fail(ErrorCode::BadPayload);
                }
            },
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
