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
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::launcher::{
    Grant, Install, List, ListReply, Permissions, Revoke, Start, Stop, Uninstall, UninstallReply,
    ADMIN_METHOD, SERVICE,
};
use ultar_bus::{Client, ErrorCode, Identity, IncomingRequest, Policy};
use ultar_service::{BusSession, Lifecycle, SessionOptions};

/// Pause before the bus is tried again.
const RETRY: Duration = Duration::from_secs(1);

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
        "start" => request.respond::<Start>(|s| {
            launcher
                .start(&s.app, s.role, s.activation_token)
                .map_err(code)
        }),
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
/// on this thread.
pub fn serve(
    socket: PathBuf,
    launcher: Arc<Launcher>,
    admins: Arc<Mutex<Admins>>,
    lifecycle: Lifecycle,
) {
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
            Ok(endpoint) => endpoint.run(|request| answer(request, &launcher, &admins)),
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
