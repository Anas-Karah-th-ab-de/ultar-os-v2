//! The helper's own surface (boards HelperSheet, HelperConfirm): what the person asked,
//! each step the helper took for it as it happens, and what it answered. Kept while the
//! shell runs; the lasting list is the helper's own (Settings › Assistant).

use ultar_bus::messages::agent::{Answer, Brain, Progress, Step};
use ultar_bus::Text;

/// Most requests kept.
pub const MAX_TURNS: usize = 20;
/// Longest request in characters (the helper takes 4096 bytes).
pub const MAX_ASK: usize = 1000;

/// One request and what came of it so far.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Turn {
    /// What the person asked (data).
    pub asked: String,
    /// The steps so far, in order.
    pub steps: Vec<Step>,
    /// The tool that waits for the person's confirmation in the system dialog.
    pub waiting: Option<String>,
    /// The answer (`None`: the helper is still at it).
    pub said: Option<Text>,
    /// Where the helper thought for it.
    pub brain: Option<Brain>,
}

/// The conversation and the text being typed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Helper {
    /// The requests, oldest first.
    pub turns: Vec<Turn>,
    /// The text in the field.
    pub query: String,
    /// Where the helper thinks right now (`agent.state`; `None`: no helper).
    pub brain: Option<Brain>,
}

impl Helper {
    /// The helper is at a request.
    pub fn busy(&self) -> bool {
        self.turns.last().is_some_and(|t| t.said.is_none())
    }

    fn pending(&mut self, asked: &str) -> Option<&mut Turn> {
        let last = self.turns.last_mut()?;
        (last.said.is_none() && last.asked == asked).then_some(last)
    }

    /// Notes a new request; `None` while another one runs or when there is nothing to ask.
    pub fn ask(&mut self, text: &str) -> Option<String> {
        let asked: String = text.trim().chars().take(MAX_ASK).collect();
        if asked.is_empty() || self.busy() {
            return None;
        }
        self.turns.push(Turn {
            asked: asked.clone(),
            ..Turn::default()
        });
        let over = self.turns.len().saturating_sub(MAX_TURNS);
        self.turns.drain(..over);
        self.query.clear();
        Some(asked)
    }

    /// The request being answered moved on; `false` when none is running.
    pub fn progress(&mut self, progress: Progress) -> bool {
        let Some(turn) = self.turns.last_mut().filter(|t| t.said.is_none()) else {
            return false;
        };
        match progress {
            Progress::Step(step) => {
                turn.waiting = None;
                turn.steps.push(step);
            }
            Progress::Waiting(tool) => turn.waiting = Some(tool),
        }
        true
    }

    /// The answer to `asked`; `false` when nobody waits for it here (asked elsewhere, or
    /// the person stopped waiting).
    pub fn answer(&mut self, asked: &str, answer: Answer) -> bool {
        let Some(turn) = self.pending(asked) else {
            return false;
        };
        turn.steps = answer.steps;
        turn.waiting = None;
        turn.brain = Some(answer.brain);
        turn.said = Some(answer.text);
        true
    }

    /// The helper did not answer `asked`; `false` when nobody waits for it here.
    pub fn failed(&mut self, asked: &str) -> bool {
        let Some(turn) = self.pending(asked) else {
            return false;
        };
        turn.waiting = None;
        turn.said = Some(Text::key("desk.helper.failed"));
        true
    }

    /// The person stops waiting: what was done stays done and stays listed.
    pub fn stop(&mut self) -> bool {
        let Some(turn) = self.turns.last_mut().filter(|t| t.said.is_none()) else {
            return false;
        };
        turn.waiting = None;
        turn.said = Some(Text::key("desk.helper.stopped"));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::agent::{Area, StepOutcome};

    #[test]
    fn a_turn_fills_step_by_step_and_takes_only_its_own_answer() {
        let mut h = Helper::default();
        assert_eq!(h.ask("  "), None);
        assert_eq!(h.ask(" restart "), Some("restart".to_owned()));
        assert!(h.busy() && h.ask("more").is_none(), "one at a time");
        assert!(h.progress(Progress::Waiting("restart".into())));
        assert_eq!(h.turns[0].waiting.as_deref(), Some("restart"));
        let step = Step::new("restart", StepOutcome::NotConfirmed, Area::Power);
        assert!(h.progress(Progress::Step(step.clone())));
        assert_eq!((h.turns[0].steps.len(), &h.turns[0].waiting), (1, &None));
        assert!(!h.answer("something else", Answer::default()));
        let answer = Answer {
            steps: vec![step],
            ..Answer::default()
        };
        assert!(h.answer("restart", answer));
        assert!(!h.busy() && !h.progress(Progress::Waiting("x".into())));
        // stopping: the late answer is not taken
        h.ask("again");
        assert!(h.stop() && !h.stop());
        assert!(!h.answer("again", Answer::default()));
        assert_eq!(h.turns[1].said.as_ref().unwrap().key, "desk.helper.stopped");
        for i in 0..30 {
            h.ask(&format!("q{i}"));
            h.failed(&format!("q{i}"));
        }
        assert_eq!(h.turns.len(), MAX_TURNS);
    }
}
