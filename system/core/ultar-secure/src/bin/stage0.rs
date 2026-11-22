//! ultar-stage0 — winziger PID 1 für verifizierten Start (dm-verity), dann switch_root in den
//! echten ultar-init. Als /init ins Image legen; echter Init liegt unter /sbin/ultar-init.
//!
//! Ablauf:
//!   1. /proc /sys /dev sicherstellen
//!   2. /ultar/verity.conf lesen
//!   3. keine conf  -> noch nicht bereitgestellt: direkt echten Init starten (Gerät bootet immer)
//!   4. conf da      -> veritysetup open; Hash falsch => START VERWEIGERN (Manipulation)
//!   5. Verity-Gerät read-only nach /newroot mounten, Mounts verschieben, switch_root, exec init
//!
//! Sicherheitslektion des Projekts: das Gerät muss immer booten. Nur echte Manipulation
//! (conf vorhanden, aber Hash passt nicht) hält an — fehlende Bereitstellung nicht.

use nix::mount::{mount, MsFlags};
use nix::unistd::{chdir, chroot, execv};
use std::ffi::CString;
use std::path::Path;
use ultar_secure::verity;

const REAL_INIT: &str = "/sbin/ultar-init";
const NEWROOT: &str = "/newroot";

fn klog(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open("/dev/kmsg") {
        use std::io::Write;
        let _ = writeln!(f, "ultar-stage0: {msg}");
    }
    eprintln!("ultar-stage0: {msg}");
}

fn ensure_mount(src: &str, tgt: &str, fstype: &str, flags: MsFlags) {
    let _ = std::fs::create_dir_all(tgt);
    let _ = mount(Some(src), tgt, Some(fstype), flags, None::<&str>);
}

fn exec_real_init() -> ! {
    let path = if Path::new(REAL_INIT).exists() {
        REAL_INIT
    } else {
        "/init.real"
    };
    let c = CString::new(path).unwrap();
    let _ = execv(&c, &[c.clone()]);
    klog("exec echter Init fehlgeschlagen — Notfall-Shell");
    // Letzter Ausweg
    let sh = CString::new("/bin/sh").unwrap();
    let _ = execv(&sh, &[sh.clone()]);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

/// Bei echter Manipulation: Meldung, dann anhalten (nicht weiterbooten).
fn halt_tamper() -> ! {
    klog("SYSTEM VERÄNDERT — Start abgebrochen (Verity-Root-Hash passt nicht)");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

fn main() {
    ensure_mount("proc", "/proc", "proc", MsFlags::empty());
    ensure_mount("sysfs", "/sys", "sysfs", MsFlags::empty());
    ensure_mount("devtmpfs", "/dev", "devtmpfs", MsFlags::empty());

    let cfg = match verity::read_cfg("/ultar/verity.conf") {
        Some(c) => c,
        None => {
            klog("keine verity.conf — ungesicherter Start (noch nicht bereitgestellt)");
            exec_real_init();
        }
    };

    // TODO(on-device): Signatur verity.sig gegen eingebrannten ed25519-Pubkey prüfen, bevor der
    // Hash benutzt wird. Ohne gültige Signatur -> halt_tamper(). (Rohschlüssel liegt in /ultar/pub.ed25519.)

    let dev = match verity::open(&cfg, "ultar-root") {
        Ok(d) => d,
        Err(e) => {
            klog(&format!("verity open fehlgeschlagen: {e}"));
            halt_tamper();
        }
    };

    let _ = std::fs::create_dir_all(NEWROOT);
    if mount(
        Some(dev.as_str()),
        NEWROOT,
        Some("ext4"),
        MsFlags::MS_RDONLY,
        None::<&str>,
    )
    .is_err()
    {
        klog("Verity-Root mounten fehlgeschlagen");
        halt_tamper();
    }

    // virtuelle Dateisysteme in den neuen Root verschieben
    for (from, to) in [
        ("/proc", "/newroot/proc"),
        ("/sys", "/newroot/sys"),
        ("/dev", "/newroot/dev"),
    ] {
        let _ = std::fs::create_dir_all(to);
        let _ = mount(Some(from), to, None::<&str>, MsFlags::MS_MOVE, None::<&str>);
    }

    // switch_root: nach /newroot wechseln, als / verschieben, chroot, echten Init starten
    if chdir(NEWROOT).is_ok()
        && mount(Some("."), "/", None::<&str>, MsFlags::MS_MOVE, None::<&str>).is_ok()
        && chroot(".").is_ok()
    {
        let _ = chdir("/");
        klog("verifizierter Root aktiv (read-only) — starte echten Init");
        exec_real_init();
    }
    klog("switch_root fehlgeschlagen");
    halt_tamper();
}
