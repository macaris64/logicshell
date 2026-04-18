// Phase 8 integration tests — LLM context + prompt composer end-to-end
//
// These tests exercise the full Phase 8 pipeline:
//   SystemContextProvider → SystemContextSnapshot → PromptComposer → LlmRequest
//
// No network calls are made; the LlmClient trait is exercised via concrete stubs.
// Traces: FR-10, FR-11, FR-21, NFR-05, LLM Module PRD §5.1–5.3

use logicshell_core::config::LlmConfig;
use logicshell_llm::{
    LlmClient, LlmError, LlmRequest, LlmResponse, PromptComposer, SystemContextProvider,
    SystemContextSnapshot,
};

// ── Stub LlmClient ────────────────────────────────────────────────────────────

/// Stub that echoes the request back in the response.
struct EchoClient;

impl LlmClient for EchoClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            text: format!("echo: {}", req.prompt.lines().last().unwrap_or("").trim()),
            model: req.model.clone(),
        })
    }
}

/// Stub that always returns a fixed command string.
struct FixedClient {
    response: String,
}

impl LlmClient for FixedClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            text: self.response.clone(),
            model: req.model,
        })
    }
}

/// Stub that always errors.
struct ErrorClient;

impl LlmClient for ErrorClient {
    async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
        Err(LlmError::Http("daemon not running".into()))
    }
}

// ── Helper context ────────────────────────────────────────────────────────────

fn test_ctx() -> SystemContextSnapshot {
    SystemContextSnapshot {
        os_family: "linux".into(),
        arch: "x86_64".into(),
        cwd: "/home/user/project".into(),
        path_dirs: vec!["/usr/bin".into(), "/bin".into(), "/usr/local/bin".into()],
    }
}

// ── SystemContextProvider integration ────────────────────────────────────────

/// FR-10: SystemContextProvider returns a consistent snapshot.
#[test]
fn provider_snapshot_is_consistent() {
    let provider = SystemContextProvider::new();
    let snap1 = provider.snapshot();
    let snap2 = provider.snapshot();

    // OS and arch never change within a run
    assert_eq!(snap1.os_family, snap2.os_family);
    assert_eq!(snap1.arch, snap2.arch);
}

/// FR-11: snapshot contains required fields on Linux CI.
#[test]
fn provider_snapshot_has_required_fields() {
    let snap = SystemContextProvider::new().snapshot();
    assert!(!snap.os_family.is_empty(), "os_family must be present");
    assert!(!snap.arch.is_empty(), "arch must be present");
    assert!(!snap.cwd.is_empty(), "cwd must be present");
}

// ── PromptComposer from LlmConfig integration ─────────────────────────────────

/// from_config with all fields set produces a working composer.
#[test]
fn composer_from_config_full_pipeline() {
    let cfg = LlmConfig {
        enabled: true,
        model: Some("llama3".into()),
        ..LlmConfig::default()
    };
    let composer = PromptComposer::from_config(&cfg).unwrap();

    let ctx = test_ctx();
    let req = composer
        .compose_nl_to_command("list files sorted by modification time", &ctx)
        .unwrap();

    assert_eq!(req.model, "llama3");
    assert!(req
        .prompt
        .contains("list files sorted by modification time"));
    assert!(req.prompt.contains("linux"));
    assert!(req.prompt.contains("x86_64"));
}

/// PromptComposer::from_config respects invocation.max_context_chars.
#[test]
fn composer_from_config_respects_context_cap() {
    let mut cfg = LlmConfig {
        enabled: true,
        model: Some("m".into()),
        ..LlmConfig::default()
    };
    cfg.invocation.max_context_chars = 50; // very tight

    let composer = PromptComposer::from_config(&cfg).unwrap();
    let result = composer.compose_nl_to_command("ls", &test_ctx());
    assert!(
        matches!(result, Err(LlmError::ContextTooLarge { .. })),
        "tight cap must trigger ContextTooLarge"
    );
}

// ── Full pipeline: context → composer → client ────────────────────────────────

/// FR-21: context → composer → echo-client round-trip.
#[tokio::test]
async fn full_nl_to_command_pipeline() {
    let ctx = SystemContextProvider::new().snapshot();
    let composer = PromptComposer::new("llama3", 8_000);

    let req = composer
        .compose_nl_to_command("show current directory contents", &ctx)
        .unwrap();
    assert_eq!(req.model, "llama3");
    assert!(req.prompt.contains("show current directory contents"));

    let client = EchoClient;
    let resp = client.complete(req).await.unwrap();
    assert_eq!(resp.model, "llama3");
    assert!(!resp.text.is_empty());
}

/// FR-21: context → composer → fixed-client round-trip for assist-on-127.
#[tokio::test]
async fn full_assist_127_pipeline() {
    let ctx = SystemContextProvider::new().snapshot();
    let composer = PromptComposer::new("codellama", 8_000);

    let req = composer
        .compose_assist_on_127(&["gti", "status"], &ctx)
        .unwrap();
    assert!(req.prompt.contains("gti status"));
    assert_eq!(req.model, "codellama");

    let client = FixedClient {
        response: "git status".into(),
    };
    let resp = client.complete(req).await.unwrap();
    assert_eq!(resp.text, "git status");
    assert_eq!(resp.model, "codellama");
}

/// FR-24: graceful degradation when LlmClient returns an error.
#[tokio::test]
async fn client_error_is_propagated_gracefully() {
    let ctx = test_ctx();
    let composer = PromptComposer::new("llama3", 8_000);
    let req = composer.compose_nl_to_command("ls", &ctx).unwrap();

    let client = ErrorClient;
    let result = client.complete(req).await;

    assert!(result.is_err(), "error must propagate");
    assert!(matches!(result.unwrap_err(), LlmError::Http(_)));
}

// ── LlmRequest / LlmResponse round-trip ──────────────────────────────────────

#[test]
fn request_response_fields_stable() {
    let req = LlmRequest {
        model: "llama3".into(),
        prompt: "ls -la".into(),
    };
    assert_eq!(req.model, "llama3");
    assert_eq!(req.prompt, "ls -la");

    let resp = LlmResponse {
        text: "ls -la /home".into(),
        model: "llama3".into(),
    };
    assert_eq!(resp.text, "ls -la /home");
    assert_eq!(resp.model, "llama3");
}

// ── LlmError propagation ──────────────────────────────────────────────────────

#[test]
fn llm_error_variants_all_display() {
    let errors = [
        LlmError::Disabled,
        LlmError::ModelNotSpecified,
        LlmError::ContextTooLarge {
            size: 9000,
            max: 8000,
        },
        LlmError::Parse("bad json".into()),
        LlmError::Http("timeout".into()),
        LlmError::Other("unknown".into()),
    ];
    for e in &errors {
        assert!(!e.to_string().is_empty(), "error must have display: {e:?}");
    }
}

// ── Context isolation: composer does not read env ─────────────────────────────

/// FR-11: PromptComposer uses the provided snapshot, not live env vars.
#[test]
fn composer_uses_snapshot_not_live_env() {
    let artificial_ctx = SystemContextSnapshot {
        os_family: "fake_os".into(),
        arch: "fake_arch".into(),
        cwd: "/fake/cwd".into(),
        path_dirs: vec!["/fake/path".into()],
    };

    let composer = PromptComposer::new("m", 8_000);
    let req = composer
        .compose_nl_to_command("ls", &artificial_ctx)
        .unwrap();

    // The prompt must reflect the artificial snapshot, not the real env
    assert!(
        req.prompt.contains("fake_os"),
        "must use snapshot os_family"
    );
    assert!(req.prompt.contains("fake_arch"), "must use snapshot arch");
    assert!(req.prompt.contains("/fake/cwd"), "must use snapshot cwd");
}

// ── Multiple composer invocations are idempotent ──────────────────────────────

#[test]
fn composer_is_idempotent() {
    let composer = PromptComposer::new("llama3", 8_000);
    let ctx = test_ctx();

    let req1 = composer.compose_nl_to_command("ls", &ctx).unwrap();
    let req2 = composer.compose_nl_to_command("ls", &ctx).unwrap();

    assert_eq!(req1, req2, "same inputs must produce identical LlmRequest");
}
