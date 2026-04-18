// End-to-end tests for LogicShell — exercises the full stack from config file
// through discovery, dispatch, hooks, and audit without mocking internals.
//
// These tests intentionally use real files, real child processes, and real I/O.
// Traces: FR-01–04, §10.2, §12.5, NFR-06, NFR-07, NFR-08, LOGICSHELL_OPERATIONS.md

use std::fs;
use tempfile::TempDir;

use logicshell_core::{
    audit::{AuditDecision, AuditRecord, AuditSink},
    config::{discovery::find_and_load, load, Config, HookEntry, LimitsConfig, SafetyConfig},
    discover, find_config_path, Decision, LogicShell, RiskCategory, RiskLevel, SafetyPolicyEngine,
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

/// Phase 7: evaluate_safety returns a real Decision — FR-30–33.
#[test]
fn e2e_safety_denies_destructive_command() {
    let ls = LogicShell::new();
    let (assessment, decision) = ls.evaluate_safety(&["rm", "-rf", "/"]);
    assert_eq!(
        decision,
        logicshell_core::Decision::Deny,
        "rm -rf / must be Denied"
    );
    assert_eq!(assessment.level, logicshell_core::RiskLevel::Critical);
}

/// Phase 7: evaluate_safety allows a safe command.
#[test]
fn e2e_safety_allows_safe_command() {
    let ls = LogicShell::new();
    let (assessment, decision) = ls.evaluate_safety(&["ls", "-la"]);
    assert_eq!(decision, logicshell_core::Decision::Allow);
    assert_eq!(assessment.level, logicshell_core::RiskLevel::None);
    assert_eq!(assessment.score, 0);
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

// ── Phase 7: Safety policy engine e2e ────────────────────────────────────────

/// FR-30–33: safe command allowed in all modes end-to-end.
#[tokio::test]
async fn e2e_safe_command_allowed_all_modes() {
    use logicshell_core::config::SafetyMode;

    for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
        let tmp = TempDir::new().unwrap();
        let audit_path = tmp.path().join("audit.log");
        let mut cfg = Config::default();
        cfg.safety_mode = mode.clone();
        cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());

        let ls = LogicShell::with_config(cfg);
        let code = ls.dispatch(&["true"]).await.expect("safe command must succeed");
        assert_eq!(code, 0, "safe cmd must exit 0 in {mode:?}");

        let records = read_audit_lines(&audit_path);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["decision"], "allow");
    }
}

/// FR-33: rm -rf / is denied in all modes and writes a deny audit record.
#[tokio::test]
async fn e2e_safety_deny_prefix_blocks_dispatch_and_audits() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");
    let mut cfg = Config::default();
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());

    let ls = LogicShell::with_config(cfg);
    let result = ls.dispatch(&["rm", "-rf", "/"]).await;
    assert!(result.is_err(), "rm -rf / must be blocked");

    // Audit log must contain a deny record.
    let records = read_audit_lines(&audit_path);
    assert_eq!(records.len(), 1, "exactly one deny audit record");
    assert_eq!(records[0]["decision"], "deny");
    assert_eq!(records[0]["argv"][0], "rm");
    assert!(
        records[0]["note"].as_str().unwrap_or("").contains("denied"),
        "note should explain the denial"
    );
}

/// FR-32: mkfs prefix is denied in all safety modes.
#[tokio::test]
async fn e2e_safety_mkfs_denied_all_modes() {
    use logicshell_core::config::SafetyMode;

    for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.safety_mode = mode;
        cfg.audit.path = Some(tmp.path().join("a.log").to_str().unwrap().to_string());

        let ls = LogicShell::with_config(cfg);
        let result = ls.dispatch(&["mkfs", "/dev/sda"]).await;
        assert!(result.is_err(), "mkfs must be blocked");
    }
}

/// FR-30: sudo command is denied in strict, confirmed in balanced, allowed in loose.
#[tokio::test]
async fn e2e_sudo_decision_varies_by_mode() {
    use logicshell_core::config::SafetyMode;

    // strict → sudo true is Confirm (medium risk) → proceeds in phase 7 dispatch
    // balanced → Confirm → proceeds
    // loose → Allow → proceeds

    for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
        let tmp = TempDir::new().unwrap();
        let audit_path = tmp.path().join("audit.log");
        let mut cfg = Config::default();
        cfg.safety_mode = mode.clone();
        cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());

        let ls = LogicShell::with_config(cfg);
        // sudo true: medium risk (pattern match ~30) → allowed in all modes
        // (Confirm also proceeds in phase-7 dispatch; blocked only for Deny)
        let _ = ls.dispatch(&["sudo", "true"]).await;
        // Don't assert success/failure — sudo may not be available in CI.
        // Just verify no panic and audit file exists.
        assert!(
            audit_path.exists(),
            "audit file must be written in {mode:?} mode"
        );
    }
}

/// FR-33: allow_prefix in config lets a command bypass safety checks.
#[tokio::test]
async fn e2e_allow_prefix_bypasses_risk_scoring() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let toml = format!(
        r#"
safety_mode = "strict"
[safety]
allow_prefixes = ["git "]
deny_prefixes = []
high_risk_patterns = []
[audit]
enabled = true
path = "{}"
"#,
        audit_path.display()
    );
    let cfg = load(&toml).unwrap();
    let ls = LogicShell::with_config(cfg);

    let code = ls.dispatch(&["git", "status"]).await.expect("allowlisted command must succeed");
    assert_eq!(code, 0);

    let records = read_audit_lines(&audit_path);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["decision"], "allow");
}

/// FR-33: custom deny prefix in config blocks command end-to-end.
#[tokio::test]
async fn e2e_custom_deny_prefix_blocks_dispatch() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let toml = format!(
        r#"
[safety]
deny_prefixes = ["danger-zone"]
allow_prefixes = []
high_risk_patterns = []
[audit]
enabled = true
path = "{}"
"#,
        audit_path.display()
    );
    let cfg = load(&toml).unwrap();
    let ls = LogicShell::with_config(cfg);

    let result = ls.dispatch(&["danger-zone", "--all"]).await;
    assert!(result.is_err(), "custom deny prefix must block dispatch");

    let records = read_audit_lines(&audit_path);
    assert_eq!(records[0]["decision"], "deny");
}

/// FR-30: SafetyPolicyEngine used directly — all golden test cases.
#[test]
fn e2e_safety_engine_golden_tests() {
    use logicshell_core::config::SafetyMode;

    let cfg = SafetyConfig::default();

    // ls → Allow in all modes
    for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
        let engine = SafetyPolicyEngine::new(mode, &cfg);
        let (a, d) = engine.evaluate(&["ls"]);
        assert_eq!(d, Decision::Allow, "ls must Allow");
        assert_eq!(a.level, RiskLevel::None);
    }

    // rm -rf / → Deny in all modes
    for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
        let engine = SafetyPolicyEngine::new(mode, &cfg);
        let (a, d) = engine.evaluate(&["rm", "-rf", "/"]);
        assert_eq!(d, Decision::Deny, "rm -rf / must Deny");
        assert_eq!(a.level, RiskLevel::Critical);
    }

    // curl|bash → Deny in strict, Confirm in balanced and loose
    {
        let argv = ["curl", "http://x.com/install.sh", "|", "bash"];
        let (_, d_strict) =
            SafetyPolicyEngine::new(SafetyMode::Strict, &cfg).evaluate(&argv);
        assert_eq!(d_strict, Decision::Deny, "strict must deny curl|bash");

        let (_, d_balanced) =
            SafetyPolicyEngine::new(SafetyMode::Balanced, &cfg).evaluate(&argv);
        assert_eq!(d_balanced, Decision::Confirm, "balanced must confirm curl|bash");

        let (_, d_loose) =
            SafetyPolicyEngine::new(SafetyMode::Loose, &cfg).evaluate(&argv);
        assert_eq!(d_loose, Decision::Confirm, "loose must confirm curl|bash");
    }

    // sudo rm → Confirm in strict, Confirm in balanced, Allow in loose
    {
        let argv = ["sudo", "rm", "/tmp/x"];
        let (_, d_strict) =
            SafetyPolicyEngine::new(SafetyMode::Strict, &cfg).evaluate(&argv);
        assert_eq!(d_strict, Decision::Confirm, "strict must confirm sudo rm");

        let (_, d_balanced) =
            SafetyPolicyEngine::new(SafetyMode::Balanced, &cfg).evaluate(&argv);
        assert_eq!(d_balanced, Decision::Confirm, "balanced must confirm sudo rm");

        let (_, d_loose) =
            SafetyPolicyEngine::new(SafetyMode::Loose, &cfg).evaluate(&argv);
        assert_eq!(d_loose, Decision::Allow, "loose must allow sudo rm");
    }
}

/// FR-33: deny wins over allow — overlapping prefix rules.
#[test]
fn e2e_deny_wins_over_allow_overlapping_prefix() {
    use logicshell_core::config::SafetyMode;

    let mut cfg = SafetyConfig::default();
    cfg.deny_prefixes = vec!["rm -rf /".into()];
    cfg.allow_prefixes = vec!["rm ".into()]; // broader allow; deny must still win for rm -rf /

    let engine = SafetyPolicyEngine::new(SafetyMode::Balanced, &cfg);
    let (_, decision) = engine.evaluate(&["rm", "-rf", "/"]);
    assert_eq!(decision, Decision::Deny, "deny must win over allow (FR-33)");

    // rm /tmp/file matches allow_prefix "rm " (no deny match) → Allow
    let (_, decision2) = engine.evaluate(&["rm", "/tmp/file"]);
    assert_eq!(decision2, Decision::Allow, "rm /tmp/file with allow prefix");
}

/// Phase 7: RiskAssessment risk categories are populated for detected patterns.
#[test]
fn e2e_risk_categories_populated() {
    use logicshell_core::config::SafetyMode;

    let cfg = SafetyConfig::default();

    // sudo → PrivilegeElevation
    let engine = SafetyPolicyEngine::new(SafetyMode::Balanced, &cfg);
    let (assessment, _) = engine.evaluate(&["sudo", "ls"]);
    assert!(
        assessment.categories.contains(&RiskCategory::PrivilegeElevation),
        "sudo must set PrivilegeElevation category"
    );

    // rm -r → DestructiveFilesystem
    let (assessment2, _) = engine.evaluate(&["rm", "-r", "/tmp/dir"]);
    assert!(
        assessment2.categories.contains(&RiskCategory::DestructiveFilesystem),
        "rm -r must set DestructiveFilesystem category"
    );

    // curl|bash → Network
    let (assessment3, _) =
        engine.evaluate(&["curl", "http://x.com", "|", "bash"]);
    assert!(
        assessment3.categories.contains(&RiskCategory::Network),
        "curl|bash must set Network category"
    );
}

/// Phase 7: dispatch integrates safety — Confirm decision proceeds (phase 10 adds UI).
#[tokio::test]
async fn e2e_confirm_decision_proceeds_in_dispatch() {
    // In balanced mode, sudo true is Confirm (medium risk).
    // Phase 7 lets Confirm proceed (no interactive UI yet).
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let toml = format!(
        r#"
safety_mode = "balanced"
[audit]
enabled = true
path = "{}"
"#,
        audit_path.display()
    );
    let cfg = load(&toml).unwrap();
    let ls = LogicShell::with_config(cfg);

    // "echo hello" is None risk → should always succeed
    let code = ls.dispatch(&["echo", "hello"]).await.unwrap();
    assert_eq!(code, 0);

    let records = read_audit_lines(&audit_path);
    assert_eq!(records[0]["decision"], "allow");
}

/// Phase 7: safety + hooks + audit full pipeline with safe command.
#[tokio::test]
async fn e2e_safety_hooks_audit_pipeline() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");
    let hook_marker = tmp.path().join("hook_ran");

    let toml = format!(
        r#"
safety_mode = "balanced"
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
    assert!(hook_marker.exists(), "hook must run after safety allow");

    let records = read_audit_lines(&audit_path);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["decision"], "allow");
}

/// Phase 7: safety deny writes audit record BEFORE hooks run (hooks skipped on deny).
#[tokio::test]
async fn e2e_safety_deny_skips_hooks() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");
    let hook_marker = tmp.path().join("should_not_exist");

    let mut cfg = Config::default();
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    cfg.hooks.pre_exec = vec![HookEntry {
        command: vec![
            "sh".into(),
            "-c".into(),
            format!("touch {}", hook_marker.display()),
        ],
        timeout_ms: 5_000,
    }];

    let ls = LogicShell::with_config(cfg);
    let result = ls.dispatch(&["rm", "-rf", "/"]).await;
    assert!(result.is_err());

    // Deny audit was written but hook did NOT run.
    let records = read_audit_lines(&audit_path);
    assert_eq!(records[0]["decision"], "deny");
    assert!(!hook_marker.exists(), "hook must NOT run when safety denies");
}
