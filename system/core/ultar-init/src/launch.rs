//! Starting a service confined: `ultar-init` executes **itself** as the
//! launcher (`ultar-init --launch …`), and the launcher – a fresh,
//! single-threaded process, still with init's ids and rights – confines
//! itself step by step and then executes the service:
//!
//! 1. moves into the service's memory group (needs `manage_groups`),
//! 2. starts a session of its own (init signals the whole process group),
//! 3. takes the console as controlling terminal (console services only),
//! 4. switches to the service's groups and ids (needs `set_id`; read back),
//! 5. forbids new privileges, drops every kernel right the service does
//!    not keep (only after 1 and 4, which need rights),
//! 6. unblocks the signals init blocked (the mask survives `execve`),
//! 7. changes to the working directory and executes the program.
//!
//! Any failed step ends the launcher with its own exit code
//! ([`LaunchFailure`]) before the service ran: nothing starts half confined.
//! No `unsafe` and no `pre_exec` – the launcher is an ordinary process.

use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// First argument of the launcher mode.
pub const LAUNCH_ARG: &str = "--launch";

/// What the launcher does for one start.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Program.
    pub exec: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Ids to switch to; `None`: keep init's.
    pub identity: Option<ultar_sys::Identity>,
    /// Kernel rights to drop (bit mask); `None`: the kernel has none.
    pub drop_rights: Option<u64>,
    /// Memory group to move into.
    pub group_dir: Option<PathBuf>,
    /// Working directory.
    pub workdir: Option<String>,
    /// Take standard input as controlling terminal.
    pub console: bool,
}

/// Why the launcher stopped before the service ran: its exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchFailure {
    /// The launcher's command line is not one init writes.
    Arguments = 120,
    /// The memory group refused the process.
    Group = 121,
    /// No session of its own.
    Session = 122,
    /// The ids could not be switched (or did not read back).
    Identity = 123,
    /// New privileges or kernel rights could not be dropped.
    Rights = 124,
    /// Console or working directory unusable.
    Setup = 125,
    /// The program could not be executed.
    Exec = 127,
}

impl LaunchFailure {
    /// The failure an exit code names, if it is one.
    pub fn from_code(code: i32) -> Option<Self> {
        [
            Self::Arguments,
            Self::Group,
            Self::Session,
            Self::Identity,
            Self::Rights,
            Self::Setup,
            Self::Exec,
        ]
        .into_iter()
        .find(|failure| *failure as i32 == code)
    }

    /// Stable lower-case word.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Arguments => "launch_arguments",
            Self::Group => "launch_group",
            Self::Session => "launch_session",
            Self::Identity => "launch_identity",
            Self::Rights => "launch_rights",
            Self::Setup => "launch_setup",
            Self::Exec => "launch_exec",
        }
    }
}

fn join(ids: &[u32]) -> String {
    ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

/// The launcher's command for `plan`; the caller sets environment and
/// standard streams.
pub fn command(launcher: &Path, plan: &Plan) -> Command {
    let mut command = Command::new(launcher);
    command.arg(LAUNCH_ARG);
    if let Some(identity) = &plan.identity {
        command.args(["--uid", &identity.uid.to_string()]);
        command.args(["--gid", &identity.gid.to_string()]);
        command.args(["--groups", &join(&identity.groups)]);
    }
    if let Some(mask) = plan.drop_rights {
        command.args(["--drop", &format!("{mask:x}")]);
    }
    if let Some(dir) = &plan.group_dir {
        command.arg("--group-dir").arg(dir);
    }
    if let Some(dir) = &plan.workdir {
        command.args(["--workdir", dir]);
    }
    if plan.console {
        command.arg("--console");
    }
    command.arg("--").arg(&plan.exec).args(&plan.args);
    command
}

fn number(text: &str) -> Option<u32> {
    (!text.is_empty() && text.len() <= 10 && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn ids(text: &str) -> Option<Vec<u32>> {
    if text.is_empty() {
        return Some(Vec::new());
    }
    text.split(',').map(number).collect()
}

/// Parses the words after [`LAUNCH_ARG`].
pub fn parse(words: &[String]) -> Option<Plan> {
    let mut plan = Plan::default();
    let mut identity = ultar_sys::Identity::default();
    let (mut uid, mut gid) = (None, None);
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        match word.as_str() {
            "--" => break,
            "--console" => plan.console = true,
            option => {
                let value = rest.next()?;
                match option {
                    "--uid" => uid = Some(number(value)?),
                    "--gid" => gid = Some(number(value)?),
                    "--groups" => identity.groups = ids(value)?,
                    "--drop" => plan.drop_rights = Some(u64::from_str_radix(value, 16).ok()?),
                    "--group-dir" => plan.group_dir = Some(PathBuf::from(value)),
                    "--workdir" => plan.workdir = Some(value.clone()),
                    _ => return None,
                }
            }
        }
    }
    plan.exec = rest.next()?.clone();
    plan.args = rest.cloned().collect();
    match (uid, gid) {
        (Some(uid), Some(gid)) => {
            identity.uid = uid;
            identity.gid = gid;
            plan.identity = Some(identity);
        }
        (None, None) if identity.groups.is_empty() => {}
        _ => return None,
    }
    Some(plan)
}

/// Steps 1–6 (see the module documentation).
fn confine(plan: &Plan) -> Result<(), LaunchFailure> {
    if let Some(dir) = &plan.group_dir {
        std::fs::write(dir.join("cgroup.procs"), std::process::id().to_string())
            .map_err(|_| LaunchFailure::Group)?;
    }
    // A kernel without sessions (ENOSYS) is detected, not assumed: init
    // then signals the process itself.
    match ultar_sys::new_session() {
        Ok(()) | Err(ultar_sys::Errno::ENOSYS) => {}
        Err(_) => return Err(LaunchFailure::Session),
    }
    if plan.console {
        ultar_sys::set_controlling_terminal(std::io::stdin()).map_err(|_| LaunchFailure::Setup)?;
    }
    if let Some(identity) = &plan.identity {
        ultar_sys::switch_identity(identity).map_err(|_| LaunchFailure::Identity)?;
    }
    // A kernel without the flag (it honours no set-id bits either) is
    // detected by its answer; any other refusal stops the start.
    match ultar_sys::set_no_new_privileges() {
        Ok(()) | Err(ultar_sys::Errno::ENOSYS | ultar_sys::Errno::EINVAL) => {}
        Err(_) => return Err(LaunchFailure::Rights),
    }
    if let Some(mask) = plan.drop_rights {
        ultar_sys::ultar_drop_rights(mask).map_err(|_| LaunchFailure::Rights)?;
        let left = ultar_sys::ultar_rights().map_err(|_| LaunchFailure::Rights)?;
        if left & mask != 0 {
            return Err(LaunchFailure::Rights);
        }
    }
    ultar_sys::set_signal_mask(0).map_err(|_| LaunchFailure::Setup)?;
    if let Some(dir) = &plan.workdir {
        std::env::set_current_dir(dir).map_err(|_| LaunchFailure::Setup)?;
    }
    Ok(())
}

/// The launcher: confines itself and executes the service. Returns only
/// on failure, with the exit code to end with.
pub fn launch(words: &[String]) -> i32 {
    let Some(plan) = parse(words) else {
        return LaunchFailure::Arguments as i32;
    };
    if let Err(failure) = confine(&plan) {
        return failure as i32;
    }
    let error = Command::new(OsString::from(&plan.exec))
        .args(&plan.args)
        .exec();
    let _ = error;
    LaunchFailure::Exec as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(command: &Command) -> Vec<String> {
        command
            .get_args()
            .skip(1)
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_plan_survives_the_command_line() {
        let plan = Plan {
            exec: "/usr/bin/ultar-logd".into(),
            args: vec!["--dir".into(), "--".into(), "".into()],
            identity: Some(ultar_sys::Identity {
                uid: 802,
                gid: 802,
                groups: vec![801, 803],
            }),
            drop_rights: Some(!0x180),
            group_dir: Some("/sys/fs/cgroup/system/logd".into()),
            workdir: Some("/".into()),
            console: true,
        };
        let command = command(Path::new("/sbin/init"), &plan);
        assert_eq!(command.get_args().next().unwrap(), LAUNCH_ARG);
        assert_eq!(parse(&words(&command)), Some(plan));
        let bare = Plan {
            exec: "/bin/true".into(),
            ..Plan::default()
        };
        assert_eq!(parse(&words(&command_for(&bare))), Some(bare));
    }

    fn command_for(plan: &Plan) -> Command {
        command(Path::new("/x"), plan)
    }

    #[test]
    fn command_lines_init_never_writes_are_refused() {
        let refused: [&[&str]; 7] = [
            &[],
            &["--uid", "1", "--", "/bin/true"],
            &["--uid", "x", "--gid", "1", "--", "/bin/true"],
            &["--groups", "1,a", "--", "/bin/true"],
            &["--drop", "zz", "--", "/bin/true"],
            &["--frob", "1", "--", "/bin/true"],
            &["--uid", "1", "--gid", "1"],
        ];
        for case in refused {
            let case: Vec<String> = case.iter().map(|w| (*w).to_owned()).collect();
            assert_eq!(parse(&case), None, "{case:?}");
        }
        assert_eq!(
            launch(&["--frob".to_owned()]),
            LaunchFailure::Arguments as i32
        );
        for code in [120, 121, 122, 123, 124, 125, 127] {
            assert_eq!(LaunchFailure::from_code(code).map(|f| f as i32), Some(code));
        }
        assert_eq!(LaunchFailure::from_code(0), None);
    }
}
