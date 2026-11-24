//! `ultar-audiod` – the audio service (see the library documentation).
//!
//! ```text
//! ultar-audiod --bus PATH --state DIR [--root DIR] [--log-socket PATH] [--token-file PATH]
//! ```
//! `--root` is the directory the sound devices are found below (`/`).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_audiod::AudioService;
use ultar_hal_linux::SoundCards;
use ultar_svckit::msg::Msg;

fn main() -> ExitCode {
    ultar_svckit::main("ultar-audiod", &["state", "root"], None, |args, _| {
        let state = args.required_path("state").map_err(|_| Msg::BadArguments)?;
        let root = args
            .path("root")
            .map_err(|_| Msg::BadArguments)?
            .unwrap_or_else(|| "/".into());
        Ok(AudioService::new(Box::new(SoundCards::new(root)), state))
    })
}
