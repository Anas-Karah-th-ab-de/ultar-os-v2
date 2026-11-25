//! Command line and the service loop. One thread owns the accounts; the
//! bus endpoint feeds it; an administrator question (the system dialog,
//! up to a minute) runs on its own thread so that listing and the Ultar
//! account keep answering meanwhile – one question at a time.

use crate::core::{Accounts, Config, Op, Ports};
use crate::link;
use crate::names::{parse_ids, SystemIds};
use crate::ports::{BusCredentials, BusDialog, Credentials, Dialog, FsHomes};

use crate::refusal::{refuse, Key};
use crate::throttle::Limiter;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::time::Instant;
use ultar_bus::messages::account_local::{
    CreateUser, GetPicture, ListUsers, LocalUsers, Picture, PictureInfo, PictureReply, RemoveUser,
    RenameUser, SetAutoLogin, SetKind, SetPassword, SetPicture, SetSession, UserKind, UsersReply,
};
use ultar_bus::messages::account_service::{Refusal, SERVICE};
use ultar_bus::messages::sysdialog::AuthResult;
use ultar_bus::{BusError, Call, ErrorCode, Identity, IncomingRequest, Policy, Text, TextArg};
use ultar_service::log::{Level, Logger, Record};
use ultar_service::{BusSession, Lifecycle, Secret, SessionOptions};

/// Environment variable with the bus socket.
pub const SOCKET_ENV: &str = "ULTAR_BUS_SOCKET";
/// Pseudo method whose call right lets an identity create the first
/// administrator without a dialog (installer, first start).
pub const SETUP_METHOD: &str = "setup";

/// Command line of `ultar-accountd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// Bus socket.
    pub bus: PathBuf,
    /// The service's own directory.
    pub dir: PathBuf,
    /// Directory of the home folders.
    pub homes: PathBuf,
    /// `ids.toml` of the system.
    pub ids: PathBuf,
    /// Bus policy (who may set up the first account).
    pub policy: PathBuf,
    /// First and last id for people.
    pub uids: (u32, u32),
    /// Log service socket.
    pub log_socket: Option<PathBuf>,
    /// Bus token file.
    pub token_file: Option<PathBuf>,
    /// Start even as root (development).
    pub allow_root: bool,
    /// Host of the Ultar account server (default [`link::DEFAULT_SERVER`]).
    pub account_server: String,
    /// Where the mesh service reads the relay token (none: no relay).
    pub relay_token: Option<PathBuf>,
    /// The group the relay token is readable by (shared only with the mesh service).
    pub relay_group: Option<u32>,
}

/// Why the command line was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageError;

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid options")
    }
}

impl std::error::Error for UsageError {}

#[derive(Default)]
struct Raw {
    bus: Option<PathBuf>,
    dir: Option<PathBuf>,
    homes: Option<PathBuf>,
    ids: Option<PathBuf>,
    policy: Option<PathBuf>,
    log_socket: Option<PathBuf>,
    token_file: Option<PathBuf>,
    uids: (u32, u32),
    allow_root: bool,
    account_server: Option<String>,
    relay_token: Option<PathBuf>,
    relay_group: Option<u32>,
}

/// Reads the command line (`None`: help).
pub fn parse(
    words: impl IntoIterator<Item = String>,
    env_socket: Option<String>,
) -> Result<Option<Args>, UsageError> {
    let mut raw = Raw {
        bus: env_socket.filter(|s| !s.is_empty()).map(PathBuf::from),
        uids: (1000, 19_999),
        ..Raw::default()
    };
    let mut it = words.into_iter();
    while let Some(word) = it.next() {
        let mut value = || it.next().ok_or(UsageError);
        let mut path = || value().map(PathBuf::from);
        match word.as_str() {
            "--help" | "-h" => return Ok(None),
            "--bus" => raw.bus = Some(path()?),
            "--dir" => raw.dir = Some(path()?),
            "--homes" => raw.homes = Some(path()?),
            "--ids" => raw.ids = Some(path()?),
            "--policy" => raw.policy = Some(path()?),
            "--log-socket" => raw.log_socket = Some(path()?),
            "--token-file" => raw.token_file = Some(path()?),
            "--account-server" => raw.account_server = Some(value()?),
            "--relay-token" => raw.relay_token = Some(path()?),
            "--relay-group" => {
                raw.relay_group = Some(value()?.parse().map_err(|_| UsageError)?);
            }
            "--uids" => {
                let v = value()?;
                let (a, b) = v.split_once('-').ok_or(UsageError)?;
                raw.uids = (
                    a.parse().map_err(|_| UsageError)?,
                    b.parse().map_err(|_| UsageError)?,
                );
            }
            "--allow-root" => raw.allow_root = true,
            _ => return Err(UsageError),
        }
    }
    finish(raw).map(Some)
}

fn finish(raw: Raw) -> Result<Args, UsageError> {
    let args = Args {
        bus: raw.bus.ok_or(UsageError)?,
        dir: raw.dir.ok_or(UsageError)?,
        homes: raw.homes.ok_or(UsageError)?,
        ids: raw.ids.ok_or(UsageError)?,
        policy: raw.policy.ok_or(UsageError)?,
        uids: raw.uids,
        log_socket: raw.log_socket,
        token_file: raw.token_file,
        allow_root: raw.allow_root,
        account_server: raw
            .account_server
            .unwrap_or_else(|| link::DEFAULT_SERVER.to_owned()),
        relay_token: raw.relay_token,
        relay_group: raw.relay_group,
    };
    let paths = [&args.bus, &args.dir, &args.homes, &args.ids, &args.policy];
    let ids_ok = args.uids.0 >= 1000 && args.uids.0 <= args.uids.1 && args.uids.1 < 20_000;
    let abs = paths.iter().all(|p| p.is_absolute())
        && args
            .log_socket
            .iter()
            .chain(&args.token_file)
            .chain(&args.relay_token)
            .all(|p| p.is_absolute())
        && !args.account_server.is_empty();
    (abs && ids_ok).then_some(args).ok_or(UsageError)
}

/// Why the service could not start.
#[derive(Debug)]
pub enum StartError {
    /// `ids.toml` or the policy could not be read.
    Files,
    /// The bus.
    Bus(BusError),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::Files => f.write_str("ids or policy file not readable"),
            StartError::Bus(e) => write!(f, "bus: {e}"),
        }
    }
}

impl std::error::Error for StartError {}

enum Input {
    Request(Box<IncomingRequest>),
    Authorized(Box<IncomingRequest>, Op, AuthResult),
    /// A setting the service follows (`users.auto_login`).
    Setting(String, ultar_bus::messages::settings::SettingValue),
    Stop,
}

fn load_files(args: &Args) -> Result<(SystemIds, Policy), StartError> {
    let text = std::fs::read_to_string(&args.ids).map_err(|_| StartError::Files)?;
    let ids = parse_ids(&text).map_err(|_| StartError::Files)?;
    let policy = Policy::load(&args.policy).map_err(|_| StartError::Files)?;
    Ok((ids, policy))
}

/// The settings follower thread (`users.auto_login`).
fn spawn_settings(session: &BusSession, tx: Sender<Input>, logger: &Logger) {
    let feed = crate::settings::feed(session.client().clone(), move |key, value| {
        tx.send(Input::Setting(key, value)).is_ok()
    });
    if feed.is_err() {
        logger.log(Level::Error, "accountd.settings_failed");
    }
}

/// Hands every call of the endpoint to the loop.
fn feed_requests(
    endpoint: ultar_bus::ServiceEndpoint,
    feed: Sender<Input>,
) -> Result<(), StartError> {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || loop {
            match endpoint.recv() {
                Ok(r) => {
                    if feed.send(Input::Request(Box::new(r))).is_err() {
                        return;
                    }
                }
                Err(BusError::Closed) => return,
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        })
        .map(|_| ())
        .map_err(|e| StartError::Bus(BusError::Io(e)))
}

/// Runs the service until the lifecycle stops it.
pub fn run(
    args: &Args,
    token: Option<Secret>,
    lifecycle: &Lifecycle,
    logger: &Logger,
    ready: &dyn Fn(),
) -> Result<(), StartError> {
    let (system, policy) = load_files(args)?;
    let mut options = SessionOptions::named("ultar-accountd");
    options.token = token;
    options.stop = Some(lifecycle.clone());
    let session = BusSession::connect(&args.bus, options).map_err(StartError::Bus)?;
    let endpoint = session.register(SERVICE).map_err(StartError::Bus)?;
    let (tx, rx) = channel();
    let stop = tx.clone();
    lifecycle.on_stop(move || {
        let _ = stop.send(Input::Stop);
    });
    feed_requests(endpoint, tx.clone())?;
    spawn_settings(&session, tx.clone(), logger);
    let config = Config {
        dir: args.dir.clone(),
        uids: args.uids,
        system,
    };
    let (link_tx, link_rx) = channel::<Box<IncomingRequest>>();
    let service = Service {
        accounts: Accounts::open(config),
        credentials: BusCredentials(session.client().clone()),
        dialog: BusDialog(session.client().clone()),
        homes: FsHomes {
            root: args.homes.clone(),
        },
        policy,
        limiter: Limiter::default(),
        asking: false,
        started: Instant::now(),
        logger,
        session: &session,
        tx,
        link: link_tx,
    };
    if !service.accounts.usable() {
        logger.log(Level::Error, "accountd.store_damaged");
    }
    service.publish();
    serve_with_link(args, service, &rx, link_rx, &session, logger, ready);
    session.close();
    Ok(())
}

/// The link thread and the service loop until stop.
fn serve_with_link(
    args: &Args,
    service: Service<'_>,
    rx: &std::sync::mpsc::Receiver<Input>,
    link_rx: std::sync::mpsc::Receiver<Box<IncomingRequest>>,
    session: &BusSession,
    logger: &Logger,
    ready: &dyn Fn(),
) {
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn_scoped(scope, move || {
                crate::link_daemon::serve_link(args, link_rx, session)
            });
        if worker.is_err() {
            logger.log(Level::Error, "accountd.link_failed");
        }
        ready();
        logger.log(Level::Info, "accountd.ready");
        serve(service, rx);
    });
}

/// The service loop until stop; dropping the service at the end ends the link thread.
fn serve(mut service: Service<'_>, rx: &std::sync::mpsc::Receiver<Input>) {
    while let Ok(input) = rx.recv() {
        match input {
            Input::Stop => break,
            Input::Request(r) => service.request(*r),
            Input::Authorized(r, op, result) => service.authorized(*r, op, result),
            Input::Setting(key, value) => service.setting(&key, &value),
        }
    }
}

struct Service<'a> {
    accounts: Accounts,
    credentials: BusCredentials,
    dialog: BusDialog,
    homes: FsHomes,
    policy: Policy,
    limiter: Limiter,
    asking: bool,
    started: Instant,
    logger: &'a Logger,
    session: &'a BusSession,
    tx: Sender<Input>,
    link: Sender<Box<IncomingRequest>>,
}

fn decode_op(r: &IncomingRequest) -> Option<Result<Op, BusError>> {
    let m = r.method();
    Some(match m {
        _ if m == CreateUser::METHOD => r.decode::<CreateUser>().map(Op::Create),
        _ if m == RenameUser::METHOD => r.decode::<RenameUser>().map(Op::Rename),
        _ if m == SetPicture::METHOD => r.decode::<SetPicture>().map(Op::Picture),
        _ if m == SetPassword::METHOD => r.decode::<SetPassword>().map(Op::Password),
        _ if m == SetKind::METHOD => r.decode::<SetKind>().map(Op::Kind),
        _ if m == SetAutoLogin::METHOD => r.decode::<SetAutoLogin>().map(Op::AutoLogin),
        _ if m == RemoveUser::METHOD => r.decode::<RemoveUser>().map(Op::Remove),
        _ => return None,
    })
}

impl Service<'_> {
    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn view(&self) -> LocalUsers {
        self.accounts.view(self.credentials.available())
    }

    fn publish(&self) {
        let _ = self.session.publish_retained(&self.view());
    }

    fn reply(&self, r: IncomingRequest, outcome: Result<(), Refusal>) {
        let outcome = outcome.map(|()| self.view());
        let changed = outcome.is_ok();
        let _ = r.reply(&UsersReply { outcome });
        if changed {
            self.publish();
        }
    }

    fn request(&mut self, r: IncomingRequest) {
        let method = r.method().to_owned();
        if link::is_link_method(&method) || link::is_device_method(&method) {
            if let Err(e) = self.link.send(Box::new(r)) {
                let _ = e.0.fail(ErrorCode::ServiceBusy);
            }
            return;
        }
        match decode_op(&r) {
            Some(Ok(op)) => return self.change(r, op),
            Some(Err(_)) => {
                let _ = r.fail(ErrorCode::BadPayload);
                return;
            }
            None => {}
        }
        match method.as_str() {
            m if m == ListUsers::METHOD => {
                let _ = r.respond::<ListUsers>(|_| {
                    Ok(UsersReply {
                        outcome: Ok(self.view()),
                    })
                });
            }
            m if m == GetPicture::METHOD => {
                let _ = r.respond::<GetPicture>(|g| {
                    Ok(PictureReply {
                        picture: self.picture(&g.user),
                    })
                });
            }
            m if m == SetSession::METHOD => match r.decode::<SetSession>() {
                Ok(s) => {
                    let outcome = self.accounts.set_session(s.user);
                    self.reply(r, outcome);
                }
                Err(_) => {
                    let _ = r.fail(ErrorCode::BadPayload);
                }
            },
            _ => {
                let _ = r.fail(ErrorCode::UnknownMethod);
            }
        }
    }

    /// A setting arrived: `users.auto_login` marks the user that signs in at start
    /// (settingsd already asked the administrator; no dialog here).
    fn setting(&mut self, key: &str, value: &ultar_bus::messages::settings::SettingValue) {
        if crate::settings::apply(&mut self.accounts, self.logger, key, value) {
            self.publish();
        }
    }

    fn picture(&self, user: &str) -> Picture {
        let view = self.accounts.view(true);
        match view
            .users
            .iter()
            .find(|u| u.name == user)
            .map(|u| u.picture)
        {
            Some(PictureInfo::Builtin(n)) => Picture::Builtin(n),
            Some(PictureInfo::Custom(_)) => crate::store::load_picture(&self.dir(), user)
                .map_or(Picture::Initials, Picture::Image),
            _ => Picture::Initials,
        }
    }

    fn dir(&self) -> PathBuf {
        self.accounts.dir().to_path_buf()
    }

    /// The first administrator of an empty device, made by the set-up identity.
    fn bootstrap(&self, caller: &str, op: &Op) -> bool {
        let first_admin = matches!(op, Op::Create(c) if c.kind == UserKind::Administrator);
        let allowed =
            Identity::new(caller).is_ok_and(|id| self.policy.may_call(&id, SERVICE, SETUP_METHOD));
        self.accounts.empty() && first_admin && allowed
    }

    fn change(&mut self, r: IncomingRequest, op: Op) {
        if !self.limiter.allow(r.caller(), self.now()) {
            return self.reply(r, Err(refuse(Key::Throttled)));
        }
        if let Err(refusal) = self.accounts.precheck(&op) {
            return self.reply(r, Err(refusal));
        }
        if !self.accounts.needs_admin(&op) || self.bootstrap(r.caller(), &op) {
            return self.authorized(r, op, AuthResult::Authorized);
        }
        if self.asking {
            return self.reply(r, Err(refuse(Key::Busy)));
        }
        self.asking = true;
        let action = Text::key(&format!("accountd.action.{}", op.word())).with(TextArg::Verbatim(
            op.target().unwrap_or_default().to_owned(),
        ));
        let (dialog, tx, caller) = (self.dialog.clone(), self.tx.clone(), r.caller().to_owned());
        self.logger
            .emit(Record::new(Level::Info, "accountd.ask_admin").word("op", op.word()));
        let spawned = std::thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || {
                let result = dialog.authenticate(&caller, action);
                let _ = tx.send(Input::Authorized(Box::new(r), op, result));
            });
        if spawned.is_err() {
            self.asking = false;
        }
    }

    fn authorized(&mut self, r: IncomingRequest, op: Op, result: AuthResult) {
        self.asking = false;
        let word = op.word();
        let outcome = match result {
            AuthResult::Authorized => {
                let ports = Ports {
                    credentials: &self.credentials as &dyn Credentials,
                    homes: &self.homes,
                };
                let now = self.now();
                self.accounts.perform(op, &ports, now)
            }
            AuthResult::Cancelled => Err(refuse(Key::NotAuthorized)),
            AuthResult::Unavailable => Err(refuse(Key::DialogUnavailable)),
        };
        let level = if outcome.is_ok() {
            Level::Notice
        } else {
            Level::Info
        };
        let key = if outcome.is_ok() {
            "accountd.changed"
        } else {
            "accountd.refused"
        };
        self.logger.emit(Record::new(level, key).word("op", word));
        for w in self.accounts.take_warnings() {
            self.logger
                .emit(Record::new(Level::Warn, "accountd.warning").word("what", w));
        }
        self.reply(r, outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn command_line() {
        let base = [
            "--dir", "/d", "--homes", "/home", "--ids", "/i", "--policy", "/p",
        ];
        let a = parse(w(&base), Some("/b".into())).unwrap().unwrap();
        assert_eq!(a.uids, (1000, 19_999));
        let mut more = base.to_vec();
        more.extend(["--uids", "2000-2999", "--allow-root"]);
        let a = parse(w(&more), Some("/b".into())).unwrap().unwrap();
        assert_eq!(a.uids, (2000, 2999));
        assert!(a.allow_root);
        assert_eq!(parse(w(&["-h"]), None), Ok(None));
        for bad in [
            &["--uids", "5-1"][..],
            &["--uids", "500-900"],
            &["--uids", "x"],
            &["--zz"],
            &["--dir", "rel"],
        ] {
            let mut v = base.to_vec();
            v.extend_from_slice(bad);
            assert_eq!(parse(w(&v), Some("/b".into())), Err(UsageError), "{bad:?}");
        }
        assert_eq!(parse(w(&base), None), Err(UsageError), "no bus");
        assert!(!StartError::Files.to_string().is_empty());
    }
}
