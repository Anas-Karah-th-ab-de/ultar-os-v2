//! Single capabilities and the compact set that stores them.

use serde::de::{Error as DeError, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// One thing a device can do. Behaviour of the system hangs on these, not on the edition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Capability {
    /// A touch screen is present.
    Touch = 0,
    /// A hardware keyboard is attached.
    Keyboard = 1,
    /// A pointer device (mouse, touchpad) is attached.
    Pointer = 2,
    /// The device runs from a battery.
    Battery = 3,
    /// A cellular modem is present.
    Modem = 4,
    /// A satellite positioning receiver is present.
    Gnss = 5,
    /// At least one camera is present.
    Camera = 6,
    /// Motion or environment sensors are present.
    Sensors = 7,
    /// A display other than the built-in one is connected.
    ExternalDisplay = 8,
    /// A trusted platform module is present.
    Tpm = 9,
    /// A GPU usable for rendering is present.
    Gpu = 10,
    /// A neural processing unit is present.
    Npu = 11,
    /// A Wi-Fi adapter is present.
    Wifi = 12,
    /// A Bluetooth adapter is present.
    Bluetooth = 13,
    /// Sound can be played.
    AudioOut = 14,
    /// Sound can be recorded.
    AudioIn = 15,
    /// A fingerprint reader is present.
    Fingerprint = 16,
    /// A vibration motor is present.
    Haptics = 17,
    /// Controllable indicator lights are present.
    Leds = 18,
}

impl Capability {
    /// Every capability, in bit order.
    pub const ALL: [Capability; 19] = [
        Capability::Touch,
        Capability::Keyboard,
        Capability::Pointer,
        Capability::Battery,
        Capability::Modem,
        Capability::Gnss,
        Capability::Camera,
        Capability::Sensors,
        Capability::ExternalDisplay,
        Capability::Tpm,
        Capability::Gpu,
        Capability::Npu,
        Capability::Wifi,
        Capability::Bluetooth,
        Capability::AudioOut,
        Capability::AudioIn,
        Capability::Fingerprint,
        Capability::Haptics,
        Capability::Leds,
    ];

    /// Bit of this capability inside a [`CapabilitySet`].
    pub const fn bit(self) -> u32 {
        1 << (self as u8)
    }

    /// Stable name used in manifests and in the stored profile.
    pub const fn as_str(self) -> &'static str {
        match self {
            Capability::Touch => "touch",
            Capability::Keyboard => "keyboard",
            Capability::Pointer => "pointer",
            Capability::Battery => "battery",
            Capability::Modem => "modem",
            Capability::Gnss => "gnss",
            Capability::Camera => "camera",
            Capability::Sensors => "sensors",
            Capability::ExternalDisplay => "external_display",
            Capability::Tpm => "tpm",
            Capability::Gpu => "gpu",
            Capability::Npu => "npu",
            Capability::Wifi => "wifi",
            Capability::Bluetooth => "bluetooth",
            Capability::AudioOut => "audio_out",
            Capability::AudioIn => "audio_in",
            Capability::Fingerprint => "fingerprint",
            Capability::Haptics => "haptics",
            Capability::Leds => "leds",
        }
    }

    /// Parses a stable name (case-insensitive, `-` accepted for `_`).
    pub fn parse(name: &str) -> Option<Capability> {
        let wanted = name.trim().to_ascii_lowercase().replace('-', "_");
        Capability::ALL
            .iter()
            .copied()
            .find(|capability| capability.as_str() == wanted)
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Compact set of [`Capability`] values (one bit each).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CapabilitySet(u32);

impl CapabilitySet {
    /// The empty set.
    pub const EMPTY: CapabilitySet = CapabilitySet(0);

    /// Creates an empty set.
    pub const fn new() -> Self {
        CapabilitySet(0)
    }

    /// Creates a set from a list of capabilities (usable in constants).
    pub const fn of(list: &[Capability]) -> Self {
        let mut bits = 0;
        let mut index = 0;
        while index < list.len() {
            bits |= list[index].bit();
            index += 1;
        }
        CapabilitySet(bits)
    }

    /// Returns the set with every capability.
    pub const fn all() -> Self {
        CapabilitySet::of(&Capability::ALL)
    }

    /// Returns a copy with `capability` added (builder style).
    #[must_use]
    pub const fn with(self, capability: Capability) -> Self {
        CapabilitySet(self.0 | capability.bit())
    }

    /// Adds a capability.
    pub fn insert(&mut self, capability: Capability) {
        self.0 |= capability.bit();
    }

    /// Removes a capability.
    pub fn remove(&mut self, capability: Capability) {
        self.0 &= !capability.bit();
    }

    /// Adds or removes a capability depending on `present`.
    pub fn set(&mut self, capability: Capability, present: bool) {
        if present {
            self.insert(capability);
        } else {
            self.remove(capability);
        }
    }

    /// True if the capability is in the set.
    pub const fn contains(self, capability: Capability) -> bool {
        self.0 & capability.bit() != 0
    }

    /// True if every capability of `other` is in this set.
    pub const fn contains_all(self, other: CapabilitySet) -> bool {
        self.0 & other.0 == other.0
    }

    /// True if at least one capability of `other` is in this set.
    pub const fn intersects(self, other: CapabilitySet) -> bool {
        self.0 & other.0 != 0
    }

    /// Union of both sets.
    #[must_use]
    pub const fn union(self, other: CapabilitySet) -> Self {
        CapabilitySet(self.0 | other.0)
    }

    /// Capabilities of `self` that are missing in `have`.
    #[must_use]
    pub const fn missing_in(self, have: CapabilitySet) -> Self {
        CapabilitySet(self.0 & !have.0)
    }

    /// True if the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Number of capabilities in the set.
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// Iterates over the contained capabilities in bit order.
    pub fn iter(self) -> CapabilitySetIter {
        CapabilitySetIter {
            set: self,
            position: 0,
        }
    }
}

/// Iterator over the capabilities of a [`CapabilitySet`].
#[derive(Clone, Debug)]
pub struct CapabilitySetIter {
    set: CapabilitySet,
    position: usize,
}

impl Iterator for CapabilitySetIter {
    type Item = Capability;

    fn next(&mut self) -> Option<Capability> {
        while let Some(capability) = Capability::ALL.get(self.position).copied() {
            self.position += 1;
            if self.set.contains(capability) {
                return Some(capability);
            }
        }
        None
    }
}

impl IntoIterator for CapabilitySet {
    type Item = Capability;
    type IntoIter = CapabilitySetIter;

    fn into_iter(self) -> CapabilitySetIter {
        self.iter()
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<I: IntoIterator<Item = Capability>>(iter: I) -> Self {
        let mut set = CapabilitySet::new();
        for capability in iter {
            set.insert(capability);
        }
        set
    }
}

impl Serialize for CapabilitySet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for capability in self.iter() {
            seq.serialize_element(capability.as_str())?;
        }
        seq.end()
    }
}

struct SetVisitor;

impl<'de> Visitor<'de> for SetVisitor {
    type Value = CapabilitySet;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a list of capability names")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<CapabilitySet, A::Error> {
        let mut set = CapabilitySet::new();
        while let Some(name) = seq.next_element::<String>()? {
            let capability = Capability::parse(&name)
                .ok_or_else(|| A::Error::custom(format_args!("unknown capability `{name}`")))?;
            set.insert(capability);
        }
        Ok(set)
    }
}

impl<'de> Deserialize<'de> for CapabilitySet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_seq(SetVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_are_unique() {
        for (index, capability) in Capability::ALL.iter().enumerate() {
            assert_eq!(*capability as usize, index);
            assert_eq!(Capability::parse(capability.as_str()), Some(*capability));
            assert_eq!(capability.to_string(), capability.as_str());
        }
        assert_eq!(Capability::parse(" Audio-Out "), Some(Capability::AudioOut));
        assert_eq!(Capability::parse("warp_drive"), None);
        assert_eq!(Capability::parse(""), None);
    }

    #[test]
    fn set_operations() {
        let mut set = CapabilitySet::new();
        assert!(set.is_empty());
        set.insert(Capability::Touch);
        set.set(Capability::Modem, true);
        set.set(Capability::Wifi, false);
        assert!(set.contains(Capability::Touch) && set.contains(Capability::Modem));
        assert!(!set.contains(Capability::Wifi));
        assert_eq!(set.len(), 2);
        set.remove(Capability::Touch);
        assert_eq!(set, CapabilitySet::of(&[Capability::Modem]));
        assert_eq!(CapabilitySet::all().len(), Capability::ALL.len());
        assert_eq!(CapabilitySet::EMPTY, CapabilitySet::default());
    }

    #[test]
    fn subset_and_intersection() {
        let have = CapabilitySet::of(&[Capability::Wifi, Capability::Touch, Capability::Gpu]);
        let need = CapabilitySet::of(&[Capability::Wifi, Capability::Modem]);
        assert!(!have.contains_all(need));
        assert!(have.intersects(need));
        assert!(have.contains_all(CapabilitySet::EMPTY));
        assert!(!have.intersects(CapabilitySet::EMPTY));
        assert_eq!(
            need.missing_in(have),
            CapabilitySet::of(&[Capability::Modem])
        );
        assert_eq!(have.union(need).len(), 4);
        assert!(have.with(Capability::Modem).contains_all(need));
    }

    #[test]
    fn iteration_is_in_bit_order() {
        let set: CapabilitySet = [Capability::Leds, Capability::Touch, Capability::Gnss]
            .into_iter()
            .collect();
        let listed: Vec<Capability> = set.into_iter().collect();
        assert_eq!(
            listed,
            vec![Capability::Touch, Capability::Gnss, Capability::Leds]
        );
        assert_eq!(CapabilitySet::EMPTY.iter().count(), 0);
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Holder {
        flags: CapabilitySet,
    }

    #[test]
    fn serde_as_name_list() {
        let holder = Holder {
            flags: CapabilitySet::of(&[Capability::Wifi, Capability::AudioOut]),
        };
        let text = toml::to_string(&holder).unwrap();
        assert_eq!(text.trim(), r#"flags = ["wifi", "audio_out"]"#);
        assert_eq!(toml::from_str::<Holder>(&text).unwrap(), holder);
        assert!(toml::from_str::<Holder>(r#"flags = ["bogus"]"#).is_err());
        assert!(toml::from_str::<Holder>("flags = 3").is_err());
        let empty: Holder = toml::from_str("flags = []").unwrap();
        assert!(empty.flags.is_empty());
    }
}
