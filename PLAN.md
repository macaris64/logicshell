# LogicShell — Implementation Plan

## Completed Milestones

### M1 — Core Dispatcher + Config (Phases 1–5) ✅

| Phase | Feature | Status |
|:------|:--------|:-------|
| 1 | Workspace bootstrap, CI skeleton | ✅ |
| 2 | `LogicShellError` enum, project structure | ✅ |
| 3 | Config schema (`Config`, `LlmConfig`, etc.) | ✅ |
| 4 | Config discovery (env var, walk-up, XDG) | ✅ |
| 5 | Async `ProcessDispatcher` (tokio::process, stdout cap) | ✅ |

### M2 — Safety, Audit, Hooks (Phases 6–7) ✅

| Phase | Feature | Status |
|:------|:--------|:-------|
| 6 | `AuditSink` (NDJSON, O_APPEND), `HookRunner`, `LogicShell` façade | ✅ |
| 7 | `SafetyPolicyEngine` (strict/balanced/loose, deny/allow lists, regexes) | ✅ |

### M3 — LLM Bridge + Ollama (Phases 8–10) ✅

| Phase | Feature | Status |
|:------|:--------|:-------|
| 8 | `SystemContextProvider`, `PromptComposer`, `LlmClient` trait | ✅ |
| 9 | `OllamaLlmClient` (reqwest, health probe, mockito tests) | ✅ |
| 10 | `LlmBridge<C>`, `ProposedCommand`, `CommandSource`, parser, AI safety floor | ✅ |

**Current state:** 506 tests · 96%+ coverage · clippy clean · `cargo fmt` clean

---

## M4 — Ratatui TUI (Phases 11–14)

### Phase 11 — TUI foundation

**Goal:** Introduce an interactive terminal UI shell powered by [Ratatui](https://ratatui.rs/) that wraps the `LogicShell` façade. The TUI is a thin presentation layer — all business logic stays in `logicshell-core` and `logicshell-llm`.

**Deliverables:**
- New crate `logicshell-tui` in the workspace.
- `App` struct: terminal state machine with `Running` / `Quitting` lifecycle.
- Raw-mode terminal setup / teardown via `crossterm`.
- Minimal event loop: keyboard input (`Ctrl-C` / `q` to quit, `Enter` to submit).
- Configurable prompt widget showing current working directory.
- Static "welcome" layout with status bar (phase, version, safety mode).
- Unit-testable event dispatch without a real terminal (mock backend).

**Tests:** App state machine, event routing, layout rendering to buffer.

---

### Phase 12 — Command input + history

**Goal:** Full-featured input line with readline-like editing and session history.

**Deliverables:**
- `InputWidget` with cursor tracking, character insert/delete, Home/End.
- Arrow-key history navigation (in-memory `VecDeque<String>`).
- Ctrl-A (beginning of line), Ctrl-E (end), Ctrl-K (kill to end).
- History persistence to `~/.local/share/logicshell/history` (one command per line, 1 000 entry cap).
- `HistoryStore` abstraction (sync, pure) — testable without the TUI.

**Tests:** InputWidget cursor math, history ring-buffer, persistence round-trip.

---

### Phase 13 — TUI dispatch + output panel

**Goal:** Wire the TUI input to `LogicShell::dispatch` and display stdout/stderr in a scrollable panel.

**Deliverables:**
- `OutputPanel` widget with ring buffer (configurable line cap, default 500).
- Dispatch stdout streamed line-by-line into the panel (no full capture in memory).
- Safety confirm dialog: when `Decision::Confirm`, show a modal overlay asking `[y/N]` before dispatching.
- Deny banner: red status bar when a command is blocked.
- Async dispatch on a separate Tokio task; TUI remains responsive during execution.
- Exit code / duration shown in the status bar after each command.

**Tests:** OutputPanel scroll math, confirm dialog state, deny-banner render, dispatch-task cancellation.

---

### Phase 14 — NL mode in TUI + LLM status widget

**Goal:** Surface the Phase 10 `LlmBridge` inside the TUI with a first-class UX for AI-assisted command entry.

**Deliverables:**
- Toggle NL mode with `Ctrl-L`; indicator in status bar (`NL` badge).
- In NL mode, input is sent to `LlmBridge::translate_nl` instead of directly dispatched.
- `LlmStatusWidget`: spinner during inference, health-probe result on startup.
- Proposed command shown in a preview pane before the confirm dialog.
- Keyboard shortcut `Ctrl-E` to edit the proposed command before confirming.
- Graceful degradation: if Ollama is down, fall back to direct dispatch with a warning.

**Tests:** NL mode toggle, preview rendering, edit-before-confirm workflow, degradation path.

---

## M5 — Remote LLM Providers (Phases 15–17)

### Phase 15 — Provider abstraction

**Goal:** Refactor `LlmClient` into a provider-agnostic interface with runtime selection.

**Deliverables:**
- `LlmProvider` enum extended: `Ollama`, `OpenAi`, `Anthropic`.
- `LlmClientFactory::build(config: &LlmConfig) -> Box<dyn LlmClientExt>` — dyn-compatible via `BoxFuture` wrapper.
- `allow_remote = true` validation: must be explicitly opted in; rejected in MVP paths.
- Provider selection stored in `Config` and surfaced in the TUI status bar.
- Feature flags: `openai` (reqwest + JSON), `anthropic` (reqwest + streaming SSE).

**Tests:** Factory routing, allow_remote guard, feature-flag compile-time gating.

---

### Phase 16 — OpenAI-compatible client

**Goal:** Implement an `OpenAiLlmClient` targeting the OpenAI Chat Completions API.

**Deliverables:**
- `POST /v1/chat/completions` with `{"model","messages":[{role,content}],"stream":false}`.
- API key from `OPENAI_API_KEY` env var; never in config file.
- Response: extract `choices[0].message.content`.
- Retry logic: exponential back-off on 429 (rate limit), up to 3 retries.
- Mockito test suite for happy path, 429 retry, 401 auth error.
- Live smoke test tagged `#[ignore]`.

**Tests:** Wire-type deserialization, retry state machine, auth error propagation.

---

### Phase 17 — Anthropic Claude client

**Goal:** Implement an `AnthropicLlmClient` targeting the Messages API with prompt caching.

**Deliverables:**
- `POST /v1/messages` with system prompt (OS context) as a `cache_control: ephemeral` block.
- Response: extract `content[0].text`.
- `cache_read_input_tokens` / `cache_creation_input_tokens` logged at TRACE level.
- `ANTHROPIC_API_KEY` env var; `anthropic-version` header pinned.
- Streaming disabled in MVP; non-streaming response only.
- Mockito suite + live `#[ignore]` test.

**Tests:** Cache header sent, response extraction, cache metrics logged.

---

## M6 — Plugin System (Phases 18–20)

### Phase 18 — Plugin trait + loader

**Goal:** Allow third-party Rust crates (or WASM modules) to extend the dispatch pipeline with custom pre/post hooks and safety rules.

**Deliverables:**
- `LogicShellPlugin` trait: `fn name(&self) -> &str`, `fn on_pre_dispatch(&self, argv: &[&str]) -> PluginDecision`, `fn on_post_dispatch(&self, argv: &[&str], exit: i32)`.
- `PluginRegistry`: runtime list of boxed plugins, iterated in registration order.
- `LogicShell::register_plugin(plugin: Box<dyn LogicShellPlugin>)`.
- Native plugin loading from a shared library via `libloading` (optional `plugin-native` feature).
- `PluginDecision::Allow | Deny(reason) | Passthrough`.

**Tests:** Registry ordering, deny short-circuits dispatch, multiple plugin composition.

---

### Phase 19 — WASM plugin sandbox

**Goal:** Load plugins compiled to WebAssembly for cross-platform, sandboxed extensibility.

**Deliverables:**
- `wasmtime`-backed `WasmPluginLoader` behind the `plugin-wasm` feature.
- ABI: plugins export `on_pre_dispatch(argv_json_ptr, len) -> i32` (0 = allow, 1 = deny).
- Resource limits: max memory 4 MiB, max execution time 50 ms (via `wasmtime::Limits`).
- `PluginManifest` (TOML): name, path, permissions, version.
- Sandbox escape prevention: no host filesystem or network access by default.

**Tests:** ABI call, memory limit enforcement, timeout kill, manifest parsing.

---

### Phase 20 — Plugin marketplace + CLI

**Goal:** `logicshell-cli` binary with `plugin` subcommands for discovering and managing plugins.

**Deliverables:**
- New crate `logicshell-cli` with `clap`-powered CLI.
- `logicshell plugin list` — show registered plugins and their versions.
- `logicshell plugin install <path|url>` — validate, copy, update config.
- `logicshell plugin remove <name>` — remove from config and filesystem.
- `logicshell plugin test <name>` — smoke-test a plugin against a configurable argv.
- Plugin directory: `$XDG_DATA_HOME/logicshell/plugins/`.
- Plugin signature verification (SHA-256 hash in manifest).

**Tests:** CLI argument parsing, install/remove round-trip, signature check, path validation.

---

## Coverage & Quality Gates (all milestones)

| Check | Requirement |
|:------|:------------|
| `cargo fmt --check` | No diff |
| `cargo clippy --workspace --all-features -- -D warnings` | Zero warnings |
| `cargo test --workspace` | All pass, 0 ignored (except live-daemon tests) |
| `cargo tarpaulin --workspace` | ≥ 90% line coverage |
| `cargo build --workspace` | Zero warnings |

---

## Architecture Principles

1. **Library-first**: all user-facing features live in crates (`logicshell-core`, `logicshell-llm`, `logicshell-tui`). CLIs and TUIs are thin shells over library APIs.
2. **No LLM in hot paths**: `SafetyPolicyEngine` and `PromptComposer` are sync and pure — no async in critical policy paths (NFR-05).
3. **AI-generated commands always need confirmation**: `ProposedCommand::evaluate_safety` applies the AI safety floor regardless of safety mode.
4. **Feature flags**: `ollama`, `openai`, `anthropic`, `plugin-native`, `plugin-wasm` — pull in heavy dependencies only when needed.
5. **Zero real network in `cargo test`**: all HTTP-backed tests use mockito or mockall; live tests are `#[ignore]`.
6. **TDD discipline**: write the test first, make it pass, then refactor (red → green → clean).
