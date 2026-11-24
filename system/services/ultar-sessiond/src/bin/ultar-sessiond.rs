//! `ultar-sessiond` – the session service (see the library documentation).
//!
//! ```text
//! ultar-sessiond --bus PATH --policy FILE --state DIR [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_bus::Policy;
use ultar_sessiond::SessionService;
use ultar_svckit::msg::Msg;

fn main() -> ExitCode {
    ultar_svckit::main("ultar-sessiond", &["policy", "state"], None, |args, _| {
        let policy = args
            .required_path("policy")
            .map_err(|_| Msg::BadArguments)?;
        let state = args.required_path("state").map_err(|_| Msg::BadArguments)?;
        let policy = Policy::load(&policy).map_err(|_| Msg::State)?;
        Ok(SessionService::new(state, Some(policy)))
    })
}
