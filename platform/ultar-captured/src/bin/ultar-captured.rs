//! `ultar-captured` – the screenshot service (see the library documentation).
//!
//! ```text
//! ultar-captured --bus PATH --compositor PATH --homes DIR [--no-chown] [--log-socket PATH] [--token-file PATH]
//! ```
//! `--no-chown yes`: leave files owned by the service (tests that run as one user).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_captured::CaptureService;
use ultar_service::harden::{Right, RightSet}; // physical: a kernel right, not a side
use ultar_svckit::msg::Msg;

fn main() -> ExitCode {
    // The one kernel right: writing into a person's home (0700) and handing the
    // file to the person (chown). Nothing else of this service needs it.
    let rights = Some(RightSet::of(&[Right::FileAdmin])); // physical: a kernel right
    ultar_svckit::main(
        "ultar-captured",
        &["compositor", "homes", "no-chown"],
        rights,
        |args, _| {
            let compositor = args
                .required_path("compositor")
                .map_err(|_| Msg::BadArguments)?;
            let homes = args.required_path("homes").map_err(|_| Msg::BadArguments)?;
            let chown = match args.option("no-chown") {
                None => true,
                Some("yes") => false,
                Some(_) => return Err(Msg::BadArguments),
            };
            Ok(CaptureService::new(compositor, homes, chown))
        },
    )
}
