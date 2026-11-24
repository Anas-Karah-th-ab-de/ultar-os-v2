//! Texts of the command line of every small service (Arabic first). The bus carries
//! keys only; these are for a person at a console.

/// Language of the console texts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    /// Arabic.
    Ar,
    /// English.
    En,
    /// German.
    De,
}

impl Lang {
    /// From a locale tag such as `LANG` (`ar_SY.UTF-8`); English when unknown.
    pub fn from_tag(tag: &str) -> Lang {
        match tag.get(..2) {
            Some("ar") => Lang::Ar,
            Some("de") => Lang::De,
            _ => Lang::En,
        }
    }
}

/// One console text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Msg {
    /// Short usage line.
    Usage,
    /// The command line was refused.
    BadArguments,
    /// Hardening at start failed.
    Hardening,
    /// The bus could not be reached or failed.
    Bus,
    /// The service could not open its state.
    State,
}

const TABLE: [(Msg, [&str; 3]); 5] = [
    (
        Msg::Usage,
        [
            "الاستعمال: ‎--bus مسار [‎--log-socket مسار] [‎--token-file مسار] [خيارات الخدمة]",
            "usage: --bus PATH [--log-socket PATH] [--token-file PATH] [service options]",
            "Aufruf: --bus PFAD [--log-socket PFAD] [--token-file PFAD] [Optionen des Dienstes]",
        ],
    ),
    (
        Msg::BadArguments,
        ["خيارات غير صحيحة", "invalid options", "ungültige Optionen"],
    ),
    (
        Msg::Hardening,
        [
            "تعذّر تقييد الخدمة عند البدء",
            "the service could not limit itself at start",
            "der Dienst konnte sich beim Start nicht einschränken",
        ],
    ),
    (
        Msg::Bus,
        [
            "تعذّر الاتصال بناقل النظام",
            "the system bus could not be reached",
            "der Systembus war nicht erreichbar",
        ],
    ),
    (
        Msg::State,
        [
            "تعذّر فتح بيانات الخدمة",
            "the service could not open its data",
            "der Dienst konnte seine Daten nicht öffnen",
        ],
    ),
];

/// The text of `msg` in `lang`.
pub fn text(lang: Lang, msg: Msg) -> &'static str {
    let column = match lang {
        Lang::Ar => 0,
        Lang::En => 1,
        Lang::De => 2,
    };
    TABLE
        .iter()
        .find(|(m, _)| *m == msg)
        .map_or("", |(_, t)| t[column])
}

/// Checks an i18n catalogue (`key = text` lines, `#` comments): the keys of `keys`
/// that are missing or empty, and the keys it has that `keys` does not list.
pub fn catalogue_gaps(keys: &[&str], catalogue: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut malformed = Vec::new();
    for line in catalogue.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line.split_once(" = ") {
            Some((key, text)) if !text.trim().is_empty() => found.push(key.trim().to_owned()),
            _ => malformed.push(format!("malformed: {line}")),
        }
    }
    let mut gaps: Vec<String> = keys
        .iter()
        .filter(|k| !found.iter().any(|f| f == *k))
        .map(|k| format!("missing: {k}"))
        .collect();
    gaps.extend(
        found
            .into_iter()
            .filter(|f| !keys.contains(&f.as_str()))
            .map(|f| format!("extra: {f}")),
    );
    gaps.extend(malformed);
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_exists_in_every_language() {
        for (msg, _) in TABLE {
            for lang in [Lang::Ar, Lang::En, Lang::De] {
                assert!(!text(lang, msg).is_empty());
            }
        }
        assert_eq!(Lang::from_tag("ar_SY.UTF-8"), Lang::Ar);
        assert_eq!(Lang::from_tag("de_DE"), Lang::De);
        assert_eq!(Lang::from_tag(""), Lang::En);
    }
}

#[cfg(test)]
mod catalogue_tests {
    use super::catalogue_gaps;

    #[test]
    fn gaps_are_found() {
        let text = "# c\na.b = x\nc.d = \nnonsense\ne.f = y\n";
        let gaps = catalogue_gaps(&["a.b", "c.d", "g.h"], text);
        assert!(gaps.contains(&"missing: c.d".to_owned()));
        assert!(gaps.contains(&"missing: g.h".to_owned()));
        assert!(gaps.contains(&"extra: e.f".to_owned()));
        assert!(gaps.iter().any(|g| g.starts_with("malformed")));
        assert!(catalogue_gaps(&["a.b"], "a.b = x").is_empty());
    }
}
