//! ultar-audio – Ton des A30 (Exynos-ABOX + COD3035 + SMA1301) ohne Android.
//!
//! Lautsprecher: eigener, am Gerät bestätigter Weg (RDMA0 → SIFS1 → UAIF3 → SMA1301).
//! Hörmuschel, Kopfhörer, Mikrofon und Gesprächston: Samsungs Wege aus /vendor/etc/mixer_paths.xml
//! (+ mixer_gains.xml), angewandt wie Androids audio_route. Der Gesprächston läuft im ABOX direkt
//! zwischen Modem und Tonchip; wir öffnen nur die Basisband-Strecken (pcmC0D4p / pcmC0D10c),
//! der RIL schaltet dazu den Modem-Takt und -Tonweg (SND_CLOCK_CTRL / SND_AUDIO_PATH_CTRL).
//!
//! Modi:
//!   --status                       ABOX-Firmware + Kernel-Log
//!   --tone [rdma] [hz]             Testton über den Lautsprecher
//!   --play <raw> [rdma]            16-bit/48k/Stereo-RAW
//!   --route [0..1]                 Medienweg für die Oberfläche (Kopfhörer, falls eingesteckt, sonst Lautsprecher)
//!   --out <speaker|handset|headset> [0..1] [hz]   Medienweg setzen (+ Testton, wenn hz angegeben)
//!   --rec <ms> [datei.wav] [mic|2nd|headset|dual]  Aufnahme prüfen (Pegel je Viertelsekunde)
//!   --call [handset|speaker|headset]  Gesprächston (läuft, bis /run/ultar/callaudio „off" enthält)
//!   --jack                         Kopfhörer-Zustand
//!   --list [text] | --get <name> | --set <name> <wert>   Regler
//!   --path <weg>... | --paths [text]                      Samsung-Wege anwenden / auflisten

mod codec;
mod codec_cod3035;
mod ctl;
mod jack;
mod mixer;
mod paths;
mod pcm;
mod route;
mod stream;

use route::Dev;

fn firmware_present() -> Vec<(String, bool)> {
    let names = ["calliope_sram.bin", "calliope_dram.bin"];
    let dirs = [
        "/vendor/firmware",
        "/lib/firmware",
        "/etc/firmware",
        "/firmware/image",
    ];
    names
        .iter()
        .map(|n| {
            (
                n.to_string(),
                dirs.iter()
                    .any(|d| std::path::Path::new(&format!("{d}/{n}")).exists()),
            )
        })
        .collect()
}

fn status() {
    println!("== ABOX/Calliope ==");
    for (n, ok) in firmware_present() {
        println!("  firmware {n}: {}", if ok { "gefunden" } else { "FEHLT" });
    }
    let v = std::fs::read_to_string("/sys/devices/platform/14a50000.abox/calliope_version")
        .unwrap_or_default();
    println!("  calliope_version = {}", v.trim());
    println!("  Kopfhörer: {}", jack::describe());
    println!("  Gesprächston läuft: {}", route::call_running());
    println!("== Kernel-Log (abox/uaif/calliope/rdma) ==");
    for l in pcm::klog_audio().iter().rev().take(25).rev() {
        println!("  {l}");
    }
}

fn play_to(rdma: u32, data: &[u8]) -> bool {
    match pcm::play(rdma, data) {
        Ok(()) => {
            println!("OK – Wiedergabe beendet (kein EIO).");
            true
        }
        Err(e) => {
            println!("FEHLER: {e}");
            for l in pcm::klog_audio().iter().rev().take(15).rev() {
                println!("  {l}");
            }
            false
        }
    }
}

fn rec(ms: u32, file: Option<&str>, src: &str) -> i32 {
    let e = route::mic(src, false);
    if e > 0 {
        println!("Aufnahmeweg: {e} Regler nicht gesetzt");
    }
    let r = stream::record(route::CAPTURE, ms);
    if let Some(c) = ctl::Ctl::open() {
        paths::Paths::load().reset(&c, false);
    }
    match r {
        Ok(s) => {
            let l: Vec<i16> = s.iter().step_by(2).copied().collect();
            let r: Vec<i16> = s.iter().skip(1).step_by(2).copied().collect();
            let q = stream::RATE as usize / 4;
            let lv: Vec<String> = l
                .chunks(q)
                .map(|c| format!("{:.0}", stream::rms(c)))
                .collect();
            let rv: Vec<String> = r
                .chunks(q)
                .map(|c| format!("{:.0}", stream::rms(c)))
                .collect();
            let peak = s.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
            println!(
                "Mikrofon ({src}): {} Rahmen, RMS L {:.1} R {:.1}, Spitze {peak}",
                l.len(),
                stream::rms(&l),
                stream::rms(&r)
            );
            println!("  L: {}", lv.join(" "));
            println!("  R: {}", rv.join(" "));
            if let Some(f) = file {
                match std::fs::write(f, stream::wav(&s)) {
                    Ok(()) => println!("  gespeichert: {f}"),
                    Err(e) => println!("  {f}: {e}"),
                }
            }
            if peak == 0 {
                println!("  nur Stille – Weg prüfen (--list MIC, --list NSRC)");
                return 2;
            }
            0
        }
        Err(e) => {
            println!("Mikrofon ({src}): {e}");
            for l in pcm::klog_audio().iter().rev().take(12).rev() {
                println!("  {l}");
            }
            1
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let arg = |i: usize| a.get(i).map(|s| s.as_str());
    let verbose = std::env::var("ULTAR_AUDIO_V").is_ok();
    let code = match arg(1) {
        Some("--status") => {
            status();
            0
        }
        Some("--tone") => {
            let rdma: u32 = arg(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let hz: f32 = arg(3).and_then(|s| s.parse().ok()).unwrap_or(440.0);
            route::media(Dev::Speaker, 0.8, verbose);
            if play_to(rdma, &pcm::tone(hz, 800, 0.8)) {
                0
            } else {
                1
            }
        }
        Some("--route") => {
            // Für die Oberfläche: Weg vor jedem Ton. Während eines Gesprächs nichts anfassen.
            let vol: f32 = arg(2).and_then(|s| s.parse().ok()).unwrap_or(0.8);
            if !route::call_running() {
                route::media(route::auto(Dev::Speaker), vol.clamp(0.0, 1.0), verbose);
            }
            0
        }
        Some("--out") => {
            let dev = arg(2).and_then(Dev::parse).unwrap_or(Dev::Speaker);
            let vol: f32 = arg(3).and_then(|s| s.parse().ok()).unwrap_or(0.8);
            let e = route::media(dev, vol, true);
            println!("Medienweg {} ({e} Fehler)", dev.key());
            match arg(4).and_then(|s| s.parse::<f32>().ok()) {
                Some(hz) => {
                    if play_to(0, &pcm::tone(hz, 1200, 0.8)) {
                        0
                    } else {
                        1
                    }
                }
                None => 0,
            }
        }
        Some("--beep") => {
            // Ton ohne Wegänderung (zum Prüfen eines vorher gesetzten Wegs)
            let hz: f32 = arg(2).and_then(|s| s.parse().ok()).unwrap_or(1000.0);
            let ms: u32 = arg(3).and_then(|s| s.parse().ok()).unwrap_or(1500);
            if play_to(0, &pcm::tone(hz, ms, 0.9)) {
                0
            } else {
                1
            }
        }
        Some("--rec") => {
            let ms: u32 = arg(2).and_then(|s| s.parse().ok()).unwrap_or(3000);
            rec(ms, arg(3).filter(|f| *f != "-"), arg(4).unwrap_or("mic"))
        }
        Some("--call") => route::call(arg(2).and_then(Dev::parse), verbose),
        Some("--call-off") => {
            let _ = std::fs::write(route::CALL_FILE, "off");
            0
        }
        Some("--jack") => {
            println!("{}", jack::describe());
            0
        }
        Some("--list") => match ctl::Ctl::open() {
            Some(c) => {
                c.dump(arg(2).unwrap_or(""));
                0
            }
            None => 1,
        },
        Some("--get") => match (ctl::Ctl::open(), arg(2)) {
            (Some(c), Some(n)) => {
                println!("{n} = {:?}", c.get(n));
                0
            }
            _ => 1,
        },
        Some("--set") => match (ctl::Ctl::open(), arg(2), arg(3)) {
            (Some(c), Some(n), Some(v)) => match c.set(n, v) {
                Ok(()) => 0,
                Err(e) => {
                    println!("{e}");
                    1
                }
            },
            _ => 1,
        },
        Some("--path") => match ctl::Ctl::open() {
            Some(c) => {
                let p = paths::Paths::load();
                let mut err = 0;
                let mut names = a[2..].iter().map(|s| s.as_str()).peekable();
                if names.peek() == Some(&"--keep") {
                    names.next();
                } else {
                    err += p.reset(&c, verbose);
                }
                for n in names {
                    println!("Weg {n}:");
                    err += p.apply_path(&c, n, true);
                }
                println!("{err} Fehler");
                (err > 0) as i32
            }
            None => 1,
        },
        Some("--paths") => {
            let p = paths::Paths::load();
            let f = arg(2).unwrap_or("");
            let mut v: Vec<&String> = p.paths.keys().filter(|k| k.contains(f)).collect();
            v.sort();
            for k in v {
                println!("{k}");
            }
            0
        }
        Some("--play") => match arg(2).map(std::fs::read) {
            Some(Ok(d)) => {
                let rdma: u32 = arg(3).and_then(|s| s.parse().ok()).unwrap_or(0);
                route::media(route::auto(Dev::Speaker), 0.8, verbose);
                if play_to(rdma, &d) {
                    0
                } else {
                    1
                }
            }
            _ => {
                eprintln!("--play <datei.raw> [rdma]");
                1
            }
        },
        _ => {
            println!("ultar-audio – Ton des A30");
            println!("  --status | --tone [rdma] [hz] | --play <raw> [rdma] | --route [0..1]");
            println!("  --out <speaker|handset|headset> [0..1] [hz]   Medienweg (+ Testton)");
            println!("  --rec <ms> [datei.wav|-] [mic|2nd|headset|dual]  Mikrofon prüfen");
            println!("  --call [handset|speaker|headset] | --call-off   Gesprächston");
            println!("  --jack | --list [text] | --get <n> | --set <n> <w> | --path [--keep] <weg>... | --paths [text]");
            0
        }
    };
    std::process::exit(code);
}
