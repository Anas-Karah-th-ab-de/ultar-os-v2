//! `ultar-mediad` – the media service (see the library documentation).
//!
//! ```text
//! ultar-mediad --bus PATH --policy FILE [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_bus::messages::media::SERVICE;
use ultar_bus::Policy;
use ultar_mediad::MediaService;
use ultar_svckit::msg::Msg;
use ultar_svckit::Access;

fn main() -> ExitCode {
    ultar_svckit::main("ultar-mediad", &["policy"], None, |args, _| {
        let policy = args
            .required_path("policy")
            .map_err(|_| Msg::BadArguments)?;
        let policy = Policy::load(&policy).map_err(|_| Msg::State)?;
        Ok(MediaService::new(Access::new(SERVICE, Some(policy))))
    })
}
