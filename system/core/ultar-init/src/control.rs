//! The bus service `init` (`ultar_bus::messages::init`): status and
//! control of the services, for administrators.
//!
//! A thread keeps init on the bus (the broker is one of init's own
//! services: it comes, goes and comes back) and hands every request to the
//! main loop through a channel, waking it with a byte on a pipe. The main
//! loop answers: all state lives there, no lock is shared.
//!
//! Access: the broker's policy decides who may call `init.status` and
//! `init.control`; init itself additionally requires the pseudo method
//! `init.admin` in the same policy (read at start and on `SIGHUP`), so a
//! policy that lets an app call `init.status` does not make it an
//! administrator.

use crate::daemon::Halt;
use crate::supervisor::{ControlError, Supervisor};
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};
use ultar_bus::messages::init::{
    Control, ControlReply, Power, PowerAction, PowerReply, Status, StatusReply, Switch,
    ADMIN_METHOD, ERROR_ALREADY, ERROR_SHUTTING_DOWN, ERROR_TOO_SOON, ERROR_UNKNOWN_SERVICE,
    MAX_SERVICES, SERVICE, SHUTDOWN_METHOD, SWITCH_METHOD,
};
use ultar_bus::{Client, ErrorCode, Identity, IncomingRequest, Policy};
use ultar_service::{BusSession, Lifecycle, SessionOptions, StopCause};

/// Pause before the bus is tried again.
const RETRY: Duration = Duration::from_secs(1);

/// The bus side of init, owned by the main loop.
pub struct BusLink {
    requests: Receiver<IncomingRequest>,
    wake: PipeReader,
    lifecycle: Lifecycle,
    client: Arc<Mutex<Option<Client>>>,
    thread: Option<thread::JoinHandle<()>>,
}

fn set_client(slot: &Mutex<Option<Client>>, client: Option<Client>) {
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = client;
}

fn serve_bus(
    socket: &Path,
    lifecycle: &Lifecycle,
    slot: &Mutex<Option<Client>>,
    requests: &Sender<IncomingRequest>,
    wake: &Mutex<PipeWriter>,
) {
    while !lifecycle.is_stopping() {
        let mut options = SessionOptions::named("ultar-init");
        options.connect_wait = RETRY;
        options.stop = Some(lifecycle.clone());
        let Ok(session) = BusSession::connect(socket, options) else {
            continue;
        };
        set_client(slot, Some(session.client().clone()));
        if lifecycle.is_stopping() {
            session.close();
        }
        if let Ok(endpoint) = session.register(SERVICE) {
            endpoint.run(|request| {
                if requests.send(request).is_ok() {
                    let mut wake = wake.lock().unwrap_or_else(PoisonError::into_inner);
                    let _ = wake.write_all(b"r");
                }
            });
        } else {
            lifecycle.wait_for_timeout(ultar_service::State::Stopping, RETRY);
        }
        set_client(slot, None);
    }
}

impl BusLink {
    /// Joins the bus at `socket` (and again whenever the connection ends).
    pub fn start(socket: PathBuf) -> std::io::Result<Self> {
        let (wake, waker) = std::io::pipe()?;
        let (sender, requests) = channel();
        let lifecycle = Lifecycle::new();
        let client = Arc::new(Mutex::new(None));
        let (life, slot) = (lifecycle.clone(), client.clone());
        let waker = Mutex::new(waker);
        let thread = thread::Builder::new()
            .name("ultar-init-bus".into())
            .spawn(move || serve_bus(&socket, &life, &slot, &sender, &waker))?;
        Ok(Self {
            requests,
            wake,
            lifecycle,
            client,
            thread: Some(thread),
        })
    }

    /// The descriptor that becomes readable when requests wait.
    pub fn wake_fd(&self) -> &PipeReader {
        &self.wake
    }

    /// Takes the waiting requests (the wake pipe was readable).
    pub fn take(&mut self) -> Vec<IncomingRequest> {
        let mut bytes = [0u8; 64];
        let _ = self.wake.read(&mut bytes);
        self.requests.try_iter().take(MAX_SERVICES).collect()
    }

    /// Leaves the bus and ends the thread.
    pub fn stop(mut self) {
        self.lifecycle.request_stop(StopCause::Request);
        if let Some(client) = self
            .client
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            client.close();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Who is an administrator of init.
pub struct Admins {
    policy: Option<Policy>,
}

impl Admins {
    /// Administrators by `policy`; `None`: nobody (fail closed).
    pub fn new(policy: Option<Policy>) -> Self {
        Self { policy }
    }

    /// Reads the policy file; a faulty file means nobody.
    pub fn load(path: &Path) -> Self {
        Self::new(Policy::load(path).ok())
    }

    /// Whether `identity` may end the machine (`init.power`): it needs the pseudo method
    /// [`SHUTDOWN_METHOD`] as well (the power service; administrators with `init.*`), so
    /// a rule that only opens the call does not let anyone power off.
    pub fn allows_power(&self, identity: &str) -> bool {
        let Some(policy) = &self.policy else {
            return false;
        };
        Identity::new(identity).is_ok_and(|id| policy.may_call(&id, SERVICE, SHUTDOWN_METHOD))
    }

    /// Whether `identity` has `init.admin`.
    pub fn allows(&self, identity: &str) -> bool {
        let Some(policy) = &self.policy else {
            return false;
        };
        Identity::new(identity).is_ok_and(|id| policy.may_call(&id, SERVICE, ADMIN_METHOD))
    }
}

fn control_code(error: ControlError) -> ErrorCode {
    ErrorCode::Service(match error {
        ControlError::Unknown => ERROR_UNKNOWN_SERVICE,
        ControlError::Already => ERROR_ALREADY,
        ControlError::ShuttingDown => ERROR_SHUTTING_DOWN,
        ControlError::TooSoon => ERROR_TOO_SOON,
    })
}

/// Answers `init.power`: the halt the main loop should begin (`pid1`: power off or
/// restart the machine; otherwise init only ends).
fn answer_power(
    request: IncomingRequest,
    supervisor: &mut Supervisor,
    admins: &Admins,
    pid1: bool,
) -> Option<Halt> {
    let caller = request.caller().to_owned();
    if !admins.allows_power(&caller) {
        supervisor.note(&format!("refused request caller={caller} reason=not_power"));
        let _ = request.fail(ErrorCode::PermissionDenied);
        return None;
    }
    let mut halt = None;
    let _ = request.respond::<Power>(|power| {
        halt = Some(match (pid1, power.action) {
            (false, _) => Halt::Exit,
            (true, PowerAction::PowerOff) => Halt::PowerOff,
            (true, PowerAction::Reboot) => Halt::Reboot,
        });
        let word = match power.action {
            PowerAction::PowerOff => "poweroff",
            PowerAction::Reboot => "reboot",
        };
        supervisor.note(&format!("power request caller={caller} action={word}"));
        Ok(PowerReply {})
    });
    halt
}

/// Answers one request; `Some` when it asked to end the machine.
pub fn answer(
    request: IncomingRequest,
    supervisor: &mut Supervisor,
    admins: &Admins,
    now: Instant,
    pid1: bool,
) -> Option<Halt> {
    if request.method() == "power" {
        return answer_power(request, supervisor, admins, pid1);
    }
    if request.method() == SWITCH_METHOD {
        // No administrator right, and none is gained: the service's own file names the
        // one identity that may switch it (checked here against the caller the broker
        // vouches for, on top of the bus policy).
        let caller = request.caller().to_owned();
        let mut refused = None;
        let _ = request.respond::<Switch>(|switch| {
            let done = supervisor.switch(&switch.service, switch.on, &caller, now);
            if done.is_err() {
                refused = Some(switch.service.clone());
            }
            done.map(|state| ControlReply { state })
                .map_err(control_code)
        });
        if let Some(service) = refused {
            supervisor.note(&format!("refused switch caller={caller} service={service}"));
        }
        return None;
    }
    let caller = request.caller().to_owned();
    if !admins.allows(&caller) {
        supervisor.note(&format!("refused request caller={caller} reason=not_admin"));
        let _ = request.fail(ErrorCode::PermissionDenied);
        return None;
    }
    let _ = match request.method() {
        "status" => request.respond::<Status>(|_| {
            let mut services = supervisor.status();
            services.truncate(MAX_SERVICES);
            Ok(StatusReply { services })
        }),
        "control" => request.respond::<Control>(|control| {
            supervisor
                .control(control.action, &control.service, now)
                .map(|state| ControlReply { state })
                .map_err(control_code)
        }),
        _ => request.fail(ErrorCode::UnknownMethod),
    };
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_admin_pseudo_method_makes_an_administrator() {
        let policy = Policy::from_toml_str(
            r#"
[[rule]]
identity = "system.admin"
call = ["init.status", "init.control", "init.admin"]
[[rule]]
identity = "app.viewer"
call = ["init.status", "init.control"]
[[rule]]
identity = "system.powerd"
call = ["init.power", "init.shutdown"]
"#,
        )
        .unwrap();
        let admins = Admins::new(Some(policy));
        assert!(admins.allows("system.admin"));
        assert!(admins.allows_power("system.powerd") && !admins.allows("system.powerd"));
        assert!(!admins.allows("app.viewer"));
        assert!(!admins.allows("Not A Name"));
        assert!(!Admins::new(None).allows("system.admin"));
        assert!(!admins.allows_power("app.viewer") && !Admins::new(None).allows_power("x.y"));
        assert!(!Admins::load(Path::new("/nonexistent/policy.toml")).allows("system.admin"));
        assert_eq!(control_code(ControlError::Unknown), ErrorCode::Service(1));
    }
}
