//! `ultar-agentd` – the local helper (see the library documentation).
//!
//! ```text
//! ultar-agentd --bus PATH [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_agentd::AgentService;

fn main() -> ExitCode {
    // No kernel right and no file of its own: everything it does is a bus call.
    ultar_svckit::main(
        "ultar-agentd",
        &[],
        None,
        |_, _| Ok(AgentService::default()),
    )
}
