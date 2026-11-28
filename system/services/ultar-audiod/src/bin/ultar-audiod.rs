//! `ultar-audiod` – the sound server (see the library documentation).
//!
//! ```text
//! ultar-audiod --bus PATH --state DIR [--root DIR] [--log-socket PATH] [--token-file PATH]
//!              [--policy FILE --stream-dir DIR --streams ID[:GID],...]
//!              [--device NODE | --play-file FILE --capture-file FILE]
//! ```
//! * `--root` is the directory the sound cards' controls are found below (`/`).
//! * `--device` is the kernel's card (`/dev/ultar/pcm`); with it the service keeps the
//!   kernel right `sound` and is the only holder of the card. `--play-file` and
//!   `--capture-file` make two plain files the card (development, tests).
//! * `--streams` lists the programs that get a stream socket below `--stream-dir`, one
//!   socket per identity (group `GID` may connect); what each may do comes from the bus
//!   policy `--policy` (`audio.play`, `audio.ring`, `audio.call`, `audio.capture`).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_audiod::device::{Device, FileDevice, NodeDevice};
use ultar_audiod::engine::{Engine, Opener};
use ultar_audiod::sound::listeners;
use ultar_audiod::AudioService;
use ultar_bus::Policy;
use ultar_hal_linux::SoundCards;
use ultar_service::harden::{Right, RightSet};
use ultar_svckit::msg::Msg;
use ultar_svckit::CommonArgs;

/// The card as the command line names it (`None`: no sound server, only the controls).
fn opener(args: &CommonArgs) -> Result<Option<Opener>, Msg> {
    let bad = |_| Msg::BadArguments;
    if let Some(node) = args.path("device").map_err(bad)? {
        return Ok(Some(Box::new(move || {
            NodeDevice::open(&node).map(|d| Box::new(d) as Box<dyn Device>)
        })));
    }
    match (
        args.path("play-file").map_err(bad)?,
        args.path("capture-file").map_err(bad)?,
    ) {
        (Some(play), Some(capture)) => Ok(Some(Box::new(move || {
            FileDevice::open(&play, &capture).map(|d| Box::new(d) as Box<dyn Device>)
        }))),
        (None, None) => Ok(None),
        _ => Err(Msg::BadArguments),
    }
}

fn engine(args: &CommonArgs) -> Result<Option<Engine>, Msg> {
    let Some(opener) = opener(args)? else {
        return Ok(None);
    };
    let specs = match args.option("streams") {
        Some(list) => {
            let dir = args.required_path("stream-dir").map_err(|_| Msg::BadArguments)?;
            let policy = args.required_path("policy").map_err(|_| Msg::BadArguments)?;
            let policy = Policy::load(&policy).map_err(|_| Msg::State)?;
            listeners(&dir, list, &policy).map_err(|_| Msg::BadArguments)?
        }
        None => Vec::new(),
    };
    Engine::new(specs, opener).map(Some).map_err(|_| Msg::State)
}

fn main() -> ExitCode {
    // The kernel right to the sound card is kept only by a service that holds the card.
    let holds_card = std::env::args().any(|a| a == "--device");
    let rights = holds_card.then(|| RightSet::of(&[Right::Sound]));
    ultar_svckit::main(
        "ultar-audiod",
        &[
            "state",
            "root",
            "policy",
            "device",
            "play-file",
            "capture-file",
            "streams",
            "stream-dir",
        ],
        rights,
        |args, _| {
            let state = args.required_path("state").map_err(|_| Msg::BadArguments)?;
            let root = args
                .path("root")
                .map_err(|_| Msg::BadArguments)?
                .unwrap_or_else(|| "/".into());
            let service = AudioService::new(Box::new(SoundCards::new(root)), state);
            Ok(match engine(args)? {
                Some(engine) => service.with_sound(engine),
                None => service,
            })
        },
    )
}
