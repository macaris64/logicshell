// Phase 10 demo: LlmBridge + AI-safety integration
//
// Shows the full pipeline:
//   NL input → LlmBridge → ProposedCommand → safety evaluation → dispatch
//
// Usage (no Ollama required — uses an inline stub client):
//   cargo run --example phase10 --package logicshell-llm
//
// With a live Ollama daemon:
//   cargo run --example phase10 --package logicshell-llm --features ollama

use std::sync::Arc;

use logicshell_core::{
    config::{LlmConfig, SafetyConfig, SafetyMode},
    Decision,
};
use logicshell_llm::{
    apply_ai_safety_floor, CommandSource, LlmBridge, LlmClient, LlmError, LlmRequest, LlmResponse,
    ProposedCommand,
};

// ── Stub client used when Ollama is not running ───────────────────────────────

#[derive(Debug)]
struct StubClient {
    fixed_response: String,
}

impl StubClient {
    fn new(response: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            fixed_response: response.into(),
        })
    }
}

impl LlmClient for StubClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            text: self.fixed_response.clone(),
            model: req.model,
        })
    }
}

#[tokio::main]
async fn main() {
    println!("[Phase 10: LlmBridge construction]");

    let bridge = LlmBridge::new(StubClient::new("ls -la"), "llama3", 8_000);
    assert_eq!(bridge.model(), "llama3");
    println!("  model = {:?}", bridge.model());

    // from_config — disabled LLM
    let disabled_cfg = LlmConfig {
        enabled: false,
        ..LlmConfig::default()
    };
    let err = LlmBridge::from_config(StubClient::new("ls"), &disabled_cfg).unwrap_err();
    println!("  disabled LLM returns: {err}");
    assert!(err.to_string().contains("disabled"));
    println!("  construction assertions OK");

    println!("[Phase 10: translate_nl — NL session mode]");

    let bridge = LlmBridge::new(StubClient::new("ls -lhS"), "llama3", 8_000);
    let proposed = bridge
        .translate_nl("list files sorted by size")
        .await
        .expect("translate_nl failed");

    assert_eq!(proposed.source, CommandSource::AiGenerated);
    assert_eq!(proposed.argv[0], "ls");
    println!("  nl_input  = \"list files sorted by size\"");
    println!("  argv      = {:?}", proposed.argv);
    println!("  source    = {:?}", proposed.source);
    println!("  raw       = {:?}", proposed.raw_response);
    println!("  translate_nl OK");

    println!("[Phase 10: assist_on_127 — typo correction mode]");

    let bridge = LlmBridge::new(StubClient::new("git status"), "llama3", 8_000);
    let proposed127 = bridge
        .assist_on_127(&["gti", "status"])
        .await
        .expect("assist_on_127 failed");

    assert_eq!(proposed127.argv, vec!["git", "status"]);
    assert_eq!(proposed127.source, CommandSource::AiGenerated);
    println!("  failed_argv = [\"gti\", \"status\"]");
    println!("  suggested   = {:?}", proposed127.argv);
    println!("  assist_on_127 OK");

    println!("[Phase 10: graceful degradation — unreachable daemon]");

    #[derive(Debug)]
    struct DownClient;
    impl LlmClient for DownClient {
        async fn complete(&self, _: LlmRequest) -> Result<LlmResponse, LlmError> {
            Err(LlmError::Http("connection refused".into()))
        }
    }

    let bridge_down = LlmBridge::new(Arc::new(DownClient), "llama3", 8_000);
    match bridge_down.translate_nl("do something").await {
        Err(LlmError::Http(msg)) => {
            println!("  daemon down → graceful Http error: {msg}");
        }
        other => panic!("expected Http error, got: {other:?}"),
    }
    println!("  graceful degradation OK");

    println!("[Phase 10: AI safety floor — ProposedCommand raises risk]");

    // Safe command (ls) is normally Allow, but AI raises it to Confirm.
    let safe_proposed = ProposedCommand::new(
        vec!["ls".into(), "-la".into()],
        CommandSource::AiGenerated,
        "ls -la",
    );
    let (assessment, decision) =
        safe_proposed.evaluate_safety(SafetyMode::Balanced, &SafetyConfig::default());
    println!("  command     = {:?}", safe_proposed.argv);
    println!("  risk_score  = {}", assessment.score);
    println!("  decision    = {decision:?}  (Allow raised to Confirm by AI floor)");
    assert_eq!(
        decision,
        Decision::Confirm,
        "safe AI command must require user confirmation"
    );

    // Dangerous command stays Deny regardless.
    let dangerous_proposed = ProposedCommand::new(
        vec!["rm".into(), "-rf".into(), "/".into()],
        CommandSource::AiGenerated,
        "rm -rf /",
    );
    let (_, deny_decision) =
        dangerous_proposed.evaluate_safety(SafetyMode::Balanced, &SafetyConfig::default());
    println!("  rm -rf / → decision = {deny_decision:?}");
    assert_eq!(deny_decision, Decision::Deny);
    println!("  safety floor assertions OK");

    println!("[Phase 10: apply_ai_safety_floor standalone]");
    assert_eq!(
        apply_ai_safety_floor(Decision::Allow, &CommandSource::AiGenerated),
        Decision::Confirm
    );
    assert_eq!(
        apply_ai_safety_floor(Decision::Confirm, &CommandSource::AiGenerated),
        Decision::Confirm
    );
    assert_eq!(
        apply_ai_safety_floor(Decision::Deny, &CommandSource::AiGenerated),
        Decision::Deny
    );
    println!("  Allow → Confirm, Confirm → Confirm, Deny → Deny  OK");

    println!("[Phase 10: code-fence stripping in response]");

    let bridge_fence = LlmBridge::new(
        StubClient::new("```bash\nfind /tmp -name '*.log'\n```"),
        "llama3",
        8_000,
    );
    let fenced = bridge_fence
        .translate_nl("find log files in tmp")
        .await
        .unwrap();
    println!("  argv = {:?}", fenced.argv);
    assert_eq!(fenced.argv[0], "find");
    println!("  code-fence stripping OK");

    // ── Optional: OllamaLlmClient path ───────────────────────────────────────

    #[cfg(feature = "ollama")]
    {
        println!("[Phase 10: OllamaLlmClient bridge — health probe]");
        use logicshell_llm::ollama::{HealthStatus, OllamaLlmClient};

        const BASE_URL: &str = "http://127.0.0.1:11434";
        const MODEL: &str = "llama3";

        let ollama = Arc::new(OllamaLlmClient::new(BASE_URL, MODEL, 30));
        match ollama.health_probe().await {
            Ok(HealthStatus::Healthy) => {
                println!("  Ollama healthy — running live translate_nl");
                let bridge = LlmBridge::new(ollama, MODEL, 8_000);
                match bridge
                    .translate_nl("list files in the current directory")
                    .await
                {
                    Ok(p) => println!("  live response: {:?}", p.argv),
                    Err(e) => println!("  generate error (graceful): {e}"),
                }
            }
            Ok(s) => println!("  Ollama not ready ({s:?}) — skipping live call"),
            Err(e) => println!("  Ollama unreachable: {e} — skipping live call"),
        }
    }

    println!("\n✓ Phase 10 features verified OK");
}
