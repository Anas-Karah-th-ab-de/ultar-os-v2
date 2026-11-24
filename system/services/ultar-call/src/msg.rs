//! Texts of the `ultar-calld` command line, Arabic first (ar/en/de).

/// Messages of the `ultar-calld` command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Wrong arguments.
    Usage,
    /// The bus cannot be reached.
    NoBus,
    /// The bus connection ended.
    Stopped,
    /// Lines of the call log could not be read.
    BadLog,
}

/// Messages, Arabic first.
pub fn text(msg: Msg, lang: &str) -> &'static str {
    match (msg, lang) {
        (Msg::Usage, "en") => "usage: ultar-calld --bus SOCKET [--log FILE] [--modem] [--fake-modem] [--ultar NUMBER]",
        (Msg::Usage, "de") => "Aufruf: ultar-calld --bus SOCKEL [--log DATEI] [--modem] [--fake-modem] [--ultar NUMMER]",
        (Msg::Usage, _) => "الاستخدام: ultar-calld --bus SOCKET [--log FILE] [--modem] [--fake-modem] [--ultar NUMBER]",
        (Msg::NoBus, "en") => "ultar-calld: bus not reachable",
        (Msg::NoBus, "de") => "ultar-calld: Bus nicht erreichbar",
        (Msg::NoBus, _) => "ultar-calld: الناقل غير متاح",
        (Msg::Stopped, "en") => "ultar-calld: bus connection ended",
        (Msg::Stopped, "de") => "ultar-calld: Busverbindung beendet",
        (Msg::Stopped, _) => "ultar-calld: انتهى الاتصال بالناقل",
        (Msg::BadLog, "en") => "ultar-calld: unreadable lines in the call log were dropped",
        (Msg::BadLog, "de") => "ultar-calld: unlesbare Zeilen der Anrufliste verworfen",
        (Msg::BadLog, _) => "ultar-calld: حُذفت أسطر غير مقروءة من سجلّ المكالمات",
    }
}
