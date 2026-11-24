//! `ultar-btd` – the Bluetooth service (see the library documentation).
//!
//! ```text
//! ultar-btd --bus PATH --state DIR [--root DIR] [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_btd::BtService;
use ultar_service::harden::{Right, RightSet};
use ultar_svckit::msg::Msg;

fn main() -> ExitCode {
    // The one kernel right: the raw HCI user channel of the controller (Linux:
    // CAP_NET_ADMIN). The Ultar kernel has no Bluetooth socket; the right opens nothing there.
    let rights = Some(RightSet::of(&[Right::NetAdmin]));
    ultar_svckit::main("ultar-btd", &["state", "root"], rights, |args, _| {
        let state = args.required_path("state").map_err(|_| Msg::BadArguments)?;
        let root = args
            .path("root")
            .map_err(|_| Msg::BadArguments)?
            .unwrap_or_else(|| "/".into());
        Ok(BtService::new(root, state))
    })
}
