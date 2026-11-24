//! What a device is and what it can do.
//!
//! Decisions are taken from capabilities only. On the wire kinds and capabilities are
//! words, so a device of a newer version (a TV, a speaker, something not thought of yet)
//! is understood as far as its words are known and never refused for the rest.

use base64::Engine;
use sha2::{Digest, Sha256};
use ultar_bus::messages::mesh::{Capability, DeviceKind};

/// Length of a device id.
pub const ID_LEN: usize = 20;

const ID_DOMAIN: &[u8] = b"ultar-mesh-v1";
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// The id of the device with the static public key `key`: 20 characters of `a-z2-7`.
pub fn id_of(key: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(ID_DOMAIN);
    hash.update(key);
    let digest = hash.finalize();
    let mut id = String::with_capacity(ID_LEN);
    let (mut acc, mut bits) = (0u32, 0u32);
    for byte in digest {
        acc = (acc << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 && id.len() < ID_LEN {
            bits -= 5;
            id.push(char::from(ALPHABET[((acc >> bits) & 31) as usize]));
        }
    }
    id
}

/// `true` when `id` has the form of a device id.
pub fn is_id(id: &str) -> bool {
    id.len() == ID_LEN && id.bytes().all(|b| ALPHABET.contains(&b))
}

/// Bytes as text for the wire.
pub fn encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD_NO_PAD.encode(bytes)
}

/// Text from the wire as bytes.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(text)
        .ok()
}

/// The word of a kind on the wire.
pub fn kind_word(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Phone => "phone",
        DeviceKind::Tablet => "tablet",
        DeviceKind::Desktop => "desktop",
        DeviceKind::Server => "server",
        DeviceKind::Board => "board",
        DeviceKind::Tv => "tv",
        DeviceKind::Speaker => "speaker",
        DeviceKind::Other => "other",
    }
}

/// The kind a word names (`Other` for a word this version does not know).
pub fn kind_from(word: &str) -> DeviceKind {
    match word {
        "phone" => DeviceKind::Phone,
        "tablet" => DeviceKind::Tablet,
        "desktop" => DeviceKind::Desktop,
        "server" => DeviceKind::Server,
        "board" => DeviceKind::Board,
        "tv" => DeviceKind::Tv,
        "speaker" => DeviceKind::Speaker,
        _ => DeviceKind::Other,
    }
}

const CAPABILITIES: [(Capability, &str); 11] = [
    (Capability::Screen, "screen"),
    (Capability::Input, "input"),
    (Capability::AudioOut, "audio-out"),
    (Capability::AudioIn, "audio-in"),
    (Capability::Files, "files"),
    (Capability::Apps, "apps"),
    (Capability::Clipboard, "clipboard"),
    (Capability::Compute, "compute"),
    (Capability::ModelSmall, "model-small"),
    (Capability::ModelMedium, "model-medium"),
    (Capability::ModelLarge, "model-large"),
];

/// The word of a capability on the wire.
pub fn capability_word(capability: Capability) -> &'static str {
    CAPABILITIES
        .iter()
        .find(|(c, _)| *c == capability)
        .map_or("", |(_, w)| w)
}

/// The capabilities a list of words names; unknown words are left out.
pub fn capabilities_from(words: &[String]) -> Vec<Capability> {
    let mut list: Vec<Capability> = words
        .iter()
        .filter_map(|w| CAPABILITIES.iter().find(|(_, n)| n == w).map(|(c, _)| *c))
        .collect();
    list.sort_unstable();
    list.dedup();
    list
}

/// What a kind of device can do when nothing else is configured.
pub fn default_capabilities(kind: DeviceKind) -> Vec<Capability> {
    use Capability::{Apps, AudioIn, AudioOut, Clipboard, Compute, Files, Input, Screen};
    match kind {
        DeviceKind::Phone | DeviceKind::Tablet | DeviceKind::Desktop => {
            vec![
                Screen, Input, AudioOut, AudioIn, Files, Apps, Clipboard, Compute,
            ]
        }
        DeviceKind::Server | DeviceKind::Board => vec![Files, Compute],
        DeviceKind::Tv => vec![Screen, AudioOut],
        DeviceKind::Speaker => vec![AudioOut],
        DeviceKind::Other => Vec::new(),
    }
}

/// A device as the mesh describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// Its id.
    pub id: String,
    /// Its name (data).
    pub name: String,
    /// What it is.
    pub kind: DeviceKind,
    /// What it can do.
    pub capabilities: Vec<Capability>,
    /// Ids of the apps installed on it.
    pub apps: Vec<String>,
}

impl Device {
    /// `true` when it has `capability`.
    pub fn can(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }
}

/// A name as it may be shown: no control characters, at most 48 characters, never empty.
pub fn clean_name(name: &str) -> String {
    let clean: String = name.chars().filter(|c| !c.is_control()).take(48).collect();
    let clean = clean.trim().to_owned();
    if clean.is_empty() {
        "device".to_owned()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_well_formed() {
        let a = id_of(&[1; 32]);
        assert_eq!(a, id_of(&[1; 32]));
        assert_ne!(a, id_of(&[2; 32]));
        assert!(is_id(&a));
        assert!(!is_id("short") && !is_id(&"A".repeat(ID_LEN)));
    }

    #[test]
    fn unknown_words_do_not_break_a_newer_device() {
        let words = vec![
            "screen".to_owned(),
            "hologram".to_owned(),
            "audio-out".to_owned(),
        ];
        assert_eq!(
            capabilities_from(&words),
            vec![Capability::Screen, Capability::AudioOut]
        );
        assert_eq!(kind_from("fridge"), DeviceKind::Other);
        assert_eq!(kind_from(kind_word(DeviceKind::Tv)), DeviceKind::Tv);
    }

    #[test]
    fn a_speaker_plays_sound_and_nothing_else() {
        assert_eq!(
            default_capabilities(DeviceKind::Speaker),
            vec![Capability::AudioOut]
        );
        assert!(default_capabilities(DeviceKind::Tv).contains(&Capability::Screen));
        assert!(!default_capabilities(DeviceKind::Tv).contains(&Capability::Files));
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Beelink\n\u{7}  "), "Beelink");
        assert_eq!(clean_name("\n"), "device");
        assert_eq!(clean_name(&"x".repeat(100)).len(), 48);
        assert_eq!(decode(&encode(b"abc")).as_deref(), Some(&b"abc"[..]));
    }
}
