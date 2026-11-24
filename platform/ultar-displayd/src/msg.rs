//! Messages of the command line in Arabic, English and German (Arabic first).

/// A language of the messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    /// Arabic.
    Ar,
    /// English.
    En,
    /// German.
    De,
}

impl Lang {
    /// From `--lang` or `LANG` (`ar_SY.UTF-8` …); English otherwise.
    pub fn from_tag(tag: &str) -> Lang {
        match tag.get(..2) {
            Some("ar") => Lang::Ar,
            Some("de") => Lang::De,
            _ => Lang::En,
        }
    }
}

/// A message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    /// How to call the program.
    Usage,
    /// The options were wrong.
    BadArguments,
    /// The service could not limit itself at start.
    Hardening,
    /// The bus could not be reached or the name not registered.
    Bus,
}

/// The text of a message.
pub fn text(lang: Lang, msg: Msg) -> &'static str {
    match (msg, lang) {
        (Msg::Usage, Lang::Ar) => "الاستعمال: ultar-displayd --bus مسار --compositor مسار [--log-socket مسار] [--token-file ملف] [--allow-root]",
        (Msg::Usage, Lang::En) => "usage: ultar-displayd --bus PATH --compositor PATH [--log-socket PATH] [--token-file FILE] [--allow-root]",
        (Msg::Usage, Lang::De) => "Aufruf: ultar-displayd --bus PFAD --compositor PFAD [--log-socket PFAD] [--token-file DATEI] [--allow-root]",
        (Msg::BadArguments, Lang::Ar) => "خيارات غير صالحة (المسارات يجب أن تكون مطلقة)",
        (Msg::BadArguments, Lang::En) => "invalid options (paths must be absolute)",
        (Msg::BadArguments, Lang::De) => "ungültige Optionen (Pfade müssen absolut sein)",
        (Msg::Hardening, Lang::Ar) => "تعذّر تقييد الخدمة عند البدء",
        (Msg::Hardening, Lang::En) => "the service could not restrict itself at start",
        (Msg::Hardening, Lang::De) => "der Dienst konnte sich beim Start nicht einschränken",
        (Msg::Bus, Lang::Ar) => "ناقل النظام غير متاح أو الاسم «display» محجوز",
        (Msg::Bus, Lang::En) => "the system bus is not reachable or the name \"display\" is taken",
        (Msg::Bus, Lang::De) => "Systembus nicht erreichbar oder der Name „display“ ist belegt",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_in_every_language() {
        for lang in [Lang::Ar, Lang::En, Lang::De] {
            for msg in [Msg::Usage, Msg::BadArguments, Msg::Hardening, Msg::Bus] {
                assert!(!text(lang, msg).is_empty());
            }
        }
        assert_eq!(Lang::from_tag("ar_SY.UTF-8"), Lang::Ar);
        assert_eq!(Lang::from_tag("de"), Lang::De);
        assert_eq!(Lang::from_tag(""), Lang::En);
    }
}
