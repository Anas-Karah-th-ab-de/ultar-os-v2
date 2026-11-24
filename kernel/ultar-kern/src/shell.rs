//! Kern-Konsole: Bedienung über Tastatur (Bildschirm) und serielle Schnittstelle (Fernsteuerung, Tests).

use crate::driver::{BLOCK, DEVICES, NET};
use crate::{arch, kprint, kprintln, mem};
use alloc::string::String;
use alloc::vec;

pub fn run(boot_rt: *mut crate::efi::RuntimeServices) -> ! {
    let mut line = String::new();
    let mut utf8: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    prompt();
    loop {
        let c = match crate::console::serial_get() {
            Some(b) if b < 0x80 && utf8.is_empty() => Some(b as char),
            Some(b) => {
                utf8.push(b);
                let need = match utf8[0] {
                    0xc0..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf7 => 4,
                    _ => 1,
                };
                if utf8.len() >= need {
                    let r = core::str::from_utf8(&utf8).ok().and_then(|t| t.chars().next());
                    utf8.clear();
                    r
                } else {
                    None
                }
            }
            None => crate::drivers::ps2kbd::poll(),
        };
        let Some(c) = c else {
            leerlauf();
            arch::pause();
            continue;
        };
        match c {
            '\r' | '\n' => {
                kprintln!();
                let cmd = core::mem::take(&mut line);
                exec(cmd.trim(), boot_rt);
                prompt();
            }
            '\x08' | '\x7f' => {
                if line.pop().is_some() {
                    kprint!("\x08 \x08");
                }
            }
            c if !c.is_control() && !('\u{E000}'..='\u{E0FF}').contains(&c) => {
                line.push(c);
                kprint!("{c}");
            }
            _ => {}
        }
    }
}

static LETZTES_NETZ: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Leerlauf der Konsole/Startseite: Netz bedienen (Verbund-Meldungen, ARP) und Hintergrund-Dienste weiterlaufen lassen
pub fn leerlauf() {
    let t = arch::uptime_ms();
    if t.wrapping_sub(LETZTES_NETZ.load(core::sync::atomic::Ordering::Relaxed)) >= 20 {
        LETZTES_NETZ.store(t, core::sync::atomic::Ordering::Relaxed);
        if crate::net::CONFIG.lock().is_some() {
            crate::socket::poll();
        }
        crate::proc::hintergrund_schritt();
    }
}

/// Entwickler-Zugang (SSH) starten, wenn eingeschaltet und vorhanden: `/etc/ultar/entwickler` oder
/// `/data/ultar/entwickler/an` (`…/aus` schaltet ab); Port aus `/data/ultar/entwickler/port` (Standard 2222). Öffnet nur diesen Port.
pub fn entwickler_starten() {
    let an = crate::fs::stat("/data/ultar/entwickler/aus").is_none()
        && (crate::fs::stat("/etc/ultar/entwickler").is_some() || crate::fs::stat("/data/ultar/entwickler/an").is_some());
    let Some(prog) = ["/usr/bin/ultar-sshd", "/boot/efi/bin/ultar-sshd"].iter().find(|p| matches!(crate::fs::stat(p), Some((0, _)))) else { return };
    if !an {
        return;
    }
    let port: u16 = crate::fs::read_all("/data/ultar/entwickler/port")
        .and_then(|d| core::str::from_utf8(&d).ok().and_then(|s| s.trim().parse().ok()))
        .unwrap_or(2222);
    let _ = crate::firewall::befehl(&alloc::format!("offen {port}"));
    let envp: alloc::vec::Vec<String> = ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", "HOME=/root", "USER=root", "TERM=linux", "LANG=C.UTF-8", "ULTAR_KERN=1"]
        .iter()
        .map(|s| String::from(*s))
        .collect();
    match crate::proc::spawn_hintergrund(prog, alloc::vec![String::from("ultar-sshd")], envp) {
        Ok(pid) => kprintln!("[entwickler] SSH-Zugang: Port {port} offen (nur Schlüssel), Prozess {pid}"),
        Err(e) => kprintln!("[entwickler] SSH-Zugang startet nicht: {e}"),
    }
}

fn prompt() {
    crate::console::set_color(0xE6C454);
    let ar = crate::drivers::ps2kbd::ARABIC.load(core::sync::atomic::Ordering::Relaxed);
    kprint!("ultar-kern:{}{}> ", cwd(), if ar { " [ع]" } else { "" });
    crate::console::set_color(0xE6E8EE);
}

/// Arbeitsverzeichnis der Konsole
pub fn cwd() -> String {
    let c = crate::proc::SHELL_CWD.lock().clone();
    if c.is_empty() { String::from("/") } else { c }
}

fn is_dir(p: &str) -> bool {
    matches!(crate::fs::stat(p), Some((1, _))) || matches!(crate::fs::open(p), Some(crate::fs::Node::Dir(_)))
}

/// Buchstabenvarianten angleichen (أ إ آ → ا, ى → ي, ة → ه), Tatweel und Vokalzeichen weg
fn ar_norm(s: &str) -> String {
    s.chars()
        .filter(|&c| !matches!(c as u32, 0x0640 | 0x064b..=0x0652 | 0x0670))
        .map(|c| match c {
            'أ' | 'إ' | 'آ' => 'ا',
            'ى' => 'ي',
            'ة' => 'ه',
            c => c,
        })
        .collect()
}

/// Arabische Befehlsnamen → Kernbefehl
fn ar_befehl(c: &str) -> Option<&'static str> {
    Some(match ar_norm(c).as_str() {
        "مساعده" | "تعليمات" => "hilfe-ar",
        "معلومات" => "info",
        "انويه" | "الانويه" | "معالجات" => "kerne",
        "اجهزه" | "الاجهزه" => "geraete",
        "تعريفات" | "التعريفات" => "treiber",
        "ورشه" | "الورشه" => "werkstatt",
        "شاشه" | "الشاشه" => "bildschirm",
        "صوت" | "نغمه" => "ton",
        "اقراص" | "الاقراص" => "platten",
        "اقسام" | "الاقسام" => "partitionen",
        "قراءه" | "اقرا" => "lesen",
        "شبكه" | "الشبكه" => "netz",
        "عنوان" => "ipfest",
        "اتحاد" | "شبكه_الاجهزه" => "verbund",
        "ملفات" | "الملفات" | "قائمه" => "ls",
        "اعرض" | "عرض" => "zeige",
        "شغل" | "تشغيل" => "start",
        "عمليات" | "العمليات" => "prozesse",
        "تتبع" => "spur",
        "لوحه_المفاتيح" | "لوحه" | "مفاتيح" => "tastatur",
        "سجل" | "السجل" => "protokoll",
        "اعاده_تشغيل" | "اعاده" => "neustart",
        "ايقاف" | "اطفاء" => "aus",
        "انتقل" | "اذهب" | "ادخل" => "cd",
        "المسار" | "اين" => "pwd",
        "مسح" | "نظف" => "clear",
        "لغه" | "اللغه" => "sprache",
        "تثبيت" | "ثبت" => "installieren",
        "واجهه" | "الواجهه" | "سطح_المكتب" => "oberflaeche",
        "بدء" | "نظام" => "init",
        "ارسل_نبض" | "نبض" => "ping",
        _ => return None,
    })
}

/// Arabische Werte für Schalter
fn ar_wert(a: &str) -> String {
    match ar_norm(a).as_str() {
        "تشغيل" | "شغل" | "نعم" => String::from("an"),
        "ايقاف" | "اطفاء" | "لا" => String::from("aus"),
        "عربي" | "عربيه" | "العربيه" => String::from("ar"),
        "الماني" | "المانيه" => String::from("de"),
        "انجليزي" | "انكليزي" => String::from("us"),
        _ => String::from(a),
    }
}

fn exec(cmd: &str, rt: *mut crate::efi::RuntimeServices) {
    let mut words = split_args(cmd);
    let Some(first) = words.first().cloned() else { return };
    // arabischer Befehl? → Kernbefehl, Schalterwerte übersetzen
    if let Some(k) = ar_befehl(&first) {
        words[0] = String::from(k);
        for w in words.iter_mut().skip(1) {
            *w = ar_wert(w);
        }
    }
    let c = words[0].as_str();
    let args: alloc::vec::Vec<&str> = words[1..].iter().map(|s| s.as_str()).collect();
    match c {
        "cd" => {
            let ziel = match args.first().copied() {
                None | Some("~") => if is_dir("/root") { String::from("/root") } else { String::from("/") },
                Some(a) => crate::fs::normalize(&cwd(), a),
            };
            if is_dir(&ziel) {
                *crate::proc::SHELL_CWD.lock() = ziel;
            } else {
                kprintln!("cd: {ziel}: {}", crate::sprache::t("kein_verz"));
            }
        }
        "pwd" => kprintln!("{}", cwd()),
        "installieren" | "install" | "einrichten" => crate::install::run(rt),
        "oberflaeche" | "ui" | "startseite" => crate::startseite::run(),
        "desktop" | "سطح-المكتب" => {
            if !desktop_starten() {
                kprintln!("ultar-shell nicht gefunden (/usr/bin/ultar-shell)");
            }
        }
        "clear" | "leeren" => kprint!("\x1b[2J\x1b[H"),
        "hilfe-ar" => {
            kprintln!("الأوامر العربية (والأمر المقابل):");
            kprintln!("مساعدة            هذه القائمة              (hilfe)");
            kprintln!("معلومات           المعالج والذاكرة          (info)");
            kprintln!("أنوية             أنوية المعالج            (kerne)");
            kprintln!("أجهزة             أجهزة PCI مع التعريف     (geraete)");
            kprintln!("تعريفات           التعريفات المدمجة         (treiber)");
            kprintln!("ورشة              أجهزة بلا تعريف          (werkstatt)");
            kprintln!("شاشة [1920x1080]  دقة الشاشة               (bildschirm)");
            kprintln!("صوت [هرتز] [مللي]  نغمة اختبار الصوت        (ton)");
            kprintln!("أقراص / أقسام 0   الأقراص والأقسام          (platten, partitionen)");
            kprintln!("شبكة / نبض <ip>   حالة الشبكة               (netz, ping)");
            kprintln!("اتحاد تشغيل|إيقاف  اتحاد أجهزة Ultar        (verbund)");
            kprintln!("ملفات [مسار]      محتوى المجلد             (ls)");
            kprintln!("اعرض <ملف>        عرض ملف                  (zeige)");
            kprintln!("انتقل <مجلد>      تغيير المجلد             (cd)");
            kprintln!("المسار            المجلد الحالي            (pwd)");
            kprintln!("شغل <برنامج>      تشغيل برنامج             (start)");
            kprintln!("عمليات            العمليات الجارية          (prozesse)");
            kprintln!("لوحة عربي|ألماني|إنجليزي  تخطيط لوحة المفاتيح (tastatur) – أو Alt+Shift");
            kprintln!("سجل               سجل النواة               (protokoll)");
            kprintln!("مسح               مسح الشاشة               (clear)");
            kprintln!(concat!("تثبيت             تثبيت ", ultar_brand::product!(), " على قرص   (installieren)"));
            kprintln!("لغة عربي|ألماني|إنجليزي  لغة النظام     (sprache)");
            kprintln!("إعادة_تشغيل | إيقاف  (neustart | aus)");
        }
        "hilfe" | "help" | "?" if crate::sprache::get() == crate::sprache::AR => exec("hilfe-ar", rt),
        "hilfe" | "help" | "?" => {
            kprintln!("info          Prozessor, Speicher, Laufzeit");
            kprintln!("geraete       alle PCI-Geräte mit Treiber");
            kprintln!("treiber       eingebaute Treiber");
            kprintln!("werkstatt     Geräte ohne Treiber als Bauplan (für tools/treiber-werkstatt)");
            kprintln!("anzeige [BxH]  Auflösung zeigen/umstellen (auch: bildschirm);  ton [hz] [ms]  Prüfton;  smbus [adr reg];  zufall");
            kprintln!("platten       Datenträger;  partitionen <n>;  lesen <n> <sektor>");
            kprintln!("netz          Netzwerk-Zustand;  dhcp;  ping <ip>;  ipfest <a.b.c.d/nn> [router]");
            kprintln!("firewall [offen|zu [tcp|udp] <port> | ping an|aus]  Brandmauer (Standard: alles zu);  sicherheit");
            kprintln!("gruppen       Gruppen: Speicher und Prozesse je Gruppe, Grenzen, Schutzregeln");
            kprintln!("verbund [an|aus]  Ultar-Verbund: Schalter und Geräte im Netz (auch /proc/ultar/verbund)");
            kprintln!("dateien [pfad]  Dateibaum (FAT der Startpartition + RAM);  zeige <datei>");
            kprintln!("start <programm> [argumente]  Linux-Programm (statisch, x86_64) starten – auch direkt: busybox sh");
            kprintln!("init [pfad]   ultar-init als PID 1 starten (Dienste aus /etc/ultar/services)");
            kprintln!("prozesse      laufende Prozesse;  kerne  Prozessorkerne;  spur an|aus  Systemaufrufe zeigen");
            kprintln!("cd <pfad>     Verzeichnis wechseln;  pwd  aktuelles Verzeichnis;  clear  Bild löschen");
            kprintln!("tastatur de|us|ar   (Alt+Shift schaltet Arabisch um)");
            kprintln!("installieren  grafische Installation auf eine andere Platte (تثبيت)");
            kprintln!("sprache ar|de|en  Systemsprache (wird gespeichert)");
            kprintln!("مساعدة        arabische Befehle (z. B. معلومات، ملفات، انتقل، شبكة)");
            kprintln!("geraet        Geräteart (PC/Laptop/Server/Handy), Ausgabe, Grafik");
            kprintln!("anzeige [BxH] Anzeigen und Auflösungen; z. B. anzeige 1920x1080");
            kprintln!("protokoll     Kernprotokoll");
            kprintln!("neustart | aus");
        }
        "info" => info(),
        "kerne" => {
            let (on, all) = crate::apic::cpu_count();
            kprintln!("{on} von {all} Prozessorkernen aktiv, Zeitgeber {} Hz, {} Takte", crate::apic::HZ, crate::apic::ticks());
        }
        "dateien" => {
            let p = crate::fs::normalize(&cwd(), args.first().copied().unwrap_or("."));
            match crate::fs::open(&p) {
                Some(crate::fs::Node::Dir(v)) => {
                    for e in v {
                        if e.dir {
                            kprintln!("  {}/", e.name);
                        } else {
                            kprintln!("  {:<28} {:>10}", e.name, e.size);
                        }
                    }
                }
                Some(_) => kprintln!("{p}: keine Verzeichnis"),
                None => kprintln!("{p}: nicht gefunden"),
            }
        }
        "zeige" => {
            let p = crate::fs::normalize(&cwd(), args.first().copied().unwrap_or(""));
            match crate::fs::read_all(&p) {
                Some(d) => kprint!("{}", String::from_utf8_lossy(&d[..d.len().min(16384)])),
                None => kprintln!("{p}: nicht gefunden"),
            }
        }
        "spur" => {
            let on = args.first() != Some(&"aus");
            crate::syscall::TRACE.store(on, core::sync::atomic::Ordering::Relaxed);
            kprintln!("Systemaufruf-Spur {}", if on { "an" } else { "aus" });
        }
        "prozesse" | "ps" => {
            for (pid, ppid, name, st) in crate::proc::list() {
                kprintln!("{pid:>5} {ppid:>5}  {name:<20} {st:?}");
            }
        }
        "init" => {
            // ultar-init als PID 1 auf dem eigenen Kern
            let path = args.first().copied().unwrap_or("/sbin/ultar-init");
            let Some(p) = find_program(path) else { return kprintln!("{path}: nicht gefunden") };
            let envp = ["PATH=/usr/sbin:/usr/bin:/sbin:/bin", "HOME=/root", "TERM=linux", "ULTAR_EDITION=desktop"].iter().map(|s| String::from(*s)).collect();
            match crate::proc::spawn_init(&p, alloc::vec![String::from("ultar-init")], envp) {
                Ok(_) => crate::proc::run_all(),
                Err(e) => kprintln!("{e}"),
            }
        }
        "start" => {
            if let Some(&prog) = args.first() {
                run_program(prog, &args);
            } else {
                kprintln!("start <programm> [argumente]");
            }
        }
        "geraete" => {
            for b in DEVICES.lock().iter() {
                let d = &b.dev;
                let drv = b.driver.unwrap_or(if b.error.is_some() { "FEHLER" } else { "-" });
                kprintln!("{}  {:<26} {}", d.id(), d.class_name(), drv);
            }
        }
        "treiber" => {
            for d in crate::drivers::ALL {
                let n = DEVICES.lock().iter().filter(|b| b.driver == Some(d.name())).count();
                kprintln!("{:<14} {:<10} {} Gerät(e)", d.name(), d.origin(), n);
            }
            kprintln!("{:<14} {:<10} Tastatur (Abfrage)", "ps2kbd", "hand");
        }
        "werkstatt" => crate::werkstatt::report(),
        "ton" => {
            let hz = args.first().and_then(|a| a.parse::<u64>().ok()).unwrap_or(880);
            let ms = args.get(1).and_then(|a| a.parse::<u64>().ok()).unwrap_or(400);
            match crate::drivers::gen_hda::info() {
                None => kprintln!("ton: keine Tonausgabe (kein HD-Audio-Gerät mit Treiber)"),
                Some(i) => match crate::drivers::gen_hda::ton(hz, ms) {
                    Ok(n) => kprintln!("[ton] {hz} Hz, {} ms ({n} Byte) über {i}", n as u64 * 1000 / 192_000),
                    Err(e) => kprintln!("ton: {e}"),
                },
            }
        }
        "smbus" => {
            use crate::drivers::gen_smbus as s;
            let zahl = |a: &str| u8::from_str_radix(a.trim_start_matches("0x"), 16).ok();
            match (args.first().and_then(|a| zahl(a)), args.get(1).and_then(|a| zahl(a))) {
                (Some(adr), Some(reg)) => match s::lies(adr, reg) {
                    Some(v) => kprintln!("smbus {adr:#04x} Register {reg:#04x} = {v:#04x}"),
                    None => kprintln!("smbus: keine Antwort von {adr:#04x}"),
                },
                _ => match s::geraete() {
                    None => kprintln!("smbus: kein Controller mit Treiber"),
                    Some(g) => {
                        kprintln!("{} Gerät(e) – lesen: smbus <adr-hex> <reg-hex>", g.len());
                        for (adr, typ) in g {
                            kprintln!("{adr:#04x}  Speicherriegel-Kennung (SPD): {}", s::speichertyp(typ));
                        }
                    }
                },
            }
        }
        "zufall" => {
            let mut b = [0u8; 16];
            let n = crate::drivers::virtio_rng::fuelle(&mut b);
            let hex: alloc::string::String = b[..n].iter().map(|x| alloc::format!("{x:02x}")).collect();
            if n > 0 {
                kprintln!("virtio-rng: {hex}");
            }
            kprintln!("Kern-Vorrat: {:016x}", crate::arch::zufall());
        }
        "geraet" | "geraeteart" => crate::kprint!("{}", crate::geraet::text()),
        "anzeige" | "aufloesung" | "bildschirm" => crate::grafik::befehl(args.first().copied()),
        "platten" => {
            for (i, b) in BLOCK.lock().iter().enumerate() {
                kprintln!("{i}: {}  {} MB  Sektor {} B", b.name(), b.sectors() * b.sector_size() as u64 >> 20, b.sector_size());
            }
        }
        "lesen" => {
            let n: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            let lba: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let mut bl = BLOCK.lock();
            let Some(d) = bl.get_mut(n) else { return kprintln!("keine Platte {n}") };
            let mut buf = vec![0u8; d.sector_size()];
            match d.read(lba, &mut buf) {
                Ok(()) => hexdump(&buf[..256]),
                Err(e) => kprintln!("Fehler: {e}"),
            }
        }
        "partitionen" => {
            let n: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            partitions(n);
        }
        "netz" => {
            match *crate::net::CONFIG.lock() {
                Some(c) => kprintln!("IP {}  Maske {}  Router {}  DNS {}", ip(c.ip), ip(c.mask), ip(c.gw), ip(c.dns)),
                None => kprintln!("keine Adresse ({} Netzwerkgerät(e)) – `dhcp`", NET.lock().len()),
            }
            for l in crate::socket::list() {
                kprintln!("  {l}");
            }
        }
        "dhcp" => dhcp(),
        "ipfest" => {
            let Some(a) = args.first() else { return kprintln!("ipfest <a.b.c.d/nn> [router]") };
            let (ipt, nn) = a.split_once('/').unwrap_or((a, "24"));
            let (Some(i), Ok(nn)) = (crate::net::parse_ip(ipt), nn.parse::<u32>()) else { return kprintln!("ungültig") };
            let m = if nn == 0 { 0 } else { u32::MAX << (32 - nn.min(32)) }.to_be_bytes();
            let gw = args.get(1).and_then(|g| crate::net::parse_ip(g)).unwrap_or([i[0], i[1], i[2], 1]);
            *crate::net::CONFIG.lock() = Some(crate::net::Config { ip: i, mask: m, gw, dns: gw });
            kprintln!("[netz] fest: IP {} Maske {} Router {}", ip(i), ip(m), ip(gw));
        }
        "firewall" => match crate::firewall::befehl(&args.join(" ")) {
            Ok(t) => kprint!("{t}{}", if t.ends_with('\n') { "" } else { "\n" }),
            Err(e) => kprintln!("{e}"),
        },
        "sicherheit" => kprint!("{}", crate::sicherheit_text()),
        // resource groups: use, limits and the dynamic rules (same text as /proc/ultar/gruppen)
        "gruppen" => kprint!("{}", crate::groups::files::table()),
        "verbund" => {
            match args.first().copied() {
                Some("an") => crate::verbund::schalten(true),
                Some("aus") => crate::verbund::schalten(false),
                _ => {}
            }
            crate::socket::poll();
            let g = crate::verbund::geraete();
            kprintln!("Ultar-Verbund {} – {} Gerät(e) im Netz", if crate::verbund::aktiv() { "AN" } else { "AUS" }, g.len());
            for x in g {
                kprintln!("  {}  {:<16} {:<8} {} Port {}  {}/{} Kerne frei", x.kennung, x.name, x.ausgabe, ip(x.ip), x.port, x.frei, x.kerne);
            }
        }
        "ping" => {
            let Some(dst) = args.first().and_then(|s| crate::net::parse_ip(s)) else { return kprintln!("ping <a.b.c.d>") };
            match crate::net::ping(0, dst) {
                Some(ms) => kprintln!("Antwort von {} in {ms} ms", ip(dst)),
                None => kprintln!("keine Antwort von {}", ip(dst)),
            }
        }
        "tastatur" => {
            use core::sync::atomic::Ordering::Relaxed;
            let wahl = args.first().copied().unwrap_or("de");
            let ar = wahl == "ar";
            crate::drivers::ps2kbd::ARABIC.store(ar, Relaxed);
            if !ar {
                crate::drivers::ps2kbd::GERMAN.store(wahl != "us", Relaxed);
            }
            match wahl {
                "ar" => kprintln!("لوحة المفاتيح: عربي (Alt+Shift للتبديل)"),
                "us" => kprintln!("Tastatur: US"),
                _ => kprintln!("Tastatur: deutsch"),
            }
        }
        "protokoll" => {
            let mut v = alloc::vec::Vec::new();
            crate::klog::dump(|b| v.extend_from_slice(b));
            kprint!("{}", String::from_utf8_lossy(&v));
        }
        "neustart" | "reboot" => {
            kprintln!("{}", crate::sprache::t("neustart"));
            crate::power::neustart()
        }
        "aus" | "poweroff" => {
            kprintln!("{}", crate::sprache::t("aus"));
            crate::power::aus()
        }
        "sprache" | "language" => {
            match args.first().and_then(|a| crate::sprache::aus_code(a)) {
                Some(l) => {
                    crate::sprache::set(l);
                    crate::sprache::speichern();
                }
                None => kprintln!("sprache ar|de|en"),
            }
            kprintln!("{}", crate::sprache::code());
        }
        _ => {
            let mut all = alloc::vec![c];
            all.extend(args.iter().copied());
            if find_program(c).is_some() {
                run_program(c, &all);
            } else {
                kprintln!("{}: {c} ({})", crate::sprache::t("unbekannt"), crate::sprache::t("hilfe"))
            }
        }
    }
}

/// Befehlszeile zerlegen: Leerzeichen trennen, '…' und "…" halten zusammen
fn split_args(s: &str) -> alloc::vec::Vec<String> {
    let mut out = alloc::vec::Vec::new();
    let mut cur = String::new();
    let mut q: Option<char> = None;
    let mut any = false;
    for c in s.chars() {
        match (q, c) {
            (None, '\'') | (None, '"') => {
                q = Some(c);
                any = true;
            }
            (Some(x), c) if c == x => q = None,
            (None, c) if c.is_whitespace() => {
                if any {
                    out.push(core::mem::take(&mut cur));
                    any = false;
                }
            }
            (_, c) => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

fn find_program(name: &str) -> Option<String> {
    let cands: alloc::vec::Vec<String> = if name.contains('/') {
        alloc::vec![crate::fs::normalize(&cwd(), name)]
    } else {
        ["/bin/", "/usr/bin/", "/ultar/bin/", "/boot/efi/bin/", "/"].iter().map(|d| alloc::format!("{d}{name}")).collect()
    };
    cands.into_iter().find(|p| matches!(crate::fs::stat(p), Some((0, _))))
}

/// Programm im Vordergrund ausführen, bis es (und alle Kinder) beendet ist
fn run_program(name: &str, args: &[&str]) {
    let Some(path) = find_program(name) else { return kprintln!("{name}: nicht gefunden (dateien /bin)") };
    let argv: alloc::vec::Vec<String> = args.iter().map(|s| String::from(*s)).collect();
    let envp = ["PATH=/bin:/ultar/bin", "HOME=/", "TERM=linux", "USER=root", "SHELL=/bin/sh", "PS1=ultar$ "].iter().map(|s| String::from(*s)).collect();
    match crate::proc::spawn(&path, argv, envp) {
        Ok(_) => crate::proc::run_all(),
        Err(e) => kprintln!("{e}"),
    }
}

/// Desktop-Oberfläche (ultar-shell im Ultar-Bereich) als Programm starten und warten, bis sie endet.
/// false: nicht vorhanden – dann zeigt der Kern seine eigene Startseite.
pub fn desktop_starten() -> bool {
    let Some(path) = ["/usr/bin/ultar-shell", "/usr/lib/ultar/ultar-shell", "/boot/efi/bin/ultar-shell"].iter().find(|p| matches!(crate::fs::stat(p), Some((0, _)))) else { return false };
    if crate::console::framebuffer().is_none() {
        return false;
    }
    let code = crate::sprache::code();
    let envp: alloc::vec::Vec<String> = [
        String::from("PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"),
        String::from("HOME=/root"),
        String::from("USER=root"),
        String::from("TERM=linux"),
        // Ausgabe aus der Geräteart – eine feste Einstellung in /etc/ultar/edition hat Vorrang (liest ultar-edition selbst)
        if crate::fs::stat("/etc/ultar/edition").is_some() { String::from("ULTAR_EDITION_VON=datei") } else { alloc::format!("ULTAR_EDITION={}", crate::geraet::art().ausgabe()) },
        alloc::format!("ULTAR_GERAET={}", crate::geraet::art().name()),
        String::from("ULTAR_KERN=1"),
        alloc::format!("ULTAR_LANG={code}"),
        alloc::format!("ULTAR_SYSTEMSPRACHE={code}"),
        String::from("LANG=C.UTF-8"),
    ]
    .into_iter()
    .collect();
    kprintln!("[oberflaeche] Desktop: {path} ({code})");
    match crate::proc::spawn(path, alloc::vec![String::from("ultar-shell")], envp) {
        Ok(_) => {
            crate::proc::run_all();
            kprintln!("[oberflaeche] Desktop beendet");
            true
        }
        Err(e) => {
            kprintln!("[oberflaeche] Desktop startet nicht: {e}");
            false
        }
    }
}

pub fn info() {
    let mut nb = [0u8; 48];
    let (total, heap, used, regions) = mem::stats();
    kprintln!(concat!("Ultar-Kern {} · ", ultar_brand::vendor!(), " · ", ultar_brand::developer!()), env!("CARGO_PKG_VERSION"));
    kprintln!("Prozessor: {} ({} MHz)", arch::cpu_name(&mut nb), arch::tsc_khz() / 1000);
    kprintln!("Speicher frei: {} MB in {regions} Bereichen, Kern-Heap {} MB ({} KB belegt)", total >> 20, heap >> 20, used >> 10);
    let (on, all) = crate::apic::cpu_count();
    kprintln!("Prozessorkerne: {on} von {all} aktiv");
    kprintln!("Laufzeit: {} ms", arch::uptime_ms());
}

pub fn dhcp() {
    if NET.lock().is_empty() {
        return kprintln!("[netz] kein Netzwerkgerät mit Treiber");
    }
    match crate::net::dhcp(0) {
        Some(c) => {
            kprintln!("[netz] DHCP: IP {} Router {} DNS {}", ip(c.ip), ip(c.gw), ip(c.dns));
            // Namensauflösung für Programme (glibc liest /etc/resolv.conf)
            crate::fs::write_file("/etc/resolv.conf", alloc::format!("nameserver {}\n", ip(c.dns)).into_bytes());
            // Zustand für die Oberfläche (wie ultar-net unter Linux: /run/ultar/net/<schnittstelle>)
            crate::fs::write_file("/run/ultar/net/eth0", alloc::format!("state=verbunden\nkind=eth\nip={}\ngw={}\ndns={}\n", ip(c.ip), ip(c.gw), ip(c.dns)).into_bytes());
        }
        None => kprintln!("[netz] DHCP: keine Antwort"),
    }
}

fn ip(a: [u8; 4]) -> String {
    alloc::format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
}

fn hexdump(b: &[u8]) {
    for (i, row) in b.chunks(16).enumerate() {
        kprint!("{:04x}  ", i * 16);
        for x in row {
            kprint!("{x:02x} ");
        }
        kprint!(" ");
        for &x in row {
            kprint!("{}", if (0x20..0x7f).contains(&x) { x as char } else { '.' });
        }
        kprintln!();
    }
}

/// GPT lesen und Partitionen mit Namen anzeigen (findet z. B. ULTAR-EFI, ultar-compat, ultar-data)
pub fn partitions(n: usize) {
    let mut bl = BLOCK.lock();
    let Some(d) = bl.get_mut(n) else { return kprintln!("keine Platte {n}") };
    let ss = d.sector_size();
    let mut h = vec![0u8; ss];
    if d.read(1, &mut h).is_err() || &h[0..8] != b"EFI PART" {
        return kprintln!("{}: keine GPT-Partitionstabelle", d.name());
    }
    let lba = u64::from_le_bytes(h[72..80].try_into().unwrap());
    let count = u32::from_le_bytes(h[80..84].try_into().unwrap()).min(128) as usize;
    let esz = u32::from_le_bytes(h[84..88].try_into().unwrap()) as usize;
    let mut t = vec![0u8; (count * esz).div_ceil(ss) * ss];
    if let Err(e) = d.read(lba, &mut t) {
        return kprintln!("Fehler: {e}");
    }
    kprintln!("{}: GPT", d.name());
    for i in 0..count {
        let e = &t[i * esz..(i + 1) * esz];
        if e[..16].iter().all(|&x| x == 0) {
            continue;
        }
        let first = u64::from_le_bytes(e[32..40].try_into().unwrap());
        let last = u64::from_le_bytes(e[40..48].try_into().unwrap());
        let name: String = char::decode_utf16(e[56..128].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&u| u != 0)).map(|r| r.unwrap_or('?')).collect();
        kprintln!("  {}  {:<16} {} MB", i + 1, name, (last - first + 1) * ss as u64 >> 20);
    }
}
