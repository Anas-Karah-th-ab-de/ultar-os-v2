//! Früher Systemstart: Pseudo-Dateisysteme, Hostname, Module, cgroups.

use crate::config::InitConfig;
use crate::log;
use nix::mount::{mount, umount2, MntFlags, MsFlags};
use std::fs;

fn mnt(src: &str, target: &str, fstype: &str, flags: MsFlags, data: Option<&str>) {
    let _ = fs::create_dir_all(target);
    if let Err(e) = mount(Some(src), target, Some(fstype), flags, data) {
        // EBUSY = schon gemountet (z. B. devtmpfs durch den Kernel)
        if e != nix::errno::Errno::EBUSY {
            log(&format!("mount {target} ({fstype}) fehlgeschlagen: {e}"));
        }
    }
}

pub fn early_mounts() {
    let nosuid = MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC;
    mnt("proc", "/proc", "proc", nosuid, None);
    mnt("sysfs", "/sys", "sysfs", nosuid, None);
    mnt("devtmpfs", "/dev", "devtmpfs", MsFlags::MS_NOSUID, Some("mode=0755"));
    mnt("devpts", "/dev/pts", "devpts", MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC, Some("gid=5,mode=620,ptmxmode=666"));
    mnt("tmpfs", "/dev/shm", "tmpfs", MsFlags::MS_NOSUID | MsFlags::MS_NODEV, Some("mode=1777"));
    mnt("tmpfs", "/run", "tmpfs", MsFlags::MS_NOSUID | MsFlags::MS_NODEV, Some("mode=0755"));
    mnt("tmpfs", "/tmp", "tmpfs", MsFlags::MS_NOSUID | MsFlags::MS_NODEV, Some("mode=1777"));
    mnt("cgroup2", "/sys/fs/cgroup", "cgroup2", nosuid, Some("nsdelegate"));
    let _ = fs::create_dir_all("/run/ultar/log");
    // Standard-Symlinks, die viele Programme erwarten
    for (l, t) in [("/dev/fd", "/proc/self/fd"), ("/dev/stdin", "/proc/self/fd/0"),
                   ("/dev/stdout", "/proc/self/fd/1"), ("/dev/stderr", "/proc/self/fd/2")] {
        let _ = std::os::unix::fs::symlink(t, l);
    }
}

/// /vendor (Hersteller-Partition) read-only einbinden und den Kernel-Firmware-Suchpfad darauf setzen.
///
/// Wichtig auf dem A30: ohne /vendor fehlen alle Firmwares (Audio-DSP „Calliope", Video-Codec
/// mfc_fw.bin, Kamera-ISP fimc_is_*, WLAN mx140). Der Kernel sucht on-demand angeforderte Firmware
/// unter dem Pfad in firmware_class; wird er früh auf /vendor/firmware gesetzt, laden Kamera/Video/
/// Sensoren ihre Firmware sauber. (Der Audio-DSP wird beim Boot geprüft und braucht ggf. Neuprobe.)
pub fn mount_vendor_firmware() {
    // Gerätespezifisch (A30): vendor = mmcblk0p26. Überschreibbar später über hw-Profil.
    let part = "mmcblk0p26";
    let node = format!("/dev/{part}");
    // Geräteknoten anlegen, falls devtmpfs ihn nicht erzeugt hat (major:minor aus /proc/partitions).
    if !std::path::Path::new(&node).exists() {
        if let Ok(parts) = fs::read_to_string("/proc/partitions") {
            for l in parts.lines() {
                let f: Vec<&str> = l.split_whitespace().collect();
                if f.len() >= 4 && f[3] == part {
                    if let (Ok(maj), Ok(min)) = (f[0].parse::<u32>(), f[1].parse::<u32>()) {
                        let c = std::ffi::CString::new(node.as_str()).unwrap();
                        let dev = nix::libc::makedev(maj, min);
                        unsafe { nix::libc::mknod(c.as_ptr(), nix::libc::S_IFBLK | 0o600, dev) };
                    }
                    break;
                }
            }
        }
    }
    let _ = fs::create_dir_all("/vendor");
    if let Err(e) = mount(Some(node.as_str()), "/vendor", Some("ext4"), MsFlags::MS_RDONLY | MsFlags::MS_NOSUID, None::<&str>) {
        if e != nix::errno::Errno::EBUSY {
            log(&format!("mount /vendor fehlgeschlagen: {e}"));
        }
    }
    // Kernel-Firmware-Suchpfad auf die Hersteller-Firmware setzen.
    if std::path::Path::new("/vendor/firmware").is_dir() {
        let _ = fs::write("/sys/module/firmware_class/parameters/path", "/vendor/firmware");
    }
    // Offene und künftige Firmware-Anfragen bedient der Nachlader (fwhelper.rs), der auch das Zeitlimit setzt.
    // Kernel-Panik -> nach 5 s neu starten statt stehen zu bleiben
    let _ = fs::write("/proc/sys/kernel/panic", "5");
}

/// Lebenszeichen für Geräte ohne Konsole (Handy): goldener Balken oben auf dunklem Grund, direkt über
/// den Framebuffer. Zeigt, dass der Kernel `ultar-init` gestartet hat – noch bevor die Oberfläche läuft.
pub fn boot_marker() {
    use std::io::Write;
    let Ok(sz) = fs::read_to_string("/sys/class/graphics/fb0/virtual_size") else { return };
    let mut it = sz.trim().split(',').filter_map(|v| v.parse::<usize>().ok());
    let (Some(w), Some(_h)) = (it.next(), it.next()) else { return };
    let bpp = fs::read_to_string("/sys/class/graphics/fb0/bits_per_pixel").ok().and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(32);
    if bpp != 32 {
        return;
    }
    let stride = fs::read_to_string("/sys/class/graphics/fb0/stride").ok().and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(w * 4);
    let Ok(mut f) = fs::OpenOptions::new().write(true).open("/dev/fb0") else { return };
    // Gold ist in beiden Byte-Reihenfolgen (BGRX/RGBX) gut als heller Balken erkennbar
    let mut row = vec![0u8; stride];
    for px in row.chunks_exact_mut(4).take(w) {
        px.copy_from_slice(&[0x2F, 0x90, 0xB8, 0xFF]);
    }
    for _ in 0..24 {
        if f.write_all(&row).is_err() {
            return;
        }
    }
    let _ = fs::write("/sys/class/graphics/fb0/blank", "0");
}

pub fn configure(cfg: &InitConfig) {
    if let Some(h) = &cfg.hostname {
        if let Err(e) = nix::unistd::sethostname(h) {
            log(&format!("Hostname setzen fehlgeschlagen: {e}"));
        }
    }
    for m in &cfg.modules {
        load_module(m);
    }
    if let Ok(list) = fs::read_to_string(crate::config::MODULE_LIST) {
        for m in list.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            load_module(m);
        }
    }
    for m in &cfg.mounts {
        let (mut flags, data) = parse_options(m.options.as_deref().unwrap_or(""));
        if m.readonly {
            flags |= MsFlags::MS_RDONLY;
        }
        let _ = fs::create_dir_all(&m.target);
        let data = if data.is_empty() { None } else { Some(data.as_str()) };
        // Quelle auflösen: mehrere Kandidaten ("LABEL=name|/dev/…"), damit dieselbe Konfiguration auf jedem Gerät passt
        // USB-Sticks und NVMe/SATA (als Modul geladen) melden sich erst nach einigen 100 ms
        let t0 = std::time::Instant::now();
        let mut found = crate::hw::resolve_source(&m.source);
        while found.is_none() && t0.elapsed().as_millis() < m.wait_ms as u128 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            found = crate::hw::resolve_source(&m.source);
        }
        if m.wait_ms > 0 && found.is_some() && t0.elapsed().as_millis() > 100 {
            log(&format!("{} nach {} ms gefunden", m.target, t0.elapsed().as_millis()));
        }
        let Some(source) = found else {
            log(&format!("{}nicht gemountet {}: keine der Quellen vorhanden ({})", if m.optional { "optional " } else { "FEHLER " }, m.target, m.source));
            continue;
        };
        match mount(Some(source.as_str()), m.target.as_str(), Some(m.fstype.as_str()), flags, data) {
            Ok(()) => log(&format!("gemountet: {} -> {}", source, m.target)),
            Err(e) if m.optional => log(&format!("optional nicht gemountet {}: {e}", m.target)),
            Err(e) => log(&format!("FEHLER mount {} -> {}: {e}", m.source, m.target)),
        }
    }
    // /data (userdata) wird NICHT hier gemountet: das Einbinden der großen ext4 kann
    // mehrere Sekunden dauern (Journal-Wiederherstellung) und würde den Boot blockieren,
    // bevor der Watchdog läuft -> Reset-Schleife. Stattdessen nach dem Watchdog im Thread
    // (siehe main(): boot::mount_userdata_bg()).
    // Eigener cgroup-Unterbaum für Dienste; Controller freischalten.
    let _ = fs::create_dir_all("/sys/fs/cgroup/ultar");
    if let Ok(ctrl) = fs::read_to_string("/sys/fs/cgroup/cgroup.controllers") {
        let enable: String = ctrl.split_whitespace().map(|c| format!("+{c} ")).collect();
        let _ = fs::write("/sys/fs/cgroup/cgroup.subtree_control", enable.trim());
        let _ = fs::write("/sys/fs/cgroup/ultar/cgroup.subtree_control", enable.trim());
    }
    loopback_up();
}

/// PC: Sprache der Installation (Startpartition `\ultar\sprache`: ar/de/en) als Voreinstellung übernehmen,
/// solange es noch keine Einstellungen gibt – die Einrichtung erscheint dann gleich in dieser Sprache.
pub fn sprache_von_startpartition() {
    let ziel = "/data/ultar/settings.toml";
    if std::path::Path::new(ziel).exists() || !std::path::Path::new("/data").is_dir() {
        return;
    }
    let Some(dev) = crate::hw::find_by_partlabel("ULTAR-EFI") else { return };
    let ort = "/run/ultar/esp";
    let _ = fs::create_dir_all(ort);
    let fl = MsFlags::MS_RDONLY | MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC;
    if mount(Some(dev.as_str()), ort, Some("vfat"), fl, None::<&str>).is_err() {
        return;
    }
    let code = fs::read_to_string(format!("{ort}/ultar/sprache")).unwrap_or_default();
    let _ = umount2(ort, MntFlags::MNT_DETACH);
    let code = code.trim();
    if matches!(code, "ar" | "de" | "en") {
        let _ = fs::create_dir_all("/data/ultar");
        if fs::write(ziel, format!("lang = \"{code}\"\n")).is_ok() {
            log(&format!("Systemsprache aus der Installation: {code}"));
        }
    }
}

/// Nach dem Watchdog im Hintergrund aufrufen: bindet /data ein, ohne den Boot zu blockieren.
pub fn mount_userdata_bg() {
    std::thread::spawn(|| {
        mount_userdata();
    });
}

/// Den internen Hauptspeicher (größte eMMC-Partition = userdata, ~63 GB) als /data einbinden.
/// Versucht ext4 und f2fs (rw). Verändert die Partition nicht; wird sie nicht erkannt, nur Protokoll.
fn mount_userdata() {
    // schon etwas auf /data gemountet?
    if fs::read_to_string("/proc/mounts").unwrap_or_default().lines().any(|l| l.split_whitespace().nth(1) == Some("/data")) {
        return;
    }
    // größte Partition der Haupt-eMMC finden
    let mut emmc = (String::new(), 0u64);
    if let Ok(rd) = fs::read_dir("/sys/block") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("mmcblk") && !n.contains('p') && !n.contains("boot") && !n.contains("rpmb") {
                let b: u64 = fs::read_to_string(format!("/sys/block/{n}/size")).unwrap_or_default().trim().parse().unwrap_or(0);
                if b > emmc.1 {
                    emmc = (n, b);
                }
            }
        }
    }
    if emmc.0.is_empty() {
        return;
    }
    let mut part = (String::new(), 0u64);
    if let Ok(rd) = fs::read_dir(format!("/sys/block/{}", emmc.0)) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with(&emmc.0) && n.contains('p') {
                let b: u64 = fs::read_to_string(format!("/sys/block/{}/{n}/size", emmc.0)).unwrap_or_default().trim().parse().unwrap_or(0);
                if b > part.1 {
                    part = (n, b);
                }
            }
        }
    }
    if part.0.is_empty() {
        return;
    }
    let dev = format!("/dev/{}", part.0);
    let _ = fs::create_dir_all("/data");
    let flags = MsFlags::MS_NOATIME;
    for fstype in ["ext4", "f2fs"] {
        match mount(Some(dev.as_str()), "/data", Some(fstype), flags, None::<&str>) {
            Ok(()) => {
                log(&format!("Speicher: {dev} ({}) als /data eingebunden [{fstype}]", human_gb(part.1 * 512)));
                let _ = fs::create_dir_all("/data/ultar");
                return;
            }
            Err(_) => continue,
        }
    }
    log(&format!("Speicher: {dev} ({}) nicht einbindbar (unbekanntes/verschlüsseltes Dateisystem) – /data bleibt lokal", human_gb(part.1 * 512)));
}

fn human_gb(bytes: u64) -> String {
    format!("{:.0} GB", bytes as f64 / 1_073_741_824.0)
}

/// Optionen wie bei fstab: Flags (noatime, ro, …) -> MsFlags, Rest -> Daten-String.
fn parse_options(opts: &str) -> (MsFlags, String) {
    let mut f = MsFlags::empty();
    let mut data = Vec::new();
    for o in opts.split(',').map(str::trim).filter(|o| !o.is_empty()) {
        match o {
            "ro" => f |= MsFlags::MS_RDONLY,
            "rw" | "defaults" => {}
            "noatime" => f |= MsFlags::MS_NOATIME,
            "nodiratime" => f |= MsFlags::MS_NODIRATIME,
            "relatime" => f |= MsFlags::MS_RELATIME,
            "nosuid" => f |= MsFlags::MS_NOSUID,
            "nodev" => f |= MsFlags::MS_NODEV,
            "noexec" => f |= MsFlags::MS_NOEXEC,
            "sync" => f |= MsFlags::MS_SYNCHRONOUS,
            other => data.push(other),
        }
    }
    (f, data.join(","))
}

fn load_module(path: &str) {
    match fs::File::open(path) {
        Ok(f) => match nix::kmod::finit_module(&f, c"", nix::kmod::ModuleInitFlags::empty()) {
            Ok(()) => log(&format!("Modul geladen: {path}")),
            Err(e) if e == nix::errno::Errno::EEXIST => {}
            Err(e) => log(&format!("Modul {path} fehlgeschlagen: {e}")),
        },
        Err(e) => log(&format!("Modul {path} nicht lesbar: {e}")),
    }
}

/// lo hochfahren ohne externe Tools (ioctl SIOCSIFFLAGS).
fn loopback_up() {
    #[repr(C)]
    struct IfReq {
        name: [u8; 16],
        flags: libc_short,
        _pad: [u8; 22],
    }
    #[allow(non_camel_case_types)]
    type libc_short = i16;
    const SIOCGIFFLAGS: u64 = 0x8913;
    const SIOCSIFFLAGS: u64 = 0x8914;
    const IFF_UP: i16 = 0x1;
    extern "C" {
        fn socket(d: i32, t: i32, p: i32) -> i32;
        fn ioctl(fd: i32, req: u64, ...) -> i32;
        fn close(fd: i32) -> i32;
    }
    unsafe {
        let fd = socket(2 /*AF_INET*/, 2 /*SOCK_DGRAM*/ | 0o2000000 /*CLOEXEC*/, 0);
        if fd < 0 {
            return;
        }
        let mut r = IfReq { name: [0; 16], flags: 0, _pad: [0; 22] };
        r.name[..2].copy_from_slice(b"lo");
        if ioctl(fd, SIOCGIFFLAGS, &mut r) == 0 {
            r.flags |= IFF_UP;
            ioctl(fd, SIOCSIFFLAGS, &mut r);
        }
        close(fd);
    }
}

/// Alles aushängen (umgekehrte Reihenfolge), Root read-only.
pub fn unmount_all() {
    nix::unistd::sync();
    let Ok(m) = fs::read_to_string("/proc/mounts") else { return };
    let targets: Vec<String> = m.lines().filter_map(|l| l.split_whitespace().nth(1).map(String::from)).collect();
    for t in targets.iter().rev() {
        if matches!(t.as_str(), "/" | "/proc" | "/sys" | "/dev") {
            continue;
        }
        let _ = umount2(t.as_str(), MntFlags::MNT_DETACH);
    }
    let _ = mount(None::<&str>, "/", None::<&str>, MsFlags::MS_REMOUNT | MsFlags::MS_RDONLY, None::<&str>);
    nix::unistd::sync();
}
