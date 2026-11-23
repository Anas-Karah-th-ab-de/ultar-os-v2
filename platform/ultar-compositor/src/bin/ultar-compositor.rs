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
use std::path::PathBuf;
use ultar_compositor::backend::{Backend, HalBackend, Headless, OutputKind};
use ultar_compositor::msg::{text, MsgKey};
use ultar_compositor::{Compositor, Config, MonotonicClock, RightsTable};
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
            "--rights" => args.rights = Some(PathBuf::from(it.next()?)),
            "--inline" => args.inline = true,
            "--lang" => args.lang = Some(Lang::parse(&it.next()?)?),
            "--light" => args.light = true,
            "--no-cursor" => args.no_cursor = true,
            "--allow-root" => args.allow_root = true,
            _ => return None,
        }
    }
    args.socket.is_some().then_some(args)
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
        ..Config::default()
    };
    if args.light {
        config.theme = ThemeKind::Light;
    }
    if let Some(path) = &args.rights {
        let body = std::fs::read_to_string(path).map_err(|_| MsgKey::RightsRejected)?;
        config.rights = RightsTable::parse(&body).map_err(|_| MsgKey::RightsRejected)?;
    }
    Ok(config)
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
        match HalBackend::open(args.output.as_ref().unwrap_or(&OutputKind::Auto)) {
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
    loop {
        if compositor.dispatch(None).is_err() {
            return exit::FAILURE;
        }
        match compositor.take_wake() {
            Some(b's') => return exit::OK,
            Some(_) => compositor.rescan_outputs(),
            None => {}
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
        ])
        .unwrap();
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
}
