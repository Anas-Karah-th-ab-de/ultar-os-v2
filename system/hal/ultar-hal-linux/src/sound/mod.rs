//! Sound cards: the playback volume and mute of each card, through the ALSA control
//! interface (`dev/snd/controlC<N>`).
//!
//! [`CardControls`] is the HAL trait the audio service uses; [`SoundCards`] is the real
//! backend. A machine without sound cards (QEMU without a sound device, the Ultar kernel
//! today) lists none – the service then reports "no device". The sample streams (playback and
//! capture) are in [`pcm`].

mod elem;
mod ioctl;
pub mod pcm;
mod pcm_ioctl;

pub use elem::{choose, decode_info, ElemInfo, ElemType, Playback};

use crate::sysfs::entries;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use ultar_hal::{HalError, HalResult};

/// Most cards and elements looked at.
const MAX_CARDS: usize = 16;
const MAX_ELEMS: u32 = 1024;

/// One sound card with playback controls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    /// Stable id (`card<N>`).
    pub id: String,
    /// Name the driver gives the card (data, shown as it is).
    pub name: String,
    /// The playback controls.
    pub playback: Playback,
}

/// The sound cards of a machine (HAL trait of the audio service).
pub trait CardControls: Send {
    /// The cards that have a playback volume or switch.
    fn cards(&self) -> Vec<Card>;
    /// Volume of `card` in percent of its range.
    fn volume(&self, card: &Card) -> HalResult<u8>;
    /// Sets the volume of `card` in percent.
    fn set_volume(&self, card: &Card, percent: u8) -> HalResult<()>;
    /// Whether `card` is muted.
    fn muted(&self, card: &Card) -> HalResult<bool>;
    /// Mutes or unmutes `card`.
    fn set_muted(&self, card: &Card, muted: bool) -> HalResult<()>;
}

/// The ALSA cards below a root directory.
#[derive(Clone, Debug)]
pub struct SoundCards {
    root: PathBuf,
}

fn io(e: impl Into<std::io::Error>) -> HalError {
    let e: std::io::Error = e.into();
    match e.kind() {
        std::io::ErrorKind::NotFound => HalError::NotFound,
        std::io::ErrorKind::PermissionDenied => HalError::PermissionDenied,
        _ => HalError::Io(e.raw_os_error().unwrap_or(0)),
    }
}

impl SoundCards {
    /// Cards below `root` (`/` on a device).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn control(&self, id: &str, write: bool) -> HalResult<File> {
        let number = id.strip_prefix("card").ok_or(HalError::InvalidArgument)?;
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(HalError::InvalidArgument);
        }
        OpenOptions::new()
            .read(true)
            .write(write)
            .open(self.root.join(format!("dev/snd/controlC{number}")))
            .map_err(io)
    }

    fn read_card(&self, number: &str) -> Option<Card> {
        let id = format!("card{number}");
        let file = self.control(&id, false).ok()?;
        let info = ioctl::card_info(&file).ok()?;
        let count = ioctl::elem_count(&file).ok()?.min(MAX_ELEMS);
        let mut elems = Vec::new();
        for numid in 1..=count {
            let mut buf = [0u8; ioctl::INFO_SIZE];
            buf[..4].copy_from_slice(&numid.to_le_bytes());
            if ioctl::elem_info(&file, &mut buf).is_ok() {
                if let Some(e) = decode_info(&buf) {
                    elems.push(e);
                }
            }
        }
        let playback = choose(&elems)?;
        Some(Card {
            id,
            name: elem::c_text(&info[40..72]),
            playback,
        })
    }

    fn read_values(&self, card: &Card, numid: u32) -> HalResult<[u8; ioctl::VALUE_SIZE]> {
        let file = self.control(&card.id, false)?;
        let mut buf = [0u8; ioctl::VALUE_SIZE];
        buf[..4].copy_from_slice(&numid.to_le_bytes());
        ioctl::elem_read(&file, &mut buf).map_err(io)?;
        Ok(buf)
    }

    fn write_values(&self, card: &Card, numid: u32, channels: u32, value: i64) -> HalResult<()> {
        let file = self.control(&card.id, true)?;
        let mut buf = [0u8; ioctl::VALUE_SIZE];
        buf[..4].copy_from_slice(&numid.to_le_bytes());
        elem::fill(&mut buf, channels, value);
        ioctl::elem_write(&file, &mut buf).map_err(io)
    }
}

impl CardControls for SoundCards {
    fn cards(&self) -> Vec<Card> {
        entries(&self.root.join("dev/snd"), 256)
            .iter()
            .filter_map(|p| {
                p.file_name()?
                    .to_str()?
                    .strip_prefix("controlC")
                    .map(str::to_owned)
            })
            .take(MAX_CARDS)
            .filter_map(|n| self.read_card(&n))
            .collect()
    }

    fn volume(&self, card: &Card) -> HalResult<u8> {
        let v = card
            .playback
            .volume
            .as_ref()
            .ok_or(HalError::NotSupported)?;
        let buf = self.read_values(card, v.numid)?;
        Ok(v.percent(elem::first_value(&buf)))
    }

    fn set_volume(&self, card: &Card, percent: u8) -> HalResult<()> {
        let v = card
            .playback
            .volume
            .as_ref()
            .ok_or(HalError::NotSupported)?;
        self.write_values(card, v.numid, v.count, v.raw(percent))
    }

    fn muted(&self, card: &Card) -> HalResult<bool> {
        let Some(s) = &card.playback.switch else {
            return Ok(false);
        };
        let buf = self.read_values(card, s.numid)?;
        // A playback switch that is on means sound: muted is "off".
        Ok(elem::first_value(&buf) == 0)
    }

    fn set_muted(&self, card: &Card, muted: bool) -> HalResult<()> {
        let s = card
            .playback
            .switch
            .as_ref()
            .ok_or(HalError::NotSupported)?;
        self.write_values(card, s.numid, s.count, i64::from(!muted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_machine_without_sound_cards_lists_none() {
        let cards = SoundCards::new("/nonexistent-ultar-root");
        assert!(cards.cards().is_empty());
        assert_eq!(
            cards.control("card0", false).err(),
            Some(HalError::NotFound)
        );
        assert_eq!(
            cards.control("../x", false).err(),
            Some(HalError::InvalidArgument)
        );
        assert_eq!(
            cards.control("card", false).err(),
            Some(HalError::InvalidArgument)
        );
        assert_eq!(
            cards.control("card1x", false).err(),
            Some(HalError::InvalidArgument)
        );
        // A regular file is no control device: the card is skipped, not guessed at.
        let root = std::env::temp_dir().join(format!("hal-snd-{}", std::process::id()));
        std::fs::create_dir_all(root.join("dev/snd")).unwrap();
        std::fs::write(root.join("dev/snd/controlC0"), b"").unwrap();
        assert!(SoundCards::new(&root).cards().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn this_machines_cards_if_any_are_read_without_failing() {
        // Real hardware when the test machine has it; nothing otherwise.
        for card in SoundCards::new("/").cards() {
            assert!(card.id.starts_with("card"));
        }
    }
}
