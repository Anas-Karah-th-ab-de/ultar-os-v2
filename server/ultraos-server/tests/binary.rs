//! The real binary: administration on the command line, messages in three languages.

use std::process::{Command, Output};

fn run(dir: &std::path::Path, lang: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ultraos-server"))
        .args(args)
        .env("ULTRAOS_DB", dir.join("db.sqlite"))
        .env("ULTRAOS_FILES", dir.join("files"))
        .env("LANG", lang)
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn admin_commands_work_on_the_database_file() {
    let dir = tempfile::tempdir().unwrap();
    let o = run(
        dir.path(),
        "en_US.UTF-8",
        &["admin", "channel", "nightly", "gated"],
    );
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(stdout(&o).trim(), "Channel nightly requires access.");
    let o = run(
        dir.path(),
        "de_DE.UTF-8",
        &["admin", "channel", "nightly", "open"],
    );
    assert_eq!(stdout(&o).trim(), "Kanal nightly ist für alle offen.");
    let o = run(
        dir.path(),
        "C",
        &["admin", "publisher", "KM Code Craft", "verified"],
    );
    assert_eq!(
        stdout(&o).trim(),
        "الناشر KM Code Craft موثّق.",
        "Arabic is the default"
    );

    std::fs::write(
        dir.path().join("meta.json"),
        r#"{"id":"a.app","name":"App","version":"1.0"}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("a.uap"), b"package").unwrap();
    let meta = dir.path().join("meta.json");
    let pkg = dir.path().join("a.uap");
    let o = run(
        dir.path(),
        "en",
        &[
            "admin",
            "publish-app",
            meta.to_str().unwrap(),
            pkg.to_str().unwrap(),
        ],
    );
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).starts_with("Published a.app version 1.0 (7 bytes, sha256 "));
    assert_eq!(
        std::fs::read_dir(dir.path().join("files")).unwrap().count(),
        1
    );
}

#[test]
fn errors_use_exit_codes_and_translated_messages() {
    let dir = tempfile::tempdir().unwrap();
    let o = run(dir.path(), "en", &["admin", "grant", "ghost", "app", "x"]);
    assert_eq!(o.status.code(), Some(1));
    // stderr also carries the log line of the first-time schema creation; the message is last.
    assert_eq!(
        stderr(&o).lines().last(),
        Some("There is no account with this username.")
    );
    let o = run(dir.path(), "en", &["admin", "bogus"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).starts_with("Usage:"));
    let o = run(dir.path(), "de", &["serve-please"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).starts_with("Aufruf:"));
}

#[test]
fn malformed_token_key_stops_the_server_before_it_listens() {
    let dir = tempfile::tempdir().unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_ultraos-server"))
        .env("ULTRAOS_DB", dir.path().join("db.sqlite"))
        .env("ULTRAOS_SECRET", "too-short")
        .env("ULTRAOS_BIND", "127.0.0.1:0")
        .env("LANG", "en")
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("ULTRAOS_SECRET must be 64 hexadecimal characters."));
}

/// Kills the server process when the test ends, also on failure.
struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get_status(addr: &str, path: &str) -> u16 {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    let mut text = String::new();
    let _ = stream.read_to_string(&mut text);
    text.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}

#[test]
fn server_process_serves_public_endpoints_with_limits_from_the_environment() {
    use std::io::BufRead;
    let dir = tempfile::tempdir().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_ultraos-server"))
        .env("ULTRAOS_DB", dir.path().join("db.sqlite"))
        .env("ULTRAOS_FILES", dir.path().join("files"))
        .env("ULTRAOS_SECRET", "11".repeat(32))
        .env("ULTRAOS_BIND", "127.0.0.1:0")
        .env("ULTRAOS_LIMIT_GENERAL", "2,60")
        .env("ULTRAOS_LIMIT_AUTH", "nonsense")
        .env("ULTRAOS_TRUSTED_PROXIES", "127.0.0.1, bogus")
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut server = Running(child);
    let stderr = std::io::BufReader::new(server.0.stderr.take().unwrap());
    let mut log = Vec::new();
    let mut addr = None;
    for line in stderr.lines() {
        let line = line.unwrap();
        addr = line
            .split("addr=")
            .nth(1)
            .filter(|_| line.contains("listening"))
            .map(str::to_string);
        log.push(line);
        if addr.is_some() {
            break;
        }
    }
    let addr = addr.unwrap_or_else(|| panic!("server did not start:\n{}", log.join("\n")));
    let log = log.join("\n");
    assert!(
        log.contains("ignored_limit") && log.contains("ULTRAOS_LIMIT_AUTH"),
        "{log}"
    );
    assert!(
        log.contains("ignored_trusted_proxy") && log.contains("bogus"),
        "{log}"
    );

    assert_eq!(get_status(&addr, "/v1/health"), 200);
    assert_eq!(get_status(&addr, "/v1/store/catalog"), 200);
    assert_eq!(get_status(&addr, "/v1/update/latest"), 204);
    assert_eq!(
        get_status(&addr, "/v1/store/catalog"),
        429,
        "burst of two from the environment"
    );
    assert_eq!(get_status(&addr, "/v1/health"), 200);
}
