//! Permission policy: which identity may register, call, publish, subscribe.
//!
//! The policy is **default deny**. A [`Rule`] grants operations to every
//! identity matched by its identity pattern; rules are additive. System apps
//! and third-party apps use the same API and only differ in the rules that
//! apply to them.
//!
//! ```toml
//! [[rule]]
//! identity  = "system.settingsd"
//! register  = ["settings"]
//! publish   = ["settings.changed"]
//! subscribe = ["devices.*"]
//!
//! [[rule]]
//! identity  = "app.*"
//! call      = ["settings.get", "notify.post"]
//! subscribe = ["settings.changed", "shell.mode"]
//! ```

use crate::error::BusError;
use crate::identity::Identity;
use crate::policy_text::{parse_policy, PolicyError, PolicyErrorKind};
use crate::topic::Pattern;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Grants a set of operations to all identities matching one pattern.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// Identities this rule applies to (`system.devd`, `app.*`, `*`).
    pub identity: Pattern,
    /// Service names the identity may register.
    #[serde(default)]
    pub register: Vec<Pattern>,
    /// Calls the identity may make, written `service.method`.
    #[serde(default)]
    pub call: Vec<Pattern>,
    /// Topics the identity may publish.
    #[serde(default)]
    pub publish: Vec<Pattern>,
    /// Topics or prefixes the identity may subscribe to.
    #[serde(default)]
    pub subscribe: Vec<Pattern>,
}

impl Rule {
    /// A rule for `identity` that grants nothing yet.
    pub fn new(identity: &str) -> Result<Self, BusError> {
        Ok(Self {
            identity: Pattern::parse(identity)?,
            register: Vec::new(),
            call: Vec::new(),
            publish: Vec::new(),
            subscribe: Vec::new(),
        })
    }

    /// Allows registering services matching `pattern`.
    pub fn register(mut self, pattern: &str) -> Result<Self, BusError> {
        self.register.push(Pattern::parse(pattern)?);
        Ok(self)
    }

    /// Allows calls matching `pattern` (`service.method`, `service.*`, `*`).
    pub fn call(mut self, pattern: &str) -> Result<Self, BusError> {
        self.call.push(Pattern::parse(pattern)?);
        Ok(self)
    }

    /// Allows publishing topics matching `pattern`.
    pub fn publish(mut self, pattern: &str) -> Result<Self, BusError> {
        self.publish.push(Pattern::parse(pattern)?);
        Ok(self)
    }

    /// Allows subscriptions covered by `pattern`.
    pub fn subscribe(mut self, pattern: &str) -> Result<Self, BusError> {
        self.subscribe.push(Pattern::parse(pattern)?);
        Ok(self)
    }
}

/// The complete rule set of a broker.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// All rules; an operation is allowed when any applicable rule grants it.
    #[serde(default, rename = "rule")]
    pub rules: Vec<Rule>,
}

impl Policy {
    /// The empty policy: nobody may do anything.
    pub fn deny_all() -> Self {
        Self::default()
    }

    /// Everybody may do everything. For development and tests only.
    pub fn allow_all() -> Self {
        let any = || vec![Pattern::any()];
        Self {
            rules: vec![Rule {
                identity: Pattern::any(),
                register: any(),
                call: any(),
                publish: any(),
                subscribe: any(),
            }],
        }
    }

    /// Adds a rule.
    pub fn with_rule(mut self, rule: Rule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Reads a policy from TOML text (see the module documentation).
    pub fn from_toml_str(text: &str) -> Result<Self, PolicyError> {
        parse_policy(text)
    }

    /// Reads a policy from a TOML file.
    pub fn load(path: &Path) -> Result<Self, PolicyError> {
        let text = std::fs::read_to_string(path).map_err(|err| PolicyError {
            line: 0,
            kind: PolicyErrorKind::Io(err.kind()),
        })?;
        Self::from_toml_str(&text)
    }

    fn rules_for<'a>(&'a self, identity: &'a Identity) -> impl Iterator<Item = &'a Rule> {
        self.rules
            .iter()
            .filter(|rule| rule.identity.matches(identity.as_str()))
    }

    /// May `identity` register the service `service`?
    pub fn may_register(&self, identity: &Identity, service: &str) -> bool {
        self.rules_for(identity)
            .any(|rule| rule.register.iter().any(|p| p.matches(service)))
    }

    /// May `identity` call `method` of `service`?
    pub fn may_call(&self, identity: &Identity, service: &str, method: &str) -> bool {
        let call = format!("{service}.{method}");
        self.rules_for(identity)
            .any(|rule| rule.call.iter().any(|p| p.matches(&call)))
    }

    /// May `identity` publish on `topic`?
    pub fn may_publish(&self, identity: &Identity, topic: &str) -> bool {
        self.rules_for(identity)
            .any(|rule| rule.publish.iter().any(|p| p.matches(topic)))
    }

    /// May `identity` subscribe to everything `pattern` matches?
    pub fn may_subscribe(&self, identity: &Identity, pattern: &Pattern) -> bool {
        self.rules_for(identity)
            .any(|rule| rule.subscribe.iter().any(|p| p.covers(pattern)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> Identity {
        Identity::new(name).unwrap()
    }

    fn sample() -> Policy {
        Policy::deny_all()
            .with_rule(
                Rule::new("system.settingsd")
                    .and_then(|rule| rule.register("settings"))
                    .and_then(|rule| rule.publish("settings.changed"))
                    .and_then(|rule| rule.subscribe("devices.*"))
                    .unwrap(),
            )
            .with_rule(
                Rule::new("app.*")
                    .and_then(|rule| rule.call("settings.get"))
                    .and_then(|rule| rule.call("notify.*"))
                    .and_then(|rule| rule.subscribe("settings.changed"))
                    .unwrap(),
            )
    }

    #[test]
    fn default_is_deny() {
        let policy = Policy::deny_all();
        let who = id("system.root");
        assert!(!policy.may_register(&who, "settings"));
        assert!(!policy.may_call(&who, "settings", "get"));
        assert!(!policy.may_publish(&who, "a.b"));
        assert!(!policy.may_subscribe(&who, &Pattern::any()));
    }

    #[test]
    fn allow_all_allows_everything() {
        let policy = Policy::allow_all();
        let who = id("app.unknown");
        assert!(policy.may_register(&who, "settings"));
        assert!(policy.may_call(&who, "settings", "set"));
        assert!(policy.may_publish(&who, "power.state"));
        assert!(policy.may_subscribe(&who, &Pattern::any()));
    }

    #[test]
    fn rules_grant_only_what_they_list() {
        let policy = sample();
        let daemon = id("system.settingsd");
        let app = id("app.phone");
        assert!(policy.may_register(&daemon, "settings"));
        assert!(!policy.may_register(&daemon, "power"));
        assert!(!policy.may_register(&app, "settings"));
        assert!(policy.may_call(&app, "settings", "get"));
        assert!(!policy.may_call(&app, "settings", "set"));
        assert!(policy.may_call(&app, "notify", "post"));
        assert!(!policy.may_call(&daemon, "settings", "get"));
        assert!(policy.may_publish(&daemon, "settings.changed"));
        assert!(!policy.may_publish(&app, "settings.changed"));
    }

    #[test]
    fn subscriptions_must_be_covered() {
        let policy = sample();
        let daemon = id("system.settingsd");
        let app = id("app.phone");
        let parse = |text: &str| Pattern::parse(text).unwrap();
        assert!(policy.may_subscribe(&daemon, &parse("devices.*")));
        assert!(policy.may_subscribe(&daemon, &parse("devices.capabilities")));
        assert!(!policy.may_subscribe(&daemon, &parse("*")));
        assert!(policy.may_subscribe(&app, &parse("settings.changed")));
        assert!(!policy.may_subscribe(&app, &parse("settings.*")));
        assert!(!policy.may_subscribe(&id("other"), &parse("settings.changed")));
    }

    #[test]
    fn invalid_patterns_are_rejected_by_the_builder() {
        assert!(Rule::new("Bad Identity").is_err());
        assert!(Rule::new("a").and_then(|rule| rule.call("**")).is_err());
        assert!(Rule::new("a").and_then(|rule| rule.register("")).is_err());
        assert!(Rule::new("a").and_then(|rule| rule.publish("A")).is_err());
        assert!(Rule::new("a")
            .and_then(|rule| rule.subscribe("a..b"))
            .is_err());
    }

    #[test]
    fn load_reports_missing_files() {
        let missing = std::env::temp_dir().join("ultar-bus-no-such-policy.toml");
        let err = Policy::load(&missing).unwrap_err();
        assert_eq!(err.kind, PolicyErrorKind::Io(std::io::ErrorKind::NotFound));
    }

    #[test]
    fn load_reads_a_file() {
        let path =
            std::env::temp_dir().join(format!("ultar-bus-policy-unit-{}.toml", std::process::id()));
        std::fs::write(&path, "[[rule]]\nidentity = \"*\"\ncall = [\"*\"]\n").unwrap();
        let policy = Policy::load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(policy.may_call(&id("anyone"), "a", "b"));
        assert!(!policy.may_publish(&id("anyone"), "a.b"));
    }
}
