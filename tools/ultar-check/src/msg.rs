//! Message tables for all tool output (Arabic first, then English, German).
//!
//! Logic never contains user-visible text: it produces a [`Key`] plus
//! positional arguments, and [`render`] turns that into text.

/// Output language of the tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// Arabic (first language of the system).
    Ar,
    /// English.
    En,
    /// German.
    De,
}

impl Lang {
    /// Parses a language code such as `ar`, `en_US` or `de-DE`.
    pub fn from_code(code: &str) -> Option<Lang> {
        let short: String = code.chars().take(2).collect::<String>().to_lowercase();
        match short.as_str() {
            "ar" => Some(Lang::Ar),
            "en" => Some(Lang::En),
            "de" => Some(Lang::De),
            _ => None,
        }
    }

    fn index(self) -> usize {
        match self {
            Lang::Ar => 0,
            Lang::En => 1,
            Lang::De => 2,
        }
    }
}

macro_rules! messages {
    ($( $name:ident => [$ar:literal, $en:literal, $de:literal $(,)?], )*) => {
        /// Identifier of one message; the English text is the variant documentation.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Key {
            $(
                #[doc = "```text"]
                #[doc = $en]
                #[doc = "```"]
                $name,
            )*
        }

        impl Key {
            /// Every message key, in table order.
            pub const ALL: &'static [Key] = &[$(Key::$name),*];

            /// Stable identifier used in JSON output.
            pub fn id(self) -> &'static str {
                match self { $(Key::$name => stringify!($name),)* }
            }

            fn texts(self) -> [&'static str; 3] {
                match self { $(Key::$name => [$ar, $en, $de],)* }
            }
        }
    };
}

messages! {
    LayerNotInArea => [
        "الحزمة «{0}» ليست داخل أي مجلد منطقة",
        "crate `{0}` is not inside any area folder",
        "Crate `{0}` liegt in keinem Bereichsordner",
    ],
    LayerForbidden => [
        "تبعية ممنوعة «{0}» ← «{1}» (المنطقة {2})",
        "forbidden dependency `{0}` -> `{1}` (area {2})",
        "verbotene Abhängigkeit `{0}` -> `{1}` (Bereich {2})",
    ],
    LayerUpward => [
        "تبعية نحو طبقة أعلى «{0}» ← «{1}» (المنطقة {2})",
        "dependency on a higher layer `{0}` -> `{1}` (area {2})",
        "Abhängigkeit auf höhere Schicht `{0}` -> `{1}` (Bereich {2})",
    ],
    LayerOutsideRepo => [
        "التبعية «{0}» تشير إلى خارج المستودع: {1}",
        "dependency `{0}` points outside the repository: {1}",
        "Abhängigkeit `{0}` zeigt aus dem Repository heraus: {1}",
    ],
    HardwarePath => [
        "مسار عتاد خارج طبقة العتاد: \"{0}\"",
        "hardware path outside the HAL: \"{0}\"",
        "Hardwarepfad außerhalb der HAL: \"{0}\"",
    ],
    EditionQuery => [
        "استعلام عن الإصدار؛ يجب أن يعتمد السلوك على القدرات",
        "edition query; behaviour must depend on capabilities",
        "Ausgabe-Abfrage; Verhalten muss an Fähigkeiten hängen",
    ],
    HardcodedText => [
        "نص ثابت في الشيفرة: \"{0}\" (استخدم مفتاح ترجمة)",
        "hardcoded text: \"{0}\" (use an i18n key)",
        "fest eingebauter Text: \"{0}\" (i18n-Schlüssel verwenden)",
    ],
    BrandLiteral => [
        "اسم من الهوية مكتوب في الشيفرة: {0} في \"{1}\" (استخدم ultar-brand)",
        "brand term written in code: {0} in \"{1}\" (use ultar-brand)",
        "Markenangabe im Code geschrieben: {0} in \"{1}\" (ultar-brand verwenden)",
    ],
    BrandSpelling => [
        "المفتاح `{0}` يكتب «{1}» بدل «{2}»",
        "key `{0}` spells \"{1}\" instead of \"{2}\"",
        "Schlüssel `{0}` schreibt „{1}“ statt „{2}“",
    ],
    BrandValue => [
        "المفتاح `{0}` يخالف مصدر الهوية؛ المطلوب: {1}",
        "key `{0}` differs from the brand source; expected: {1}",
        "Schlüssel `{0}` weicht von der Markenquelle ab; erwartet: {1}",
    ],
    BrandAddress => [
        "العنوان {0} ليس عنوان التواصل المعلن؛ المطلوب: {1}",
        "address {0} is not the public contact address; expected: {1}",
        "Adresse {0} ist nicht die öffentliche Kontaktadresse; erwartet: {1}",
    ],
    BrandFactInCatalog => [
        "المفتاح `{0}` لا يُترجم: مصدره ultar-brand وليس ملف ترجمة",
        "key `{0}` is not translated: it comes from ultar-brand, not from a catalog",
        "Schlüssel `{0}` wird nicht übersetzt: er kommt aus ultar-brand, nicht aus einem Katalog",
    ],
    BrandKeyOutside => [
        "مفتاح الهوية `{0}` خارج system/core/ultar-brand/locales",
        "brand key `{0}` outside system/core/ultar-brand/locales",
        "Markenschlüssel `{0}` außerhalb von system/core/ultar-brand/locales",
    ],
    BrandKeyMissing => [
        "مفتاح الهوية `{0}` ناقص في `{1}` (موجود في `{2}`)",
        "brand key `{0}` is missing in `{1}` (present in `{2}`)",
        "Markenschlüssel `{0}` fehlt in `{1}` (vorhanden in `{2}`)",
    ],
    BrandCatalogMissing => [
        "ملف نصوص الهوية للغة `{0}` غير موجود",
        "the brand catalog of language `{0}` does not exist",
        "der Markenkatalog der Sprache `{0}` fehlt",
    ],
    BrandNotArabic => [
        "نص الهوية `{0}` في العربية بلا حروف عربية",
        "Arabic brand text `{0}` has no Arabic letters",
        "arabischer Markentext `{0}` enthält keine arabischen Buchstaben",
    ],
    BrandOsReleaseMissing => [
        "ملف os-release غير موجود (المصدر: ultar_brand::release::OS_RELEASE)",
        "os-release does not exist (source: ultar_brand::release::OS_RELEASE)",
        "os-release fehlt (Quelle: ultar_brand::release::OS_RELEASE)",
    ],
    BrandOsRelease => [
        "os-release يخالف مصدر الهوية؛ المطلوب هنا: {0}",
        "os-release differs from the brand source; expected here: {0}",
        "os-release weicht von der Markenquelle ab; hier erwartet: {0}",
    ],
    BrandAuthors => [
        "حقل authors يخالف مصدر الهوية؛ المطلوب: {0}",
        "authors differs from the brand source; expected: {0}",
        "authors weicht von der Markenquelle ab; erwartet: {0}",
    ],
    BrandMark => [
        "علامة النظام في مجموعة الرموز تخالف المصدر؛ المسار المطلوب: {0} (بلا انعكاس)",
        "the brand mark of the icon set differs from the source; expected path: {0} (never mirrored)",
        "das Markenzeichen im Symbolsatz weicht von der Quelle ab; erwarteter Pfad: {0} (nie gespiegelt)",
    ],
    FileTooLong => [
        "الملف طويل جدًا: {0} سطرًا (الحد {1})",
        "file too long: {0} lines (max {1})",
        "Datei zu lang: {0} Zeilen (max. {1})",
    ],
    FileLong => [
        "الملف يكبر: {0} سطرًا (تحذير من {1})",
        "file is getting long: {0} lines (warn above {1})",
        "Datei wird lang: {0} Zeilen (Warnung ab {1})",
    ],
    FnTooLong => [
        "الدالة «{0}» طويلة جدًا: {1} سطرًا (الحد {2})",
        "function `{0}` too long: {1} lines (max {2})",
        "Funktion `{0}` zu lang: {1} Zeilen (max. {2})",
    ],
    StructTooLarge => [
        "البنية «{0}» فيها {1} حقلًا (الحد {2})",
        "struct `{0}` has {1} fields (max {2})",
        "Struct `{0}` hat {1} Felder (max. {2})",
    ],
    UnsafeArea => [
        "unsafe غير مسموح في هذه المنطقة",
        "`unsafe` is not allowed in this area",
        "`unsafe` ist in diesem Bereich nicht erlaubt",
    ],
    UnsafeNoComment => [
        "unsafe بلا تعليق // SAFETY: في الأسطر الثلاثة السابقة",
        "`unsafe` without a `// SAFETY:` comment in the 3 lines before",
        "`unsafe` ohne `// SAFETY:`-Kommentar in den 3 Zeilen davor",
    ],
    PanicCall => [
        "«{0}» خارج شيفرة الاختبار",
        "`{0}` outside test code",
        "`{0}` außerhalb von Testcode",
    ],
    PhysicalSide => [
        "المعرّف «{0}» يستخدم left/right؛ استخدم start/end أو علّم السطر بـ // physical",
        "identifier `{0}` uses left/right; use start/end or mark the line `// physical`",
        "Bezeichner `{0}` nutzt left/right; start/end verwenden oder Zeile mit `// physical` markieren",
    ],
    LinuxCall => [
        "استدعاء برنامج خارجي «{0}»",
        "call of external program `{0}`",
        "Aufruf des externen Programms `{0}`",
    ],
    BudgetExceeded => [
        "الملف {0} حجمه {1} بايت (الميزانية {2})",
        "binary {0} is {1} bytes (budget {2})",
        "Binary {0} hat {1} Bytes (Budget {2})",
    ],
    BudgetNotBuilt => [
        "الميزانية: «{0}» غير مبني، تم التخطي",
        "budget: `{0}` is not built, skipped",
        "Budget: `{0}` ist nicht gebaut, übersprungen",
    ],
    BudgetNoConfig => [
        "الميزانية: لا يوجد {0}، تم التخطي",
        "budget: {0} not found, skipped",
        "Budget: {0} fehlt, übersprungen",
    ],
    BaselineExceeded => [
        "{0}: {1}: {2} مخالفة، المسموح في خط الأساس {3}",
        "{0}: {1}: {2} violations, baseline allows {3}",
        "{0}: {1}: {2} Verstöße, Baseline erlaubt {3}",
    ],
    BaselineCanLower => [
        "{0}: {1}: يمكن خفض خط الأساس من {2} إلى {3}",
        "{0}: {1}: baseline can be lowered from {2} to {3}",
        "{0}: {1}: Baseline kann von {2} auf {3} gesenkt werden",
    ],
    BaselineCanRemove => [
        "{0}: {1}: لا مخالفات بعد الآن، يمكن حذف المدخل من خط الأساس",
        "{0}: {1}: no violations left, the baseline entry can be removed",
        "{0}: {1}: keine Verstöße mehr, Baseline-Eintrag kann entfernt werden",
    ],
    LabelWarning => ["تحذير", "warning", "Warnung",],
    LabelNote => ["ملاحظة", "note", "Hinweis",],
    SummaryOk => [
        "ultar-check: ناجح ({0} ضمن خط الأساس، {1} تحذير)",
        "ultar-check: ok ({0} baselined, {1} warnings)",
        "ultar-check: in Ordnung ({0} in Baseline, {1} Warnungen)",
    ],
    SummaryFail => [
        "ultar-check: فشل – {0} مخالفة ({1} ضمن خط الأساس، {2} تحذير)",
        "ultar-check: FAILED - {0} violations ({1} baselined, {2} warnings)",
        "ultar-check: FEHLGESCHLAGEN - {0} Verstöße ({1} in Baseline, {2} Warnungen)",
    ],
    ErrIo => [
        "تعذر الوصول إلى {0}: {1}",
        "cannot access {0}: {1}",
        "Zugriff auf {0} nicht möglich: {1}",
    ],
    ErrToml => [
        "ملف TOML غير صالح {0}: {1}",
        "invalid TOML in {0}: {1}",
        "ungültiges TOML in {0}: {1}",
    ],
    ErrUnknownRule => [
        "{0}: قاعدة غير معروفة «{1}»",
        "{0}: unknown rule `{1}`",
        "{0}: unbekannte Regel `{1}`",
    ],
    ErrDuplicateEntry => [
        "{0}: مدخل مكرر {1} / {2}",
        "{0}: duplicate entry {1} / {2}",
        "{0}: doppelter Eintrag {1} / {2}",
    ],
    ErrUnknownArea => [
        "{0}: منطقة غير معروفة «{1}»",
        "{0}: unknown area `{1}`",
        "{0}: unbekannter Bereich `{1}`",
    ],
    ErrDuplicateArea => [
        "{0}: منطقة مكررة «{1}»",
        "{0}: duplicate area `{1}`",
        "{0}: doppelter Bereich `{1}`",
    ],
    ErrMissingCommand => [
        "لم يُحدَّد أمر (جرّب --help)",
        "no command given (try --help)",
        "kein Befehl angegeben (siehe --help)",
    ],
    ErrUnknownCommand => [
        "أمر غير معروف «{0}»",
        "unknown command `{0}`",
        "unbekannter Befehl `{0}`",
    ],
    ErrUnknownOption => [
        "خيار غير معروف «{0}»",
        "unknown option `{0}`",
        "unbekannte Option `{0}`",
    ],
    ErrMissingValue => [
        "الخيار «{0}» يحتاج إلى قيمة",
        "option `{0}` needs a value",
        "Option `{0}` braucht einen Wert",
    ],
    ErrInvalidValue => [
        "قيمة غير صالحة «{1}» للخيار «{0}»",
        "invalid value `{1}` for option `{0}`",
        "ungültiger Wert `{1}` für Option `{0}`",
    ],
    Help => [
        "الاستخدام: ultar-check [خيارات] <أمر>\n\nالأوامر:\n  all            تشغيل كل القواعد\n  layers paths edition text size unsafe unwrap logical-layout linux-calls budget brand\n  baseline       طباعة خط الأساس للحالة الحالية (TOML)\n\nالخيارات:\n  --root <مجلد>        جذر المستودع (الافتراضي: المجلد الحالي)\n  --format text|json   صيغة الإخراج\n  --baseline <ملف>     ملف خط الأساس (الافتراضي: tools/check-baseline.toml)\n  --no-baseline        تجاهل خط الأساس\n  --target-dir <مجلد>  مجلد البناء لفحص الميزانية (الافتراضي: target)\n  --lang ar|en|de      لغة الإخراج (أو المتغير ULTAR_LANG)\n  --help               هذه المساعدة",
        "usage: ultar-check [options] <command>\n\ncommands:\n  all            run every rule\n  layers paths edition text size unsafe unwrap logical-layout linux-calls budget brand\n  baseline       print a baseline for the current state (TOML)\n\noptions:\n  --root <dir>         repository root (default: current directory)\n  --format text|json   output format\n  --baseline <file>    baseline file (default: tools/check-baseline.toml)\n  --no-baseline        ignore the baseline\n  --target-dir <dir>   build directory for the budget check (default: target)\n  --lang ar|en|de      output language (or ULTAR_LANG)\n  --help               this help",
        "Aufruf: ultar-check [Optionen] <Befehl>\n\nBefehle:\n  all            alle Regeln\n  layers paths edition text size unsafe unwrap logical-layout linux-calls budget brand\n  baseline       Baseline des aktuellen Stands ausgeben (TOML)\n\nOptionen:\n  --root <Ordner>      Repository-Wurzel (Standard: aktueller Ordner)\n  --format text|json   Ausgabeformat\n  --baseline <Datei>   Baseline-Datei (Standard: tools/check-baseline.toml)\n  --no-baseline        Baseline ignorieren\n  --target-dir <Ordner> Bauordner für die Budgetprüfung (Standard: target)\n  --lang ar|en|de      Ausgabesprache (oder ULTAR_LANG)\n  --help               diese Hilfe",
    ],
}

/// Renders `key` in `lang`, replacing `{0}`, `{1}` ... with `args`.
pub fn render(lang: Lang, key: Key, args: &[String]) -> String {
    let template = key.texts()[lang.index()];
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let slot = after
            .find('}')
            .and_then(|close| after[..close].parse::<usize>().ok().map(|n| (n, close)));
        match slot {
            Some((index, close)) => {
                if let Some(arg) = args.get(index) {
                    out.push_str(arg);
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_codes_are_parsed_by_prefix() {
        assert_eq!(Lang::from_code("ar"), Some(Lang::Ar));
        assert_eq!(Lang::from_code("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::from_code("DE-de"), Some(Lang::De));
        assert_eq!(Lang::from_code("fr"), None);
        assert_eq!(Lang::from_code(""), None);
    }

    #[test]
    fn render_replaces_positional_arguments() {
        let args = vec!["700".to_string(), "600".to_string()];
        assert_eq!(
            render(Lang::En, Key::FileTooLong, &args),
            "file too long: 700 lines (max 600)"
        );
        assert!(render(Lang::De, Key::FileTooLong, &args).contains("700 Zeilen"));
        assert!(render(Lang::Ar, Key::FileTooLong, &args).contains("700"));
    }

    #[test]
    fn render_tolerates_missing_arguments() {
        assert_eq!(
            render(Lang::En, Key::FileTooLong, &[]),
            "file too long:  lines (max )"
        );
    }

    #[test]
    fn every_key_has_all_languages_with_the_same_slots() {
        for key in Key::ALL {
            let texts = key.texts();
            for text in texts {
                assert!(!text.is_empty(), "{key:?}");
            }
            for slot in ["{0}", "{1}", "{2}", "{3}"] {
                let en = texts[1].contains(slot);
                assert_eq!(texts[0].contains(slot), en, "{key:?} ar {slot}");
                assert_eq!(texts[2].contains(slot), en, "{key:?} de {slot}");
            }
        }
    }

    #[test]
    fn key_ids_are_unique() {
        let mut ids: Vec<&str> = Key::ALL.iter().map(|k| k.id()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len());
    }
}
