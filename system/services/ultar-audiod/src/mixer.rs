//! The audio logic over the HAL, without the bus.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ultar_bus::messages::audio::{AudioOutcome, AudioState, Output};
use ultar_hal::HalError;
use ultar_hal_linux::{Card, CardControls};

/// What is kept across restarts.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Saved {
    /// The chosen output.
    pub output: Option<String>,
    /// Volume and mute per card id.
    pub levels: BTreeMap<String, (u8, bool)>,
}

/// Most cards remembered.
const MAX_SAVED: usize = 32;

/// The cards and the choice.
pub struct Mixer {
    hal: Box<dyn CardControls>,
    cards: Vec<Card>,
    saved: Saved,
}

fn outcome(result: Result<(), HalError>) -> AudioOutcome {
    match result {
        Ok(()) => AudioOutcome::Done,
        Err(HalError::NotSupported) => AudioOutcome::Unsupported,
        Err(_) => AudioOutcome::Failed,
    }
}

impl Mixer {
    /// A mixer over `hal` with what was saved.
    pub fn new(hal: Box<dyn CardControls>, saved: Saved) -> Self {
        Self {
            hal,
            cards: Vec::new(),
            saved,
        }
    }

    /// What is to be saved now.
    pub fn saved(&self) -> &Saved {
        &self.saved
    }

    fn active(&self) -> Option<&Card> {
        let chosen = self.saved.output.as_ref();
        chosen
            .and_then(|id| self.cards.iter().find(|c| &c.id == id))
            .or_else(|| self.cards.first())
    }

    /// Looks at the cards again; restores the saved levels of cards that appeared.
    /// `true` when the set of cards changed.
    pub fn rescan(&mut self) -> bool {
        let now = self.hal.cards();
        let changed = now
            .iter()
            .map(|c| &c.id)
            .ne(self.cards.iter().map(|c| &c.id));
        for card in now
            .iter()
            .filter(|c| !self.cards.iter().any(|o| o.id == c.id))
        {
            if let Some((volume, muted)) = self.saved.levels.get(&card.id).copied() {
                if card.playback.volume.is_some() {
                    let _ = self.hal.set_volume(card, volume);
                }
                if card.playback.switch.is_some() {
                    let _ = self.hal.set_muted(card, muted);
                }
            }
        }
        self.cards = now;
        changed
    }

    /// The bus view.
    pub fn state(&self) -> AudioState {
        let Some(card) = self.active() else {
            return AudioState::default();
        };
        AudioState {
            present: true,
            volume_percent: self.hal.volume(card).unwrap_or(0),
            muted: self.hal.muted(card).unwrap_or(false),
            output_name: card.name.clone(),
            output_id: Some(card.id.clone()),
            has_volume: card.playback.volume.is_some(),
        }
    }

    /// The outputs.
    pub fn outputs(&self) -> Vec<Output> {
        let active = self.active().map(|c| c.id.clone());
        self.cards
            .iter()
            .map(|c| Output {
                id: c.id.clone(),
                name: c.name.clone(),
                active: active.as_ref() == Some(&c.id),
            })
            .collect()
    }

    fn remember(&mut self) {
        let Some(card) = self.active().cloned() else {
            return;
        };
        let level = (
            self.hal.volume(&card).unwrap_or(0),
            self.hal.muted(&card).unwrap_or(false),
        );
        if self.saved.levels.len() >= MAX_SAVED && !self.saved.levels.contains_key(&card.id) {
            let first = self.saved.levels.keys().next().cloned();
            if let Some(first) = first {
                self.saved.levels.remove(&first);
            }
        }
        self.saved.levels.insert(card.id, level);
    }

    /// Sets the volume of the output in use.
    pub fn set_volume(&mut self, percent: u8) -> AudioOutcome {
        if percent > 100 {
            return AudioOutcome::Invalid;
        }
        let Some(card) = self.active().cloned() else {
            return AudioOutcome::NoDevice;
        };
        let result = outcome(self.hal.set_volume(&card, percent));
        self.remember();
        result
    }

    /// Mutes the output in use.
    pub fn set_mute(&mut self, on: bool) -> AudioOutcome {
        let Some(card) = self.active().cloned() else {
            return AudioOutcome::NoDevice;
        };
        let result = outcome(self.hal.set_muted(&card, on));
        self.remember();
        result
    }

    /// Uses another output.
    pub fn set_output(&mut self, id: &str) -> AudioOutcome {
        if self.cards.is_empty() {
            return AudioOutcome::NoDevice;
        }
        if !self.cards.iter().any(|c| c.id == id) {
            return AudioOutcome::Invalid;
        }
        self.saved.output = Some(id.to_owned());
        AudioOutcome::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use ultar_hal::HalResult;
    use ultar_hal_linux::sound::{decode_info, Playback};

    /// A card set in memory: (card, volume, muted).
    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<Vec<(Card, u8, bool)>>>);

    fn card(id: &str, volume: bool) -> Card {
        let mut b = [0u8; 272];
        b[..4].copy_from_slice(&1u32.to_le_bytes());
        b[4..8].copy_from_slice(&2u32.to_le_bytes());
        let name = b"Master Playback Volume";
        b[16..16 + name.len()].copy_from_slice(name);
        b[64..68].copy_from_slice(&2u32.to_le_bytes());
        b[68..72].copy_from_slice(&3u32.to_le_bytes());
        b[88..96].copy_from_slice(&100i64.to_le_bytes());
        Card {
            id: id.into(),
            name: format!("Card {id}"),
            playback: Playback {
                volume: volume.then(|| decode_info(&b).unwrap()),
                switch: volume.then(|| decode_info(&b).unwrap()),
            },
        }
    }

    impl CardControls for Fake {
        fn cards(&self) -> Vec<Card> {
            self.0.lock().unwrap().iter().map(|c| c.0.clone()).collect()
        }
        fn volume(&self, card: &Card) -> HalResult<u8> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.0.id == card.id)
                .map(|c| c.1)
                .ok_or(HalError::NotFound)
        }
        fn set_volume(&self, card: &Card, percent: u8) -> HalResult<()> {
            if card.playback.volume.is_none() {
                return Err(HalError::NotSupported);
            }
            let mut all = self.0.lock().unwrap();
            let c = all
                .iter_mut()
                .find(|c| c.0.id == card.id)
                .ok_or(HalError::NotFound)?;
            c.1 = percent;
            Ok(())
        }
        fn muted(&self, card: &Card) -> HalResult<bool> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.0.id == card.id)
                .map(|c| c.2)
                .ok_or(HalError::NotFound)
        }
        fn set_muted(&self, card: &Card, muted: bool) -> HalResult<()> {
            if card.playback.switch.is_none() {
                return Err(HalError::NotSupported);
            }
            let mut all = self.0.lock().unwrap();
            let c = all
                .iter_mut()
                .find(|c| c.0.id == card.id)
                .ok_or(HalError::NotFound)?;
            c.2 = muted;
            Ok(())
        }
    }

    #[test]
    fn no_card_means_no_device() {
        let mut m = Mixer::new(Box::new(Fake::default()), Saved::default());
        assert!(!m.rescan());
        assert_eq!(m.state(), AudioState::default());
        assert_eq!(m.set_volume(10), AudioOutcome::NoDevice);
        assert_eq!(m.set_mute(true), AudioOutcome::NoDevice);
        assert_eq!(m.set_output("card0"), AudioOutcome::NoDevice);
        assert!(m.outputs().is_empty());
    }

    #[test]
    fn volume_mute_output_and_restore() {
        let fake = Fake::default();
        fake.0
            .lock()
            .unwrap()
            .push((card("card0", true), 70, false));
        fake.0
            .lock()
            .unwrap()
            .push((card("card1", false), 0, false));
        let mut m = Mixer::new(Box::new(fake.clone()), Saved::default());
        assert!(m.rescan());
        let s = m.state();
        assert_eq!(
            (s.present, s.volume_percent, s.output_id.as_deref()),
            (true, 70, Some("card0"))
        );
        assert_eq!(m.set_volume(101), AudioOutcome::Invalid);
        assert_eq!(m.set_volume(30), AudioOutcome::Done);
        assert_eq!(m.set_mute(true), AudioOutcome::Done);
        assert_eq!(m.saved().levels["card0"], (30, true));
        assert_eq!(m.set_output("card9"), AudioOutcome::Invalid);
        assert_eq!(m.set_output("card1"), AudioOutcome::Done);
        assert_eq!(
            m.set_volume(10),
            AudioOutcome::Unsupported,
            "a switch-only card"
        );
        assert!(m.outputs().iter().any(|o| o.id == "card1" && o.active));
        assert!(!m.state().has_volume);
        // The driver resets card0 (a new start); the saved level comes back.
        let saved = m.saved().clone();
        fake.0.lock().unwrap()[0].1 = 100;
        fake.0.lock().unwrap()[0].2 = false;
        let mut again = Mixer::new(Box::new(fake.clone()), saved);
        again.rescan();
        assert_eq!(fake.0.lock().unwrap()[0].1, 30);
        assert!(fake.0.lock().unwrap()[0].2);
        // A card that goes away: the other one is used.
        fake.0.lock().unwrap().remove(1);
        assert!(again.rescan());
        assert_eq!(again.state().output_id.as_deref(), Some("card0"));
    }
}
