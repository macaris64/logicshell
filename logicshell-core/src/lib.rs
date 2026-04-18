// logicshell-core: dispatcher, config, safety, audit, hooks — no HTTP

pub mod config;
pub mod dispatcher;
pub mod error;
pub use config::discovery::{discover, find_config_path};
pub use error::{LogicShellError, Result};

use config::Config;
use dispatcher::{DispatchOptions, Dispatcher};

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
    /// Uses `limits.max_stdout_capture_bytes` from the active config (NFR-08).
    /// A nonzero exit code is returned as `Ok(n)`, not an error.
    pub async fn dispatch(&self, argv: &[&str]) -> Result<i32> {
        let d = Dispatcher::new(&self.config.limits);
        let opts = DispatchOptions {
            argv: argv.iter().map(|s| s.to_string()).collect(),
            ..DispatchOptions::default()
        };
        let output = d.dispatch(opts).await?;
        Ok(output.exit_code)
    }

    /// Stub: evaluate a command through the safety policy engine.
    ///
    /// Full implementation: Phase 7 (Safety policy engine — FR-30–33).
    pub fn evaluate_safety(&self, _argv: &[&str]) -> Result<()> {
        Err(LogicShellError::Safety(
            "not yet implemented (phase 7)".into(),
        ))
    }

    /// Stub: append a record to the audit log.
    ///
    /// Full implementation: Phase 6 (Audit log — §10.2, NFR-07).
    pub fn audit(&self, _record: &str) -> Result<()> {
        Err(LogicShellError::Audit(
            "not yet implemented (phase 6)".into(),
        ))
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
        let ls = LogicShell::new();
        // `true` always exits 0 on Unix
        let result = ls.dispatch(&["true"]).await;
        assert!(result.is_ok(), "dispatch returned Err: {result:?}");
        assert_eq!(result.unwrap(), 0);
    }

    /// Phase 5: dispatch propagates nonzero exit — FR-03
    #[tokio::test]
    async fn dispatch_propagates_nonzero_exit() {
        let ls = LogicShell::new();
        // `false` always exits 1 on Unix
        let result = ls.dispatch(&["false"]).await;
        assert!(result.is_ok(), "expected Ok(1), got Err: {result:?}");
        assert_eq!(result.unwrap(), 1);
    }

    /// Stub safety returns a `Safety` error, not a panic — NFR-06
    #[test]
    fn safety_stub_returns_error() {
        let ls = LogicShell::new();
        let result = ls.evaluate_safety(&["ls"]);
        assert!(matches!(result, Err(LogicShellError::Safety(_))));
    }

    /// Stub audit returns an `Audit` error, not a panic — NFR-06
    #[test]
    fn audit_stub_returns_error() {
        let ls = LogicShell::new();
        let result = ls.audit("test record");
        assert!(matches!(result, Err(LogicShellError::Audit(_))));
    }
}
