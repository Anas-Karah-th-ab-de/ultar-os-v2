//! What the start screen needs before the data is open: the language and the keyboard of the
//! installation. Kept unencrypted in a LUKS2 token of type `ultar-start` (an external token:
//! cryptsetup keeps and ignores it). Nothing secret is in it.

use crate::{header, Error, Reader};
use serde_json::{json, Value};

/// The token type.
pub const TOKEN_TYPE: &str = "ultar-start";
/// Longest value accepted from a header.
const MAX_VALUE: usize = 16;

/// Language and keyboard id of the installation (`ar`, `en`, `de`; `ar+us`, `us`, `de`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartInfo {
    /// Language code.
    pub lang: String,
    /// Keyboard id of the installer.
    pub keyboard: String,
}

impl StartInfo {
    pub(crate) fn token(&self) -> Value {
        json!({ "type": TOKEN_TYPE, "keyslots": [], "ultar_lang": self.lang, "ultar_keyboard": self.keyboard })
    }
}

/// A short value of the token (`None` when missing or not a plain short word).
fn value(token: &Value, key: &str) -> Option<String> {
    let s = token[key].as_str()?;
    let plain =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '_');
    (!s.is_empty() && s.len() <= MAX_VALUE && s.chars().all(plain)).then(|| s.to_owned())
}

/// The start information of a volume, if its header carries one (no passphrase needed).
pub fn start_info(read: &mut Reader<'_>) -> Result<Option<StartInfo>, Error> {
    let meta = header::read_best(read)?;
    let Some(tokens) = meta["tokens"].as_object() else {
        return Ok(None);
    };
    Ok(tokens
        .values()
        .find(|t| t["type"] == TOKEN_TYPE)
        .map(|t| StartInfo {
            lang: value(t, "ultar_lang").unwrap_or_default(),
            keyboard: value(t, "ultar_keyboard").unwrap_or_default(),
        }))
}
