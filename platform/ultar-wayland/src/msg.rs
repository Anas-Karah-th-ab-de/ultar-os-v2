//! What the bridge tells the operator (stderr): keys in the logic, text in
//! the tables, Arabic first. The language comes from `ULTAR_LANG` (the
//! launcher sets it), then `LANG`; Arabic when neither names en or de.

/// Languages of the tables.
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
    /// From the environment.
    pub fn from_env() -> Lang {
        let pick = |v: &str| match v.get(..2) {
            Some("en") => Some(Lang::En),
            Some("de") => Some(Lang::De),
            Some("ar") => Some(Lang::Ar),
            _ => None,
        };
        std::env::var("ULTAR_LANG")
            .ok()
            .and_then(|v| pick(&v))
            .or_else(|| std::env::var("LANG").ok().and_then(|v| pick(&v)))
            .unwrap_or(Lang::Ar)
    }
}

/// Everything the bridge can say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    /// How to call it.
    Usage,
    /// The compositor could not be reached.
    CompositorFailed,
    /// The Wayland socket could not be made.
    SocketFailed,
    /// The program could not be started.
    StartFailed,
    /// The compositor went away; the program is ended.
    CompositorGone,
}

impl Msg {
    /// Every message (for the completeness test).
    pub const ALL: [Msg; 5] = [
        Msg::Usage,
        Msg::CompositorFailed,
        Msg::SocketFailed,
        Msg::StartFailed,
        Msg::CompositorGone,
    ];

    /// The text.
    pub fn text(self, lang: Lang) -> &'static str {
        let [ar, en, de] = self.texts();
        match lang {
            Lang::Ar => ar,
            Lang::En => en,
            Lang::De => de,
        }
    }

    fn texts(self) -> [&'static str; 3] {
        match self {
            Msg::Usage => [
                "الاستخدام: ultar-wayland [--compositor مسار] [--runtime-dir مجلد] [--app-id معرّف] -- برنامج [وسائط…]",
                "usage: ultar-wayland [--compositor PATH] [--runtime-dir DIR] [--app-id ID] -- program [args…]",
                "Aufruf: ultar-wayland [--compositor PFAD] [--runtime-dir ORDNER] [--app-id KENNUNG] -- Programm [Argumente…]",
            ],
            Msg::CompositorFailed => [
                "تعذّر الاتصال بمركّب النوافذ",
                "could not reach the compositor",
                "Compositor nicht erreichbar",
            ],
            Msg::SocketFailed => [
                "تعذّر إنشاء مقبس Wayland",
                "could not create the Wayland socket",
                "Wayland-Socket konnte nicht angelegt werden",
            ],
            Msg::StartFailed => [
                "تعذّر تشغيل البرنامج",
                "could not start the program",
                "Programm konnte nicht gestartet werden",
            ],
            Msg::CompositorGone => [
                "انقطع الاتصال بمركّب النوافذ؛ أُنهي البرنامج",
                "the compositor went away; the program was ended",
                "Verbindung zum Compositor verloren; das Programm wurde beendet",
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_in_every_language() {
        for m in Msg::ALL {
            for l in [Lang::Ar, Lang::En, Lang::De] {
                assert!(!m.text(l).is_empty());
            }
            assert_ne!(m.text(Lang::Ar), m.text(Lang::En));
        }
    }
}
