//! The few texts of the command line, in Arabic, English and German (Arabic first).
//! The language comes from `LANG`/`LC_ALL`; everything else printed is data (keys, paths).

/// One message of the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Msg {
    /// Usage text.
    Usage,
    /// A command failed; the error code follows.
    Failed,
}

/// Language of the messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// Arabic (default).
    Ar,
    /// English.
    En,
    /// German.
    De,
}

impl Lang {
    /// From a locale string such as `de_DE.UTF-8`; unknown → Arabic.
    pub fn from_locale(locale: &str) -> Lang {
        match locale.get(..2) {
            Some("en") => Lang::En,
            Some("de") => Lang::De,
            _ => Lang::Ar,
        }
    }

    /// From the environment (`LC_ALL`, then `LANG`).
    pub fn from_env() -> Lang {
        let v = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default();
        Lang::from_locale(&v)
    }
}

const USAGE_AR: &str = "الاستخدام: ultar-signd serve <مقبس> <مجلد-المفاتيح> <سجل-التدقيق> | keygen <مجلد-المفاتيح> <os|system|store> | sign-uap <مقبس> <system|store> <manifest> <payload.tar.gz> <ناتج.uap> | sign-catalog <مقبس> <ملف> | sign-release <مقبس> <ملف> | pubkey <مقبس> <os|system|store> | ssh-signers <مقبس>";
const USAGE_EN: &str = "usage: ultar-signd serve <socket> <key-dir> <audit-log> | keygen <key-dir> <os|system|store> | sign-uap <socket> <system|store> <manifest> <payload.tar.gz> <out.uap> | sign-catalog <socket> <file> | sign-release <socket> <file> | pubkey <socket> <os|system|store> | ssh-signers <socket>";
const USAGE_DE: &str = "Aufruf: ultar-signd serve <Socket> <Schlüsselordner> <Prüfprotokoll> | keygen <Schlüsselordner> <os|system|store> | sign-uap <Socket> <system|store> <Manifest> <payload.tar.gz> <Ziel.uap> | sign-catalog <Socket> <Datei> | sign-release <Socket> <Datei> | pubkey <Socket> <os|system|store> | ssh-signers <Socket>";

/// The text of `msg` in `lang`.
pub fn text(msg: Msg, lang: Lang) -> &'static str {
    match (msg, lang) {
        (Msg::Usage, Lang::Ar) => USAGE_AR,
        (Msg::Usage, Lang::En) => USAGE_EN,
        (Msg::Usage, Lang::De) => USAGE_DE,
        (Msg::Failed, Lang::Ar) => "فشل التوقيع:",
        (Msg::Failed, Lang::En) => "signing failed:",
        (Msg::Failed, Lang::De) => "Signieren fehlgeschlagen:",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_exists_in_every_language() {
        assert_eq!(Lang::from_locale("de_DE.UTF-8"), Lang::De);
        assert_eq!(Lang::from_locale("en_US"), Lang::En);
        assert_eq!(Lang::from_locale(""), Lang::Ar);
        for lang in [Lang::Ar, Lang::En, Lang::De] {
            for m in [Msg::Usage, Msg::Failed] {
                assert!(!text(m, lang).is_empty());
            }
        }
    }
}
