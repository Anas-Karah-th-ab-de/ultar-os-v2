//! Operator messages of `ulaunch` and of the daemon's start errors: keys in
//! the logic, text in the tables, Arabic first. (The daemon's records are
//! keys and words, not sentences.)

pub use ultar_bus::msg::Lang;

/// Everything `ulaunch` and the daemon can say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Msg {
    /// How to call the program (the synopsis follows).
    Usage,
    /// The command line was not understood.
    BadArguments,
    /// The launcher could not be reached; details follow.
    BusFailed,
    /// The caller may not do this.
    NotPermitted,
    /// No installed app of that name.
    UnknownApp,
    /// The manifest does not declare it.
    NotDeclared,
    /// The app's profile cannot be granted it.
    NotGrantable,
    /// A permission is missing (background for a service).
    NotGranted,
    /// The package was refused.
    PackageRefused,
    /// The kernel lacks an isolation feature.
    Platform,
    /// Too many apps run, or no user id is left.
    Limit,
    /// The app could not be started.
    StartFailed,
    /// Other refusal; the key follows.
    Refused,
    /// The request was done; details follow.
    Done,
    /// Column headers of the list.
    ListHeader,
    /// The daemon could not start; the key follows.
    DaemonFailed,
}

impl Msg {
    /// Every message (for the completeness test).
    pub const ALL: [Msg; 16] = [
        Msg::Usage,
        Msg::BadArguments,
        Msg::BusFailed,
        Msg::NotPermitted,
        Msg::UnknownApp,
        Msg::NotDeclared,
        Msg::NotGrantable,
        Msg::NotGranted,
        Msg::PackageRefused,
        Msg::Platform,
        Msg::Limit,
        Msg::StartFailed,
        Msg::Refused,
        Msg::Done,
        Msg::ListHeader,
        Msg::DaemonFailed,
    ];
}

/// Synopsis of `ulaunch` (language independent).
pub const SYNOPSIS: &str = "ulaunch [--bus PATH] [--lang ar|en|de] (list | start APP [--service] \
[--token N] | stop APP [--service] | permissions APP | grant APP PERMISSION | revoke APP PERMISSION \
| install FILE.uap | uninstall APP [--keep-data])";

fn arabic(msg: Msg) -> &'static str {
    match msg {
        Msg::Usage => "طريقة الاستخدام:",
        Msg::BadArguments => "خيارات غير صحيحة.",
        Msg::BusFailed => "تعذّر الوصول إلى مشغّل التطبيقات:",
        Msg::NotPermitted => "غير مسموح لك بهذا.",
        Msg::UnknownApp => "لا يوجد تطبيق مثبّت بهذا الاسم.",
        Msg::NotDeclared => "لم يطلب التطبيق هذا في بيانه.",
        Msg::NotGrantable => "لا يمكن منح هذه الصلاحية لهذا النوع من التطبيقات.",
        Msg::NotGranted => "صلاحية لازمة لم تُمنح بعد.",
        Msg::PackageRefused => "رُفضت الحزمة (التوقيع أو البيان).",
        Msg::Platform => "النواة تفتقد ميزة عزل لازمة، فلم يُشغَّل التطبيق.",
        Msg::Limit => "تم بلوغ الحد الأقصى.",
        Msg::StartFailed => "تعذّر تشغيل التطبيق.",
        Msg::Refused => "رُفض الطلب:",
        Msg::Done => "تم:",
        Msg::ListHeader => "التطبيق\tالمستخدم\tالملف الشخصي\tالإصدار\tالدور\tالحالة\tالمعرّف\tإعادات التشغيل\tآخر نهاية",
        Msg::DaemonFailed => "تعذّر بدء مشغّل التطبيقات:",
    }
}

fn english(msg: Msg) -> &'static str {
    match msg {
        Msg::Usage => "Usage:",
        Msg::BadArguments => "Invalid arguments.",
        Msg::BusFailed => "The app launcher could not be reached:",
        Msg::NotPermitted => "You are not permitted to do this.",
        Msg::UnknownApp => "There is no installed app of that name.",
        Msg::NotDeclared => "The app's manifest does not declare this.",
        Msg::NotGrantable => "This permission cannot be granted to this kind of app.",
        Msg::NotGranted => "A permission it needs is not granted.",
        Msg::PackageRefused => "The package was refused (signature or manifest).",
        Msg::Platform => "The kernel lacks an isolation feature; the app was not started.",
        Msg::Limit => "A limit was reached.",
        Msg::StartFailed => "The app could not be started.",
        Msg::Refused => "Refused:",
        Msg::Done => "Done:",
        Msg::ListHeader => "app\tuser\tprofile\tversion\trole\tstate\tpid\trestarts\tlast end",
        Msg::DaemonFailed => "The app launcher could not start:",
    }
}

fn german(msg: Msg) -> &'static str {
    match msg {
        Msg::Usage => "Aufruf:",
        Msg::BadArguments => "Ungültige Angaben.",
        Msg::BusFailed => "Der App-Starter ist nicht erreichbar:",
        Msg::NotPermitted => "Das ist dir nicht erlaubt.",
        Msg::UnknownApp => "Es ist keine App dieses Namens installiert.",
        Msg::NotDeclared => "Das Manifest der App gibt das nicht an.",
        Msg::NotGrantable => "Dieses Recht kann dieser Art von App nicht gegeben werden.",
        Msg::NotGranted => "Ein nötiges Recht ist nicht erteilt.",
        Msg::PackageRefused => "Das Paket wurde abgelehnt (Signatur oder Manifest).",
        Msg::Platform => "Dem Kern fehlt eine Abschottung; die App wurde nicht gestartet.",
        Msg::Limit => "Eine Grenze ist erreicht.",
        Msg::StartFailed => "Die App konnte nicht gestartet werden.",
        Msg::Refused => "Abgelehnt:",
        Msg::Done => "Erledigt:",
        Msg::ListHeader => {
            "App\tNutzer\tProfil\tVersion\tRolle\tZustand\tPID\tNeustarts\tletztes Ende"
        }
        Msg::DaemonFailed => "Der App-Starter konnte nicht starten:",
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

/// The message for a service error code of the contract.
pub fn for_code(code: u32) -> Msg {
    use ultar_bus::messages::launcher as c;
    match code {
        c::ERROR_UNKNOWN_APP => Msg::UnknownApp,
        c::ERROR_NOT_DECLARED => Msg::NotDeclared,
        c::ERROR_NOT_GRANTABLE => Msg::NotGrantable,
        c::ERROR_NOT_GRANTED => Msg::NotGranted,
        c::ERROR_PACKAGE_REFUSED => Msg::PackageRefused,
        c::ERROR_PLATFORM => Msg::Platform,
        c::ERROR_LIMIT => Msg::Limit,
        c::ERROR_START_FAILED => Msg::StartFailed,
        _ => Msg::Refused,
    }
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
        assert!(SYNOPSIS.starts_with("ulaunch"));
        let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        assert_eq!(language(&words(&["list", "--lang", "de"])), Lang::German);
        assert_eq!(for_code(1), Msg::UnknownApp);
        assert_eq!(for_code(99), Msg::Refused);
        for code in 1..=11 {
            let _ = for_code(code);
        }
    }
}
