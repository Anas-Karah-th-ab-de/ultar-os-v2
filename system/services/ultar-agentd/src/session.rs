//! One request from the first word to the answer: the brain decides, the tools act, and
//! what was done is written down step by step.

use crate::minimal;
use crate::model::{self, Move, Thinker};
use crate::tools::{self, Call, Toolbox};
use ultar_bus::messages::agent::{Answer, Brain, Step, StepOutcome};
use ultar_bus::{Text, TextArg};

/// Tools one request may use in a row.
pub const MAX_STEPS: usize = 6;

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

/// The smallest help: one fixed command, or the list of what it understands.
fn smallest(request: &str, toolbox: &dyn Toolbox) -> (Text, Vec<Step>) {
    let Some(call) = minimal::understand(request) else {
        // A question about this system is answered from the help topics.
        let topic = crate::help::topic_of(request).and_then(crate::help::key);
        let key = topic.as_deref().unwrap_or("agent.minimal.help");
        return (says(key), Vec::new());
    };
    let (outcome, told) = toolbox.run(&call);
    let step = Step {
        tool: call.tool.clone(),
        outcome,
    };
    // A tool that only looks has something to tell.
    let reads = tools::find(&call.tool).is_some_and(|t| t.risk == tools::Risk::Reads);
    let text = if reads && outcome == StepOutcome::Done {
        says("agent.says").with(TextArg::Verbatim(told))
    } else {
        says(outcome_key(outcome))
    };
    (text, vec![step])
}

fn use_tool(call: &Call, toolbox: &dyn Toolbox, steps: &mut Vec<Step>) -> String {
    let (outcome, told) = match tools::find(&call.tool) {
        Some(_) => toolbox.run(call),
        None => (StepOutcome::NotAllowed, "no such tool".to_owned()),
    };
    steps.push(Step {
        tool: call.tool.clone(),
        outcome,
    });
    format!("\n{} -> {:?}: {}", call.tool, outcome, told)
}

/// Lets a model work on the request; `None` when the model did not keep to the form
/// (the caller falls back to the smallest help).
fn with_model(
    request: &str,
    thinker: &dyn Thinker,
    toolbox: &dyn Toolbox,
) -> Option<(Text, Vec<Step>)> {
    let system = model::system_prompt();
    let mut prompt = format!("The person asks: {request}");
    let mut steps = Vec::new();
    for _ in 0..MAX_STEPS {
        match model::read(&thinker.think(&system, &prompt)?)? {
            Move::Say(text) => {
                let text = says("agent.says").with(TextArg::Verbatim(text));
                return Some((text, steps));
            }
            Move::Use(call) => prompt.push_str(&use_tool(&call, toolbox, &mut steps)),
        }
    }
    Some((says("agent.too_many_steps"), steps))
}

/// Answers one request with `brain`.
pub fn answer(request: &str, brain: Brain, thinker: &dyn Thinker, toolbox: &dyn Toolbox) -> Answer {
    let thought = match brain {
        Brain::Minimal => None,
        Brain::Local | Brain::Mesh(_) | Brain::Cloud => with_model(request, thinker, toolbox),
    };
    match thought {
        Some((text, steps)) => Answer { text, brain, steps },
        None => {
            // No model, or it failed: the smallest help, and the answer says so.
            let (text, steps) = smallest(request, toolbox);
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

    fn script(lines: &[&'static str]) -> Script {
        Script(RefCell::new(lines.to_vec()))
    }

    #[test]
    fn without_a_model_a_fixed_command_works_and_anything_else_gets_the_help_text() {
        let notes = Notes::default();
        let a = answer("mute", Brain::Minimal, &script(&[]), &notes);
        assert_eq!((a.text.key.as_str(), a.steps.len()), ("agent.done", 1));
        assert_eq!(notes.0.borrow()[0], Call::new("mute", &[("on", "true")]));
        let b = answer("tell me a story", Brain::Minimal, &script(&[]), &notes);
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
        let a = answer("go online", Brain::Local, &model, &notes);
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
        let a = answer("wipe it", Brain::Local, &model, &notes);
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
        );
        assert_eq!(
            a.brain,
            Brain::Minimal,
            "the answer says where it really thought"
        );
        assert_eq!(a.text.key, "agent.done");
        let gone = answer("mute", Brain::Local, &script(&[]), &notes);
        assert_eq!(gone.brain, Brain::Minimal);
    }

    #[test]
    fn a_model_that_never_stops_is_stopped() {
        let notes = Notes::default();
        let model = script(&["{\"tool\":\"devices\"}"; 20]);
        let a = answer("loop", Brain::Local, &model, &notes);
        assert_eq!(a.steps.len(), MAX_STEPS);
        assert_eq!(a.text.key, "agent.too_many_steps");
    }
}
