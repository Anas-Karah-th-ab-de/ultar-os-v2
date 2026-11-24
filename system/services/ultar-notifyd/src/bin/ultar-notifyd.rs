//! `ultar-notifyd` – the notification service (see the library documentation).
//!
//! ```text
//! ultar-notifyd --bus PATH --policy FILE [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_bus::messages::notify::SERVICE;
use ultar_bus::Policy;
use ultar_notifyd::NotifyService;
use ultar_svckit::msg::Msg;
use ultar_svckit::Access;

fn main() -> ExitCode {
    ultar_svckit::main("ultar-notifyd", &["policy"], None, |args, _| {
        let policy = args
            .required_path("policy")
            .map_err(|_| Msg::BadArguments)?;
        let policy = Policy::load(&policy).map_err(|_| Msg::State)?;
        Ok(NotifyService::new(Access::new(SERVICE, Some(policy))))
    })
}
