//! The A/B disk layout and the load options that carry the slot from the loader to the kernel.
//!
//! ```text
//! GPT name        content                                         written by
//! ULTAR-EFI       FAT: \EFI\BOOT\BOOTX64.EFI = loader only         installer
//! ULTAR-BOOTCTL   raw: table copy 0 @ 0, copy 1 @ 32 KiB,          installer, loader, updater
//!                      slot record A @ 64 KiB, B @ 96 KiB
//! ultar-kernel-a  raw: kernel PE image of slot A                   installer, updater
//! ultar-kernel-b  raw: kernel PE image of slot B                   updater
//! ultar-system-a  ext4 system image of slot A (mounted read-only)  installer, updater
//! ultar-system-b  ext4 system image of slot B                      updater
//! ultar-data      ext4, writable, shared by both slots             installer
//! ```
//!
//! Offsets on `ULTAR-BOOTCTL` are in bytes and are multiples of 32 KiB, so every record starts
//! on a block of its own for block sizes from 512 bytes to 32 KiB: a torn write of one record
//! can never touch another.

use crate::slot::Slot;

/// GPT name of the start partition (FAT, holds only the loader).
pub const EFI_PART: &str = "ULTAR-EFI";
/// GPT name of the raw boot control partition.
pub const BOOTCTL_PART: &str = "ULTAR-BOOTCTL";
/// GPT name of the writable data partition shared by both slots.
pub const DATA_PART: &str = "ultar-data";
/// GPT name of the root partition of an installation from before A/B (single root).
pub const LEGACY_ROOT_PART: &str = "ultar-compat";

/// Byte offsets of the two slot table copies on [`BOOTCTL_PART`].
pub const TABLE_COPY_OFFSETS: [u64; 2] = [0, 32 * 1024];
/// Byte offsets of the slot records ([`crate::meta`]) of slot A and B on [`BOOTCTL_PART`].
pub const META_OFFSETS: [u64; 2] = [64 * 1024, 96 * 1024];
/// Smallest valid [`BOOTCTL_PART`].
pub const BOOTCTL_MIN_BYTES: u64 = 128 * 1024;
/// Size the installer gives [`BOOTCTL_PART`].
pub const BOOTCTL_INSTALL_BYTES: u64 = 1024 * 1024;
/// Smallest valid kernel partition.
pub const KERNEL_PART_MIN_BYTES: u64 = 16 * 1024 * 1024;
/// Size the installer gives each kernel partition (at least).
pub const KERNEL_PART_INSTALL_BYTES: u64 = 32 * 1024 * 1024;
/// Largest kernel image the loader reads (bounds its memory use).
pub const KERNEL_IMAGE_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// GPT name of the kernel partition of `slot`.
pub fn kernel_part(slot: Slot) -> &'static str {
    match slot {
        Slot::A => "ultar-kernel-a",
        Slot::B => "ultar-kernel-b",
    }
}

/// GPT name of the system partition of `slot`.
pub fn system_part(slot: Slot) -> &'static str {
    match slot {
        Slot::A => "ultar-system-a",
        Slot::B => "ultar-system-b",
    }
}

/// The partitions a running system may give to root for raw access: boot control and both
/// slots' kernel and system partitions. Nothing else (start partition, data, foreign
/// partitions) is ever reachable raw.
pub const RAW_PARTS: [&str; 5] = [
    BOOTCTL_PART,
    "ultar-kernel-a",
    "ultar-kernel-b",
    "ultar-system-a",
    "ultar-system-b",
];

/// Whether `name` belongs to `slot` (its kernel or system partition).
pub fn part_of_slot(name: &str, slot: Slot) -> bool {
    name == kernel_part(slot) || name == system_part(slot)
}

const OPT_SLOT: &str = "ultar.slot=";
const OPT_TRIAL: &str = "ultar.trial=1";
const OPT_TICKS: &str = "ultar.loader_tsc=";
const OPT_DISK: &str = "ultar.disk=";
/// Longest load option string [`BootOptions::encode`] produces (UTF-16 units, with the final 0).
pub const OPTIONS_MAX: usize = 128;

/// What the loader tells the kernel (UEFI load options, UTF-16, words separated by spaces).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BootOptions {
    /// The slot that was started.
    pub slot: Option<Slot>,
    /// The start is a trial (a counted attempt of a new slot).
    pub trial: bool,
    /// Processor time stamp ticks the loader spent from its entry to the start of the kernel.
    pub loader_ticks: Option<u64>,
    /// GPT disk GUID of the disk the loader read the slot table from: the kernel takes its
    /// partitions from that disk only (another disk with the same names, such as an
    /// installation stick, is never used).
    pub disk: Option<[u8; 16]>,
}

/// Why load options were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsError {
    /// Not valid UTF-16, or longer than the parser accepts.
    Encoding,
    /// An `ultar.` word with a bad value, or one given twice.
    Value,
}

impl BootOptions {
    /// Parses UTF-16 load options. Words that do not start with `ultar.` are ignored (firmware
    /// and boot managers may add their own); an `ultar.` word that is unknown, malformed or
    /// repeated makes the whole string invalid – the kernel must not guess its root.
    pub fn parse(units: &[u16]) -> Result<Self, OptionsError> {
        let mut text = [0u8; 4 * OPTIONS_MAX];
        let mut len = 0;
        for c in char::decode_utf16(units.iter().copied().take_while(|u| *u != 0)) {
            let c = c.map_err(|_| OptionsError::Encoding)?;
            let mut buf = [0u8; 4];
            let bytes = c.encode_utf8(&mut buf).as_bytes();
            let end = len + bytes.len();
            text.get_mut(len..end)
                .ok_or(OptionsError::Encoding)?
                .copy_from_slice(bytes);
            len = end;
        }
        let text = core::str::from_utf8(&text[..len]).map_err(|_| OptionsError::Encoding)?;
        let mut out = Self::default();
        let mut seen = [false; 4];
        for word in text.split(' ').filter(|w| w.starts_with("ultar.")) {
            let index = if let Some(v) = word.strip_prefix(OPT_SLOT) {
                out.slot = Some(Slot::from_letter(v).ok_or(OptionsError::Value)?);
                0
            } else if word == OPT_TRIAL {
                out.trial = true;
                1
            } else if let Some(v) = word.strip_prefix(OPT_TICKS) {
                out.loader_ticks = Some(parse_decimal(v).ok_or(OptionsError::Value)?);
                2
            } else if let Some(v) = word.strip_prefix(OPT_DISK) {
                out.disk = Some(parse_guid(v).ok_or(OptionsError::Value)?);
                3
            } else {
                return Err(OptionsError::Value);
            };
            if core::mem::replace(&mut seen[index], true) {
                return Err(OptionsError::Value);
            }
        }
        Ok(out)
    }

    /// Writes the options as UTF-16 with a final 0 into `out` and returns the number of units
    /// including the 0. `None` when `out` is too short (it never is with [`OPTIONS_MAX`]).
    pub fn encode(&self, out: &mut [u16]) -> Option<usize> {
        let mut w = Writer { out, len: 0 };
        if let Some(slot) = self.slot {
            w.word(OPT_SLOT)?;
            w.push(slot.letter())?;
        }
        if self.trial {
            w.word(OPT_TRIAL)?;
        }
        if let Some(ticks) = self.loader_ticks {
            w.word(OPT_TICKS)?;
            let mut digits = [0u8; 20];
            let mut n = ticks;
            let mut i = digits.len();
            loop {
                i -= 1;
                digits[i] = b'0' + (n % 10) as u8;
                n /= 10;
                if n == 0 {
                    break;
                }
            }
            w.push(core::str::from_utf8(&digits[i..]).ok()?)?;
        }
        if let Some(guid) = self.disk {
            w.word(OPT_DISK)?;
            for byte in guid {
                let hex = [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 15)]];
                w.push(core::str::from_utf8(&hex).ok()?)?;
            }
        }
        *w.out.get_mut(w.len)? = 0;
        Some(w.len + 1)
    }
}

const HEX: [u8; 16] = *b"0123456789abcdef";

fn parse_guid(text: &str) -> Option<[u8; 16]> {
    let bytes = text.as_bytes();
    if bytes.len() != 32 {
        return None;
    }
    let digit = |c: u8| HEX.iter().position(|h| *h == c).map(|v| v as u8);
    let mut out = [0u8; 16];
    for (i, pair) in bytes.chunks(2).enumerate() {
        out[i] = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(out)
}

fn parse_decimal(text: &str) -> Option<u64> {
    if text.is_empty() || text.len() > 20 {
        return None;
    }
    text.bytes().try_fold(0u64, |acc, b| {
        b.is_ascii_digit().then_some(())?;
        acc.checked_mul(10)?.checked_add(u64::from(b - b'0'))
    })
}

struct Writer<'a> {
    out: &'a mut [u16],
    len: usize,
}

impl Writer<'_> {
    fn push(&mut self, text: &str) -> Option<()> {
        for unit in text.encode_utf16() {
            *self.out.get_mut(self.len)? = unit;
            self.len += 1;
        }
        Some(())
    }

    fn word(&mut self, text: &str) -> Option<()> {
        if self.len > 0 {
            self.push(" ")?;
        }
        self.push(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> [u16; 128] {
        let mut out = [0u16; 128];
        for (i, u) in s.encode_utf16().enumerate() {
            out[i] = u;
        }
        out
    }

    #[test]
    fn names_and_offsets() {
        assert_eq!(kernel_part(Slot::B), "ultar-kernel-b");
        assert_eq!(system_part(Slot::A), "ultar-system-a");
        assert!(part_of_slot("ultar-system-b", Slot::B));
        assert!(!part_of_slot("ultar-system-b", Slot::A));
        assert!(!part_of_slot(BOOTCTL_PART, Slot::A));
        for slot in Slot::BOTH {
            assert!(
                RAW_PARTS.contains(&kernel_part(slot)) && RAW_PARTS.contains(&system_part(slot))
            );
        }
        let all = [
            TABLE_COPY_OFFSETS[0],
            TABLE_COPY_OFFSETS[1],
            META_OFFSETS[0],
            META_OFFSETS[1],
        ];
        for (i, o) in all.iter().enumerate() {
            assert_eq!(o % (32 * 1024), 0);
            assert!(o + 32 * 1024 <= BOOTCTL_MIN_BYTES);
            assert!(all[..i].iter().all(|p| p != o));
        }
    }

    #[test]
    fn options_round_trip() {
        let mut buf = [0u16; OPTIONS_MAX];
        for opts in [
            BootOptions::default(),
            BootOptions {
                slot: Some(Slot::B),
                trial: true,
                loader_ticks: Some(u64::MAX),
                disk: Some([0xFE; 16]),
            },
            BootOptions {
                slot: Some(Slot::A),
                trial: false,
                loader_ticks: Some(0),
                disk: None,
            },
        ] {
            let n = opts.encode(&mut buf).unwrap();
            assert_eq!(buf[n - 1], 0);
            assert_eq!(BootOptions::parse(&buf[..n]).unwrap(), opts);
        }
        let mut guid = [0u8; 16];
        guid[0] = 0x0A;
        guid[15] = 0xF1;
        let opts = BootOptions {
            slot: Some(Slot::B),
            trial: true,
            loader_ticks: Some(42),
            disk: Some(guid),
        };
        let n = opts.encode(&mut buf).unwrap();
        let text = String::from_utf16(&buf[..n - 1]).unwrap();
        assert_eq!(
            text,
            "ultar.slot=b ultar.trial=1 ultar.loader_tsc=42 ultar.disk=0a0000000000000000000000000000f1"
        );
        assert!(BootOptions {
            slot: Some(Slot::B),
            ..Default::default()
        }
        .encode(&mut buf[..12])
        .is_none());
    }

    #[test]
    fn foreign_words_are_ignored_and_bad_ultar_words_refused() {
        let o = BootOptions::parse(&utf16("\\EFI\\BOOT\\BOOTX64.EFI quiet ultar.slot=a")).unwrap();
        assert_eq!(o.slot, Some(Slot::A));
        assert_eq!(
            BootOptions::parse(&utf16("")).unwrap(),
            BootOptions::default()
        );
        for bad in [
            "ultar.slot=c",
            "ultar.slot=",
            "ultar.slot=a ultar.slot=a",
            "ultar.slot=a ultar.slot=b",
            "ultar.trial=0",
            "ultar.loader_tsc=x",
            "ultar.loader_tsc=99999999999999999999",
            "ultar.loader_tsc=",
            "ultar.root=/dev/sda",
            "ultar.disk=0a",
            "ultar.disk=0A0000000000000000000000000000F1",
            "ultar.disk=0a0000000000000000000000000000g1",
        ] {
            assert_eq!(
                BootOptions::parse(&utf16(bad)),
                Err(OptionsError::Value),
                "{bad}"
            );
        }
        assert_eq!(
            BootOptions::parse(&[0xD800, 0x41]),
            Err(OptionsError::Encoding)
        );
        let long = [u16::from(b'x'); 4 * OPTIONS_MAX + 1];
        assert_eq!(BootOptions::parse(&long), Err(OptionsError::Encoding));
    }
}
