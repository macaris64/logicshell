// LlmBridge — Phase 10, LLM Module PRD §5.7
//
// Orchestrates: SystemContextProvider → PromptComposer → LlmClient → parser
// → ProposedCommand.
//
// Three operational modes (FR-25, FR-26, FR-27):
//   - `translate_nl`    — natural-language session mode
//   - `assist_on_127`   — suggest a fix when a command returns exit 127
//
// Graceful degradation (FR-24): any `LlmError::Http` from the client is
// propagated unchanged so callers can fall back to the non-AI path without
// panicking.
//
// The bridge is independent of the `ollama` feature; callers inject any
// `Arc<dyn LlmClient>` implementation.

use std::sync::Arc;

use logicshell_core::config::LlmConfig;

use crate::{
    client::LlmClient,
    context::SystemContextProvider,
    error::LlmError,
    parser::parse_command_response,
    prompt::PromptComposer,
    proposed::{CommandSource, ProposedCommand},
};

/// Orchestrates the full LLM-to-command pipeline.
///
/// Generic over the client type `C` (must implement [`LlmClient`]) so that
/// production code uses the concrete `OllamaLlmClient` and tests inject a
/// `MockLlmClient` — no `dyn` trait objects needed (async traits are not
/// dyn-compatible without boxing).
///
/// Construct with [`LlmBridge::new`] (direct) or [`LlmBridge::from_config`]
/// (from a validated [`LlmConfig`]).  All methods are `async` because
/// inference is I/O-bound (NFR-05).
#[derive(Debug)]
pub struct LlmBridge<C> {
    composer: PromptComposer,
    client: Arc<C>,
    context_provider: SystemContextProvider,
}

impl<C: LlmClient> LlmBridge<C> {
    /// Construct a bridge directly with a model name and context cap.
    ///
    /// Use [`LlmBridge::from_config`] when a validated [`LlmConfig`] is
    /// available; use this constructor in tests or when the config is not
    /// available.
    pub fn new(client: Arc<C>, model: impl Into<String>, max_context_chars: usize) -> Self {
        Self {
            composer: PromptComposer::new(model, max_context_chars),
            client,
            context_provider: SystemContextProvider::new(),
        }
    }

    /// Construct a bridge from an [`LlmConfig`], returning errors for disabled
    /// or misconfigured LLM settings.
    pub fn from_config(client: Arc<C>, config: &LlmConfig) -> Result<Self, LlmError> {
        let composer = PromptComposer::from_config(config)?;
        Ok(Self {
            composer,
            client,
            context_provider: SystemContextProvider::new(),
        })
    }

    /// The model name this bridge sends in every request.
    pub fn model(&self) -> &str {
        self.composer.model()
    }

    /// Translate a natural-language description into a [`ProposedCommand`].
    ///
    /// Pipeline: snapshot → compose_nl_to_command → complete → parse.
    ///
    /// # Errors
    ///
    /// - `LlmError::ContextTooLarge` if the rendered prompt exceeds the cap.
    /// - `LlmError::Http` if the daemon is unreachable (graceful degradation).
    /// - `LlmError::Parse` if the model response cannot be tokenized.
    pub async fn translate_nl(&self, nl_input: &str) -> Result<ProposedCommand, LlmError> {
        let snap = self.context_provider.snapshot();
        let req = self.composer.compose_nl_to_command(nl_input, &snap)?;
        let resp = self.client.complete(req).await?;
        let argv = parse_command_response(&resp.text)?;
        Ok(ProposedCommand::new(
            argv,
            CommandSource::AiGenerated,
            resp.text,
        ))
    }

    /// Suggest a corrected command when the original returned exit code 127.
    ///
    /// Pipeline: snapshot → compose_assist_on_127 → complete → parse.
    ///
    /// # Errors
    ///
    /// Same as [`translate_nl`].
    pub async fn assist_on_127(&self, failed_argv: &[&str]) -> Result<ProposedCommand, LlmError> {
        let snap = self.context_provider.snapshot();
        let req = self.composer.compose_assist_on_127(failed_argv, &snap)?;
        let resp = self.client.complete(req).await?;
        let argv = parse_command_response(&resp.text)?;
        Ok(ProposedCommand::new(
            argv,
            CommandSource::AiGenerated,
            resp.text,
        ))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{LlmResponse, MockLlmClient};
    use logicshell_core::config::LlmConfig;

    fn mock_client_returning(text: &str) -> Arc<MockLlmClient> {
        let mut mock = MockLlmClient::new();
        let text = text.to_string();
        mock.expect_complete().returning(move |req| {
            Ok(LlmResponse {
                text: text.clone(),
                model: req.model,
            })
        });
        Arc::new(mock)
    }

    fn error_client(err: LlmError) -> Arc<MockLlmClient> {
        let mut mock = MockLlmClient::new();
        // LlmError is Clone-able via the explicit impl in error module tests
        let msg = err.to_string();
        mock.expect_complete()
            .returning(move |_| Err(LlmError::Http(msg.clone())));
        Arc::new(mock)
    }

    // ── LlmBridge::new ────────────────────────────────────────────────────────

    #[test]
    fn new_stores_model() {
        let client = mock_client_returning("ls");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        assert_eq!(bridge.model(), "llama3");
    }

    #[test]
    fn new_with_zero_cap_stores_model() {
        let client = mock_client_returning("ls");
        let bridge = LlmBridge::new(client, "m", 0);
        assert_eq!(bridge.model(), "m");
    }

    // ── LlmBridge::from_config ────────────────────────────────────────────────

    #[test]
    fn from_config_disabled_returns_error() {
        let client = mock_client_returning("ls");
        let cfg = LlmConfig {
            enabled: false,
            ..LlmConfig::default()
        };
        let result = LlmBridge::from_config(client, &cfg);
        assert_eq!(result.unwrap_err(), LlmError::Disabled);
    }

    #[test]
    fn from_config_enabled_no_model_returns_error() {
        let client = mock_client_returning("ls");
        let cfg = LlmConfig {
            enabled: true,
            model: None,
            ..LlmConfig::default()
        };
        let result = LlmBridge::from_config(client, &cfg);
        assert_eq!(result.unwrap_err(), LlmError::ModelNotSpecified);
    }

    #[test]
    fn from_config_valid_stores_model() {
        let client = mock_client_returning("ls");
        let cfg = LlmConfig {
            enabled: true,
            model: Some("mistral".into()),
            ..LlmConfig::default()
        };
        let bridge = LlmBridge::from_config(client, &cfg).unwrap();
        assert_eq!(bridge.model(), "mistral");
    }

    // ── translate_nl ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn translate_nl_returns_proposed_command() {
        let client = mock_client_returning("ls -la");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge.translate_nl("list files").await.unwrap();
        assert_eq!(proposed.argv, vec!["ls", "-la"]);
    }

    #[tokio::test]
    async fn translate_nl_source_is_ai_generated() {
        let client = mock_client_returning("pwd");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge
            .translate_nl("print working directory")
            .await
            .unwrap();
        assert_eq!(proposed.source, CommandSource::AiGenerated);
    }

    #[tokio::test]
    async fn translate_nl_raw_response_preserved() {
        let client = mock_client_returning("ls -lhS");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge.translate_nl("list files by size").await.unwrap();
        assert_eq!(proposed.raw_response, "ls -lhS");
    }

    #[tokio::test]
    async fn translate_nl_strips_code_fence_in_response() {
        let client = mock_client_returning("```bash\nls -la\n```");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge.translate_nl("list files").await.unwrap();
        assert_eq!(proposed.argv, vec!["ls", "-la"]);
    }

    #[tokio::test]
    async fn translate_nl_context_too_large_propagated() {
        let client = mock_client_returning("ls");
        let bridge = LlmBridge::new(client, "m", 10); // cap too small
        let result = bridge.translate_nl("list files").await;
        assert!(matches!(result, Err(LlmError::ContextTooLarge { .. })));
    }

    #[tokio::test]
    async fn translate_nl_http_error_propagated_for_degradation() {
        let client = error_client(LlmError::Http("connection refused".into()));
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let result = bridge.translate_nl("list files").await;
        assert!(matches!(result, Err(LlmError::Http(_))));
    }

    #[tokio::test]
    async fn translate_nl_parse_error_on_empty_response() {
        let client = mock_client_returning("");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let result = bridge.translate_nl("list files").await;
        assert!(matches!(result, Err(LlmError::Parse(_))));
    }

    #[tokio::test]
    async fn translate_nl_git_command() {
        let client = mock_client_returning("git log --oneline -10");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge.translate_nl("show last 10 commits").await.unwrap();
        assert_eq!(proposed.argv, vec!["git", "log", "--oneline", "-10"]);
    }

    // ── assist_on_127 ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn assist_on_127_returns_proposed_command() {
        let client = mock_client_returning("git status");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge.assist_on_127(&["gti", "status"]).await.unwrap();
        assert_eq!(proposed.argv, vec!["git", "status"]);
    }

    #[tokio::test]
    async fn assist_on_127_source_is_ai_generated() {
        let client = mock_client_returning("git status");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge.assist_on_127(&["gti", "status"]).await.unwrap();
        assert_eq!(proposed.source, CommandSource::AiGenerated);
    }

    #[tokio::test]
    async fn assist_on_127_raw_response_preserved() {
        let client = mock_client_returning("docker ps");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge.assist_on_127(&["docekr", "ps"]).await.unwrap();
        assert_eq!(proposed.raw_response, "docker ps");
    }

    #[tokio::test]
    async fn assist_on_127_empty_argv_works() {
        let client = mock_client_returning("ls");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let proposed = bridge.assist_on_127(&[]).await.unwrap();
        assert!(!proposed.argv.is_empty());
    }

    #[tokio::test]
    async fn assist_on_127_http_error_propagated() {
        let client = error_client(LlmError::Http("timeout".into()));
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let result = bridge.assist_on_127(&["gti", "status"]).await;
        assert!(matches!(result, Err(LlmError::Http(_))));
    }

    #[tokio::test]
    async fn assist_on_127_context_too_large_propagated() {
        let client = mock_client_returning("git status");
        let bridge = LlmBridge::new(client, "m", 10); // tiny cap
        let result = bridge.assist_on_127(&["gti"]).await;
        assert!(matches!(result, Err(LlmError::ContextTooLarge { .. })));
    }

    #[tokio::test]
    async fn assist_on_127_parse_error_on_empty_response() {
        let client = mock_client_returning("   ");
        let bridge = LlmBridge::new(client, "llama3", 8_000);
        let result = bridge.assist_on_127(&["cmd"]).await;
        assert!(matches!(result, Err(LlmError::Parse(_))));
    }
}
