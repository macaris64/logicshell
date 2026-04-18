// Configuration loading for `.logicshell.toml` — Framework PRD §12

pub mod schema;
pub use schema::*;

use crate::{LogicShellError, Result};

/// Parse a `.logicshell.toml` string into a [`Config`].
///
/// Unknown keys are rejected (§12.7 default: error). Malformed TOML fails fast
/// and includes line/column information from the TOML parser.
pub fn from_str(toml_str: &str) -> Result<Config> {
    toml::from_str(toml_str).map_err(|e| LogicShellError::Config(e.to_string()))
}

/// Validate semantic constraints that cannot be expressed in Serde alone.
///
/// Returns `Err` if `llm.enabled = true` but `llm.model` is absent (§12.2).
pub fn validate(config: &Config) -> Result<()> {
    if config.llm.enabled && config.llm.model.is_none() {
        return Err(LogicShellError::Config(
            "llm.model is required when llm.enabled = true".into(),
        ));
    }
    Ok(())
}

/// Parse and validate a TOML string in one step.
pub fn load(toml_str: &str) -> Result<Config> {
    let config = from_str(toml_str)?;
    validate(&config)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── from_str: empty / minimal ─────────────────────────────────────────────

    /// Empty TOML → all defaults applied — §12.1
    #[test]
    fn empty_toml_gives_defaults() {
        let cfg = from_str("").unwrap();
        assert_eq!(cfg, Config::default());
    }

    /// Only schema_version overridden; everything else defaults.
    #[test]
    fn parse_schema_version_only() {
        let cfg = from_str("schema_version = 2").unwrap();
        assert_eq!(cfg.schema_version, 2);
        assert_eq!(cfg.safety_mode, schema::SafetyMode::Balanced);
    }

    // ── safety_mode enum variants ─────────────────────────────────────────────

    #[test]
    fn parse_safety_mode_strict() {
        let cfg = from_str(r#"safety_mode = "strict""#).unwrap();
        assert_eq!(cfg.safety_mode, schema::SafetyMode::Strict);
    }

    #[test]
    fn parse_safety_mode_balanced() {
        let cfg = from_str(r#"safety_mode = "balanced""#).unwrap();
        assert_eq!(cfg.safety_mode, schema::SafetyMode::Balanced);
    }

    #[test]
    fn parse_safety_mode_loose() {
        let cfg = from_str(r#"safety_mode = "loose""#).unwrap();
        assert_eq!(cfg.safety_mode, schema::SafetyMode::Loose);
    }

    #[test]
    fn invalid_safety_mode_is_config_error() {
        assert!(from_str(r#"safety_mode = "ultra""#).is_err());
    }

    // ── [llm] table ───────────────────────────────────────────────────────────

    #[test]
    fn parse_llm_enabled_with_model() {
        let toml = r#"
[llm]
enabled = true
model = "llama3"
"#;
        let cfg = from_str(toml).unwrap();
        assert!(cfg.llm.enabled);
        assert_eq!(cfg.llm.model.as_deref(), Some("llama3"));
    }

    #[test]
    fn parse_llm_base_url() {
        let toml = r#"
[llm]
base_url = "http://10.0.0.5:11434"
"#;
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.llm.base_url, "http://10.0.0.5:11434");
    }

    #[test]
    fn parse_llm_timeout_secs() {
        let toml = "[llm]\ntimeout_secs = 120\n";
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.llm.timeout_secs, 120);
    }

    #[test]
    fn parse_llm_allow_remote() {
        let toml = "[llm]\nallow_remote = true\n";
        let cfg = from_str(toml).unwrap();
        assert!(cfg.llm.allow_remote);
    }

    #[test]
    fn parse_llm_provider_ollama() {
        let toml = r#"[llm]
provider = "ollama"
"#;
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.llm.provider, schema::LlmProvider::Ollama);
    }

    // ── [llm.invocation] ──────────────────────────────────────────────────────

    #[test]
    fn parse_llm_invocation_nl_session() {
        let toml = "[llm.invocation]\nnl_session = true\n";
        let cfg = from_str(toml).unwrap();
        assert!(cfg.llm.invocation.nl_session);
    }

    #[test]
    fn parse_llm_invocation_assist_on_not_found() {
        let toml = "[llm.invocation]\nassist_on_not_found = true\n";
        let cfg = from_str(toml).unwrap();
        assert!(cfg.llm.invocation.assist_on_not_found);
    }

    #[test]
    fn parse_llm_invocation_max_context_chars() {
        let toml = "[llm.invocation]\nmax_context_chars = 4000\n";
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.llm.invocation.max_context_chars, 4_000);
    }

    // ── [safety] table ────────────────────────────────────────────────────────

    #[test]
    fn parse_safety_deny_prefixes() {
        let toml = r#"[safety]
deny_prefixes = ["sudo rm", "halt"]
"#;
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.safety.deny_prefixes, vec!["sudo rm", "halt"]);
    }

    #[test]
    fn parse_safety_allow_prefixes() {
        let toml = r#"[safety]
allow_prefixes = ["ls", "cat"]
"#;
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.safety.allow_prefixes, vec!["ls", "cat"]);
    }

    #[test]
    fn parse_safety_high_risk_patterns() {
        let toml = r#"[safety]
high_risk_patterns = ["my-pattern"]
"#;
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.safety.high_risk_patterns, vec!["my-pattern"]);
    }

    // ── [audit] table ─────────────────────────────────────────────────────────

    #[test]
    fn parse_audit_disabled() {
        let toml = "[audit]\nenabled = false\n";
        let cfg = from_str(toml).unwrap();
        assert!(!cfg.audit.enabled);
    }

    #[test]
    fn parse_audit_path() {
        let toml = r#"[audit]
path = "/var/log/logicshell.log"
"#;
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.audit.path.as_deref(), Some("/var/log/logicshell.log"));
    }

    // ── [hooks] table ─────────────────────────────────────────────────────────

    #[test]
    fn parse_hooks_pre_exec() {
        let toml = r#"[[hooks.pre_exec]]
command = ["notify-send", "dispatching"]
timeout_ms = 2000
"#;
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.hooks.pre_exec.len(), 1);
        assert_eq!(cfg.hooks.pre_exec[0].command[0], "notify-send");
        assert_eq!(cfg.hooks.pre_exec[0].timeout_ms, 2_000);
    }

    #[test]
    fn parse_hook_entry_default_timeout() {
        let toml = r#"[[hooks.pre_exec]]
command = ["true"]
"#;
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.hooks.pre_exec[0].timeout_ms, 5_000);
    }

    // ── [limits] table ────────────────────────────────────────────────────────

    #[test]
    fn parse_limits_stdout_cap() {
        let toml = "[limits]\nmax_stdout_capture_bytes = 65536\n";
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.limits.max_stdout_capture_bytes, 65_536);
    }

    #[test]
    fn parse_limits_llm_payload() {
        let toml = "[limits]\nmax_llm_payload_bytes = 128000\n";
        let cfg = from_str(toml).unwrap();
        assert_eq!(cfg.limits.max_llm_payload_bytes, 128_000);
    }

    // ── §12.7 Validation errors ───────────────────────────────────────────────

    /// Unknown top-level key → Config error (§12.7 default: error).
    #[test]
    fn unknown_top_level_key_is_error() {
        let result = from_str("unknown_key = true");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("configuration error"));
    }

    /// Unknown key inside [llm] table → Config error.
    #[test]
    fn unknown_llm_key_is_error() {
        let toml = "[llm]\nfoo = 1\n";
        assert!(from_str(toml).is_err());
    }

    /// Unknown key inside [safety] → Config error.
    #[test]
    fn unknown_safety_key_is_error() {
        let toml = "[safety]\nnot_a_key = true\n";
        assert!(from_str(toml).is_err());
    }

    /// Unknown key inside [audit] → Config error.
    #[test]
    fn unknown_audit_key_is_error() {
        let toml = "[audit]\nsecret = 42\n";
        assert!(from_str(toml).is_err());
    }

    /// Unknown key inside [[hooks.pre_exec]] → Config error.
    #[test]
    fn unknown_hook_key_is_error() {
        let toml = "[[hooks.pre_exec]]\ncommand = [\"ls\"]\nextra = true\n";
        assert!(from_str(toml).is_err());
    }

    /// Unknown key inside [limits] → Config error.
    #[test]
    fn unknown_limits_key_is_error() {
        let toml = "[limits]\nmax_everything = 999\n";
        assert!(from_str(toml).is_err());
    }

    /// Malformed TOML fails with a Config error that references the issue (§12.7).
    #[test]
    fn malformed_toml_is_config_error() {
        let result = from_str("schema_version = [[[");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("configuration error"));
    }

    // ── validate() ────────────────────────────────────────────────────────────

    /// llm.enabled = true without model → validation error.
    #[test]
    fn validate_llm_enabled_without_model_errors() {
        let mut cfg = Config::default();
        cfg.llm.enabled = true;
        cfg.llm.model = None;
        let result = validate(&cfg);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("llm.model is required"));
    }

    /// llm.enabled = true with model → validation passes.
    #[test]
    fn validate_llm_enabled_with_model_ok() {
        let mut cfg = Config::default();
        cfg.llm.enabled = true;
        cfg.llm.model = Some("llama3".into());
        assert!(validate(&cfg).is_ok());
    }

    /// llm.enabled = false without model → validation passes.
    #[test]
    fn validate_llm_disabled_no_model_ok() {
        assert!(validate(&Config::default()).is_ok());
    }

    // ── load() ────────────────────────────────────────────────────────────────

    /// `load` combines parse + validate; enabled LLM with model succeeds.
    #[test]
    fn load_valid_llm_config() {
        let toml = "[llm]\nenabled = true\nmodel = \"llama3\"\n";
        assert!(load(toml).is_ok());
    }

    /// `load` returns error when parse fails.
    #[test]
    fn load_malformed_toml_errors() {
        assert!(load("= bad toml").is_err());
    }

    /// `load` returns error when semantic validation fails.
    #[test]
    fn load_enabled_without_model_errors() {
        let toml = "[llm]\nenabled = true\n";
        assert!(load(toml).is_err());
    }

    // ── full config round-trip ────────────────────────────────────────────────

    /// Parse a comprehensive TOML with all sections populated.
    #[test]
    fn full_config_parses_correctly() {
        let toml = r#"
schema_version = 1
safety_mode = "strict"

[llm]
enabled = true
provider = "ollama"
base_url = "http://127.0.0.1:11434"
model = "llama3"
timeout_secs = 30
allow_remote = false

[llm.invocation]
nl_session = true
assist_on_not_found = true
max_context_chars = 6000

[safety]
deny_prefixes = ["rm -rf /", "mkfs"]
allow_prefixes = ["ls", "git status"]
high_risk_patterns = ["sudo"]

[audit]
enabled = true
path = "/tmp/ls-audit.log"

[[hooks.pre_exec]]
command = ["echo", "pre-hook"]
timeout_ms = 1000

[limits]
max_stdout_capture_bytes = 524288
max_llm_payload_bytes = 100000
"#;
        let cfg = load(toml).unwrap();
        assert_eq!(cfg.schema_version, 1);
        assert_eq!(cfg.safety_mode, schema::SafetyMode::Strict);
        assert!(cfg.llm.enabled);
        assert_eq!(cfg.llm.model.as_deref(), Some("llama3"));
        assert_eq!(cfg.llm.timeout_secs, 30);
        assert!(cfg.llm.invocation.nl_session);
        assert_eq!(cfg.llm.invocation.max_context_chars, 6_000);
        assert_eq!(cfg.safety.deny_prefixes, vec!["rm -rf /", "mkfs"]);
        assert_eq!(cfg.safety.allow_prefixes, vec!["ls", "git status"]);
        assert_eq!(cfg.audit.path.as_deref(), Some("/tmp/ls-audit.log"));
        assert_eq!(cfg.hooks.pre_exec[0].command, vec!["echo", "pre-hook"]);
        assert_eq!(cfg.limits.max_stdout_capture_bytes, 524_288);
    }
}
