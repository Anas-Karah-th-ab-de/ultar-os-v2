//! Operator-facing lines of the `ultar-compositor` program (Arabic first).
//! Logic uses [`MsgKey`]; the text lives only in these tables.

use ultar_i18n::Lang;

/// Everything the program prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgKey {
    /// How to call the program.
    Usage,
    /// Serving clients on the socket named after the message.
    Listening,
    /// The command line could not be understood.
    BadArguments,
    /// No display could be opened.
    NoDisplay,
    /// The socket could not be created.
    BindFailed,
    /// The rights file was refused.
    RightsRejected,
    /// The service runtime could not start.
    StartFailed,
}

const USAGE_AR: &str = "\u{627}\u{644}\u{627}\u{633}\u{62A}\u{62E}\u{62F}\u{627}\u{645}: ultar-compositor --socket \u{645}\u{633}\u{627}\u{631} [--headless WxH] [--kms \u{645}\u{633}\u{627}\u{631}] [--fb \u{645}\u{633}\u{627}\u{631}] [--rights \u{645}\u{644}\u{641}] [--inline] [--lang ar|en|de] [--light] [--no-cursor]";
const USAGE_EN: &str = "usage: ultar-compositor --socket PATH [--headless WxH] [--kms PATH] [--fb PATH] [--rights FILE] [--inline] [--lang ar|en|de] [--light] [--no-cursor]";
const USAGE_DE: &str = "Aufruf: ultar-compositor --socket PFAD [--headless BxH] [--kms PFAD] [--fb PFAD] [--rights DATEI] [--inline] [--lang ar|en|de] [--light] [--no-cursor]";

/// The text of a message in a language.
pub fn text(key: MsgKey, lang: Lang) -> &'static str {
    use MsgKey::*;
    match (key, lang) {
        (Usage, Lang::Ar) => USAGE_AR,
        (Usage, Lang::En) => USAGE_EN,
        (Usage, Lang::De) => USAGE_DE,
        (Listening, Lang::Ar) => "\u{64A}\u{633}\u{62A}\u{645}\u{639} \u{639}\u{644}\u{649}",
        (Listening, Lang::En) => "listening on",
        (Listening, Lang::De) => "wartet auf",
        (BadArguments, Lang::Ar) => "\u{648}\u{633}\u{627}\u{626}\u{637} \u{63A}\u{64A}\u{631} \u{635}\u{627}\u{644}\u{62D}\u{629}",
        (BadArguments, Lang::En) => "invalid arguments",
        (BadArguments, Lang::De) => "ung\u{fc}ltige Argumente",
        (NoDisplay, Lang::Ar) => "\u{644}\u{627} \u{62A}\u{648}\u{62C}\u{62F} \u{634}\u{627}\u{634}\u{629}",
        (NoDisplay, Lang::En) => "no display",
        (NoDisplay, Lang::De) => "kein Bildschirm",
        (BindFailed, Lang::Ar) => "\u{62A}\u{639}\u{630}\u{631} \u{625}\u{646}\u{634}\u{627}\u{621} \u{627}\u{644}\u{645}\u{642}\u{628}\u{633}",
        (BindFailed, Lang::En) => "cannot create the socket",
        (BindFailed, Lang::De) => "Socket kann nicht angelegt werden",
        (RightsRejected, Lang::Ar) => "\u{645}\u{644}\u{641} \u{627}\u{644}\u{635}\u{644}\u{627}\u{62D}\u{64A}\u{627}\u{62A} \u{645}\u{631}\u{641}\u{648}\u{636}",
        (RightsRejected, Lang::En) => "rights file rejected",
        (RightsRejected, Lang::De) => "Rechtedatei abgelehnt",
        (StartFailed, Lang::Ar) => "\u{62A}\u{639}\u{630}\u{631} \u{627}\u{644}\u{62A}\u{634}\u{63A}\u{64A}\u{644}",
        (StartFailed, Lang::En) => "cannot start",
        (StartFailed, Lang::De) => "Start nicht m\u{f6}glich",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_exists_in_every_language() {
        let keys = [
            MsgKey::Usage,
            MsgKey::Listening,
            MsgKey::BadArguments,
            MsgKey::NoDisplay,
            MsgKey::BindFailed,
            MsgKey::RightsRejected,
            MsgKey::StartFailed,
        ];
        for key in keys {
            let ar = text(key, Lang::Ar);
            assert!(
                ar.chars().any(|c| ('\u{600}'..='\u{6ff}').contains(&c)),
                "{key:?}"
            );
            assert_ne!(text(key, Lang::En), text(key, Lang::De));
        }
    }
}
