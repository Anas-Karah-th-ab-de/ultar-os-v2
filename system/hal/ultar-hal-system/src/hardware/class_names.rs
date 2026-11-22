//! Reverse table of the device class names the Ultar kernel prints.
//!
//! The kernel's device tables (`geraete` console command, self test) show the PCI class as
//! a German name produced by `pci::class_name(class, subclass, prog_if)` instead of the
//! class code. This module maps such a name back to the part of the class code it
//! determines. Digits the name does not determine are filled with `f`, so a class pattern
//! longer than the known prefix never matches by accident.

/// A PCI class code of which only a prefix is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartialClass {
    /// 24-bit class code; unknown trailing digits are `f`.
    pub code: u32,
    /// Number of known leading hex digits: 0, 2, 4 or 6.
    pub known_digits: u8,
}

impl PartialClass {
    /// Nothing is known about the class.
    pub const UNKNOWN: Self = Self {
        code: 0x00ff_ffff,
        known_digits: 0,
    };

    /// A completely known class code.
    pub fn exact(code: u32) -> Self {
        Self {
            code: code & 0x00ff_ffff,
            known_digits: 6,
        }
    }

    fn prefix(prefix: u32, digits: u8) -> Self {
        let unknown_bits = u32::from(6 - digits) * 4;
        let fill = (1u32 << unknown_bits) - 1;
        Self {
            code: (prefix << unknown_bits) | fill,
            known_digits: digits,
        }
    }

    /// `true` when every digit is known.
    pub fn is_exact(self) -> bool {
        self.known_digits == 6
    }
}

/// `(name printed by the kernel, known prefix, number of known digits)`.
/// Order and texts mirror `kernel/ultar-kern/src/pci.rs::class_name`. These are tokens of
/// the kernel's text output, compared byte for byte and never shown to a user; non-ASCII
/// letters are written as escapes (`\u{fc}` = ü) to keep them apart from interface text.
const NAMES: &[(&str, u32, u8)] = &[
    ("IDE-Controller", 0x0101, 4),
    ("SATA (AHCI)", 0x01_0601, 6),
    ("NVMe-Speicher", 0x01_0802, 6),
    ("SCSI-Speicher", 0x0100, 4),
    ("Massenspeicher", 0x01, 2),
    ("Ethernet", 0x0200, 4),
    ("Netzwerk (WLAN u. a.)", 0x0280, 4),
    ("Netzwerk", 0x02, 2),
    ("Grafik (VGA)", 0x0300, 4),
    ("Grafik", 0x03, 2),
    ("Audio (HD Audio)", 0x0403, 4),
    ("Multimedia", 0x04, 2),
    ("Host-Br\u{fc}cke", 0x0600, 4),
    ("ISA-Br\u{fc}cke", 0x0601, 4),
    ("PCI-Br\u{fc}cke", 0x0604, 4),
    ("Br\u{fc}cke", 0x06, 2),
    ("Kommunikation (seriell)", 0x07, 2),
    ("Systemperipherie", 0x08, 2),
    ("Eingabe", 0x09, 2),
    ("USB 3 (xHCI)", 0x0c_0330, 6),
    ("USB 2 (EHCI)", 0x0c_0320, 6),
    ("USB (alt)", 0x0c03, 4),
    ("SMBus", 0x0c05, 4),
    ("Serieller Bus", 0x0c, 2),
    ("Funk (Bluetooth u. a.)", 0x0d, 2),
    ("Verschl\u{fc}sselung", 0x10, 2),
    ("Signalverarbeitung", 0x11, 2),
    ("Rechenbeschleuniger (NPU)", 0x12, 2),
];

/// The class prefix a kernel class name stands for; [`PartialClass::UNKNOWN`] for
/// `unbekannt` and for names this table does not know.
pub fn class_from_name(name: &str) -> PartialClass {
    NAMES
        .iter()
        .find(|(known, _, _)| *known == name)
        .map_or(PartialClass::UNKNOWN, |(_, prefix, digits)| {
            PartialClass::prefix(*prefix, *digits)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_map_to_prefixes() {
        assert_eq!(
            class_from_name("NVMe-Speicher"),
            PartialClass::exact(0x01_0802)
        );
        assert_eq!(
            class_from_name("USB 3 (xHCI)"),
            PartialClass::exact(0x0c_0330)
        );
        let ethernet = class_from_name("Ethernet");
        assert_eq!((ethernet.code, ethernet.known_digits), (0x02_00ff, 4));
        let audio = class_from_name("Audio (HD Audio)");
        assert_eq!((audio.code, audio.known_digits), (0x04_03ff, 4));
        let multimedia = class_from_name("Multimedia");
        assert_eq!((multimedia.code, multimedia.known_digits), (0x04_ffff, 2));
        assert!(!multimedia.is_exact());
        assert_eq!(class_from_name("unbekannt"), PartialClass::UNKNOWN);
        assert_eq!(class_from_name("Quantum"), PartialClass::UNKNOWN);
        assert_eq!(PartialClass::UNKNOWN.code, 0x00ff_ffff);
    }

    #[test]
    fn table_is_unambiguous() {
        for (index, (name, _, digits)) in NAMES.iter().enumerate() {
            assert!(matches!(digits, 2 | 4 | 6), "{name}");
            assert!(
                NAMES[index + 1..].iter().all(|(other, _, _)| other != name),
                "{name} listed twice"
            );
        }
        assert_eq!(NAMES.len(), 28);
    }
}
