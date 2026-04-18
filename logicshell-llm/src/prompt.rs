// Prompt composer — LLM Module PRD §5.3
//
// Pure, sync, deterministic. All environment access is delegated to
// `SystemContextProvider`; this module never reads `std::env` directly (FR-11).
// Templates are embedded at compile time via `include_str!`.

use logicshell_core::config::LlmConfig;

use crate::{client::LlmRequest, context::SystemContextSnapshot, error::LlmError};

const NL_TO_COMMAND_TEMPLATE: &str = include_str!("templates/nl_to_command.txt");
const ASSIST_ON_127_TEMPLATE: &str = include_str!("templates/assist_on_127.txt");

/// Builds LLM prompts from templates, enforcing the `max_context_chars` cap.
///
/// `PromptComposer` is pure and sync — no I/O, no side effects. Construct once
/// from a [`LlmConfig`] or directly with [`PromptComposer::new`].
#[derive(Debug)]
pub struct PromptComposer {
    model: String,
    max_context_chars: usize,
}

impl PromptComposer {
    /// Construct directly with a model name and character cap.
    pub fn new(model: impl Into<String>, max_context_chars: usize) -> Self {
        Self {
            model: model.into(),
            max_context_chars,
        }
    }

    /// Construct from an [`LlmConfig`], returning errors for disabled / missing model.
    pub fn from_config(config: &LlmConfig) -> Result<Self, LlmError> {
        if !config.enabled {
            return Err(LlmError::Disabled);
        }
        let model = config.model.clone().ok_or(LlmError::ModelNotSpecified)?;
        Ok(Self {
            model,
            max_context_chars: config.invocation.max_context_chars as usize,
        })
    }

    /// Model name this composer will embed in every [`LlmRequest`].
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Character cap applied to every composed prompt.
    pub fn max_context_chars(&self) -> usize {
        self.max_context_chars
    }

    /// Compose a natural-language-to-command prompt.
    ///
    /// Returns `Err(LlmError::ContextTooLarge)` if the rendered prompt exceeds
    /// `max_context_chars`.
    pub fn compose_nl_to_command(
        &self,
        nl_input: &str,
        ctx: &SystemContextSnapshot,
    ) -> Result<LlmRequest, LlmError> {
        let prompt = NL_TO_COMMAND_TEMPLATE
            .replace("{os_family}", &ctx.os_family)
            .replace("{arch}", &ctx.arch)
            .replace("{cwd}", &ctx.cwd)
            .replace("{path_dirs}", &ctx.path_dirs.join(":"))
            .replace("{nl_input}", nl_input);

        self.check_length(&prompt)?;

        Ok(LlmRequest {
            model: self.model.clone(),
            prompt,
        })
    }

    /// Compose an exit-code-127 assist prompt.
    ///
    /// `failed_argv` is the argv that returned 127; the composer joins it with
    /// spaces to embed in the template. Returns `Err(LlmError::ContextTooLarge)`
    /// if the prompt is too long.
    pub fn compose_assist_on_127(
        &self,
        failed_argv: &[&str],
        ctx: &SystemContextSnapshot,
    ) -> Result<LlmRequest, LlmError> {
        let failed_cmd = failed_argv.join(" ");
        let prompt = ASSIST_ON_127_TEMPLATE
            .replace("{os_family}", &ctx.os_family)
            .replace("{arch}", &ctx.arch)
            .replace("{cwd}", &ctx.cwd)
            .replace("{path_dirs}", &ctx.path_dirs.join(":"))
            .replace("{failed_cmd}", &failed_cmd);

        self.check_length(&prompt)?;

        Ok(LlmRequest {
            model: self.model.clone(),
            prompt,
        })
    }

    /// Return `Err(ContextTooLarge)` if `prompt` exceeds the configured cap.
    fn check_length(&self, prompt: &str) -> Result<(), LlmError> {
        let size = prompt.chars().count();
        if size > self.max_context_chars {
            return Err(LlmError::ContextTooLarge {
                size,
                max: self.max_context_chars,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use logicshell_core::config::LlmConfig;

    fn ctx() -> SystemContextSnapshot {
        SystemContextSnapshot {
            os_family: "linux".into(),
            arch: "x86_64".into(),
            cwd: "/home/user/project".into(),
            path_dirs: vec!["/usr/bin".into(), "/bin".into()],
        }
    }

    fn ctx_empty_path() -> SystemContextSnapshot {
        SystemContextSnapshot {
            path_dirs: vec![],
            ..ctx()
        }
    }

    // ── PromptComposer::new ───────────────────────────────────────────────────

    #[test]
    fn new_stores_model_and_cap() {
        let c = PromptComposer::new("llama3", 8_000);
        assert_eq!(c.model(), "llama3");
        assert_eq!(c.max_context_chars(), 8_000);
    }

    #[test]
    fn new_with_zero_cap() {
        let c = PromptComposer::new("m", 0);
        assert_eq!(c.max_context_chars(), 0);
    }

    // ── PromptComposer::from_config ───────────────────────────────────────────

    #[test]
    fn from_config_disabled_llm_returns_disabled_error() {
        let cfg = LlmConfig {
            enabled: false,
            ..LlmConfig::default()
        };
        let result = PromptComposer::from_config(&cfg);
        assert_eq!(result.unwrap_err(), LlmError::Disabled);
    }

    #[test]
    fn from_config_enabled_no_model_returns_model_not_specified() {
        let cfg = LlmConfig {
            enabled: true,
            model: None,
            ..LlmConfig::default()
        };
        let result = PromptComposer::from_config(&cfg);
        assert_eq!(result.unwrap_err(), LlmError::ModelNotSpecified);
    }

    #[test]
    fn from_config_enabled_with_model_succeeds() {
        let cfg = LlmConfig {
            enabled: true,
            model: Some("llama3".into()),
            ..LlmConfig::default()
        };
        let c = PromptComposer::from_config(&cfg).unwrap();
        assert_eq!(c.model(), "llama3");
        assert_eq!(c.max_context_chars(), 8_000); // default max_context_chars
    }

    #[test]
    fn from_config_respects_max_context_chars() {
        let mut cfg = LlmConfig {
            enabled: true,
            model: Some("m".into()),
            ..LlmConfig::default()
        };
        cfg.invocation.max_context_chars = 4_000;
        let c = PromptComposer::from_config(&cfg).unwrap();
        assert_eq!(c.max_context_chars(), 4_000);
    }

    // ── compose_nl_to_command ─────────────────────────────────────────────────

    #[test]
    fn nl_to_command_includes_nl_input() {
        let c = PromptComposer::new("llama3", 8_000);
        let req = c
            .compose_nl_to_command("list files sorted by size", &ctx())
            .unwrap();
        assert!(
            req.prompt.contains("list files sorted by size"),
            "prompt must contain nl_input"
        );
    }

    #[test]
    fn nl_to_command_model_propagated() {
        let c = PromptComposer::new("mistral", 8_000);
        let req = c.compose_nl_to_command("show disk usage", &ctx()).unwrap();
        assert_eq!(req.model, "mistral");
    }

    #[test]
    fn nl_to_command_contains_os_family() {
        let c = PromptComposer::new("llama3", 8_000);
        let req = c.compose_nl_to_command("ls", &ctx()).unwrap();
        assert!(
            req.prompt.contains("linux"),
            "prompt must contain os_family"
        );
    }

    #[test]
    fn nl_to_command_contains_arch() {
        let c = PromptComposer::new("llama3", 8_000);
        let req = c.compose_nl_to_command("ls", &ctx()).unwrap();
        assert!(req.prompt.contains("x86_64"), "prompt must contain arch");
    }

    #[test]
    fn nl_to_command_contains_cwd() {
        let c = PromptComposer::new("llama3", 8_000);
        let req = c.compose_nl_to_command("ls", &ctx()).unwrap();
        assert!(
            req.prompt.contains("/home/user/project"),
            "prompt must contain cwd"
        );
    }

    #[test]
    fn nl_to_command_contains_path_dirs() {
        let c = PromptComposer::new("llama3", 8_000);
        let req = c.compose_nl_to_command("ls", &ctx()).unwrap();
        assert!(
            req.prompt.contains("/usr/bin"),
            "prompt must contain PATH entries"
        );
    }

    #[test]
    fn nl_to_command_empty_path_dirs() {
        let c = PromptComposer::new("llama3", 8_000);
        let req = c.compose_nl_to_command("ls", &ctx_empty_path()).unwrap();
        assert!(!req.prompt.is_empty());
    }

    #[test]
    fn nl_to_command_context_too_large() {
        let c = PromptComposer::new("m", 10); // tiny cap
        let result = c.compose_nl_to_command("ls", &ctx());
        assert!(
            matches!(result, Err(LlmError::ContextTooLarge { .. })),
            "should error when prompt > cap"
        );
    }

    #[test]
    fn nl_to_command_zero_cap_always_errors() {
        let c = PromptComposer::new("m", 0);
        let result = c.compose_nl_to_command("ls", &ctx());
        assert!(matches!(result, Err(LlmError::ContextTooLarge { .. })));
    }

    #[test]
    fn nl_to_command_context_too_large_reports_sizes() {
        let c = PromptComposer::new("m", 5);
        match c.compose_nl_to_command("ls", &ctx()).unwrap_err() {
            LlmError::ContextTooLarge { size, max } => {
                assert!(size > 5, "size should be > 5, got {size}");
                assert_eq!(max, 5);
            }
            other => panic!("expected ContextTooLarge, got {other:?}"),
        }
    }

    // ── compose_assist_on_127 ─────────────────────────────────────────────────

    #[test]
    fn assist_127_includes_failed_command() {
        let c = PromptComposer::new("llama3", 8_000);
        let req = c.compose_assist_on_127(&["lss", "-la"], &ctx()).unwrap();
        assert!(
            req.prompt.contains("lss -la"),
            "prompt must contain failed command"
        );
    }

    #[test]
    fn assist_127_model_propagated() {
        let c = PromptComposer::new("codellama", 8_000);
        let req = c.compose_assist_on_127(&["gti", "status"], &ctx()).unwrap();
        assert_eq!(req.model, "codellama");
    }

    #[test]
    fn assist_127_contains_os_context() {
        let c = PromptComposer::new("m", 8_000);
        let req = c.compose_assist_on_127(&["cmd"], &ctx()).unwrap();
        assert!(req.prompt.contains("linux"));
        assert!(req.prompt.contains("x86_64"));
    }

    #[test]
    fn assist_127_single_argv_element() {
        let c = PromptComposer::new("m", 8_000);
        let req = c.compose_assist_on_127(&["dockerr"], &ctx()).unwrap();
        assert!(req.prompt.contains("dockerr"));
    }

    #[test]
    fn assist_127_empty_argv_works() {
        let c = PromptComposer::new("m", 8_000);
        let req = c.compose_assist_on_127(&[], &ctx()).unwrap();
        // Empty argv → empty failed_cmd substituted; prompt still valid
        assert!(!req.prompt.is_empty());
    }

    #[test]
    fn assist_127_context_too_large() {
        let c = PromptComposer::new("m", 10);
        let result = c.compose_assist_on_127(&["cmd"], &ctx());
        assert!(matches!(result, Err(LlmError::ContextTooLarge { .. })));
    }

    #[test]
    fn assist_127_zero_cap_always_errors() {
        let c = PromptComposer::new("m", 0);
        let result = c.compose_assist_on_127(&["cmd"], &ctx());
        assert!(matches!(result, Err(LlmError::ContextTooLarge { .. })));
    }

    // ── check_length boundary ─────────────────────────────────────────────────

    #[test]
    fn exact_cap_length_succeeds() {
        // Build a prompt that is exactly at the limit
        // Use a very large cap so the template passes
        let c = PromptComposer::new("m", usize::MAX);
        let req = c.compose_nl_to_command("ls", &ctx()).unwrap();
        let len = req.prompt.chars().count();

        // Create a composer with exactly the prompt length as cap
        let c2 = PromptComposer::new("m", len);
        let result = c2.compose_nl_to_command("ls", &ctx());
        assert!(result.is_ok(), "prompt at exactly cap length must succeed");
    }

    #[test]
    fn one_over_cap_fails() {
        let c = PromptComposer::new("m", usize::MAX);
        let req = c.compose_nl_to_command("ls", &ctx()).unwrap();
        let len = req.prompt.chars().count();

        // Cap is one less than the prompt length
        let c2 = PromptComposer::new("m", len - 1);
        let result = c2.compose_nl_to_command("ls", &ctx());
        assert!(
            matches!(result, Err(LlmError::ContextTooLarge { .. })),
            "one char over cap must fail"
        );
    }

    // ── Templates are embedded (non-empty) ───────────────────────────────────

    #[test]
    fn nl_to_command_template_is_non_empty() {
        assert!(!NL_TO_COMMAND_TEMPLATE.is_empty());
        assert!(NL_TO_COMMAND_TEMPLATE.contains("{nl_input}"));
    }

    #[test]
    fn assist_on_127_template_is_non_empty() {
        assert!(!ASSIST_ON_127_TEMPLATE.is_empty());
        assert!(ASSIST_ON_127_TEMPLATE.contains("{failed_cmd}"));
    }
}
