//! `ultar-powerd` – the power service (see the library documentation).
//!
//! ```text
//! ultar-powerd --bus PATH --policy FILE [--root DIR] [--log-socket PATH] [--token-file PATH]
//! ```
//! `--root` is the directory the kernel's attribute files are read below (`/`).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_bus::messages::power::SERVICE;
use ultar_bus::Policy;
use ultar_hal_linux::LinuxPower;
use ultar_powerd::PowerService;
use ultar_svckit::msg::Msg;
use ultar_svckit::Access;

fn main() -> ExitCode {
    ultar_svckit::main("ultar-powerd", &["policy", "root"], None, |args, _| {
        let policy = args
            .required_path("policy")
            .map_err(|_| Msg::BadArguments)?;
        let root = args
            .path("root")
            .map_err(|_| Msg::BadArguments)?
            .unwrap_or_else(|| "/".into());
        let policy = Policy::load(&policy).map_err(|_| Msg::State)?;
        Ok(PowerService::new(
            LinuxPower::new(root),
            Access::new(SERVICE, Some(policy)),
        ))
    })
}
