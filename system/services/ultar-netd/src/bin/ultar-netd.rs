//! `ultar-netd` – the network service (see the library documentation).
//!
//! ```text
//! ultar-netd --bus PATH --state DIR [--root DIR] [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_netd::NetService;
use ultar_svckit::msg::Msg;

fn main() -> ExitCode {
    ultar_svckit::main("ultar-netd", &["state", "root"], None, |args, _| {
        let state = args.required_path("state").map_err(|_| Msg::BadArguments)?;
        let root = args
            .path("root")
            .map_err(|_| Msg::BadArguments)?
            .unwrap_or_else(|| "/".into());
        Ok(NetService::new(root, state))
    })
}
