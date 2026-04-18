// OllamaLlmClient — Phase 9, LLM Module PRD §5.4
//
// HTTP-backed LlmClient targeting a local Ollama daemon.
// Only compiled under the `ollama` feature flag (reqwest is gated there).
//
// Wire format:
//   POST {base_url}/api/generate  {model, prompt, stream:false} → {model, response}
//   GET  {base_url}/api/tags                                     → {models:[{name},...]}
//
// Zero live network in default `cargo test`. Tests in phase9_integration.rs use
// mockito; live-daemon smoke tests are `#[ignore]`.

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{
    client::{LlmClient, LlmRequest, LlmResponse},
    error::LlmError,
};

// ── Ollama wire types ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct OllamaGenerateBody<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
}

/// Response from `POST /api/generate` (non-streaming).
/// Extra fields (done, context, durations) are ignored intentionally.
#[derive(Debug, Deserialize)]
struct OllamaGenerateResp {
    model: String,
    response: String,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResp {
    models: Vec<OllamaModelEntry>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelEntry {
    name: String,
}

// ── Health status ─────────────────────────────────────────────────────────────

/// Result of a health probe against the Ollama daemon — graceful degradation
/// matrix (Phase 9, FR-24).
///
/// Callers decide whether to proceed, warn, or surface an error based on the
/// variant; they never panic on an unexpected daemon state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    /// Daemon is reachable and the configured model is listed in `/api/tags`.
    Healthy,
    /// Daemon is reachable but the configured model is not yet pulled.
    ModelMissing,
    /// Daemon responded with an unexpected HTTP status code.
    UnexpectedStatus(u16),
}

// ── OllamaLlmClient ───────────────────────────────────────────────────────────

/// HTTP-backed [`LlmClient`] that targets a local Ollama daemon.
///
/// Enable the `ollama` feature to pull in `reqwest` and include this type.
///
/// ```text
/// POST {base_url}/api/generate  → LlmResponse
/// GET  {base_url}/api/tags      → HealthStatus (probe)
/// ```
#[derive(Debug)]
pub struct OllamaLlmClient {
    base_url: String,
    model: String,
    http: Client,
}

impl OllamaLlmClient {
    /// Build a new client.
    ///
    /// - `base_url` — e.g. `"http://127.0.0.1:11434"`
    /// - `model` — model name used in requests and health probing
    /// - `timeout_secs` — per-request timeout; `0` means no timeout
    pub fn new(base_url: impl Into<String>, model: impl Into<String>, timeout_secs: u64) -> Self {
        let mut builder = Client::builder();
        if timeout_secs > 0 {
            builder = builder.timeout(Duration::from_secs(timeout_secs));
        }
        let http = builder.build().expect("reqwest Client build is infallible");
        Self {
            base_url: base_url.into(),
            model: model.into(),
            http,
        }
    }

    /// The base URL this client is configured to reach.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The model name embedded in every request.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Probe `GET /api/tags` and return a [`HealthStatus`].
    ///
    /// Returns `Err(LlmError::Http(_))` when the daemon is unreachable —
    /// callers use this for graceful degradation (FR-24) and should fall back
    /// to non-AI paths rather than propagating the raw network error.
    ///
    /// Model name matching accepts both exact (`"llama3"`) and tag-qualified
    /// (`"llama3:latest"`) entries returned by the daemon.
    pub async fn health_probe(&self) -> Result<HealthStatus, LlmError> {
        let url = format!("{}/api/tags", self.base_url);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            return Ok(HealthStatus::UnexpectedStatus(resp.status().as_u16()));
        }

        let tags: OllamaTagsResp = resp
            .json()
            .await
            .map_err(|e| LlmError::Parse(e.to_string()))?;

        let found = tags
            .models
            .iter()
            .any(|m| m.name == self.model || m.name.starts_with(&format!("{}:", self.model)));

        if found {
            Ok(HealthStatus::Healthy)
        } else {
            Ok(HealthStatus::ModelMissing)
        }
    }
}

impl LlmClient for OllamaLlmClient {
    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/api/generate", self.base_url);
        let body = OllamaGenerateBody {
            model: &request.model,
            prompt: &request.prompt,
            stream: false,
        };

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(LlmError::Http(format!(
                "Ollama /api/generate returned HTTP {status}: {body_text}"
            )));
        }

        let gen: OllamaGenerateResp = resp
            .json()
            .await
            .map_err(|e| LlmError::Parse(e.to_string()))?;

        Ok(LlmResponse {
            text: gen.response,
            model: gen.model,
        })
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Wire type deserialization ─────────────────────────────────────────────

    #[test]
    fn deserialize_generate_response_minimal() {
        let json = r#"{"model":"llama3","response":"ls -la"}"#;
        let resp: OllamaGenerateResp = serde_json::from_str(json).unwrap();
        assert_eq!(resp.model, "llama3");
        assert_eq!(resp.response, "ls -la");
    }

    #[test]
    fn deserialize_generate_response_ignores_extra_fields() {
        // Ollama adds done, context, durations — all should be silently ignored.
        let json = r#"{"model":"llama3","response":"ls","done":true,"context":[1,2],"total_duration":100}"#;
        let resp: OllamaGenerateResp = serde_json::from_str(json).unwrap();
        assert_eq!(resp.model, "llama3");
        assert_eq!(resp.response, "ls");
    }

    #[test]
    fn deserialize_tags_response_multiple_models() {
        let json = r#"{"models":[{"name":"llama3:latest"},{"name":"mistral"}]}"#;
        let tags: OllamaTagsResp = serde_json::from_str(json).unwrap();
        assert_eq!(tags.models.len(), 2);
        assert_eq!(tags.models[0].name, "llama3:latest");
        assert_eq!(tags.models[1].name, "mistral");
    }

    #[test]
    fn deserialize_tags_empty_models() {
        let json = r#"{"models":[]}"#;
        let tags: OllamaTagsResp = serde_json::from_str(json).unwrap();
        assert!(tags.models.is_empty());
    }

    #[test]
    fn deserialize_tags_single_model() {
        let json = r#"{"models":[{"name":"codellama"}]}"#;
        let tags: OllamaTagsResp = serde_json::from_str(json).unwrap();
        assert_eq!(tags.models.len(), 1);
        assert_eq!(tags.models[0].name, "codellama");
    }

    // ── OllamaGenerateBody serialization ──────────────────────────────────────

    #[test]
    fn serialize_generate_body_includes_all_fields() {
        let body = OllamaGenerateBody {
            model: "llama3",
            prompt: "list files",
            stream: false,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains("\"model\":\"llama3\""));
        assert!(json.contains("\"prompt\":\"list files\""));
        assert!(json.contains("\"stream\":false"));
    }

    #[test]
    fn serialize_generate_body_stream_false() {
        let body = OllamaGenerateBody {
            model: "m",
            prompt: "p",
            stream: false,
        };
        let json = serde_json::to_string(&body).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["stream"], false);
    }

    // ── OllamaLlmClient constructor ───────────────────────────────────────────

    #[test]
    fn new_stores_base_url_and_model() {
        let client = OllamaLlmClient::new("http://127.0.0.1:11434", "llama3", 30);
        assert_eq!(client.base_url(), "http://127.0.0.1:11434");
        assert_eq!(client.model(), "llama3");
    }

    #[test]
    fn new_with_zero_timeout() {
        let client = OllamaLlmClient::new("http://127.0.0.1:11434", "m", 0);
        assert_eq!(client.base_url(), "http://127.0.0.1:11434");
        assert_eq!(client.model(), "m");
    }

    #[test]
    fn new_with_custom_base_url() {
        let client = OllamaLlmClient::new("http://10.0.0.1:11434", "mistral", 60);
        assert_eq!(client.base_url(), "http://10.0.0.1:11434");
        assert_eq!(client.model(), "mistral");
    }

    #[test]
    fn client_debug_contains_base_url() {
        let client = OllamaLlmClient::new("http://127.0.0.1:11434", "llama3", 30);
        let s = format!("{client:?}");
        assert!(s.contains("OllamaLlmClient"));
        assert!(s.contains("http://127.0.0.1:11434"));
    }

    // ── HealthStatus ─────────────────────────────────────────────────────────

    #[test]
    fn health_status_clone_eq() {
        assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_eq!(HealthStatus::ModelMissing, HealthStatus::ModelMissing);
        assert_eq!(
            HealthStatus::UnexpectedStatus(404),
            HealthStatus::UnexpectedStatus(404)
        );
        assert_ne!(
            HealthStatus::UnexpectedStatus(200),
            HealthStatus::UnexpectedStatus(404)
        );
    }

    #[test]
    fn health_status_ne_variants() {
        assert_ne!(HealthStatus::Healthy, HealthStatus::ModelMissing);
        assert_ne!(HealthStatus::Healthy, HealthStatus::UnexpectedStatus(200));
        assert_ne!(
            HealthStatus::ModelMissing,
            HealthStatus::UnexpectedStatus(503)
        );
    }

    #[test]
    fn health_status_debug() {
        assert!(format!("{:?}", HealthStatus::Healthy).contains("Healthy"));
        assert!(format!("{:?}", HealthStatus::ModelMissing).contains("ModelMissing"));
        assert!(format!("{:?}", HealthStatus::UnexpectedStatus(503)).contains("UnexpectedStatus"));
    }
}
