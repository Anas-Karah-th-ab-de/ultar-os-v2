//! Was kann dieses Gerät beitragen?

pub fn kerne() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

pub fn last() -> f32 {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse().ok())
        .unwrap_or(0.0)
}

pub fn ram_frei_mb() -> u64 {
    let s = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    for l in s.lines() {
        if let Some(r) = l.strip_prefix("MemAvailable:") {
            return r
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
                / 1024;
        }
    }
    0
}

/// Handy, Laptop, Server oder Platine
pub fn ausgabe() -> String {
    if let Ok(s) = std::fs::read_to_string("/etc/ultar/ausgabe") {
        let s = s.trim();
        if !s.is_empty() {
            return s.to_string();
        }
    }
    let akku = std::fs::read_dir("/sys/class/power_supply")
        .map(|d| {
            d.flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with("BAT"))
        })
        .unwrap_or(false);
    let arm = cfg!(target_arch = "aarch64") || cfg!(target_arch = "arm");
    let modem = std::path::Path::new("/dev/wwan0at0").exists()
        || std::path::Path::new("/dev/ttyUSB2").exists();
    match (arm, akku, modem) {
        (_, true, true) | (true, true, _) => "handy".into(),
        (false, true, _) => "laptop".into(),
        (true, false, _) => "platine".into(),
        _ => "server".into(),
    }
}
