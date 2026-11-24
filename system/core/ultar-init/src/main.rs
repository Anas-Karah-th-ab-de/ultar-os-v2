//! `ultar-init` – PID 1 of Ultar OS.
//!
//! ```text
//! ultar-init [--config FILE]        PID 1, or a service manager for tests
//! ultar-init --launch …             the launcher (see `launch.rs`)
//! ```
//!
//! As PID 1 it mounts the base file systems, loads modules, starts the
//! device helpers and the emergency exit, and then runs the services
//! (`ultar_init::daemon`). It never ends on its own: a programming error
//! stops it in place (the USB diagnosis stays reachable), a stop signal
//! leads to the ordered shutdown and then to power off or restart. Not as
//! PID 1 (development, tests) it skips everything that belongs to the
//! machine and ends after the shutdown.

mod boot;
mod config;
mod diag;
mod fwhelper;
mod hw;
mod rescue;

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use ultar_init::daemon::{block_signals, Daemon, Halt, Paths};
use ultar_init::groups::GroupTree;
use ultar_init::output::Forwarder;
use ultar_init::spec::{load_dir, ServiceSpec};
use ultar_init::start::{own_program, Environment};
use ultar_init::supervisor::Supervisor;

/// One journal line: standard error (the console as PID 1) and the
/// diagnosis buffer.
pub fn log(msg: &str) {
    let line = format!("[ultar-init {:>8.3}] {msg}\n", uptime());
    let _ = std::io::stderr().write_all(line.as_bytes());
    diag::remember(&line);
}

fn uptime() -> f64 {
    ultar_sys::machine::uptime()
        .unwrap_or_default()
        .as_secs_f64()
}

fn is_phone() -> bool {
    ultar_hal_system::current_edition() == ultar_profile::Edition::Phone
}

/// The machine's part of the boot (PID 1 only).
fn boot_machine() {
    // PID 1 must never end (a kernel panic and a restart loop would
    // follow): on a programming error it stops here.
    std::panic::set_hook(Box::new(|info| {
        log(&format!("init panic: {info}"));
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }));
    boot::early_mounts();
    // Samsung firmware (/vendor) only on phones.
    if is_phone() {
        boot::mount_vendor_firmware();
    }
    fwhelper::start();
    boot::boot_marker();
    let _ = nix::sys::reboot::set_cad_enabled(false);
}

fn boot_after_config(config: &config::InitConfig) {
    boot::configure(config);
    if !is_phone() {
        boot::sprache_von_startpartition();
    }
    diag::start();
    // Only phones: the largest eMMC partition as /data, in the background.
    if is_phone() {
        boot::mount_userdata_bg();
    }
}

/// The services of this edition; `ultar.start=` names manual ones to start.
fn services(dirs: &[PathBuf]) -> Vec<ServiceSpec> {
    let (specs, refused) = load_dir(dirs);
    for (path, error) in refused {
        log(&format!("refused file={} reason={error}", path.display()));
    }
    let start = config::cmdline_start();
    let edition = ultar_hal_system::current_edition();
    let profile = config::cmdline_profile();
    log(&format!("profile {profile}"));
    specs
        .into_iter()
        .filter_map(|spec| spec.in_profile(&profile))
        .filter(|spec| edition.matches(&spec.life.editions) || start.contains(&spec.name))
        .map(|mut spec| {
            if start.contains(&spec.name) {
                spec.life.manual = false;
            }
            spec
        })
        .collect()
}

fn environment(config: &config::InitConfig, pid1: bool) -> Environment {
    let group_root = config
        .group_root
        .clone()
        .or_else(|| pid1.then(|| PathBuf::from(ultar_sys::machine::GROUP_ROOT)));
    let groups = group_root.map_or_else(GroupTree::none, |root| GroupTree::detect(&root));
    let env = Environment {
        launcher: own_program(config.launcher.as_deref()),
        groups,
        kernel_rights: ultar_sys::ultar_rights().is_ok(),
        root: ultar_sys::euid() == 0,
        stop_timeout: Duration::from_millis(config.shutdown_grace_ms),
        edition: ultar_hal_system::current_edition().as_str().to_owned(),
        profile: config::cmdline_profile(),
        net_allowed: config.net_allowed.clone(),
        all_rights_allowed: config.all_rights_allowed.clone(),
    };
    log(&format!(
        "platform groups={} rights={} ids={}",
        if env.groups.is_present() { "yes" } else { "no" },
        if env.kernel_rights { "yes" } else { "no" },
        if env.root { "root" } else { "own" },
    ));
    env
}

fn config_path(words: &[String]) -> PathBuf {
    words
        .iter()
        .position(|word| word == "--config")
        .and_then(|index| words.get(index + 1))
        .map_or_else(|| PathBuf::from(config::INIT_CONF), PathBuf::from)
}

/// `rescue 1|2|3` on the control socket (the emergency exit).
fn rescue_command(command: &str, argument: &str) -> Option<(String, Option<Halt>)> {
    if command != "rescue" {
        return None;
    }
    Some(match rescue::Level::from_arg(argument) {
        Some(rescue::Level::Reboot) => ("rescue: reboot\n".to_owned(), Some(Halt::Reboot)),
        Some(level) => (rescue::act(level, "ultar-ctl", "none"), None),
        None => ("rescue 1|2|3\n".to_owned(), None),
    })
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    if words.first().map(String::as_str) == Some(ultar_init::launch::LAUNCH_ARG) {
        std::process::exit(ultar_init::launch::launch(
            words.get(1..).unwrap_or_default(),
        ));
    }
    let pid1 = std::process::id() == 1;
    // Before any thread exists: every thread inherits the blocked mask.
    let signals = match block_signals() {
        Ok(signals) => signals,
        Err(error) => {
            log(&format!("signals unavailable detail={error}"));
            return;
        }
    };
    if pid1 {
        boot_machine();
    }
    log(&format!(
        "start version={} pid1={pid1}",
        env!("CARGO_PKG_VERSION")
    ));
    let config = config::load(&config_path(&words));
    if pid1 {
        boot_after_config(&config);
    }
    let env = environment(&config, pid1);
    let specs = services(&config.service_dirs);
    log(&format!("services loaded={}", specs.len()));
    let forwarder = Forwarder::new(config.output_socket.clone());
    let supervisor = Supervisor::new(specs, env, forwarder, Box::new(log));
    let paths = Paths {
        services: config.service_dirs.clone(),
        bus_socket: config.bus_socket.clone(),
        bus_policy: config.bus_policy.clone(),
        control_socket: config.control_socket.clone(),
    };
    let halt = Daemon::new(supervisor, signals, paths, pid1).run(rescue_command);
    if pid1 {
        end_machine(halt);
    }
    log("end");
}

/// After the services: end everything else, unmount, power off or restart.
fn end_machine(halt: Halt) -> ! {
    // Everything but PID 1.
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(-1),
        nix::sys::signal::Signal::SIGKILL,
    );
    std::thread::sleep(Duration::from_millis(100));
    while let Ok(status) = nix::sys::wait::waitpid(
        nix::unistd::Pid::from_raw(-1),
        Some(nix::sys::wait::WaitPidFlag::WNOHANG),
    ) {
        if status == nix::sys::wait::WaitStatus::StillAlive {
            break;
        }
    }
    boot::unmount_all();
    log(&format!("halt {halt:?}"));
    let how = match halt {
        Halt::PowerOff => nix::sys::reboot::RebootMode::RB_POWER_OFF,
        Halt::Reboot | Halt::Exit => nix::sys::reboot::RebootMode::RB_AUTOBOOT,
    };
    let _ = nix::sys::reboot::reboot(how);
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
