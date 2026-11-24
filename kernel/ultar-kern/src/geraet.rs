//! Geräteart erkennen: PC, Laptop, Server, Tablet oder Handy – und daraus die passende Ausgabe von Ultar OS
//! (desktop / server / phone) ableiten. Quellen der Reihe nach: SMBIOS-Gehäusetyp (Norm: DMTF SMBIOS 3.x, Typ 3),
//! Akku laut SMBIOS (Typ 22), sonst ACPI-FADT „Preferred_PM_Profile“. Ergebnis in `/proc/ultar/geraet`,
//! Konsole `geraet` und als `ULTAR_EDITION`/`ULTAR_GERAET` für die Oberfläche.
//!
//! Beispiel server3 (Beelink GTi15 Ultra): FADT meldet „Mobil“ (Laptop-Chip), SMBIOS aber „Mini PC“ → PC.
//! Deshalb hat SMBIOS Vorrang.

use alloc::format;
use alloc::string::String;
use spin::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Art {
    Pc,
    Laptop,
    Server,
    Tablet,
    Handy,
}

impl Art {
    pub fn name(self) -> &'static str {
        match self {
            Art::Pc => "pc",
            Art::Laptop => "laptop",
            Art::Server => "server",
            Art::Tablet => "tablet",
            Art::Handy => "handy",
        }
    }
    /// Ausgabe von Ultar OS (wie crates/ultar-edition)
    pub fn ausgabe(self) -> &'static str {
        match self {
            Art::Pc | Art::Laptop => "desktop",
            Art::Server => "server",
            Art::Tablet | Art::Handy => "phone",
        }
    }
    pub fn anzeige_name(self) -> &'static str {
        match (self, crate::sprache::get() == crate::sprache::AR) {
            (Art::Pc, false) => "PC",
            (Art::Laptop, false) => "Laptop",
            (Art::Server, false) => "Server",
            (Art::Tablet, false) => "Tablet",
            (Art::Handy, false) => "Handy",
            (Art::Pc, true) => "حاسوب مكتبي",
            (Art::Laptop, true) => "حاسوب محمول",
            (Art::Server, true) => "خادم",
            (Art::Tablet, true) => "جهاز لوحي",
            (Art::Handy, true) => "هاتف",
        }
    }
}

/// SMBIOS-Gehäusetyp → Geräteart (None = nichtssagend, z. B. „Other“/„Unknown“)
pub fn aus_gehaeuse(t: u8) -> Option<Art> {
    match t & 0x7f {
        3 | 4 | 5 | 6 | 7 | 13 | 15 | 16 | 24 | 34 | 35 | 36 => Some(Art::Pc),
        8 | 9 | 10 | 14 | 31 | 32 => Some(Art::Laptop),
        17 | 23 | 25 | 28 | 29 => Some(Art::Server),
        30 => Some(Art::Tablet),
        11 => Some(Art::Handy),
        _ => None,
    }
}

fn gehaeuse_text(t: u8) -> &'static str {
    match t & 0x7f {
        3 => "Desktop",
        4 => "Low-Profile-Desktop",
        6 => "Mini-Tower",
        7 => "Tower",
        8 => "Tragbar",
        9 => "Laptop",
        10 => "Notebook",
        11 => "Handgerät",
        13 => "All-in-One",
        14 => "Sub-Notebook",
        17 => "Server-Gehäuse",
        23 => "Rack",
        28 => "Blade",
        30 => "Tablet",
        31 => "Convertible",
        32 => "Detachable",
        34 => "Embedded-PC",
        35 => "Mini-PC",
        36 => "Stick-PC",
        1 => "Sonstiges",
        2 => "Unbekannt",
        _ => "anderes",
    }
}

/// ACPI-FADT Preferred_PM_Profile → Geräteart
pub fn aus_pm_profil(p: u8) -> Option<Art> {
    match p {
        1 | 3 | 6 => Some(Art::Pc),
        2 => Some(Art::Laptop),
        4 | 5 | 7 => Some(Art::Server),
        8 => Some(Art::Tablet),
        _ => None,
    }
}

pub struct Info {
    pub art: Art,
    pub quelle: &'static str,
    pub gehaeuse: u8,
    pub pm_profil: u8,
    pub akku: bool,
    pub hersteller: String,
    pub modell: String,
}

static INFO: Mutex<Option<Info>> = Mutex::new(None);

unsafe fn rd8(a: u64) -> u8 {
    core::ptr::read_volatile(a as *const u8)
}
unsafe fn rd16(a: u64) -> u16 {
    core::ptr::read_unaligned(a as *const u16)
}
unsafe fn rd32(a: u64) -> u32 {
    core::ptr::read_unaligned(a as *const u32)
}
unsafe fn rd64(a: u64) -> u64 {
    core::ptr::read_unaligned(a as *const u64)
}

/// n-te Zeichenkette (1-basiert) hinter einer SMBIOS-Struktur
unsafe fn smbios_str(p: u64, len: u8, n: u8) -> String {
    if n == 0 {
        return String::new();
    }
    let mut q = p + len as u64;
    let mut i = 1u8;
    while i < n {
        while rd8(q) != 0 {
            q += 1;
        }
        q += 1;
        if rd8(q) == 0 {
            return String::new();
        }
        i += 1;
    }
    let mut s = String::new();
    while rd8(q) != 0 && s.len() < 64 {
        let c = rd8(q);
        s.push(if c.is_ascii_graphic() || c == b' ' {
            c as char
        } else {
            '?'
        });
        q += 1;
    }
    let t = s.trim();
    if t.eq_ignore_ascii_case("Default string") || t.eq_ignore_ascii_case("To be filled by O.E.M.")
    {
        String::new()
    } else {
        String::from(t)
    }
}

/// SMBIOS lesen: (Gehäusetyp, Akku vorhanden, Hersteller, Modell)
unsafe fn smbios(ep: u64) -> Option<(u8, bool, String, String)> {
    if ep == 0 {
        return None;
    }
    let (tab, len) = if &*(ep as *const [u8; 5]) == b"_SM3_" {
        (rd64(ep + 0x10), rd32(ep + 0x0c) as u64)
    } else if &*(ep as *const [u8; 4]) == b"_SM_" {
        (rd32(ep + 0x18) as u64, rd16(ep + 0x16) as u64)
    } else {
        return None;
    };
    if tab == 0 || len == 0 || len > 1 << 20 {
        return None;
    }
    let (mut gehaeuse, mut akku, mut hersteller, mut modell) =
        (0u8, false, String::new(), String::new());
    let mut p = tab;
    while p + 4 <= tab + len {
        let typ = rd8(p);
        let l = rd8(p + 1);
        if l < 4 {
            break;
        }
        match typ {
            1 if l >= 6 => {
                hersteller = smbios_str(p, l, rd8(p + 4));
                modell = smbios_str(p, l, rd8(p + 5));
            }
            3 if l >= 6 && gehaeuse == 0 => gehaeuse = rd8(p + 5) & 0x7f,
            22 => akku = true,
            127 => break,
            _ => {}
        }
        // hinter dem formatierten Teil: Zeichenketten bis zur doppelten Null
        let mut q = p + l as u64;
        while q + 1 < tab + len && !(rd8(q) == 0 && rd8(q + 1) == 0) {
            q += 1;
        }
        p = q + 2;
    }
    Some((gehaeuse, akku, hersteller, modell))
}

/// Beim Start einmal bestimmen (nach den Seitentabellen, die Tabellen liegen 1:1 abgebildet)
pub fn init(smbios_ep: u64, rsdp: u64) {
    let sm = unsafe { smbios(smbios_ep) };
    let pm = unsafe {
        crate::apic::find_table(rsdp, b"FACP")
            .map(|f| rd8(f + 45))
            .unwrap_or(0)
    };
    let (gehaeuse, akku, hersteller, modell) =
        sm.clone()
            .unwrap_or((0, false, String::new(), String::new()));
    let (art, quelle) = if let Some(a) = aus_gehaeuse(gehaeuse) {
        (a, "smbios")
    } else if let Some(a) = aus_pm_profil(pm) {
        // „Mobil“ ohne Akku ist meist ein Mini-PC mit Laptop-Chip
        (
            if a == Art::Laptop && sm.is_some() && !akku {
                Art::Pc
            } else {
                a
            },
            "acpi",
        )
    } else if akku {
        (Art::Laptop, "akku")
    } else {
        (Art::Pc, "standard")
    };
    crate::kprintln!(
        "[geraet] {} ({}{}{}) · Gehäuse {} · Ausgabe {} · Quelle {quelle}",
        art.anzeige_name(),
        hersteller,
        if hersteller.is_empty() || modell.is_empty() {
            ""
        } else {
            " "
        },
        modell,
        gehaeuse_text(gehaeuse),
        art.ausgabe()
    );
    *INFO.lock() = Some(Info {
        art,
        quelle,
        gehaeuse,
        pm_profil: pm,
        akku,
        hersteller,
        modell,
    });
}

pub fn art() -> Art {
    INFO.lock().as_ref().map(|i| i.art).unwrap_or(Art::Pc)
}

/// Inhalt von /proc/ultar/geraet (Schlüssel=Wert, von ultar-edition gelesen)
pub fn text() -> String {
    let g = INFO.lock();
    let Some(i) = g.as_ref() else {
        return String::from("art=pc\nausgabe=desktop\nquelle=standard\n");
    };
    format!(
        "art={}\nausgabe={}\nquelle={}\ngehaeuse={} ({})\nacpi_profil={}\nakku={}\nhersteller={}\nmodell={}\n{}",
        i.art.name(),
        i.art.ausgabe(),
        i.quelle,
        i.gehaeuse,
        gehaeuse_text(i.gehaeuse),
        i.pm_profil,
        if i.akku { "ja" } else { "nein" },
        i.hersteller,
        i.modell,
        crate::grafik::text()
    )
}
