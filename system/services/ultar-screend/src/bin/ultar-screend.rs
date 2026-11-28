//! `ultar-screend` - the screen service (see the library documentation).
//!
//! ```text
//! ultar-screend --bus PATH [--compositor PATH] [--root DIR] [--log-socket PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_screend::screen::socket;
use ultar_screend::ScreendService;
use ultar_service::harden::{Right, RightSet};
use ultar_svckit::msg::Msg;

fn main() -> ExitCode {
    // Kernel right: virtual input devices for the viewer's pointer and keys.
    let rights = Some(RightSet::of(&[Right::InputInject]));
    ultar_svckit::main(
        "ultar-screend",
        &["compositor", "root"],
        rights,
        |args, _| {
            let compositor = args.path("compositor").map_err(|_| Msg::BadArguments)?;
            let root = args
                .path("root")
                .map_err(|_| Msg::BadArguments)?
                .unwrap_or_else(|| "/".into());
            Ok(ScreendService::new(socket(compositor), root))
        },
    )
}
