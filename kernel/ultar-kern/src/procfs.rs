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
    // kernel log, firewall and mesh state belong to the machine: only readers in the initial pid namespace see them
    let host_reader = proc::reader_in_initial_pid_ns();
    match first {
        "" => {
            let mut v: Vec<DirEnt> = ["meminfo", "uptime", "cpuinfo", "version", "loadavg", "mounts", "stat", "cmdline", "self", "sys", "ultar"]
                .iter()
                .filter(|n| host_reader || **n != "ultar")
                .map(|n| DirEnt { name: String::from(*n), dir: *n == "self" || *n == "ultar" || *n == "sys", size: 0 })
                .collect();
            for p in proc::infos() {
                v.push(DirEnt { name: format!("{}", p.pid), dir: true, size: 0 });
            }
            Some(Node::Dir(v))
        }
        "ultar" if !host_reader => None,
        "ultar" => match sub {
            "" => Some(Node::Dir(["verbund", "firewall", "sicherheit", "geraet", "anzeige", "protokoll", "gruppen", "boot_slot"].iter().map(|n| DirEnt { name: String::from(*n), dir: false, size: 0 }).collect())),
            "verbund" => file(crate::verbund::text()),
            "geraet" => file(crate::geraet::text()),
            "anzeige" => file(crate::grafik::text()),
            "firewall" => file(crate::firewall::text()),
            // resource groups: the tree with use and limits, the dynamic rules, leak counters
            "gruppen" => file(crate::groups::files::table()),
            "sicherheit" => file(crate::sicherheit_text()),
            // A/B start: the running slot (a, b, or - without the A/B loader)
            "boot_slot" => file(crate::bootslot::proc_text()),
            // Kernprotokoll (Ringpuffer) – für den Entwickler-Zugang (`ssh ultar kernlog`) und Fehlersuche
            "protokoll" => {
                let mut v = alloc::vec::Vec::new();
                crate::klog::dump(|b| v.extend_from_slice(b));
                file(String::from_utf8_lossy(&v).into_owned())
            }
            _ => None,
        },
        // kernel settings in the Linux layout: only the largest pipe size (see pipe.rs)
        "sys" => match sub {
            "" => Some(Node::Dir(alloc::vec![DirEnt { name: String::from("fs"), dir: true, size: 0 }])),
            "fs" => Some(Node::Dir(alloc::vec![DirEnt { name: String::from("pipe-max-size"), dir: false, size: 0 }])),
            "fs/pipe-max-size" => file(format!("{}\n", crate::pipe::max_size())),
            _ => None,
        },
        "meminfo" => file(format!(
            "MemTotal:       {:>8} kB\nMemFree:        {:>8} kB\nMemAvailable:   {:>8} kB\nBuffers:               0 kB\nCached:                0 kB\nSwapCached:            0 kB\nSwapTotal:             0 kB\nSwapFree:              0 kB\nShmem:                 0 kB\n",
            total >> 10,
            free >> 10,
            free >> 10
        )),
        // kernel arguments for user space: the installer session names its profile (ultar-init)
        "cmdline" if sub.is_empty() => file(format!("{}\n", if crate::install::sitzung_aktiv() { "ultar.profile=installer" } else { "" })),
        "uptime" => file(format!("{}.{:02} {}.{:02}\n", up / 1000, up % 1000 / 10, up / 1000, up % 1000 / 10)),
        "loadavg" => {
            let (all, own) = (proc::infos(), proc::current_group());
            file(format!("0.00 0.00 0.00 1/{} {}\n", all.len(), all.iter().find(|p| p.global == own).map(|p| p.pid).unwrap_or(0)))
        }
        "version" => file(String::from(concat!("Linux version 6.8.0-ultar-kern (Ultar-Kern 0.4, ", ultar_brand::vendor!(), ") #1\n"))),
        "mounts" => {
            let mut s = String::new();
            // a process in its own mount namespace reads that namespace's table
            if let Some(ns) = proc::reader_mount_ns() {
                for (at, fstype, ro) in ns.table() {
                    s += &format!("{fstype} {at} {fstype} {} 0 0\n", if ro { "ro" } else { "rw" });
                }
                return file(s);
            }
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
            // numbers are those of the reader's pid namespace; processes outside it do not exist here
            let p = if pid == "self" {
                let own = proc::current_group();
                proc::infos().into_iter().find(|p| p.global == own)?
            } else {
                let pid: u64 = pid.parse().ok()?;
                proc::infos().into_iter().find(|p| p.pid == pid)?
            };
            match sub {
                "ns" => Some(Node::Dir(["mnt", "pid"].iter().map(|n| DirEnt { name: String::from(*n), dir: false, size: 0 }).collect())),
                "ns/mnt" | "ns/pid" => file(ns_name(p.global, &sub[3..])?),
                "auxv" => Some(Node::File(alloc::sync::Arc::new(p.auxv.clone()))),
                // resource group in the Linux form; a reader inside a pid namespace (a cell) only learns
                // that it is in "its" group, not where that lies in the machine's tree
                "cgroup" => file(if host_reader { format!("0::{}\n", crate::groups::files::path_of(p.res)) } else { String::from("0::/\n") }),
                "" => Some(Node::Dir(["stat", "cmdline", "status", "comm", "exe", "auxv", "cgroup", "ns"].iter().map(|n| DirEnt { name: String::from(*n), dir: *n == "ns", size: 0 }).collect())),
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

/// Name of a namespace as Linux prints it (`mnt:[4]`), for the group with the kernel-wide number `tgid`.
pub fn ns_name(tgid: u64, kind: &str) -> Option<String> {
    Some(match proc::ns_ref(tgid, kind)? {
        crate::isolation::NsRef::Mnt(m) => format!("mnt:[{}]", m.id()),
        crate::isolation::NsRef::Pid(n) => format!("pid:[{}]", n.id()),
    })
}

/// If the host path is `/proc/<pid>/ns/<kind>` or `/proc/<pid>/exe` of a process the reader can see:
/// (kernel-wide group number, entry name).
pub fn pid_entry(path: &str) -> Option<(u64, &str)> {
    let rest = path.strip_prefix("/proc/")?;
    let (pid, entry) = rest.split_once('/')?;
    if pid != "self" && pid.parse::<u64>().is_err() {
        return None;
    }
    let own = proc::current_group();
    let infos = proc::infos();
    let p = if pid == "self" { infos.iter().find(|p| p.global == own)? } else { infos.iter().find(|p| Some(p.pid) == pid.parse().ok())? };
    Some((p.global, entry))
}
