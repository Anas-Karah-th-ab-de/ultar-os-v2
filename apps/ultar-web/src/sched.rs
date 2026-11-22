//! Ressourcen-Steuerung: Prozessorkerne (big.LITTLE), Zeichen-Threads, Speicherbudget, Akku und Wärme.
//! Leitlinie: volle Leistung, solange Arbeit ansteht – im Leerlauf nichts.

use std::sync::OnceLock;

pub struct Topo {
    pub ncpu: usize,
    /// schnelle Kerne (höchste Maximalfrequenz)
    pub big: Vec<usize>,
    /// sparsame Kerne
    pub little: Vec<usize>,
    pub ram_mb: u64,
}

static TOPO: OnceLock<Topo> = OnceLock::new();

pub fn topo() -> &'static Topo {
    TOPO.get_or_init(|| {
        let ncpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let freqs: Vec<(usize, u64)> = (0..ncpu)
            .map(|c| {
                let f = std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{c}/cpufreq/cpuinfo_max_freq"))
                    .ok()
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(0);
                (c, f)
            })
            .collect();
        let max = freqs.iter().map(|x| x.1).max().unwrap_or(0);
        let min = freqs.iter().map(|x| x.1).min().unwrap_or(0);
        let (big, little): (Vec<_>, Vec<_>) = if max > min { freqs.iter().partition(|x| x.1 == max) } else { (freqs.iter().collect(), Vec::new()) };
        Topo {
            ncpu,
            big: big.iter().map(|x| x.0).collect(),
            little: little.iter().map(|x| x.0).collect(),
            ram_mb: meminfo("MemTotal").unwrap_or(2_000_000) / 1024,
        }
    })
}

pub fn meminfo(key: &str) -> Option<u64> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    s.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse().ok())
}

/// Anteil freier Arbeitsspeicher (0..1)
pub fn mem_free_ratio() -> f64 {
    match (meminfo("MemAvailable"), meminfo("MemTotal")) {
        (Some(a), Some(t)) if t > 0 => a as f64 / t as f64,
        _ => 1.0,
    }
}

fn set_affinity(tid: i32, cpus: &[usize]) -> bool {
    if cpus.is_empty() {
        return false;
    }
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        for &c in cpus {
            libc::CPU_SET(c, &mut set);
        }
        libc::sched_setaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &set) == 0
    }
}

/// Aktuellen Thread auf die schnellen Kerne legen (Compositor, Eingabe). Prozesse, die später
/// gestartet werden, erben das nicht, weil nur dieser Thread betroffen ist.
pub fn pin_current_to_big() {
    let t = topo();
    if !t.little.is_empty() {
        set_affinity(0, &t.big);
    }
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, -4);
    }
}

/// Umgebung für WebKit vor dem Start setzen: so viele Zeichen-Threads wie Kerne, CPU-Darstellung.
pub fn configure_env() {
    let t = topo();
    std::env::set_var("WEBKIT_SKIA_CPU_PAINTING_THREADS", t.ncpu.to_string());
    std::env::set_var("WEBKIT_SKIA_ENABLE_CPU_RENDERING", "1");
    // GStreamer-Dekoder: mehrere Threads
    std::env::set_var("GST_AV_THREADS", t.ncpu.to_string());
}

/// Hilfsprozesse von WebKit einordnen: Netzwerkprozess auf die sparsamen Kerne mit niedrigerer Priorität,
/// Seitenprozesse dürfen alle Kerne nutzen. `saver`: Energiesparen (Akku knapp/zu warm).
pub fn place_children(saver: bool) -> Vec<(i32, String)> {
    let t = topo();
    let me = std::process::id() as i32;
    let mut found = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return found };
    for e in rd.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else { continue };
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        let ppid = stat.rsplit(')').next().and_then(|r| r.split_whitespace().nth(1)).and_then(|p| p.parse::<i32>().ok()).unwrap_or(0);
        if ppid != me {
            continue;
        }
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default().trim().to_string();
        let all: Vec<usize> = (0..t.ncpu).collect();
        if comm.contains("Network") {
            set_affinity(pid, if t.little.is_empty() { &all } else { &t.little });
            unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as u32, 5) };
        } else if comm.contains("WebProcess") {
            set_affinity(pid, if saver && !t.little.is_empty() { &t.little } else { &all });
        }
        found.push((pid, comm));
    }
    found
}

/// Energiesparen nötig? (Akku unter 15 % und nicht am Laden, oder Gerät über 70 °C)
pub fn power_saver() -> bool {
    let mut low = false;
    if let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") {
        for e in rd.flatten() {
            let p = e.path();
            let cap: Option<u32> = std::fs::read_to_string(p.join("capacity")).ok().and_then(|s| s.trim().parse().ok());
            let st = std::fs::read_to_string(p.join("status")).unwrap_or_default();
            if let Some(c) = cap {
                if c < 15 && !st.contains("Charging") && !st.contains("Full") {
                    low = true;
                }
            }
        }
    }
    let hot = std::fs::read_dir("/sys/class/thermal")
        .map(|rd| {
            rd.flatten().any(|e| {
                std::fs::read_to_string(e.path().join("temp")).ok().and_then(|s| s.trim().parse::<i64>().ok()).is_some_and(|t| t > 70_000 && t < 150_000)
            })
        })
        .unwrap_or(false);
    low || hot
}

/// Mobile Daten statt WLAN aktiv? (dann wird nichts vorgeladen)
pub fn metered() -> bool {
    std::path::Path::new("/run/ultar/mobiledata.on").exists() && !std::fs::read_dir("/sys/class/net").map(|rd| rd.flatten().any(|e| {
        let n = e.file_name().to_string_lossy().to_string();
        n.starts_with("wlan") && std::fs::read_to_string(e.path().join("operstate")).unwrap_or_default().trim() == "up"
    })).unwrap_or(false)
}

/// CPU-Zeiten je Kern (für Auslastung in ultar://leistung)
pub fn cpu_times() -> Vec<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/stat").unwrap_or_default();
    s.lines()
        .filter(|l| l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(|c| c.is_ascii_digit()))
        .map(|l| {
            let v: Vec<u64> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
            let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
            (v.iter().sum(), idle)
        })
        .collect()
}

/// RSS (kB) und CPU-Zeit (Ticks) eines Prozesses
pub fn proc_usage(pid: i32) -> (u64, u64) {
    let st = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let rss = st.lines().find(|l| l.starts_with("VmRSS")).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse().ok()).unwrap_or(0);
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    let f: Vec<&str> = stat.rsplit(')').next().unwrap_or("").split_whitespace().collect();
    let cpu = f.get(11).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) + f.get(12).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    (rss, cpu)
}
