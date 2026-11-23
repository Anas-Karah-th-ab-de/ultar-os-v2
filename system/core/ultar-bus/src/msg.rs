//! Message tables for the command line output of `ultar-busd`.
//!
//! The bus sits below the i18n crate in the dependency order, so its few
//! operator-facing lines live here: keys in the logic, text in the tables,
//! Arabic first.

/// Languages the tables cover.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    /// Arabic – the first language of the system.
    #[default]
    Arabic,
    /// English.
    English,
    /// German.
    German,
}

impl Lang {
    /// All languages, Arabic first.
    pub const ALL: [Lang; 3] = [Lang::Arabic, Lang::English, Lang::German];

    /// Picks the language from a locale tag such as `de_DE.UTF-8` or `en`;
    /// anything unknown is Arabic.
    pub fn from_tag(tag: &str) -> Self {
        let lower = tag.to_ascii_lowercase();
        if lower.starts_with("en") {
            Self::English
        } else if lower.starts_with("de") {
            Self::German
        } else {
            Self::Arabic
        }
    }

    /// Picks the language from `ULTAR_LANG`, then `LANG`; Arabic otherwise.
    pub fn from_env() -> Self {
        std::env::var("ULTAR_LANG")
            .or_else(|_| std::env::var("LANG"))
            .map(|tag| Self::from_tag(&tag))
            .unwrap_or_default()
    }
}

/// Everything `ultar-busd` can say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgKey {
    /// How to call the program.
    Usage,
    /// The broker serves the socket named after the message.
    Listening,
    /// The command line could not be understood.
    BadArguments,
    /// Neither a policy file nor `--allow-all` was given.
    PolicyRequired,
    /// The policy file was rejected; details follow.
    PolicyRejected,
    /// The broker could not start; details follow.
    StartFailed,
    /// Warning that every client may do everything.
    AllowAllWarning,
    /// `--policy` and `--allow-all` were given together.
    PolicyConflict,
    /// The kernel does not report peer credentials; a credential listener
    /// cannot identify anybody.
    CredentialsUnsupported,
    /// The uid map file was rejected; details follow.
    UidMapRejected,
    /// The token file was rejected; details follow.
    TokenFileRejected,
    /// The process runs as root without `--allow-root`.
    RootRefused,
    /// The self-limits could not be applied; details follow.
    HardeningFailed,
    /// Something that is not a socket lies at a socket path.
    PathNotSocket,
    /// The broker stopped.
    Stopped,
}

impl MsgKey {
    /// All keys, for completeness checks.
    pub const ALL: [MsgKey; 15] = [
        MsgKey::Usage,
        MsgKey::Listening,
        MsgKey::BadArguments,
        MsgKey::PolicyRequired,
        MsgKey::PolicyRejected,
        MsgKey::StartFailed,
        MsgKey::AllowAllWarning,
        MsgKey::PolicyConflict,
        MsgKey::CredentialsUnsupported,
        MsgKey::UidMapRejected,
        MsgKey::TokenFileRejected,
        MsgKey::RootRefused,
        MsgKey::HardeningFailed,
        MsgKey::PathNotSocket,
        MsgKey::Stopped,
    ];
}

const USAGE_SYNOPSIS: &str =
    "ultar-busd [--listen PATH=IDENTITY | --listen-cred PATH | --listen-token PATH]... \
     [--mode OCTAL] [--owner UID] [--group GID] [--uid-map FILE] [--token-file FILE] \
     [--policy FILE | --allow-all] [--log-socket PATH] [--allow-root] [PATH]";

fn arabic(key: MsgKey) -> &'static str {
    match key {
        MsgKey::Usage => "طريقة الاستخدام:",
        MsgKey::Listening => "ناقل النظام يستمع على:",
        MsgKey::BadArguments => "خيارات غير صحيحة.",
        MsgKey::PolicyRequired => "يجب تحديد ملف الصلاحيات (‎--policy‎) أو ‎--allow-all‎.",
        MsgKey::PolicyRejected => "ملف الصلاحيات مرفوض:",
        MsgKey::StartFailed => "تعذّر تشغيل ناقل النظام:",
        MsgKey::AllowAllWarning => "تحذير: كل عميل مسموح له بكل شيء (للتطوير فقط).",
        MsgKey::PolicyConflict => "لا يجوز الجمع بين ‎--policy‎ و‎--allow-all‎.",
        MsgKey::CredentialsUnsupported => {
            "النواة لا تُبلّغ عن هوية الطرف المتصل؛ استخدم مقبسًا بهوية ثابتة أو رمزًا سريًا."
        }
        MsgKey::UidMapRejected => "ملف ربط المستخدمين بالهويات مرفوض:",
        MsgKey::TokenFileRejected => "ملف الرموز السرية مرفوض:",
        MsgKey::RootRefused => "لن يعمل ناقل النظام بصلاحيات الجذر دون ‎--allow-root‎.",
        MsgKey::HardeningFailed => "تعذّر تطبيق قيود الحماية الذاتية:",
        MsgKey::PathNotSocket => "مسار المقبس مشغول بملف ليس مقبسًا؛ لن يُحذف.",
        MsgKey::Stopped => "توقّف ناقل النظام.",
    }
}

fn english(key: MsgKey) -> &'static str {
    match key {
        MsgKey::Usage => "Usage:",
        MsgKey::Listening => "System bus listening on:",
        MsgKey::BadArguments => "Invalid arguments.",
        MsgKey::PolicyRequired => "A policy file (--policy) or --allow-all is required.",
        MsgKey::PolicyRejected => "Policy file rejected:",
        MsgKey::StartFailed => "The system bus could not start:",
        MsgKey::AllowAllWarning => "Warning: every client may do everything (development only).",
        MsgKey::PolicyConflict => "--policy and --allow-all cannot be combined.",
        MsgKey::CredentialsUnsupported => {
            "The kernel does not report peer credentials; use a fixed-identity socket or a token."
        }
        MsgKey::UidMapRejected => "User-to-identity map rejected:",
        MsgKey::TokenFileRejected => "Token file rejected:",
        MsgKey::RootRefused => "The system bus does not run as root without --allow-root.",
        MsgKey::HardeningFailed => "The self-limits could not be applied:",
        MsgKey::PathNotSocket => {
            "The socket path is occupied by something that is not a socket; it is left alone."
        }
        MsgKey::Stopped => "The system bus stopped.",
    }
}

fn german(key: MsgKey) -> &'static str {
    match key {
        MsgKey::Usage => "Aufruf:",
        MsgKey::Listening => "Systembus lauscht auf:",
        MsgKey::BadArguments => "Ungültige Argumente.",
        MsgKey::PolicyRequired => "Eine Rechtedatei (--policy) oder --allow-all ist nötig.",
        MsgKey::PolicyRejected => "Rechtedatei abgelehnt:",
        MsgKey::StartFailed => "Der Systembus konnte nicht starten:",
        MsgKey::AllowAllWarning => "Warnung: jeder Client darf alles (nur für die Entwicklung).",
        MsgKey::PolicyConflict => "--policy und --allow-all schließen sich aus.",
        MsgKey::CredentialsUnsupported => {
            "Der Kern meldet keine Kennung des Gegenübers; festen Sockel oder Token verwenden."
        }
        MsgKey::UidMapRejected => "Zuordnung Benutzer → Kennung abgelehnt:",
        MsgKey::TokenFileRejected => "Token-Datei abgelehnt:",
        MsgKey::RootRefused => "Der Systembus läuft ohne --allow-root nicht als root.",
        MsgKey::HardeningFailed => "Die Selbstbeschränkung ließ sich nicht anwenden:",
        MsgKey::PathNotSocket => {
            "Am Sockel-Pfad liegt etwas, das kein Sockel ist; es wird nicht gelöscht."
        }
        MsgKey::Stopped => "Der Systembus wurde beendet.",
    }
}

/// The text of `key` in `lang`.
pub fn text(lang: Lang, key: MsgKey) -> &'static str {
    match lang {
        Lang::Arabic => arabic(key),
        Lang::English => english(key),
        Lang::German => german(key),
    }
}

/// The language-independent synopsis printed after [`MsgKey::Usage`].
pub fn synopsis() -> &'static str {
    USAGE_SYNOPSIS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_text_in_every_language() {
        for lang in Lang::ALL {
            for key in MsgKey::ALL {
                assert!(!text(lang, key).is_empty(), "{lang:?} {key:?}");
            }
        }
        assert!(synopsis().starts_with("ultar-busd"));
    }

    #[test]
    fn translations_differ_between_languages() {
        for key in MsgKey::ALL {
            assert_ne!(text(Lang::Arabic, key), text(Lang::English, key));
            assert_ne!(text(Lang::English, key), text(Lang::German, key));
        }
    }

    #[test]
    fn arabic_is_first_and_default() {
        assert_eq!(Lang::ALL[0], Lang::Arabic);
        assert_eq!(Lang::default(), Lang::Arabic);
        assert_eq!(Lang::from_tag("ar_SY.UTF-8"), Lang::Arabic);
        assert_eq!(Lang::from_tag("en_US.UTF-8"), Lang::English);
        assert_eq!(Lang::from_tag("DE"), Lang::German);
        assert_eq!(Lang::from_tag("C"), Lang::Arabic);
        assert_eq!(Lang::from_tag(""), Lang::Arabic);
    }
}
