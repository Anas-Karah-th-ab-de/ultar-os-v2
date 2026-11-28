//! `ultar-compositor` – the compositor service. Started by `ultar-init`
//! as the user "ui" with the right to open raw devices (display, input)
//! and nothing else; clients connect to `--socket`.
//!
//! Signals: SIGTERM/SIGINT stop it, SIGHUP probes the displays again
//! (hot-plug on backends without hot-plug events).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use ultar_compositor::app_rights::AppRights;
use ultar_compositor::backend::{Backend, HalBackend, Headless, OutputKind};
use ultar_compositor::msg::{text, MsgKey};
use ultar_compositor::{Compositor, Config, MonotonicClock, RightsTable};
use ultar_hal_gfx::evdev::SeatPolicy;
use ultar_i18n::Lang;
use ultar_service::rights::{Right, RightSet}; // not a physical side: kernel rights
use ultar_service::{exit, Runtime, ServiceSpec};
use ultar_ui::tokens::ThemeKind;

/// Parsed command line.
#[derive(Debug, Default)]
struct Args {
    socket: Option<PathBuf>,
    headless: Vec<(u32, u32)>,
    output: Option<OutputKind>,
    rights: Option<PathBuf>,
    inline: bool,
    lang: Option<Lang>,
    light: bool,
    no_cursor: bool,
    allow_root: bool,
    seat: SeatPolicy,
    seat_state: Option<PathBuf>,
    app_rights: Option<PathBuf>,
    app_uids: Option<(u32, u32)>,
    app_rights_owner: Option<u32>,
    /// Test only (`desktoptest`): a file that holds the number of key events taken so far.
    input_ack: Option<PathBuf>,
    /// `--start-locked-in a,b`: boot profiles in which the session starts locked.
    start_locked_in: Vec<String>,
}

/// The boot profile `ultar-init` names (`ULTAR_PROFILE`); without one the ordinary system.
fn boot_profile() -> String {
    std::env::var("ULTAR_PROFILE")
        .ok()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "system".to_owned())
}

/// Whether the session starts locked in `profile` (fail closed: an empty list entry or an
/// unreadable profile never unlocks a profile that is listed).
fn starts_locked(listed: &[String], profile: &str) -> bool {
    listed.iter().any(|p| p == profile)
}

/// `FIRST-LAST`, a range of app user ids without root and the unknown id.
fn uid_range(value: &str) -> Option<(u32, u32)> {
    let (first, last) = value.split_once('-')?;
    let (first, last): (u32, u32) = (first.parse().ok()?, last.parse().ok()?);
    (first > 0 && first <= last && last < u32::MAX).then_some((first, last))
}

fn size(value: &str) -> Option<(u32, u32)> {
    let (w, h) = value.split_once('x')?;
    let (w, h) = (w.parse().ok()?, h.parse().ok()?);
    (w > 0 && h > 0 && w <= 16_384 && h <= 16_384).then_some((w, h))
}

fn parse(mut it: impl Iterator<Item = String>) -> Option<Args> {
    let mut args = Args::default();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--socket" => args.socket = Some(PathBuf::from(it.next()?)),
            "--headless" => args.headless.push(size(&it.next()?)?),
            "--kms" => args.output = Some(OutputKind::Kms(Some(PathBuf::from(it.next()?)))),
            "--fb" => args.output = Some(OutputKind::Framebuffer(Some(PathBuf::from(it.next()?)))),
            "--decon" => args.output = Some(OutputKind::Decon(Some(PathBuf::from(it.next()?)))),
            "--rights" => args.rights = Some(PathBuf::from(it.next()?)),
            "--inline" => args.inline = true,
            "--lang" => args.lang = Some(Lang::parse(&it.next()?)?),
            "--light" => args.light = true,
            "--no-cursor" => args.no_cursor = true,
            "--allow-root" => args.allow_root = true,
            "--seat-group" => args.seat.group = Some(it.next()?.parse().ok()?),
            "--allow-virtual-input" => args.seat.virtual_allowed.push(it.next()?),
            "--allow-virtual-owner" => args.seat.virtual_owners.push(it.next()?.parse().ok()?),
            "--seat-state" => args.seat_state = Some(PathBuf::from(it.next()?)),
            "--app-rights" => args.app_rights = Some(PathBuf::from(it.next()?)),
            "--app-uids" => args.app_uids = Some(uid_range(&it.next()?)?),
            // test builds only: the count would tell how long a typed password is (S-13)
            #[cfg(feature = "test-hooks")]
            "--input-ack" => args.input_ack = Some(PathBuf::from(it.next()?)),
            "--app-rights-owner" => args.app_rights_owner = Some(it.next()?.parse().ok()?),
            "--start-locked-in" => {
                let list: Vec<String> = it.next()?.split(',').map(str::to_owned).collect();
                if list.iter().any(String::is_empty) {
                    return None;
                }
                args.start_locked_in.extend(list);
            }
            _ => return None,
        }
    }
    // the app rights file needs its range and its owner, and only with it
    let apps = [
        args.app_rights.is_some(),
        args.app_uids.is_some(),
        args.app_rights_owner.is_some(),
    ];
    let complete = apps.iter().all(|a| *a) || apps.iter().all(|a| !*a);
    (args.socket.is_some() && complete).then_some(args)
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let lang = Lang::from_env();
    let Some(args) = parse(std::env::args().skip(1)) else {
        eprintln!("{}", text(MsgKey::BadArguments, lang));
        eprintln!("{}", text(MsgKey::Usage, lang));
        return exit::USAGE;
    };
    let mut spec = ServiceSpec::new("compositor");
    spec.hardening.allow_root = args.allow_root;
    spec.hardening.rights = Some(RightSet::of(&[Right::RawDevice])); // not a physical side: kernel rights
    let runtime = match Runtime::start(spec) {
        Ok(r) => r,
        Err(_) => {
            eprintln!("{}", text(MsgKey::StartFailed, lang));
            return exit::FAILURE;
        }
    };
    runtime.run(|rt| serve(rt, &args, args.lang.unwrap_or(lang)))
}

fn config(args: &Args, lang: Lang) -> Result<Config, MsgKey> {
    let mut config = Config {
        lang,
        inline_only: args.inline,
        cursor: !args.no_cursor,
        start_locked: starts_locked(&args.start_locked_in, &boot_profile()),
        ..Config::default()
    };
    if args.light {
        config.theme = ThemeKind::Light;
    }
    if let Some(path) = &args.rights {
        let body = std::fs::read_to_string(path).map_err(|_| MsgKey::RightsRejected)?;
        config.rights =
            RightsTable::parse_with(&body, args.allow_root).map_err(|_| MsgKey::RightsRejected)?;
    }
    if let (Some(path), Some((first, last)), Some(owner)) =
        (&args.app_rights, args.app_uids, args.app_rights_owner)
    {
        config
            .rights
            .set_apps(AppRights::new(path.clone(), first, last, owner));
    }
    Ok(config)
}

/// The seat policy after the state file of `ultar-devd-seat`
/// (`--seat-state`). On a kernel that gates device nodes by the right
/// `raw_device` instead of owners (word `kernel-gated`; Ultar kernels before
/// device node modes – the current one keeps them and is `kept`) the group of
/// a node says nothing, so the group check is dropped there. A
/// missing file, any other word, or a file or directory that someone other
/// than root could have written keeps the check (fail closed).
fn seat_policy(seat: &SeatPolicy, state: Option<&Path>) -> SeatPolicy {
    let mut seat = seat.clone();
    if seat.group.is_some() && state.is_some_and(kernel_gated) {
        seat.group = None;
    }
    seat
}

fn kernel_gated(path: &Path) -> bool {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let trusted = |p: &Path| {
        std::fs::symlink_metadata(p).is_ok_and(|m| m.uid() == 0 && m.mode() & 0o022 == 0)
    };
    let is_file = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file());
    if !is_file || !trusted(path) || !path.parent().is_some_and(trusted) {
        return false;
    }
    let mut word = String::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(64).read_to_string(&mut word))
        .is_ok()
        && word.trim() == "kernel-gated"
}

fn serve(rt: &Runtime, args: &Args, lang: Lang) -> i32 {
    let fail = |key| {
        eprintln!("{}", text(key, lang));
        exit::FAILURE
    };
    let config = match config(args, lang) {
        Ok(c) => c,
        Err(key) => return fail(key),
    };
    let backend: Box<dyn Backend> = if args.headless.is_empty() {
        let kind = args.output.as_ref().unwrap_or(&OutputKind::Auto);
        match HalBackend::open(kind, seat_policy(&args.seat, args.seat_state.as_deref())) {
            Ok(b) => Box::new(b),
            Err(_) => return fail(MsgKey::NoDisplay),
        }
    } else {
        Box::new(Headless::new(&args.headless))
    };
    let Some(path) = args.socket.as_deref() else {
        return fail(MsgKey::BadArguments);
    };
    let Ok(listener) = ultar_service::socket::bind_listener(path, Some(0o666)) else {
        return fail(MsgKey::BindFailed);
    };
    let Ok((wake_tx, wake_rx)) = UnixStream::pair() else {
        return fail(MsgKey::StartFailed);
    };
    let mut compositor = Compositor::new(config, backend, Box::new(MonotonicClock::default()));
    if compositor.set_listener(listener).is_err() || compositor.set_wake(wake_rx).is_err() {
        return fail(MsgKey::BindFailed);
    }
    for (byte, register) in [(b's', true), (b'r', false)] {
        let Ok(tx) = wake_tx.try_clone() else {
            return fail(MsgKey::StartFailed);
        };
        let hook = move || {
            let _ = (&tx).write_all(&[byte]);
        };
        let _ = if register {
            rt.lifecycle().on_stop(hook)
        } else {
            rt.lifecycle().on_reload(hook)
        };
    }
    println!("{} {}", text(MsgKey::Listening, lang), path.display());
    rt.ready();
    let mut acked = 0u64;
    loop {
        if compositor.dispatch(None).is_err() {
            return exit::FAILURE;
        }
        input_ack(args.input_ack.as_deref(), &compositor, &mut acked);
        match compositor.take_wake() {
            Some(b's') => return exit::OK,
            Some(_) => compositor.rescan_outputs(),
            None => {}
        }
    }
}

/// `O_NOFOLLOW` (the generic Linux value on x86_64; aarch64 has its own).
#[cfg(target_arch = "aarch64")]
const O_NOFOLLOW: i32 = 0o100_000;
#[cfg(not(target_arch = "aarch64"))]
const O_NOFOLLOW: i32 = 0o400_000;

/// `--input-ack`: the number of key events taken so far into the file (only the count –
/// never which key; the end-to-end test waits for it instead of pressing keys blindly).
fn input_ack(path: Option<&std::path::Path>, compositor: &Compositor, acked: &mut u64) {
    let seen = compositor.state.keyboard.events;
    if let Some(path) = path.filter(|_| seen != *acked) {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        *acked = seen;
        // a link in place of the file is never followed (O_NOFOLLOW), the file is the owner's only
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW)
            .open(path);
        if let Ok(mut file) = file {
            let _ = writeln!(file, "{seen}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn args(list: &[&str]) -> Option<Args> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn the_session_starts_locked_only_in_the_listed_profiles() {
        let a = args(&["--socket", "s", "--start-locked-in", "system,recovery"]).unwrap();
        assert!(starts_locked(&a.start_locked_in, "system"));
        assert!(starts_locked(&a.start_locked_in, "recovery"));
        assert!(!starts_locked(&a.start_locked_in, "installer"));
        assert!(
            !starts_locked(&[], "system"),
            "without the option: as before"
        );
        assert!(args(&["--socket", "s", "--start-locked-in"]).is_none());
        assert!(args(&["--socket", "s", "--start-locked-in", "system,"]).is_none());
        assert!(args(&["--socket", "s", "--start-locked-in", ""]).is_none());
    }

    #[cfg(not(feature = "test-hooks"))]
    #[test]
    fn input_ack_is_refused_outside_test_builds() {
        assert!(args(&["--socket", "s", "--input-ack", "/tmp/ack"]).is_none());
    }

    #[cfg(feature = "test-hooks")]
    #[test]
    fn input_ack_is_an_option_and_counts_key_events() {
        let a = args(&["--socket", "s", "--input-ack", "/tmp/ack"]).unwrap();
        assert_eq!(
            a.input_ack.as_deref(),
            Some(std::path::Path::new("/tmp/ack"))
        );
        assert!(args(&["--socket", "s", "--input-ack"]).is_none());
        let backend = Box::new(Headless::new(&[(64, 48)]));
        let mut c = Compositor::new(
            Config::default(),
            backend,
            Box::new(MonotonicClock::default()),
        );
        let path = std::env::temp_dir().join(format!("ack-{}", std::process::id()));
        let mut acked = 0;
        input_ack(Some(&path), &c, &mut acked);
        assert!(!path.exists(), "nothing yet");
        c.state.keyboard.events = 2;
        input_ack(Some(&path), &c, &mut acked);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "2\n");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn command_line() {
        assert!(args(&[]).is_none());
        let a = args(&[
            "--socket",
            "/run/ultar/ui",
            "--headless",
            "800x600",
            "--inline",
            "--lang",
            "en",
        ])
        .unwrap();
        assert_eq!(a.headless, vec![(800, 600)]);
        assert!(a.inline && a.lang == Some(Lang::En));
        assert!(args(&["--socket", "s", "--headless", "0x5"]).is_none());
        assert!(args(&["--socket", "s", "--bogus"]).is_none());
        assert!(args(&["--socket"]).is_none());
        let k = args(&[
            "--socket",
            "s",
            "--kms",
            "/c",
            "--rights",
            "r",
            "--light",
            "--no-cursor",
            "--allow-root",
            "--seat-group",
            "812",
            "--allow-virtual-input",
            "ultar-osk",
        ])
        .unwrap();
        assert_eq!(k.seat.group, Some(812));
        assert_eq!(k.seat.virtual_allowed, vec!["ultar-osk".to_string()]);
        assert!(args(&["--socket", "s", "--seat-group", "x"]).is_none());
        let st = args(&["--socket", "s", "--seat-state", "/run/ultar/seat/modes"]).unwrap();
        assert_eq!(st.seat_state, Some(PathBuf::from("/run/ultar/seat/modes")));
        assert!(
            matches!(k.output, Some(OutputKind::Kms(_))) && k.light && k.no_cursor && k.allow_root
        );
        let f = args(&["--socket", "s", "--fb", "/f"]).unwrap();
        assert!(matches!(f.output, Some(OutputKind::Framebuffer(_))));
        let c = config(&f, Lang::De).unwrap();
        assert!(c.cursor && c.lang == Lang::De);
        let bad = Args {
            rights: Some(PathBuf::from("/nonexistent/rights")),
            ..Args::default()
        };
        assert_eq!(config(&bad, Lang::Ar).err(), Some(MsgKey::RightsRejected));
        let mut buf = [0u8; 1];
        let (mut a, mut b) = UnixStream::pair().unwrap();
        a.write_all(b"x").unwrap();
        b.read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"x");
    }

    #[test]
    fn app_rights_need_their_range_and_owner() {
        let apps = args(&[
            "--socket",
            "s",
            "--app-rights",
            "/r",
            "--app-uids",
            "20000-49999",
            "--app-rights-owner",
            "805",
        ])
        .unwrap();
        assert_eq!(apps.app_uids, Some((20000, 49999)));
        assert!(config(&apps, Lang::Ar).is_ok());
        assert!(args(&["--socket", "s", "--app-rights", "/r"]).is_none());
        let owner = ["--app-rights", "/r", "--app-rights-owner", "1"];
        for range in ["5-1", "0-9", "x-9", "1"] {
            let mut words = vec!["--socket", "s", "--app-uids", range];
            words.extend(owner);
            assert!(args(&words).is_none(), "{range}");
        }
    }

    #[test]
    fn the_group_check_is_dropped_only_on_a_trusted_kernel_gated_state() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = std::env::temp_dir().join(format!("seat-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let file = dir.join("modes");
        let seat = SeatPolicy {
            group: Some(812),
            virtual_allowed: Vec::new(),
            virtual_owners: Vec::new(),
        };
        assert_eq!(seat_policy(&seat, None).group, Some(812));
        assert_eq!(
            seat_policy(&seat, Some(&file)).group,
            Some(812),
            "missing file"
        );
        std::fs::write(&file, "kept\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(seat_policy(&seat, Some(&file)).group, Some(812));
        std::fs::write(&file, "kernel-gated\n").unwrap();
        let root = std::fs::metadata(&file).unwrap().uid() == 0;
        assert_eq!(
            seat_policy(&seat, Some(&file)).group,
            if root { None } else { Some(812) }
        );
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o666)).unwrap();
        assert_eq!(
            seat_policy(&seat, Some(&file)).group,
            Some(812),
            "writable by others"
        );
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(
            seat_policy(&seat, Some(&file)).group,
            Some(812),
            "directory writable by others"
        );
        let none = SeatPolicy::default();
        assert_eq!(seat_policy(&none, Some(&file)).group, None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
