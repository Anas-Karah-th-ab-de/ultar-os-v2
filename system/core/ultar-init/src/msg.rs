//! Operator messages of `uinit`: keys in the logic, text in the tables,
//! Arabic first. (Init's own journal lines are words and numbers, like the
//! kernel's, not sentences.)

pub use ultar_bus::msg::Lang;

/// Everything `uinit` can say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Msg {
    /// How to call the program (the synopsis follows).
    Usage,
    /// The command line was not understood.
    BadArguments,
    /// The bus could not be reached; details follow.
    BusFailed,
    /// The caller may not do this.
    NotPermitted,
    /// No service of that name.
    UnknownService,
    /// The service already is in the requested state.
    Already,
    /// The system is shutting down.
    ShuttingDown,
    /// The token was rejected; details follow.
    TokenRejected,
    /// The request was accepted; the new state follows.
    Accepted,
    /// Column headers of the status table.
    StatusHeader,
}

impl Msg {
    /// Every message (for the completeness test).
    pub const ALL: [Msg; 10] = [
        Msg::Usage,
        Msg::BadArguments,
        Msg::BusFailed,
        Msg::NotPermitted,
        Msg::UnknownService,
        Msg::Already,
        Msg::ShuttingDown,
        Msg::TokenRejected,
        Msg::Accepted,
        Msg::StatusHeader,
    ];
}

/// Synopsis of `uinit` (language independent).
pub const UINIT_SYNOPSIS: &str = "uinit [--bus PATH] [--token-file FILE | --token-fd N] \
[--lang ar|en|de] (status | start SERVICE | stop SERVICE | restart SERVICE)";

fn arabic(msg: Msg) -> &'static str {
    match msg {
        Msg::Usage => "طريقة الاستخدام:",
        Msg::BadArguments => "خيارات غير صحيحة.",
        Msg::BusFailed => "تعذّر الوصول إلى مدير الخدمات:",
        Msg::NotPermitted => "غير مسموح لك بهذا.",
        Msg::UnknownService => "لا توجد خدمة بهذا الاسم.",
        Msg::Already => "الخدمة في هذه الحالة أصلًا.",
        Msg::ShuttingDown => "النظام يُطفأ الآن.",
        Msg::TokenRejected => "الرمز السري مرفوض:",
        Msg::Accepted => "تم قبول الطلب، الحالة الآن:",
        Msg::StatusHeader => "الخدمة\tالحالة\tالمعرّف\tإعادات التشغيل\tالمستخدم\tآخر نهاية",
    }
}

fn english(msg: Msg) -> &'static str {
    match msg {
        Msg::Usage => "Usage:",
        Msg::BadArguments => "Invalid arguments.",
        Msg::BusFailed => "The service manager could not be reached:",
        Msg::NotPermitted => "You are not permitted to do this.",
        Msg::UnknownService => "There is no service of that name.",
        Msg::Already => "The service already is in that state.",
        Msg::ShuttingDown => "The system is shutting down.",
        Msg::TokenRejected => "The token was rejected:",
        Msg::Accepted => "Request accepted, state now:",
        Msg::StatusHeader => "service\tstate\tpid\trestarts\tuser\tlast end",
    }
}

fn german(msg: Msg) -> &'static str {
    match msg {
        Msg::Usage => "Aufruf:",
        Msg::BadArguments => "Ungültige Argumente.",
        Msg::BusFailed => "Der Dienstverwalter ist nicht erreichbar:",
        Msg::NotPermitted => "Das ist dir nicht erlaubt.",
        Msg::UnknownService => "Einen Dienst dieses Namens gibt es nicht.",
        Msg::Already => "Der Dienst ist bereits in diesem Zustand.",
        Msg::ShuttingDown => "Das System fährt gerade herunter.",
        Msg::TokenRejected => "Das Token wurde abgelehnt:",
        Msg::Accepted => "Auftrag angenommen, Zustand jetzt:",
        Msg::StatusHeader => "Dienst\tZustand\tPID\tNeustarts\tBenutzer\tletztes Ende",
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

/// `--lang` wherever it stands on the command line, else the environment.
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
        assert!(UINIT_SYNOPSIS.starts_with("uinit"));
        let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        assert_eq!(language(&words(&["status", "--lang", "de"])), Lang::German);
    }
}
