//! The bus side: `llm.state` (retained, looked up regularly) and `llm.complete`; and the
//! model service runs exactly while the person has the assistant switched on
//! (`assistant.enabled` → `init.switch`). The model size the person chose
//! (`assistant.model`) decides which installed model answers and what the state reports.

use crate::backend::Backend;
use crate::choice::{self, MODEL_KEY};
use crate::place::{self, PLACE_KEY};
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::init::{Switch, ERROR_UNKNOWN_SERVICE};
use ultar_bus::messages::llm::{Complete, Completion, LlmState, ModelClass, MAX_PROMPT, SERVICE};
use ultar_bus::messages::settings::{Changed, Get, SettingValue};
use ultar_bus::Topic;
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_svckit::{Bus, Service};

/// How often the model service is asked what it can do.
pub const TICK: Duration = Duration::from_secs(10);
/// How long the model may take for one answer.
pub const THINK_TIME: Duration = Duration::from_secs(110);

/// The setting the model service follows: the person switched the assistant on.
pub const ENABLED_KEY: &str = "assistant.enabled";
/// The model service's name in the service manager.
pub const MODEL_SERVICE: &str = "llm";
const CALL_TIME: Duration = Duration::from_secs(5);

/// The language model on the bus.
pub struct LlmBus {
    backend: Backend,
    last: Option<LlmState>,
    next: u64,
    waiting: Vec<(u64, IncomingRequest)>,
    /// What the service manager was last told (`None`: nothing yet).
    switched: Option<bool>,
    /// The model size the person chose (`assistant.model`, default small).
    choice: ModelClass,
    /// The model size was read from the settings service.
    choice_read: bool,
    /// Where the model place is passed to the model service (`None`: not passed on).
    place_file: Option<PathBuf>,
    /// The model place was read from the settings service.
    place_read: bool,
}

impl LlmBus {
    /// The bus face of the model service behind `backend`.
    pub fn new(backend: Backend) -> Self {
        Self {
            backend,
            last: None,
            next: 0,
            waiting: Vec::new(),
            switched: None,
            choice: ModelClass::default(),
            choice_read: false,
            place_file: None,
            place_read: false,
        }
    }

    /// Also passes the person's model place (`assistant.model_place`) on in `file`.
    pub fn with_place_file(mut self, file: PathBuf) -> Self {
        self.place_file = Some(file);
        self
    }

    /// Writes the place for the model service; a wrong type changes nothing.
    fn set_place(&mut self, value: &SettingValue) {
        let (Some(file), Some(text)) = (&self.place_file, place::of(value)) else {
            return;
        };
        // not written: the model service keeps the last place; tried again with the next change
        let _ = place::publish(file, &text);
    }

    /// Reads the model place once (at start, and again until the settings service answered).
    fn read_place(&mut self, bus: &Bus<'_, Self>) {
        if self.place_read || self.place_file.is_none() {
            return;
        }
        let _ = bus.later(move |client| {
            let key = PLACE_KEY.to_owned();
            let value = client
                .call(&Get { key }, CALL_TIME)
                .ok()
                .and_then(|r| r.value);
            Box::new(move |service: &mut Self, _: &Bus<'_, Self>| {
                let Some(value) = value else { return };
                service.place_read = true;
                service.set_place(&value);
            })
        });
    }

    fn look(&mut self, bus: &Bus<'_, Self>) {
        let backend = self.backend.clone();
        let choice = self.choice;
        let _ = bus.later(move |_| {
            let state = backend.state_with(Some(choice));
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                if service.last.as_ref() != Some(&state) {
                    bus.retain(&state);
                    service.last = Some(state);
                }
            })
        });
    }

    /// The model service runs exactly while the person has the assistant switched on.
    fn switch(&mut self, on: bool, bus: &Bus<'_, Self>) {
        if self.switched == Some(on) {
            return;
        }
        let switch = Switch {
            service: MODEL_SERVICE.to_owned(),
            on,
        };
        let _ = bus.later(move |client| {
            let done = settled(&client.call(&switch, CALL_TIME));
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                // not done: asked again with the next look
                service.switched = done.then_some(on);
                service.look(bus);
            })
        });
    }

    /// Reads the model size once (at start, and again until the settings service answered).
    fn read_choice(&mut self, bus: &Bus<'_, Self>) {
        if self.choice_read {
            return;
        }
        let _ = bus.later(move |client| {
            let key = MODEL_KEY.to_owned();
            let value = client
                .call(&Get { key }, CALL_TIME)
                .ok()
                .and_then(|r| r.value);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                let Some(value) = value else { return };
                service.choice_read = true;
                if let Some(class) = choice::of(&value) {
                    service.choice = class;
                }
                service.look(bus);
            })
        });
    }

    /// Reads the setting once (at start, and again until the service manager did it).
    fn follow_setting(&mut self, bus: &Bus<'_, Self>) {
        if self.switched.is_some() {
            return;
        }
        let _ = bus.later(move |client| {
            let key = ENABLED_KEY.to_owned();
            let value = client
                .call(&Get { key }, CALL_TIME)
                .ok()
                .and_then(|r| r.value);
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                if let Some(SettingValue::Flag(on)) = value {
                    service.switch(on, bus);
                }
            })
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

    fn complete(&mut self, request: IncomingRequest, ask: Complete, bus: &Bus<'_, Self>) {
        self.next += 1;
        let id = self.next;
        let backend = self.backend.clone();
        let choice = self.choice;
        let started = bus.later(move |_| {
            let text = backend.complete_with(
                Some(choice),
                &ask.system,
                &ask.prompt,
                ask.max_tokens,
                THINK_TIME,
            );
            Box::new(move |service: &mut Self, _: &Bus<'_, Self>| service.answer(id, text))
        });
        if started {
            self.waiting.push((id, request));
        } else {
            let _ = request.fail(ErrorCode::ServiceBusy);
        }
    }
}

impl Service for LlmBus {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![Changed::TOPIC]
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        // Until the model service answered: no model.
        bus.retain(&LlmState::default());
        self.last = Some(LlmState::default());
        self.follow_setting(bus);
        self.read_choice(bus);
        self.read_place(bus);
        self.look(bus);
    }

    fn event(&mut self, event: ultar_bus::RawEvent, bus: &Bus<'_, Self>) {
        let Ok(changed) = event.decode::<Changed>() else {
            return;
        };
        match (changed.key.as_str(), &changed.value) {
            (ENABLED_KEY, SettingValue::Flag(on)) => {
                let on = *on;
                self.switched = self.switched.filter(|was| *was == on);
                self.switch(on, bus);
            }
            (MODEL_KEY, value) => {
                if let Some(class) = choice::of(value) {
                    if self.choice != class {
                        self.choice = class;
                        self.look(bus);
                    }
                }
            }
            (PLACE_KEY, value) => self.set_place(value),
            _ => {}
        }
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        if request.method() != Complete::METHOD {
            let _ = request.fail(ErrorCode::UnknownMethod);
            return;
        }
        match request.decode::<Complete>() {
            Ok(ask) if ask.system.len() + ask.prompt.len() <= MAX_PROMPT => {
                self.complete(request, ask, bus);
            }
            _ => {
                let _ = request.fail(ErrorCode::BadPayload);
            }
        }
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.follow_setting(bus);
        self.read_choice(bus);
        self.read_place(bus);
        self.look(bus);
    }
}

/// Whether a switch call needs no repeat: it was done, or the image has no model service
/// (init names it unknown – asking again every few seconds changes nothing and only fills
/// the log).
fn settled<T>(answer: &Result<T, ultar_bus::BusError>) -> bool {
    match answer {
        Ok(_) => true,
        Err(ultar_bus::BusError::Remote(ErrorCode::Service(code))) => {
            *code == ERROR_UNKNOWN_SERVICE
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::BusError;

    #[test]
    fn a_switch_is_asked_again_only_when_it_may_still_succeed() {
        assert!(settled(&Ok::<(), BusError>(())));
        let unknown = BusError::Remote(ErrorCode::Service(ERROR_UNKNOWN_SERVICE));
        assert!(
            settled(&Err::<(), _>(unknown)),
            "no model service in this image"
        );
        assert!(!settled(&Err::<(), _>(BusError::Timeout)));
        let too_soon = BusError::Remote(ErrorCode::Service(
            ultar_bus::messages::init::ERROR_TOO_SOON,
        ));
        assert!(!settled(&Err::<(), _>(too_soon)));
    }
}
