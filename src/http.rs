//! Minimal JSON-over-HTTPS POST helper.
//!
//! Uses `ureq` with the `tls` feature (rustls), so the binary carries its own
//! TLS stack with no OpenSSL or system-crypto dependency — works identically on
//! Windows, macOS, and Linux (including musl).

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;
use ureq::Agent;

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
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
