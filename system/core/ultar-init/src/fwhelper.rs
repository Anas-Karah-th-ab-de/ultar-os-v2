//! Firmware-Nachlader. Der Kernel (4.4, FW_LOADER_USER_HELPER_FALLBACK) wartet nach einem fehlgeschlagenen
//! Direktladen auf den Userspace: /sys/class/firmware/<name>/{loading,data}. Unter Android bedient das
//! ueventd; ohne Bediener wartet der Treiber bis zum Zeitlimit und gibt auf.
//!
//! Wichtig für fest eingebaute Treiber, die ihre Firmware bei den initcalls anfordern – lange bevor PID 1
//! etwas einbindet (A30: Audio-DSP „Calliope" bei t≈1,3 s → ohne Nachlader bleibt der Ton tot).
//! Ablauf: einmal alle offenen Anfragen bedienen, danach per Kernel-Ereignis (uevent) ohne Polling.
//! Nicht vorhandene Firmware wird sofort abgelehnt (loading = -1) → kein Hängen im Treiber.

use std::fs;

const DIRS: &[&str] = &["/lib/firmware", "/vendor/firmware", "/vendor/etc/wifi", "/vendor/etc", "/data/ultar/firmware", "/compat/lib/firmware"];
const CLASS: &str = "/sys/class/firmware";

fn find(name: &str) -> Option<String> {
    if name.contains("..") {
        return None;
    }
    DIRS.iter().map(|d| format!("{d}/{name}")).find(|p| fs::metadata(p).is_ok_and(|m| m.is_file()))
}

/// Eine offene Anfrage bedienen (sysfs-Verzeichnis der Anfrage, Firmware-Name).
fn serve(dir: &str, name: &str) {
    let loading = format!("{dir}/loading");
    if fs::metadata(&loading).is_err() {
        return;
    }
    let data = find(name).and_then(|p| fs::read(&p).ok().map(|d| (p, d)));
    match data {
        Some((p, d)) => {
            let ok = fs::write(&loading, "1").is_ok() && fs::write(format!("{dir}/data"), &d).is_ok();
            let _ = fs::write(&loading, if ok { "0" } else { "-1" });
            crate::log(&format!("Firmware {name}: {} ({} Bytes aus {p})", if ok { "geladen" } else { "FEHLER beim Schreiben" }, d.len()));
        }
        None => {
            let _ = fs::write(&loading, "-1");
            crate::log(&format!("Firmware {name}: nicht vorhanden – Anfrage sofort abgelehnt"));
        }
    }
}

fn serve_pending() {
    let Ok(rd) = fs::read_dir(CLASS) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name != "timeout" {
            serve(&format!("{CLASS}/{name}"), &name);
        }
    }
}

/// Im Hintergrund starten (nach dem Einbinden von /vendor).
pub fn start() {
    std::thread::spawn(|| {
        use nix::libc;
        // Erst den Ereigniskanal öffnen, dann den Bestand bedienen – so geht keine Anfrage verloren.
        let fd = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, libc::NETLINK_KOBJECT_UEVENT) };
        let mut bound = false;
        if fd >= 0 {
            let mut sa: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
            sa.nl_family = libc::AF_NETLINK as u16;
            sa.nl_groups = 1;
            bound = unsafe { libc::bind(fd, &sa as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_nl>() as u32) } == 0;
        }
        serve_pending();
        // Ab jetzt wird jede Anfrage sofort bedient oder abgelehnt; das Zeitlimit greift nur noch als Reserve.
        let _ = fs::write(format!("{CLASS}/timeout"), "10");
        if !bound {
            crate::log("Firmware-Nachlader: kein uevent-Kanal – Rückfall auf Abfrage alle 500 ms");
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                serve_pending();
            }
        }
        let mut buf = vec![0u8; 8192];
        loop {
            let n = unsafe { libc::recv(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0) };
            if n <= 0 {
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            }
            let msg = &buf[..n as usize];
            let mut fields = msg.split(|b| *b == 0).filter_map(|f| std::str::from_utf8(f).ok());
            if !fields.next().is_some_and(|h| h.starts_with("add@")) {
                continue;
            }
            let (mut sub, mut path, mut fw) = ("", "", "");
            for f in fields {
                if let Some(v) = f.strip_prefix("SUBSYSTEM=") { sub = v } else if let Some(v) = f.strip_prefix("DEVPATH=") { path = v } else if let Some(v) = f.strip_prefix("FIRMWARE=") { fw = v }
            }
            if sub == "firmware" && !fw.is_empty() && !path.is_empty() {
                serve(&format!("/sys{path}"), fw);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejects_traversal() {
        assert!(super::find("../etc/passwd").is_none());
    }
}
