//! `ultar-voiced` – the voice control service (see the library documentation).
//!
//! ```text
//! ultar-voiced --bus PATH --state DIR [--models DIR] [--whisper PATH] [--espeak PATH]
//!              [--espeak-data DIR] [--port N] [--threads N] [--log-socket PATH] [--token-file PATH]
//! ```
//! `--models` holds the `ggml-<name>.bin` files, `--whisper` is the speech-to-text server
//! program and `--espeak` the speech output program of the packages `whisper` and `espeak-ng`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use ultar_svckit::msg::Msg;
use ultar_voiced::espeak::EspeakSpeaker;
use ultar_voiced::models::ModelCatalog;
use ultar_voiced::state::Store;
use ultar_voiced::whisper::WhisperServer;
use ultar_voiced::VoiceService;

/// Where the packages put their programs.
const WHISPER: &str = "/pkg/whisper/bin/whisper-server";
const ESPEAK: &str = "/pkg/espeak-ng/bin/espeak-ng";
const MODELS: &str = "/data/ultar/voice/models";
/// The loopback port of the speech-to-text server.
const PORT: u16 = 38_080;
const THREADS: u32 = 4;

fn main() -> ExitCode {
    let options = [
        "state",
        "models",
        "whisper",
        "espeak",
        "espeak-data",
        "port",
        "threads",
    ];
    ultar_svckit::main("ultar-voiced", &options, None, |args, _| {
        let state = args.required_path("state").map_err(|_| Msg::BadArguments)?;
        let path = |name: &str, default: &str| -> Result<PathBuf, Msg> {
            Ok(args
                .path(name)
                .map_err(|_| Msg::BadArguments)?
                .unwrap_or_else(|| default.into()))
        };
        let number = |name: &str| -> Result<Option<u32>, Msg> {
            args.option(name)
                .map(|v| v.parse::<u32>().map_err(|_| Msg::BadArguments))
                .transpose()
        };
        let models = ModelCatalog::new(&path("models", MODELS)?);
        let port = number("port")?
            .map(|p| u16::try_from(p).map_err(|_| Msg::BadArguments))
            .transpose()?
            .unwrap_or(PORT);
        let stt = WhisperServer::new(
            path("whisper", WHISPER)?,
            models.clone(),
            port,
            number("threads")?.unwrap_or(THREADS),
        );
        let tts = EspeakSpeaker::new(
            path("espeak", ESPEAK)?,
            args.path("espeak-data").map_err(|_| Msg::BadArguments)?,
        );
        Ok(VoiceService::new(
            Store::new(&state),
            Arc::new(stt),
            Arc::new(tts),
            models,
        ))
    })
}
