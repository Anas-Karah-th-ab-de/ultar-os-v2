//! `ultar-mediad` - the media service (see the library documentation).
//!
//! ```text
//! ultar-mediad --bus PATH --policy FILE [--there-sink PCM] [--log-socket PATH] [--token-file PATH]
//! ```
//! `--there-sink` is the sound card's stream (`/dev/ultar/pcm`): this device then plays
//! the sound of another device's media session ("play there"); it needs the right `sound`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_bus::messages::media::SERVICE;
use ultar_bus::Policy;
use ultar_mediad::there::CardSink;
use ultar_mediad::MediaService;
use ultar_service::harden::{Right, RightSet};
use ultar_svckit::msg::Msg;
use ultar_svckit::Access;

fn main() -> ExitCode {
    // The sound right is kept only by a device that plays for others.
    let plays = std::env::args().any(|a| a == "--there-sink");
    let rights = plays.then(|| RightSet::of(&[Right::Sound]));
    ultar_svckit::main(
        "ultar-mediad",
        &["policy", "there-sink"],
        rights,
        |args, _| {
            let policy = args
                .required_path("policy")
                .map_err(|_| Msg::BadArguments)?;
            let policy = Policy::load(&policy).map_err(|_| Msg::State)?;
            let service = MediaService::new(Access::new(SERVICE, Some(policy)));
            Ok(
                match args.path("there-sink").map_err(|_| Msg::BadArguments)? {
                    Some(node) => service.with_sink(Box::new(CardSink::new(node))),
                    None => service,
                },
            )
        },
    )
}
