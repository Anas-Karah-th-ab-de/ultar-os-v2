//! What the helper knows about this system without any model: a few topics, each one a
//! short text in the three languages (`ultar-bus/locales/agent/*.ftl`, keys
//! `agent.help.<topic>`).

use crate::msg;
use ultar_bus::messages::agent::LOCALES;

/// The topics.
pub const TOPICS: [&str; 8] = [
    "pairing",
    "continue",
    "files",
    "model",
    "wifi",
    "bluetooth",
    "updates",
    "helper",
];

/// The catalog key of a topic.
pub fn key(topic: &str) -> Option<String> {
    TOPICS
        .contains(&topic)
        .then(|| format!("agent.help.{topic}"))
}

/// The English text of a topic (what a model is told).
pub fn text(topic: &str) -> Option<&'static str> {
    let key = key(topic)?;
    let english = LOCALES.iter().find(|(tag, _)| *tag == "en")?.1;
    let line = english.lines().find_map(|l| l.strip_prefix(key.as_str()))?;
    line.strip_prefix(" = ")
}

/// The topic a question is about: it asks "how" or for help, and names the topic.
pub fn topic_of(request: &str) -> Option<&'static str> {
    let text = format!(" {} ", request.to_lowercase());
    let asks = msg::HOW.iter().any(|w| text.contains(w));
    let named = |words: &&[&str]| words.iter().any(|w| text.contains(w));
    let topic = msg::HELP_TOPICS.iter().find(|(words, _)| named(words))?;
    asks.then_some(topic.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_has_a_text_in_all_three_languages() {
        for (lang, catalog) in LOCALES {
            for topic in TOPICS {
                let key = format!("{} = ", key(topic).unwrap());
                assert!(
                    catalog.lines().any(|l| l.starts_with(&key)),
                    "{lang} {topic}"
                );
            }
        }
        assert!(text("pairing").unwrap().contains("six-digit"));
        assert_eq!(text("weather"), None);
        for (_, topic) in msg::HELP_TOPICS {
            assert!(TOPICS.contains(&topic), "{topic}");
        }
    }

    #[test]
    fn a_question_is_understood_in_three_languages_and_a_command_is_not_a_question() {
        assert_eq!(topic_of("How do I pair my phone?"), Some("pairing"));
        assert_eq!(topic_of("كيف أقرن هاتفي؟"), Some("pairing"));
        assert_eq!(
            topic_of("Wie mache ich auf dem Handy weiter?"),
            Some("continue")
        );
        assert_eq!(topic_of("كيف أتصل بالواي فاي"), Some("wifi"));
        assert_eq!(topic_of("what can you do? help"), Some("helper"));
        assert_eq!(topic_of("wifi off"), None);
        assert_eq!(topic_of("how is the weather"), None);
    }
}
