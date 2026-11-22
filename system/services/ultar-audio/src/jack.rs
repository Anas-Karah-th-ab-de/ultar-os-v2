//! Kopfhörer-Erkennung: Samsungs earjack-Klasse (/sys/class/audio/earjack/state) bzw. die
//! Schalter des Eingabegeräts „Codec3035 Headset Events" (SW_HEADPHONE_INSERT/SW_MICROPHONE_INSERT).

use std::os::fd::AsRawFd;

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

const SW_HEADPHONE_INSERT: usize = 0x02;
const SW_MICROPHONE_INSERT: usize = 0x04;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jack {
    None,
    /// Kopfhörer mit Mikrofon (4-polig)
    Headset,
    /// nur Kopfhörer (3-polig)
    Headphone,
}

fn from_sysfs() -> Option<Jack> {
    let s = std::fs::read_to_string("/sys/class/audio/earjack/state").ok()?;
    let v: i32 = s.trim().split(|c: char| !c.is_ascii_digit()).find(|t| !t.is_empty())?.parse().ok()?;
    // Samsung: 0 = nichts, 1 = 4-polig (mit Mikro), 2 = 3-polig
    Some(match v {
        0 => Jack::None,
        2 => Jack::Headphone,
        _ => Jack::Headset,
    })
}

fn from_input() -> Option<Jack> {
    for i in 0..32 {
        let n = std::fs::read_to_string(format!("/sys/class/input/input{i}/name")).unwrap_or_default();
        if !n.contains("Headset") && !n.to_lowercase().contains("jack") {
            continue;
        }
        let dir = std::fs::read_dir(format!("/sys/class/input/input{i}")).ok()?;
        let ev = dir.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).find(|n| n.starts_with("event"))?;
        let f = std::fs::File::open(format!("/dev/input/{ev}")).ok()?;
        let mut bits = [0u8; 8];
        // EVIOCGSW(len) = _IOC(READ, 'E', 0x1b, len)
        let req = (2u64 << 30) | ((bits.len() as u64) << 16) | ((b'E' as u64) << 8) | 0x1b;
        if unsafe { ioctl(f.as_raw_fd(), req, bits.as_mut_ptr()) } < 0 {
            return None;
        }
        let on = |b: usize| bits[b / 8] & (1 << (b % 8)) != 0;
        return Some(match (on(SW_HEADPHONE_INSERT), on(SW_MICROPHONE_INSERT)) {
            (true, true) => Jack::Headset,
            (true, false) => Jack::Headphone,
            (false, true) => Jack::Headset,
            _ => Jack::None,
        });
    }
    None
}

pub fn state() -> Jack {
    from_sysfs().or_else(from_input).unwrap_or(Jack::None)
}

pub fn describe() -> String {
    format!(
        "earjack/state={:?} input={:?} -> {:?}",
        std::fs::read_to_string("/sys/class/audio/earjack/state").map(|s| s.trim().to_string()).ok(),
        from_input(),
        state()
    )
}
