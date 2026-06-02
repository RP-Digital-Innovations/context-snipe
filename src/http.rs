//! Minimal JSON-over-HTTPS POST helper.
//!
//! Uses `ureq` with the native TLS stack (SChannel on Windows), so the binary
//! builds without a C toolchain and carries no OpenSSL baggage. A single shared
//! agent is reused for connection pooling.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::Value;
use ureq::Agent;

/// Lazily-built shared agent. TLS init can fail (rare), so the result is cached
/// as a `Result` and surfaced as an error to the caller — never a panic, which
/// under `panic = "abort"` would take the whole MCP server down with it.
fn agent() -> Result<&'static Agent, String> {
    static AGENT: OnceLock<Result<Agent, String>> = OnceLock::new();
    AGENT
        .get_or_init(|| {
            let connector = native_tls::TlsConnector::new()
                .map_err(|e| format!("TLS initialization failed: {e}"))?;
            Ok(ureq::AgentBuilder::new()
                .tls_connector(Arc::new(connector))
                .timeout(Duration::from_secs(25))
                .build())
        })
        .as_ref()
        .map_err(|e| e.clone())
}

pub fn post_json(url: &str, body: &Value) -> Result<Value, String> {
    let resp = agent()?
        .post(url)
        .set("Content-Type", "application/json")
        .set("User-Agent", concat!("context-snipe/", env!("CARGO_PKG_VERSION")))
        .send_json(body)
        .map_err(|e| format!("request to {url} failed: {e}"))?;
    resp.into_json::<Value>()
        .map_err(|e| format!("invalid JSON from {url}: {e}"))
}
