//! Message table of this crate: Arabic first, then English and German.
//!
//! The HAL only hands out stable keys ([`crate::HalError::key`],
//! [`crate::DeviceClass::key`]). This table is the interim home of the texts until the
//! shared i18n crate takes them over; the keys stay the same.

pub use ultar_profile::msg::Lang;

/// Key followed by the texts in the order Arabic, English, German.
type Row = (&'static str, [&'static str; 3]);

const TABLE: &[Row] = &[
    (
        "hal.error.not_supported",
        [
            "هذا الجهاز لا يدعم هذه الوظيفة",
            "This device does not support this function",
            "Dieses Gerät unterstützt diese Funktion nicht",
        ],
    ),
    (
        "hal.error.not_found",
        [
            "العتاد المطلوب غير موجود",
            "The requested hardware was not found",
            "Die angeforderte Hardware wurde nicht gefunden",
        ],
    ),
    (
        "hal.error.invalid_argument",
        [
            "القيمة خارج النطاق الذي يقبله العتاد",
            "The value is outside the range the hardware accepts",
            "Der Wert liegt außerhalb des Bereichs, den die Hardware annimmt",
        ],
    ),
    (
        "hal.error.busy",
        [
            "العتاد مشغول حاليًا، حاول مرة أخرى",
            "The hardware is busy, try again",
            "Die Hardware ist beschäftigt, bitte erneut versuchen",
        ],
    ),
    (
        "hal.error.timeout",
        [
            "العتاد لم يستجب في الوقت المحدد",
            "The hardware did not answer in time",
            "Die Hardware hat nicht rechtzeitig geantwortet",
        ],
    ),
    (
        "hal.error.permission_denied",
        [
            "لا توجد صلاحية للوصول إلى هذا العتاد",
            "No permission to use this hardware",
            "Keine Berechtigung für diese Hardware",
        ],
    ),
    (
        "hal.error.disconnected",
        [
            "تم فصل العتاد أو إيقافه",
            "The hardware was removed or switched off",
            "Die Hardware wurde entfernt oder ausgeschaltet",
        ],
    ),
    (
        "hal.error.rejected",
        [
            "رفض العتاد هذا الطلب",
            "The hardware refused the request",
            "Die Hardware hat die Anfrage abgelehnt",
        ],
    ),
    (
        "hal.error.io",
        [
            "خطأ في الاتصال بالعتاد",
            "Error while talking to the hardware",
            "Fehler bei der Verbindung zur Hardware",
        ],
    ),
    ("hal.class.display", ["الشاشة", "Display", "Bildschirm"]),
    ("hal.class.renderer", ["معالج الرسوم", "Graphics", "Grafik"]),
    (
        "hal.class.input",
        ["أجهزة الإدخال", "Input devices", "Eingabegeräte"],
    ),
    ("hal.class.audio", ["الصوت", "Sound", "Ton"]),
    (
        "hal.class.power",
        ["الطاقة والبطارية", "Power and battery", "Energie und Akku"],
    ),
    ("hal.class.network", ["الشبكة", "Network", "Netzwerk"]),
    ("hal.class.wifi", ["واي فاي", "Wi-Fi", "WLAN"]),
    ("hal.class.bluetooth", ["بلوتوث", "Bluetooth", "Bluetooth"]),
    (
        "hal.class.radio",
        ["شبكة الجوّال", "Mobile network", "Mobilfunk"],
    ),
    ("hal.class.camera", ["الكاميرا", "Camera", "Kamera"]),
    ("hal.class.sensors", ["المستشعرات", "Sensors", "Sensoren"]),
    ("hal.class.gnss", ["تحديد الموقع", "Positioning", "Ortung"]),
    ("hal.class.haptics", ["الاهتزاز", "Vibration", "Vibration"]),
    (
        "hal.class.leds",
        ["أضواء التنبيه", "Indicator lights", "Anzeigeleuchten"],
    ),
    ("hal.class.storage", ["التخزين", "Storage", "Speicher"]),
    (
        "hal.class.tpm",
        ["شريحة الأمان", "Security chip", "Sicherheitschip"],
    ),
];

/// Every key of the table.
pub fn keys() -> impl Iterator<Item = &'static str> {
    TABLE.iter().map(|(key, _)| *key)
}

/// Text of a key in the given language, or `None` for an unknown key.
pub fn text(key: &str, lang: Lang) -> Option<&'static str> {
    let column = match lang {
        Lang::Ar => 0,
        Lang::En => 1,
        Lang::De => 2,
    };
    TABLE
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, texts)| texts[column])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_every_language() {
        for key in keys() {
            for lang in Lang::ALL {
                assert!(
                    text(key, lang).is_some_and(|value| !value.is_empty()),
                    "{key} {lang:?}"
                );
            }
        }
        assert_eq!(keys().count(), 25);
    }

    #[test]
    fn arabic_texts_are_arabic() {
        for key in keys() {
            let arabic = text(key, Lang::Ar).unwrap();
            assert!(
                arabic
                    .chars()
                    .any(|c| ('\u{0600}'..='\u{06ff}').contains(&c)),
                "{key}"
            );
            assert!(!arabic.chars().any(|c| c.is_ascii_alphabetic()), "{key}");
        }
    }

    #[test]
    fn keys_are_unique_and_unknown_is_none() {
        let mut all: Vec<&str> = keys().collect();
        let count = all.len();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), count);
        assert_eq!(text("hal.error.unknown", Lang::Ar), None);
    }
}
