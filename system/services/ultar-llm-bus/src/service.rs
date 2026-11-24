//! The bus side: `llm.state` (retained, looked up regularly) and `llm.complete`.

use crate::backend::Backend;
use std::time::Duration;
use ultar_bus::messages::llm::{Complete, Completion, LlmState, MAX_PROMPT, SERVICE};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_svckit::{Bus, Service};

/// How often the model service is asked what it can do.
pub const TICK: Duration = Duration::from_secs(10);
/// How long the model may take for one answer.
pub const THINK_TIME: Duration = Duration::from_secs(110);

/// The language model on the bus.
pub struct LlmBus {
    backend: Backend,
    last: Option<LlmState>,
    next: u64,
    waiting: Vec<(u64, IncomingRequest)>,
}

impl LlmBus {
    /// The bus face of the model service behind `backend`.
    pub fn new(backend: Backend) -> Self {
        Self {
            backend,
            last: None,
            next: 0,
            waiting: Vec::new(),
        }
    }

    fn look(&mut self, bus: &Bus<'_, Self>) {
        let backend = self.backend.clone();
        let _ = bus.later(move |_| {
            let state = backend.state();
            Box::new(move |service: &mut Self, bus: &Bus<'_, Self>| {
                if service.last.as_ref() != Some(&state) {
                    bus.retain(&state);
                    service.last = Some(state);
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
        let started = bus.later(move |_| {
            let text = backend.complete(&ask.system, &ask.prompt, ask.max_tokens, THINK_TIME);
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

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        // Until the model service answered: no model.
        bus.retain(&LlmState::default());
        self.last = Some(LlmState::default());
        self.look(bus);
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
        self.look(bus);
    }
}
