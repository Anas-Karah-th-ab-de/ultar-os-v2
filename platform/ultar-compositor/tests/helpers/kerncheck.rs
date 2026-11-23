//! The compositor scenario for the Ultar kernel (and any machine with a
//! framebuffer): start the compositor on the real display, start two
//! `ultar-ui` apps as separate processes, capture the composed frame as a
//! privileged client and print one stable verdict line:
//! `COMPOSITOR-CHECK: OK windows=2 frame=WxH colours=N` or
//! `COMPOSITOR-CHECK: FAILED <reason>`.

use super::{say, ClientError};
use std::path::Path;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use ultar_surface::client::Client;
use ultar_surface::proto::{Event, OutputId, Request};

const MARK: &str = "COMPOSITOR-CHECK:";

struct Kids(Vec<Child>);

impl Drop for Kids {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn fail(reason: &str) -> Result<(), ClientError> {
    say(&format!("{MARK} FAILED {reason}"));
    Err(ClientError::Timeout)
}

/// `kerncheck SOCKET COMPOSITOR [--stay]`
pub fn run(socket: &str, args: &[String]) -> Result<(), ClientError> {
    let Some(compositor) = args.first() else {
        return fail("usage");
    };
    let stay = args.iter().any(|a| a == "--stay");
    if !stay {
        // The kernel self-test waits for this program: never longer than this.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(180));
            say(&format!("{MARK} FAILED timeout"));
            std::process::exit(1);
        });
    }
    let (_kids, mut capture) = match start_all(socket, compositor) {
        Ok(v) => v,
        Err(reason) => return fail(reason),
    };
    // Capture until both windows are on the screen (or give up).
    let deadline = Instant::now() + Duration::from_secs(120);
    let verdict = loop {
        let Some(shot) = grab(&mut capture) else {
            return fail("capture");
        };
        if shot.windows >= 2 || Instant::now() >= deadline {
            break shot;
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    if verdict.windows < 2 {
        return fail(&format!("windows={}", verdict.windows));
    }
    say(&format!(
        "{MARK} OK windows={} frame={}x{} colours={}",
        verdict.windows, verdict.width, verdict.height, verdict.colours
    ));
    if stay {
        // Keep the compositor and the apps on the screen for the screenshot.
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
    Ok(())
}

/// Starts the compositor (with a rights file granting capture to this
/// user) and the two apps; returns the children and a capture connection.
fn start_all(socket: &str, compositor: &str) -> Result<(Kids, Client), &'static str> {
    let rights = format!("{socket}.rights");
    if std::fs::write(&rights, "0 = \"ui.capture\"\n").is_err() {
        return Err("rights-file");
    }
    let me = std::env::current_exe().map_err(|_| "current-exe")?;
    // Fork + exec instead of posix_spawn: the Ultar kernel releases a vfork
    // parent only when the child exits (kernel gap K10 in docs/COMPOSITOR.md),
    // so posix_spawn would block here for the compositor's whole life.
    // Setting the (unchanged) user id makes std take the fork path. The
    // children print to the console; nothing is piped.
    let uid = {
        let (a, _b) = std::os::unix::net::UnixStream::pair().map_err(|_| "socketpair")?;
        ultar_compositor::server::peer_of(&a).uid
    };
    let spawn = |program: &Path, args: &[&str]| {
        use std::os::unix::process::CommandExt;
        Command::new(program).args(args).uid(uid).spawn()
    };
    let comp_args = [
        "--socket",
        socket,
        "--rights",
        &rights,
        "--allow-root",
        "--lang",
        "ar",
    ];
    let mut kids = Kids(Vec::new());
    match spawn(Path::new(compositor), &comp_args) {
        Ok(c) => kids.0.push(c),
        Err(_) => return Err("spawn-compositor"),
    }
    let capture = match Client::connect_retry(
        Path::new(socket),
        "org.ultar.check",
        300,
        Duration::from_millis(100),
    ) {
        Ok(c) => c,
        Err(_) => return Err("compositor-not-listening"),
    };
    for (title, background) in [("Ultar 1", "info"), ("Ultar 2", "success")] {
        match spawn(&me, &[socket, "ui", title, background]) {
            Ok(c) => kids.0.push(c),
            Err(_) => return Err("spawn-app"),
        }
    }
    Ok((kids, capture))
}

/// What a captured frame shows.
struct Shot {
    width: u32,
    height: u32,
    colours: usize,
    windows: usize,
}

/// Captures output 0 and counts the app windows on it: each app paints its
/// background in its own token colour (info, success); an app counts when
/// at least 10 000 pixels of its colour are on the screen.
fn grab(client: &mut Client) -> Option<Shot> {
    client
        .send(&Request::Capture {
            output: OutputId(0),
        })
        .ok()?;
    let mut pixels = Vec::new();
    let (width, height) = loop {
        match client.next_event(Some(Duration::from_secs(30))).ok()?? {
            Event::CaptureData {
                width,
                height,
                data,
                last,
                ..
            } => {
                pixels.extend_from_slice(&data);
                if last {
                    break (width, height);
                }
            }
            Event::Error { .. } => return None,
            _ => {}
        }
    };
    let px: Vec<u32> = pixels
        .chunks_exact(4)
        .map(|p| u32::from_le_bytes([p[0], p[1], p[2], 0]))
        .collect();
    let mut colours = px.clone();
    colours.sort_unstable();
    colours.dedup();
    let env = ultar_ui::paint::PaintEnv::new(ultar_ui::Lang::Ar, ultar_ui::tokens::ThemeKind::Dark);
    let role = |r| ultar_compositor::decor::to_pixel(env.palette().get(r)) & 0x00ff_ffff;
    let apps = [
        role(ultar_ui::tokens::ColorRole::Info),
        role(ultar_ui::tokens::ColorRole::Success),
    ];
    let windows = apps
        .iter()
        .filter(|c| px.iter().filter(|p| *p == *c).count() >= 10_000)
        .count();
    Some(Shot {
        width,
        height,
        colours: colours.len(),
        windows,
    })
}

/// Diagnostics: `spawncheck fork|vfork PROGRAM ARGS...` starts a child and
/// reports whether `spawn` returned while the child still runs.
pub fn spawn_check(args: &[String]) -> Result<(), ClientError> {
    use std::os::unix::process::CommandExt;
    let (Some(how), Some(program)) = (args.first(), args.get(1)) else {
        return Err(ClientError::InvalidSize);
    };
    let mut command = Command::new(program);
    command.args(&args[2..]);
    if how == "fork" {
        command.uid(0);
    }
    say(&format!("spawncheck: spawning ({how})"));
    let child = command.spawn();
    say(&format!("spawncheck: spawned {}", child.is_ok()));
    Ok(())
}
