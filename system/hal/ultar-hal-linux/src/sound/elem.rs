//! Decoding ALSA element descriptions and choosing the playback controls.

/// Type of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElemType {
    /// On/off per channel.
    Boolean,
    /// Integer range per channel.
    Integer,
    /// Anything else (enumerations, bytes, …).
    Other,
}

/// One element of a card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElemInfo {
    /// Numeric id.
    pub numid: u32,
    /// Interface (2 = mixer).
    pub iface: i32,
    /// Name, e.g. `Master Playback Volume`.
    pub name: String,
    /// Type.
    pub kind: ElemType,
    /// Channels.
    pub count: u32,
    /// Lowest value (integers).
    pub min: i64,
    /// Highest value (integers).
    pub max: i64,
    /// Readable and writable.
    pub rw: bool,
}

impl ElemInfo {
    /// `raw` as percent of the range.
    pub fn percent(&self, raw: i64) -> u8 {
        if self.max <= self.min {
            return 0;
        }
        let span = i128::from(self.max) - i128::from(self.min);
        let r = i128::from(raw.clamp(self.min, self.max)) - i128::from(self.min);
        ((r * 100 + span / 2) / span) as u8
    }

    /// `percent` (0–100) as a raw value of the range.
    pub fn raw(&self, percent: u8) -> i64 {
        let p = i128::from(percent.min(100));
        let span = i128::from(self.max) - i128::from(self.min);
        (i128::from(self.min) + (span * p + 50) / 100) as i64
    }
}

/// The controls the audio service drives on one card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playback {
    /// Volume (integer element).
    pub volume: Option<ElemInfo>,
    /// Switch (boolean element: on = sound).
    pub switch: Option<ElemInfo>,
}

const MIXER: i32 = 2;
const READ: u32 = 1;
const WRITE: u32 = 2;
/// Names in the order they are preferred.
const VOLUMES: [&str; 4] = [
    "Master Playback Volume",
    "PCM Playback Volume",
    "Speaker Playback Volume",
    "Headphone Playback Volume",
];
const SWITCHES: [&str; 3] = [
    "Master Playback Switch",
    "Speaker Playback Switch",
    "Headphone Playback Switch",
];

fn word(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn long(b: &[u8], o: usize) -> i64 {
    let mut x = [0u8; 8];
    x.copy_from_slice(&b[o..o + 8]);
    i64::from_le_bytes(x)
}

/// A NUL-terminated text field.
pub fn c_text(b: &[u8]) -> String {
    let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// Decodes a `snd_ctl_elem_info` (`None` for a buffer of another size).
pub fn decode_info(b: &[u8]) -> Option<ElemInfo> {
    if b.len() != super::ioctl::INFO_SIZE {
        return None;
    }
    let kind = match word(b, 64) {
        1 => ElemType::Boolean,
        2 => ElemType::Integer,
        _ => ElemType::Other,
    };
    let access = word(b, 68);
    let (min, max) = match kind {
        ElemType::Integer => (long(b, 80), long(b, 88)),
        _ => (0, 1),
    };
    Some(ElemInfo {
        numid: word(b, 0),
        iface: word(b, 4) as i32,
        name: c_text(&b[16..60]),
        kind,
        count: word(b, 72).clamp(1, 128),
        min,
        max,
        rw: access & (READ | WRITE) == READ | WRITE,
    })
}

/// Chooses the playback volume and switch of a card (`None`: it has neither).
pub fn choose(elems: &[ElemInfo]) -> Option<Playback> {
    let find = |names: &[&str], kind: ElemType| {
        names.iter().find_map(|n| {
            elems
                .iter()
                .find(|e| e.iface == MIXER && e.rw && e.kind == kind && e.name == *n)
                .cloned()
        })
    };
    let volume = find(&VOLUMES, ElemType::Integer).filter(|v| v.max > v.min);
    let switch = find(&SWITCHES, ElemType::Boolean);
    (volume.is_some() || switch.is_some()).then_some(Playback { volume, switch })
}

/// The first channel's value of a `snd_ctl_elem_value`.
pub fn first_value(b: &[u8]) -> i64 {
    long(b, 72)
}

/// Writes `value` into the first `channels` channels of a `snd_ctl_elem_value`.
pub fn fill(b: &mut [u8], channels: u32, value: i64) {
    for c in 0..channels.min(128) as usize {
        b[72 + c * 8..80 + c * 8].copy_from_slice(&value.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(numid: u32, name: &str, kind: u32, min: i64, max: i64, access: u32) -> [u8; 272] {
        let mut b = [0u8; 272];
        b[..4].copy_from_slice(&numid.to_le_bytes());
        b[4..8].copy_from_slice(&2u32.to_le_bytes());
        b[16..16 + name.len()].copy_from_slice(name.as_bytes());
        b[64..68].copy_from_slice(&kind.to_le_bytes());
        b[68..72].copy_from_slice(&access.to_le_bytes());
        b[72..76].copy_from_slice(&2u32.to_le_bytes());
        b[80..88].copy_from_slice(&min.to_le_bytes());
        b[88..96].copy_from_slice(&max.to_le_bytes());
        b
    }

    #[test]
    fn the_master_controls_are_chosen() {
        let elems: Vec<ElemInfo> = [
            info(1, "PCM Playback Volume", 2, 0, 255, 3),
            info(2, "Master Playback Volume", 2, 0, 87, 3),
            info(3, "Master Playback Switch", 1, 0, 0, 3),
            info(4, "Capture Volume", 2, 0, 63, 3),
            info(5, "Speaker Playback Volume", 2, 0, 87, 1),
        ]
        .iter()
        .filter_map(|b| decode_info(b))
        .collect();
        let p = choose(&elems).unwrap();
        let v = p.volume.unwrap();
        assert_eq!((v.numid, v.count, v.max), (2, 2, 87));
        assert_eq!(p.switch.unwrap().numid, 3);
        assert!(
            choose(&elems[3..]).is_none(),
            "capture only and read-only: nothing"
        );
        assert!(decode_info(&[0u8; 10]).is_none());
    }

    #[test]
    fn percent_and_raw_are_inverse_enough() {
        let v = decode_info(&info(1, "Master Playback Volume", 2, -10, 90, 3)).unwrap();
        assert_eq!(
            (v.percent(-10), v.percent(90), v.percent(40), v.percent(500)),
            (0, 100, 50, 100)
        );
        for p in 0..=100u8 {
            assert!(v.percent(v.raw(p)).abs_diff(p) <= 1);
        }
        assert_eq!(v.raw(200), 90);
        let flat = ElemInfo {
            min: 5,
            max: 5,
            ..v
        };
        assert_eq!(flat.percent(5), 0);
        let mut buf = [0u8; 1224];
        fill(&mut buf, 2, 42);
        assert_eq!(first_value(&buf), 42);
        assert_eq!(long(&buf, 80), 42);
        assert_eq!(c_text(b"HDA Intel\0junk"), "HDA Intel");
    }

    #[test]
    fn extreme_ranges_do_not_overflow() {
        let e = decode_info(&info(1, "Master Playback Volume", 2, i64::MIN, i64::MAX, 3)).unwrap();
        assert_eq!((e.percent(i64::MAX), e.percent(i64::MIN)), (100, 0));
        assert_eq!((e.raw(0), e.raw(100)), (i64::MIN, i64::MAX));
    }

    #[test]
    fn fuzzed_descriptions_never_panic() {
        let mut seed = 0x9e37_79b9_u64;
        for _ in 0..2000 {
            let mut b = [0u8; 272];
            for byte in b.iter_mut() {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                *byte = seed as u8;
            }
            if let Some(e) = decode_info(&b) {
                let _ = (
                    e.percent(i64::MAX),
                    e.percent(i64::MIN),
                    e.raw(255),
                    choose(&[e]),
                );
            }
        }
    }
}
