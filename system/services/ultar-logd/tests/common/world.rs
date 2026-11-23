//! A broker with token identities, the files the daemon needs, and helpers
//! to start the real `ultar-logd` and `ulog` binaries.
#![allow(dead_code)]

use super::{temp_dir, WAIT};
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use ultar_bus::messages::log::{Query, QueryReply, Stats, StatsReply};
use ultar_bus::{Broker, BusError, Client, ClientOptions, Identity, Policy, TokenIdentity};
use ultar_service::log::{LogTarget, Logger, LoggerConfig, RateLimit};

pub const LOGD: &str = env!("CARGO_BIN_EXE_ultar-logd");
pub const ULOG: &str = env!("CARGO_BIN_EXE_ulog");

pub const POLICY: &str = r#"
[[rule]]
identity = "system.logd"
register = ["log"]

[[rule]]
identity = "app.*"
call     = ["log.query", "log.stats"]

[[rule]]
identity = "admin.support"
call     = ["log.query", "log.stats", "log.read_all"]

[[rule]]
identity = "app.mute"
"#;

pub const TOKENS: [(&str, &str); 5] = [
    ("system.logd", "token-0123456789-logd"),
    ("app.one", "token-0123456789-one"),
    ("app.two", "token-0123456789-two"),
    ("admin.support", "token-0123456789-admin"),
    ("other.mute", "token-0123456789-mute"),
];

pub fn send_signal(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .args([&format!("-{signal}"), &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

pub fn write_private(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

pub struct World {
    pub dir: PathBuf,
    _broker: Broker,
}

impl World {
    pub fn new(tag: &str) -> Self {
        let dir = temp_dir(tag);
        let mut auth = TokenIdentity::new();
        for (identity, token) in TOKENS {
            auth = auth.with_token(token, Identity::new(identity).unwrap());
        }
        let broker = Broker::builder(Policy::from_toml_str(POLICY).unwrap())
            .listen(&dir.join("bus.sock"), Arc::new(auth))
            .start()
            .unwrap();
        std::fs::write(dir.join("policy.toml"), POLICY).unwrap();
        let uid = ultar_service::effective_uid();
        std::fs::write(dir.join("uids.toml"), format!("{uid} = \"app.one\"\n")).unwrap();
        write_private(&dir.join("logd.token"), TOKENS[0].1);
        write_private(
            &dir.join("chain.key"),
            "a chain key only the log service reads\n",
        );
        std::fs::write(
            dir.join("kernel.log"),
            "[kern] bereit nach 145 ms\n[kern] nvme0 ok\n",
        )
        .unwrap();
        Self {
            dir,
            _broker: broker,
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub fn logd(&self, extra: &[&str]) -> Command {
        let token = self.path("logd.token");
        let key = self.path("chain.key");
        let secrets = [
            "--bus-token-file".as_ref(),
            token.as_os_str(),
            "--chain-key-file".as_ref(),
            key.as_os_str(),
        ];
        self.logd_with(Command::new(LOGD), &secrets, extra)
    }

    /// The daemon's command line on `command` with the secret options
    /// `secrets`.
    pub fn logd_with(
        &self,
        mut command: Command,
        secrets: &[&std::ffi::OsStr],
        extra: &[&str],
    ) -> Command {
        command
            .args(["--lang", "en", "--dir"])
            .arg(self.path("log"))
            .arg("--listen")
            .arg(self.path("ingest.sock"))
            .arg("--listen")
            .arg(format!("{}=app.two", self.path("two.sock").display()))
            .arg("--uid-map")
            .arg(self.path("uids.toml"))
            .arg("--bus")
            .arg(self.path("bus.sock"))
            .args(secrets)
            .arg("--policy")
            .arg(self.path("policy.toml"))
            .arg("--kernel-log")
            .arg(self.path("kernel.log"))
            .args([
                "--segment-bytes",
                "16384",
                "--total-bytes",
                "131072",
                "--fsync",
                "rotate",
            ])
            // No rate limit unless a test asks for one (later options win).
            .args(["--rate-burst", "1000000", "--rate-per-second", "0"])
            .args(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if ultar_service::effective_uid() == 0 {
            command.arg("--allow-root");
        }
        command
    }

    /// Starts the daemon and waits for its ready line.
    pub fn start(&self, extra: &[&str]) -> Child {
        ready(self.logd(extra))
    }
}

/// Spawns `command` and waits for the daemon's ready line.
pub fn ready(mut command: Command) -> Child {
    let mut child = command.spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "Log service ready");
    child
}

impl World {
    pub fn client(&self, identity: &str) -> Client {
        let token = TOKENS.iter().find(|(name, _)| *name == identity).unwrap().1;
        let options = ClientOptions {
            token: Some(token.to_owned()),
            ..ClientOptions::named("test")
        };
        Client::connect(&self.path("bus.sock"), options).unwrap()
    }

    /// Calls `log.query`, waiting until the service has joined the bus.
    pub fn query(&self, client: &Client, query: &Query) -> QueryReply {
        loop {
            match client.call(query, WAIT) {
                Ok(reply) => return reply,
                Err(BusError::Remote(ultar_bus::ErrorCode::UnknownService)) => {
                    std::thread::yield_now();
                }
                Err(other) => panic!("{other}"),
            }
        }
    }

    pub fn logger(&self, socket: &str) -> Logger {
        let mut config = LoggerConfig::to(LogTarget::Socket(self.path(socket)));
        config.rate = RateLimit {
            burst: u32::MAX,
            per_second: 0,
        };
        config.buffer_records = 4096;
        Logger::start(config)
    }

    pub fn ulog(&self, identity: &str, words: &[&str]) -> Output {
        let token = TOKENS.iter().find(|(name, _)| *name == identity).unwrap().1;
        let file = self.path(&format!("{identity}.token"));
        write_private(&file, token);
        Command::new(ULOG)
            .args(["--lang", "en", "--bus"])
            .arg(self.path("bus.sock"))
            .arg("--token-file")
            .arg(file)
            .args(words)
            .output()
            .unwrap()
    }

    /// Waits until the record with message key `key` is stored.
    pub fn wait_for_key(&self, admin: &Client, key: &str) -> QueryReply {
        let mut after = 0;
        loop {
            let query = Query {
                after_seq: after,
                wait_ms: 5_000,
                ..Query::default()
            };
            let reply = self.query(admin, &query);
            if reply.records.iter().any(|record| record.key == key) {
                return reply;
            }
            after = reply.next_seq;
        }
    }
}

pub fn stop(child: Child) -> (Option<i32>, String) {
    send_signal(child.id(), "TERM");
    let output = child.wait_with_output().unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

pub fn keys(reply: &QueryReply) -> Vec<&str> {
    reply
        .records
        .iter()
        .map(|record| record.key.as_str())
        .collect()
}

impl World {
    /// Calls `log.stats`, waiting until the service has joined the bus.
    pub fn stats(&self, client: &Client) -> StatsReply {
        loop {
            match client.call(&Stats {}, WAIT) {
                Ok(stats) => return stats,
                Err(BusError::Remote(ultar_bus::ErrorCode::UnknownService)) => {
                    std::thread::yield_now();
                }
                Err(other) => panic!("{other}"),
            }
        }
    }

    /// Runs `ulog verify` on the log directory with the chain key; the
    /// result must be `expect_ok`. Returns the tool's output.
    pub fn verify(&self, expect_ok: bool) -> String {
        let output = Command::new(ULOG)
            .args(["--lang", "en", "verify"])
            .arg(self.path("log"))
            .arg("--key-file")
            .arg(self.path("chain.key"))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        assert_eq!(output.status.success(), expect_ok, "{text}");
        text
    }
}
