//! Mehrere Grafikkarten sinnvoll nutzen.
//!
//! * **auto** (Standard): beim Start einer App die Karte mit dem meisten freien Grafikspeicher und der
//!   geringsten Auslastung – zwei gleichzeitige Rechenaufgaben landen so auf verschiedenen Karten.
//! * **alle**: alle Karten sind sichtbar; Programme, die Arbeit aufteilen können (KI-Modelle, Rendering,
//!   Rechnen über CUDA/ROCm/Vulkan), nutzen sie zusammen – Aufteilung nach Grafikspeicher (z. B. 24 + 12 GB → 2:1).
//! * **<Nummer>**: fest diese Karte (in den Einstellungen gewählt).
//! * **aus**: keine Grafikkarte (z. B. für eine misstrauische App).
//!
//! Ehrlich: Zwei Karten lassen sich nicht zu *einer* für ein Spiel/Bild zusammenschalten (SLI/CrossFire gibt es unter
//! Linux nicht mehr). Summiert wird Rechenleistung und Grafikspeicher dort, wo das Programm die Arbeit aufteilt.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Karte {
    pub nr: usize,
    pub hersteller: String,
    pub treiber: String,
    pub pci: String,
    pub name: String,
    /// /dev/dri/renderD…
    pub render: String,
    pub vram_mb: u64,
    pub vram_frei_mb: u64,
    pub last: u32,
    /// Index in CUDA (NVIDIA) bzw. ROCm (AMD) – Reihenfolge je Hersteller
    pub index_hersteller: usize,
}

fn lies(p: &Path) -> Option<String> {
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Karten aus /sys/class/drm (ULTAR_SYS für Tests) und nvidia-smi
pub fn karten() -> Vec<Karte> {
    let sys = std::env::var("ULTAR_SYS").unwrap_or_else(|_| "/sys".into());
    let drm = PathBuf::from(&sys).join("class/drm");
    let mut v = Vec::new();
    let mut eintraege: Vec<_> = std::fs::read_dir(&drm)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    eintraege.sort();
    for n in eintraege
        .iter()
        .filter(|n| n.starts_with("card") && n[4..].chars().all(|c| c.is_ascii_digit()))
    {
        let dev = drm.join(n).join("device");
        let vendor = lies(&dev.join("vendor")).unwrap_or_default();
        let hersteller = match vendor.as_str() {
            "0x10de" => "nvidia",
            "0x1002" => "amd",
            "0x8086" => "intel",
            _ => "andere",
        }
        .to_string();
        let treiber = std::fs::read_link(dev.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let pci = std::fs::read_link(&dev)
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let render = std::fs::read_dir(dev.join("drm"))
            .ok()
            .and_then(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .find(|f| f.starts_with("renderD"))
            })
            .map(|r| format!("/dev/dri/{r}"))
            .unwrap_or_default();
        let mb = |f: &str| {
            lies(&dev.join(f))
                .and_then(|s| s.parse::<u64>().ok())
                .map(|b| b >> 20)
        };
        let vram_mb = mb("mem_info_vram_total").unwrap_or(0);
        let vram_frei_mb = vram_mb.saturating_sub(mb("mem_info_vram_used").unwrap_or(0));
        let last = lies(&dev.join("gpu_busy_percent"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let index_hersteller = v
            .iter()
            .filter(|k: &&Karte| k.hersteller == hersteller)
            .count();
        v.push(Karte {
            nr: v.len(),
            name: format!("{hersteller} {pci}"),
            hersteller,
            treiber,
            pci,
            render,
            vram_mb,
            vram_frei_mb,
            last,
            index_hersteller,
        });
    }
    // NVIDIA: Speicher und Auslastung über nvidia-smi (sysfs zeigt sie nicht)
    if v.iter().any(|k| k.hersteller == "nvidia") {
        if let Ok(o) = std::process::Command::new(
            std::env::var("ULTAR_NVIDIA_SMI").unwrap_or_else(|_| "nvidia-smi".into()),
        )
        .args([
            "--query-gpu=index,name,memory.total,memory.used,utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        {
            for z in String::from_utf8_lossy(&o.stdout).lines() {
                let f: Vec<&str> = z.split(',').map(|s| s.trim()).collect();
                if f.len() < 5 {
                    continue;
                }
                let i: usize = f[0].parse().unwrap_or(99);
                if let Some(k) = v.iter_mut().filter(|k| k.hersteller == "nvidia").nth(i) {
                    k.name = f[1].to_string();
                    k.vram_mb = f[2].parse().unwrap_or(0);
                    k.vram_frei_mb = k.vram_mb.saturating_sub(f[3].parse().unwrap_or(0));
                    k.last = f[4].parse().unwrap_or(0);
                }
            }
        }
    }
    v
}

/// globale Vorgabe (/etc/ultar/gpu.toml oder <wurzel>/gpu.toml): modus = "auto" | "alle" | "aus" | Nummer
pub fn vorgabe() -> String {
    for p in [
        crate::wurzel().join("gpu.toml"),
        PathBuf::from("/etc/ultar/gpu.toml"),
    ] {
        if let Some(s) = lies(&p) {
            if let Some(m) = s.lines().find_map(|l| {
                l.trim().strip_prefix("modus").map(|r| {
                    r.trim_start_matches([' ', '='])
                        .trim()
                        .trim_matches('"')
                        .to_string()
                })
            }) {
                return m;
            }
        }
    }
    "auto".into()
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Wahl {
    pub karten: Vec<usize>,
    pub umgebung: Vec<(String, String)>,
    /// Gerätedateien, die die Zelle sehen darf
    pub geraete: Vec<String>,
    pub begruendung: String,
}

/// Karten für eine App wählen und die passende Umgebung bauen
pub fn waehlen(modus: &str, k: &[Karte]) -> Wahl {
    let modus = if modus.is_empty() { "auto" } else { modus };
    if k.is_empty() || modus == "aus" {
        return Wahl {
            begruendung: if k.is_empty() {
                "keine Grafikkarte".into()
            } else {
                "Grafik aus".into()
            },
            ..Default::default()
        };
    }
    let gewaehlt: Vec<usize> = match modus {
        "alle" => (0..k.len()).collect(),
        "auto" => {
            // meister freier Grafikspeicher, dann geringste Auslastung
            let mut b = k.to_vec();
            b.sort_by(|a, c| {
                c.vram_frei_mb
                    .cmp(&a.vram_frei_mb)
                    .then(a.last.cmp(&c.last))
                    .then(a.nr.cmp(&c.nr))
            });
            vec![b[0].nr]
        }
        n => match n.parse::<usize>() {
            Ok(i) if i < k.len() => vec![i],
            _ => vec![0],
        },
    };
    let mut w = Wahl {
        karten: gewaehlt.clone(),
        ..Default::default()
    };
    let sel: Vec<&Karte> = gewaehlt.iter().map(|i| &k[*i]).collect();
    // Rechnen: CUDA/ROCm sehen nur die gewählten Karten
    let nv: Vec<String> = sel
        .iter()
        .filter(|c| c.hersteller == "nvidia")
        .map(|c| c.index_hersteller.to_string())
        .collect();
    let amd: Vec<String> = sel
        .iter()
        .filter(|c| c.hersteller == "amd")
        .map(|c| c.index_hersteller.to_string())
        .collect();
    w.umgebung
        .push(("CUDA_VISIBLE_DEVICES".into(), nv.join(",")));
    if !amd.is_empty() {
        w.umgebung
            .push(("ROCR_VISIBLE_DEVICES".into(), amd.join(",")));
        w.umgebung
            .push(("HIP_VISIBLE_DEVICES".into(), amd.join(",")));
    }
    if sel.len() > 1 {
        // KI-Modelle (llama.cpp, ultar-llm) teilen Schichten nach Grafikspeicher auf
        let anteile: Vec<String> = sel
            .iter()
            .map(|c| (c.vram_mb.max(1) / 1024).max(1).to_string())
            .collect();
        w.umgebung
            .push(("LLAMA_ARG_TENSOR_SPLIT".into(), anteile.join(",")));
        w.umgebung
            .push(("ULTAR_GPU_ANTEILE".into(), anteile.join(",")));
        w.begruendung = format!(
            "alle {} Karten gemeinsam (Aufteilung {})",
            sel.len(),
            anteile.join(":")
        );
    } else {
        let c = sel[0];
        // Bildausgabe/OpenGL/Vulkan auf dieser Karte (Mesa: DRI_PRIME, NVIDIA: PRIME-Offload)
        if c.hersteller == "nvidia" {
            w.umgebung
                .push(("__NV_PRIME_RENDER_OFFLOAD".into(), "1".into()));
            w.umgebung
                .push(("__GLX_VENDOR_LIBRARY_NAME".into(), "nvidia".into()));
            w.umgebung
                .push(("__VK_LAYER_NV_optimus".into(), "NVIDIA_only".into()));
        } else if !c.pci.is_empty() {
            w.umgebung.push((
                "DRI_PRIME".into(),
                format!("pci-{}", c.pci.replace([':', '.'], "_")),
            ));
        }
        if w.begruendung.is_empty() {
            w.begruendung = match modus {
                "auto" => format!(
                    "Karte {} ({}): {} MB frei, {} % belegt",
                    c.nr, c.name, c.vram_frei_mb, c.last
                ),
                _ => format!("fest Karte {} ({})", c.nr, c.name),
            };
        }
    }
    for c in &sel {
        if !c.render.is_empty() {
            w.geraete.push(c.render.clone());
        }
        if c.hersteller == "nvidia" {
            w.geraete.push(format!("/dev/nvidia{}", c.index_hersteller));
        }
    }
    if sel.iter().any(|c| c.hersteller == "nvidia") {
        for d in [
            "/dev/nvidiactl",
            "/dev/nvidia-uvm",
            "/dev/nvidia-uvm-tools",
            "/dev/nvidia-modeset",
        ] {
            w.geraete.push(d.into());
        }
    }
    if sel.iter().any(|c| c.hersteller == "amd") {
        w.geraete.push("/dev/kfd".into());
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    fn k(nr: usize, h: &str, ih: usize, vram: u64, frei: u64, last: u32) -> Karte {
        Karte {
            nr,
            hersteller: h.into(),
            treiber: String::new(),
            pci: format!("0000:0{nr}:00.0"),
            name: format!("{h}{nr}"),
            render: format!("/dev/dri/renderD{}", 128 + nr),
            vram_mb: vram,
            vram_frei_mb: frei,
            last,
            index_hersteller: ih,
        }
    }
    #[test]
    fn verteilen() {
        let ks = vec![
            k(0, "nvidia", 0, 24576, 2000, 90),
            k(1, "nvidia", 1, 12288, 11000, 5),
        ];
        let a = waehlen("auto", &ks);
        assert_eq!(a.karten, vec![1], "freiere Karte");
        assert!(a
            .umgebung
            .contains(&("CUDA_VISIBLE_DEVICES".into(), "1".into())));
        let b = waehlen("alle", &ks);
        assert_eq!(b.karten, vec![0, 1]);
        assert!(b
            .umgebung
            .contains(&("LLAMA_ARG_TENSOR_SPLIT".into(), "24,12".into())));
        assert!(b.geraete.contains(&"/dev/nvidiactl".to_string()));
        assert_eq!(waehlen("0", &ks).karten, vec![0]);
        assert!(waehlen("aus", &ks).karten.is_empty());
        let gemischt = vec![k(0, "intel", 0, 0, 0, 0), k(1, "amd", 0, 16384, 16000, 0)];
        let g = waehlen("auto", &gemischt);
        assert_eq!(g.karten, vec![1]);
        assert!(g
            .umgebung
            .iter()
            .any(|(a, b)| a == "DRI_PRIME" && b == "pci-0000_01_00_0"));
        assert!(g.geraete.contains(&"/dev/kfd".to_string()));
    }
}
