//! ultar-compat – Ubuntu-Kompatibilitätsschicht.
//!
//! Das Kernsystem bleibt minimal (nur Rust-Binaries). Ubuntu-Pakete (.deb/apt)
//! leben in einem eigenen Ubuntu-Root (Standard: /compat) und werden in einem
//! privaten Mount-Namespace ausgeführt – gleicher Kernel, keine Virtualisierung,
//! daher praktisch ohne Leistungsverlust.
//!
//!   ultar-compat run [--root DIR] -- <programm> [args…]
//!   ultar-compat apt <apt-get-args…>      (z. B. apt install python3)
//!   ultar-compat dpkg -i /pfad/paket.deb  (Offline-Installation per USB)
//!   ultar-compat shell
//!   ultar-compat check
//!   ultar-compat modules                  (beim Start: gemerkte Treiber aus dem Ubuntu-Bereich laden)

use nix::mount::{mount, MsFlags};
use nix::sched::{unshare, CloneFlags};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

const DEFAULT_ROOT: &str = "/compat";

fn die(msg: &str) -> ! {
    eprintln!("ultar-compat: {msg}");
    std::process::exit(1)
}

fn bind(src: &str, root: &str, rec: bool) {
    if !Path::new(src).exists() {
        return;
    }
    let dst = format!("{root}{src}");
    let _ = std::fs::create_dir_all(&dst);
    let mut f = MsFlags::MS_BIND;
    if rec {
        f |= MsFlags::MS_REC;
    }
    if let Err(e) = mount(Some(src), dst.as_str(), None::<&str>, f, None::<&str>) {
        die(&format!("bind {src}: {e}"));
    }
}

fn enter(root: &str, argv: &[String]) -> ! {
    if !Path::new(&format!("{root}/usr/bin/dpkg")).exists() {
        die(&format!("{root} ist kein Ubuntu-Root (dpkg fehlt)"));
    }
    if argv.is_empty() {
        die("kein Programm angegeben");
    }
    unshare(CloneFlags::CLONE_NEWNS).unwrap_or_else(|e| die(&format!("unshare: {e}")));
    // Änderungen nicht zurück ins Host-System propagieren
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>,
    )
    .unwrap_or_else(|e| die(&format!("make-private: {e}")));
    bind("/dev", root, true);
    bind("/proc", root, true);
    bind("/sys", root, true);
    bind("/run", root, true);
    bind("/tmp", root, false);
    // Daten-Austausch mit dem Kernsystem (z. B. USB-Stick unter /media)
    bind("/media", root, true);
    bind("/data", root, true);
    // App-Installer des Kernsystems auch im Ubuntu-Bereich (Startmenü-Einträge „uapp run <id>“,
    // Doppelklick auf .exe/.msi/.apk/.deb im Desktop). Er erkennt dort ULTAR_IN_COMPAT=1.
    for (src, name) in [
        ("/usr/bin/uapp", "uapp"),
        ("/usr/bin/ultar-ctl", "ultar-ctl"),
    ] {
        let dst = format!("{root}/usr/local/bin/{name}");
        if Path::new(src).exists() && root != "/" {
            if !Path::new(&dst).exists() {
                let _ = std::fs::create_dir_all(format!("{root}/usr/local/bin"));
                let _ = std::fs::write(&dst, b"");
            }
            let _ = mount(
                Some(src),
                dst.as_str(),
                None::<&str>,
                MsFlags::MS_BIND,
                None::<&str>,
            );
        }
    }
    // DNS vom Netzwerkdienst (ultar-net) übernehmen
    for src in ["/run/ultar/resolv.conf", "/etc/resolv.conf"] {
        if Path::new(src).exists() {
            let _ = std::fs::copy(src, format!("{root}/etc/resolv.conf"));
            break;
        }
    }
    nix::unistd::chroot(root).unwrap_or_else(|e| die(&format!("chroot: {e}")));
    std::env::set_current_dir("/").ok();
    let err = Command::new(&argv[0])
        .args(&argv[1..])
        .env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        )
        .env("DEBIAN_FRONTEND", "noninteractive")
        .env(
            "LANG",
            std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".into()),
        )
        .env("ULTAR_IN_COMPAT", "1")
        .exec();
    die(&format!("exec {}: {err}", argv[0]))
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut root = std::env::var("ULTAR_COMPAT_ROOT").unwrap_or_else(|_| DEFAULT_ROOT.into());
    // Als /sbin/modprobe aufgerufen (vom Kernel, wenn er ein Modul nachladen will, z. B. WLAN-
    // Verschlüsselung): an modprobe im Ubuntu-Bereich weiterreichen, dort liegen die Modulpakete.
    if std::env::args()
        .next()
        .is_some_and(|a| a.ends_with("/modprobe") || a == "modprobe")
    {
        if !Path::new(&format!("{root}/usr/sbin/modprobe")).exists() {
            std::process::exit(1);
        }
        enter(
            &root,
            &[vec!["/usr/sbin/modprobe".to_string()], args].concat(),
        )
    }
    if args.len() >= 2 && args[0] == "--root" {
        root = args[1].clone();
        args.drain(..2);
    }
    let sub = args.first().cloned().unwrap_or_default();
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();
    match sub.as_str() {
        "run" => {
            let mut r = rest;
            if r.len() >= 2 && r[0] == "--root" {
                root = r[1].clone();
                r.drain(..2);
            }
            if r.first().map(|s| s == "--").unwrap_or(false) {
                r.remove(0);
            }
            enter(&root, &r)
        }
        "apt" => enter(
            &root,
            &[vec!["/usr/bin/apt-get".into(), "-y".into()], rest].concat(),
        ),
        "dpkg" => enter(&root, &[vec!["/usr/bin/dpkg".into()], rest].concat()),
        "shell" => enter(&root, &["/bin/bash".into(), "-l".into()]),
        "modules" => {
            // Beim Start: Firmware-Pfad setzen und gemerkte Treiber laden (siehe Oberfläche -> Hardware)
            // Hersteller-Pfad (/vendor/firmware, von ultar-init gesetzt) nicht überschreiben
            if std::fs::read_to_string("/sys/module/firmware_class/parameters/path")
                .map_or(true, |p| p.trim().is_empty())
            {
                let _ = std::fs::write(
                    "/sys/module/firmware_class/parameters/path",
                    format!("{root}/lib/firmware"),
                );
            }
            let list = std::env::var("ULTAR_DRIVER_LIST")
                .unwrap_or_else(|_| "/data/ultar/drivers.list".into());
            let aliases: Vec<String> = std::fs::read_to_string(list)
                .unwrap_or_default()
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect();
            if aliases.is_empty() || !Path::new(&format!("{root}/usr/sbin/modprobe")).exists() {
                return;
            }
            enter(
                &root,
                &[
                    vec![
                        "/usr/sbin/modprobe".into(),
                        "-a".into(),
                        "-q".into(),
                        "-b".into(),
                    ],
                    aliases,
                ]
                .concat(),
            )
        }
        "check" => {
            let ok = Path::new(&format!("{root}/usr/bin/dpkg")).exists();
            let rel = std::fs::read_to_string(format!("{root}/etc/os-release")).unwrap_or_default();
            let name = rel
                .lines()
                .find(|l| l.starts_with("PRETTY_NAME="))
                .unwrap_or("PRETTY_NAME=?")
                .trim_start_matches("PRETTY_NAME=");
            println!(
                "Compat-Root: {root}\nUbuntu: {name}\nbereit: {}",
                if ok { "ja" } else { "nein" }
            );
            std::process::exit(if ok { 0 } else { 1 })
        }
        _ => {
            eprintln!("Benutzung: ultar-compat [--root DIR] run -- <prog> [args] | apt <args> | dpkg <args> | shell | check");
            std::process::exit(2)
        }
    }
}
