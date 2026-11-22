//! Geräteerkennung: Handy oder Desktop, schnelle Kerne, Speicher, Akku, Temperatur, Speicherdruck.
//! Alles direkt aus /proc und /sys – keine Zusatzprogramme.

use serde::Serialize;
use std::fs;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Mobile,
    Desktop,
}

#[derive(Clone, Debug, Serialize)]
pub struct Profile {
    pub kind: Kind,
    pub arch: &'static str,
    /// Logische CPUs
    pub cpus: usize,
    /// Physische Kerne (ohne Hyperthreading)
    pub cores: usize,
    /// CPUs mit dem höchsten Maximaltakt (big-Kerne bei big.LITTLE)
    pub big_cpus: Vec<usize>,
    pub simd: String,
    pub mem_total: u64,
}

fn read(p: &str) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn cpu_list() -> Vec<usize> {
    let online = read("/sys/devices/system/cpu/online").unwrap_or_default();
    let mut v = Vec::new();
    for part in online.split(',').filter(|s| !s.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                    v.extend(a..=b)
                }
            }
            None => {
                if let Ok(a) = part.parse() {
                    v.push(a)
                }
            }
        }
    }
    if v.is_empty() {
        v = (0..std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1))
            .collect();
    }
    v
}

/// Speicherwerte aus /proc/meminfo in Bytes.
pub fn meminfo(key: &str) -> u64 {
    let s = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    s.lines()
        .find(|l| l.starts_with(key) && l[key.len()..].starts_with(':'))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|n| n.parse::<u64>().ok())
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

pub fn mem_available() -> u64 {
    meminfo("MemAvailable")
}

fn simd() -> String {
    #[cfg(target_arch = "x86_64")]
    {
        let mut f = vec![];
        for (n, on) in [
            ("sse4.2", std::is_x86_feature_detected!("sse4.2")),
            ("avx", std::is_x86_feature_detected!("avx")),
            ("avx2", std::is_x86_feature_detected!("avx2")),
            ("fma", std::is_x86_feature_detected!("fma")),
            ("f16c", std::is_x86_feature_detected!("f16c")),
            ("avx512f", std::is_x86_feature_detected!("avx512f")),
        ] {
            if on {
                f.push(n)
            }
        }
        return f.join(" ");
    }
    #[cfg(target_arch = "aarch64")]
    {
        let mut f = vec!["neon"];
        for (n, on) in [
            (
                "dotprod",
                std::arch::is_aarch64_feature_detected!("dotprod"),
            ),
            ("i8mm", std::arch::is_aarch64_feature_detected!("i8mm")),
            ("fp16", std::arch::is_aarch64_feature_detected!("fp16")),
            ("sve", std::arch::is_aarch64_feature_detected!("sve")),
        ] {
            if on {
                f.push(n)
            }
        }
        return f.join(" ");
    }
    #[allow(unreachable_code)]
    String::new()
}

/// Hat das Gerät einen Akku?
pub fn has_battery() -> bool {
    battery().is_some()
}

/// Akku: (Prozent, lädt gerade). Erkennt Samsung-Knoten (battery) und Standard (type=Battery).
pub fn battery() -> Option<(u8, bool)> {
    let dir = fs::read_dir("/sys/class/power_supply").ok()?;
    let mut cands: Vec<String> = dir
        .flatten()
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();
    cands.sort_by_key(|p| !p.ends_with("/battery"));
    for p in cands {
        let ty = read(&format!("{p}/type")).unwrap_or_default();
        let name = p.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
        if ty != "Battery" && !name.contains("battery") && name != "bms" {
            continue;
        }
        if read(&format!("{p}/scope")).as_deref() == Some("Device") {
            continue; // Akku einer Maus o. Ä.
        }
        let Some(cap) = read(&format!("{p}/capacity")).and_then(|c| c.parse::<i32>().ok()) else {
            continue;
        };
        let st = read(&format!("{p}/status")).unwrap_or_default();
        let charging = matches!(st.as_str(), "Charging" | "Full")
            || read("/sys/class/power_supply/ac/online").as_deref() == Some("1");
        return Some((cap.clamp(0, 100) as u8, charging));
    }
    None
}

/// Höchste Temperatur der CPU-nahen Messstellen in °C.
pub fn cpu_temp() -> Option<f32> {
    let dir = fs::read_dir("/sys/class/thermal").ok()?;
    let mut best: Option<f32> = None;
    for e in dir.flatten() {
        let p = e.path();
        if !p
            .file_name()
            .map(|n| n.to_string_lossy().starts_with("thermal_zone"))
            .unwrap_or(false)
        {
            continue;
        }
        let ty = read(&format!("{}/type", p.display()))
            .unwrap_or_default()
            .to_ascii_lowercase();
        // Akku-, Funk- und Gehäusefühler zählen nicht
        if ty.contains("battery")
            || ty.contains("skin")
            || ty.contains("pa_")
            || ty.contains("modem")
        {
            continue;
        }
        if let Some(t) = read(&format!("{}/temp", p.display())).and_then(|t| t.parse::<f32>().ok())
        {
            let c = if t > 1000.0 { t / 1000.0 } else { t };
            if (0.0..150.0).contains(&c) {
                best = Some(best.map_or(c, |b: f32| b.max(c)));
            }
        }
    }
    best
}

/// Speicherdruck (PSI) „some avg10" in Prozent; None, wenn der Kernel kein PSI hat (z. B. A30, Kernel 4.4).
pub fn mem_pressure() -> Option<f32> {
    let s = fs::read_to_string("/proc/pressure/memory").ok()?;
    let l = s.lines().find(|l| l.starts_with("some"))?;
    l.split_whitespace()
        .find_map(|kv| kv.strip_prefix("avg10="))
        .and_then(|v| v.parse().ok())
}

pub fn detect() -> Profile {
    let cpus_list = cpu_list();
    let cpus = cpus_list.len();
    // Maximaltakt je CPU → big-Kerne
    let freqs: Vec<(usize, u64)> = cpus_list
        .iter()
        .map(|&c| {
            (
                c,
                read(&format!(
                    "/sys/devices/system/cpu/cpu{c}/cpufreq/cpuinfo_max_freq"
                ))
                .and_then(|f| f.parse().ok())
                .unwrap_or(0),
            )
        })
        .collect();
    let top = freqs.iter().map(|f| f.1).max().unwrap_or(0);
    let mut big_cpus: Vec<usize> = freqs.iter().filter(|f| f.1 == top).map(|f| f.0).collect();
    if big_cpus.is_empty() {
        big_cpus = cpus_list.clone();
    }
    // physische Kerne
    let mut ids = std::collections::BTreeSet::new();
    for &c in &cpus_list {
        let core = read(&format!("/sys/devices/system/cpu/cpu{c}/topology/core_id"))
            .unwrap_or_else(|| c.to_string());
        let pkg = read(&format!(
            "/sys/devices/system/cpu/cpu{c}/topology/physical_package_id"
        ))
        .unwrap_or_default();
        let cluster = read(&format!(
            "/sys/devices/system/cpu/cpu{c}/topology/cluster_id"
        ))
        .unwrap_or_default();
        ids.insert(format!("{pkg}:{cluster}:{core}"));
    }
    let cores = ids.len().clamp(1, cpus.max(1));
    let arch = std::env::consts::ARCH;
    let mem_total = meminfo("MemTotal");
    let kind = match std::env::var("ULTAR_LLM_PROFILE").as_deref() {
        Ok("mobile") => Kind::Mobile,
        Ok("desktop") => Kind::Desktop,
        _ => {
            // ARM mit Akku oder wenig Speicher = Handy/Tablet; sonst Desktop (PC, Mini-Server, RK3588-Platine)
            if arch != "x86_64" && (has_battery() || mem_total < 6_000_000_000) {
                Kind::Mobile
            } else {
                Kind::Desktop
            }
        }
    };
    Profile {
        kind,
        arch,
        cpus,
        cores,
        big_cpus,
        simd: simd(),
        mem_total,
    }
}

impl Profile {
    /// Threads für das Erzeugen (speicherbandbreiten-begrenzt) und für das Einlesen (rechenbegrenzt).
    pub fn auto_threads(&self) -> (i32, i32) {
        match self.kind {
            // big.LITTLE: Erzeugen auf so vielen Threads, wie es schnelle Kerne gibt (mindestens 2, höchstens 4);
            // langsame Kerne bremsen beim Erzeugen eher, helfen aber beim Einlesen des Prompts.
            Kind::Mobile => {
                let gen = if self.big_cpus.len() == self.cpus {
                    self.cpus.min(4)
                } else {
                    self.big_cpus.len().clamp(2, 4)
                };
                (gen as i32, self.cpus.min(8) as i32)
            }
            Kind::Desktop => {
                let n = self.cores.clamp(1, 16) as i32;
                (n, n)
            }
        }
    }

    pub fn auto_ctx(&self) -> u32 {
        let gb = self.mem_total as f64 / 1e9;
        match self.kind {
            Kind::Mobile => {
                if gb < 2.5 {
                    2048
                } else {
                    4096
                }
            }
            Kind::Desktop => {
                if gb >= 15.0 {
                    16384
                } else {
                    8192
                }
            }
        }
    }
}

/// Prozess auf bestimmte CPUs festlegen (Threads, die danach entstehen, erben das).
pub fn pin_to(cpus: &[usize]) -> bool {
    if cpus.is_empty() {
        return false;
    }
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        for &c in cpus {
            libc::CPU_SET(c, &mut set);
        }
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) == 0
    }
}

pub fn renice(n: i32) {
    if n != 0 {
        unsafe {
            libc::setpriority(libc::PRIO_PROCESS, 0, n);
        }
    }
}
