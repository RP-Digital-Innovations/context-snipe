//! Minimal JSON-over-HTTPS POST helper.
//!
//! Uses `ureq` with the native TLS stack (SChannel on Windows), so the binary
//! builds without a C toolchain and carries no OpenSSL baggage. A single shared
//! agent is reused for connection pooling.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::Value;
use ureq::Agent;

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let connector = native_tls::TlsConnector::new().expect("failed to initialize TLS");
        ureq::AgentBuilder::new()
            .tls_connector(Arc::new(connector))
            .timeout(Duration::from_secs(25))
            .build()
    })
}

pub fn post_json(url: &str, body: &Value) -> Result<Value, String> {
    let resp = agent()
        .post(url)
        .set("Content-Type", "application/json")
        .set("User-Agent", concat!("context-snipe/", env!("CARGO_PKG_VERSION")))
        .send_json(body)
        .map_err(|e| format!("request to {url} failed: {e}"))?;
    resp.into_json::<Value>()
        .map_err(|e| format!("invalid JSON from {url}: {e}"))
}
