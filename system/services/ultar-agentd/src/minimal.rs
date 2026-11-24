//! The smallest help: fixed commands understood without any model. This is what a device
//! gets that cannot run a model and has no paired device that lends one – and what the
//! helper falls back to when a model fails.

use crate::msg;
use crate::tools::Call;

fn has(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| text.contains(w))
}

fn number(text: &str) -> Option<u8> {
    let digits: String = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse::<u8>().ok().filter(|n| *n <= 100)
}

/// The one call a request means, or `None` when the smallest help does not understand it.
pub fn understand(request: &str) -> Option<Call> {
    let text = format!(" {} ", request.to_lowercase());
    let text = text.as_str();
    if has(text, &msg::UNMUTE) {
        return Some(Call::new("mute", &[("on", "false")]));
    }
    if has(text, &msg::MUTE) {
        return Some(Call::new("mute", &[("on", "true")]));
    }
    if has(text, &msg::WIFI) {
        // "Off" is looked for first: nobody loses the network by a misread word.
        let on = if has(text, &msg::OFF) {
            "false"
        } else if has(text, &msg::ON) {
            "true"
        } else {
            return None;
        };
        return Some(Call::new("wifi", &[("on", on)]));
    }
    if has(text, &msg::VOLUME) {
        let percent = number(text)?.to_string();
        return Some(Call::new("set_volume", &[("percent", &percent)]));
    }
    if has(text, &msg::CONTINUE) {
        return Some(Call::new("continue_on", &[("device", "")]));
    }
    if has(text, &msg::DEVICES) {
        return Some(Call::new("devices", &[]));
    }
    if has(text, &msg::OPEN) {
        let app = msg::APPS.iter().find(|(words, _)| has(text, words))?;
        return Some(Call::new("open_app", &[("app", app.1)]));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(request: &str) -> Option<(String, String)> {
        let call = understand(request)?;
        let first = call.args.values().next().cloned().unwrap_or_default();
        Some((call.tool, first))
    }

    #[test]
    fn fixed_commands_are_understood_in_three_languages() {
        let wanted = |tool: &str, arg: &str| Some((tool.to_owned(), arg.to_owned()));
        assert_eq!(
            call("افتح الإعدادات"),
            wanted("open_app", "org.ultar.settings")
        );
        assert_eq!(
            call("Open the browser"),
            wanted("open_app", "org.ultar.browser")
        );
        assert_eq!(call("Öffne Dateien"), wanted("open_app", "org.ultar.files"));
        assert_eq!(call("الصوت 40"), wanted("set_volume", "40"));
        assert_eq!(call("Lautstärke auf 15"), wanted("set_volume", "15"));
        assert_eq!(call("mute"), wanted("mute", "true"));
        assert_eq!(call("unmute please"), wanted("mute", "false"));
        assert_eq!(call("أطفئ الواي فاي"), wanted("wifi", "false"));
        assert_eq!(call("turn wifi on"), wanted("wifi", "true"));
        assert_eq!(call("أكمل على جهاز آخر"), wanted("continue_on", ""));
        assert_eq!(call("أجهزتي"), wanted("devices", ""));
    }

    #[test]
    fn what_is_not_a_fixed_command_is_not_guessed() {
        assert_eq!(call("write a poem"), None);
        assert_eq!(call("volume 300"), None, "out of range");
        assert_eq!(call("wifi"), None, "on or off was not said");
        assert_eq!(call("open the pod bay doors"), None, "no such app");
        assert_eq!(
            call("switch off"),
            None,
            "the smallest help never powers off"
        );
    }
}
