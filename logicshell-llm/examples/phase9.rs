// Phase 9 demo: OllamaLlmClient
//
// Usage (requires the `ollama` feature flag; live daemon is optional):
//   cargo run --example phase9 --package logicshell-llm --features ollama
//
// Without a live Ollama daemon the health probe shows graceful degradation and
// the example exits cleanly — no panics.

#[cfg(feature = "ollama")]
use logicshell_llm::{
    ollama::{HealthStatus, OllamaLlmClient},
    LlmClient, PromptComposer, SystemContextProvider,
};

#[cfg(not(feature = "ollama"))]
fn main() {
    println!("[Phase 9: OllamaLlmClient]");
    println!("  The `ollama` feature flag is not enabled.");
    println!("  Run: cargo run --example phase9 --package logicshell-llm --features ollama");
}

#[cfg(feature = "ollama")]
#[tokio::main]
async fn main() {
    const BASE_URL: &str = "http://127.0.0.1:11434";
    const MODEL: &str = "llama3";

    println!("[Phase 9: OllamaLlmClient constructor]");
    let client = OllamaLlmClient::new(BASE_URL, MODEL, 30);
    println!("  base_url = {:?}", client.base_url());
    println!("  model    = {:?}", client.model());
    assert_eq!(client.base_url(), BASE_URL);
    assert_eq!(client.model(), MODEL);
    println!("  constructor assertions OK");

    println!("[Phase 9: Health probe — graceful degradation matrix]");
    match client.health_probe().await {
        Ok(HealthStatus::Healthy) => {
            println!("  daemon reachable, model '{MODEL}' available ✓");
        }
        Ok(HealthStatus::ModelMissing) => {
            println!("  daemon reachable, but '{MODEL}' not listed — run: ollama pull {MODEL}");
        }
        Ok(HealthStatus::UnexpectedStatus(code)) => {
            println!("  daemon responded with unexpected HTTP {code}");
        }
        Err(e) => {
            println!("  daemon unreachable (graceful degradation): {e}");
            println!("  Start Ollama with: ollama serve");
            println!("\n✓ Phase 9 graceful-degradation path verified OK");
            return;
        }
    }

    println!("[Phase 9: PromptComposer → OllamaLlmClient pipeline]");
    let snap = SystemContextProvider::new().snapshot();
    let composer = PromptComposer::new(MODEL, 8_000);

    // NL-to-command path
    match composer.compose_nl_to_command("list files sorted by size", &snap) {
        Ok(req) => {
            println!("  prompt composed ({} chars)", req.prompt.len());
            match client.complete(req).await {
                Ok(resp) => {
                    println!("  model    = {:?}", resp.model);
                    println!("  response = {:?}", resp.text);
                    assert!(!resp.text.is_empty(), "response must not be empty");
                    println!("  nl-to-command round-trip OK");
                }
                Err(e) => println!("  generate error (graceful): {e}"),
            }
        }
        Err(e) => println!("  compose error: {e}"),
    }

    // Assist-on-127 path
    match composer.compose_assist_on_127(&["gti", "status"], &snap) {
        Ok(req) => {
            println!(
                "  assist-on-127 prompt composed ({} chars)",
                req.prompt.len()
            );
            match client.complete(req).await {
                Ok(resp) => println!("  assist-on-127 response: {:?}", resp.text),
                Err(e) => println!("  generate error (graceful): {e}"),
            }
        }
        Err(e) => println!("  compose error: {e}"),
    }

    println!("\n✓ Phase 9 features verified OK");
}
