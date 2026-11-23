//! Operator messages of `ultar-logd` and `ulog`: keys in the logic, text
//! in the tables, Arabic first. (Log *records* need no translation here:
//! they are message keys that the reading app translates.)

pub use ultar_bus::msg::Lang;

/// Everything the two programs can say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Msg {
    /// How to call a program (the synopsis follows).
    Usage,
    /// The command line was not understood.
    BadArguments,
    /// Neither a policy file nor `--allow-all`.
    PolicyRequired,
    /// `--policy` together with `--allow-all`.
    PolicyConflict,
    /// The policy file was rejected; details follow.
    PolicyRejected,
    /// The uid map was rejected; details follow.
    UidMapRejected,
    /// The kernel reports no peer credentials.
    CredentialsUnsupported,
    /// A secret file (chain key, token) was rejected; details follow.
    SecretRejected,
    /// The limits given do not fit together.
    LimitsInvalid,
    /// Refusing to run as root.
    RootRefused,
    /// Self-limits could not be applied; details follow.
    HardeningFailed,
    /// A socket could not be opened; details follow.
    SocketFailed,
    /// The log directory cannot be used; the service keeps records in
    /// memory only.
    DirectoryUnusable,
    /// The service is ready.
    Ready,
    /// The service stopped.
    Stopped,
    /// The log service could not be reached; details follow.
    BusFailed,
    /// The caller may not do this.
    NotPermitted,
    /// The log directory could not be read; details follow.
    DirectoryUnreadable,
    /// The log is intact.
    VerifyIntact,
    /// The log is not intact.
    VerifyBroken,
    /// Verified without a key: what that proves.
    VerifyUnkeyedNote,
    /// Positions that were never written to disk.
    VerifyGap,
    /// The newest record is only partly written.
    VerifyTornTail,
    /// The export was written.
    ExportDone,
    /// How many kernel lines were withheld.
    ExportRedacted,
    /// The export could not be written; details follow.
    ExportFailed,
}

impl Msg {
    /// All messages, for completeness checks.
    pub const ALL: [Msg; 26] = [
        Msg::Usage,
        Msg::BadArguments,
        Msg::PolicyRequired,
        Msg::PolicyConflict,
        Msg::PolicyRejected,
        Msg::UidMapRejected,
        Msg::CredentialsUnsupported,
        Msg::SecretRejected,
        Msg::LimitsInvalid,
        Msg::RootRefused,
        Msg::HardeningFailed,
        Msg::SocketFailed,
        Msg::DirectoryUnusable,
        Msg::Ready,
        Msg::Stopped,
        Msg::BusFailed,
        Msg::NotPermitted,
        Msg::DirectoryUnreadable,
        Msg::VerifyIntact,
        Msg::VerifyBroken,
        Msg::VerifyUnkeyedNote,
        Msg::VerifyGap,
        Msg::VerifyTornTail,
        Msg::ExportDone,
        Msg::ExportRedacted,
        Msg::ExportFailed,
    ];
}

/// Synopsis of `ultar-logd` (language independent).
pub const LOGD_SYNOPSIS: &str =
    "ultar-logd --dir DIR (--listen PATH | --listen PATH=NAME)... [--mode OCTAL] \
[--uid-map FILE] [--bus PATH (--policy FILE | --allow-all) [--bus-token-file FILE]] \
[--chain-key-file FILE] [--kernel-log PATH] [--segment-bytes N] [--total-bytes N] [--max-age-s N] \
[--ring-records N] [--rate-burst N] [--rate-per-second N] [--fsync always|batch|rotate] \
[--allow-root] [--lang ar|en|de]";

/// Synopsis of `ulog` (language independent).
pub const ULOG_SYNOPSIS: &str = "ulog [--bus PATH] [--token-file FILE] [--lang ar|en|de] \
(query | follow [--count N] | stats) [--service NAME] [--level LEVEL] [--since MS] [--until MS] \
[--after SEQ] [--limit N]\n  ulog verify DIR [--key-file FILE]\n  \
ulog export DIR OUT [--key-file FILE] [--include-kernel-text]";

fn arabic(msg: Msg) -> &'static str {
    match msg {
        Msg::Usage => "طريقة الاستخدام:",
        Msg::BadArguments => "خيارات غير صحيحة.",
        Msg::PolicyRequired => "يجب تحديد ملف الصلاحيات (‎--policy‎) أو ‎--allow-all‎.",
        Msg::PolicyConflict => "لا يجوز الجمع بين ‎--policy‎ و‎--allow-all‎.",
        Msg::PolicyRejected => "ملف الصلاحيات مرفوض:",
        Msg::UidMapRejected => "ملف ربط المستخدمين بالهويات مرفوض:",
        Msg::CredentialsUnsupported => {
            "النواة لا تُبلّغ عن هوية الطرف المتصل؛ استخدم مقبسًا باسم ثابت لكل خدمة."
        }
        Msg::SecretRejected => "الملف السري مرفوض:",
        Msg::LimitsInvalid => "الحدود المحددة لا تتوافق فيما بينها.",
        Msg::RootRefused => "لن تعمل خدمة السجل بصلاحيات الجذر دون ‎--allow-root‎.",
        Msg::HardeningFailed => "تعذّر تطبيق قيود الحماية الذاتية:",
        Msg::SocketFailed => "تعذّر فتح المقبس:",
        Msg::DirectoryUnusable => "مجلد السجل غير قابل للاستخدام؛ تُحفظ السجلات في الذاكرة فقط.",
        Msg::Ready => "خدمة السجل جاهزة.",
        Msg::Stopped => "توقفت خدمة السجل.",
        Msg::BusFailed => "تعذّر الوصول إلى خدمة السجل:",
        Msg::NotPermitted => "غير مسموح لك بهذه العملية.",
        Msg::DirectoryUnreadable => "تعذّرت قراءة مجلد السجل:",
        Msg::VerifyIntact => "السجل سليم.",
        Msg::VerifyBroken => "السجل غير سليم!",
        Msg::VerifyUnkeyedNote => {
            "تم التحقق دون مفتاح: يُكشف التلف والتعديل البسيط، لا إعادة كتابة السلسلة كاملة."
        }
        Msg::VerifyGap => "مواضع لم تُكتب على القرص (عطل في التخزين):",
        Msg::VerifyTornTail => {
            "آخر سجل مكتوب جزئيًا (انقطاع أثناء الكتابة)؛ يُصلَح عند التشغيل التالي."
        }
        Msg::ExportDone => "تم تصدير السجل:",
        Msg::ExportRedacted => "أسطر النواة المحجوبة:",
        Msg::ExportFailed => "تعذّر تصدير السجل:",
    }
}

fn english(msg: Msg) -> &'static str {
    match msg {
        Msg::Usage => "Usage:",
        Msg::BadArguments => "Invalid arguments.",
        Msg::PolicyRequired => "A policy file (--policy) or --allow-all is required.",
        Msg::PolicyConflict => "--policy and --allow-all cannot be combined.",
        Msg::PolicyRejected => "Policy file rejected:",
        Msg::UidMapRejected => "User-to-identity map rejected:",
        Msg::CredentialsUnsupported => {
            "The kernel does not report peer credentials; use one fixed-name socket per service."
        }
        Msg::SecretRejected => "Secret file rejected:",
        Msg::LimitsInvalid => "The given limits do not fit together.",
        Msg::RootRefused => "The log service does not run as root without --allow-root.",
        Msg::HardeningFailed => "The self-limits could not be applied:",
        Msg::SocketFailed => "The socket could not be opened:",
        Msg::DirectoryUnusable => {
            "The log directory cannot be used; records are kept in memory only."
        }
        Msg::Ready => "Log service ready",
        Msg::Stopped => "The log service stopped.",
        Msg::BusFailed => "The log service could not be reached:",
        Msg::NotPermitted => "You are not permitted to do this.",
        Msg::DirectoryUnreadable => "The log directory could not be read:",
        Msg::VerifyIntact => "The log is intact.",
        Msg::VerifyBroken => "The log is NOT intact!",
        Msg::VerifyUnkeyedNote => {
            "Verified without a key: damage and simple edits are detected, a fully recomputed chain is not."
        }
        Msg::VerifyGap => "Positions never written to disk (storage failure):",
        Msg::VerifyTornTail => {
            "The newest record is only partly written (interrupted write); repaired at the next start."
        }
        Msg::ExportDone => "Log exported:",
        Msg::ExportRedacted => "Kernel lines withheld:",
        Msg::ExportFailed => "The log could not be exported:",
    }
}

fn german(msg: Msg) -> &'static str {
    match msg {
        Msg::Usage => "Aufruf:",
        Msg::BadArguments => "Ungültige Argumente.",
        Msg::PolicyRequired => "Eine Rechtedatei (--policy) oder --allow-all ist nötig.",
        Msg::PolicyConflict => "--policy und --allow-all schließen sich aus.",
        Msg::PolicyRejected => "Rechtedatei abgelehnt:",
        Msg::UidMapRejected => "Zuordnung Benutzer → Kennung abgelehnt:",
        Msg::CredentialsUnsupported => {
            "Der Kern meldet keine Kennung des Gegenübers; je Dienst einen Sockel mit festem Namen verwenden."
        }
        Msg::SecretRejected => "Geheime Datei abgelehnt:",
        Msg::LimitsInvalid => "Die angegebenen Grenzen passen nicht zusammen.",
        Msg::RootRefused => "Der Protokolldienst läuft ohne --allow-root nicht als root.",
        Msg::HardeningFailed => "Die Selbstbeschränkung ließ sich nicht anwenden:",
        Msg::SocketFailed => "Der Sockel ließ sich nicht öffnen:",
        Msg::DirectoryUnusable => {
            "Der Protokollordner ist nicht nutzbar; Einträge bleiben nur im Arbeitsspeicher."
        }
        Msg::Ready => "Protokolldienst bereit",
        Msg::Stopped => "Der Protokolldienst wurde beendet.",
        Msg::BusFailed => "Der Protokolldienst ist nicht erreichbar:",
        Msg::NotPermitted => "Dafür fehlt die Berechtigung.",
        Msg::DirectoryUnreadable => "Der Protokollordner ließ sich nicht lesen:",
        Msg::VerifyIntact => "Das Protokoll ist unversehrt.",
        Msg::VerifyBroken => "Das Protokoll ist NICHT unversehrt!",
        Msg::VerifyUnkeyedNote => {
            "Ohne Schlüssel geprüft: Schäden und einfache Änderungen fallen auf, eine vollständig neu berechnete Kette nicht."
        }
        Msg::VerifyGap => "Nie auf den Datenträger geschriebene Positionen (Speicherfehler):",
        Msg::VerifyTornTail => {
            "Der neueste Eintrag ist nur teilweise geschrieben (Abbruch beim Schreiben); wird beim nächsten Start bereinigt."
        }
        Msg::ExportDone => "Protokoll exportiert:",
        Msg::ExportRedacted => "Zurückgehaltene Kern-Zeilen:",
        Msg::ExportFailed => "Das Protokoll ließ sich nicht exportieren:",
    }
}

/// The text of `msg` in `lang`.
pub fn text(lang: Lang, msg: Msg) -> &'static str {
    match lang {
        Lang::Arabic => arabic(msg),
        Lang::English => english(msg),
        Lang::German => german(msg),
    }
}

/// The language for operator messages: `--lang` wherever it stands on the
/// command line (also when the rest cannot be parsed), else the
/// environment.
pub fn language(words: &[String]) -> Lang {
    words
        .iter()
        .position(|word| word == "--lang")
        .and_then(|index| words.get(index + 1))
        .map_or_else(Lang::from_env, |tag| Lang::from_tag(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_exists_and_differs_in_all_languages() {
        for msg in Msg::ALL {
            let texts = Lang::ALL.map(|lang| text(lang, msg));
            assert!(texts.iter().all(|text| !text.is_empty()), "{msg:?}");
            assert_ne!(texts[0], texts[1], "{msg:?}");
            assert_ne!(texts[1], texts[2], "{msg:?}");
        }
        assert_eq!(Lang::ALL[0], Lang::Arabic);
        assert!(LOGD_SYNOPSIS.starts_with("ultar-logd") && ULOG_SYNOPSIS.starts_with("ulog"));
    }

    #[test]
    fn the_language_option_is_found_anywhere() {
        let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        assert_eq!(language(&words(&["--bogus", "--lang", "de"])), Lang::German);
        assert_eq!(language(&words(&["--lang", "en", "x"])), Lang::English);
        assert_eq!(language(&words(&["x", "--lang", "ar"])), Lang::Arabic);
    }
}
