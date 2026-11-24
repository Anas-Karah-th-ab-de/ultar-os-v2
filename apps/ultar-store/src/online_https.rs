//! The real HTTPS client of the online source (feature `online`).
//!
//! Pure Rust: `ureq` with rustls and the Graviola crypto provider (the same stack as the system
//! updater), certificates always validated against the Mozilla roots compiled in, HTTPS only,
//! no redirects, no proxy taken from the environment, fixed timeouts.

use crate::online::{Fetch, FetchError};
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

/// Connect/response/read timeout of every request.
const TIMEOUT: Duration = Duration::from_secs(20);

/// True when this CPU can run the TLS provider (Graviola) – the same check as the updater's
/// `tls_available`; on other CPUs the store stays offline instead of stopping.
pub fn tls_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("aes")
            && std::arch::is_x86_feature_detected!("pclmulqdq")
            && std::arch::is_x86_feature_detected!("bmi1")
            && std::arch::is_x86_feature_detected!("adx")
            && std::arch::is_x86_feature_detected!("avx")
            && std::arch::is_x86_feature_detected!("avx2")
    }
    #[cfg(target_arch = "aarch64")]
    {
        std::arch::is_aarch64_feature_detected!("neon")
            && std::arch::is_aarch64_feature_detected!("aes")
            && std::arch::is_aarch64_feature_detected!("pmull")
            && std::arch::is_aarch64_feature_detected!("sha2")
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        false
    }
}

/// HTTPS `GET` with ureq.
pub struct UreqFetch {
    agent: ureq::Agent,
}

impl UreqFetch {
    /// The client; `None` on a CPU without the TLS provider's features.
    pub fn new() -> Option<UreqFetch> {
        if !tls_available() {
            return None;
        }
        let tls = TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .unversioned_rustls_crypto_provider(Arc::new(rustls_graviola::default_provider()))
            .root_certs(RootCerts::WebPki)
            .disable_verification(false)
            .build();
        let agent_name = ultar_brand::release::user_agent("ultar-store", env!("CARGO_PKG_VERSION"))
            .map(|l| l.as_str().to_string())
            .unwrap_or_else(|_| "ultar-store".to_string());
        let config = ureq::Agent::config_builder()
            .tls_config(tls)
            .user_agent(agent_name)
            .max_redirects(0)
            .http_status_as_error(false)
            .https_only(true)
            .proxy(None)
            .timeout_resolve(Some(TIMEOUT))
            .timeout_connect(Some(TIMEOUT))
            .timeout_send_request(Some(TIMEOUT))
            .timeout_recv_response(Some(TIMEOUT))
            .timeout_per_read(Some(TIMEOUT))
            .build();
        Some(UreqFetch {
            agent: ureq::Agent::new_with_config(config),
        })
    }
}

impl Fetch for UreqFetch {
    fn get(&self, url: &str, max: u64, sink: &mut dyn Write) -> Result<u64, FetchError> {
        let response = self
            .agent
            .get(url)
            .call()
            .map_err(|_| FetchError::Unreachable)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(FetchError::Status(status));
        }
        let mut reader = response.into_body().into_reader().take(max + 1);
        let n = std::io::copy(&mut reader, sink).map_err(|_| FetchError::Unreachable)?;
        if n > max {
            return Err(FetchError::TooLarge);
        }
        Ok(n)
    }
}
