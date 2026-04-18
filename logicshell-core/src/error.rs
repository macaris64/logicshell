use thiserror::Error;

/// All errors that can originate from the LogicShell core library.
///
/// Traces: NFR-06 (no unhandled panics; errors as `Result`)
#[derive(Debug, Error)]
pub enum LogicShellError {
    /// Configuration file was malformed, missing required keys, or contained unknown keys.
    #[error("configuration error: {0}")]
    Config(String),

    /// A child process could not be spawned or its exit could not be observed.
    #[error("dispatch error: {0}")]
    Dispatch(String),

    /// The safety policy engine rejected or could not evaluate a command.
    #[error("safety policy error: {0}")]
    Safety(String),

    /// The audit sink could not record an entry.
    #[error("audit error: {0}")]
    Audit(String),

    /// A pre-exec hook failed or timed out.
    #[error("hook error: {0}")]
    Hook(String),

    /// Underlying OS I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience `Result` alias for the LogicShell core library.
pub type Result<T> = std::result::Result<T, LogicShellError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// NFR-06: errors are structured values, not panics.
    #[test]
    fn config_error_display() {
        let e = LogicShellError::Config("missing schema_version".into());
        assert_eq!(e.to_string(), "configuration error: missing schema_version");
    }

    #[test]
    fn dispatch_error_display() {
        let e = LogicShellError::Dispatch("no such executable".into());
        assert_eq!(e.to_string(), "dispatch error: no such executable");
    }

    #[test]
    fn safety_error_display() {
        let e = LogicShellError::Safety("pattern denied".into());
        assert_eq!(e.to_string(), "safety policy error: pattern denied");
    }

    #[test]
    fn audit_error_display() {
        let e = LogicShellError::Audit("write failed".into());
        assert_eq!(e.to_string(), "audit error: write failed");
    }

    #[test]
    fn hook_error_display() {
        let e = LogicShellError::Hook("pre-exec hook timed out".into());
        assert_eq!(e.to_string(), "hook error: pre-exec hook timed out");
    }

    /// `std::io::Error` converts into `LogicShellError::Io` via `From`.
    #[test]
    fn io_error_from_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let ls_err: LogicShellError = io_err.into();
        assert!(matches!(ls_err, LogicShellError::Io(_)));
        assert!(ls_err.to_string().contains("I/O error"));
    }

    /// `Result<T>` alias resolves to `std::result::Result<T, LogicShellError>`.
    #[test]
    fn result_alias_ok() {
        let r: Result<u32> = Ok(42);
        assert_eq!(r.unwrap(), 42);
    }

    #[test]
    fn result_alias_err() {
        let r: Result<u32> = Err(LogicShellError::Config("bad".into()));
        assert!(r.is_err());
    }

    /// Error variants are `Debug`-printable (required for `unwrap`/`?` ergonomics).
    #[test]
    fn errors_are_debug() {
        let e = LogicShellError::Safety("denied".into());
        let s = format!("{e:?}");
        assert!(s.contains("Safety"));
    }
}
