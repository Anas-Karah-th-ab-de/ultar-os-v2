//! `ultar-mediad` - the media service (see the library documentation).
//!
//! ```text
//! ultar-mediad --bus PATH --policy FILE [--there-sink SOCKET] [--log-socket PATH] [--token-file PATH]
//! ```
//! `--there-sink` is the sound server's stream socket (`/run/ultar/audio/system.mediad.sock`): this
//! device then plays the sound of another device's media session ("play there"). The
//! service holds no kernel right: `ultar-audiod` owns the card.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_bus::messages::media::SERVICE;
use ultar_bus::Policy;
use ultar_mediad::there::StreamSink;
use ultar_mediad::MediaService;
use ultar_svckit::msg::Msg;
use ultar_svckit::Access;

fn main() -> ExitCode {
    ultar_svckit::main(
        "ultar-mediad",
        &["policy", "there-sink"],
        None,
        |args, _| {
            let policy = args
                .required_path("policy")
                .map_err(|_| Msg::BadArguments)?;
            let policy = Policy::load(&policy).map_err(|_| Msg::State)?;
            let service = MediaService::new(Access::new(SERVICE, Some(policy)));
            Ok(
                match args.path("there-sink").map_err(|_| Msg::BadArguments)? {
                    Some(node) => service.with_sink(Box::new(StreamSink::new(node))),
                    None => service,
                },
            )
        },
    )
}
