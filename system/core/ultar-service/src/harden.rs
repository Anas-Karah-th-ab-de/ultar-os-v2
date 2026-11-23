//! Self-limits and hardening a service applies to itself at start.
//!
//! Every step is **on by default** ([`Hardening::default`]); a service
//! turns a step off or widens it only by saying so in its own code, where
//! a reviewer sees it. A step that is on and cannot be carried out stops
//! the start (fail closed) – with one exception: the Ultar kernel does not
//! implement some of the mechanisms yet. Those steps are reported as
//! [`Outcome::Unsupported`] in the [`HardeningReport`] and logged, they do
//! not stop the service (it could never start otherwise).
//!
//! | Step | Linux | Ultar kernel (state 23.11.2026) |
//! |---|---|---|
//! | close inherited descriptors | applied | applied |
//! | `umask 077` | applied | applied (call 95) |
//! | working directory `/` | applied | applied |
//! | refuse root | applied | unsupported: the kernel has no user ids (`getuid` is always 0) – rights replace them |
//! | no new privileges | applied | accepted, no effect (no set-uid programs exist) |
//! | resource limits | applied and read back | unsupported: `prlimit` ignores new values |
//! | drop kernel rights | not applicable (note) | applied and read back (`prctl` 0x554c5201 / 0x554c5200) |

pub use crate::rights::{Right, RightSet, PR_ULTAR_RIGHTS_DROP, PR_ULTAR_RIGHTS_GET};
use std::fmt;
use ultar_sys::{self as sys, Errno, Resource};

/// Which kernel the process runs on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    /// A Linux kernel (development machine, compatibility).
    Linux,
    /// The Ultar kernel.
    Ultar,
}

impl Platform {
    /// Detects the kernel: only the Ultar kernel answers the right query.
    pub fn detect() -> Self {
        match sys::ultar_rights() {
            Ok(_) => Self::Ultar,
            Err(_) => Self::Linux,
        }
    }
}

/// Resource limits of a service. `None` leaves a limit as inherited.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limits {
    /// Address space in bytes (`RLIMIT_AS`).
    pub address_space: Option<u64>,
    /// Open file descriptors (`RLIMIT_NOFILE`).
    pub open_files: Option<u64>,
    /// Processes and threads of the service's user (`RLIMIT_NPROC`).
    pub processes: Option<u64>,
    /// Whether core dumps stay allowed; `false` sets `RLIMIT_CORE` to 0
    /// (a core file would contain secrets and user data).
    pub core_dumps: bool,
}

impl Limits {
    /// The limits of an ordinary small daemon: 256 descriptors, 128
    /// threads, no core dumps. **No address-space limit:** the C library
    /// reserves address space per thread (arenas of 64 MiB), so a limit
    /// low enough to matter breaks thread starts long before memory is
    /// short (review round 3, R3-B0). Memory is bounded by the service's
    /// group instead (`memory.max` in its service file; the Ultar kernel
    /// enforces it since its memory groups).
    pub const STANDARD: Limits = Limits {
        address_space: None,
        open_files: Some(256),
        processes: Some(128),
        core_dumps: false,
    };
}

/// One hardening step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// Close descriptors inherited from the parent.
    CloseDescriptors,
    /// Set the file creation mask.
    Umask,
    /// Change the working directory to `/`.
    WorkingDirectory,
    /// Refuse to run as root.
    RefuseRoot,
    /// Forbid privilege gain through `execve`.
    NoNewPrivileges,
    /// A resource limit.
    Limit(Resource),
    /// Drop undeclared kernel rights.
    Rights,
}

impl Step {
    /// Stable key for logs.
    pub const fn key(self) -> &'static str {
        match self {
            Self::CloseDescriptors => "close_fds",
            Self::Umask => "umask",
            Self::WorkingDirectory => "workdir",
            Self::RefuseRoot => "refuse_root",
            Self::NoNewPrivileges => "no_new_privs",
            Self::Limit(Resource::AddressSpace) => "limit_as",
            Self::Limit(Resource::OpenFiles) => "limit_nofile",
            Self::Limit(Resource::Processes) => "limit_nproc",
            Self::Limit(Resource::Core) => "limit_core",
            Self::Rights => "rights",
        }
    }
}

/// What happened to a step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Done and, where the kernel allows it, read back.
    Applied,
    /// Turned off by the service's own declaration.
    Disabled,
    /// The running kernel has no such mechanism (see the module table).
    Unsupported,
}

/// Result of [`Hardening::apply`]: one entry per step, in order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HardeningReport {
    /// The detected kernel.
    pub platform: Platform,
    /// Steps and what happened to them.
    pub steps: Vec<(Step, Outcome)>,
    /// Inherited descriptors that were closed.
    pub closed_descriptors: usize,
}

impl HardeningReport {
    /// The outcome of `step`, if it is part of the report.
    pub fn outcome(&self, step: Step) -> Option<Outcome> {
        self.steps
            .iter()
            .find(|(candidate, _)| *candidate == step)
            .map(|(_, outcome)| *outcome)
    }

    /// Steps the kernel could not carry out.
    pub fn unsupported(&self) -> impl Iterator<Item = Step> + '_ {
        self.steps
            .iter()
            .filter(|(_, outcome)| *outcome == Outcome::Unsupported)
            .map(|(step, _)| *step)
    }
}

/// Why hardening stopped the start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HardeningError {
    /// The process runs as root and the service did not declare that.
    RunningAsRoot,
    /// The kernel refused a step.
    Refused(Step, Errno),
    /// A step reported success but reading back shows another value.
    NotEffective(Step),
}

impl HardeningError {
    /// The step that failed.
    pub const fn step(&self) -> Step {
        match self {
            Self::RunningAsRoot => Step::RefuseRoot,
            Self::Refused(step, _) | Self::NotEffective(step) => *step,
        }
    }
}

impl fmt::Display for HardeningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RunningAsRoot => f.write_str("refusing to run as root (not declared)"),
            Self::Refused(step, errno) => {
                write!(f, "hardening step {} refused: {errno}", step.key())
            }
            Self::NotEffective(step) => write!(f, "hardening step {} had no effect", step.key()),
        }
    }
}

impl std::error::Error for HardeningError {}

/// What a service applies to itself at start. See the module documentation.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hardening {
    /// Close every inherited descriptor above the standard streams that the
    /// service did not declare (`ServiceSpec::inherited`). Carried out by
    /// the runtime before anything else, not by [`Hardening::apply`].
    pub close_descriptors: bool,
    /// File creation mask; `Some(0o077)`: new files are private.
    pub umask: Option<u32>,
    /// Change the working directory to `/`.
    pub working_directory: bool,
    /// The service declares that it has to run as root.
    pub allow_root: bool,
    /// Forbid privilege gain through `execve`.
    pub no_new_privileges: bool,
    /// Resource limits; `None` turns the whole group off.
    pub limits: Option<Limits>,
    /// Kernel rights the service declares it needs; every other right is
    /// dropped. `None` leaves the rights untouched.
    pub rights: Option<RightSet>,
}

impl Default for Hardening {
    fn default() -> Self {
        Self {
            close_descriptors: true,
            umask: Some(0o077),
            working_directory: true,
            allow_root: false,
            no_new_privileges: true,
            limits: Some(Limits::STANDARD),
            rights: Some(RightSet::NONE),
        }
    }
}

/// Upper end of the descriptor numbers that are closed when the soft limit
/// is larger (a table that large is not scanned at every start).
pub const CLOSE_SCAN_MAX: u64 = 65_536;

impl Hardening {
    /// Applies every step except the closing of inherited descriptors (the
    /// runtime does that first, see `Runtime::start`). Call before threads
    /// are spawned.
    pub fn apply(&self) -> Result<HardeningReport, HardeningError> {
        let platform = Platform::detect();
        let mut report = HardeningReport {
            platform,
            steps: Vec::new(),
            closed_descriptors: 0,
        };
        self.files(&mut report)?;
        self.identity(&mut report)?;
        if let Some(limits) = &self.limits {
            for (resource, value) in limit_plan(limits) {
                let outcome = apply_limit(platform, resource, value)?;
                report.steps.push((Step::Limit(resource), outcome));
            }
        } else {
            for resource in ALL_RESOURCES {
                report
                    .steps
                    .push((Step::Limit(resource), Outcome::Disabled));
            }
        }
        report
            .steps
            .push((Step::Rights, self.drop_rights(platform)?));
        Ok(report)
    }

    fn files(&self, report: &mut HardeningReport) -> Result<(), HardeningError> {
        match self.umask {
            Some(mask) => {
                sys::set_umask(mask);
                // Reading the mask means setting it; set it twice and
                // compare what the second call reports.
                if sys::set_umask(mask) != mask & 0o777 {
                    return Err(HardeningError::NotEffective(Step::Umask));
                }
                report.steps.push((Step::Umask, Outcome::Applied));
            }
            None => report.steps.push((Step::Umask, Outcome::Disabled)),
        }
        if self.working_directory {
            std::env::set_current_dir("/").map_err(|error| {
                let errno = Errno(error.raw_os_error().unwrap_or(0));
                HardeningError::Refused(Step::WorkingDirectory, errno)
            })?;
            report
                .steps
                .push((Step::WorkingDirectory, Outcome::Applied));
        } else {
            report
                .steps
                .push((Step::WorkingDirectory, Outcome::Disabled));
        }
        Ok(())
    }

    fn identity(&self, report: &mut HardeningReport) -> Result<(), HardeningError> {
        let root = match (self.allow_root, report.platform) {
            (true, _) => Outcome::Disabled,
            // No user ids: every process reports uid 0. Rights are the
            // privilege model there (see `drop_rights`).
            (false, Platform::Ultar) => Outcome::Unsupported,
            (false, Platform::Linux) => {
                if sys::euid() == 0 || sys::uid() == 0 {
                    return Err(HardeningError::RunningAsRoot);
                }
                Outcome::Applied
            }
        };
        report.steps.push((Step::RefuseRoot, root));
        let privileges = if self.no_new_privileges {
            sys::set_no_new_privileges()
                .map_err(|errno| HardeningError::Refused(Step::NoNewPrivileges, errno))?;
            Outcome::Applied
        } else {
            Outcome::Disabled
        };
        report.steps.push((Step::NoNewPrivileges, privileges));
        Ok(())
    }

    fn drop_rights(&self, platform: Platform) -> Result<Outcome, HardeningError> {
        let Some(keep) = self.rights else {
            return Ok(Outcome::Disabled);
        };
        if platform == Platform::Linux {
            // Note: Linux has no Ultar rights. Its equivalent (capability
            // bounding set) only matters for root, which is refused above.
            return Ok(Outcome::Unsupported);
        }
        let refused = |errno| HardeningError::Refused(Step::Rights, errno);
        sys::ultar_drop_rights(keep.drop_mask()).map_err(refused)?;
        let left = sys::ultar_rights().map_err(refused)?;
        if left & !keep.bits() != 0 {
            return Err(HardeningError::NotEffective(Step::Rights));
        }
        Ok(Outcome::Applied)
    }
}

const ALL_RESOURCES: [Resource; 4] = [
    Resource::Core,
    Resource::AddressSpace,
    Resource::OpenFiles,
    Resource::Processes,
];

/// The limits to set, in order; `None` entries are left out.
fn limit_plan(limits: &Limits) -> Vec<(Resource, u64)> {
    let mut plan = Vec::new();
    if !limits.core_dumps {
        plan.push((Resource::Core, 0));
    }
    let optional = [
        (Resource::AddressSpace, limits.address_space),
        (Resource::OpenFiles, limits.open_files),
        (Resource::Processes, limits.processes),
    ];
    plan.extend(
        optional
            .into_iter()
            .filter_map(|(resource, value)| value.map(|value| (resource, value))),
    );
    plan
}

/// Sets soft = hard = `value` (never above the inherited hard limit, which
/// an unprivileged process cannot raise) and reads the result back.
fn apply_limit(
    platform: Platform,
    resource: Resource,
    value: u64,
) -> Result<Outcome, HardeningError> {
    let step = Step::Limit(resource);
    let refused = |errno| HardeningError::Refused(step, errno);
    let (_, hard) = sys::limit(resource).map_err(refused)?;
    let target = value.min(hard);
    sys::set_limit(resource, target, target).map_err(refused)?;
    let now = sys::limit(resource).map_err(refused)?;
    if now == (target, target) {
        return Ok(Outcome::Applied);
    }
    match platform {
        // `sys_prlimit` of the Ultar kernel accepts and ignores new values.
        Platform::Ultar => Ok(Outcome::Unsupported),
        Platform::Linux => Err(HardeningError::NotEffective(step)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_everything_on_and_no_rights() {
        let hardening = Hardening::default();
        assert!(hardening.close_descriptors && hardening.working_directory);
        assert!(hardening.no_new_privileges && !hardening.allow_root);
        assert_eq!(hardening.umask, Some(0o077));
        assert_eq!(hardening.limits, Some(Limits::STANDARD));
        assert_eq!(hardening.rights, Some(RightSet::NONE));
        assert_eq!(
            hardening.limits.map(|limits| limits.core_dumps),
            Some(false)
        );
    }

    #[test]
    fn the_limit_plan_follows_the_declaration() {
        let plan = limit_plan(&Limits::STANDARD);
        assert_eq!(
            plan,
            vec![
                (Resource::Core, 0),
                (Resource::OpenFiles, 256),
                (Resource::Processes, 128),
            ]
        );
        let bounded = Limits {
            address_space: Some(1 << 30),
            ..Limits::STANDARD
        };
        assert_eq!(limit_plan(&bounded)[1], (Resource::AddressSpace, 1 << 30));
        let open = Limits {
            address_space: None,
            open_files: Some(9),
            processes: None,
            core_dumps: true,
        };
        assert_eq!(limit_plan(&open), vec![(Resource::OpenFiles, 9)]);
    }

    #[test]
    fn the_test_machine_is_linux() {
        assert_eq!(Platform::detect(), Platform::Linux);
    }

    #[test]
    fn report_lookup_and_error_texts() {
        let report = HardeningReport {
            platform: Platform::Ultar,
            steps: vec![
                (Step::Umask, Outcome::Applied),
                (Step::RefuseRoot, Outcome::Unsupported),
                (Step::Limit(Resource::Core), Outcome::Unsupported),
            ],
            closed_descriptors: 0,
        };
        assert_eq!(report.outcome(Step::Umask), Some(Outcome::Applied));
        assert_eq!(report.outcome(Step::Rights), None);
        assert_eq!(report.unsupported().count(), 2);
        let errors = [
            HardeningError::RunningAsRoot,
            HardeningError::Refused(Step::Umask, Errno::EPERM),
            HardeningError::NotEffective(Step::Limit(Resource::OpenFiles)),
        ];
        for error in errors {
            assert!(!error.to_string().is_empty());
            assert!(!error.step().key().is_empty());
        }
        let mut keys: Vec<&str> = [
            Step::CloseDescriptors,
            Step::Umask,
            Step::WorkingDirectory,
            Step::RefuseRoot,
            Step::NoNewPrivileges,
            Step::Rights,
        ]
        .iter()
        .map(|step| step.key())
        .collect();
        keys.extend(ALL_RESOURCES.iter().map(|r| Step::Limit(*r).key()));
        let unique: std::collections::BTreeSet<&str> = keys.iter().copied().collect();
        assert_eq!(unique.len(), keys.len());
    }
}
