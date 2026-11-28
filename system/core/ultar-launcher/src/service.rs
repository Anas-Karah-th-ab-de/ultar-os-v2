//! The bus service `launcher` (`ultar_bus::messages::launcher`).
//!
//! The broker's policy decides who may call which method (default deny);
//! the shell gets `start`, `stop`, `list`, `permissions`, `grant`,
//! `revoke`. `install` and `uninstall` additionally need the pseudo method
//! `launcher.admin` in the same policy file (read at start and on
//! `SIGHUP`), so a policy line that opens them by mistake – or a pattern
//! like `launcher.*` for the shell – does not make the caller an installer.

use crate::error::LaunchError;
use crate::launcher::{lock, Launcher};
use crate::open::Opening;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::launcher::{
    Candidates, CandidatesReply, Grant, Install, List, ListReply, Permissions, Resolve,
    ResolveReply, Revoke, Start, StartReplyV1, StartV1, Stop, Uninstall, UninstallReply,
    ADMIN_METHOD, SERVICE,
};
use ultar_bus::Message;
use ultar_bus::{Client, ErrorCode, Identity, IncomingRequest, Policy};
use ultar_service::{BusSession, Lifecycle, SessionOptions};

/// Pause before the bus is tried again.
const RETRY: Duration = Duration::from_secs(1);
/// Threads that answer `start` (a start waits up to `START_DEADLINE` for the
/// cell's handshake).
const START_THREADS: usize = 4;
/// Starts that may wait for a free start thread.
const START_QUEUE: usize = 16;

/// Answers `launcher.start` on threads of their own, so a slow cell start
/// never holds up `stop`, `list` or `revoke` on the bus thread (review
/// R2-5). The threads live as long as the launcher: stage 1 of a cell ends
/// with the thread that started it (`PR_SET_PDEATHSIG`), so a start never
/// runs on a short-lived thread.
pub struct StartPool {
    jobs: Option<std::sync::mpsc::SyncSender<IncomingRequest>>,
}

impl StartPool {
    /// `threads` start threads for `launcher` (none: every start is answered
    /// where it arrives).
    pub fn new(launcher: &Arc<Launcher>, threads: usize) -> Self {
        let (jobs, queue) = std::sync::mpsc::sync_channel::<IncomingRequest>(START_QUEUE);
        let queue = Arc::new(Mutex::new(queue));
        let mut started = 0;
        for n in 0..threads {
            let (launcher, queue) = (Arc::clone(launcher), Arc::clone(&queue));
            let spawned = std::thread::Builder::new()
                .name(format!("ultar-launcher-start-{n}"))
                .spawn(move || loop {
                    let next = lock(&queue).recv();
                    let Ok(request) = next else { return };
                    let _ = start(request, &launcher);
                });
            started += usize::from(spawned.is_ok());
        }
        Self {
            jobs: (started > 0).then_some(jobs),
        }
    }

    /// Hands `request` to a start thread; `Some(request)` back when none can
    /// take it now (the caller answers it itself).
    pub fn submit(&self, request: IncomingRequest) -> Option<IncomingRequest> {
        let Some(jobs) = &self.jobs else {
            return Some(request);
        };
        match jobs.try_send(request) {
            Ok(()) => None,
            Err(
                std::sync::mpsc::TrySendError::Full(r)
                | std::sync::mpsc::TrySendError::Disconnected(r),
            ) => Some(r),
        }
    }
}

/// Who may install and uninstall.
#[derive(Default)]
pub struct Admins {
    policy: Option<Policy>,
}

impl Admins {
    /// Installers by `policy`; `None`: nobody (fail closed).
    pub fn new(policy: Option<Policy>) -> Self {
        Self { policy }
    }

    /// Reads the policy file; a missing or faulty file means nobody.
    pub fn load(path: Option<&Path>) -> Self {
        Self::new(path.and_then(|p| Policy::load(p).ok()))
    }

    /// Whether `identity` holds `launcher.admin`.
    pub fn allows(&self, identity: &str) -> bool {
        let Some(policy) = &self.policy else {
            return false;
        };
        Identity::new(identity).is_ok_and(|id| policy.may_call(&id, SERVICE, ADMIN_METHOD))
    }
}

fn code(error: LaunchError) -> ErrorCode {
    ErrorCode::Service(error.code())
}

/// `launcher.start` in its version (version 1: no URI, no `opened`).
fn start(request: IncomingRequest, launcher: &Arc<Launcher>) -> Result<(), ultar_bus::BusError> {
    if request.version() == StartV1::VERSION {
        let Ok(old) = request.decode_raw::<StartV1>() else {
            return request.fail(ErrorCode::BadPayload);
        };
        return match launcher.start(&old.app, old.role, old.activation_token) {
            Ok(r) => request.reply(&StartReplyV1 {
                state: r.state,
                pid: r.pid,
                already: r.already,
            }),
            Err(error) => request.fail(code(error)),
        };
    }
    request.respond::<Start>(|s| {
        let opening = Opening {
            token: s.activation_token,
            uri: s.uri,
            new_window: s.new_window,
        };
        launcher
            .start_opening(&s.app, s.role, &opening)
            .map_err(code)
    })
}

/// Answers one request.
pub fn answer(request: IncomingRequest, launcher: &Arc<Launcher>, admins: &Mutex<Admins>) {
    let admin_call = matches!(request.method(), "install" | "uninstall");
    if admin_call && !lock(admins).allows(request.caller()) {
        launcher.refused(request.caller());
        let _ = request.fail(ErrorCode::PermissionDenied);
        return;
    }
    let _ = match request.method() {
        "list" => request.respond::<List>(|_| {
            Ok(ListReply {
                apps: launcher.list(),
            })
        }),
        "resolve" => request.respond::<Resolve>(|r| {
            Ok(ResolveReply {
                app: launcher.resolve(&r.uri),
            })
        }),
        "candidates" => request.respond::<Candidates>(|c| {
            Ok(CandidatesReply {
                apps: launcher.candidates(c.kind),
            })
        }),
        "start" => start(request, launcher),
        "stop" => request.respond::<Stop>(|s| launcher.stop(&s.app, s.role).map_err(code)),
        "permissions" => {
            request.respond::<Permissions>(|p| launcher.permissions(&p.app).map_err(code))
        }
        "grant" => request.respond::<Grant>(|g| {
            launcher
                .set_permission(&g.app, &g.permission, true)
                .map_err(code)
        }),
        "revoke" => request.respond::<Revoke>(|r| {
            launcher
                .set_permission(&r.app, &r.permission, false)
                .map_err(code)
        }),
        "install" => request.respond::<Install>(|i| launcher.install(&i.package).map_err(code)),
        "uninstall" => request.respond::<Uninstall>(|u| {
            launcher
                .uninstall(&u.app, u.keep_data)
                .map(|stopped| UninstallReply { stopped })
                .map_err(code)
        }),
        _ => request.fail(ErrorCode::UnknownMethod),
    };
}

/// Keeps the launcher on the bus at `socket` (again whenever the broker
/// comes back) until `lifecycle` stops: the stop closes the connection,
/// which ends the wait for requests. Requests are answered one at a time
/// on this thread, except `start`, which goes to the [`StartPool`].
pub fn serve(
    socket: PathBuf,
    launcher: Arc<Launcher>,
    admins: Arc<Mutex<Admins>>,
    lifecycle: Lifecycle,
) {
    let starts = StartPool::new(&launcher, START_THREADS);
    let slot: Arc<Mutex<Option<Client>>> = Arc::default();
    let closer = Arc::clone(&slot);
    lifecycle.on_stop(move || {
        if let Some(client) = lock(&closer).take() {
            client.close();
        }
    });
    while !lifecycle.is_stopping() {
        let mut options = SessionOptions::named("ultar-launcher");
        options.connect_wait = RETRY;
        options.stop = Some(lifecycle.clone());
        let Ok(session) = BusSession::connect(&socket, options) else {
            continue;
        };
        *lock(&slot) = Some(session.client().clone());
        if lifecycle.is_stopping() {
            session.close();
        }
        match session.register(SERVICE) {
            Ok(endpoint) => endpoint.run(|request| {
                let request = if request.method() == "start" {
                    match starts.submit(request) {
                        Some(back) => back,
                        None => return,
                    }
                } else {
                    request
                };
                answer(request, &launcher, &admins)
            }),
            Err(_) => {
                lifecycle.wait_for_timeout(ultar_service::State::Stopping, RETRY);
            }
        }
        *lock(&slot) = None;
        session.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_admin_pseudo_method_makes_an_installer() {
        let policy = Policy::from_toml_str(
            r#"
[[rule]]
identity = "system.admin"
call = ["launcher.*"]
[[rule]]
identity = "system.shell"
call = ["launcher.start", "launcher.install"]
"#,
        )
        .unwrap();
        let admins = Admins::new(Some(policy));
        assert!(admins.allows("system.admin"));
        assert!(!admins.allows("system.shell"));
        assert!(!admins.allows("Not A Name"));
        assert!(!Admins::default().allows("system.admin"));
        assert!(!Admins::load(Some(Path::new("/nonexistent/policy.toml"))).allows("system.admin"));
        assert!(!Admins::load(None).allows("system.admin"));
        assert_eq!(code(LaunchError::Limit), ErrorCode::Service(6));
    }
}
