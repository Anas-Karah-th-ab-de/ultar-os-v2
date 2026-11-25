//! `ultar-agentd` – the local helper (see the library documentation).
//!
//! ```text
//! ultar-agentd --bus PATH [--state DIR] [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_agentd::AgentService;

fn main() -> ExitCode {
    // No kernel right: everything it does is a bus call. Its one file is the list of
    // what it did, in its own folder, for the person to read.
    ultar_svckit::main("ultar-agentd", &["state"], None, |args, _| {
        let state = args.option("state").map(std::path::PathBuf::from);
        Ok(AgentService::new(state.as_deref()))
    })
}
