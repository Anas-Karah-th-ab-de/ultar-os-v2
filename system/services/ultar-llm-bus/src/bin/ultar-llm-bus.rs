//! `ultar-llm-bus` – the language model on the system bus (see the library documentation).
//!
//! ```text
//! ultar-llm-bus --bus PATH --socket PATH [--place-file PATH] [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use ultar_llm_bus::backend::Backend;
use ultar_llm_bus::LlmBus;
use ultar_svckit::msg::Msg;

fn main() -> ExitCode {
    // No kernel right: one bus connection and the model service's socket.
    ultar_svckit::main(
        "ultar-llm-bus",
        &["socket", "place-file"],
        None,
        |args, _| {
            let socket = args
                .required_path("socket")
                .map_err(|_| Msg::BadArguments)?;
            let place = args.path("place-file").map_err(|_| Msg::BadArguments)?;
            let service = LlmBus::new(Backend::new(&socket));
            Ok(match place {
                Some(file) => service.with_place_file(file),
                None => service,
            })
        },
    )
}
