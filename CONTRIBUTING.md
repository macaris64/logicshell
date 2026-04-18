# Contributing to LogicShell

This document describes how to work on **LogicShell Framework** in alignment with [LogicShell Framework PRD.md](LogicShell%20Framework%20PRD.md) (Version 2.0.0-draft): **TDD**, **coverage gates**, and **trait-based** LLM integration.

---

## Principles

- **Test-first:** No feature ships without tests (Framework PRD §6).  
- **Requirements traceability:** Prefer test names or comments that reference **FR-xx** / **NFR-xx** where practical (§6.3).  
- **Library-first:** Public API stability and embeddability matter as much as CLI ergonomics (§3.1).

---

## TDD workflow

Follow **Red–Green–Refactor** (§6.1):

1. **Red:** Add a failing test that specifies one behavior (one concern per test when possible).  
2. **Green:** Implement the minimum code to pass.  
3. **Refactor:** Improve structure, names, and boundaries while keeping tests green.

For cross-cutting behavior (safety modes, LLM-off paths), add tests that lock **acceptance criteria** from the PRD tables (§7–8), not only implementation details.

---

## Running tests

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

Integration tests that require a local Ollama daemon must be marked `#[ignore]` so default CI remains deterministic (LLM Module PRD: no GPU/inference required for `cargo test`).

---

## Coverage (90% target)

The framework targets **≥90% line coverage** on agreed scope (Framework PRD §11.3–11.4). Use **cargo-tarpaulin** (or the project’s documented successor) locally and in CI:

```bash
cargo tarpaulin --out Html --output-dir target/coverage
```

**Scope policy (typical):**

- **Include:** Dispatch, safety policy, config loading, LLM bridge orchestration, prompt composition, parsers.  
- **Exclude:** Generated code, trivial `main` shims, or third-party vendored snippets—list exclusions in CI config or `tarpaulin.toml` when introduced.

Fail the pipeline if coverage drops below the threshold on `main`, per §11.4.

---

## Implementing the LLM inference trait (`LlmClient`)

The product PRD names the async trait **`LlmClient`** (see [LogicShell LLM Module PRD.md](LogicShell%20LLM%20Module%20PRD.md) §5.1). Colloquially this is the **inference provider** boundary; new backends (e.g. additional HTTP APIs) implement **`LlmClient`**, not a separate “`LlmProvider`” name unless the codebase standardises otherwise.

### Contract

- **Signature (conceptual):** `complete(request: LlmRequest) -> Result<LlmResponse, LlmError>`.  
- **No environment reads:** Implementations must **not** call `std::env` or discover cwd for prompts; **system context** is supplied via `SystemContextSnapshot` assembled upstream (FR-10, FR-11).  
- **Wire format:** MVP targets **Ollama-compatible HTTP** (`OllamaLlmClient`); parsing and serialization must be unit-testable with **fixture JSON** (no network in unit tests).

### Steps to add or change a backend

1. **Define or extend** `LlmRequest` / `LlmResponse` only if the abstraction truly needs it; avoid leaking vendor JSON into safety or dispatch layers.  
2. **Implement `LlmClient`** for the backend (e.g. `OllamaLlmClient`).  
3. **Unit tests:** Deserialize canned responses; reject malformed payloads with `LlmError`.  
4. **Integration tests:** Use **mockito** (or equivalent) against a loopback server to assert path, headers, and body for one or two golden requests (FR-21).  
5. **Bridge:** `LlmBridge` composes prompts and calls `LlmClient`; it never executes argv without **SafetyPolicyEngine** (Framework PRD §11.1).

### Async

`LlmClient` is **async** because inference is I/O-bound (NFR-05). Use **`async_trait`** if needed for object safety; keep blocking work out of the runtime (Framework PRD §11.2: Tokio).

---

## Pull request checklist

- [ ] Tests added or updated for the change.  
- [ ] `cargo fmt`, `cargo clippy -D warnings`, `cargo test` pass.  
- [ ] Coverage meets project threshold on scoped crates/modules.  
- [ ] If touching LLM or safety: confirm **FR-24** (graceful degradation when AI unavailable) and **FR-31** (AI proposals gated by policy) remain covered.

---

## References

- [LogicShell Framework PRD.md](LogicShell%20Framework%20PRD.md) — methodology, FR/NFR, CI expectations.  
- [LogicShell LLM Module PRD.md](LogicShell%20LLM%20Module%20PRD.md) — `LlmClient`, `LlmBridge`, testing split.  
- [TESTING_STRATEGY.md](TESTING_STRATEGY.md) — pyramid, mocks, process isolation.  
- [ARCHITECTURE.md](ARCHITECTURE.md) — component boundaries.
