// Phase 8 demo: LLM context + prompt composer
//
// Usage:
//   cargo run --example phase8 --package logicshell-llm
//
// No network calls; demonstrates SystemContextProvider, PromptComposer,
// and the LlmClient trait without a live Ollama daemon.

use logicshell_core::config::LlmConfig;
use logicshell_llm::{
    LlmClient, LlmError, LlmRequest, LlmResponse, PromptComposer, SystemContextProvider,
};

/// Stub LlmClient that returns a hard-coded suggestion.
struct StubClient;

impl LlmClient for StubClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            text: "ls -lhS".into(),
            model: req.model,
        })
    }
}

#[tokio::main]
async fn main() {
    println!("[Phase 8: SystemContextProvider]");
    let provider = SystemContextProvider::new();
    let snap = provider.snapshot();
    assert!(!snap.os_family.is_empty(), "os_family must be set");
    assert!(!snap.arch.is_empty(), "arch must be set");
    assert!(!snap.cwd.is_empty(), "cwd must be set");
    println!("  os_family = {:?}", snap.os_family);
    println!("  arch      = {:?}", snap.arch);
    println!("  cwd       = {:?}", snap.cwd);
    println!("  PATH[0..] = {:?}", snap.path_dirs);

    println!("[Phase 8: PromptComposer — NL to command]");
    let cfg = LlmConfig {
        enabled: true,
        model: Some("llama3".into()),
        ..LlmConfig::default()
    };
    let composer = PromptComposer::from_config(&cfg).expect("composer from config");
    let req = composer
        .compose_nl_to_command("list files sorted by size", &snap)
        .expect("compose_nl_to_command");

    assert_eq!(req.model, "llama3");
    assert!(req.prompt.contains("list files sorted by size"));
    println!("  model  = {:?}", req.model);
    println!(
        "  prompt (first 80 chars) = {:?}",
        &req.prompt[..80.min(req.prompt.len())]
    );

    println!("[Phase 8: PromptComposer — assist on exit 127]");
    let req2 = composer
        .compose_assist_on_127(&["gti", "status"], &snap)
        .expect("compose_assist_on_127");
    assert!(req2.prompt.contains("gti status"));
    println!("  failed_cmd = \"gti status\" embedded in prompt: OK");

    println!("[Phase 8: LlmClient trait — stub round-trip]");
    let client = StubClient;
    let resp = client.complete(req).await.expect("complete");
    assert_eq!(resp.model, "llama3");
    assert!(!resp.text.is_empty());
    println!("  response = {:?}", resp.text);

    println!("[Phase 8: ContextTooLarge error]");
    let tight_composer = PromptComposer::new("m", 10);
    let snap2 = snap.clone();
    match tight_composer.compose_nl_to_command("ls", &snap2) {
        Err(logicshell_llm::LlmError::ContextTooLarge { size, max }) => {
            println!("  ContextTooLarge: size={size} > max={max} ✓");
        }
        other => panic!("expected ContextTooLarge, got: {other:?}"),
    }

    println!("\n✓ Phase 8 features verified OK");
}
