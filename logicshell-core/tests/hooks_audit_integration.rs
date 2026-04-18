// Integration tests for Phase 6 — Pre-exec hooks & audit log
// Tests the full pipeline: hooks → dispatch → audit record written.
// Traces: §12.5, §10.2, NFR-07

use logicshell_core::{
    audit::{AuditDecision, AuditRecord, AuditSink},
    config::{AuditConfig, HookEntry, HooksConfig},
    error::LogicShellError,
};
use tempfile::TempDir;

// ── AuditSink integration ────────────────────────────────────────────────────

/// §10.2: audit line is parseable as NDJSON.
#[test]
fn audit_line_is_parseable_ndjson() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("audit.log");
    let mut sink = AuditSink::open(&path).unwrap();

    let record = AuditRecord {
        timestamp_secs: 1_700_000_042,
        cwd: "/home/ops".to_string(),
        argv: vec!["git".to_string(), "push".to_string()],
        decision: AuditDecision::Allow,
        note: None,
    };
    sink.write(&record).unwrap();
    drop(sink); // flush on drop

    let line = std::fs::read_to_string(&path).unwrap();
    let line = line.trim();
    assert!(!line.is_empty(), "audit line should not be empty");
    assert!(line.starts_with('{'), "expected JSON object, got: {line:?}");
    assert!(line.ends_with('}'), "expected JSON object, got: {line:?}");

    let v: serde_json::Value = serde_json::from_str(line).expect("audit line must be valid JSON");
    assert_eq!(v["timestamp_secs"], 1_700_000_042u64);
    assert_eq!(v["cwd"], "/home/ops");
    assert_eq!(v["argv"][0], "git");
    assert_eq!(v["argv"][1], "push");
    assert_eq!(v["decision"], "allow");
}

/// NFR-07: multiple records are each on their own line and all parseable.
#[test]
fn multiple_audit_records_each_on_own_line() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("audit.log");
    let mut sink = AuditSink::open(&path).unwrap();

    let decisions = [
        AuditDecision::Allow,
        AuditDecision::Deny,
        AuditDecision::Confirm,
    ];
    for d in &decisions {
        let rec = AuditRecord {
            timestamp_secs: 0,
            cwd: "/".to_string(),
            argv: vec!["cmd".to_string()],
            decision: d.clone(),
            note: None,
        };
        sink.write(&rec).unwrap();
    }
    drop(sink);

    let content = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 3, "expected 3 lines, one per record");
    for line in &lines {
        serde_json::from_str::<serde_json::Value>(line).expect("every line should be valid JSON");
    }
}

/// NFR-07: flush on drop — bytes reach disk without explicit flush().
#[test]
fn audit_flush_on_drop_integration() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("audit.log");

    {
        let mut sink = AuditSink::open(&path).unwrap();
        let rec = AuditRecord::new("/tmp", vec!["ls".into()], AuditDecision::Allow);
        sink.write(&rec).unwrap();
        // No explicit flush — rely on Drop.
    }

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(
        !content.is_empty(),
        "Drop should have flushed the record to disk"
    );
}

/// §10.2: audit records survive process crash simulation (close + reopen).
#[test]
fn audit_records_survive_reopen() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("audit.log");

    // First "process" writes two records.
    {
        let mut sink = AuditSink::open(&path).unwrap();
        for d in [AuditDecision::Allow, AuditDecision::Deny] {
            sink.write(&AuditRecord::new("/a", vec!["x".into()], d))
                .unwrap();
        }
    }

    // Second "process" appends one more.
    {
        let mut sink = AuditSink::open(&path).unwrap();
        sink.write(&AuditRecord::new(
            "/b",
            vec!["y".into()],
            AuditDecision::Confirm,
        ))
        .unwrap();
    }

    let content = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        content.lines().count(),
        3,
        "all 3 records should persist across reopen"
    );
}

/// from_config with path → writes to the specified path.
#[test]
fn from_config_with_path_writes_to_configured_location() {
    let tmp = TempDir::new().unwrap();
    let explicit_path = tmp.path().join("explicit-audit.log");
    let cfg = AuditConfig {
        enabled: true,
        path: Some(explicit_path.to_str().unwrap().to_string()),
    };

    let mut sink = AuditSink::from_config(&cfg).unwrap();
    sink.write(&AuditRecord::new(
        "/",
        vec!["true".into()],
        AuditDecision::Allow,
    ))
    .unwrap();
    drop(sink);

    assert!(
        explicit_path.exists(),
        "audit file should be created at the configured path"
    );
    let content = std::fs::read_to_string(&explicit_path).unwrap();
    assert!(!content.is_empty());
}

/// from_config disabled → writes nothing, returns no error.
#[test]
fn from_config_disabled_writes_nothing() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("should-not-exist.log");
    let cfg = AuditConfig {
        enabled: false,
        path: Some(path.to_str().unwrap().to_string()),
    };

    let mut sink = AuditSink::from_config(&cfg).unwrap();
    sink.write(&AuditRecord::new(
        "/",
        vec!["ls".into()],
        AuditDecision::Deny,
    ))
    .unwrap();
    drop(sink);

    // File should not have been created since auditing is disabled.
    assert!(
        !path.exists(),
        "disabled audit sink should not create the log file"
    );
}

// ── HookRunner integration ────────────────────────────────────────────────────

use logicshell_core::hooks::HookRunner;

/// §12.5: hook timeout behavior — slow hook returns Hook error.
#[cfg(unix)]
#[tokio::test]
async fn hook_timeout_integration() {
    let cfg = HooksConfig {
        pre_exec: vec![HookEntry {
            command: vec!["sleep".to_string(), "60".to_string()],
            timeout_ms: 80, // intentionally short
        }],
    };

    let result = HookRunner::new(&cfg).run_pre_exec().await;
    assert!(
        matches!(result, Err(LogicShellError::Hook(_))),
        "expected Hook error from timeout, got: {result:?}"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("timed out"),
        "error should mention 'timed out': {msg}"
    );
}

/// §12.5: successful hook chain → all hooks execute in order.
#[cfg(unix)]
#[tokio::test]
async fn successful_hook_chain_integration() {
    let tmp = TempDir::new().unwrap();
    let first = tmp.path().join("first");
    let second = tmp.path().join("second");

    let cfg = HooksConfig {
        pre_exec: vec![
            HookEntry {
                command: vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    format!("touch {}", first.display()),
                ],
                timeout_ms: 5_000,
            },
            HookEntry {
                command: vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    format!("touch {}", second.display()),
                ],
                timeout_ms: 5_000,
            },
        ],
    };

    HookRunner::new(&cfg).run_pre_exec().await.unwrap();

    assert!(first.exists(), "first hook should have created its marker");
    assert!(
        second.exists(),
        "second hook should have created its marker"
    );
}

/// §12.5: failing hook stops execution before subsequent hooks run.
#[cfg(unix)]
#[tokio::test]
async fn failing_hook_stops_chain_integration() {
    let tmp = TempDir::new().unwrap();
    let should_not_exist = tmp.path().join("should_not_exist");

    let cfg = HooksConfig {
        pre_exec: vec![
            HookEntry {
                command: vec!["false".to_string()],
                timeout_ms: 5_000,
            },
            HookEntry {
                command: vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    format!("touch {}", should_not_exist.display()),
                ],
                timeout_ms: 5_000,
            },
        ],
    };

    let result = HookRunner::new(&cfg).run_pre_exec().await;
    assert!(matches!(result, Err(LogicShellError::Hook(_))));
    assert!(
        !should_not_exist.exists(),
        "second hook must not run after first hook fails"
    );
}

// ── End-to-end pipeline: hooks → dispatch → audit ────────────────────────────

use logicshell_core::LogicShell;

/// Full Phase 6 pipeline: successful pre-exec hook, dispatch, audit record written.
#[cfg(unix)]
#[tokio::test]
async fn full_pipeline_hooks_dispatch_audit() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");
    let hook_marker = tmp.path().join("hook_ran");

    let mut cfg = logicshell_core::config::Config::default();
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    cfg.hooks.pre_exec = vec![HookEntry {
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            format!("touch {}", hook_marker.display()),
        ],
        timeout_ms: 5_000,
    }];

    let ls = LogicShell::with_config(cfg);
    let exit_code = ls.dispatch(&["true"]).await.unwrap();

    assert_eq!(exit_code, 0, "dispatch should return 0 for 'true'");
    assert!(hook_marker.exists(), "pre-exec hook should have run");

    // Verify audit record is written and parseable.
    let content = std::fs::read_to_string(&audit_path).unwrap();
    assert!(!content.is_empty(), "audit log should not be empty");
    let v: serde_json::Value =
        serde_json::from_str(content.trim()).expect("audit line must be valid JSON");
    assert_eq!(v["decision"], "allow");
    assert_eq!(v["argv"][0], "true");
}

/// Failing hook in the pipeline returns Hook error; no audit record is written.
#[cfg(unix)]
#[tokio::test]
async fn failing_hook_aborts_pipeline_no_audit() {
    let tmp = TempDir::new().unwrap();
    let audit_path = tmp.path().join("audit.log");

    let mut cfg = logicshell_core::config::Config::default();
    cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    cfg.hooks.pre_exec = vec![HookEntry {
        command: vec!["false".to_string()],
        timeout_ms: 5_000,
    }];

    let ls = LogicShell::with_config(cfg);
    let result = ls.dispatch(&["true"]).await;

    assert!(matches!(result, Err(LogicShellError::Hook(_))));
    // Audit file should not exist (hooks failed before dispatch & audit).
    assert!(
        !audit_path.exists(),
        "audit file should not be created when hooks fail"
    );
}
