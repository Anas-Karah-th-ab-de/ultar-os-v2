//! Texte des Browsers in Deutsch, Englisch und Arabisch (Schlüssel = deutscher Text).

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lang {
    De,
    En,
    Ar,
}

impl Lang {
    pub fn from(code: &str) -> Lang {
        match code {
            "de" => Lang::De,
            "en" => Lang::En,
            _ => Lang::Ar,
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Lang::De => "de",
            Lang::En => "en",
            Lang::Ar => "ar",
        }
    }
}

const T: &[(&str, &str, &str)] = &[
    ("aktiv", "active", "نشط"),
    ("verborgen", "hidden", "مخفي"),
    ("eingefroren", "frozen", "مجمّد"),
    ("verworfen", "discarded", "مُفرَّغ"),
    ("Standort", "location", "الموقع"),
    (
        "Kamera/Mikrofon",
        "camera/microphone",
        "الكاميرا/الميكروفون",
    ),
    ("Benachrichtigungen", "notifications", "الإشعارات"),
    ("Zwischenablage", "clipboard", "الحافظة"),
    ("Leistung", "Performance", "الأداء"),
    (
        "Schnell, privat, ohne Werbung.",
        "Fast, private, no ads.",
        "سريع، خاص، بلا إعلانات.",
    ),
    (
        "Suchen oder Adresse eingeben",
        "Search or enter address",
        "ابحث أو أدخل عنواناً",
    ),
    (
        "Werbe- und Trackingschutz",
        "Ad and tracking protection",
        "حماية من الإعلانات والتتبع",
    ),
    ("Kerne", "Cores", "الأنوية"),
    ("schnell", "fast", "سريعة"),
    ("sparsam", "efficient", "موفرة"),
    ("frei", "free", "متاح"),
    ("Bilder", "Frames", "الإطارات"),
    ("Bildzeit", "frame time", "زمن الإطار"),
    ("zuletzt", "last", "الأخير"),
    ("übersprungen", "skipped", "متخطاة"),
    ("Laufzeit", "uptime", "مدة التشغيل"),
    ("Zeichen-Threads", "Paint threads", "خيوط الرسم"),
    ("Energiesparen", "Power saving", "توفير الطاقة"),
    ("an", "on", "مفعّل"),
    ("aus", "off", "متوقف"),
    ("Prozesse", "Processes", "العمليات"),
    ("Name", "Name", "الاسم"),
    ("Seite", "Page", "الصفحة"),
    ("Zustand", "State", "الحالة"),
    (
        "Die Seite wurde beendet.",
        "The page stopped working.",
        "توقفت الصفحة عن العمل.",
    ),
    ("Neu laden", "Reload", "إعادة التحميل"),
    ("Neuer Tab", "New tab", "علامة تبويب جديدة"),
    (
        "Neuer privater Tab",
        "New private tab",
        "علامة تبويب خاصة جديدة",
    ),
    ("Desktop-Version", "Desktop site", "نسخة سطح المكتب"),
    ("App schließen", "Close app", "إغلاق التطبيق"),
    ("Browser schließen", "Close browser", "إغلاق المتصفح"),
    ("Abbrechen", "Cancel", "إلغاء"),
    ("möchte zugreifen", "wants access", "يطلب الوصول"),
    ("Ablehnen", "Deny", "رفض"),
    ("Erlauben", "Allow", "سماح"),
    ("Öffnen:", "Open:", "فتح:"),
    ("Suchen:", "Search:", "بحث:"),
    ("Fertig", "Done", "تم"),
];

pub fn t(l: Lang, key: &'static str) -> &'static str {
    match l {
        Lang::De => key,
        Lang::En => T.iter().find(|x| x.0 == key).map(|x| x.1).unwrap_or(key),
        Lang::Ar => T.iter().find(|x| x.0 == key).map(|x| x.2).unwrap_or(key),
    }
}
