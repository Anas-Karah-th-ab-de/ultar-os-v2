//! One request from the first word to the answer: the brain decides, the tools act, and
//! what was done is written down step by step.

use crate::minimal;
use crate::model::{self, Move, Thinker};
use crate::tools::{self, Call, Limits, Toolbox};
use ultar_bus::messages::agent::{Answer, Brain, Progress, Step, StepOutcome};
use ultar_bus::{Text, TextArg};

/// Tools one request may use in a row.
pub const MAX_STEPS: usize = 6;
/// Longest note of what a step was used on, in bytes.
const MAX_DETAIL: usize = 96;

fn says(key: &str) -> Text {
    Text::key(key)
}

fn outcome_key(outcome: StepOutcome) -> &'static str {
    match outcome {
        StepOutcome::Done => "agent.done",
        StepOutcome::Failed => "agent.failed",
        StepOutcome::NotConfirmed => "agent.not_confirmed",
        StepOutcome::NotAllowed => "agent.not_allowed",
    }
}

/// What a call was used on, in short: its one argument, or `name=value` pairs.
fn detail(call: &Call) -> String {
    let mut text = match call.args.len() {
        1 => call.args.values().next().cloned().unwrap_or_default(),
        _ => {
            let pairs: Vec<String> = call.args.iter().map(|(k, v)| format!("{k}={v}")).collect();
            pairs.join(", ")
        }
    };
    if text.len() > MAX_DETAIL {
        let cut = (0..=MAX_DETAIL).rev().find(|i| text.is_char_boundary(*i));
        text.truncate(cut.unwrap_or(0));
    }
    text
}

/// Uses one tool – unless it does not exist or the person closed it – and
/// writes the step down.
fn run(
    call: &Call,
    toolbox: &dyn Toolbox,
    limits: &Limits,
    steps: &mut Vec<Step>,
) -> tools::Outcome {
    let tool = tools::find(&call.tool);
    let (outcome, told) = match tool {
        None => (StepOutcome::NotAllowed, "no such tool".to_owned()),
        Some(tool) => match limits.closes(tool) {
            Some(why) => (StepOutcome::NotAllowed, why.to_owned()),
            None => toolbox.run(call),
        },
    };
    let step = Step {
        tool: call.tool.clone(),
        outcome,
        area: tool.map(|t| t.area).unwrap_or_default(),
        detail: detail(call),
    };
    toolbox.note(&Progress::Step(step.clone()));
    steps.push(step);
    (outcome, told)
}

/// The smallest help: one fixed command, or the list of what it understands.
fn smallest(request: &str, toolbox: &dyn Toolbox, limits: &Limits) -> (Text, Vec<Step>) {
    let Some(call) = minimal::understand(request) else {
        // A question about this system is answered from the help topics.
        let topic = crate::help::topic_of(request).and_then(crate::help::key);
        let key = topic.as_deref().unwrap_or("agent.minimal.help");
        return (says(key), Vec::new());
    };
    let mut steps = Vec::new();
    let (outcome, told) = run(&call, toolbox, limits, &mut steps);
    // A tool that only looks has something to tell.
    let reads = tools::find(&call.tool).is_some_and(|t| t.risk == tools::Risk::Reads);
    let text = if reads && outcome == StepOutcome::Done {
        says("agent.says").with(TextArg::Verbatim(told))
    } else if limits.read_only && !reads {
        // The person can change that: say where.
        says("agent.act_off")
    } else {
        says(outcome_key(outcome))
    };
    (text, steps)
}

/// The model's words as the answer; with `assistant.check_links` a suspicious link in them
/// adds the warning of the catalogue (the words stay verbatim, the warning is translated).
fn checked(words: String, limits: &Limits) -> Text {
    if limits.check_links && crate::links::first_suspicious(&words).is_some() {
        return says("agent.says_checked")
            .with(TextArg::Verbatim(words))
            .with(TextArg::Key("agent.link_warning".to_owned()));
    }
    says("agent.says").with(TextArg::Verbatim(words))
}

/// Lets a model work on the request; `None` when the model did not keep to the form
/// (the caller falls back to the smallest help).
fn with_model(
    request: &str,
    thinker: &dyn Thinker,
    toolbox: &dyn Toolbox,
    limits: &Limits,
) -> Option<(Text, Vec<Step>)> {
    let system = model::system_prompt(limits);
    let mut prompt = format!("The person asks: {request}");
    let mut steps = Vec::new();
    for _ in 0..MAX_STEPS {
        match model::read(&thinker.think(&system, &prompt)?)? {
            Move::Say(text) => {
                return Some((checked(text, limits), steps));
            }
            Move::Use(call) => {
                let (outcome, told) = run(&call, toolbox, limits, &mut steps);
                prompt.push_str(&format!("\n{} -> {:?}: {}", call.tool, outcome, told));
            }
        }
    }
    Some((says("agent.too_many_steps"), steps))
}

/// Answers one request with `brain`; what `limits` closes stays closed.
pub fn answer(
    request: &str,
    brain: Brain,
    thinker: &dyn Thinker,
    toolbox: &dyn Toolbox,
    limits: &Limits,
) -> Answer {
    let thought = match brain {
        Brain::Minimal => None,
        Brain::Local | Brain::Mesh(_) | Brain::Cloud => {
            with_model(request, thinker, toolbox, limits)
        }
    };
    match thought {
        Some((text, steps)) => Answer { text, brain, steps },
        None => {
            // No model, or it failed: the smallest help, and the answer says so.
            let (text, steps) = smallest(request, toolbox, limits);
            Answer {
                text,
                brain: Brain::Minimal,
                steps,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use ultar_bus::messages::agent::Area;

    /// A model that answers from a script; a toolbox that notes what it was asked.
    struct Script(RefCell<Vec<&'static str>>);
    impl Thinker for Script {
        fn think(&self, _: &str, _: &str) -> Option<String> {
            let mut lines = self.0.borrow_mut();
            (!lines.is_empty()).then(|| lines.remove(0).to_owned())
        }
    }
    #[derive(Default)]
    struct Notes(RefCell<Vec<Call>>);
    impl Toolbox for Notes {
        fn run(&self, call: &Call) -> tools::Outcome {
            self.0.borrow_mut().push(call.clone());
            (StepOutcome::Done, "ok".to_owned())
        }
    }

    fn open() -> Limits {
        Limits::default()
    }

    fn script(lines: &[&'static str]) -> Script {
        Script(RefCell::new(lines.to_vec()))
    }

    #[test]
    fn without_a_model_a_fixed_command_works_and_anything_else_gets_the_help_text() {
        let notes = Notes::default();
        let a = answer("mute", Brain::Minimal, &script(&[]), &notes, &open());
        assert_eq!((a.text.key.as_str(), a.steps.len()), ("agent.done", 1));
        assert_eq!(notes.0.borrow()[0], Call::new("mute", &[("on", "true")]));
        let b = answer(
            "tell me a story",
            Brain::Minimal,
            &script(&[]),
            &notes,
            &open(),
        );
        assert_eq!(
            (b.text.key.as_str(), b.steps.len()),
            ("agent.minimal.help", 0)
        );
    }

    #[test]
    fn a_model_uses_tools_step_by_step_and_then_answers() {
        let notes = Notes::default();
        let model = script(&[
            "{\"tool\":\"wifi\",\"args\":{\"on\":true}}",
            "{\"tool\":\"open_app\",\"args\":{\"app\":\"org.ultar.browser\"}}",
            "{\"say\":\"Wi-Fi is on and the browser is open.\"}",
        ]);
        let a = answer("go online", Brain::Local, &model, &notes, &open());
        assert_eq!(a.brain, Brain::Local);
        assert_eq!(a.steps.len(), 2);
        assert_eq!(a.text.key, "agent.says");
        assert_eq!(notes.0.borrow().len(), 2);
    }

    #[test]
    fn a_tool_the_model_invents_is_never_run() {
        let notes = Notes::default();
        let model = script(&[
            "{\"tool\":\"format_disk\"}",
            "{\"say\":\"I cannot do that.\"}",
        ]);
        let a = answer("wipe it", Brain::Local, &model, &notes, &open());
        assert_eq!(a.steps[0].outcome, StepOutcome::NotAllowed);
        assert!(notes.0.borrow().is_empty());
    }

    #[test]
    fn a_model_that_fails_leaves_the_smallest_help() {
        let notes = Notes::default();
        let a = answer(
            "mute",
            Brain::Mesh("desk".into()),
            &script(&["nonsense"]),
            &notes,
            &open(),
        );
        assert_eq!(
            a.brain,
            Brain::Minimal,
            "the answer says where it really thought"
        );
        assert_eq!(a.text.key, "agent.done");
        let gone = answer("mute", Brain::Local, &script(&[]), &notes, &open());
        assert_eq!(gone.brain, Brain::Minimal);
    }

    #[test]
    fn an_area_the_person_switched_off_is_closed_with_and_without_a_model() {
        let notes = Notes::default();
        let off = Limits {
            off: vec![Area::Sound],
            read_only: false,
            ..Limits::default()
        };
        let fixed = answer("mute", Brain::Minimal, &script(&[]), &notes, &off);
        assert_eq!(fixed.text.key, "agent.not_allowed");
        let model = script(&[
            "{\"tool\":\"set_volume\",\"args\":{\"percent\":30}}",
            "{\"tool\":\"wifi\",\"args\":{\"on\":true}}",
            "{\"say\":\"ok\"}",
        ]);
        let a = answer("quiet and online", Brain::Local, &model, &notes, &off);
        let seen: Vec<_> = a.steps.iter().map(|s| (s.area, s.outcome)).collect();
        assert_eq!(
            seen,
            [
                (Area::Sound, StepOutcome::NotAllowed),
                (Area::Network, StepOutcome::Done)
            ]
        );
        assert_eq!(a.steps[0].detail, "30");
        assert_eq!(notes.0.borrow().len(), 1, "only the allowed tool ran");
        assert!(!model::system_prompt(&off).contains("set_volume"));
        assert!(model::system_prompt(&open()).contains("setting_set"));
        // "only look": nothing is changed, looking still works
        let look = Limits {
            off: Vec::new(),
            read_only: true,
            ..Limits::default()
        };
        let refused = answer("mute", Brain::Minimal, &script(&[]), &notes, &look);
        assert_eq!(refused.text.key, "agent.act_off");
        assert!(!model::system_prompt(&look).contains("- wifi("));
        assert!(model::system_prompt(&look).contains("- network_status("));
    }

    #[test]
    fn suspicious_links_are_warned_about_only_when_checking_is_on() {
        const LINK: &str = "{\"say\":\"open http://203.0.113.7/login to continue\"}";
        const GOOD: &str = "{\"say\":\"open https://example.com to continue\"}";
        let notes = Notes::default();
        let on = Limits {
            check_links: true,
            ..Limits::default()
        };
        let a = answer("what next?", Brain::Local, &script(&[LINK]), &notes, &on);
        assert_eq!(a.text.key, "agent.says_checked");
        assert_eq!(a.text.args.len(), 2);
        assert!(matches!(&a.text.args[0], TextArg::Verbatim(t) if t.contains("203.0.113.7")));
        assert!(matches!(&a.text.args[1], TextArg::Key(k) if k == "agent.link_warning"));
        // Off (the person switched it): the words as they are.
        let off = Limits {
            check_links: false,
            ..Limits::default()
        };
        let b = answer("what next?", Brain::Local, &script(&[LINK]), &notes, &off);
        assert_eq!(b.text.key, "agent.says");
        // A harmless link is not warned about.
        let c = answer("what next?", Brain::Local, &script(&[GOOD]), &notes, &on);
        assert_eq!(c.text.key, "agent.says");
    }

    #[test]
    fn seeing_the_screen_is_closed_until_the_person_allowed_it() {
        const SEE: &str = "{\"tool\":\"see_screen\"}";
        const SAY: &str = "{\"say\":\"ok\"}";
        let notes = Notes::default();
        let a = answer(
            "what is on my screen?",
            Brain::Local,
            &script(&[SEE, SAY]),
            &notes,
            &Limits::default(),
        );
        assert_eq!(a.steps[0].outcome, StepOutcome::NotAllowed);
        assert!(notes.0.borrow().is_empty());
        let on = Limits {
            see_screen: true,
            ..Limits::default()
        };
        let b = answer(
            "what is on my screen?",
            Brain::Local,
            &script(&[SEE, SAY]),
            &notes,
            &on,
        );
        assert_eq!(b.steps[0].outcome, StepOutcome::Done);
        assert_eq!(notes.0.borrow()[0].tool, "see_screen");
    }

    #[test]
    fn a_model_that_never_stops_is_stopped() {
        let notes = Notes::default();
        let model = script(&["{\"tool\":\"devices\"}"; 20]);
        let a = answer("loop", Brain::Local, &model, &notes, &open());
        assert_eq!(a.steps.len(), MAX_STEPS);
        assert_eq!(a.text.key, "agent.too_many_steps");
    }
}
