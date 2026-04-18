// LogicShell feature demonstration — runs each implemented phase end-to-end.
//
// Usage:
//   cargo run --example demo
//
// Exit code: 0 on success, non-zero on first feature failure.

use std::fs;
use std::path::PathBuf;

use logicshell_core::{
    audit::{AuditDecision, AuditRecord, AuditSink},
    config::{AuditConfig, Config, HookEntry, HooksConfig, LimitsConfig},
    dispatcher::{DispatchOptions, Dispatcher, StdinMode},
    hooks::HookRunner,
    LogicShell,
};
use tempfile::TempDir;

#[tokio::main]
async fn main() {
    let tmp = TempDir::new().expect("temp dir");
    let all_ok = true;

    // ── Phase 3–4: Config schema + discovery ─────────────────────────────────

    print!("[Phase 3–4: Config schema + discovery] ");
    let toml = r#"
schema_version = 1
safety_mode = "strict"
[audit]
enabled = true
[limits]
max_stdout_capture_bytes = 65536
"#;
    let cfg = logicshell_core::config::load(toml).expect("parse");
    assert_eq!(cfg.schema_version, 1);
    assert_eq!(cfg.limits.max_stdout_capture_bytes, 65_536);

    let disc_dir = TempDir::new().expect("tmp");
    fs::write(disc_dir.path().join(".logicshell.toml"), toml).expect("write cfg");
    let discovered = logicshell_core::discover(disc_dir.path()).expect("discover");
    assert_eq!(discovered.schema_version, 1);
    println!("config parsed + discovered OK");

    // ── Phase 5: Dispatcher ───────────────────────────────────────────────────

    print!("[Phase 5: Dispatcher] ");
    let d = Dispatcher::with_capture_limit(4096);

    let out = d
        .dispatch(DispatchOptions {
            argv: vec!["echo".into(), "hello logicshell".into()],
            ..Default::default()
        })
        .await
        .expect("echo");
    assert_eq!(out.exit_code, 0);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello logicshell"), "got: {stdout:?}");

    let out2 = d
        .dispatch(DispatchOptions {
            argv: vec!["sh".into(), "-c".into(), "exit 42".into()],
            ..Default::default()
        })
        .await
        .expect("exit 42");
    assert_eq!(out2.exit_code, 42);

    let out3 = d
        .dispatch(DispatchOptions {
            argv: vec!["cat".into()],
            stdin: StdinMode::Piped(b"stdin_data\n".to_vec()),
            ..Default::default()
        })
        .await
        .expect("cat stdin");
    assert_eq!(out3.stdout, b"stdin_data\n");

    let out4 = d
        .dispatch(DispatchOptions {
            argv: vec!["pwd".into()],
            cwd: Some(PathBuf::from("/tmp")),
            ..Default::default()
        })
        .await
        .expect("pwd");
    let pwd = String::from_utf8_lossy(&out4.stdout);
    assert!(pwd.trim().contains("tmp"), "pwd: {pwd:?}");

    let tiny = Dispatcher::with_capture_limit(5);
    let out5 = tiny
        .dispatch(DispatchOptions {
            argv: vec!["echo".into(), "0123456789".into()],
            ..Default::default()
        })
        .await
        .expect("truncate");
    assert!(out5.stdout_truncated);
    assert!(out5.stdout.len() <= 5);

    let out6 = d
        .dispatch(DispatchOptions {
            argv: vec!["sh".into(), "-c".into(), "echo $DEMO_VAR".into()],
            env_extra: vec![("DEMO_VAR".into(), "injected".into())],
            ..Default::default()
        })
        .await
        .expect("env");
    assert!(String::from_utf8_lossy(&out6.stdout).contains("injected"));

    println!("echo, nonzero exit, stdin, cwd, truncation, env OK");

    // ── Phase 6: Pre-exec hooks ───────────────────────────────────────────────

    print!("[Phase 6: HookRunner] ");
    let marker = tmp.path().join("hook_marker");

    let ok_hooks = HooksConfig {
        pre_exec: vec![HookEntry {
            command: vec![
                "sh".into(),
                "-c".into(),
                format!("touch {}", marker.display()),
            ],
            timeout_ms: 5_000,
        }],
    };
    HookRunner::new(&ok_hooks)
        .run_pre_exec()
        .await
        .expect("hook");
    assert!(marker.exists(), "marker missing");

    let bad_hooks = HooksConfig {
        pre_exec: vec![HookEntry {
            command: vec!["false".into()],
            timeout_ms: 5_000,
        }],
    };
    assert!(HookRunner::new(&bad_hooks).run_pre_exec().await.is_err());

    let slow_hooks = HooksConfig {
        pre_exec: vec![HookEntry {
            command: vec!["sleep".into(), "60".into()],
            timeout_ms: 80,
        }],
    };
    let err = HookRunner::new(&slow_hooks)
        .run_pre_exec()
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("timed out"), "err: {err}");

    println!("success, nonzero exit, timeout OK");

    // ── Phase 6: Audit sink ───────────────────────────────────────────────────

    print!("[Phase 6: AuditSink] ");
    let log_path = tmp.path().join("demo_audit.log");

    let mut sink = AuditSink::open(&log_path).expect("open");
    for decision in [
        AuditDecision::Allow,
        AuditDecision::Deny,
        AuditDecision::Confirm,
    ] {
        sink.write(&AuditRecord::new("/demo", vec!["cmd".into()], decision))
            .expect("write");
    }
    drop(sink);

    let content = fs::read_to_string(&log_path).expect("read audit");
    let lines: Vec<_> = content.lines().collect();
    assert_eq!(lines.len(), 3);
    for line in &lines {
        serde_json::from_str::<serde_json::Value>(line).expect("JSON");
    }

    let disabled_cfg = AuditConfig {
        enabled: false,
        path: None,
    };
    let mut disabled = AuditSink::from_config(&disabled_cfg).expect("disabled");
    assert!(!disabled.is_enabled());
    disabled
        .write(&AuditRecord::new("/", vec![], AuditDecision::Deny))
        .expect("noop");

    println!("3 NDJSON records, flush-on-drop, disabled no-op OK");

    // ── Phase 6: LogicShell façade — full pipeline ────────────────────────────

    print!("[Phase 6: LogicShell façade] ");
    let audit_path = tmp.path().join("facade_audit.log");
    let hook_marker = tmp.path().join("facade_hook");

    let mut facade_cfg = Config::default();
    facade_cfg.limits = LimitsConfig {
        max_stdout_capture_bytes: 4096,
        ..LimitsConfig::default()
    };
    facade_cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    facade_cfg.hooks.pre_exec = vec![HookEntry {
        command: vec![
            "sh".into(),
            "-c".into(),
            format!("touch {}", hook_marker.display()),
        ],
        timeout_ms: 5_000,
    }];

    let ls = LogicShell::with_config(facade_cfg);
    let code = ls.dispatch(&["true"]).await.expect("dispatch true");
    assert_eq!(code, 0);
    assert!(hook_marker.exists(), "hook marker missing");

    let content = fs::read_to_string(&audit_path).expect("read facade audit");
    let v: serde_json::Value = serde_json::from_str(content.trim()).expect("JSON");
    assert_eq!(v["decision"], "allow");
    assert_eq!(v["argv"][0], "true");

    let manual_record = AuditRecord::new("/manual", vec!["custom".into()], AuditDecision::Confirm)
        .with_note("user said yes");
    ls.audit(&manual_record).expect("audit()");

    let content2 = fs::read_to_string(&audit_path).expect("audit2");
    assert_eq!(content2.lines().count(), 2, "2 records in audit log");

    let mut bad_facade_cfg = Config::default();
    bad_facade_cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
    bad_facade_cfg.hooks.pre_exec = vec![HookEntry {
        command: vec!["false".into()],
        timeout_ms: 5_000,
    }];
    let ls2 = LogicShell::with_config(bad_facade_cfg);
    assert!(
        ls2.dispatch(&["true"]).await.is_err(),
        "failing hook should abort"
    );

    println!("hook ran, audit written, façade.audit() appended, failing hook aborted OK");

    // ── Summary ───────────────────────────────────────────────────────────────

    if all_ok {
        println!("\n✓ All features verified OK");
    } else {
        eprintln!("\n✗ One or more features failed");
        std::process::exit(1);
    }
}
