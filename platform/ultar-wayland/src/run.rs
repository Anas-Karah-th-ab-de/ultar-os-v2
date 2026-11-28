//! The bridge's life: connect to the compositor, offer the Wayland socket,
//! start the program, move events both ways until the program ends.

use crate::args::{Args, SOCKET_NAME};
use crate::msg::{Lang, Msg};
use crate::seat::{Keymap, Seat};
use crate::state::{Objects, Serials, State};
use crate::{data, events, limits};
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_surface::client::{Client as NativeClient, ClientError};
use wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use wayland_server::{Display, ListeningSocket};

/// Exit code when the bridge itself fails (EX_SOFTWARE-like).
pub const EXIT_BRIDGE: i32 = 70;
/// Exit code when the program cannot start.
pub const EXIT_START: i32 = 127;
/// Longest sleep of the loop (the program's end is noticed within it).
const TICK: Duration = Duration::from_millis(50);

struct Conn;

impl ClientData for Conn {
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {}
}

/// Runs the bridge; returns the exit code (the program's, or a bridge failure).
pub fn run(args: Args, lang: Lang) -> i32 {
    let dir = runtime_dir(&args);
    let Ok(native) = NativeClient::connect_retry(
        &args.compositor,
        &args.app_id,
        20,
        Duration::from_millis(100),
    ) else {
        eprintln!("ultar-wayland: {}", Msg::CompositorFailed.text(lang));
        return EXIT_BRIDGE;
    };
    let Some((mut display, listener, mut state)) = setup(native, &dir) else {
        eprintln!("ultar-wayland: {}", Msg::SocketFailed.text(lang));
        return EXIT_BRIDGE;
    };
    let Ok(mut child) = spawn(&args, &dir) else {
        eprintln!("ultar-wayland: {}", Msg::StartFailed.text(lang));
        return EXIT_START;
    };
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let _ = display.flush_clients();
            return exit_code(status);
        }
        wait(&display, &listener, &state);
        accept(&listener, &mut display, &mut state);
        // An error here is the poll itself failing; the next round retries.
        let _ = display.dispatch_clients(&mut state);
        native_events(&mut state);
        data::pump(&mut state);
        let _ = display.flush_clients();
        if state.native_failed {
            eprintln!("ultar-wayland: {}", Msg::CompositorGone.text(lang));
            let _ = child.kill();
            let _ = child.wait();
            return EXIT_BRIDGE;
        }
    }
}

fn runtime_dir(args: &Args) -> PathBuf {
    let dir = args
        .runtime_dir
        .clone()
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
        .unwrap_or_else(|| {
            PathBuf::from(format!(
                "/tmp/ultar-wayland-{}",
                rustix::process::getuid().as_raw()
            ))
        });
    let _ = std::fs::create_dir_all(&dir);
    let _ = rustix::fs::chmod(&dir, rustix::fs::Mode::from_raw_mode(0o700));
    dir
}

fn setup(
    native: NativeClient,
    dir: &std::path::Path,
) -> Option<(Display<State>, ListeningSocket, State)> {
    let display: Display<State> = Display::new().ok()?;
    let dh = display.handle();
    crate::globals::create(&dh);
    let listener = ListeningSocket::bind_absolute(dir.join(SOCKET_NAME)).ok()?;
    let keymap = Keymap::new(&crate::keymap::keymap_text()).ok()?;
    let rights = native.rights();
    let outputs = native.outputs().to_vec();
    let mut state = State {
        dh,
        native: crate::native::Native::new(native),
        rights,
        objects: Objects::default(),
        seat: Seat::new(keymap),
        outputs: Default::default(),
        clipboard: Default::default(),
        serials: Serials::default(),
        start: Instant::now(),
        native_failed: false,
    };
    for info in outputs {
        crate::output::update(&mut state, info);
    }
    Some((display, listener, state))
}

fn spawn(args: &Args, dir: &std::path::Path) -> std::io::Result<Child> {
    let mut cmd = Command::new(&args.program[0]);
    cmd.args(&args.program[1..])
        .env("XDG_RUNTIME_DIR", dir)
        .env("WAYLAND_DISPLAY", SOCKET_NAME)
        .env("XDG_SESSION_TYPE", "wayland")
        .env("GDK_BACKEND", "wayland")
        .env("QT_QPA_PLATFORM", "wayland")
        .env("SDL_VIDEODRIVER", "wayland")
        .env("SDL_VIDEO_DRIVER", "wayland")
        .env("MOZ_ENABLE_WAYLAND", "1")
        .env("CLUTTER_BACKEND", "wayland")
        .env("_JAVA_AWT_WM_NONREPARENTING", "1")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .env_remove(ultar_surface::client::ACTIVATION_ENV);
    // Fork + exec instead of posix_spawn: the Ultar kernel releases a vfork
    // parent only when the child exits (kernel gap K10, docs/COMPOSITOR.md).
    // Setting the unchanged user id makes std take the fork path.
    std::os::unix::process::CommandExt::uid(&mut cmd, rustix::process::getuid().as_raw());
    cmd.spawn()
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

/// Sleeps until something is ready (or a tick passed).
fn wait(display: &Display<State>, listener: &ListeningSocket, state: &State) {
    let extra = data::poll_fds(state);
    let mut fds = vec![
        PollFd::from_borrowed_fd(display.as_fd(), PollFlags::IN),
        PollFd::from_borrowed_fd(listener.as_fd(), PollFlags::IN),
        PollFd::from_borrowed_fd(state.native.client.fd(), PollFlags::IN),
    ];
    for (fd, write) in extra {
        fds.push(PollFd::from_borrowed_fd(
            fd,
            if write { PollFlags::OUT } else { PollFlags::IN },
        ));
    }
    let tick = Timespec {
        tv_sec: 0,
        tv_nsec: TICK.as_nanos() as i64,
    };
    let _ = poll(&mut fds, Some(&tick));
}

fn accept(listener: &ListeningSocket, display: &mut Display<State>, state: &mut State) {
    while let Ok(Some(stream)) = listener.accept() {
        let handle = state.dh.backend_handle();
        state
            .objects
            .connections
            .retain(|c| handle.get_client_data(c.clone()).is_ok());
        if state.objects.connections.len() >= limits::MAX_CLIENTS {
            continue;
        }
        if let Ok(client) = display.handle().insert_client(stream, Arc::new(Conn)) {
            state.objects.connections.push(client.id());
        }
    }
}

fn native_events(state: &mut State) {
    for _ in 0..4096 {
        match state.native.client.next_event(Some(Duration::ZERO)) {
            Ok(Some(event)) => events::handle(state, event),
            Ok(None) => return,
            Err(ClientError::Timeout) => return,
            Err(_) => {
                state.native_failed = true;
                return;
            }
        }
    }
}
