//! The message catalogues of this crate (`locales/*.ftl`, Arabic first) and helpers to render
//! errors for the user.

use crate::error::{CloudError, Subject};
use ultar_i18n::{Arg, Lang, ParseError, Translator};

/// Arabic catalogue (reference).
pub const AR: &str = include_str!("../locales/ar.ftl");
/// English catalogue.
pub const EN: &str = include_str!("../locales/en.ftl");
/// German catalogue.
pub const DE: &str = include_str!("../locales/de.ftl");

/// Adds the `cloud.*` messages of all three languages to a translator.
pub fn register(translator: &mut Translator) -> Result<(), ParseError> {
    translator.add_source(Lang::Ar, AR)?;
    translator.add_source(Lang::En, EN)?;
    translator.add_source(Lang::De, DE)
}

/// The text of an error for the user, in the translator's language.
pub fn describe(translator: &Translator, error: &CloudError, subject: Subject) -> String {
    let key = error.message_key(subject);
    match error.retry_after_secs() {
        Some(seconds) => {
            let seconds = i64::try_from(seconds).unwrap_or(i64::MAX);
            translator.t_args(key, &[("seconds", Arg::Int(seconds))])
        }
        None => translator.t(key).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AppAccess;

    fn translator(lang: Lang) -> Translator {
        let mut tr = Translator::empty(lang);
        register(&mut tr).unwrap();
        tr
    }

    fn all_errors() -> Vec<CloudError> {
        vec![
            CloudError::AuthRequired,
            CloudError::NotEntitled,
            CloudError::NotFound,
            CloudError::RateLimited {
                retry_after_secs: 3,
            },
            CloudError::InvalidRequest,
            CloudError::InvalidCredentials,
            CloudError::UsernameTaken,
            CloudError::Server { status: 500 },
            CloudError::Transport(String::new()),
            CloudError::InvalidResponse,
            CloudError::Io(std::io::Error::other("x")),
            CloudError::Integrity,
            CloudError::Package(String::new()),
            CloudError::NotSignedIn,
            CloudError::Vpn(String::new()),
        ]
    }

    #[test]
    fn every_error_key_exists_in_every_language() {
        for lang in Lang::ALL {
            let tr = translator(lang);
            for error in all_errors() {
                for subject in [Subject::App, Subject::UpdateChannel, Subject::General] {
                    let key = error.message_key(subject);
                    assert!(tr.catalog(lang).contains(key), "{key} missing in {lang:?}");
                    assert_ne!(describe(&tr, &error, subject), key);
                }
            }
            for state in [
                AppAccess::Free,
                AppAccess::PremiumSignInRequired,
                AppAccess::PremiumNotIncluded,
                AppAccess::Unlocked,
            ] {
                assert!(
                    tr.catalog(lang).contains(state.message_key()),
                    "{:?}",
                    state
                );
            }
        }
    }

    #[test]
    fn required_texts_read_as_specified() {
        let en = translator(Lang::En);
        assert_eq!(
            describe(&en, &CloudError::AuthRequired, Subject::App),
            "Sign in to get this app."
        );
        assert_eq!(
            describe(&en, &CloudError::NotEntitled, Subject::App),
            "This app is not included in your access."
        );
        let de = translator(Lang::De);
        assert_eq!(
            describe(&de, &CloudError::AuthRequired, Subject::App),
            "Melde dich an, um diese App zu erhalten."
        );
        let ar = translator(Lang::Ar);
        assert_eq!(
            describe(&ar, &CloudError::AuthRequired, Subject::App),
            "سجّل الدخول للحصول على هذا التطبيق."
        );
    }

    #[test]
    fn rate_limit_text_carries_the_wait_with_plural_forms() {
        let en = translator(Lang::En);
        let one = CloudError::RateLimited {
            retry_after_secs: 1,
        };
        let many = CloudError::RateLimited {
            retry_after_secs: 30,
        };
        assert_eq!(
            describe(&en, &one, Subject::App),
            "Too many requests. Try again in 1 second."
        );
        assert_eq!(
            describe(&en, &many, Subject::App),
            "Too many requests. Try again in 30 seconds."
        );
        let ar = translator(Lang::Ar);
        assert!(describe(
            &ar,
            &CloudError::RateLimited {
                retry_after_secs: 2
            },
            Subject::App
        )
        .contains("ثانيتين"));
        assert!(describe(&ar, &many, Subject::App).contains("30"));
    }

    #[test]
    fn catalogues_are_complete_and_consistent() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
        let report = ultar_i18n::check_dir::check_directory(&dir).unwrap();
        assert_eq!(report.files.len(), 3);
        assert!(report.is_clean(), "{:?}", report.problems);
    }
}
