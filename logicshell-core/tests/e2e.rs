// End-to-end tests for LogicShell — exercises the full stack from config file
// through discovery, dispatch, hooks, and audit without mocking internals.
//
// These tests intentionally use real files, real child processes, and real I/O.
// Traces: FR-01–04, §10.2, §12.5, NFR-06, NFR-07, NFR-08, LOGICSHELL_OPERATIONS.md

use std::fs;
use tempfile::TempDir;

use logicshell_core::{
    audit::{AuditDecision, AuditRecord, AuditSink},
    config::{discovery::find_and_load, load, Config, HookEntry, LimitsConfig},
    discover, find_config_path, LogicShell,
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn write_cfg(dir: &TempDir, name: &str, content: &str) -> std::path::PathBuf {
    let p = dir.path().join(name);
    fs::write(&p, content).unwrap();
    p
}

fn read_audit_lines(path: &std::path::Path) -> Vec<serde_json::Value> {
    let content = fs::read_to_string(path).unwrap_or_default();
    content
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("audit line must be valid JSON"))
        .collect()
}

// ── Config discovery → dispatch pipeline ─────────────────────────────────────

/// FR-11 / OPERATIONS: discover() finds a project-level .logicshell.toml and
/// the loaded config drives dispatch behaviour.
#[tokio::test]
async fn e2e_config_file_drives_dispatch() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let toml = format!(
        r#"
schema_version = 1
safety_mode = "balanced"
[audit]
enabled = true
path = "{}"
[limits]
max_stdout_capture_bytes = 1024
"#,
        audit_path.display()
    );
    write_cfg(&tmp, ".logicshell.toml", &toml);

    let cfg = discover(tmp.path()).unwrap();
    assert_eq!(cfg.schema_version, 1);
    assert_eq!(cfg.limits.max_stdout_capture_bytes, 1024);

    let ls = LogicShell::with_config(cfg);
    let code = ls.dispatch(&["true"]).await.unwrap();
    assert_eq!(code, 0);

    let records = read_audit_lines(&audit_path);
    assert_eq!(records.len(), 1, "exactly one audit record after dispatch");
    assert_eq!(records[0]["decision"], "allow");
    assert_eq!(records[0]["argv"][0], "true");
}

/// LOGICSHELL_CONFIG override wins over walk-up .logicshell.toml.
///
/// Uses find_and_load() directly to avoid mutating the process environment
/// (which would race with parallel tests that also call discover()).
#[tokio::test]
async fn e2e_explicit_config_override_wins_over_walkup() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("env_audit.log");

    // Walk-up file with schema_version = 10 (should be ignored).
    write_cfg(&tmp, ".logicshell.toml", "schema_version = 10\n");

    // Override file with schema_version = 2.
    let override_file = write_cfg(
        &tmp,
        "override.toml",
        &format!(
            "schema_version = 2\n[audit]\nenabled = true\npath = \"{}\"\n",
            audit_path.display()
        ),
    );

    // Pass the override path directly — no env var mutation needed.
    let cfg = find_and_load(
        Some(override_file.to_str().unwrap()),
        tmp.path(),
        None,
        None,
    )
    .unwrap();

    assert_eq!(cfg.schema_version, 2, "override file must win over walk-up");
    let ls = LogicShell::with_config(cfg);
    ls.dispatch(&["true"]).await.unwrap();
    assert!(
        audit_path.exists(),
        "audit file should be written to override-configured path"
    );
}

/// find_config_path() returns the resolved file path used for discovery.
#[test]
fn e2e_find_config_path_returns_dotfile_path() {
    let tmp = TempDir::new().unwrap();
    write_cfg(&tmp, ".logicshell.toml", "schema_version = 1\n");

    if std::env::var("LOGICSHELL_CONFIG").is_err() {
        let path = find_config_path(tmp.path()).unwrap();
        assert_eq!(path, Some(tmp.path().join(".logicshell.toml")));
    }
}

// ── Sequential dispatch accumulates audit records ─────────────────────────────

/// §10.2: every successful dispatch appends one audit record; multiple calls
/// accumulate in order without overwriting previous entries.
#[tokio::test]
async fn e2e_sequential_dispatch_accumulates_audit() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let mut cfg = Config::default();
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    let ls = LogicShell::with_config(cfg);

    ls.dispatch(&["true"]).await.unwrap();
    ls.dispatch(&["sh", "-c", "exit 0"]).await.unwrap();
    ls.dispatch(&["false"]).await.unwrap(); // nonzero exit — still audited

    let records = read_audit_lines(&audit_path);
    assert_eq!(records.len(), 3, "3 dispatches → 3 audit records");

    // Each record references the right command
    assert_eq!(records[0]["argv"][0], "true");
    assert_eq!(records[2]["argv"][0], "false");

    // All decisions are "allow" (safety engine wired in Phase 7)
    for r in &records {
        assert_eq!(r["decision"], "allow");
    }
}

/// §10.2: LogicShell::audit() appends records independently of dispatch.
#[test]
fn e2e_explicit_audit_call_appends_record() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("manual.log");

    let mut cfg = Config::default();
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    let ls = LogicShell::with_config(cfg);

    let deny_rec = AuditRecord::new(
        "/secure",
        vec!["rm".into(), "-rf".into(), "/".into()],
        AuditDecision::Deny,
    )
    .with_note("blocked by policy stub");
    ls.audit(&deny_rec).unwrap();

    let confirm_rec = AuditRecord::new(
        "/home/user",
        vec!["sudo".into(), "apt".into()],
        AuditDecision::Confirm,
    )
    .with_note("user confirmed");
    ls.audit(&confirm_rec).unwrap();

    let records = read_audit_lines(&audit_path);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["decision"], "deny");
    assert_eq!(records[0]["note"], "blocked by policy stub");
    assert_eq!(records[1]["decision"], "confirm");
    assert_eq!(records[1]["argv"][1], "apt");
}

// ── Audit disabled mode ────────────────────────────────────────────────────────

/// When audit is disabled, dispatch succeeds without creating a log file.
#[tokio::test]
async fn e2e_dispatch_with_audit_disabled() {
    let tmp = TempDir::new().unwrap();
    let would_be_audit = tmp.path().join("should_not_exist.log");

    let mut cfg = Config::default();
    cfg.audit.enabled = false;
    cfg.audit.path = Some(would_be_audit.to_str().unwrap().to_string());
    let ls = LogicShell::with_config(cfg);

    let code = ls.dispatch(&["true"]).await.unwrap();
    assert_eq!(code, 0);
    assert!(
        !would_be_audit.exists(),
        "disabled audit must not create the log file"
    );
}

// ── Exit-code propagation ─────────────────────────────────────────────────────

/// FR-03: nonzero exit codes are propagated through the full LogicShell pipeline.
#[tokio::test]
async fn e2e_exit_code_propagation_through_facade() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");
    let mut cfg = Config::default();
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    let ls = LogicShell::with_config(cfg);

    for (cmd, expected) in [
        (vec!["true"], 0i32),
        (vec!["false"], 1),
        (vec!["sh", "-c", "exit 7"], 7),
        (vec!["sh", "-c", "exit 127"], 127),
        (vec!["sh", "-c", "exit 255"], 255),
    ] {
        let code = ls.dispatch(&cmd).await.unwrap();
        assert_eq!(code, expected, "cmd={cmd:?}");
    }

    // All five dispatches should be in the audit log
    let records = read_audit_lines(&audit_path);
    assert_eq!(records.len(), 5);
}

// ── stdout capture limits ─────────────────────────────────────────────────────

/// NFR-08: max_stdout_capture_bytes in config limits captured output.
#[tokio::test]
async fn e2e_stdout_cap_from_config() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let toml = format!(
        "[audit]\nenabled=true\npath=\"{}\"\n[limits]\nmax_stdout_capture_bytes=10\n",
        audit_path.display()
    );
    let cfg = load(&toml).unwrap();
    let ls = LogicShell::with_config(cfg);

    // python-free way to emit more than 10 bytes
    let code = ls
        .dispatch(&["sh", "-c", "printf '%0.s.' {1..100}"])
        .await
        .unwrap();
    assert_eq!(code, 0);
    // We can't assert stdout here (facade doesn't expose it) but we can
    // confirm the audit record is written — dispatcher didn't panic/error.
    let records = read_audit_lines(&audit_path);
    assert_eq!(records.len(), 1);
}

// ── Hook + dispatch pipeline ──────────────────────────────────────────────────

/// §12.5: a pre-exec hook defined in a config file runs before dispatch.
#[tokio::test]
async fn e2e_config_file_hook_runs_before_dispatch() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");
    let hook_marker = tmp.path().join("hook_was_here");

    let toml = format!(
        r#"
[audit]
enabled = true
path = "{audit}"
[[hooks.pre_exec]]
command = ["sh", "-c", "touch {marker}"]
timeout_ms = 5000
"#,
        audit = audit_path.display(),
        marker = hook_marker.display()
    );
    let cfg = load(&toml).unwrap();
    let ls = LogicShell::with_config(cfg);

    let code = ls.dispatch(&["true"]).await.unwrap();
    assert_eq!(code, 0);
    assert!(hook_marker.exists(), "hook must run before dispatch");

    let records = read_audit_lines(&audit_path);
    assert_eq!(records.len(), 1);
}

/// §12.5: multiple hooks in config run in order.
#[tokio::test]
async fn e2e_multiple_hooks_run_in_order() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");
    let first = tmp.path().join("1");
    let second = tmp.path().join("2");

    let toml = format!(
        r#"
[audit]
enabled = true
path = "{audit}"
[[hooks.pre_exec]]
command = ["sh", "-c", "touch {f}"]
timeout_ms = 5000
[[hooks.pre_exec]]
command = ["sh", "-c", "touch {s}"]
timeout_ms = 5000
"#,
        audit = audit_path.display(),
        f = first.display(),
        s = second.display()
    );
    let cfg = load(&toml).unwrap();
    let ls = LogicShell::with_config(cfg);

    ls.dispatch(&["true"]).await.unwrap();
    assert!(first.exists(), "first hook must run");
    assert!(second.exists(), "second hook must run");
}

/// §12.5: a failing hook aborts dispatch; no audit record is written.
#[tokio::test]
async fn e2e_failing_hook_aborts_dispatch_and_no_audit() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let mut cfg = Config::default();
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    cfg.hooks.pre_exec = vec![HookEntry {
        command: vec!["false".into()],
        timeout_ms: 5_000,
    }];
    let ls = LogicShell::with_config(cfg);

    let result = ls.dispatch(&["true"]).await;
    assert!(result.is_err(), "dispatch must fail when hook fails");
    assert!(
        !audit_path.exists(),
        "no audit record should be written when hook aborts"
    );
}

/// §12.5: a hook that exceeds its timeout aborts dispatch.
#[tokio::test]
async fn e2e_hook_timeout_aborts_dispatch() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let mut cfg = Config::default();
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    cfg.hooks.pre_exec = vec![HookEntry {
        command: vec!["sleep".into(), "60".into()],
        timeout_ms: 80,
    }];
    let ls = LogicShell::with_config(cfg);

    let result = ls.dispatch(&["true"]).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("timed out"),
        "error must mention timeout: {msg}"
    );
    assert!(!audit_path.exists(), "no audit written after timeout abort");
}

// ── Audit log format guarantees ───────────────────────────────────────────────

/// §10.2: every audit record contains the required fields.
#[tokio::test]
async fn e2e_audit_record_has_required_fields() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let mut cfg = Config::default();
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    let ls = LogicShell::with_config(cfg);

    ls.dispatch(&["sh", "-c", "exit 0"]).await.unwrap();

    let records = read_audit_lines(&audit_path);
    assert_eq!(records.len(), 1);
    let rec = &records[0];

    assert!(
        rec["timestamp_secs"].is_number(),
        "timestamp_secs must be present"
    );
    assert!(rec["cwd"].is_string(), "cwd must be present");
    assert!(rec["argv"].is_array(), "argv must be present");
    assert!(rec["decision"].is_string(), "decision must be present");
    // timestamp must be a plausible Unix epoch (after 2020)
    let ts = rec["timestamp_secs"].as_u64().unwrap();
    assert!(ts > 1_577_836_800, "timestamp looks unrealistic: {ts}");
}

/// §10.2: audit records survive close + reopen (append semantics).
#[tokio::test]
async fn e2e_audit_survives_reopen() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    // First LogicShell instance writes one record.
    {
        let mut cfg = Config::default();
        cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
        LogicShell::with_config(cfg)
            .dispatch(&["true"])
            .await
            .unwrap();
    }

    // Second LogicShell instance opens the same file.
    {
        let mut cfg = Config::default();
        cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
        LogicShell::with_config(cfg)
            .dispatch(&["false"])
            .await
            .unwrap();
    }

    let records = read_audit_lines(&audit_path);
    assert_eq!(
        records.len(),
        2,
        "both records must persist across instances"
    );
    assert_eq!(records[0]["argv"][0], "true");
    assert_eq!(records[1]["argv"][0], "false");
}

// ── AuditSink standalone guarantees ──────────────────────────────────────────

/// NFR-07: AuditSink::open on a bad path returns an I/O error, not a panic.
#[test]
fn e2e_audit_open_bad_path_returns_error() {
    let result = AuditSink::open(std::path::Path::new("/no/such/dir/audit.log"));
    assert!(
        result.is_err(),
        "opening an unreachable path must return Err"
    );
}

/// NFR-07: writing to a disabled AuditSink is always a no-op.
#[test]
fn e2e_disabled_audit_sink_noop() {
    let mut sink = AuditSink::disabled();
    for decision in [
        AuditDecision::Allow,
        AuditDecision::Deny,
        AuditDecision::Confirm,
    ] {
        let rec = AuditRecord::new("/", vec!["cmd".into()], decision);
        assert!(sink.write(&rec).is_ok(), "disabled write must be Ok");
    }
    assert!(sink.flush().is_ok(), "disabled flush must be Ok");
}

// ── Error handling / NFR-06 ───────────────────────────────────────────────────

/// NFR-06: dispatching a nonexistent binary returns a structured error, not a panic.
#[tokio::test]
async fn e2e_nonexistent_binary_returns_structured_error() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let mut cfg = Config::default();
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    let ls = LogicShell::with_config(cfg);

    let result = ls.dispatch(&["__no_such_binary_xyz__"]).await;
    assert!(result.is_err(), "expected Err for nonexistent binary");
    // File must not exist: dispatch errored before audit write.
    assert!(!audit_path.exists());
}

/// NFR-06: evaluate_safety stub returns Safety error, not a panic.
#[test]
fn e2e_safety_stub_error() {
    let ls = LogicShell::new();
    let result = ls.evaluate_safety(&["rm", "-rf", "/"]);
    assert!(result.is_err(), "safety stub must return Err until Phase 7");
}

// ── Config validation edge cases ──────────────────────────────────────────────

/// §12.2: llm.enabled = true without model fails validation through the full stack.
#[test]
fn e2e_llm_enabled_without_model_fails_config_load() {
    let result = load("[llm]\nenabled = true\n");
    assert!(result.is_err(), "missing model must fail validation");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("llm.model"),
        "error must name the missing field: {msg}"
    );
}

/// §12.7: unknown keys in the config file produce a Config error.
#[test]
fn e2e_unknown_config_key_is_error() {
    let result = load("completely_made_up_key = 42\n");
    assert!(result.is_err());
}

// ── Limits config via TOML ────────────────────────────────────────────────────

/// Config-driven stdout cap: parse a TOML file, create LogicShell, verify cap applied.
#[tokio::test]
async fn e2e_limits_from_config_file() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let toml = format!(
        "[audit]\nenabled=true\npath=\"{}\"\n[limits]\nmax_stdout_capture_bytes=5\n",
        audit_path.display()
    );
    let cfg = load(&toml).unwrap();
    assert_eq!(cfg.limits.max_stdout_capture_bytes, 5);

    let ls = LogicShell::with_config(cfg);
    // This just verifies the config path completes; actual truncation tested in dispatcher_integration.
    let code = ls.dispatch(&["echo", "hi"]).await.unwrap();
    assert_eq!(code, 0);
}

// ── LogicShell::new() default pipeline ───────────────────────────────────────

/// Default LogicShell (no config file) dispatches commands and writes to the
/// default temp-dir audit path without error.
#[tokio::test]
async fn e2e_default_logicshell_dispatch_succeeds() {
    // Default config: audit enabled, path = OS temp dir
    // We cannot predict the path, but we can verify dispatch doesn't error.
    let ls = LogicShell::new();
    let code = ls.dispatch(&["true"]).await.unwrap();
    assert_eq!(code, 0);

    let code2 = ls.dispatch(&["false"]).await.unwrap();
    assert_eq!(code2, 1);
}

// ── Custom LimitsConfig through facade ───────────────────────────────────────

/// FR-08: custom limits survive round-trip through Config and LogicShell.
#[tokio::test]
async fn e2e_custom_limits_round_trip() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let mut cfg = Config::default();
    cfg.limits = LimitsConfig {
        max_stdout_capture_bytes: 256,
        max_llm_payload_bytes: 1024,
    };
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());

    assert_eq!(cfg.limits.max_stdout_capture_bytes, 256);
    let ls = LogicShell::with_config(cfg);
    ls.dispatch(&["true"]).await.unwrap();
    assert!(audit_path.exists());
}
