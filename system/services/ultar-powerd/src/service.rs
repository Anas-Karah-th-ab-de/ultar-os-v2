//! The bus side: calls, the session topic, the regular battery reading.

use crate::state;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::init::{Power as InitPower, PowerAction};
use ultar_bus::messages::lock::Lock;
use ultar_bus::messages::power::{
    ActionOutcome, ActionReply, PowerActions, PowerOff, PowerState, Restart, Suspend, Torch,
    TorchReply, SERVICE,
};
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate};
use ultar_bus::{Call, Client, ErrorCode, IncomingRequest, RawEvent, Text, Topic};
use ultar_hal::{HalError, Power};
use ultar_hal_linux::LinuxPower;
use ultar_service::log::{Level, Record};
use ultar_svckit::serve::Done;
use ultar_svckit::{Access, Bus, Service};

/// How often the battery is read.
pub const TICK: Duration = Duration::from_secs(30);
/// Longest wait for the lock screen.
const LOCK_WAIT: Duration = Duration::from_secs(5);
/// Longest wait for an administrator in the system dialog.
const DIALOG_WAIT: Duration = Duration::from_secs(90);
/// Longest wait for the service manager.
const INIT_WAIT: Duration = Duration::from_secs(5);

/// The power service.
pub struct PowerService {
    hal: LinuxPower,
    access: Access,
    signed_in: usize,
    state: Option<PowerState>,
    actions: PowerActions,
    /// An action runs. Shared with the worker: it is cleared there *before* the caller
    /// gets its answer, so a call sent right after an answer never finds the service busy
    /// (clearing it later on the loop raced with the next call).
    busy: Arc<AtomicBool>,
    /// The torch and its last state (the Samsung control cannot be read back).
    torch: (Option<ultar_hal_system::torch::TorchNode>, bool),
}

/// Which action of the menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Suspend,
    Restart,
    Off,
}

impl Action {
    fn word(self) -> &'static str {
        match self {
            Action::Suspend => "suspend",
            Action::Restart => "restart",
            Action::Off => "power_off",
        }
    }
}

fn outcome_word(outcome: ActionOutcome) -> &'static str {
    match outcome {
        ActionOutcome::Done => "done",
        ActionOutcome::Unsupported => "unsupported",
        ActionOutcome::Cancelled => "cancelled",
        ActionOutcome::Refused => "refused",
        ActionOutcome::Busy => "busy",
        ActionOutcome::Failed => "failed",
    }
}

fn answer(request: IncomingRequest, outcome: ActionOutcome) {
    let _ = request.reply(&ActionReply { outcome });
}

impl PowerService {
    /// A service reading the kernel below `hal`'s root, with `access` deciding callers.
    pub fn new(hal: LinuxPower, access: Access) -> Self {
        let actions = PowerActions {
            suspend: hal.suspend_supported(),
            restart: true,
            power_off: true,
            needs_admin: false,
        };
        let torch = (ultar_hal_system::torch::find(hal.root()), false);
        Self {
            torch,
            hal,
            access,
            signed_in: 0,
            state: None,
            actions,
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    fn refresh(&mut self, bus: &Bus<'_, Self>) {
        let now = state::read(&self.hal);
        if self.state != Some(now) {
            bus.retain(&now);
            self.state = Some(now);
        }
    }

    fn publish_actions(&mut self, bus: &Bus<'_, Self>) {
        let actions = PowerActions {
            suspend: self.hal.suspend_supported(),
            needs_admin: self.signed_in > 1,
            ..self.actions
        };
        self.actions = actions;
        bus.retain(&actions);
    }

    fn finish(&mut self, bus: &Bus<'_, Self>, action: Action, outcome: ActionOutcome) {
        bus.log(
            Record::new(Level::Info, "powerd.action")
                .word("op", action.word())
                .word("outcome", outcome_word(outcome)),
        );
    }

    /// `power.torch`: system components only (shell, lock screen).
    fn torch(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let Ok(t) = request.decode::<Torch>() else {
            let _ = request.fail(ErrorCode::BadPayload);
            return;
        };
        let caller = request.caller().to_owned();
        if !self.access.is_system(&caller) && !self.access.is_admin(&caller) {
            bus.log(Record::new(Level::Warn, "powerd.refused").word("op", "torch"));
            let _ = request.fail(ErrorCode::PermissionDenied);
            return;
        }
        let (node, last) = &mut self.torch;
        let Some(node) = node.as_ref() else {
            let _ = request.reply(&TorchReply::default());
            return;
        };
        if let Some(on) = t.on {
            if ultar_hal_system::torch::set(node, on).is_ok() {
                *last = on;
            } else {
                bus.log(Record::new(Level::Warn, "powerd.torch_failed"));
            }
        }
        let on = ultar_hal_system::torch::is_on(node).unwrap_or(*last);
        let _ = request.reply(&TorchReply { present: true, on });
    }

    fn act(&mut self, request: IncomingRequest, action: Action, bus: &Bus<'_, Self>) {
        let caller = request.caller().to_owned();
        let admin = self.access.is_admin(&caller);
        if !admin && !self.access.is_system(&caller) {
            bus.log(Record::new(Level::Warn, "powerd.refused").word("op", action.word()));
            return answer(request, ActionOutcome::Refused);
        }
        if action == Action::Suspend && !self.hal.suspend_supported() {
            return answer(request, ActionOutcome::Unsupported);
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            return answer(request, ActionOutcome::Busy);
        }
        let busy = self.busy.clone();
        let hal = self.hal.clone();
        let lock_first = self.signed_in > 0;
        let ask_admin = self.signed_in > 1 && !admin;
        let started = bus.later(move |client| {
            let outcome = match action {
                Action::Suspend => suspend(client, &hal, lock_first),
                Action::Restart => end_machine(client, &caller, PowerAction::Reboot, ask_admin),
                Action::Off => end_machine(client, &caller, PowerAction::PowerOff, ask_admin),
            };
            // Answered here, not on the loop: after init accepted a power off, this
            // service is told to stop at once and its loop may not run again. Free
            // before answering: the caller may ask again as soon as it has the answer.
            busy.store(false, Ordering::SeqCst);
            answer(request, outcome);
            let done: Done<Self> =
                Box::new(move |s: &mut Self, bus: &Bus<'_, Self>| s.finish(bus, action, outcome));
            done
        });
        if !started {
            self.busy.store(false, Ordering::SeqCst);
        }
    }
}

fn suspend(client: &Client, hal: &LinuxPower, lock_first: bool) -> ActionOutcome {
    if lock_first {
        // Nobody may find an unlocked screen after waking up: no lock, no sleep.
        match client.call(&Lock {}, LOCK_WAIT) {
            Ok(reply) if reply.locked => {}
            _ => return ActionOutcome::Failed,
        }
    }
    match hal.suspend() {
        Ok(()) => ActionOutcome::Done,
        Err(HalError::NotSupported) => ActionOutcome::Unsupported,
        Err(_) => ActionOutcome::Failed,
    }
}

fn end_machine(
    client: &Client,
    caller: &str,
    action: PowerAction,
    ask_admin: bool,
) -> ActionOutcome {
    if ask_admin {
        let key = match action {
            PowerAction::Reboot => "powerd.action.restart",
            PowerAction::PowerOff => "powerd.action.power_off",
        };
        let ask = Authenticate {
            app_id: caller.to_owned(),
            app_name: caller.to_owned(),
            action: Text::key(key),
        };
        match client.call(&ask, DIALOG_WAIT).map(|r| r.result) {
            Ok(AuthResult::Authorized) => {}
            Ok(AuthResult::Cancelled) => return ActionOutcome::Cancelled,
            Ok(AuthResult::Unavailable) | Err(_) => return ActionOutcome::Failed,
        }
    }
    match client.call(&InitPower { action }, INIT_WAIT) {
        Ok(_) => ActionOutcome::Done,
        Err(_) => ActionOutcome::Failed,
    }
}

impl Service for PowerService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![SessionState::TOPIC]
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        self.refresh(bus);
        self.publish_actions(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        // The three calls carry no fields; a payload that is not theirs is refused.
        if request.method() == Torch::METHOD {
            return self.torch(request, bus);
        }
        let (action, valid) = match request.method() {
            m if m == Suspend::METHOD => (Action::Suspend, request.decode::<Suspend>().is_ok()),
            m if m == Restart::METHOD => (Action::Restart, request.decode::<Restart>().is_ok()),
            m if m == PowerOff::METHOD => (Action::Off, request.decode::<PowerOff>().is_ok()),
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
                return;
            }
        };
        if !valid {
            let _ = request.fail(ErrorCode::BadPayload);
            return;
        }
        self.act(request, action, bus);
    }

    fn event(&mut self, event: RawEvent, bus: &Bus<'_, Self>) {
        if let Ok(state) = event.decode::<SessionState>() {
            let count = state.signed_in.len();
            if count != self.signed_in {
                self.signed_in = count;
                self.publish_actions(bus);
            }
        }
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.refresh(bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_distinct() {
        let words: Vec<_> = [Action::Suspend, Action::Restart, Action::Off]
            .iter()
            .map(|a| a.word())
            .collect();
        assert_eq!(words, ["suspend", "restart", "power_off"]);
        let outcomes = [
            ActionOutcome::Done,
            ActionOutcome::Unsupported,
            ActionOutcome::Cancelled,
            ActionOutcome::Refused,
            ActionOutcome::Busy,
            ActionOutcome::Failed,
        ];
        let mut seen: Vec<_> = outcomes.iter().map(|o| outcome_word(*o)).collect();
        seen.dedup();
        assert_eq!(seen.len(), 6);
    }

    #[test]
    fn catalogues_are_complete() {
        let keys = ["powerd.action.restart", "powerd.action.power_off"];
        for text in [
            include_str!("../locales/ar.ftl"),
            include_str!("../locales/en.ftl"),
            include_str!("../locales/de.ftl"),
        ] {
            assert!(ultar_svckit::msg::catalogue_gaps(&keys, text).is_empty());
        }
    }
}
