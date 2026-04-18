// Pre-execution hook runner — §12.5, NFR-07
//
// Runs each HookEntry in HooksConfig.pre_exec sequentially.
// If any hook exits non-zero or exceeds its timeout_ms, an error is returned
// and the remaining hooks are skipped (fail-fast semantics).

use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use crate::{config::HooksConfig, LogicShellError, Result};

/// Runs pre-exec hooks from a [`HooksConfig`] (§12.5).
///
/// Hooks execute sequentially; the first failure stops the chain.
pub struct HookRunner<'a> {
    config: &'a HooksConfig,
}

impl<'a> HookRunner<'a> {
    pub fn new(config: &'a HooksConfig) -> Self {
        Self { config }
    }

    /// Run all `pre_exec` hooks in order.
    ///
    /// Returns the first [`LogicShellError::Hook`] encountered, or `Ok(())` when
    /// all hooks succeed.  An empty `pre_exec` list is a no-op.
    pub async fn run_pre_exec(&self) -> Result<()> {
        for hook in &self.config.pre_exec {
            if hook.command.is_empty() {
                continue;
            }
            let dur = Duration::from_millis(hook.timeout_ms);
            match timeout(dur, spawn_hook(&hook.command)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e),
                Err(_elapsed) => {
                    return Err(LogicShellError::Hook(format!(
                        "hook '{}' timed out after {}ms",
                        hook.command[0], hook.timeout_ms
                    )));
                }
            }
        }
        Ok(())
    }
}

async fn spawn_hook(command: &[String]) -> Result<()> {
    let mut cmd = Command::new(&command[0]);
    if command.len() > 1 {
        cmd.args(&command[1..]);
    }
    let status = cmd
        .spawn()
        .map_err(|e| LogicShellError::Hook(format!("hook spawn failed: {e}")))?
        .wait()
        .await
        .map_err(|e| LogicShellError::Hook(format!("hook wait failed: {e}")))?;

    if status.success() {
        Ok(())
    } else {
        Err(LogicShellError::Hook(format!(
            "hook '{}' exited with non-zero status: {:?}",
            command[0],
            status.code()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HookEntry, HooksConfig};

    fn make_config(entries: Vec<HookEntry>) -> HooksConfig {
        HooksConfig { pre_exec: entries }
    }

    fn entry(command: &[&str], timeout_ms: u64) -> HookEntry {
        HookEntry {
            command: command.iter().map(|s| s.to_string()).collect(),
            timeout_ms,
        }
    }

    // ── no hooks ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn empty_hooks_returns_ok() {
        let cfg = make_config(vec![]);
        assert!(HookRunner::new(&cfg).run_pre_exec().await.is_ok());
    }

    // ── successful hooks ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn single_successful_hook_returns_ok() {
        let cfg = make_config(vec![entry(&["true"], 5_000)]);
        assert!(HookRunner::new(&cfg).run_pre_exec().await.is_ok());
    }

    #[tokio::test]
    async fn multiple_successful_hooks_all_run() {
        let cfg = make_config(vec![
            entry(&["true"], 5_000),
            entry(&["true"], 5_000),
            entry(&["true"], 5_000),
        ]);
        assert!(HookRunner::new(&cfg).run_pre_exec().await.is_ok());
    }

    // ── nonzero exit ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn failing_hook_returns_hook_error() {
        let cfg = make_config(vec![entry(&["false"], 5_000)]);
        let result = HookRunner::new(&cfg).run_pre_exec().await;
        assert!(matches!(result, Err(LogicShellError::Hook(_))));
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("non-zero"), "expected 'non-zero' in: {msg}");
    }

    #[tokio::test]
    async fn first_failing_hook_stops_chain() {
        // Second hook would also fail; we only get one error.
        let cfg = make_config(vec![entry(&["false"], 5_000), entry(&["false"], 5_000)]);
        let result = HookRunner::new(&cfg).run_pre_exec().await;
        assert!(matches!(result, Err(LogicShellError::Hook(_))));
    }

    #[tokio::test]
    async fn failing_hook_before_success_stops_chain() {
        let cfg = make_config(vec![entry(&["false"], 5_000), entry(&["true"], 5_000)]);
        let result = HookRunner::new(&cfg).run_pre_exec().await;
        assert!(matches!(result, Err(LogicShellError::Hook(_))));
    }

    // ── timeout ───────────────────────────────────────────────────────────────

    #[cfg(unix)]
    #[tokio::test]
    async fn hook_exceeding_timeout_returns_hook_error() {
        // 10 s sleep, 50 ms timeout → guaranteed timeout.
        let cfg = make_config(vec![entry(&["sleep", "10"], 50)]);
        let result = HookRunner::new(&cfg).run_pre_exec().await;
        assert!(matches!(result, Err(LogicShellError::Hook(_))));
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("timed out"), "expected 'timed out' in: {msg}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn hook_error_message_includes_timeout_ms() {
        let cfg = make_config(vec![entry(&["sleep", "10"], 50)]);
        let err = HookRunner::new(&cfg)
            .run_pre_exec()
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("50"), "expected timeout ms '50' in: {err}");
    }

    // ── empty command entry ───────────────────────────────────────────────────

    #[tokio::test]
    async fn empty_command_entry_is_skipped() {
        let cfg = make_config(vec![HookEntry {
            command: vec![],
            timeout_ms: 100,
        }]);
        assert!(HookRunner::new(&cfg).run_pre_exec().await.is_ok());
    }

    // ── spawn failure ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn nonexistent_executable_returns_hook_error() {
        let cfg = make_config(vec![entry(&["__no_such_binary_xyz__"], 5_000)]);
        let result = HookRunner::new(&cfg).run_pre_exec().await;
        assert!(matches!(result, Err(LogicShellError::Hook(_))));
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("spawn failed"),
            "expected 'spawn failed' in: {msg}"
        );
    }

    // ── hook with args ────────────────────────────────────────────────────────

    #[cfg(unix)]
    #[tokio::test]
    async fn hook_with_args_runs_correctly() {
        // sh -c 'exit 0' → success
        let cfg = make_config(vec![entry(&["sh", "-c", "exit 0"], 5_000)]);
        assert!(HookRunner::new(&cfg).run_pre_exec().await.is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn hook_with_failing_args_returns_error() {
        // sh -c 'exit 2' → nonzero
        let cfg = make_config(vec![entry(&["sh", "-c", "exit 2"], 5_000)]);
        let result = HookRunner::new(&cfg).run_pre_exec().await;
        assert!(matches!(result, Err(LogicShellError::Hook(_))));
    }

    // ── hook name in error message ────────────────────────────────────────────

    #[tokio::test]
    async fn error_message_contains_hook_name() {
        let cfg = make_config(vec![entry(&["false"], 5_000)]);
        let err = HookRunner::new(&cfg)
            .run_pre_exec()
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("false"),
            "expected hook name 'false' in: {err}"
        );
    }
}
