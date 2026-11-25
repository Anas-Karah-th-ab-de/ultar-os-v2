//! Shared fixture: an in-process broker with token identities, stand-ins
//! for the authentication service (`auth.*`, in memory) and the system
//! dialog (`sysdialog.authenticate`, answers scripted by the test), and the
//! real `ultar-accountd` binary as a child process.
#![allow(dead_code)]

use std::collections::{BTreeMap, VecDeque};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_bus::messages::account_local::{ListUsers, LocalUsers};
use ultar_bus::messages::auth::{
    Method, Outcome, SetOutcome, SetSecret, UserInfo, Users, UsersReply, Verify, VerifyReply,
};
use ultar_bus::messages::auth_admin::{ManageOutcome, ManageReply, RemoveUser, UpdateUser};
use ultar_bus::messages::sysdialog::{AuthResult, Authenticate, AuthenticateReply};
use ultar_bus::{Broker, Call, Client, ClientOptions, Identity, Policy, TokenIdentity};

pub const DAEMON: &str = env!("CARGO_BIN_EXE_ultar-accountd");
pub const WAIT: Duration = Duration::from_secs(20);

pub const POLICY: &str = r#"
[[rule]]
identity = "system.accountd"
register = ["account"]
publish  = ["account.users", "account.state"]
call     = ["auth.users", "auth.verify", "auth.set_secret", "auth.update_user", "auth.remove_user", "auth.admin", "sysdialog.authenticate"]

[[rule]]
identity = "system.authd"
register = ["auth"]

[[rule]]
identity = "system.sysdialog"
register = ["sysdialog"]

[[rule]]
identity  = "app.settings"
call      = ["account.status", "account.sign_in", "account.set_service", "account.users_list", "account.user_create", "account.user_rename", "account.user_picture", "account.user_password", "account.user_kind", "account.user_auto_login", "account.user_remove", "account.user_picture_get"]
subscribe = ["account.users", "account.state"]

[[rule]]
identity = "system.lock"
call     = ["account.session", "account.users_list"]

[[rule]]
identity = "system.installer"
call     = ["account.setup", "account.user_create", "account.users_list"]

[[rule]]
identity = "app.evil"
call     = ["account.users_list"]
"#;

const IDENTITIES: [&str; 7] = [
    "system.accountd",
    "system.authd",
    "system.sysdialog",
    "app.settings",
    "system.lock",
    "system.installer",
    "app.evil",
];

/// An account in the stand-in credential store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cred {
    pub password: String,
    pub display: String,
    pub admin: bool,
}

/// How the stand-in dialog answers the next question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    /// Never answers (the caller waits).
    Hang,
}

#[derive(Default)]
pub struct Shared {
    pub creds: BTreeMap<String, Cred>,
    pub answers: VecDeque<Answer>,
    pub asked: Vec<String>,
}

pub struct World {
    // First: the broker needs its socket file to shut down.
    broker: Option<Broker>,
    pub socket: PathBuf,
    pub dir: PathBuf,
    pub shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    held: Arc<Mutex<Vec<ultar_bus::IncomingRequest>>>,
}

pub struct Daemon(pub Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        // The broker needs its socket file to shut down: first the broker.
        drop(self.broker.take());
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn me() -> u32 {
    ultar_service::effective_uid()
}

pub fn connect(socket: &Path, identity: &str) -> Client {
    let options = ClientOptions {
        token: Some(identity.to_owned()),
        ..ClientOptions::named(identity)
    };
    Client::connect(socket, options).unwrap()
}

fn auth_answer(shared: &Mutex<Shared>, r: ultar_bus::IncomingRequest) {
    let mut s = shared.lock().unwrap();
    let _ = match r.method() {
        m if m == Users::METHOD => r.respond::<Users>(|_| {
            let users = s
                .creds
                .iter()
                .map(|(name, c)| UserInfo {
                    name: name.clone(),
                    display_name: c.display.clone(),
                    has_pin: false,
                    admin: c.admin,
                })
                .collect();
            Ok(UsersReply { users })
        }),
        m if m == Verify::METHOD => r.respond::<Verify>(|v| {
            let ok = v.method == Method::Password
                && s.creds
                    .get(&v.user)
                    .is_some_and(|c| c.password == v.secret.expose());
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
            let first = s.creds.is_empty();
            let entry = s.creds.entry(v.user.clone()).or_insert(Cred {
                password: String::new(),
                display: v.display_name.clone(),
                admin: first,
            });
            entry.password = v.new.expose().to_owned();
            Ok(ultar_bus::messages::auth::SetSecretReply {
                outcome: SetOutcome::Done,
            })
        }),
        _ => return manage_answer(&mut s, r),
    };
}

fn manage_answer(s: &mut Shared, r: ultar_bus::IncomingRequest) {
    let _ = match r.method() {
        m if m == UpdateUser::METHOD => r.respond::<UpdateUser>(|u| {
            let outcome = match s.creds.get_mut(&u.user) {
                Some(c) => {
                    if let Some(d) = u.display_name {
                        c.display = d;
                    }
                    if let Some(a) = u.admin {
                        c.admin = a;
                    }
                    ManageOutcome::Done
                }
                None => ManageOutcome::Unknown,
            };
            Ok(ManageReply { outcome })
        }),
        m if m == RemoveUser::METHOD => r.respond::<RemoveUser>(|u| {
            let outcome = match s.creds.remove(&u.user) {
                Some(_) => ManageOutcome::Done,
                None => ManageOutcome::Unknown,
            };
            Ok(ManageReply { outcome })
        }),
        _ => r.fail(ultar_bus::ErrorCode::UnknownMethod),
    };
}

fn serve(
    socket: PathBuf,
    identity: &'static str,
    service: &'static str,
    stop: Arc<AtomicBool>,
    mut handle: impl FnMut(ultar_bus::IncomingRequest) + Send + 'static,
) {
    let client = connect(&socket, identity);
    let endpoint = client.register(service).unwrap();
    std::thread::spawn(move || {
        let _keep = client;
        while !stop.load(Ordering::SeqCst) {
            if let Ok(r) = endpoint.recv_timeout(Duration::from_millis(50)) {
                handle(r);
            }
        }
    });
}

/// The stand-in system dialog: answers as the test scripted, or holds the
/// question (never answers).
fn serve_dialog(
    socket: &Path,
    stop: &Arc<AtomicBool>,
    s: Arc<Mutex<Shared>>,
    h: Arc<Mutex<Vec<ultar_bus::IncomingRequest>>>,
) {
    serve(
        socket.to_path_buf(),
        "system.sysdialog",
        "sysdialog",
        stop.clone(),
        move |r| {
            let Ok(ask) = r.decode::<Authenticate>() else {
                return;
            };
            let answer = {
                let mut s = s.lock().unwrap();
                s.asked.push(ask.action.key.clone());
                s.answers.pop_front().unwrap_or(Answer::No)
            };
            let result = match answer {
                Answer::Yes => AuthResult::Authorized,
                Answer::No => AuthResult::Cancelled,
                Answer::Hang => {
                    h.lock().unwrap().push(r);
                    return;
                }
            };
            let _ = r.reply(&AuthenticateReply { result });
        },
    );
}

impl World {
    pub fn new(tag: &str) -> World {
        let dir = std::env::temp_dir().join(format!("ultar-accd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("state")).unwrap();
        std::fs::create_dir_all(dir.join("home")).unwrap();
        for d in [&dir, &dir.join("state")] {
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let socket = dir.join("bus.sock");
        let mut auth = TokenIdentity::new();
        for name in IDENTITIES {
            auth = auth.with_token(name, Identity::new(name).unwrap());
        }
        let broker = Broker::builder(Policy::from_toml_str(POLICY).unwrap())
            .listen(&socket, Arc::new(auth))
            .start()
            .unwrap();
        std::fs::write(dir.join("policy.toml"), POLICY).unwrap();
        std::fs::write(
            dir.join("ids.toml"),
            "[users]\nroot = 0\nbusd = 801\naccountd = 818\n",
        )
        .unwrap();
        let token = dir.join("token");
        std::fs::write(&token, "system.accountd").unwrap();
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let held = Arc::new(Mutex::new(Vec::new()));
        let s = shared.clone();
        serve(
            socket.clone(),
            "system.authd",
            "auth",
            stop.clone(),
            move |r| auth_answer(&s, r),
        );
        serve_dialog(&socket, &stop, shared.clone(), held.clone());
        World {
            broker: Some(broker),
            socket,
            dir,
            shared,
            stop,
            held,
        }
    }

    /// The id range for people: as root 1000–1009; as another user it
    /// starts at that user's own id (a new home can only be handed to it).
    pub fn uids(&self) -> (u32, u32) {
        if me() == 0 {
            (1000, 1009)
        } else {
            (me(), me() + 9)
        }
    }

    pub fn start(&self) -> Daemon {
        let (a, b) = self.uids();
        let mut cmd = Command::new(DAEMON);
        cmd.arg("--bus")
            .arg(&self.socket)
            .arg("--dir")
            .arg(self.dir.join("state"))
            .arg("--homes")
            .arg(self.dir.join("home"))
            .arg("--ids")
            .arg(self.dir.join("ids.toml"))
            .arg("--policy")
            .arg(self.dir.join("policy.toml"))
            .arg("--uids")
            .arg(format!("{a}-{b}"))
            .arg("--token-file")
            .arg(self.dir.join("token"));
        if me() == 0 {
            cmd.arg("--allow-root");
        }
        let mut daemon = Daemon(cmd.spawn().unwrap());
        let probe = connect(&self.socket, "app.settings");
        let start = Instant::now();
        while probe
            .call(&ListUsers {}, Duration::from_millis(500))
            .is_err()
        {
            assert!(start.elapsed() < WAIT, "accountd did not come up");
            // A daemon that gave up at once (seen on the CI runner while several tests
            // start theirs together) is started again, as the service manager would;
            // how it ended is printed, so the cause shows in the log.
            if let Ok(Some(status)) = daemon.0.try_wait() {
                eprintln!("accountd ended before it was ready ({status}); starting it again");
                daemon = Daemon(cmd.spawn().unwrap());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        daemon
    }

    pub fn answer(&self, a: Answer) {
        self.shared.lock().unwrap().answers.push_back(a);
    }

    pub fn asked(&self) -> Vec<String> {
        std::mem::take(&mut self.shared.lock().unwrap().asked)
    }

    pub fn cred(&self, name: &str) -> Option<Cred> {
        self.shared.lock().unwrap().creds.get(name).cloned()
    }

    pub fn seed(&self, name: &str, password: &str, admin: bool) {
        self.shared.lock().unwrap().creds.insert(
            name.into(),
            Cred {
                password: password.into(),
                display: name.into(),
                admin,
            },
        );
    }

    pub fn home(&self, name: &str) -> PathBuf {
        self.dir.join("home").join(name)
    }
}

pub fn users(client: &Client) -> LocalUsers {
    client.call(&ListUsers {}, WAIT).unwrap().outcome.unwrap()
}
