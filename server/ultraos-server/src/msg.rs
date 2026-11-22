//! Texts of the command line (administration and start-up errors) in Arabic, English and German.
//!
//! Logic only ever names a message key; this table is the single place with wording. Arabic is
//! the first and the fallback language. Placeholders are `{0}`, `{1}`, …

/// Output language.
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
    /// Parses a locale string such as `de_DE.UTF-8`; `None` for other languages.
    pub fn parse(locale: &str) -> Option<Lang> {
        let code = locale.get(..2)?.to_ascii_lowercase();
        match code.as_str() {
            "ar" => Some(Lang::Ar),
            "en" => Some(Lang::En),
            "de" => Some(Lang::De),
            _ => None,
        }
    }

    /// Picks the language from the first usable candidate (order: `LC_ALL`, `LC_MESSAGES`,
    /// `LANG`); Arabic when none names a supported language.
    pub fn from_candidates<'a>(candidates: impl IntoIterator<Item = &'a str>) -> Lang {
        candidates
            .into_iter()
            .find_map(Lang::parse)
            .unwrap_or(Lang::Ar)
    }

    /// Picks the language from the process environment.
    pub fn from_env() -> Lang {
        let values: Vec<String> = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .collect();
        Lang::from_candidates(values.iter().map(String::as_str))
    }

    fn index(self) -> usize {
        match self {
            Lang::Ar => 0,
            Lang::En => 1,
            Lang::De => 2,
        }
    }
}

const USAGE_AR: &str = "الاستخدام:
  ultraos-server                                   تشغيل الخادم
  ultraos-server admin grant <مستخدم> <app|channel> <معرّف>
  ultraos-server admin revoke <مستخدم> <app|channel> <معرّف>
  ultraos-server admin entitlements <مستخدم>
  ultraos-server admin channel <اسم> <open|gated>
  ultraos-server admin publisher <اسم> <verified|unverified>
  ultraos-server admin premium <معرّف-التطبيق> <on|off>
  ultraos-server admin publish-app <وصف.json> <ملف-الحزمة>
  ultraos-server admin publish-release <وصف.json> <ملف-الصورة>";
const USAGE_EN: &str = "Usage:
  ultraos-server                                   run the server
  ultraos-server admin grant <username> <app|channel> <id>
  ultraos-server admin revoke <username> <app|channel> <id>
  ultraos-server admin entitlements <username>
  ultraos-server admin channel <name> <open|gated>
  ultraos-server admin publisher <name> <verified|unverified>
  ultraos-server admin premium <app-id> <on|off>
  ultraos-server admin publish-app <meta.json> <package-file>
  ultraos-server admin publish-release <meta.json> <image-file>";
const USAGE_DE: &str = "Aufruf:
  ultraos-server                                   Server starten
  ultraos-server admin grant <Benutzer> <app|channel> <Kennung>
  ultraos-server admin revoke <Benutzer> <app|channel> <Kennung>
  ultraos-server admin entitlements <Benutzer>
  ultraos-server admin channel <Name> <open|gated>
  ultraos-server admin publisher <Name> <verified|unverified>
  ultraos-server admin premium <App-Kennung> <on|off>
  ultraos-server admin publish-app <meta.json> <Paketdatei>
  ultraos-server admin publish-release <meta.json> <Abbilddatei>";

/// All messages: key, then Arabic, English, German.
pub const MESSAGES: &[(&str, [&str; 3])] = &[
    ("admin.usage", [USAGE_AR, USAGE_EN, USAGE_DE]),
    (
        "admin.granted",
        ["تم منح الوصول: {0} ← {1} {2}", "Access granted: {0} → {1} {2}", "Zugang erteilt: {0} → {1} {2}"],
    ),
    (
        "admin.already_granted",
        ["الوصول ممنوح من قبل: {0} ← {1} {2}", "Access was already granted: {0} → {1} {2}", "Zugang bestand bereits: {0} → {1} {2}"],
    ),
    (
        "admin.revoked",
        ["تم سحب الوصول: {0} ← {1} {2}", "Access revoked: {0} → {1} {2}", "Zugang entzogen: {0} → {1} {2}"],
    ),
    (
        "admin.not_granted",
        ["لا يوجد وصول لسحبه: {0} ← {1} {2}", "There was no access to revoke: {0} → {1} {2}", "Es gab keinen Zugang zum Entziehen: {0} → {1} {2}"],
    ),
    ("admin.entitlements.header", ["صلاحيات الوصول للحساب {0}:", "Access of account {0}:", "Zugänge des Kontos {0}:"]),
    ("admin.entitlements.none", ["لا توجد صلاحيات وصول للحساب {0}.", "Account {0} has no access records.", "Konto {0} hat keine Zugänge."]),
    ("admin.channel.open", ["القناة {0} مفتوحة للجميع.", "Channel {0} is open to everyone.", "Kanal {0} ist für alle offen."]),
    ("admin.channel.gated", ["القناة {0} تتطلب صلاحية وصول.", "Channel {0} requires access.", "Kanal {0} erfordert einen Zugang."]),
    ("admin.publisher.verified", ["الناشر {0} موثّق.", "Publisher {0} is verified.", "Herausgeber {0} ist geprüft."]),
    ("admin.publisher.unverified", ["الناشر {0} غير موثّق.", "Publisher {0} is not verified.", "Herausgeber {0} ist nicht geprüft."]),
    ("admin.premium.on", ["التطبيق {0} أصبح مميزًا.", "App {0} is now premium.", "App {0} ist jetzt Premium."]),
    ("admin.premium.off", ["التطبيق {0} أصبح مجانيًا.", "App {0} is now free.", "App {0} ist jetzt frei."]),
    (
        "admin.published.app",
        ["تم نشر {0} بالإصدار {1} ({2} بايت، sha256 {3}).", "Published {0} version {1} ({2} bytes, sha256 {3}).", "{0} Version {1} veröffentlicht ({2} Bytes, sha256 {3})."],
    ),
    (
        "admin.published.release",
        ["تم نشر الإصدار رقم {0} ({1} بايت، sha256 {2}).", "Published release number {0} ({1} bytes, sha256 {2}).", "Release Nummer {0} veröffentlicht ({1} Bytes, sha256 {2})."],
    ),
    ("admin.error.unknown_account", ["لا يوجد حساب بهذا الاسم.", "There is no account with this username.", "Es gibt kein Konto mit diesem Benutzernamen."]),
    ("admin.error.unknown_item", ["التطبيق أو القناة غير موجود.", "The app or channel does not exist.", "Die App oder der Kanal existiert nicht."]),
    ("admin.error.invalid_field", ["قيمة غير صالحة في الحقل: {0}", "Invalid value in field: {0}", "Ungültiger Wert im Feld: {0}"]),
    (
        "admin.error.already_published",
        ["هذا الإصدار منشور من قبل ولا يمكن تغييره.", "This version is already published and cannot be changed.", "Diese Version ist bereits veröffentlicht und kann nicht geändert werden."],
    ),
    (
        "admin.error.premium_needs_file",
        ["لا يمكن جعل التطبيق مميزًا قبل نشر حزمته على هذا الخادم.", "The app can only become premium after its package was published on this server.", "Die App kann erst Premium werden, wenn ihr Paket auf diesem Server veröffentlicht wurde."],
    ),
    ("admin.error.database", ["خطأ في قاعدة البيانات: {0}", "Database error: {0}", "Datenbankfehler: {0}"]),
    ("admin.error.file", ["خطأ في الملف: {0}", "File error: {0}", "Dateifehler: {0}"]),
    ("cli.error.read_meta", ["تعذّرت قراءة ملف الوصف: {0}", "Could not read the description file: {0}", "Die Beschreibungsdatei konnte nicht gelesen werden: {0}"]),
    ("cli.error.parse_meta", ["ملف الوصف غير صالح: {0}", "The description file is not valid: {0}", "Die Beschreibungsdatei ist ungültig: {0}"]),
    ("server.error.database", ["تعذّر فتح قاعدة البيانات: {0}", "Could not open the database: {0}", "Die Datenbank konnte nicht geöffnet werden: {0}"]),
    ("server.error.bind", ["تعذّر فتح عنوان الاستماع: {0}", "Could not bind the listen address: {0}", "Die Lauschadresse konnte nicht geöffnet werden: {0}"]),
    ("server.error.serve", ["توقّف الخادم بسبب خطأ: {0}", "The server stopped with an error: {0}", "Der Server wurde mit einem Fehler beendet: {0}"]),
    ("server.error.secret", ["ULTRAOS_SECRET يجب أن يكون 64 رمزًا ست عشريًا.", "ULTRAOS_SECRET must be 64 hexadecimal characters.", "ULTRAOS_SECRET muss aus 64 Hexadezimalzeichen bestehen."]),
    ("server.error.runtime", ["تعذّر بدء بيئة التشغيل: {0}", "Could not start the runtime: {0}", "Die Laufzeitumgebung konnte nicht gestartet werden: {0}"]),
];

/// The text of `key` in `lang`; `None` for an unknown key.
pub fn text(lang: Lang, key: &str) -> Option<&'static str> {
    MESSAGES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, texts)| texts[lang.index()])
}

/// Renders a message with its arguments. An unknown key renders as the key itself.
pub fn render(lang: Lang, key: &str, args: &[String]) -> String {
    let mut out = text(lang, key).unwrap_or(key).to_string();
    for (i, arg) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), arg);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_selection() {
        assert_eq!(Lang::parse("de_DE.UTF-8"), Some(Lang::De));
        assert_eq!(Lang::parse("EN"), Some(Lang::En));
        assert_eq!(Lang::parse("ar_SY"), Some(Lang::Ar));
        assert_eq!(Lang::parse("fr_FR"), None);
        assert_eq!(Lang::parse("C"), None);
        assert_eq!(Lang::from_candidates(["C", "de_DE"]), Lang::De);
        assert_eq!(Lang::from_candidates(["POSIX"]), Lang::Ar);
        assert_eq!(Lang::from_candidates([]), Lang::Ar);
    }

    #[test]
    fn every_message_has_three_texts_with_the_same_placeholders() {
        let mut seen = std::collections::HashSet::new();
        for (key, texts) in MESSAGES {
            assert!(seen.insert(*key), "duplicate key {key}");
            for i in 0..4 {
                let p = format!("{{{i}}}");
                let uses: Vec<bool> = texts.iter().map(|t| t.contains(&p)).collect();
                assert!(
                    uses.iter().all(|u| *u == uses[0]),
                    "{key}: placeholder {p} differs"
                );
            }
            assert!(texts.iter().all(|t| !t.trim().is_empty()), "{key}");
            assert!(
                texts[0]
                    .chars()
                    .any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)),
                "{key}: Arabic"
            );
        }
    }

    #[test]
    fn render_substitutes_and_falls_back_to_the_key() {
        let args = ["anna".to_string(), "app".to_string(), "x".to_string()];
        assert_eq!(
            render(Lang::En, "admin.granted", &args),
            "Access granted: anna → app x"
        );
        assert!(render(Lang::Ar, "admin.granted", &args).contains("anna"));
        assert_eq!(render(Lang::De, "no.such.key", &[]), "no.such.key");
        assert_eq!(text(Lang::De, "no.such.key"), None);
    }
}
