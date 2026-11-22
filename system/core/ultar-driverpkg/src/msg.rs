//! Message tables for the `udrv` tool: Arabic first, then English and German.
//!
//! Logic never contains user-visible text; it returns keys (see [`crate::error::Coded`])
//! that are looked up here. When the system i18n crate is available these tables move there.

mod errors;

/// A user interface language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// Arabic – the first language of the system.
    Ar,
    /// English.
    En,
    /// German.
    De,
}

impl Lang {
    /// All languages in table order.
    pub const ALL: [Self; 3] = [Self::Ar, Self::En, Self::De];

    /// Interprets a language tag such as `ar`, `en_US` or `de-DE`; anything else is Arabic.
    pub fn from_tag(tag: Option<&str>) -> Self {
        let tag = tag.unwrap_or_default().trim().to_ascii_lowercase();
        if tag.starts_with("en") {
            Self::En
        } else if tag.starts_with("de") {
            Self::De
        } else {
            Self::Ar
        }
    }

    /// Reads the language from the environment variable `ULTAR_LANG`.
    pub fn from_env() -> Self {
        Self::from_tag(std::env::var("ULTAR_LANG").ok().as_deref())
    }

    fn column(self) -> usize {
        match self {
            Self::Ar => 0,
            Self::En => 1,
            Self::De => 2,
        }
    }
}

/// One table row: key and the texts in [`Lang::ALL`] order.
type Row = (&'static str, [&'static str; 3]);

const USAGE_AR: &str = "الاستخدام:
  udrv keygen
  udrv pack <manifest> <payload-dir> <out.utr> <privkey>
  udrv verify <file.utr> <trusted-keys-file>
  udrv info <file.utr>
  udrv index <dir> <privkey> [--trusted <trusted-keys-file>]
  udrv match <index.toml> <hardware-list> [--abi <kernel-abi>] [--trusted <trusted-keys-file>]";

const USAGE_EN: &str = "Usage:
  udrv keygen
  udrv pack <manifest> <payload-dir> <out.utr> <privkey>
  udrv verify <file.utr> <trusted-keys-file>
  udrv info <file.utr>
  udrv index <dir> <privkey> [--trusted <trusted-keys-file>]
  udrv match <index.toml> <hardware-list> [--abi <kernel-abi>] [--trusted <trusted-keys-file>]";

const USAGE_DE: &str = "Aufruf:
  udrv keygen
  udrv pack <manifest> <payload-dir> <out.utr> <privkey>
  udrv verify <file.utr> <trusted-keys-file>
  udrv info <file.utr>
  udrv index <dir> <privkey> [--trusted <trusted-keys-file>]
  udrv match <index.toml> <hardware-list> [--abi <kernel-abi>] [--trusted <trusted-keys-file>]";

const CLI: &[Row] = &[
    ("driverpkg.cli.usage", [USAGE_AR, USAGE_EN, USAGE_DE]),
    (
        "driverpkg.cli.unknown-command",
        ["أمر غير معروف", "Unknown command", "Unbekannter Befehl"],
    ),
    (
        "driverpkg.cli.io",
        [
            "تعذّر الوصول إلى الملف",
            "Cannot access the file",
            "Datei nicht zugänglich",
        ],
    ),
    (
        "driverpkg.cli.output",
        [
            "تعذّرت كتابة النتيجة",
            "Cannot write the output",
            "Ausgabe nicht schreibbar",
        ],
    ),
    (
        "driverpkg.cli.keygen.private",
        [
            "المفتاح الخاص (سرّي، يُحفظ دون اتصال)",
            "Private key (secret, keep offline)",
            "Privater Schlüssel (geheim, offline halten)",
        ],
    ),
    (
        "driverpkg.cli.keygen.public",
        ["المفتاح العام", "Public key", "Öffentlicher Schlüssel"],
    ),
    (
        "driverpkg.cli.key-id",
        ["معرّف المفتاح", "Key id", "Schlüsselkennung"],
    ),
    (
        "driverpkg.cli.pack.done",
        [
            "تم إنشاء الحزمة وتوقيعها",
            "Package created and signed",
            "Paket erstellt und signiert",
        ],
    ),
    (
        "driverpkg.cli.verify.ok",
        [
            "التوقيع صحيح والحزمة موثوقة",
            "Signature valid, package trusted",
            "Signatur gültig, Paket vertrauenswürdig",
        ],
    ),
    (
        "driverpkg.cli.index.done",
        [
            "تم إنشاء الفهرس وتوقيعه، عدد الحزم",
            "Index created and signed, packages",
            "Index erstellt und signiert, Pakete",
        ],
    ),
    (
        "driverpkg.cli.info.not-verified",
        [
            "للعرض فقط – لم يُتحقَّق من التوقيع",
            "Display only – signature not verified",
            "Nur Anzeige – Signatur nicht geprüft",
        ],
    ),
    ("driverpkg.cli.info.id", ["المعرّف", "Id", "Kennung"]),
    (
        "driverpkg.cli.info.version",
        ["الإصدار", "Version", "Version"],
    ),
    ("driverpkg.cli.info.class", ["الفئة", "Class", "Klasse"]),
    (
        "driverpkg.cli.info.origin",
        ["المصدر", "Origin", "Herkunft"],
    ),
    ("driverpkg.cli.info.rank", ["الأولوية", "Rank", "Rang"]),
    (
        "driverpkg.cli.info.kernel-abi",
        ["واجهة النواة", "Kernel ABI", "Kern-ABI"],
    ),
    (
        "driverpkg.cli.info.name-key",
        ["مفتاح الاسم", "Name key", "Namensschlüssel"],
    ),
    (
        "driverpkg.cli.info.unsigned",
        [
            "الحزمة غير موقّعة",
            "Package is unsigned",
            "Paket ist unsigniert",
        ],
    ),
    (
        "driverpkg.cli.info.payload-sha256",
        [
            "بصمة المحتوى (SHA-256)",
            "Payload SHA-256",
            "Inhalt SHA-256",
        ],
    ),
    ("driverpkg.cli.info.file", ["ملف", "File", "Datei"]),
    (
        "driverpkg.cli.match.driver",
        ["  التعريف", "  Driver", "  Treiber"],
    ),
    (
        "driverpkg.cli.match.alternatives",
        ["  البدائل", "  Alternatives", "  Alternativen"],
    ),
    (
        "driverpkg.cli.match.no-driver",
        [
            "  لا يوجد تعريف – يُطلب مخطط من الورشة",
            "  No driver – workshop blueprint requested",
            "  Kein Treiber – Bauplan für die Werkstatt anlegen",
        ],
    ),
    (
        "driverpkg.cli.match.with-driver",
        [
            "أجهزة لها تعريف",
            "Devices with driver",
            "Geräte mit Treiber",
        ],
    ),
    (
        "driverpkg.cli.match.without-driver",
        [
            "أجهزة بلا تعريف",
            "Devices without driver",
            "Geräte ohne Treiber",
        ],
    ),
    (
        "driverpkg.cli.match.index-not-verified",
        [
            "تنبيه: لم يُتحقَّق من توقيع الفهرس (استخدم ‎--trusted)",
            "Warning: index signature not verified (use --trusted)",
            "Achtung: Index-Signatur nicht geprüft (--trusted verwenden)",
        ],
    ),
];

/// Looks up the text for a message key.
pub fn text(lang: Lang, key: &str) -> Option<&'static str> {
    CLI.iter()
        .chain(errors::ERRORS)
        .find(|(k, _)| *k == key)
        .map(|(_, texts)| texts[lang.column()])
}

/// All known message keys (for completeness checks).
pub fn keys() -> impl Iterator<Item = &'static str> {
    CLI.iter().chain(errors::ERRORS).map(|(k, _)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_tags() {
        assert_eq!(Lang::from_tag(None), Lang::Ar);
        assert_eq!(Lang::from_tag(Some("")), Lang::Ar);
        assert_eq!(Lang::from_tag(Some("fr")), Lang::Ar);
        assert_eq!(Lang::from_tag(Some("ar_SY")), Lang::Ar);
        assert_eq!(Lang::from_tag(Some("EN")), Lang::En);
        assert_eq!(Lang::from_tag(Some("en_US.UTF-8")), Lang::En);
        assert_eq!(Lang::from_tag(Some(" de-DE ")), Lang::De);
    }

    #[test]
    fn lookup() {
        assert_eq!(text(Lang::En, "driverpkg.cli.info.id"), Some("Id"));
        assert_eq!(text(Lang::De, "driverpkg.cli.info.id"), Some("Kennung"));
        assert_eq!(text(Lang::Ar, "driverpkg.cli.info.id"), Some("المعرّف"));
        assert_eq!(
            text(Lang::En, "driverpkg.package.unsigned"),
            Some("Package is not signed – unsigned drivers are never loaded")
        );
        assert_eq!(text(Lang::En, "no.such.key"), None);
    }

    #[test]
    fn tables_are_complete_and_unique() {
        let all: Vec<&str> = keys().collect();
        let mut unique = all.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(all.len(), unique.len(), "duplicate message key");
        for key in all {
            for lang in Lang::ALL {
                assert!(
                    !text(lang, key).unwrap().trim().is_empty(),
                    "{key} {lang:?}"
                );
            }
            let arabic = text(Lang::Ar, key).unwrap();
            assert!(
                arabic
                    .chars()
                    .any(|c| ('\u{0600}'..='\u{06ff}').contains(&c)),
                "{key} has no Arabic text"
            );
        }
    }
}
