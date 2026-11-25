//! `ultar-llm-cloud` – a provider's language model on the bus (see the library).
//!
//! ```text
//! ultar-llm-cloud --bus PATH --state DIR --seal DIR [--claude-endpoint URL]
//!                 [--claude-api-version V] [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::ExitCode;
use std::sync::Arc;
use ultar_llm_cloud::config::ConfigFile;
use ultar_llm_cloud::key::KeyStore;
use ultar_llm_cloud::provider::{Claude, Compatible, Provider};
use ultar_llm_cloud::LlmCloud;
use ultar_svckit::msg::Msg;

fn main() -> ExitCode {
    // No kernel right: one bus connection, its two folders, and the way out to the provider.
    let options = ["state", "seal", "claude-endpoint", "claude-api-version"];
    ultar_svckit::main("ultar-llm-cloud", &options, None, |args, _| {
        let state = args.required_path("state").map_err(|_| Msg::BadArguments)?;
        let seal = args.required_path("seal").map_err(|_| Msg::BadArguments)?;
        let mut claude = Claude::default();
        if let Some(endpoint) = args.option("claude-endpoint") {
            endpoint.clone_into(&mut claude.endpoint);
        }
        if let Some(version) = args.option("claude-api-version") {
            version.clone_into(&mut claude.version);
        }
        let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(claude), Arc::new(Compatible)];
        let keys = KeyStore::new(&state, &seal);
        Ok(LlmCloud::new(keys, ConfigFile::new(&state), providers))
    })
}
