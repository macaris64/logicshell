# LogicShell

> A TDD-engineered, AI-augmented OS execution framework — embeddable Rust library for safe, auditable, policy-driven command dispatch with optional local LLM assistance.

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![Coverage](https://img.shields.io/badge/coverage-98%25-brightgreen)](#running-tests--coverage)

---

## What is LogicShell?

LogicShell is a **library-first** Rust framework that sits between a host application (or operator CLI) and the OS process layer. It provides:

- **Process dispatcher** — spawn external programs with explicit argv, environment, cwd, and stdin modes; capture bounded stdout; propagate structured exit status.
- **Pre-execution hooks** — run configurable shell scripts before every dispatch, with per-hook timeouts and fail-fast semantics.
- **Append-only audit log** — every dispatch writes a NDJSON record (timestamp, cwd, argv, safety decision, optional note) that survives process restarts.
- **Configuration discovery** — TOML config file resolved via `LOGICSHELL_CONFIG`, project walk-up, XDG, or built-in defaults, with strict unknown-key rejection.
- **Safety policy engine** — `strict` / `balanced` / `loose` modes with deny/allow prefix lists, high-risk regex patterns, sudo heuristics, and a four-category risk taxonomy (destructive filesystem, privilege elevation, network, package).
- **Local LLM bridge** _(Phases 8–10, planned)_ — Ollama-backed natural-language-to-command translation, gated behind safety policy and explicit user confirmation.

LogicShell is **not** a POSIX shell replacement. It is an embeddable dispatcher + policy + optional-AI stack that host applications link as a crate.

---

## Project status

| Milestone | Phases | Status |
|:----------|:-------|:-------|
| **M1** — Dispatcher, config, CI | 1–5 | ✅ Complete |
| **M2** — Safety engine, audit, hooks | 6–7 | ✅ Complete |
| **M3** — LLM bridge, Ollama | 8–10 | 📋 Planned |
| **M4** — Ratatui TUI | — | 📋 Planned |
| **M5** — Remote LLM providers | — | 📋 Planned |

**Current:** 294 tests · **98%+ line coverage** · `cargo clippy -D warnings` clean

---

## Architecture

```
Host application / CLI
        │
        ▼
  LogicShell facade  ──── AuditSink (NDJSON, append-only)
        │
        ├─► HookRunner (pre_exec hooks, per-hook timeout)
        │
        ├─► SafetyPolicyEngine ──────────► AuditSink
        │
        ├─► ProcessDispatcher (tokio::process, stdout cap)
        │
        └─► LlmBridge [Phases 8-10]
                │
                └─► OllamaLlmClient (HTTP, mockable trait)
```

**Crate layout:**

```
logicshell-core/   — dispatcher, config, safety, audit, hooks (no HTTP)
logicshell-llm/    — LLM context, prompt composer, Ollama client (behind `ollama` feature)
```

**Key design decisions:**
- `SafetyPolicyEngine` and `PromptComposer` are **sync/pure** — no async in hot policy paths.
- `LlmClient` is an **async trait** — I/O-bound inference uses Tokio.
- No LLM calls in default `cargo test` (all mocked or gated behind `#[ignore]`).

---

## Prerequisites

| Tool | Version | Purpose |
|:-----|:--------|:--------|
| Rust toolchain | 1.75+ (stable) | Build and test |
| cargo-tarpaulin | latest | Coverage reports |
| Ollama _(optional)_ | any | Local LLM features (Phases 8–10) |

Install Rust via [rustup](https://rustup.rs/). The repository pins the toolchain:

```bash
# toolchain is automatically selected from rust-toolchain.toml
rustup show
```

Install tarpaulin once:

```bash
cargo install cargo-tarpaulin
```

---

## Setup

```bash
git clone https://github.com/logicshell/logicshell
cd logicshell
cargo build --workspace
```

No environment variables, daemons, or network access are required for the core build.

---

## Running tests & coverage

```bash
# Format check
cargo fmt --check

# Lint (warnings are errors)
cargo clippy --workspace --all-features -- -D warnings

# Full test suite (no network, no Ollama)
cargo test --workspace

# Coverage report (HTML + XML, threshold ≥ 90%)
cargo tarpaulin --workspace

# Open coverage report
xdg-open target/coverage/tarpaulin-report.html   # Linux
open target/coverage/tarpaulin-report.html        # macOS
```

Tests that require a live Ollama daemon are marked `#[ignore]` and excluded from default CI:

```bash
# Run Ollama-backed tests explicitly (requires local daemon)
cargo test --workspace -- --ignored
```

---

## Running the demo

A runnable example exercises every implemented phase end-to-end:

```bash
cargo run --example demo
```

Expected output:

```
[Phase 3–4: Config schema + discovery] config parsed + discovered OK
[Phase 5: Dispatcher] echo, nonzero exit, stdin, cwd, truncation, env OK
[Phase 6: HookRunner] success, nonzero exit, timeout OK
[Phase 6: AuditSink] 3 NDJSON records, flush-on-drop, disabled no-op OK
[Phase 6: LogicShell façade] hook ran, audit written, façade.audit() appended, failing hook aborted OK
[Phase 7: SafetyPolicyEngine] ls allow, rm -rf / deny, curl|bash strict-deny/balanced-confirm, sudo rm strict-confirm/loose-allow, dispatch blocked OK

✓ All features verified OK
```

---

## Configuration

LogicShell reads **`.logicshell.toml`** using the following search order (first match wins):

1. `LOGICSHELL_CONFIG` environment variable — must be an absolute path.
2. Walk up from the current working directory for `.logicshell.toml`.
3. `$XDG_CONFIG_HOME/logicshell/config.toml` (falls back to `~/.config/…`).
4. Built-in defaults — no file required.

### Full configuration reference

```toml
schema_version = 1          # forward-compatible migrations
safety_mode = "balanced"    # strict | balanced | loose

[llm]
enabled = false             # master switch; no LLM calls when false
provider = "ollama"         # ollama (MVP); openai/anthropic post-MVP
base_url = "http://127.0.0.1:11434"
model = "llama3"            # required when enabled = true
timeout_secs = 60
allow_remote = false        # must be false in MVP

[llm.invocation]
nl_session = false          # explicit natural-language mode
assist_on_not_found = false # suggest on exit code 127
max_context_chars = 8000    # combined prompt context cap

[safety]
deny_prefixes  = ["rm -rf /", "mkfs", "dd if="]
allow_prefixes = []
high_risk_patterns = [
  "rm\\s+-[rf]*r",
  "sudo\\s+",
  "curl.*\\|\\s*bash",
  "wget.*\\|\\s*sh",
]

[audit]
enabled = true
path = "/var/log/logicshell-audit.log"   # omit → OS temp dir

[[hooks.pre_exec]]
command    = ["notify-send", "dispatching"]
timeout_ms = 5000           # hook killed after this many ms

[limits]
max_stdout_capture_bytes = 1048576   # 1 MiB
max_llm_payload_bytes    = 256000
```

Unknown keys are **rejected at parse time** — the framework fails fast with file path and line number.

---

## Embedding LogicShell

Add `logicshell-core` to `Cargo.toml`:

```toml
[dependencies]
logicshell-core = { path = "path/to/logicshell-core" }  # crates.io once published
tokio = { version = "1", features = ["full"] }
```

Minimal host integration:

```rust
use logicshell_core::{
    LogicShell,
    config::Config,
    audit::{AuditRecord, AuditDecision},
    Decision, SafetyPolicyEngine,
};

#[tokio::main]
async fn main() {
    // Load config from .logicshell.toml (walk-up) or use defaults
    let config = logicshell_core::discover(std::env::current_dir().unwrap().as_path())
        .unwrap_or_default();

    let ls = LogicShell::with_config(config);

    // Query the safety engine directly (sync, pure)
    let (assessment, decision) = ls.evaluate_safety(&["rm", "-rf", "/"]);
    assert_eq!(decision, Decision::Deny);
    println!("risk score: {}, level: {:?}", assessment.score, assessment.level);

    // Dispatch a command; safety check + pre-exec hooks run automatically
    let exit_code = ls.dispatch(&["git", "status"]).await.expect("dispatch failed");
    println!("exit: {exit_code}");

    // Write a manual audit record (e.g. after a user-denied command)
    let record = AuditRecord::new(
        std::env::current_dir().unwrap().to_string_lossy(),
        vec!["rm".into(), "-rf".into(), "/".into()],
        AuditDecision::Deny,
    ).with_note("blocked by operator");
    ls.audit(&record).expect("audit failed");
}
```

---

## Audit log format

Each dispatch writes one JSON line to the configured file:

```json
{"timestamp_secs":1714000000,"cwd":"/home/user/project","argv":["git","push"],"decision":"allow"}
{"timestamp_secs":1714000001,"cwd":"/","argv":["rm","-rf","/"],"decision":"deny","note":"blocked by policy stub"}
```

Fields:

| Field | Type | Description |
|:------|:-----|:------------|
| `timestamp_secs` | `u64` | Unix timestamp (seconds) |
| `cwd` | `string` | Working directory at dispatch time |
| `argv` | `string[]` | Full command argv |
| `decision` | `"allow"` \| `"deny"` \| `"confirm"` | Safety policy outcome |
| `note` | `string?` | Optional annotation (omitted when absent) |

The file is opened with `O_APPEND`; records survive process restarts and are flushed on `AuditSink` drop.

---

## Use cases

### 1. Auditable DevOps scripts

Wrap deployment pipelines in LogicShell to capture every spawned command with timestamps and decisions. Replay the NDJSON log for incident forensics.

```toml
safety_mode = "strict"
[audit]
enabled = true
path = "/var/log/deploy-audit.log"
[[hooks.pre_exec]]
command = ["slack-notify", "deploying to prod"]
timeout_ms = 3000
```

### 2. AI-assisted terminal (planned — Phase 8+)

Enable `llm.invocation.assist_on_not_found = true` to have LogicShell query a local Ollama model when a command returns exit code 127. The suggested correction is presented for confirmation before running — never auto-executed.

```toml
[llm]
enabled = true
model = "llama3"
[llm.invocation]
assist_on_not_found = true
```

### 3. Embedded safety layer in custom tools

Link `logicshell-core` in a Rust CLI to enforce deny/allow lists before spawning any subprocess. Set `safety_mode = "strict"` to require explicit confirmation for all high-risk patterns.

### 4. Pre-exec hook orchestration

Run arbitrary scripts before every dispatch — health checks, secret injection, notification webhooks — with hard timeouts so a slow hook never hangs the pipeline.

---

## Next steps (roadmap)

### Phase 8 — LLM context + prompt composer

- `SystemContextProvider` — reads OS family, architecture, abbreviated PATH, cwd.
- `PromptComposer` — pure, sync, templates via `include_str!`, enforces `max_context_chars`.
- `LlmClient` async trait + `LlmRequest` / `LlmResponse` types.
- Build without the `ollama` feature; no HTTP deps in `logicshell-core`.

### Phase 9 — OllamaLlmClient

- `OllamaLlmClient` behind the `ollama` feature flag using `reqwest`.
- Health probe (`GET /api/tags`) with graceful degradation matrix.
- Full mockito test suite; zero real network in default `cargo test`.

### Phase 10 — LlmBridge + AI-safety integration

- `LlmBridge` orchestrates context → composer → client → parser → safety.
- `ProposedCommand` with `source: CommandSource::AiGenerated` raises the risk floor.
- NL session mode, argv-only mode, and assist-on-127 mode.
- Graceful degradation when Ollama is unreachable.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the TDD workflow, PR checklist, and LLM backend extension guide.

**Quick start:**

```bash
# All checks must pass before opening a PR
cargo fmt --check
cargo clippy --workspace --all-features -- -D warnings
cargo test --workspace
cargo tarpaulin --workspace   # must remain ≥ 90%
```

**Commit style:** conventional commits (`feat:`, `fix:`, `phase N:`, `test:`, `docs:`).

---

## Repository structure

```
logicshell/
├── logicshell-core/          # Core library (no HTTP)
│   ├── src/
│   │   ├── lib.rs            # Public façade: LogicShell struct
│   │   ├── dispatcher.rs     # Async process dispatcher (Phase 5)
│   │   ├── audit.rs          # Append-only NDJSON audit sink (Phase 6)
│   │   ├── hooks.rs          # Pre-exec hook runner (Phase 6)
│   │   ├── safety.rs         # Safety policy engine (Phase 7)
│   │   ├── error.rs          # LogicShellError enum (Phase 2)
│   │   └── config/
│   │       ├── mod.rs        # load() + validate()
│   │       ├── schema.rs     # Serde config types (Phase 3)
│   │       └── discovery.rs  # Config discovery (Phase 4)
│   ├── tests/
│   │   ├── dispatcher_integration.rs
│   │   ├── hooks_audit_integration.rs
│   │   └── e2e.rs            # Full-stack end-to-end tests
│   └── examples/
│       └── demo.rs           # Runnable feature demonstration
├── logicshell-llm/           # LLM bridge (Phases 8–10)
├── docs/
│   ├── PLAN.md
│   ├── ARCHITECTURE.md
│   ├── TESTING_STRATEGY.md
│   ├── LOGICSHELL_OPERATIONS.md
│   ├── LogicShell Framework PRD.md
│   └── LogicShell LLM Module PRD.md
├── tarpaulin.toml            # Coverage config (gate: ≥ 90%)
├── rust-toolchain.toml       # Pinned stable channel
└── Cargo.toml                # Workspace root
```

---

## License

Licensed under either of

- **MIT License** ([LICENSE-MIT](LICENSE-MIT))
- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project shall be dual-licensed as above, without any additional terms or conditions.
