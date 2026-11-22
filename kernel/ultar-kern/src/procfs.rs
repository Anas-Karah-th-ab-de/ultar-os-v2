//! /proc – Prozess- und Systemangaben im Linux-Format (für ps, free, uptime, top …)

use crate::fs::{DirEnt, Node};
use crate::proc::{self, State};
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

fn file(s: String) -> Option<Node> {
    Some(Node::File(Arc::new(s.into_bytes())))
}

fn state_char(s: State) -> char {
    match s {
        State::Ready | State::Running(_) => 'R',
        State::Blocked(_) => 'S',
        State::Zombie(_) | State::Exiting | State::Dead => 'Z',
    }
}

pub fn open(path: &str) -> Option<Node> {
    let rest = path.strip_prefix("/proc").unwrap_or("").trim_start_matches('/');
    let mut it = rest.splitn(2, '/');
    let first = it.next().unwrap_or("");
    let sub = it.next().unwrap_or("");
    let (total, _, _, _) = crate::mem::stats();
    let free = crate::mem::free_bytes();
    let up = crate::arch::uptime_ms();
    match first {
        "" => {
            let mut v: Vec<DirEnt> = ["meminfo", "uptime", "cpuinfo", "version", "loadavg", "mounts", "stat", "self", "ultar"]
                .iter()
                .map(|n| DirEnt { name: String::from(*n), dir: *n == "self" || *n == "ultar", size: 0 })
                .collect();
            for p in proc::infos() {
                v.push(DirEnt { name: format!("{}", p.pid), dir: true, size: 0 });
            }
            Some(Node::Dir(v))
        }
        "ultar" => match sub {
            "" => Some(Node::Dir(["verbund", "firewall", "sicherheit", "geraet", "anzeige", "protokoll"].iter().map(|n| DirEnt { name: String::from(*n), dir: false, size: 0 }).collect())),
            "verbund" => file(crate::verbund::text()),
            "geraet" => file(crate::geraet::text()),
            "anzeige" => file(crate::grafik::text()),
            "firewall" => file(crate::firewall::text()),
            "sicherheit" => file(crate::sicherheit_text()),
            // Kernprotokoll (Ringpuffer) – für den Entwickler-Zugang (`ssh ultar kernlog`) und Fehlersuche
            "protokoll" => {
                let mut v = alloc::vec::Vec::new();
                crate::klog::dump(|b| v.extend_from_slice(b));
                file(String::from_utf8_lossy(&v).into_owned())
            }
            _ => None,
        },
        "meminfo" => file(format!(
            "MemTotal:       {:>8} kB\nMemFree:        {:>8} kB\nMemAvailable:   {:>8} kB\nBuffers:               0 kB\nCached:                0 kB\nSwapCached:            0 kB\nSwapTotal:             0 kB\nSwapFree:              0 kB\nShmem:                 0 kB\n",
            total >> 10,
            free >> 10,
            free >> 10
        )),
        "uptime" => file(format!("{}.{:02} {}.{:02}\n", up / 1000, up % 1000 / 10, up / 1000, up % 1000 / 10)),
        "loadavg" => file(format!("0.00 0.00 0.00 1/{} {}\n", proc::infos().len(), proc::current())),
        "version" => file(String::from("Linux version 6.8.0-ultar-kern (Ultar-Kern 0.4, KM Code Craft) #1\n")),
        "mounts" => {
            let mut s = String::new();
            for (p, t) in crate::fs::mounts() {
                s += &format!("ultar {p} {t} ro 0 0\n");
            }
            s += "proc /proc proc rw 0 0\nram / ramfs rw 0 0\n";
            file(s)
        }
        "stat" => {
            let t = crate::apic::ticks();
            file(format!("cpu  {t} 0 0 0 0 0 0 0 0 0\nbtime 0\nprocesses {}\n", proc::infos().len()))
        }
        "cpuinfo" => {
            let mut nb = [0u8; 48];
            let name = String::from(crate::arch::cpu_name(&mut nb));
            let (on, _) = crate::apic::cpu_count();
            let mut s = String::new();
            for i in 0..on {
                s += &format!("processor\t: {i}\nvendor_id\t: x86_64\nmodel name\t: {name}\ncpu MHz\t\t: {}\n\n", crate::arch::tsc_khz() / 1000);
            }
            file(s)
        }
        pid => {
            let pid: u64 = if pid == "self" { proc::current() } else { pid.parse().ok()? };
            let p = proc::infos().into_iter().find(|p| p.pid == pid)?;
            match sub {
                "auxv" => Some(Node::File(alloc::sync::Arc::new(p.auxv.clone()))),
                "" => Some(Node::Dir(["stat", "cmdline", "status", "comm", "exe", "auxv"].iter().map(|n| DirEnt { name: String::from(*n), dir: false, size: 0 }).collect())),
                "stat" => file(format!(
                    "{} ({}) {} {} {} {} 0 0 0 0 0 0 0 0 0 0 0 20 0 1 0 0 {} {} 0\n",
                    p.pid,
                    p.name.rsplit('/').next().unwrap_or(""),
                    state_char(p.state),
                    p.ppid,
                    p.pgid,
                    p.pgid,
                    p.pages * 4096,
                    p.pages
                )),
                "cmdline" => {
                    let mut s = String::new();
                    for a in &p.cmdline {
                        s += a;
                        s.push('\0');
                    }
                    file(s)
                }
                "comm" => file(format!("{}\n", p.name.rsplit('/').next().unwrap_or(""))),
                "exe" => crate::fs::open(&p.exe),
                "status" => file(format!(
                    "Name:\t{}\nState:\t{}\nPid:\t{}\nPPid:\t{}\nUid:\t0\t0\t0\t0\nGid:\t0\t0\t0\t0\nVmRSS:\t{} kB\n",
                    p.name.rsplit('/').next().unwrap_or(""),
                    state_char(p.state),
                    p.pid,
                    p.ppid,
                    p.pages * 4
                )),
                _ => None,
            }
        }
    }
}
