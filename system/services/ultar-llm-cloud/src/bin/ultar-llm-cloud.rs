//! `ultar-llm-cloud` – a provider's language model on the bus (see the library).
//!
//! ```text
//! ultar-llm-cloud --bus PATH --state DIR [--endpoint URL] [--model ID] [--api-version V]
//!                 [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use std::sync::Arc;
use ultar_llm_cloud::key::KeyStore;
use ultar_llm_cloud::provider::Claude;
use ultar_llm_cloud::LlmCloud;
use ultar_svckit::msg::Msg;

fn main() -> ExitCode {
    // No kernel right: one bus connection, its own folder, and the way out to the provider.
    let options = ["state", "endpoint", "model", "api-version"];
    ultar_svckit::main("ultar-llm-cloud", &options, None, |args, _| {
        let state = args.required_path("state").map_err(|_| Msg::BadArguments)?;
        let mut claude = Claude::default();
        let set = |field: &mut String, name: &str| {
            if let Some(value) = args.option(name) {
                value.clone_into(field);
            }
        };
        set(&mut claude.endpoint, "endpoint");
        set(&mut claude.model, "model");
        set(&mut claude.version, "api-version");
        Ok(LlmCloud::new(KeyStore::new(&state), Arc::new(claude)))
    })
}
