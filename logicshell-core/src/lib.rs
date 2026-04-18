// logicshell-core: dispatcher, config, safety, audit, hooks — no HTTP

pub mod config;
pub mod error;
pub use config::discovery::{discover, find_config_path};
pub use error::{LogicShellError, Result};

/// Top-level façade that coordinates configuration, safety, dispatch, and audit.
///
/// Hosts link this crate and call methods here; the TUI and CLI are thin layers
/// over the same boundaries (Framework PRD §3.1, §11.1).
pub struct LogicShell {
    _private: (),
}

impl LogicShell {
    /// Create a new `LogicShell` instance.
    ///
    /// Later phases will accept a validated `Config`; for now the constructor
    /// is a zero-argument stub so the façade can be imported and tested.
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Stub: spawn and await a child process by argv.
    ///
    /// Full implementation: Phase 5 (Process dispatcher — FR-01–04).
    pub async fn dispatch(&self, _argv: &[&str]) -> Result<i32> {
        Err(LogicShellError::Dispatch(
            "not yet implemented (phase 5)".into(),
        ))
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

    /// Stub dispatch returns a `Dispatch` error, not a panic — NFR-06
    #[tokio::test]
    async fn dispatch_stub_returns_error() {
        let ls = LogicShell::new();
        let result = ls.dispatch(&["ls"]).await;
        assert!(matches!(result, Err(LogicShellError::Dispatch(_))));
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
