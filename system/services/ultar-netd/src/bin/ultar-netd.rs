//! `ultar-netd` – the network service (see the library documentation).
//!
//! ```text
//! ultar-netd --bus PATH --state DIR [--run DIR] [--root DIR] [--log-socket PATH] [--token-file PATH]
//! ```
//!
//! `--run DIR`: where `resolv.conf` is written (left alone without it).
//! The VPN (`vpn` on the bus, `ultar_vpnd`) runs in the same process on its own
//! bus connection, its tunnels below `--state DIR/vpn`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_netd::NetService;
use ultar_service::harden::{Right, RightSet};
use ultar_svckit::msg::Msg;
use ultar_vpnd::VpnService;

fn main() -> ExitCode {
    // Joining networks configures addresses and routes: the network right.
    let rights = Some(RightSet::of(&[Right::NetAdmin]));
    ultar_svckit::main_pair(
        "ultar-netd",
        &["state", "root", "run"],
        rights,
        |args, _| {
            let state = args.required_path("state").map_err(|_| Msg::BadArguments)?;
            let root = args
                .path("root")
                .map_err(|_| Msg::BadArguments)?
                .unwrap_or_else(|| "/".into());
            let run = args.path("run").map_err(|_| Msg::BadArguments)?;
            let vpn = VpnService::new(root.clone(), state.join("vpn"));
            Ok((NetService::new(root, state).with_run_dir(run), vpn))
        },
    )
}
