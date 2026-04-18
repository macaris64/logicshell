// LLM-specific error type — LLM Module PRD §5.x

use thiserror::Error;

/// Errors that can originate from the LLM bridge subsystem.
#[derive(Debug, Error, PartialEq)]
pub enum LlmError {
    /// LLM is disabled in configuration (`llm.enabled = false`).
    #[error("LLM is disabled by configuration")]
    Disabled,

    /// `llm.enabled = true` but `llm.model` is absent — caught at composition time.
    #[error("llm.model is required when llm.enabled = true")]
    ModelNotSpecified,

    /// Composed prompt exceeds the configured `max_context_chars` limit.
    #[error("prompt length {size} chars exceeds max_context_chars limit {max}")]
    ContextTooLarge { size: usize, max: usize },

    /// LLM response could not be parsed into a usable form.
    #[error("LLM response parse error: {0}")]
    Parse(String),

    /// HTTP or transport error from the Ollama client (Phase 9+).
    #[error("LLM HTTP error: {0}")]
    Http(String),

    /// Catch-all for unforeseen LLM subsystem errors.
    #[error("LLM error: {0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_display() {
        let e = LlmError::Disabled;
        assert_eq!(e.to_string(), "LLM is disabled by configuration");
    }

    #[test]
    fn model_not_specified_display() {
        let e = LlmError::ModelNotSpecified;
        assert!(e.to_string().contains("llm.model is required"));
    }

    #[test]
    fn context_too_large_display() {
        let e = LlmError::ContextTooLarge {
            size: 9000,
            max: 8000,
        };
        let s = e.to_string();
        assert!(s.contains("9000"), "size in message: {s}");
        assert!(s.contains("8000"), "max in message: {s}");
    }

    #[test]
    fn parse_error_display() {
        let e = LlmError::Parse("unexpected EOF".into());
        assert!(e.to_string().contains("parse error"));
        assert!(e.to_string().contains("unexpected EOF"));
    }

    #[test]
    fn http_error_display() {
        let e = LlmError::Http("connection refused".into());
        assert!(e.to_string().contains("HTTP error"));
        assert!(e.to_string().contains("connection refused"));
    }

    #[test]
    fn other_error_display() {
        let e = LlmError::Other("something went wrong".into());
        assert!(e.to_string().contains("something went wrong"));
    }

    #[test]
    fn errors_are_debug() {
        let e = LlmError::Disabled;
        assert!(!format!("{e:?}").is_empty());
    }

    #[test]
    fn errors_are_partial_eq() {
        assert_eq!(LlmError::Disabled, LlmError::Disabled);
        assert_eq!(LlmError::ModelNotSpecified, LlmError::ModelNotSpecified);
        assert_eq!(
            LlmError::ContextTooLarge { size: 1, max: 0 },
            LlmError::ContextTooLarge { size: 1, max: 0 }
        );
        assert_ne!(LlmError::Disabled, LlmError::ModelNotSpecified);
    }

    #[test]
    fn parse_error_partial_eq() {
        assert_eq!(LlmError::Parse("x".into()), LlmError::Parse("x".into()));
        assert_ne!(LlmError::Parse("a".into()), LlmError::Parse("b".into()));
    }

    #[test]
    fn http_error_partial_eq() {
        assert_eq!(LlmError::Http("e".into()), LlmError::Http("e".into()));
    }

    #[test]
    fn other_error_partial_eq() {
        assert_eq!(LlmError::Other("x".into()), LlmError::Other("x".into()));
    }
}
