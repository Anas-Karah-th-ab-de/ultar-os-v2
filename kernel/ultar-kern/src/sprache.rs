//! Systemsprache des Ultar-Kerns (Arabisch, Deutsch, Englisch). Gespeichert in /etc/ultar/sprache;
//! die Installation setzt sie, Meldungen des Kerns und der Konsole folgen ihr.

use core::sync::atomic::{AtomicU8, Ordering};

pub const DE: u8 = 0;
pub const AR: u8 = 1;
pub const EN: u8 = 2;

static AKTUELL: AtomicU8 = AtomicU8::new(DE);

/// Sprache stand schon beim Start auf der Startpartition (\ultar\sprache) = installiertes System:
/// ruhiger Start mit Startbild, danach die Startseite statt der Konsole.
pub static FEST: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn get() -> u8 {
    AKTUELL.load(Ordering::Relaxed)
}

pub fn set(l: u8) {
    AKTUELL.store(l.min(2), Ordering::Relaxed);
}

pub fn code() -> &'static str {
    match get() {
        AR => "ar",
        EN => "en",
        _ => "de",
    }
}

pub fn aus_code(c: &str) -> Option<u8> {
    match c.trim() {
        "ar" => Some(AR),
        "de" => Some(DE),
        "en" => Some(EN),
        _ => None,
    }
}

/// beim Start: /etc/ultar/sprache lesen
pub fn laden() {
    if let Some(d) = crate::fs::read_all("/etc/ultar/sprache") {
        if let Some(l) = aus_code(&alloc::string::String::from_utf8_lossy(&d)) {
            set(l);
        }
    }
}

/// Sprache dauerhaft speichern (ext4 der Ultar-Partition)
pub fn speichern() {
    crate::fs::write_file(
        "/etc/ultar/sprache",
        alloc::format!("{}\n", code()).into_bytes(),
    );
    // auch auf der Startpartition (gilt dann schon ab dem ersten Bild; bei der Installation auf der Nutzlast)
    crate::fs::fat_overwrite_alle("ultar/sprache", alloc::format!("{}\n", code()).as_bytes());
}

/// A text of the brand source (`ultar-brand`, `brand.*`) in the system language.
pub fn marke(key: &str) -> &'static str {
    use ultar_brand::text::Lang;
    let lang = match get() {
        AR => Lang::Ar,
        EN => Lang::En,
        _ => Lang::De,
    };
    ultar_brand::text::lookup(lang, key).unwrap_or("")
}

/// Meldungen des Kerns
pub fn t(k: &str) -> &'static str {
    match (k, get()) {
        ("ausnahme", AR) => "*** خطأ في النواة",
        ("ausnahme", EN) => "*** KERNEL EXCEPTION",
        ("ausnahme", _) => "*** AUSNAHME im Kern",
        ("bei", AR) => "عند",
        ("bei", EN) => "at",
        ("bei", _) => "bei",
        // one text for every stop of the kernel, with the contact address (ultar-brand)
        ("haelt", _) => marke("brand.crash.kernel"),
        ("panik", AR) => "*** عطل في النواة",
        ("panik", EN) => "*** KERNEL PANIC",
        ("panik", _) => "*** KERN-PANIK",
        ("bereit", AR) => "[kern] جاهز بعد {} ms – اكتب «مساعدة» للأوامر",
        ("bereit", EN) => "[kern] ready after {} ms – type `help` for commands",
        ("bereit", _) => "[kern] bereit nach {} ms – `hilfe` für Befehle",
        ("unbekannt", AR) => "أمر غير معروف",
        ("unbekannt", EN) => "unknown command",
        ("unbekannt", _) => "unbekannt",
        ("hilfe", AR) => "مساعدة",
        ("hilfe", EN) => "help",
        ("hilfe", _) => "hilfe",
        ("kein_verz", AR) => "ليس مجلداً",
        ("kein_verz", EN) => "not a directory",
        ("kein_verz", _) => "kein Verzeichnis",
        ("nicht_gefunden", AR) => "غير موجود",
        ("nicht_gefunden", EN) => "not found",
        ("nicht_gefunden", _) => "nicht gefunden",
        ("neustart", AR) => "إعادة التشغيل …",
        ("neustart", EN) => "Restarting …",
        ("neustart", _) => "Neustart …",
        ("aus", AR) => "إيقاف التشغيل …",
        ("aus", EN) => "Shutting down …",
        ("aus", _) => "Ausschalten …",
        ("von_prozess", AR) => "بطلب من العملية",
        ("von_prozess", EN) => "requested by process",
        ("von_prozess", _) => "angefordert von Prozess",
        ("aus_von_hand", AR) => "تعذّر إيقاف التشغيل تلقائيًا – يمكن فصل الجهاز الآن.",
        ("aus_von_hand", EN) => "The machine could not switch itself off – it is safe to turn it off now.",
        ("aus_von_hand", _) => "Die Maschine konnte sich nicht selbst ausschalten – sie kann jetzt ausgeschaltet werden.",
        ("stapel", AR) => "المكدس",
        ("stapel", EN) => "stack",
        ("stapel", _) => "Stapel",
        ("code", AR) => "الرمز",
        ("code", EN) => "code",
        ("code", _) => "Code",
        ("adresse", AR) => "العنوان",
        ("adresse", EN) => "address",
        ("adresse", _) => "Adresse",
        ("beendet", AR) => "أُنهيت",
        ("beendet", EN) => "terminated",
        ("beendet", _) => "beendet",
        _ => "",
    }
}

/// Name einer Prozessor-Ausnahme (Vektor 0–19) in der Systemsprache – eine Sprache je Zeile
pub fn ausnahme_name(v: u64) -> &'static str {
    const DE_N: [&str; 20] = [
        "Division durch null",
        "Einzelschritt",
        "NMI",
        "Haltepunkt",
        "Überlauf",
        "Bereichsgrenze",
        "ungültiger Befehl",
        "Gleitkomma nicht verfügbar",
        "Doppelfehler",
        "?",
        "ungültiges TSS",
        "Segment fehlt",
        "Stapelfehler",
        "allgemeine Schutzverletzung",
        "Seitenfehler",
        "?",
        "Gleitkommafehler",
        "Ausrichtungsfehler",
        "Maschinenprüfung",
        "SIMD-Gleitkommafehler",
    ];
    const EN_N: [&str; 20] = [
        "divide error",
        "debug",
        "NMI",
        "breakpoint",
        "overflow",
        "bound range exceeded",
        "invalid opcode",
        "device not available",
        "double fault",
        "?",
        "invalid TSS",
        "segment not present",
        "stack fault",
        "general protection fault",
        "page fault",
        "?",
        "x87 floating-point error",
        "alignment check",
        "machine check",
        "SIMD floating-point error",
    ];
    const AR_N: [&str; 20] = [
        "قسمة على صفر",
        "خطوة منفردة",
        "مقاطعة غير قابلة للحجب",
        "نقطة توقف",
        "فيض",
        "تجاوز الحدود",
        "تعليمة غير صالحة",
        "الفاصلة العائمة غير متاحة",
        "خطأ مزدوج",
        "?",
        "TSS غير صالح",
        "مقطع غير موجود",
        "خطأ في المكدس",
        "خرق حماية عام",
        "خطأ صفحة",
        "?",
        "خطأ في الفاصلة العائمة",
        "خطأ محاذاة",
        "فحص الجهاز",
        "خطأ فاصلة عائمة SIMD",
    ];
    let (n, sonst) = match get() {
        AR => (&AR_N, "مقاطعة"),
        EN => (&EN_N, "interrupt"),
        _ => (&DE_N, "Unterbrechung"),
    };
    n.get(v as usize).copied().unwrap_or(sonst)
}

/// Fehlerbild einer Ausnahme im Kern (eine Zeile, eine Sprache)
pub fn ausnahme_zeile(v: u64, rip: u64, rsp: u64, code: Option<u64>, cr2: u64) {
    match code {
        Some(c) => crate::kprintln!(
            "\n{}: {} {} {:#x} ({} {:#x}, {} {:#x}, CR2 {:#x})",
            t("ausnahme"),
            ausnahme_name(v),
            t("bei"),
            rip,
            t("stapel"),
            rsp,
            t("code"),
            c,
            cr2
        ),
        None => crate::kprintln!(
            "\n{}: {} {} {:#x} ({} {:#x}, CR2 {:#x})",
            t("ausnahme"),
            ausnahme_name(v),
            t("bei"),
            rip,
            t("stapel"),
            rsp,
            cr2
        ),
    }
    crate::kprintln!("{}", t("haelt"));
}
