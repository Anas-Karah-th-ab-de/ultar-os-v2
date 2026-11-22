//! Abschottung: Bildschirm und Eingabe werden als root geöffnet, danach gibt der Browser alle Rechte ab.
//! Alle Seiten- und Netzwerkprozesse von WebKit erben das (eigener Benutzer, eigene Sicht aufs Dateisystem,
//! eigene cgroup mit Speichergrenze, keine neuen Rechte per setuid).

use std::ffi::CString;

pub const WEB_UID: u32 = 9901;
pub const WEB_GID: u32 = 9901;

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

unsafe fn mount(src: &str, dst: &str, fs: &str, flags: libc::c_ulong, data: &str) -> bool {
    let (s, d, f, o) = (c(src), c(dst), c(fs), c(data));
    libc::mount(
        s.as_ptr(),
        d.as_ptr(),
        if fs.is_empty() {
            std::ptr::null()
        } else {
            f.as_ptr()
        },
        flags,
        if data.is_empty() {
            std::ptr::null()
        } else {
            o.as_ptr() as *const _
        },
    ) == 0
}

/// Eigene cgroup mit Speichergrenze (75 % des RAM); cgroup v2 oder v1-Speicher-Controller.
fn cgroup(ram_mb: u64) {
    let pid = std::process::id().to_string();
    let limit = (ram_mb * 1024 * 1024 * 3 / 4).to_string();
    if std::path::Path::new("/sys/fs/cgroup/cgroup.controllers").exists() {
        let d = "/sys/fs/cgroup/ultar-web";
        let _ = std::fs::create_dir_all(d);
        let _ = std::fs::write(format!("{d}/memory.high"), &limit);
        let _ = std::fs::write(format!("{d}/cgroup.procs"), &pid);
    } else if std::path::Path::new("/sys/fs/cgroup/memory").is_dir() {
        let d = "/sys/fs/cgroup/memory/ultar-web";
        let _ = std::fs::create_dir_all(d);
        let _ = std::fs::write(format!("{d}/memory.soft_limit_in_bytes"), &limit);
        let _ = std::fs::write(format!("{d}/cgroup.procs"), &pid);
    }
}

/// Rechte abgeben. `data_dir`: Profilordner, der beschreibbar bleiben muss. Liefert true, wenn abgegeben.
pub fn drop_privileges(data_dir: &str, ram_mb: u64) -> bool {
    if unsafe { libc::geteuid() } != 0 {
        return false; // läuft schon ohne Rechte (z. B. Test am PC)
    }
    cgroup(ram_mb);
    let _ = std::fs::create_dir_all(data_dir);
    let _ = std::process::Command::new("chown")
        .args(["-R", &format!("{WEB_UID}:{WEB_GID}"), data_dir])
        .status();
    // eigener Ordner für Downloads
    let _ = std::fs::create_dir_all("/data/Download");
    let _ = std::process::Command::new("chown")
        .args([&format!("{WEB_UID}:{WEB_GID}"), "/data/Download"])
        .status();
    // Ton: Gruppe des Browsers bekommt Zugriff auf /dev/snd
    if let Ok(rd) = std::fs::read_dir("/dev/snd") {
        for e in rd.flatten() {
            let p = c(&e.path().to_string_lossy());
            unsafe {
                libc::chown(p.as_ptr(), 0, WEB_GID);
                libc::chmod(p.as_ptr(), 0o660);
            }
        }
    }
    unsafe {
        // Eigene Sicht aufs Dateisystem: Systemdaten (/data/ultar: WLAN-Kennwörter, Schlüssel, Konten) und
        // der Ubuntu-Bereich sind für den Browser unsichtbar; nur sein Profil bleibt erreichbar.
        if libc::unshare(libc::CLONE_NEWNS) == 0 {
            mount("", "/", "", libc::MS_REC | libc::MS_PRIVATE, "");
            let keep = "/run/ultar-web-keep";
            let _ = std::fs::create_dir_all(keep);
            if mount(data_dir, keep, "", libc::MS_BIND, "") {
                if mount(
                    "tmpfs",
                    "/data/ultar",
                    "tmpfs",
                    libc::MS_NOSUID | libc::MS_NODEV,
                    "mode=755",
                ) {
                    let _ = std::fs::create_dir_all(data_dir);
                    mount(keep, data_dir, "", libc::MS_BIND, "");
                } else {
                    eprintln!("ultar-web: /data/ultar konnte nicht verdeckt werden");
                }
            }
            if std::path::Path::new("/compat").is_dir() {
                mount(
                    "tmpfs",
                    "/compat",
                    "tmpfs",
                    libc::MS_NOSUID | libc::MS_NODEV | libc::MS_RDONLY,
                    "mode=755",
                );
            }
        } else {
            eprintln!("ultar-web: kein eigener Mount-Namensraum möglich");
        }
        // 3003 = AID_INET: Android-Kerne (CONFIG_ANDROID_PARANOID_NETWORK, z. B. Samsung A30) erlauben
        // Netzwerk-Sockets nur Prozessen in dieser Gruppe. Auf anderen Kernen ist die Gruppe bedeutungslos.
        let groups = [WEB_GID, 3003];
        libc::setgroups(groups.len(), groups.as_ptr());
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        if libc::setresgid(WEB_GID, WEB_GID, WEB_GID) != 0
            || libc::setresuid(WEB_UID, WEB_UID, WEB_UID) != 0
        {
            eprintln!("ultar-web: Rechte konnten nicht abgegeben werden – Abbruch");
            std::process::exit(3);
        }
    }
    std::env::set_var("HOME", data_dir);
    true
}
