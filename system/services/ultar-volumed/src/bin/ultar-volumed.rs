//! `ultar-volumed` – the removable-drive service (see the library documentation).
//!
//! ```text
//! ultar-volumed --bus PATH --media DIR [--root DIR] [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_service::harden::{Right, RightSet};
use ultar_svckit::msg::Msg;
use ultar_volumed::VolumeService;

fn main() -> ExitCode {
    // The one kernel right: mounting and unmounting the drives.
    let rights = Some(RightSet::of(&[Right::Mount]));
    ultar_svckit::main("ultar-volumed", &["media", "root"], rights, |args, _| {
        let media = args.required_path("media").map_err(|_| Msg::BadArguments)?;
        let root = args
            .path("root")
            .map_err(|_| Msg::BadArguments)?
            .unwrap_or_else(|| "/".into());
        Ok(VolumeService::new(root, media))
    })
}
