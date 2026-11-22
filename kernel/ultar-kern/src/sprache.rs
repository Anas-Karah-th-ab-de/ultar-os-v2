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
    crate::fs::write_file("/etc/ultar/sprache", alloc::format!("{}\n", code()).into_bytes());
    // auch auf der Startpartition (gilt dann schon ab dem ersten Bild; bei der Installation auf der Nutzlast)
    crate::fs::fat_overwrite_alle("ultar/sprache", alloc::format!("{}\n", code()).as_bytes());
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
        ("haelt", AR) => "*** توقفت نواة Ultar. أرسل صورة الشاشة إلى KM Code Craft.",
        ("haelt", EN) => "*** The Ultar kernel has stopped. Please send a screenshot/log to KM Code Craft.",
        ("haelt", _) => "*** Der Ultar-Kern hält an. Bitte Bildschirmfoto/Protokoll an KM Code Craft.",
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
        _ => "",
    }
}
