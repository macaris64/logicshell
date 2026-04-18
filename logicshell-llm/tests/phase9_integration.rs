// Phase 9 integration tests — OllamaLlmClient via mockito
//
// All tests here require the `ollama` feature flag; without it this file is a
// no-op. Run with:
//   cargo test --package logicshell-llm --features ollama
//
// No live network is required. mockito spins up a loopback HTTP server for
// each test. Live-daemon smoke tests are tagged `#[ignore]`.
//
// Traces: FR-21, FR-24, LLM Module PRD §5.4

#[cfg(feature = "ollama")]
mod tests {
    use logicshell_llm::{
        ollama::{HealthStatus, OllamaLlmClient},
        LlmClient, LlmError, LlmRequest,
    };
    use mockito::Matcher;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn generate_ok_body(model: &str, response: &str) -> String {
        format!(r#"{{"model":"{model}","response":"{response}","done":true}}"#)
    }

    fn tags_body(names: &[&str]) -> String {
        let entries: Vec<String> = names
            .iter()
            .map(|n| format!(r#"{{"name":"{n}"}}"#))
            .collect();
        format!(r#"{{"models":[{}]}}"#, entries.join(","))
    }

    // ── POST /api/generate — happy path ──────────────────────────────────────

    #[tokio::test]
    async fn generate_returns_response_text() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("llama3", "ls -la"))
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "llama3", 30);
        let req = LlmRequest {
            model: "llama3".into(),
            prompt: "list files".into(),
        };
        let resp = client.complete(req).await.unwrap();

        assert_eq!(resp.text, "ls -la");
        assert_eq!(resp.model, "llama3");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn generate_uses_post_to_api_generate() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("m", "ok"))
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "m", 30);
        let req = LlmRequest {
            model: "m".into(),
            prompt: "p".into(),
        };
        client.complete(req).await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn generate_sends_json_content_type() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/generate")
            .match_header("content-type", Matcher::Regex("application/json".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("m", "cmd"))
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "m", 30);
        let req = LlmRequest {
            model: "m".into(),
            prompt: "p".into(),
        };
        client.complete(req).await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn generate_sends_stream_false_in_body() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/generate")
            .match_body(Matcher::PartialJsonString(
                r#"{"stream":false}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("m", "out"))
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "m", 30);
        let req = LlmRequest {
            model: "m".into(),
            prompt: "p".into(),
        };
        client.complete(req).await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn generate_model_from_response_used_not_request() {
        // The response model field is what we use — demonstrates we honour the
        // daemon's reported model name.
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("codellama", "git log --oneline"))
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "codellama", 30);
        let req = LlmRequest {
            model: "codellama".into(),
            prompt: "show last commit".into(),
        };
        let resp = client.complete(req).await.unwrap();
        assert_eq!(resp.model, "codellama");
        assert_eq!(resp.text, "git log --oneline");
    }

    #[tokio::test]
    async fn generate_prompt_embedded_in_request_body() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/generate")
            .match_body(Matcher::PartialJsonString(
                r#"{"prompt":"list all running processes"}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("llama3", "ps aux"))
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "llama3", 30);
        let req = LlmRequest {
            model: "llama3".into(),
            prompt: "list all running processes".into(),
        };
        let resp = client.complete(req).await.unwrap();
        assert_eq!(resp.text, "ps aux");
        mock.assert_async().await;
    }

    // ── POST /api/generate — error paths ─────────────────────────────────────

    #[tokio::test]
    async fn generate_503_returns_http_error_with_status() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(503)
            .with_body("service unavailable")
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "m", 30);
        let req = LlmRequest {
            model: "m".into(),
            prompt: "p".into(),
        };
        let result = client.complete(req).await;
        assert!(matches!(result, Err(LlmError::Http(_))));
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("503"),
            "error should mention status code: {msg}"
        );
    }

    #[tokio::test]
    async fn generate_500_includes_body_in_error_message() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(500)
            .with_body("internal error detail")
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "m", 30);
        let req = LlmRequest {
            model: "m".into(),
            prompt: "p".into(),
        };
        let result = client.complete(req).await;
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("500"));
    }

    #[tokio::test]
    async fn generate_404_returns_http_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(404)
            .with_body("not found")
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "m", 30);
        let req = LlmRequest {
            model: "m".into(),
            prompt: "p".into(),
        };
        let result = client.complete(req).await;
        assert!(matches!(result, Err(LlmError::Http(_))));
    }

    #[tokio::test]
    async fn generate_malformed_json_returns_parse_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("not valid json {{{")
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "m", 30);
        let req = LlmRequest {
            model: "m".into(),
            prompt: "p".into(),
        };
        let result = client.complete(req).await;
        assert!(
            matches!(result, Err(LlmError::Parse(_))),
            "malformed JSON must produce Parse error, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn generate_empty_body_returns_parse_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("")
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "m", 30);
        let req = LlmRequest {
            model: "m".into(),
            prompt: "p".into(),
        };
        let result = client.complete(req).await;
        assert!(matches!(result, Err(LlmError::Parse(_))));
    }

    // ── GET /api/tags — health probe ─────────────────────────────────────────

    #[tokio::test]
    async fn health_probe_uses_get_to_api_tags() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(tags_body(&[]))
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "llama3", 30);
        client.health_probe().await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn health_probe_healthy_when_model_exact_match() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(tags_body(&["llama3", "mistral"]))
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "llama3", 30);
        assert_eq!(client.health_probe().await.unwrap(), HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn health_probe_healthy_when_model_has_tag_suffix() {
        // Ollama returns "llama3:latest" even for base model requests.
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(tags_body(&["llama3:latest"]))
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "llama3", 30);
        assert_eq!(client.health_probe().await.unwrap(), HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn health_probe_model_missing_when_not_in_list() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(tags_body(&["mistral", "codellama"]))
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "llama3", 30);
        assert_eq!(
            client.health_probe().await.unwrap(),
            HealthStatus::ModelMissing
        );
    }

    #[tokio::test]
    async fn health_probe_model_missing_on_empty_list() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(tags_body(&[]))
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "llama3", 30);
        assert_eq!(
            client.health_probe().await.unwrap(),
            HealthStatus::ModelMissing
        );
    }

    #[tokio::test]
    async fn health_probe_unexpected_status_503() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/api/tags")
            .with_status(503)
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "llama3", 30);
        assert_eq!(
            client.health_probe().await.unwrap(),
            HealthStatus::UnexpectedStatus(503)
        );
    }

    #[tokio::test]
    async fn health_probe_unexpected_status_404() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/api/tags")
            .with_status(404)
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "llama3", 30);
        assert_eq!(
            client.health_probe().await.unwrap(),
            HealthStatus::UnexpectedStatus(404)
        );
    }

    #[tokio::test]
    async fn health_probe_malformed_tags_json_returns_parse_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/api/tags")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("{{broken")
            .create_async()
            .await;

        let client = OllamaLlmClient::new(server.url(), "llama3", 30);
        let result = client.health_probe().await;
        assert!(matches!(result, Err(LlmError::Parse(_))));
    }

    // ── Graceful degradation — FR-24 ─────────────────────────────────────────

    #[tokio::test]
    async fn health_probe_connection_refused_returns_http_error() {
        // Port 19998 should be closed; connection refused → Http error.
        let client = OllamaLlmClient::new("http://127.0.0.1:19998", "m", 1);
        let result = client.health_probe().await;
        assert!(
            matches!(result, Err(LlmError::Http(_))),
            "unreachable daemon must return Http error, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn generate_connection_refused_returns_http_error() {
        let client = OllamaLlmClient::new("http://127.0.0.1:19998", "m", 1);
        let req = LlmRequest {
            model: "m".into(),
            prompt: "p".into(),
        };
        let result = client.complete(req).await;
        assert!(
            matches!(result, Err(LlmError::Http(_))),
            "unreachable daemon must return Http error, got: {result:?}"
        );
    }

    // ── Integration with PromptComposer (Phase 8 + Phase 9) ──────────────────

    #[tokio::test]
    async fn composer_to_ollama_full_pipeline() {
        use logicshell_llm::{PromptComposer, SystemContextSnapshot};

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("llama3", "ls -lhS"))
            .create_async()
            .await;

        let ctx = SystemContextSnapshot {
            os_family: "linux".into(),
            arch: "x86_64".into(),
            cwd: "/home/user/project".into(),
            path_dirs: vec!["/usr/bin".into(), "/bin".into()],
        };
        let composer = PromptComposer::new("llama3", 8_000);
        let req = composer
            .compose_nl_to_command("list files sorted by size", &ctx)
            .unwrap();

        let client = OllamaLlmClient::new(server.url(), "llama3", 30);
        let resp = client.complete(req).await.unwrap();

        assert_eq!(resp.text, "ls -lhS");
        assert_eq!(resp.model, "llama3");
    }

    #[tokio::test]
    async fn assist_on_127_pipeline_with_ollama() {
        use logicshell_llm::{PromptComposer, SystemContextSnapshot};

        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/api/generate")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(generate_ok_body("codellama", "git status"))
            .create_async()
            .await;

        let ctx = SystemContextSnapshot {
            os_family: "linux".into(),
            arch: "x86_64".into(),
            cwd: "/repo".into(),
            path_dirs: vec!["/usr/bin".into()],
        };
        let composer = PromptComposer::new("codellama", 8_000);
        let req = composer
            .compose_assist_on_127(&["gti", "status"], &ctx)
            .unwrap();

        let client = OllamaLlmClient::new(server.url(), "codellama", 30);
        let resp = client.complete(req).await.unwrap();

        assert_eq!(resp.text, "git status");
    }

    // ── Live daemon smoke tests (excluded from default CI) ───────────────────

    #[tokio::test]
    #[ignore = "requires live Ollama daemon at http://127.0.0.1:11434"]
    async fn live_health_probe_with_real_daemon() {
        let client = OllamaLlmClient::new("http://127.0.0.1:11434", "llama3", 30);
        let status = client.health_probe().await;
        assert!(status.is_ok(), "health probe must not error: {status:?}");
        println!("health status: {:?}", status.unwrap());
    }

    #[tokio::test]
    #[ignore = "requires live Ollama daemon with llama3 model pulled"]
    async fn live_generate_with_real_daemon() {
        let client = OllamaLlmClient::new("http://127.0.0.1:11434", "llama3", 60);
        let req = LlmRequest {
            model: "llama3".into(),
            prompt: "Output only the shell command to list files in the current directory. No explanation.".into(),
        };
        let resp = client.complete(req).await;
        assert!(resp.is_ok(), "generate should succeed: {resp:?}");
        println!("response: {:?}", resp.unwrap().text);
    }
}
