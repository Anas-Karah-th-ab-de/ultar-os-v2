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
    // Kernel rights: the raw HCI user channel of the controller (Linux: CAP_NET_ADMIN), the
    // raw HCI node of a USB controller and virtual input devices (Ultar kernel).
    let rights = Some(RightSet::of(&[
        Right::NetAdmin,
        Right::Bluetooth,
        Right::InputInject,
    ]));
    ultar_svckit::main("ultar-btd", &["state", "root"], rights, |args, _| {
        let state = args.required_path("state").map_err(|_| Msg::BadArguments)?;
        let root = args
            .path("root")
            .map_err(|_| Msg::BadArguments)?
            .unwrap_or_else(|| "/".into());
        Ok(BtService::new(root, state))
    })
}
