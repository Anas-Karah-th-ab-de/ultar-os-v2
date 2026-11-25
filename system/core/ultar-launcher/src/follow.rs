//! The launcher follows the settings and the session on a bus connection of its own: it
//! reads `apps.default.*` and `apps.autostart` at start, takes `settings.changed` at runtime
//! ([`crate::defaults`]), and starts the autostart apps once after a sign-in (`session.state`).
//! Without the bus the launcher still starts apps on request; only the defaults stay at the
//! schema's and no autostart happens.

use crate::defaults::AUTOSTART;
use crate::launcher::Launcher;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::launcher::{Kind, Role};
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::settings::{Changed, Get};
use ultar_bus::{BusError, Client, TypedSubscription};
use ultar_service::{BusSession, Lifecycle, SessionOptions, State};

/// Pause before the bus is tried again.
const RETRY: Duration = Duration::from_secs(1);
/// How often the topics are looked at while nothing arrives.
const POLL: Duration = Duration::from_millis(200);

/// Reads the followed settings once into the launcher (the schema's defaults stay for any
/// key the settings service does not answer).
fn read_start(client: &Client, launcher: &Launcher) {
    for kind in Kind::ALL {
        let key = kind.key();
        if let Ok(reply) = client.call(&Get { key: key.clone() }, RETRY) {
            if let Some(value) = reply.value {
                launcher.follow_setting(&key, &value);
            }
        }
    }
    let key = AUTOSTART.to_owned();
    if let Ok(reply) = client.call(&Get { key: key.clone() }, RETRY) {
        if let Some(value) = reply.value {
            launcher.follow_setting(&key, &value);
        }
    }
}

/// Starts the autostart apps a sign-in makes due (each once; the launcher refuses any that
/// is not installed, so only apps the person may start run).
fn sign_in(launcher: &Arc<Launcher>, state: &SessionState) {
    for app in launcher.autostart_due(state) {
        let _ = launcher.start(&app, Role::Ui, None);
    }
}

/// Drains a subscription into `take`; `false` when the connection ended (reconnect).
fn drain<T: ultar_bus::Topic>(sub: &Option<TypedSubscription<T>>, mut take: impl FnMut(T)) -> bool {
    let Some(sub) = sub else {
        return true;
    };
    loop {
        match sub.try_recv() {
            Ok(Some(m)) => take(m),
            Ok(None) => return true,
            Err(BusError::Closed) => return false,
            Err(_) => return true,
        }
    }
}

/// Keeps the launcher following the settings and the session at `socket` (again whenever
/// the broker comes back) until `lifecycle` stops.
pub fn serve(socket: PathBuf, launcher: Arc<Launcher>, lifecycle: Lifecycle) {
    while !lifecycle.is_stopping() {
        let mut options = SessionOptions::named("ultar-launcher");
        options.connect_wait = RETRY;
        options.stop = Some(lifecycle.clone());
        let Ok(session) = BusSession::connect(&socket, options) else {
            continue;
        };
        let client = session.client().clone();
        read_start(&client, &launcher);
        let changed = client.subscribe::<Changed>().ok();
        let sessions = client.subscribe::<SessionState>().ok();
        while !lifecycle.is_stopping() {
            let alive = drain(&changed, |c| launcher.follow_setting(&c.key, &c.value));
            let alive = drain(&sessions, |s| sign_in(&launcher, &s)) && alive;
            if !alive {
                break;
            }
            lifecycle.wait_for_timeout(State::Stopping, POLL);
        }
        session.close();
    }
}
