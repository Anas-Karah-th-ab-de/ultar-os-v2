//! `udevctl`: the settings panel's operations on the command line, through the bus
//! service `devices`. All output comes from message keys (ar/en/de).

use crate::msg::Messages;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::device_service::{
    ClearQuarantine, DeviceInfo, DevicePhase, DriverInfo, List, ListReply, OperationReply, Outcome,
    Refusal, Rescan, RescanReply, Rollback, Shutdown, Switch,
};
use ultar_bus::{BusError, Client, ClientOptions};
use ultar_i18n::Arg;

/// Default socket of the system bus.
pub const DEFAULT_BUS: &str = "/run/ultar/bus/system.sock";
/// Longest wait for an answer of the daemon (a rescan reads and verifies the whole depot).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Exit code: the operation was done.
pub const EXIT_OK: i32 = 0;
/// Exit code: the daemon refused the operation or could not be reached.
pub const EXIT_FAILED: i32 = 1;
/// Exit code: wrong command line.
pub const EXIT_USAGE: i32 = 2;

/// What `udevctl` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Show all devices.
    List,
    /// Use another driver for a device.
    Switch {
        /// Address of the device.
        address: String,
        /// Driver identifier.
        driver: String,
        /// Driver version.
        version: String,
    },
    /// Return to the previous driver.
    Rollback {
        /// Address of the device.
        address: String,
    },
    /// Search again (one device, or everything).
    Rescan {
        /// Address of the device, `None` for everything.
        address: Option<String>,
    },
    /// Release quarantined drivers.
    ClearQuarantine(ClearQuarantine),
    /// Stop the daemon.
    Shutdown,
}

/// The parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    /// Bus socket.
    pub bus: PathBuf,
    /// Language option.
    pub lang: Option<String>,
    /// The command.
    pub command: Command,
}

/// The command line is not understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageError;

fn command(words: &[&str]) -> Result<Command, UsageError> {
    let owned = |text: &&str| (*text).to_string();
    Ok(match words {
        ["list"] => Command::List,
        ["switch", address, driver, version] => Command::Switch {
            address: owned(address),
            driver: owned(driver),
            version: owned(version),
        },
        ["rollback", address] => Command::Rollback {
            address: owned(address),
        },
        ["rescan"] => Command::Rescan { address: None },
        ["rescan", address] => Command::Rescan {
            address: Some(owned(address)),
        },
        ["clear-quarantine"] => Command::ClearQuarantine(ClearQuarantine::default()),
        ["clear-quarantine", address] => Command::ClearQuarantine(ClearQuarantine {
            address: Some(owned(address)),
            driver: None,
        }),
        ["clear-quarantine", address, driver, version] => {
            Command::ClearQuarantine(ClearQuarantine {
                address: Some(owned(address)),
                driver: Some((owned(driver), owned(version))),
            })
        }
        ["shutdown"] => Command::Shutdown,
        _ => return Err(UsageError),
    })
}

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<CliArgs, UsageError> {
    let mut bus = PathBuf::from(DEFAULT_BUS);
    let mut lang = None;
    let mut words = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--bus" => bus = PathBuf::from(iter.next().ok_or(UsageError)?),
            "--lang" => lang = Some(iter.next().ok_or(UsageError)?.clone()),
            option if option.starts_with("--") => return Err(UsageError),
            word => words.push(word),
        }
    }
    Ok(CliArgs {
        bus,
        lang,
        command: command(&words)?,
    })
}

fn line(out: &mut dyn Write, text: &str) {
    // A closed output (pipe) must not turn a finished operation into a failure.
    let _ = writeln!(out, "{text}");
}

fn indented(out: &mut dyn Write, messages: &Messages, key: &str, values: &[&str]) {
    let args: Vec<(&str, Arg<'_>)> = crate::msg::POSITIONAL
        .iter()
        .zip(values)
        .map(|(name, value)| (*name, Arg::Str(value)))
        .collect();
    line(out, &format!("    {}", messages.t_args(key, &args)));
}

fn phase_key(phase: DevicePhase) -> &'static str {
    match phase {
        DevicePhase::Detected => "devd.phase.detected",
        DevicePhase::Probing => "devd.phase.probing",
        DevicePhase::Active => "devd.phase.active",
        DevicePhase::NoDriver => "devd.phase.no-driver",
        DevicePhase::Failed => "devd.phase.failed",
    }
}

fn print_alternative(out: &mut dyn Write, messages: &Messages, driver: &DriverInfo) {
    match &driver.quarantined {
        Some(code) => {
            let why = messages.t(&format!("devd.reason.{code}"));
            let values = [driver.id.as_str(), driver.version.as_str(), why.as_str()];
            indented(out, messages, "devd.cli.alternative-quarantined", &values);
        }
        None => {
            let values = [driver.id.as_str(), driver.version.as_str()];
            indented(out, messages, "devd.cli.alternative", &values);
        }
    }
}

/// Prints one device with driver, alternatives and reason.
pub fn print_device(out: &mut dyn Write, messages: &Messages, device: &DeviceInfo) {
    let phase = messages.t(phase_key(device.phase));
    let head = [
        ("address", Arg::Str(&device.address)),
        ("id", Arg::Str(&device.hardware_id)),
        ("phase", Arg::Str(&phase)),
    ];
    line(out, &messages.t_args("devd.cli.device", &head));
    if let Some(label) = &device.label {
        indented(out, messages, "devd.cli.label", &[label]);
    }
    if let Some(active) = &device.active {
        indented(
            out,
            messages,
            "devd.cli.driver",
            &[&active.id, &active.version],
        );
    }
    for alternative in &device.alternatives {
        print_alternative(out, messages, alternative);
    }
    if let Some(previous) = &device.previous {
        indented(out, messages, "devd.cli.previous", &[previous]);
    }
    if let Some(reason) = &device.reason {
        indented(
            out,
            messages,
            "devd.cli.reason",
            &[&messages.render(reason)],
        );
    }
}

/// Prints the device list with the daemon's notices.
pub fn print_list(out: &mut dyn Write, messages: &Messages, reply: &ListReply) {
    if reply.devices.is_empty() {
        line(out, &messages.t("devd.cli.no-devices"));
    }
    for device in &reply.devices {
        print_device(out, messages, device);
    }
    for notice in &reply.notices {
        let text = messages.render(notice);
        line(
            out,
            &messages.t_args("devd.cli.notice", &[("a", Arg::Str(&text))]),
        );
    }
    if !reply.kernel_switch_supported {
        line(out, &messages.t("devd.cli.kernel-switch-unsupported"));
    }
}

fn refused(out: &mut dyn Write, messages: &Messages, refusal: &Refusal) -> i32 {
    let text = messages.render(&refusal.reason);
    line(
        out,
        &messages.t_args("devd.cli.refused", &[("a", Arg::Str(&text))]),
    );
    EXIT_FAILED
}

fn print_operation(out: &mut dyn Write, messages: &Messages, reply: &OperationReply) -> i32 {
    match &reply.outcome {
        Outcome::Done(device) => {
            let key = if reply.restored {
                "devd.cli.restored"
            } else {
                "devd.cli.done"
            };
            line(out, &messages.t(key));
            print_device(out, messages, device);
            if reply.restored {
                EXIT_FAILED
            } else {
                EXIT_OK
            }
        }
        Outcome::Refused(refusal) => refused(out, messages, refusal),
    }
}

fn print_rescan(out: &mut dyn Write, messages: &Messages, reply: &RescanReply) -> i32 {
    let devices = match &reply.devices {
        Ok(devices) => devices,
        Err(refusal) => return refused(out, messages, refusal),
    };
    for (key, addresses) in [
        ("devd.cli.added", &reply.added),
        ("devd.cli.removed", &reply.removed),
    ] {
        for address in addresses {
            line(out, &messages.t_args(key, &[("a", Arg::Str(address))]));
        }
    }
    for device in devices {
        print_device(out, messages, device);
    }
    EXIT_OK
}

/// Runs one command against a connected bus and prints the result.
pub fn execute(
    bus: &Client,
    command: &Command,
    messages: &Messages,
    out: &mut dyn Write,
) -> Result<i32, BusError> {
    Ok(match command {
        Command::List => {
            print_list(out, messages, &bus.call(&List {}, CALL_TIMEOUT)?);
            EXIT_OK
        }
        Command::Switch {
            address,
            driver,
            version,
        } => {
            let call = Switch {
                address: address.clone(),
                driver: driver.clone(),
                version: version.clone(),
            };
            print_operation(out, messages, &bus.call(&call, CALL_TIMEOUT)?)
        }
        Command::Rollback { address } => {
            let call = Rollback {
                address: address.clone(),
            };
            print_operation(out, messages, &bus.call(&call, CALL_TIMEOUT)?)
        }
        Command::Rescan { address } => {
            let call = Rescan {
                address: address.clone(),
            };
            print_rescan(out, messages, &bus.call(&call, CALL_TIMEOUT)?)
        }
        Command::ClearQuarantine(call) => match bus.call(call, CALL_TIMEOUT)?.released {
            Ok(count) => {
                let args = [("count", Arg::Int(i64::from(count)))];
                line(out, &messages.t_args("devd.cli.released", &args));
                EXIT_OK
            }
            Err(refusal) => refused(out, messages, &refusal),
        },
        Command::Shutdown => {
            bus.call(&Shutdown {}, CALL_TIMEOUT)?;
            line(out, &messages.t("devd.cli.shutdown"));
            EXIT_OK
        }
    })
}

/// The whole tool: parse, connect, execute. Returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(UsageError) => {
            let lang = Messages::choose_lang(None);
            line(err, &Messages::new(lang).t("devd.cli.usage"));
            return EXIT_USAGE;
        }
    };
    let messages = Messages::new(Messages::choose_lang(parsed.lang.as_deref()));
    let result = Client::connect(&parsed.bus, ClientOptions::named("udevctl"))
        .and_then(|bus| execute(&bus, &parsed.command, &messages, out));
    match result {
        Ok(code) => code,
        Err(error) => {
            let detail = error.to_string();
            let args = [("a", Arg::Str(&detail))];
            line(err, &messages.t_args("devd.cli.bus-failed", &args));
            EXIT_FAILED
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn parses_every_command() {
        let parsed = parse(&args(&["--bus", "/tmp/b.sock", "--lang", "de", "list"])).unwrap();
        assert_eq!(parsed.bus, PathBuf::from("/tmp/b.sock"));
        assert_eq!(parsed.lang.as_deref(), Some("de"));
        assert_eq!(parsed.command, Command::List);
        let parsed = parse(&args(&["switch", "pci:01:00.0", "nvme", "1.1.0"])).unwrap();
        assert_eq!(parsed.bus, PathBuf::from(DEFAULT_BUS));
        assert!(matches!(parsed.command, Command::Switch { ref driver, .. } if driver == "nvme"));
        let command_of = |words: &[&str]| parse(&args(words)).unwrap().command;
        assert_eq!(
            command_of(&["rollback", "pci:1"]),
            Command::Rollback {
                address: "pci:1".into()
            }
        );
        assert_eq!(command_of(&["rescan"]), Command::Rescan { address: None });
        assert_eq!(
            command_of(&["rescan", "pci:1"]),
            Command::Rescan {
                address: Some("pci:1".into())
            }
        );
        assert_eq!(
            command_of(&["clear-quarantine"]),
            Command::ClearQuarantine(ClearQuarantine::default())
        );
        assert_eq!(
            command_of(&["clear-quarantine", "pci:1", "nvme", "1.0.0"]),
            Command::ClearQuarantine(ClearQuarantine {
                address: Some("pci:1".into()),
                driver: Some(("nvme".into(), "1.0.0".into()))
            })
        );
        assert_eq!(command_of(&["shutdown"]), Command::Shutdown);
    }

    #[test]
    fn rejects_bad_command_lines() {
        for bad in [
            &[][..],
            &["frobnicate"],
            &["switch", "pci:1"],
            &["rollback"],
            &["list", "extra"],
            &["--bus"],
            &["--lang"],
            &["--verbose", "list"],
            &["clear-quarantine", "pci:1", "nvme"],
        ] {
            assert_eq!(parse(&args(bad)), Err(UsageError), "{bad:?}");
        }
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(run(&args(&["nope"]), &mut out, &mut err), EXIT_USAGE);
        assert!(out.is_empty() && !err.is_empty());
    }

    #[test]
    fn unreachable_bus_is_reported() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let socket = std::env::temp_dir().join(format!("udevctl-none-{}.sock", std::process::id()));
        let line = args(&["--bus", &socket.to_string_lossy(), "--lang", "en", "list"]);
        assert_eq!(run(&line, &mut out, &mut err), EXIT_FAILED);
        let text = String::from_utf8(err).unwrap();
        assert!(
            text.starts_with("Could not reach the device service:"),
            "{text}"
        );
    }
}
