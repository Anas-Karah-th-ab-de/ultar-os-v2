//! ultar-modem – CP-(Modem-)Boot für Ultar OS auf dem Samsung A30 (SM-A305F, Exynos 7904).
//!
//! WICHTIG: Das Gerät nutzt den Treiber **modem_v1** (CONFIG_UMTS_MODEM_SS310AP,
//! drivers/misc/modem_v1/modem_ctrl_ss310ap.c + link_device_shmem.c), NICHT modem_if!
//! Darum andere ioctls/Strukturen und eine andere Reihenfolge als in der ersten Fassung.
//!
//! Verifizierte Boot-Reihenfolge (aus der Kernelquelle, CP_SECURE_BOOT=y):
//!   1. IOCTL_MODEM_ON            – CP mit Strom versorgen (pmu power on, 300 ms warten)
//!   2. IOCTL_MODEM_XMIT_BOOT     – BOOT-Stufe(n) in den Boot-Speicher laden (struct modem_firmware, mode=0)
//!   3. IOCTL_SECURITY_REQ        – exynos_smc: lädt die Signatur, authentifiziert das Boot-Image
//!                                  (struct modem_sec_req{mode=0,size_boot,size_main})
//!   4. IOCTL_MODEM_DL_START      – setzt MEM_BOOT_MAGIC + ap_status=1 (boot_confirm) → CP startet PBL
//!   5. IOCTL_MODEM_BOOT_ON       – wartet auf cp_status != 0 (CP läuft jetzt); vorher kam hier -EACCES!
//!   6. IOCTL_MODEM_XMIT_BOOT     – MAIN/VSS/NV in den Hauptspeicher laden
//!   7. IOCTL_MODEM_BOOT_DONE
//!   8. IOCTL_MODEM_BOOT_OFF
//!   9. IOCTL_MODEM_STATUS pollen bis STATE_ONLINE (=4)
//!
//! Der frühere Fehler (BOOT_ON direkt nach MODEM_ON → ss310ap_boot_on wartet 100×~15 ms vergeblich auf
//! cp_status und gibt -EACCES) lag daran, dass der CP nichts zu starten hatte: Boot-Image + SECURITY_REQ
//! + DL_START MÜSSEN vor BOOT_ON kommen.
//!
//! Sicherer Boot: nur die ORIGINAL signierte Samsung-CP-Firmware besteht SECURITY_REQ.

use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::raw::c_ulong;

// ---------- ioctl-Nummern (modem_v1) : _IO('o', nr) => (0x6f<<8)|nr ----------
const fn io_o(nr: u32) -> c_ulong {
    ((0x6f << 8) | nr) as c_ulong
}
const IOCTL_MODEM_ON: c_ulong = io_o(0x19);
const IOCTL_MODEM_OFF: c_ulong = io_o(0x20);
const IOCTL_MODEM_RESET: c_ulong = io_o(0x21);
const IOCTL_MODEM_BOOT_ON: c_ulong = io_o(0x22);
const IOCTL_MODEM_BOOT_OFF: c_ulong = io_o(0x23);
#[allow(dead_code)]
const IOCTL_MODEM_BOOT_DONE: c_ulong = io_o(0x24);
const IOCTL_MODEM_STATUS: c_ulong = io_o(0x27);
const IOCTL_MODEM_DL_START: c_ulong = io_o(0x28);
const IOCTL_MODEM_XMIT_BOOT: c_ulong = io_o(0x40);
const IOCTL_SECURITY_REQ: c_ulong = io_o(0x53);

const STATE_ONLINE: i32 = 4; // enum: OFFLINE=0, CRASH_RESET=1, CRASH_EXIT=2, BOOTING=3, ONLINE=4

extern "C" {
    fn ioctl(fd: i32, req: c_ulong, ...) -> i32;
}

// ---------- Strukturen (1:1 aus modem_v1/modem_prj.h) ----------

/// struct modem_firmware (__packed) – Argument für IOCTL_MODEM_XMIT_BOOT
#[repr(C, packed)]
struct ModemFirmware {
    binary: u64,   // Userspace-Zeiger auf die Nutzdaten
    size: u32,     // Größe der Stufe
    m_offset: u32, // Ziel-Offset im CP-Speicher (aus TOC)
    b_offset: u32, // Quell-Offset (hier 0, wir übergeben den Puffer direkt)
    mode: u32,     // 0 = BOOT_MODE (boot_base/boot_size), 1 = DUMP
    len: u32,      // Länge dieses Stücks
}

/// struct modem_sec_req (__packed) – Argument für IOCTL_SECURITY_REQ
#[repr(C, packed)]
struct ModemSecReq {
    mode: u32, // cp_boot_mode: 0 = NORMAL
    size_boot: u32,
    size_main: u32,
    dummy: u32,
}

/// Samsung-CP-TOC-Eintrag (32 Byte). VERIFY am echten Image.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TocEntry {
    name: [u8; 12],
    b_offset: u32,
    m_offset: u32,
    size: u32,
    crc: u32,
    toc_count: u32,
}
const TOC_ENTRY_SIZE: usize = 32;

fn parse_toc(fw: &[u8]) -> Vec<TocEntry> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + TOC_ENTRY_SIZE <= fw.len() && out.len() < 16 {
        let e = &fw[off..off + TOC_ENTRY_SIZE];
        let mut name = [0u8; 12];
        name.copy_from_slice(&e[0..12]);
        if name[0] == 0 {
            break;
        }
        let rd = |a: usize| u32::from_le_bytes([e[a], e[a + 1], e[a + 2], e[a + 3]]);
        out.push(TocEntry {
            name,
            b_offset: rd(12),
            m_offset: rd(16),
            size: rd(20),
            crc: rd(24),
            toc_count: rd(28),
        });
        off += TOC_ENTRY_SIZE;
        if out[0].toc_count != 0 && out.len() as u32 >= out[0].toc_count {
            break;
        }
    }
    out
}

fn name_str(n: &[u8; 12]) -> String {
    let end = n.iter().position(|&c| c == 0).unwrap_or(12);
    String::from_utf8_lossy(&n[..end]).trim().to_string()
}

fn ioc0(fd: i32, req: c_ulong, name: &str) -> i32 {
    let r = unsafe { ioctl(fd, req) };
    eprintln!(
        "ioctl {name} -> {r}{}",
        if r < 0 {
            format!(" ({})", std::io::Error::last_os_error())
        } else {
            String::new()
        }
    );
    r
}

/// Stückgröße wie im Original-Lader (cbd): 0xf800 Byte je IOCTL_MODEM_XMIT_BOOT.
const CHUNK: usize = 0xf800;

/// Eine Stufe in Stücken senden – 1:1 wie cbd `shannon310_boot_xmit_bin`:
/// size = Gesamtgröße der Stufe, m_offset wandert mit, b_offset = Quell-Offset (konstant), mode = 0.
fn xmit_stage(fd: i32, name: &str, e: &TocEntry, base: u32, data: &[u8]) -> bool {
    // Die TOC nennt CP-Adressen (Basis 0x4000_0000 = m_offset von BOOT); der Kernel erwartet den Abstand
    // zum Anfang des CP-Speichers (am A30 geprüft: roher Wert → „Invalid args").
    let m_off = e.m_offset.wrapping_sub(base);
    let mut done = 0usize;
    while done < data.len() {
        let len = (data.len() - done).min(CHUNK);
        let mf = ModemFirmware {
            binary: data[done..].as_ptr() as u64,
            size: data.len() as u32,
            m_offset: m_off + done as u32,
            b_offset: e.b_offset,
            mode: 0,
            len: len as u32,
        };
        let r = unsafe { ioctl(fd, IOCTL_MODEM_XMIT_BOOT, &mf as *const _) };
        if r != 0 {
            eprintln!(
                "XMIT_BOOT {name} m_offset=0x{:x} len={len} -> {r} ({})",
                m_off + done as u32,
                std::io::Error::last_os_error()
            );
            return false;
        }
        done += len;
    }
    eprintln!(
        "XMIT_BOOT {name}: {} Bytes nach m_offset=0x{m_off:x} gesendet",
        data.len()
    );
    true
}

fn sec_req(fd: i32, mode: u32, size_boot: u32, size_main: u32) -> i32 {
    let sec = ModemSecReq {
        mode,
        size_boot,
        size_main,
        dummy: 0,
    };
    let r = unsafe { ioctl(fd, IOCTL_SECURITY_REQ, &sec as *const _) };
    eprintln!(
        "SECURITY_REQ mode={mode} size_boot={size_boot} size_main={size_main} -> {r} (0 = ok)"
    );
    r
}

/// Anfrage/Antwort auf dem Boot-Kanal (cbd `std_udl_req_resp`): 4 Byte schreiben, 4 Byte lesen, vergleichen.
fn req_resp(dev: &mut File, req: u32, exp: u32) -> bool {
    use std::io::Write;
    if let Err(e) = dev.write_all(&req.to_le_bytes()) {
        eprintln!("Handshake 0x{req:04x}: Schreiben fehlgeschlagen: {e}");
        return false;
    }
    // Antwort mit Zeitlimit (poll), damit ein stummer CP das Werkzeug nicht aufhängt
    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }
    extern "C" {
        fn poll(fds: *mut PollFd, n: u64, timeout: i32) -> i32;
    }
    let mut p = PollFd {
        fd: dev.as_raw_fd(),
        events: 1,
        revents: 0,
    };
    if unsafe { poll(&mut p, 1, 5000) } <= 0 {
        eprintln!("Handshake 0x{req:04x}: keine Antwort vom CP (5 s)");
        return false;
    }
    let mut b = [0u8; 4];
    match dev.read(&mut b) {
        Ok(4) => {
            let got = u32::from_le_bytes(b);
            eprintln!("Handshake 0x{req:04x} -> 0x{got:04x} (erwartet 0x{exp:04x})");
            got == exp
        }
        r => {
            eprintln!("Handshake 0x{req:04x}: Lesen {r:?}");
            false
        }
    }
}

/// CP-Boot in der Reihenfolge des Original-Laders (cbd `start_shannon310_boot`, per Analyse des Ablaufs):
///   STATUS → MODEM_RESET → SECURITY_REQ(mode 2) → alle Stufen senden (BOOT, MAIN, VSS, NV; mode 0, rohe
///   TOC-m_offsets, 0xf800-Stücke) → SECURITY_REQ(mode 0, Größe BOOT, Größe MAIN) → MODEM_ON → BOOT_ON →
///   DL_START → Handshake 0x900d/0xa00d, 0x9f00/0xaf00 → BOOT_OFF → STATUS bis ONLINE.
/// NV kommt nicht aus der Firmware, sondern aus nv_data.bin (Kopie aus EFS – EFS selbst wird nie beschrieben).
fn boot(fw: &[u8], toc: &[TocEntry], nv: Option<Vec<u8>>) -> std::io::Result<()> {
    let mut dev = File::options()
        .read(true)
        .write(true)
        .open("/dev/umts_boot0")?;
    let fd = dev.as_raw_fd();
    let st = unsafe { ioctl(fd, IOCTL_MODEM_STATUS) };
    eprintln!("modem_status vorher: {st}");

    ioc0(fd, IOCTL_MODEM_RESET, "MODEM_RESET");
    sec_req(fd, 2, 0, 0);

    let (mut boot_size, mut main_size) = (0u32, 0u32);
    let base = toc
        .iter()
        .find(|e| name_str(&e.name) == "BOOT")
        .map(|e| e.m_offset)
        .unwrap_or(0);
    for e in toc.iter().skip(1) {
        let nm = name_str(&e.name);
        if nm == "OFFSET" {
            continue;
        }
        let data: std::borrow::Cow<[u8]> = if nm.starts_with("NV") {
            match &nv {
                Some(n) => {
                    let mut d = n.clone();
                    d.resize(e.size as usize, 0xff);
                    std::borrow::Cow::Owned(d)
                }
                None => {
                    eprintln!("{nm}: keine nv_data.bin angegeben (--nv) – Stufe übersprungen");
                    continue;
                }
            }
        } else {
            let (s, l) = (e.b_offset as usize, e.size as usize);
            if l == 0 || s + l > fw.len() {
                eprintln!(
                    "{nm}: liegt nicht in der Firmware (b_offset=0x{s:x}, size={l}) – übersprungen"
                );
                continue;
            }
            std::borrow::Cow::Borrowed(&fw[s..s + l])
        };
        if nm == "BOOT" {
            boot_size = e.size
        }
        if nm == "MAIN" {
            main_size = e.size
        }
        if !xmit_stage(fd, &nm, e, base, &data) {
            return Err(std::io::Error::other(format!("Stufe {nm} abgelehnt")));
        }
    }

    if sec_req(fd, 0, boot_size, main_size) != 0 {
        return Err(std::io::Error::other(
            "Signaturprüfung (SECURITY_REQ) fehlgeschlagen",
        ));
    }
    if ioc0(fd, IOCTL_MODEM_ON, "MODEM_ON") < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if ioc0(fd, IOCTL_MODEM_BOOT_ON, "BOOT_ON") < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if ioc0(fd, IOCTL_MODEM_DL_START, "DL_START") < 0 {
        return Err(std::io::Error::last_os_error());
    }

    let hs = req_resp(&mut dev, 0x900d, 0xa00d) && req_resp(&mut dev, 0x9f00, 0xaf00);
    if !hs {
        eprintln!("Handshake unvollständig – BOOT_OFF wird trotzdem versucht");
    }
    ioc0(fd, IOCTL_MODEM_BOOT_OFF, "BOOT_OFF");

    for _ in 0..100 {
        let st = unsafe { ioctl(fd, IOCTL_MODEM_STATUS) };
        if st == STATE_ONLINE {
            eprintln!("Modem ONLINE (status={st}).");
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    eprintln!("Modem nicht online geworden (status={}).", unsafe {
        ioctl(fd, IOCTL_MODEM_STATUS)
    });
    Ok(())
}

/// Nach einem Fehlversuch steht der CP in STATE_BOOTING. MODEM_OFF (optional RESET) setzt ihn zurück.
fn reset() -> std::io::Result<()> {
    let dev = File::options()
        .read(true)
        .write(true)
        .open("/dev/umts_boot0")?;
    let fd = dev.as_raw_fd();
    ioc0(fd, IOCTL_MODEM_OFF, "MODEM_OFF");
    std::thread::sleep(std::time::Duration::from_millis(300));
    ioc0(fd, IOCTL_MODEM_RESET, "MODEM_RESET");
    Ok(())
}

// ---------- Automatik für den Systemstart ----------

const WORK_DIR: &str = "/data/ultar/modem";

extern "C" {
    fn mknod(path: *const std::os::raw::c_char, mode: u32, dev: u64) -> i32;
    fn mount(
        src: *const std::os::raw::c_char,
        tgt: *const std::os::raw::c_char,
        fs: *const std::os::raw::c_char,
        flags: c_ulong,
        data: *const std::os::raw::c_char,
    ) -> i32;
    fn umount(tgt: *const std::os::raw::c_char) -> i32;
    fn sync();
}

/// Blockgerät einer Partition über ihren Namen finden (PARTNAME in sysfs) und einen Geräteknoten anlegen.
fn part_node(name: &str) -> Option<String> {
    for e in std::fs::read_dir("/sys/class/block").ok()?.flatten() {
        let ue = std::fs::read_to_string(e.path().join("uevent")).unwrap_or_default();
        if !ue.lines().any(|l| {
            l.strip_prefix("PARTNAME=")
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        }) {
            continue;
        }
        let dev = std::fs::read_to_string(e.path().join("dev")).ok()?;
        let (maj, min) = dev.trim().split_once(':')?;
        let (maj, min): (u64, u64) = (maj.parse().ok()?, min.parse().ok()?);
        // Knoten unter /dev anlegen: /run ist ohne Gerätedateien eingebunden (nodev → „Permission denied")
        let path = format!("/dev/ultar-part-{name}");
        let c = std::ffi::CString::new(path.clone()).ok()?;
        // makedev wie glibc; S_IFBLK | 0600
        let devno =
            ((maj & 0xfff) << 8) | (min & 0xff) | ((min & !0xff) << 12) | ((maj & !0xfff) << 32);
        unsafe { mknod(c.as_ptr(), 0o060600, devno) };
        return Some(path);
    }
    None
}

/// Dateien aus der EFS-Partition, die der Original-Lader (cbd) liest: nv_data.bin (Kalibrierung) und
/// factory.prop (Anzahl SIM-Steckplätze). Beides wird einmalig nach WORK_DIR kopiert; EFS wird nur lesend
/// (ohne Journal-Wiedergabe) eingebunden und nie beschrieben.
fn ensure_efs() -> Option<Vec<u8>> {
    let nv = format!("{WORK_DIR}/nv_data.bin");
    let fp = format!("{WORK_DIR}/factory.prop");
    let have_nv = std::fs::metadata(&nv).map(|m| m.len() > 0).unwrap_or(false);
    // factory.prop.none = EFS wurde schon durchsucht, die Datei gibt es dort nicht
    let have_fp =
        std::path::Path::new(&fp).exists() || std::path::Path::new(&format!("{fp}.none")).exists();
    if !(have_nv && have_fp) {
        if let Some(efs) = part_node("efs") {
            let mnt = "/run/ultar/modem/efs";
            let _ = std::fs::create_dir_all(mnt);
            let cs = |s: &str| std::ffi::CString::new(s).unwrap();
            let r = unsafe {
                mount(
                    cs(&efs).as_ptr(),
                    cs(mnt).as_ptr(),
                    cs("ext4").as_ptr(),
                    1,
                    cs("noload").as_ptr(),
                )
            };
            if r != 0 {
                eprintln!("EFS nicht einbindbar: {}", std::io::Error::last_os_error());
            } else {
                let _ = std::fs::create_dir_all(WORK_DIR);
                if !have_nv {
                    if let Ok(d) = std::fs::read(format!("{mnt}/nv_data.bin")) {
                        match std::fs::write(&nv, &d) {
                            Ok(_) => eprintln!(
                                "nv_data.bin aus EFS kopiert ({} Bytes) – EFS bleibt unverändert",
                                d.len()
                            ),
                            Err(e) => eprintln!("{nv}: {e}"),
                        }
                    }
                }
                if !have_fp {
                    match std::fs::read(format!("{mnt}/factory.prop")) {
                        Ok(d) => {
                            let _ = std::fs::write(&fp, &d);
                            eprintln!(
                                "factory.prop aus EFS kopiert: {}",
                                String::from_utf8_lossy(&d).trim()
                            );
                        }
                        Err(_) => {
                            let _ = std::fs::write(format!("{fp}.none"), "");
                            eprintln!("factory.prop fehlt in EFS – Kernel-Vorgabe für SIM-Steckplätze bleibt");
                        }
                    }
                }
                unsafe { umount(cs(mnt).as_ptr()) };
            }
        }
    }
    std::fs::read(&nv).ok().filter(|d| !d.is_empty())
}

/// Wie cbd `get_factory_prop`: das Zeichen nach dem ersten '=' ist die Anzahl der SIM-Steckplätze.
fn sim_count_from_prop(txt: &str) -> Option<u32> {
    let c = txt.split_once('=')?.1.chars().next()?;
    c.to_digit(10)
}

const DS_DETECT: &str = "/sys/devices/platform/10000.mif_pdata/sim/ds_detect";
const DS_DETECT_PARAM: &str = "/sys/module/modem_ctrl_ss310ap/parameters/ds_detect";

/// Wie cbd `set_sim_configuration` (VOR dem CP-Start): Anzahl SIM-Steckplätze in `ds_detect` schreiben.
/// Der Kernel legt den Wert bei MODEM_ON (init_mailbox_regs) in das AP→CP-Statusregister; daran richtet der
/// CP seine SIM-Erkennung aus. Vorrang: /data/ultar/modem/simslots (Hand-Vorgabe) vor EFS-factory.prop.
fn set_sim_configuration() -> Option<u32> {
    let manual = std::fs::read_to_string(format!("{WORK_DIR}/simslots"))
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    let prop = std::fs::read_to_string(format!("{WORK_DIR}/factory.prop")).ok();
    let n = manual.or_else(|| prop.as_deref().and_then(sim_count_from_prop));
    let Some(n) = n else {
        eprintln!("SIM-Konfiguration: keine Angabe (factory.prop/simslots) – ds_detect bleibt auf Kernel-Vorgabe");
        return None;
    };
    // cbd nimmt 1..4, der Kernel (ds_detect_store) nur 0..2
    if !(1..=4).contains(&n) {
        eprintln!("SIM-Konfiguration: ungültige Anzahl {n}");
        return None;
    }
    let n = n.min(2);
    let ok = [DS_DETECT, DS_DETECT_PARAM]
        .iter()
        .any(|p| match std::fs::write(p, format!("{n}")) {
            Ok(_) => {
                eprintln!("SIM-Konfiguration: {n} Steckplatz/-plätze -> {p}");
                true
            }
            Err(e) => {
                eprintln!("SIM-Konfiguration: {p}: {e}");
                false
            }
        });
    let _ = std::fs::create_dir_all("/run/ultar/radio");
    let _ = std::fs::write("/run/ultar/radio/simslots", format!("{n}\n"));
    ok.then_some(n)
}

/// Systemstart: Modem starten, falls es noch nicht läuft. Firmware direkt aus der Partition „radio".
fn auto() -> std::io::Result<()> {
    {
        let dev = File::options()
            .read(true)
            .write(true)
            .open("/dev/umts_boot0")?;
        let st = unsafe { ioctl(dev.as_raw_fd(), IOCTL_MODEM_STATUS) };
        if st == STATE_ONLINE {
            println!("Modem läuft bereits (status={st}).");
            return Ok(());
        }
    }
    let radio = part_node("radio")
        .ok_or_else(|| std::io::Error::other("Partition radio nicht gefunden"))?;
    let mut f = File::open(&radio)?;
    let mut head = vec![0u8; 512];
    f.read_exact(&mut head)?;
    let toc = parse_toc(&head);
    if toc.first().map(|e| name_str(&e.name)) != Some("TOC".into()) {
        return Err(std::io::Error::other(
            "keine gültige TOC in der Partition radio",
        ));
    }
    // nur so viel lesen, wie die Stufen brauchen
    let need = toc
        .iter()
        .filter(|e| !name_str(&e.name).starts_with("NV") && name_str(&e.name) != "OFFSET")
        .map(|e| e.b_offset as usize + e.size as usize)
        .max()
        .unwrap_or(512);
    let mut fw = head;
    fw.resize(need.min(256 << 20), 0);
    f.read_exact(&mut fw[512..])?;
    let toc: Vec<TocEntry> = toc
        .into_iter()
        .filter(|e| name_str(&e.name) != "OFFSET")
        .collect();
    let nv = ensure_efs();
    set_sim_configuration();
    if nv.is_none() {
        return Err(std::io::Error::other(
            "nv_data.bin fehlt (EFS nicht lesbar) – ohne Kalibrierdaten wird nicht gebootet",
        ));
    }
    boot(&fw, &toc, nv)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--auto") {
        // Schutz vor Neustart-Schleifen: stürzt das Gerät beim Modemstart ab, bleibt die Marke liegen und der
        // nächste Systemstart lässt das Modem aus (Marke löschen = wieder versuchen).
        let guard = format!("{WORK_DIR}/boot.try");
        if std::path::Path::new(&guard).exists() && !args.iter().any(|a| a == "--force") {
            eprintln!("ultar-modem --auto: letzter Start wurde nicht beendet ({guard} vorhanden) – Modem bleibt aus. Mit --force erneut versuchen.");
            std::process::exit(2);
        }
        let _ = std::fs::create_dir_all(WORK_DIR);
        let _ = std::fs::write(&guard, "1");
        unsafe { sync() };
        let r = auto();
        let _ = std::fs::remove_file(&guard);
        if let Err(e) = r {
            eprintln!("ultar-modem --auto: {e}");
            std::process::exit(1);
        }
        return;
    }
    if args.iter().any(|a| a == "--reset") {
        println!("ultar-modem --reset: CP zurücksetzen (MODEM_OFF + RESET)");
        if let Err(e) = reset() {
            eprintln!("reset: {e}");
        }
        return;
    }

    println!("ultar-modem (modem_v1/ss310ap). ON=0x{:x} XMIT_BOOT=0x{:x} SECURITY_REQ=0x{:x} DL_START=0x{:x} BOOT_ON=0x{:x}",
        IOCTL_MODEM_ON, IOCTL_MODEM_XMIT_BOOT, IOCTL_SECURITY_REQ, IOCTL_MODEM_DL_START, IOCTL_MODEM_BOOT_ON);

    let fw_path = args
        .iter()
        .position(|a| a == "--fw")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let Some(path) = fw_path else {
        println!(
            "Aufruf: ultar-modem --auto | --fw <cp.bin> [--nv <nv_data.bin>] [--arm] | --reset"
        );
        return;
    };

    let mut buf = Vec::new();
    if let Err(e) = File::open(&path).and_then(|mut f| f.read_to_end(&mut buf)) {
        eprintln!("Firmware {path} nicht lesbar: {e}");
        return;
    }
    println!("Firmware {path}: {} Bytes", buf.len());

    let toc = parse_toc(&buf);
    println!("TOC-Einträge ({}):", toc.len());
    for e in &toc {
        println!(
            "  {:<10} b_offset=0x{:08x} m_offset=0x{:08x} size={} crc=0x{:08x} count={}",
            name_str(&e.name),
            { e.b_offset },
            { e.m_offset },
            { e.size },
            { e.crc },
            { e.toc_count }
        );
    }

    if !args.iter().any(|a| a == "--arm") {
        println!("\nTrockentest fertig. Mit --arm wird der CP wirklich gebootet (Originalfirmware nötig).");
        return;
    }
    set_sim_configuration();
    let nv = args
        .iter()
        .position(|a| a == "--nv")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| std::fs::read(p).map_err(|e| eprintln!("NV {p}: {e}")).ok());
    if let Err(e) = boot(&buf, &toc, nv) {
        eprintln!("Boot fehlgeschlagen: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::sim_count_from_prop;
    #[test]
    fn factory_prop_wie_cbd() {
        assert_eq!(sim_count_from_prop("ro.multisim.simslotcount=2\n"), Some(2));
        assert_eq!(sim_count_from_prop("1=1"), Some(1));
        assert_eq!(sim_count_from_prop("kein Gleichheitszeichen"), None);
        assert_eq!(sim_count_from_prop("x=ds"), None);
    }
}
