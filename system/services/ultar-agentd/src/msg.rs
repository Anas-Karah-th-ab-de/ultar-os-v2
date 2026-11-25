//! The words the smallest help understands, in Arabic, English and German. No model is
//! involved: a request is matched against these words and nothing else.

/// Words that ask to open something, and the apps they can name: `(words, app id)`.
pub const OPEN: [&str; 7] = ["افتح", "شغّل", "شغل", "open", "start", "öffne", "starte"];

/// Apps by the words people use for them.
pub const APPS: [(&[&str], &str); 6] = [
    (&["الملفات", "ملفات", "files", "dateien"], "org.ultar.files"),
    (&["المتصفح", "متصفح", "browser"], "org.ultar.browser"),
    (
        &[
            "الإعدادات",
            "اعدادات",
            "إعدادات",
            "settings",
            "einstellungen",
        ],
        "org.ultar.settings",
    ),
    (&["الطرفية", "طرفية", "terminal"], "org.ultar.terminal"),
    (
        &["المحرر", "محرر", "editor", "texteditor"],
        "org.ultar.editor",
    ),
    (&["المتجر", "متجر", "store"], "org.ultar.store"),
];

/// Words for the sound volume (followed by a number).
pub const VOLUME: [&str; 6] = ["الصوت", "صوت", "volume", "lautstärke", "مستوى الصوت", "ton"];
/// Words that mute.
pub const MUTE: [&str; 5] = ["اكتم", "كتم", "mute", "stumm", "صامت"];
/// Words that unmute.
pub const UNMUTE: [&str; 4] = ["ألغ الكتم", "الغ الكتم", "unmute", "ton an"];
/// Words for Wi-Fi.
pub const WIFI: [&str; 6] = [
    "واي فاي",
    "الواي فاي",
    "wi-fi",
    "wifi",
    "wlan",
    "الشبكة اللاسلكية",
];
/// Words that switch something on.
pub const ON: [&str; 7] = ["شغّل", "شغل", "فعّل", "فعل", " on", "ein", " an"];
/// Words that switch something off.
pub const OFF: [&str; 6] = ["أطفئ", "اطفئ", "أوقف", "اوقف", "off", "aus"];
/// Words that ask to continue on another device.
pub const CONTINUE: [&str; 7] = [
    "أكمل",
    "اكمل",
    "تابع",
    "continue",
    "hand over",
    "fortsetzen",
    "weitergeben",
];
/// Words that ask for the paired devices.
pub const DEVICES: [&str; 5] = ["أجهزتي", "اجهزتي", "الأجهزة", "devices", "geräte"];

/// Words for Bluetooth.
pub const BLUETOOTH: [&str; 3] = ["بلوتوث", "البلوتوث", "bluetooth"];
/// Words for airplane mode.
pub const AIRPLANE: [&str; 5] = [
    "وضع الطيران",
    "الطيران",
    "airplane",
    "flight mode",
    "flugmodus",
];
/// Words that ask for a screenshot.
pub const SCREENSHOT: [&str; 5] = [
    "لقطة شاشة",
    "لقطة",
    "screenshot",
    "bildschirmfoto",
    "صوّر الشاشة",
];
/// Words that lock the screen.
pub const LOCK: [&str; 6] = ["اقفل", "أقفل", "قفل الشاشة", "lock", "sperre", "sperren"];
/// Words that make a request a question about this system.
pub const HOW: [&str; 9] = [
    "كيف",
    "ما هو",
    "ماذا",
    "how",
    "what",
    "help",
    "wie",
    "was ",
    "hilfe",
];
/// Help topics by the words people use for them: `(words, topic)`. The first match counts.
pub const HELP_TOPICS: [(&[&str], &str); 8] = [
    (
        &[
            "أقرن",
            "اقرن",
            "إقران",
            "اقتران",
            "pair",
            "koppel",
            "kopple",
        ],
        "pairing",
    ),
    (
        &[
            "أكمل",
            "اكمل",
            "أتابع",
            "متابعة",
            "continue",
            "hand over",
            "weiter",
        ],
        "continue",
    ),
    (&["ملف", "ملفات", "file", "datei"], "files"),
    (&["النموذج", "نموذج", "model", "modell"], "model"),
    (&["واي فاي", "الواي فاي", "wi-fi", "wifi", "wlan"], "wifi"),
    (&["بلوتوث", "bluetooth"], "bluetooth"),
    (&["تحديث", "التحديث", "update", "aktualis"], "updates"),
    (
        &[
            "تستطيع",
            "تفعل",
            "can you",
            "help",
            "kannst du",
            "hilfe",
            "مساعدة",
        ],
        "helper",
    ),
];
