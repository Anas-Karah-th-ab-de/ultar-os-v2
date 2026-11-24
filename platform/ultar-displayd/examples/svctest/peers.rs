//! Stand-ins for the authentication service (`auth.*`, accounts in memory)
//! and the system dialog (`sysdialog.authenticate`: "no" unless the run
//! allowed the next question) – the real ones belong to the shell work.

use super::setup::Run;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::auth::{
    Outcome, SetOutcome, SetSecret, SetSecretReply, UserInfo, Users, UsersReply, Verify,
    VerifyReply,
};
use ultar_bus::messages::auth_admin::{ManageOutcome, ManageReply, RemoveUser, UpdateUser};
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, AuthenticateReply};
use ultar_bus::{Call, IncomingRequest};

/// The running stand-ins.
pub struct Peers {
    asked: Arc<AtomicU32>,
    allow: Arc<AtomicU32>,
}

type Store = Arc<Mutex<BTreeMap<String, (String, bool)>>>;

fn auth(store: &Store, r: IncomingRequest) {
    let Ok(mut s) = store.lock() else { return };
    let _ = match r.method() {
        m if m == Users::METHOD => r.respond::<Users>(|_| {
            let users = s
                .iter()
                .map(|(n, (_, a))| UserInfo {
                    name: n.clone(),
                    display_name: n.clone(),
                    has_pin: false,
                    admin: *a,
                })
                .collect();
            Ok(UsersReply { users })
        }),
        m if m == Verify::METHOD => r.respond::<Verify>(|v| {
            let ok = s.get(&v.user).is_some_and(|(p, _)| p == v.secret.expose());
            let outcome = if ok {
                Outcome::Granted
            } else {
                Outcome::Denied {
                    retry_after_ms: 1000,
                }
            };
            Ok(VerifyReply { outcome })
        }),
        m if m == SetSecret::METHOD => r.respond::<SetSecret>(|v| {
            let admin = s.get(&v.user).is_some_and(|e| e.1);
            s.insert(v.user.clone(), (v.new.expose().to_owned(), admin));
            Ok(SetSecretReply {
                outcome: SetOutcome::Done,
            })
        }),
        m if m == UpdateUser::METHOD => r.respond::<UpdateUser>(|u| {
            let outcome = match (s.get_mut(&u.user), u.admin) {
                (Some(e), Some(a)) => {
                    e.1 = a;
                    ManageOutcome::Done
                }
                (Some(_), None) => ManageOutcome::Done,
                (None, _) => ManageOutcome::Unknown,
            };
            Ok(ManageReply { outcome })
        }),
        m if m == RemoveUser::METHOD => r.respond::<RemoveUser>(|u| {
            let outcome = if s.remove(&u.user).is_some() {
                ManageOutcome::Done
            } else {
                ManageOutcome::Unknown
            };
            Ok(ManageReply { outcome })
        }),
        _ => r.fail(ultar_bus::ErrorCode::UnknownMethod),
    };
}

impl Peers {
    /// Connects both stand-ins and serves them on their own threads.
    pub fn start(run: &Run) -> Result<Peers, String> {
        let store: Store = Arc::default();
        let client = run.client("system.authd")?;
        let endpoint = client.register("auth").map_err(|e| format!("auth {e}"))?;
        std::thread::spawn(move || {
            let _keep = client;
            while let Ok(r) = endpoint.recv() {
                auth(&store, r);
            }
        });
        let (asked, allow) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
        let dialog = run.client("system.sysdialog")?;
        let endpoint = dialog
            .register("sysdialog")
            .map_err(|e| format!("sysdialog {e}"))?;
        let (a, l) = (asked.clone(), allow.clone());
        std::thread::spawn(move || {
            let _keep = dialog;
            while let Ok(r) = endpoint.recv() {
                a.fetch_add(1, Ordering::SeqCst);
                let yes = l.swap(0, Ordering::SeqCst) > 0;
                let result = if yes {
                    AuthResult::Authorized
                } else {
                    AuthResult::Cancelled
                };
                let _ = r.respond::<Authenticate>(|_| Ok(AuthenticateReply { result }));
            }
        });
        std::thread::sleep(Duration::from_millis(200));
        Ok(Peers { asked, allow })
    }

    /// Questions asked so far.
    pub fn asked(&self) -> u32 {
        self.asked.load(Ordering::SeqCst)
    }

    /// The next question is answered "an administrator confirmed".
    pub fn allow_next(&self) {
        self.allow.store(1, Ordering::SeqCst);
    }
}
