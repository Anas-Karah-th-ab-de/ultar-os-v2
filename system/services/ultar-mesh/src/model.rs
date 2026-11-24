//! Lending the language model: a device lets a paired device's model think for it.
//!
//! The lending device announces the class of its model as a capability and answers only
//! devices it granted [`Right::Model`] (off after pairing). Prompt and answer travel in the
//! encrypted channel like everything else; nothing is kept on the lending device.

use crate::node::{Effect, Node};
use crate::wire::Frame;
use ultar_bus::messages::llm::{ModelClass, MAX_ANSWER, MAX_PROMPT};
use ultar_bus::messages::mesh::{Capability, Right};
use ultar_bus::messages::mesh_model::Lender;

const MODELS: [(Capability, ModelClass); 3] = [
    (Capability::ModelSmall, ModelClass::Small),
    (Capability::ModelMedium, ModelClass::Medium),
    (Capability::ModelLarge, ModelClass::Large),
];

/// The class of the model a device with `capabilities` has.
pub fn class_of(capabilities: &[Capability]) -> Option<ModelClass> {
    MODELS
        .iter()
        .filter(|(c, _)| capabilities.contains(c))
        .map(|(_, class)| *class)
        .max()
}

fn cut(mut text: String, max: usize) -> String {
    let mut end = max.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

impl Node {
    /// The model of this device changed (`None`: it has none, or cannot run it now).
    /// Linked devices are told.
    pub fn set_model(&mut self, class: Option<ModelClass>) -> Vec<Effect> {
        if class_of(&self.config.capabilities) == class {
            return Vec::new();
        }
        let caps = &mut self.config.capabilities;
        caps.retain(|c| !MODELS.iter().any(|(m, _)| m == c));
        caps.extend(
            MODELS
                .iter()
                .filter(|(_, c)| Some(*c) == class)
                .map(|(m, _)| *m),
        );
        let hello = self.hello();
        let mut effects: Vec<Effect> = self
            .links
            .keys()
            .map(|peer| Effect::Send(peer.clone(), hello.clone()))
            .collect();
        effects.push(Effect::Changed);
        effects
    }

    /// The reachable paired devices that announce a model, best model first.
    pub fn lenders(&self) -> Vec<Lender> {
        let mut list: Vec<Lender> = self
            .links
            .iter()
            .filter(|(id, _)| self.paired(id))
            .filter_map(|(id, link)| {
                let device = link.device.as_ref()?;
                Some(Lender {
                    device: id.clone(),
                    name: device.name.clone(),
                    class: class_of(&device.capabilities)?,
                })
            })
            .collect();
        list.sort_by(|a, b| b.class.cmp(&a.class).then(a.device.cmp(&b.device)));
        list
    }

    /// Asks the model of `device` (`None`: the best lender); the id of the request and
    /// the device it went to, or `None` when no such device lends a model.
    pub fn think(
        &mut self,
        device: Option<&str>,
        system: &str,
        prompt: &str,
        max_tokens: u32,
    ) -> Option<(u64, String, Vec<Effect>)> {
        if system.len() + prompt.len() > MAX_PROMPT {
            return None;
        }
        let lenders = self.lenders();
        let lender = match device {
            Some(id) => lenders.into_iter().find(|l| l.device == id),
            None => lenders.into_iter().next(),
        }?;
        self.thinking += 1;
        let id = self.thinking;
        let frame = Frame::Think {
            id,
            system: system.to_owned(),
            prompt: prompt.to_owned(),
            max_tokens,
        };
        let effects = vec![Effect::Send(lender.device.clone(), frame)];
        Some((id, lender.device, effects))
    }

    /// The local model answered (or failed) for `peer`'s request `id`.
    pub fn thought(&mut self, peer: &str, id: u64, ok: bool, text: String) -> Vec<Effect> {
        let frame = Frame::Thought {
            id,
            ok,
            text: cut(text, MAX_ANSWER),
        };
        vec![Effect::Send(peer.to_owned(), frame)]
    }

    pub(crate) fn on_model_frame(&mut self, peer: &str, frame: Frame) -> Vec<Effect> {
        match frame {
            Frame::Think {
                id,
                system,
                prompt,
                max_tokens,
            } => {
                let has = class_of(&self.config.capabilities).is_some();
                let fits = system.len() + prompt.len() <= MAX_PROMPT;
                if !has || !fits || !self.allows(peer, Right::Model) {
                    return self.thought(peer, id, false, String::new());
                }
                vec![Effect::Think {
                    peer: peer.to_owned(),
                    id,
                    system,
                    prompt,
                    max_tokens,
                }]
            }
            Frame::Thought { id, ok, text } => vec![Effect::Thought {
                id,
                peer: peer.to_owned(),
                ok,
                text: cut(text, MAX_ANSWER),
            }],
            _ => Vec::new(),
        }
    }
}
