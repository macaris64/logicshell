# LLM Guide — Running Ollama with LogicShell

This guide explains how to run a local Ollama daemon alongside LogicShell so that the LLM bridge (Phase 10) can translate natural language into shell commands and suggest corrections for failed commands.

---

## Prerequisites

| Tool | Version | Purpose |
|:-----|:--------|:--------|
| Ollama | any | Local LLM daemon |
| Rust toolchain | 1.75+ | Building LogicShell |
| llama3 (or another model) | any | Language model for inference |

---

## 1. Install Ollama

```bash
# Linux / macOS
curl -fsSL https://ollama.ai/install.sh | sh

# Or via package manager (macOS)
brew install ollama
```

Verify installation:

```bash
ollama --version
```

---

## 2. Start the Ollama Daemon

```bash
ollama serve
```

The daemon listens on `http://127.0.0.1:11434` by default. To use a different address, set:

```bash
OLLAMA_HOST=0.0.0.0:11434 ollama serve
```

Verify the daemon is running:

```bash
curl http://127.0.0.1:11434/api/tags
# Expected: {"models":[...]}
```

---

## 3. Pull a Model

```bash
# Pull llama3 (default model in LogicShell config)
ollama pull llama3

# Or a smaller/faster model
ollama pull mistral
ollama pull codellama
```

List available models:

```bash
ollama list
```

---

## 4. Configure LogicShell

Create or update `.logicshell.toml` in your project root:

```toml
schema_version = 1
safety_mode = "balanced"   # strict | balanced | loose

[llm]
enabled = true
provider = "ollama"
base_url = "http://127.0.0.1:11434"
model = "llama3"           # must match a pulled model
timeout_secs = 60
allow_remote = false

[llm.invocation]
nl_session = true              # enable natural-language mode
assist_on_not_found = true     # suggest corrections on exit 127
max_context_chars = 8000       # combined prompt cap
```

---

## 5. Build LogicShell with Ollama Support

The `ollama` feature flag enables `OllamaLlmClient` (requires `reqwest`):

```bash
# Build with Ollama HTTP client
cargo build --workspace --features ollama

# Run tests (includes mockito-backed Ollama tests)
cargo test --workspace --features ollama

# Run the Phase 10 demo (no live Ollama required — uses a stub client)
cargo run --example phase10 --package logicshell-llm

# Run the Phase 9 demo (health probe + optional live inference)
cargo run --example phase9 --package logicshell-llm --features ollama
```

---

## 6. Using LlmBridge in Your Code

### Natural-language to command (NL session mode)

```rust
use std::sync::Arc;
use logicshell_core::config::{LlmConfig, SafetyConfig, SafetyMode};
use logicshell_llm::{LlmBridge, apply_ai_safety_floor};

#[cfg(feature = "ollama")]
use logicshell_llm::ollama::OllamaLlmClient;

#[tokio::main]
async fn main() {
    #[cfg(feature = "ollama")]
    {
        let config = LlmConfig {
            enabled: true,
            model: Some("llama3".into()),
            ..LlmConfig::default()
        };

        let client = Arc::new(
            OllamaLlmClient::new(&config.base_url, config.model.as_deref().unwrap(), config.timeout_secs)
        );

        let bridge = LlmBridge::from_config(client, &config).expect("bridge config valid");

        // Translate natural language to a command
        match bridge.translate_nl("list all rust files recursively").await {
            Ok(proposed) => {
                // AI-generated commands always return at least Decision::Confirm
                let (assessment, decision) = proposed.evaluate_safety(
                    SafetyMode::Balanced,
                    &SafetyConfig::default(),
                );
                println!("Suggested: {:?}", proposed.argv);
                println!("Safety: {decision:?} (score: {})", assessment.score);
                println!("Raw response: {:?}", proposed.raw_response);
                // Dispatch only after user confirms...
            }
            Err(e) => eprintln!("LLM error (falling back to manual): {e}"),
        }
    }
}
```

### Assist-on-127 (correction mode)

```rust
// When a command returns exit code 127 (not found), ask the LLM for a correction
match bridge.assist_on_127(&["gti", "status"]).await {
    Ok(proposed) => {
        println!("Did you mean: {:?}", proposed.argv);
        // Ask user to confirm before dispatching
    }
    Err(e) => eprintln!("Suggestion unavailable: {e}"),
}
```

### Health probe (check before using)

```rust
#[cfg(feature = "ollama")]
{
    use logicshell_llm::ollama::{HealthStatus, OllamaLlmClient};

    let client = OllamaLlmClient::new("http://127.0.0.1:11434", "llama3", 10);
    match client.health_probe().await {
        Ok(HealthStatus::Healthy)              => println!("Ready"),
        Ok(HealthStatus::ModelMissing)         => println!("Run: ollama pull llama3"),
        Ok(HealthStatus::UnexpectedStatus(n))  => println!("Daemon error: HTTP {n}"),
        Err(e)                                 => println!("Daemon unreachable: {e}"),
    }
}
```

---

## 7. AI Safety Floor

All commands produced by `LlmBridge` have `source: CommandSource::AiGenerated`. When evaluated through `ProposedCommand::evaluate_safety`, the safety floor is applied:

| Base Decision | After AI Floor |
|:-------------|:--------------|
| `Allow`      | `Confirm`      |
| `Confirm`    | `Confirm`      |
| `Deny`       | `Deny`         |

This means **AI-generated commands always require explicit user confirmation** — they are never silently dispatched, regardless of the safety mode.

```rust
use logicshell_llm::{apply_ai_safety_floor, CommandSource};
use logicshell_core::Decision;

// Floor function can be applied standalone
let decision = apply_ai_safety_floor(Decision::Allow, &CommandSource::AiGenerated);
assert_eq!(decision, Decision::Confirm);
```

---

## 8. Graceful Degradation

If Ollama is not running, `LlmBridge` returns `LlmError::Http`. Your application should fall back gracefully:

```rust
match bridge.translate_nl("show disk usage").await {
    Ok(proposed) => { /* use the proposed command */ }
    Err(logicshell_llm::LlmError::Http(msg)) => {
        eprintln!("LLM unavailable ({msg}), please enter command manually");
        // Fall back to manual input
    }
    Err(e) => eprintln!("Unexpected error: {e}"),
}
```

---

## 9. Running the Live Tests

Tests that require a running Ollama daemon are tagged `#[ignore]`:

```bash
# Run live Ollama tests (requires: ollama serve + ollama pull llama3)
cargo test --package logicshell-llm --features ollama -- --ignored

# Run all tests including live
cargo test --workspace --features ollama -- --include-ignored
```

---

## 10. Troubleshooting

| Problem | Solution |
|:--------|:---------|
| `LlmError::Http("connection refused")` | Start Ollama: `ollama serve` |
| `HealthStatus::ModelMissing` | Pull the model: `ollama pull llama3` |
| Slow responses | Increase `timeout_secs` in `[llm]` config |
| Wrong command suggestions | Try a larger model: `ollama pull llama3:70b` |
| Response not parsed | LLM returned explanation text — check `raw_response` and file an issue |
| `LlmError::ContextTooLarge` | Increase `max_context_chars` or shorten the NL input |

---

## 11. Supported Models

Any model available via `ollama pull` works. Recommended models for command generation:

| Model | Size | Notes |
|:------|:-----|:------|
| `llama3` | 4.7 GB | Good balance of speed and accuracy |
| `codellama` | 3.8 GB | Optimized for code/shell commands |
| `mistral` | 4.1 GB | Fast and capable |
| `llama3:70b` | 39 GB | Highest accuracy, requires GPU |
| `phi3` | 2.3 GB | Lightweight, good on CPU |

---

## 12. Configuration Reference

```toml
[llm]
enabled = true                      # master switch (default: false)
provider = "ollama"                 # only supported provider in M3
base_url = "http://127.0.0.1:11434" # Ollama daemon URL
model = "llama3"                    # model name (required when enabled)
timeout_secs = 60                   # per-request HTTP timeout
allow_remote = false                # must be false (MVP: local only)

[llm.invocation]
nl_session = false          # enable NL session mode
assist_on_not_found = false # suggest on exit 127
max_context_chars = 8000    # combined prompt character cap
```

---

See [README.md](README.md) for the full project documentation and [CONTRIBUTING.md](CONTRIBUTING.md) for the TDD workflow.
