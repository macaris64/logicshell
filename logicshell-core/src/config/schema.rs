// Configuration schema types for `.logicshell.toml` — Framework PRD §12.1–12.6

use serde::{Deserialize, Serialize};

/// Root configuration loaded from `.logicshell.toml`.
///
/// All fields have compile-time defaults so an empty TOML file is valid.
/// Unknown keys are rejected at parse time (§12.7).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Schema version for forward-compatible migrations. Default: 1.
    #[serde(default = "defaults::schema_version")]
    pub schema_version: u32,
    /// Safety enforcement mode. Default: `balanced`.
    #[serde(default)]
    pub safety_mode: SafetyMode,
    /// LLM integration settings (§12.2).
    #[serde(default)]
    pub llm: LlmConfig,
    /// Safety policy lists (§12.3).
    #[serde(default)]
    pub safety: SafetyConfig,
    /// Audit log settings (§12.4).
    #[serde(default)]
    pub audit: AuditConfig,
    /// Pre-execution hooks (§12.5).
    #[serde(default)]
    pub hooks: HooksConfig,
    /// Payload size caps (§12.6).
    #[serde(default)]
    pub limits: LimitsConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: defaults::schema_version(),
            safety_mode: SafetyMode::default(),
            llm: LlmConfig::default(),
            safety: SafetyConfig::default(),
            audit: AuditConfig::default(),
            hooks: HooksConfig::default(),
            limits: LimitsConfig::default(),
        }
    }
}

/// Safety enforcement modes (§12.1, FR-30–33).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SafetyMode {
    /// Block all high-risk and AI-generated commands without confirmation.
    Strict,
    /// Require confirmation for high-risk and AI-generated commands (default).
    #[default]
    Balanced,
    /// Run all commands; AI suggestions still presented but auto-confirmed.
    Loose,
}

/// LLM integration configuration (`[llm]`, §12.2).
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LlmConfig {
    /// Master switch for any LLM call. Default: false.
    #[serde(default)]
    pub enabled: bool,
    /// LLM provider. Only `ollama` in MVP. Default: `ollama`.
    #[serde(default)]
    pub provider: LlmProvider,
    /// Ollama API base URL. Default: `http://127.0.0.1:11434`.
    #[serde(default = "defaults::base_url")]
    pub base_url: String,
    /// Model identifier — required when `enabled = true` (§12.2).
    pub model: Option<String>,
    /// HTTP timeout in seconds. Default: 60.
    #[serde(default = "defaults::timeout_secs")]
    pub timeout_secs: u64,
    /// Allow non-local endpoints. Must be false in MVP. Default: false.
    #[serde(default)]
    pub allow_remote: bool,
    /// Invocation mode toggles.
    #[serde(default)]
    pub invocation: LlmInvocationConfig,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: LlmProvider::default(),
            base_url: defaults::base_url(),
            model: None,
            timeout_secs: defaults::timeout_secs(),
            allow_remote: false,
            invocation: LlmInvocationConfig::default(),
        }
    }
}

/// LLM provider selection (§12.2).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    /// Local Ollama daemon (MVP-only provider).
    #[default]
    Ollama,
}

/// `[llm.invocation]` toggles controlling when the LLM is consulted (§12.2).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LlmInvocationConfig {
    /// Enable explicit natural-language session mode. Default: false.
    #[serde(default)]
    pub nl_session: bool,
    /// Suggest corrections on exit code 127. Default: false.
    #[serde(default)]
    pub assist_on_not_found: bool,
    /// Combined prompt context character cap. Default: 8 000.
    #[serde(default = "defaults::max_context_chars")]
    pub max_context_chars: u64,
}

impl Default for LlmInvocationConfig {
    fn default() -> Self {
        Self {
            nl_session: false,
            assist_on_not_found: false,
            max_context_chars: defaults::max_context_chars(),
        }
    }
}

/// Safety policy lists (`[safety]`, §12.3).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyConfig {
    /// Commands blocked if argv matches any prefix. Default: built-in destructive set.
    #[serde(default = "defaults::deny_prefixes")]
    pub deny_prefixes: Vec<String>,
    /// Commands explicitly allowed even when deny rules would match. Default: empty.
    #[serde(default)]
    pub allow_prefixes: Vec<String>,
    /// Regex/glob patterns for high-risk classification. Default: built-in set.
    #[serde(default = "defaults::high_risk_patterns")]
    pub high_risk_patterns: Vec<String>,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            deny_prefixes: defaults::deny_prefixes(),
            allow_prefixes: vec![],
            high_risk_patterns: defaults::high_risk_patterns(),
        }
    }
}

/// Audit log settings (`[audit]`, §12.4).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuditConfig {
    /// Enable audit logging. Default: true.
    #[serde(default = "defaults::audit_enabled")]
    pub enabled: bool,
    /// Writable path for the audit log. `None` → OS temp / `.logicshell-audit.log`.
    pub path: Option<String>,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: None,
        }
    }
}

/// Pre-execution hook container (`[hooks]`, §12.5).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HooksConfig {
    /// Hooks run before every dispatch. Default: empty.
    #[serde(default)]
    pub pre_exec: Vec<HookEntry>,
}

/// A single pre-exec hook definition (§12.5).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookEntry {
    /// Command argv — first element is the executable.
    pub command: Vec<String>,
    /// Milliseconds before the hook is killed. Default: 5 000.
    #[serde(default = "defaults::hook_timeout_ms")]
    pub timeout_ms: u64,
}

/// Payload size caps (`[limits]`, §12.6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsConfig {
    /// Maximum bytes captured from child stdout. Default: 1 MiB (1 048 576).
    #[serde(default = "defaults::max_stdout_capture_bytes")]
    pub max_stdout_capture_bytes: u64,
    /// Hard cap for payload bytes forwarded to the LLM. Default: 256 000.
    #[serde(default = "defaults::max_llm_payload_bytes")]
    pub max_llm_payload_bytes: u64,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_stdout_capture_bytes: defaults::max_stdout_capture_bytes(),
            max_llm_payload_bytes: defaults::max_llm_payload_bytes(),
        }
    }
}

/// Default value functions — each referenced by `#[serde(default = "...")]`.
pub(crate) mod defaults {
    pub fn schema_version() -> u32 {
        1
    }
    pub fn base_url() -> String {
        "http://127.0.0.1:11434".into()
    }
    pub fn timeout_secs() -> u64 {
        60
    }
    pub fn max_context_chars() -> u64 {
        8_000
    }
    pub fn deny_prefixes() -> Vec<String> {
        vec!["rm -rf /".into(), "mkfs".into(), "dd if=".into()]
    }
    pub fn high_risk_patterns() -> Vec<String> {
        vec![
            r"rm\s+-[rf]*r".into(),
            r"sudo\s+".into(),
            r"curl.*\|\s*bash".into(),
            r"wget.*\|\s*sh".into(),
        ]
    }
    pub fn audit_enabled() -> bool {
        true
    }
    pub fn hook_timeout_ms() -> u64 {
        5_000
    }
    pub fn max_stdout_capture_bytes() -> u64 {
        1_048_576
    }
    pub fn max_llm_payload_bytes() -> u64 {
        256_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Default values ────────────────────────────────────────────────────────

    #[test]
    fn default_schema_version_is_one() {
        assert_eq!(defaults::schema_version(), 1);
    }

    #[test]
    fn default_base_url() {
        assert_eq!(defaults::base_url(), "http://127.0.0.1:11434");
    }

    #[test]
    fn default_timeout_secs() {
        assert_eq!(defaults::timeout_secs(), 60);
    }

    #[test]
    fn default_max_context_chars() {
        assert_eq!(defaults::max_context_chars(), 8_000);
    }

    #[test]
    fn default_deny_prefixes_contains_rm_rf() {
        let p = defaults::deny_prefixes();
        assert!(p.iter().any(|s| s == "rm -rf /"));
        assert!(p.iter().any(|s| s == "mkfs"));
        assert!(p.iter().any(|s| s == "dd if="));
    }

    #[test]
    fn default_high_risk_patterns_non_empty() {
        assert!(!defaults::high_risk_patterns().is_empty());
    }

    #[test]
    fn default_audit_enabled_is_true() {
        assert!(defaults::audit_enabled());
    }

    #[test]
    fn default_hook_timeout_ms() {
        assert_eq!(defaults::hook_timeout_ms(), 5_000);
    }

    #[test]
    fn default_max_stdout_capture_bytes_is_1mib() {
        assert_eq!(defaults::max_stdout_capture_bytes(), 1_048_576);
    }

    #[test]
    fn default_max_llm_payload_bytes() {
        assert_eq!(defaults::max_llm_payload_bytes(), 256_000);
    }

    // ── Config::default() ─────────────────────────────────────────────────────

    #[test]
    fn config_default_schema_version() {
        assert_eq!(Config::default().schema_version, 1);
    }

    #[test]
    fn config_default_safety_mode_is_balanced() {
        assert_eq!(Config::default().safety_mode, SafetyMode::Balanced);
    }

    #[test]
    fn config_default_llm_disabled() {
        assert!(!Config::default().llm.enabled);
    }

    #[test]
    fn config_default_llm_provider_ollama() {
        assert_eq!(Config::default().llm.provider, LlmProvider::Ollama);
    }

    #[test]
    fn config_default_llm_model_none() {
        assert!(Config::default().llm.model.is_none());
    }

    #[test]
    fn config_default_llm_allow_remote_false() {
        assert!(!Config::default().llm.allow_remote);
    }

    #[test]
    fn config_default_invocation_nl_session_false() {
        assert!(!Config::default().llm.invocation.nl_session);
    }

    #[test]
    fn config_default_invocation_assist_on_not_found_false() {
        assert!(!Config::default().llm.invocation.assist_on_not_found);
    }

    #[test]
    fn config_default_invocation_max_context_chars() {
        assert_eq!(Config::default().llm.invocation.max_context_chars, 8_000);
    }

    #[test]
    fn config_default_safety_deny_prefixes_non_empty() {
        assert!(!Config::default().safety.deny_prefixes.is_empty());
    }

    #[test]
    fn config_default_safety_allow_prefixes_empty() {
        assert!(Config::default().safety.allow_prefixes.is_empty());
    }

    #[test]
    fn config_default_audit_enabled() {
        assert!(Config::default().audit.enabled);
    }

    #[test]
    fn config_default_audit_path_none() {
        assert!(Config::default().audit.path.is_none());
    }

    #[test]
    fn config_default_hooks_pre_exec_empty() {
        assert!(Config::default().hooks.pre_exec.is_empty());
    }

    #[test]
    fn config_default_limits_stdout_cap() {
        assert_eq!(Config::default().limits.max_stdout_capture_bytes, 1_048_576);
    }

    #[test]
    fn config_default_limits_llm_cap() {
        assert_eq!(Config::default().limits.max_llm_payload_bytes, 256_000);
    }

    // ── SafetyMode variants ───────────────────────────────────────────────────

    #[test]
    fn safety_mode_variants_clone_debug_eq() {
        let modes = [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose];
        for m in &modes {
            let c = m.clone();
            assert_eq!(&c, m);
            assert!(!format!("{m:?}").is_empty());
        }
    }

    // ── LlmProvider variants ──────────────────────────────────────────────────

    #[test]
    fn llm_provider_clone_debug_eq() {
        let p = LlmProvider::Ollama;
        assert_eq!(p.clone(), LlmProvider::Ollama);
        assert!(format!("{p:?}").contains("Ollama"));
    }

    // ── HookEntry ─────────────────────────────────────────────────────────────

    #[test]
    fn hook_entry_fields() {
        let h = HookEntry {
            command: vec!["echo".into(), "hello".into()],
            timeout_ms: 1_000,
        };
        assert_eq!(h.command[0], "echo");
        assert_eq!(h.timeout_ms, 1_000);
    }

    // ── Clone + PartialEq on all structs ──────────────────────────────────────

    #[test]
    fn config_clone_eq() {
        let a = Config::default();
        assert_eq!(a.clone(), a);
    }

    #[test]
    fn llm_config_clone_eq() {
        let a = LlmConfig::default();
        assert_eq!(a.clone(), a);
    }

    #[test]
    fn llm_invocation_clone_eq() {
        let a = LlmInvocationConfig::default();
        assert_eq!(a.clone(), a);
    }

    #[test]
    fn safety_config_clone_eq() {
        let a = SafetyConfig::default();
        assert_eq!(a.clone(), a);
    }

    #[test]
    fn audit_config_clone_eq() {
        let a = AuditConfig::default();
        assert_eq!(a.clone(), a);
    }

    #[test]
    fn hooks_config_clone_eq() {
        let a = HooksConfig::default();
        assert_eq!(a.clone(), a);
    }

    #[test]
    fn limits_config_clone_eq() {
        let a = LimitsConfig::default();
        assert_eq!(a.clone(), a);
    }

    #[test]
    fn hook_entry_clone_eq() {
        let h = HookEntry {
            command: vec!["ls".into()],
            timeout_ms: 200,
        };
        assert_eq!(h.clone(), h);
    }
}
