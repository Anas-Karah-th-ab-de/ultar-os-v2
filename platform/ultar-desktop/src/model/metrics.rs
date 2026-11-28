//! The system display on the desktop background (`desktop.metrics.enabled`): processor load,
//! memory in use, GPU load and network traffic (download and upload), each with a short
//! history. Read straight from the kernel's files every [`SAMPLE_MS`] – `/proc/stat`,
//! `/proc/meminfo`, `/proc/net/dev`, the GPU's busy time (`/proc/ultar/gpu_busy_ns` on the
//! Ultar kernel, `gpu_busy_percent` of the DRM card or the Adreno's busy share on Linux).
//! What a device does not offer stays `None` and is shown as not measurable.

use std::path::PathBuf;

/// How often the values are read.
pub const SAMPLE_MS: u64 = 2000;
/// Samples kept per value (a minute).
pub const HISTORY: usize = 30;

/// One value of the display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gauge {
    /// Processor load.
    Cpu,
    /// Memory in use.
    Ram,
    /// GPU load.
    Gpu,
    /// Network traffic.
    Net,
}

impl Gauge {
    /// All values in the order they are shown.
    pub const ALL: [Gauge; 4] = [Gauge::Cpu, Gauge::Ram, Gauge::Gpu, Gauge::Net];

    /// Index into [`Metrics::history`].
    pub const fn index(self) -> usize {
        match self {
            Gauge::Cpu => 0,
            Gauge::Ram => 1,
            Gauge::Gpu => 2,
            Gauge::Net => 3,
        }
    }
}

/// The values as last read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metrics {
    /// Processor load in percent.
    pub cpu: Option<u8>,
    /// Memory in use and in all (bytes).
    pub ram: Option<(u64, u64)>,
    /// GPU load in percent.
    pub gpu: Option<u8>,
    /// Download and upload (bytes per second).
    pub net: Option<(u64, u64)>,
    /// History per gauge in percent (network: the download, of the largest rate in the
    /// window), oldest first.
    pub history: [Vec<u8>; 4],
    /// The upload's history (same scale as the network's).
    pub upload: Vec<u8>,
}

impl Metrics {
    /// The share shown for a gauge now, in percent (`None`: not measurable here).
    pub fn percent(&self, gauge: Gauge) -> Option<u8> {
        match gauge {
            Gauge::Cpu => self.cpu,
            Gauge::Gpu => self.gpu,
            Gauge::Ram => self
                .ram
                .filter(|(_, total)| *total > 0)
                .map(|(used, total)| (used.saturating_mul(100) / total).min(100) as u8),
            Gauge::Net => self.history[3].last().copied(),
        }
    }
}

/// Busy and total time of all processors (first line of `/proc/stat`).
pub fn cpu_times(stat: &str) -> Option<(u64, u64)> {
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let values: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .filter_map(|v| v.parse().ok())
        .collect();
    if values.len() < 4 {
        return None;
    }
    let total: u64 = values.iter().sum();
    let idle = values[3] + values.get(4).copied().unwrap_or(0);
    Some((total.saturating_sub(idle), total))
}

/// Memory in use and in all, in bytes (`MemTotal` − `MemAvailable`).
pub fn memory(meminfo: &str) -> Option<(u64, u64)> {
    let field = |name: &str| -> Option<u64> {
        meminfo
            .lines()
            .find(|l| l.starts_with(name))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    let total = field("MemTotal:")?;
    let available = field("MemAvailable:").or_else(|| field("MemFree:"))?;
    (total > 0).then(|| (total.saturating_sub(available) * 1024, total * 1024))
}

/// Bytes received and sent by all interfaces but the loopback (`/proc/net/dev`).
pub fn traffic(netdev: &str) -> Option<(u64, u64)> {
    let mut sum = None;
    for line in netdev.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" || name.contains('|') {
            continue;
        }
        let v: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        if v.len() >= 9 {
            let (rx, tx) = sum.unwrap_or((0, 0));
            sum = Some((rx + v[0], tx + v[8]));
        }
    }
    sum
}

/// Where the GPU's load is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpuSource {
    /// Cumulative busy time in ns (the Ultar kernel).
    BusyNs,
    /// A share in percent (Linux: amdgpu, Adreno).
    Percent,
}

const GPU_FILES: [(&str, GpuSource); 3] = [
    ("proc/ultar/gpu_busy_ns", GpuSource::BusyNs),
    (
        "sys/class/drm/card0/device/gpu_busy_percent",
        GpuSource::Percent,
    ),
    (
        "sys/class/kgsl/kgsl-3d0/gpu_busy_percentage",
        GpuSource::Percent,
    ),
];

/// Reads the values and keeps what the next reading needs (the counters of the last one).
#[derive(Debug)]
pub struct Sampler {
    root: PathBuf,
    cpu: Option<(u64, u64)>,
    gpu: Option<(u64, u64)>,
    net: Option<(u64, u64, u64)>,
    /// The raw network rates of the window (bytes per second).
    rates: Vec<(u64, u64)>,
}

impl Sampler {
    /// A sampler of the files under `root` (`/` on a device; a folder in the tests).
    pub fn new(root: impl Into<PathBuf>) -> Sampler {
        Sampler {
            root: root.into(),
            cpu: None,
            gpu: None,
            net: None,
            rates: Vec::new(),
        }
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(rel)).ok()
    }

    /// Reads every value at `now_ms` into `m` (a rate needs two readings: the first one
    /// leaves it unknown).
    pub fn sample(&mut self, now_ms: u64, m: &mut Metrics) {
        let times = self.read("proc/stat").as_deref().and_then(cpu_times);
        m.cpu = match (self.cpu, times) {
            (Some(a), Some(b)) if b.1 > a.1 => {
                Some(((b.0.saturating_sub(a.0)) * 100 / (b.1 - a.1)).min(100) as u8)
            }
            _ => None,
        };
        self.cpu = times;
        m.ram = self.read("proc/meminfo").as_deref().and_then(memory);
        m.gpu = self.gpu(now_ms);
        m.net = self.net(now_ms);
        push(&mut m.history[0], m.cpu);
        let ram = m.percent(Gauge::Ram);
        push(&mut m.history[1], ram);
        push(&mut m.history[2], m.gpu);
        self.net_history(m);
    }

    fn gpu(&mut self, now_ms: u64) -> Option<u8> {
        let (rel, source) = GPU_FILES
            .iter()
            .find(|(rel, _)| self.root.join(rel).exists())?;
        let value: u64 = self.read(rel)?.trim().parse().ok()?;
        match source {
            GpuSource::Percent => Some(value.min(100) as u8),
            GpuSource::BusyNs => {
                let last = self.gpu.replace((value, now_ms));
                let (before, at) = last?;
                let wall_ns = now_ms.checked_sub(at).filter(|d| *d > 0)? * 1_000_000;
                Some((value.saturating_sub(before) * 100 / wall_ns).min(100) as u8)
            }
        }
    }

    fn net(&mut self, now_ms: u64) -> Option<(u64, u64)> {
        let (rx, tx) = self.read("proc/net/dev").as_deref().and_then(traffic)?;
        let last = self.net.replace((rx, tx, now_ms));
        let (rx0, tx0, at) = last?;
        let ms = now_ms.checked_sub(at).filter(|d| *d > 0)?;
        Some((
            rx.saturating_sub(rx0) * 1000 / ms,
            tx.saturating_sub(tx0) * 1000 / ms,
        ))
    }

    /// The network's history on the scale of the largest rate of the window.
    fn net_history(&mut self, m: &mut Metrics) {
        let Some((down, up)) = m.net else {
            return;
        };
        self.rates.push((down, up));
        if self.rates.len() > HISTORY {
            self.rates.remove(0);
        }
        let max = self
            .rates
            .iter()
            .map(|(d, u)| (*d).max(*u))
            .max()
            .unwrap_or(0)
            .max(1);
        m.history[3] = self
            .rates
            .iter()
            .map(|(d, _)| (d * 100 / max) as u8)
            .collect();
        m.upload = self
            .rates
            .iter()
            .map(|(_, u)| (u * 100 / max) as u8)
            .collect();
    }
}

/// Adds a sample (an unknown value adds nothing) and keeps the last [`HISTORY`].
fn push(history: &mut Vec<u8>, value: Option<u8>) {
    if let Some(v) = value {
        history.push(v);
        if history.len() > HISTORY {
            history.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kernels_files_are_read() {
        let stat = "cpu  300 0 100 600 0 0 0 0 0 0\ncpu0 1 2 3 4\n";
        assert_eq!(cpu_times(stat), Some((400, 1000)));
        assert_eq!(cpu_times("nothing"), None);
        let mem = "MemTotal:  16000 kB\nMemFree: 1000 kB\nMemAvailable:  4000 kB\n";
        assert_eq!(memory(mem), Some((12000 * 1024, 16000 * 1024)));
        let ultar = "MemTotal: 8 kB\nMemFree: 2 kB\n";
        assert_eq!(
            memory(ultar),
            Some((6 * 1024, 8 * 1024)),
            "MemFree without MemAvailable"
        );
        let dev = "Inter-| Receive |Transmit\n face |bytes packets|bytes\n    lo: 5 1 0 0 0 0 0 0 5 1 0 0 0 0 0 0\n  eth0: 1000 10 0 0 0 0 0 0 200 3 0 0 0 0 0 0\n wlan0: 24 1 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n";
        assert_eq!(traffic(dev), Some((1024, 200)));
        assert_eq!(traffic("no table"), None);
    }

    #[test]
    fn two_readings_give_loads_and_rates() {
        let dir = std::env::temp_dir().join(format!("desk-metrics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("proc/ultar")).unwrap();
        std::fs::create_dir_all(dir.join("proc/net")).unwrap();
        let write = |rel: &str, text: &str| std::fs::write(dir.join(rel), text).unwrap();
        write("proc/stat", "cpu  100 0 0 900 0 0 0 0 0 0\n");
        write("proc/meminfo", "MemTotal: 1000 kB\nMemAvailable: 250 kB\n");
        write("proc/net/dev", "  eth0: 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n");
        write("proc/ultar/gpu_busy_ns", "0\n");
        let mut s = Sampler::new(&dir);
        let mut m = Metrics::default();
        s.sample(0, &mut m);
        assert_eq!(
            (m.cpu, m.gpu, m.net),
            (None, None, None),
            "rates need two readings"
        );
        assert_eq!(m.percent(Gauge::Ram), Some(75));
        write("proc/stat", "cpu  150 0 0 950 0 0 0 0 0 0\n");
        write(
            "proc/net/dev",
            "  eth0: 4000 0 0 0 0 0 0 0 1000 0 0 0 0 0 0 0\n",
        );
        write("proc/ultar/gpu_busy_ns", "500000000\n");
        s.sample(2000, &mut m);
        assert_eq!(m.cpu, Some(50));
        assert_eq!(m.gpu, Some(25), "0.5 s busy in 2 s");
        assert_eq!(m.net, Some((2000, 500)));
        assert_eq!(m.history[0], vec![50]);
        assert_eq!(m.history[1], vec![75, 75]);
        assert_eq!(m.history[3], vec![100]);
        assert_eq!(m.upload, vec![25]);
        // A device without the files: nothing measurable.
        let mut none = Metrics::default();
        Sampler::new(dir.join("missing")).sample(0, &mut none);
        assert_eq!(none, Metrics::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
