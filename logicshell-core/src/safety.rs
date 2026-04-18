// Safety policy engine — FR-30–33, §10.1–10.2
//
// Sync, pure, deterministic: identical input → identical output.
// Regexes are compiled once at construction time; every `evaluate` call is
// allocation-minimal and free of I/O.

use regex::Regex;

use crate::config::{SafetyConfig, SafetyMode};

/// Risk classification category per FR-30.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskCategory {
    /// Irreversible or destructive filesystem modifications (rm, dd, mkfs, shred).
    DestructiveFilesystem,
    /// Commands that elevate process privileges (sudo, su, doas).
    PrivilegeElevation,
    /// Network operations piped into a shell or code executor (curl|bash).
    Network,
    /// Package or system-level changes (apt, yum, dnf, pip, npm install/remove).
    PackageSystem,
}

/// Risk severity level derived from the accumulated score.
///
/// Levels are orderable: `None < Low < Medium < High < Critical`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    /// No risk patterns matched; safe to run without restriction.
    None,
    /// Marginal risk — generally allowed even in strict mode.
    Low,
    /// Elevated risk — confirmation required in strict/balanced modes.
    Medium,
    /// Very high risk — blocked in strict mode, confirmation in balanced.
    High,
    /// Catastrophic risk (explicit deny prefix) — blocked in all modes.
    Critical,
}

/// Full output of a single safety policy evaluation.
#[derive(Debug, Clone)]
pub struct RiskAssessment {
    /// Severity derived from `score`.
    pub level: RiskLevel,
    /// Numeric risk score 0–100; accumulates from matched patterns.
    pub score: u32,
    /// Human-readable explanations for each risk contribution.
    pub reasons: Vec<String>,
    /// Risk categories triggered by this command.
    pub categories: Vec<RiskCategory>,
}

/// Safety policy decision returned alongside a [`RiskAssessment`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Command is safe to dispatch immediately.
    Allow,
    /// Command is blocked by policy; do not dispatch.
    Deny,
    /// Command requires explicit user confirmation before dispatch.
    Confirm,
}

/// Internal compiled pattern entry.
struct PatternEntry {
    source: String,
    regex: Regex,
    score: u32,
    category: RiskCategory,
}

/// Sync, pure safety policy engine (FR-30–33).
///
/// Constructed once from a [`SafetyMode`] and [`SafetyConfig`]; regex patterns
/// from config are compiled at construction time.  All [`evaluate`] calls are
/// deterministic: identical input always produces identical output.
///
/// [`evaluate`]: SafetyPolicyEngine::evaluate
pub struct SafetyPolicyEngine {
    mode: SafetyMode,
    deny_prefixes: Vec<String>,
    allow_prefixes: Vec<String>,
    patterns: Vec<PatternEntry>,
}

impl SafetyPolicyEngine {
    /// Build the engine from a [`SafetyMode`] and [`SafetyConfig`].
    ///
    /// Invalid regex patterns in `high_risk_patterns` are silently skipped;
    /// they cannot match any command, so they contribute no risk score.
    pub fn new(mode: SafetyMode, config: &SafetyConfig) -> Self {
        let patterns = config
            .high_risk_patterns
            .iter()
            .filter_map(|p| {
                Regex::new(p).ok().map(|re| PatternEntry {
                    source: p.clone(),
                    regex: re,
                    score: score_for_pattern(p),
                    category: category_for_pattern(p),
                })
            })
            .collect();

        Self {
            mode,
            deny_prefixes: config.deny_prefixes.clone(),
            allow_prefixes: config.allow_prefixes.clone(),
            patterns,
        }
    }

    /// Evaluate the safety policy for `argv` and return the assessment + decision.
    ///
    /// Evaluation order (FR-33: deny wins over allow):
    /// 1. **Deny prefix** — if `argv` matches any deny prefix, return `Deny` immediately.
    /// 2. **Allow prefix** — if `argv` matches any allow prefix, return `Allow` immediately.
    /// 3. **Pattern scoring** — accumulate risk score from compiled high-risk patterns.
    /// 4. **Mode-based decision** — convert the final score to a `Decision` per `safety_mode`.
    pub fn evaluate(&self, argv: &[&str]) -> (RiskAssessment, Decision) {
        if argv.is_empty() {
            return (
                RiskAssessment {
                    level: RiskLevel::None,
                    score: 0,
                    reasons: vec![],
                    categories: vec![],
                },
                Decision::Allow,
            );
        }

        let cmd_str = argv.join(" ");

        // 1. Deny prefix (FR-33: deny wins over allow).
        for prefix in &self.deny_prefixes {
            if cmd_str.starts_with(prefix.as_str()) {
                return (
                    RiskAssessment {
                        level: RiskLevel::Critical,
                        score: 100,
                        reasons: vec![format!("explicitly denied (prefix: '{prefix}')")],
                        categories: vec![category_for_prefix(prefix)],
                    },
                    Decision::Deny,
                );
            }
        }

        // 2. Allow prefix.
        for prefix in &self.allow_prefixes {
            if cmd_str.starts_with(prefix.as_str()) {
                return (
                    RiskAssessment {
                        level: RiskLevel::None,
                        score: 0,
                        reasons: vec![format!("explicitly allowlisted (prefix: '{prefix}')")],
                        categories: vec![],
                    },
                    Decision::Allow,
                );
            }
        }

        // 3. Accumulate risk from compiled high-risk patterns.
        let mut score: u32 = 0;
        let mut reasons = Vec::new();
        let mut categories: Vec<RiskCategory> = Vec::new();

        for entry in &self.patterns {
            if entry.regex.is_match(&cmd_str) {
                score = score.saturating_add(entry.score);
                reasons.push(format!(
                    "matched high-risk pattern '{}' (+{})",
                    entry.source, entry.score
                ));
                if !categories.contains(&entry.category) {
                    categories.push(entry.category.clone());
                }
            }
        }

        let score = score.min(100);
        let level = level_from_score(score);
        let decision = self.decide(&level);

        (
            RiskAssessment {
                level,
                score,
                reasons,
                categories,
            },
            decision,
        )
    }

    fn decide(&self, level: &RiskLevel) -> Decision {
        match &self.mode {
            SafetyMode::Strict => match level {
                RiskLevel::None | RiskLevel::Low => Decision::Allow,
                RiskLevel::Medium => Decision::Confirm,
                RiskLevel::High | RiskLevel::Critical => Decision::Deny,
            },
            SafetyMode::Balanced => match level {
                RiskLevel::None | RiskLevel::Low => Decision::Allow,
                RiskLevel::Medium | RiskLevel::High => Decision::Confirm,
                RiskLevel::Critical => Decision::Deny,
            },
            SafetyMode::Loose => match level {
                RiskLevel::None | RiskLevel::Low | RiskLevel::Medium => Decision::Allow,
                RiskLevel::High => Decision::Confirm,
                RiskLevel::Critical => Decision::Deny,
            },
        }
    }
}

fn level_from_score(score: u32) -> RiskLevel {
    match score {
        0 => RiskLevel::None,
        1..=25 => RiskLevel::Low,
        26..=50 => RiskLevel::Medium,
        51..=80 => RiskLevel::High,
        _ => RiskLevel::Critical,
    }
}

fn score_for_pattern(pattern: &str) -> u32 {
    // Pipe-to-shell (curl|bash, wget|sh) — highest risk after explicit denies.
    if (pattern.contains("curl") || pattern.contains("wget"))
        && (pattern.contains("bash") || pattern.contains("sh"))
    {
        55
    } else if pattern.contains("rm") {
        // Recursive delete
        50
    } else {
        // sudo, privilege escalation, and other patterns
        30
    }
}

fn category_for_pattern(pattern: &str) -> RiskCategory {
    if pattern.contains("rm") || pattern.contains("dd") || pattern.contains("mkfs") {
        RiskCategory::DestructiveFilesystem
    } else if pattern.contains("sudo") || pattern.contains("su ") || pattern.contains("doas") {
        RiskCategory::PrivilegeElevation
    } else if pattern.contains("curl") || pattern.contains("wget") || pattern.contains("http") {
        RiskCategory::Network
    } else if pattern.contains("apt")
        || pattern.contains("pip")
        || pattern.contains("npm")
        || pattern.contains("yum")
    {
        RiskCategory::PackageSystem
    } else {
        RiskCategory::DestructiveFilesystem
    }
}

fn category_for_prefix(prefix: &str) -> RiskCategory {
    if prefix.contains("rm ")
        || prefix.contains("mkfs")
        || prefix.contains("dd if=")
        || prefix.contains("shred")
    {
        RiskCategory::DestructiveFilesystem
    } else if prefix.contains("sudo") || prefix.contains("su ") || prefix.contains("doas") {
        RiskCategory::PrivilegeElevation
    } else if prefix.contains("curl") || prefix.contains("wget") {
        RiskCategory::Network
    } else if prefix.contains("apt")
        || prefix.contains("pip")
        || prefix.contains("yum")
        || prefix.contains("dnf")
        || prefix.contains("pacman")
    {
        RiskCategory::PackageSystem
    } else {
        RiskCategory::DestructiveFilesystem
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SafetyConfig, SafetyMode};

    fn default_engine(mode: SafetyMode) -> SafetyPolicyEngine {
        SafetyPolicyEngine::new(mode, &SafetyConfig::default())
    }

    fn allow_engine(mode: SafetyMode, allow: Vec<&str>) -> SafetyPolicyEngine {
        let mut cfg = SafetyConfig::default();
        cfg.allow_prefixes = allow.iter().map(|s| s.to_string()).collect();
        SafetyPolicyEngine::new(mode, &cfg)
    }

    fn custom_engine(mode: SafetyMode, deny: Vec<&str>, allow: Vec<&str>) -> SafetyPolicyEngine {
        let mut cfg = SafetyConfig::default();
        cfg.deny_prefixes = deny.iter().map(|s| s.to_string()).collect();
        cfg.allow_prefixes = allow.iter().map(|s| s.to_string()).collect();
        SafetyPolicyEngine::new(mode, &cfg)
    }

    // ── Decision and RiskLevel trait impls ────────────────────────────────────

    #[test]
    fn decision_clone_debug_eq() {
        for d in [Decision::Allow, Decision::Deny, Decision::Confirm] {
            let c = d.clone();
            assert_eq!(c, d);
            assert!(!format!("{d:?}").is_empty());
        }
    }

    #[test]
    fn risk_level_ordering() {
        assert!(RiskLevel::None < RiskLevel::Low);
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);
    }

    #[test]
    fn risk_level_clone_debug_eq() {
        let levels = [
            RiskLevel::None,
            RiskLevel::Low,
            RiskLevel::Medium,
            RiskLevel::High,
            RiskLevel::Critical,
        ];
        for l in &levels {
            assert_eq!(l.clone(), *l);
            assert!(!format!("{l:?}").is_empty());
        }
    }

    #[test]
    fn risk_category_clone_debug_eq() {
        for c in [
            RiskCategory::DestructiveFilesystem,
            RiskCategory::PrivilegeElevation,
            RiskCategory::Network,
            RiskCategory::PackageSystem,
        ] {
            let cloned = c.clone();
            assert_eq!(cloned, c);
            assert!(!format!("{c:?}").is_empty());
        }
    }

    #[test]
    fn risk_assessment_fields_accessible() {
        let a = RiskAssessment {
            level: RiskLevel::Medium,
            score: 40,
            reasons: vec!["test reason".into()],
            categories: vec![RiskCategory::PrivilegeElevation],
        };
        assert_eq!(a.score, 40);
        assert_eq!(a.level, RiskLevel::Medium);
        assert_eq!(a.reasons.len(), 1);
        assert_eq!(a.categories.len(), 1);
    }

    #[test]
    fn risk_assessment_clone() {
        let a = RiskAssessment {
            level: RiskLevel::High,
            score: 70,
            reasons: vec!["x".into()],
            categories: vec![RiskCategory::Network],
        };
        let b = a.clone();
        assert_eq!(b.score, 70);
    }

    // ── level_from_score boundaries ───────────────────────────────────────────

    #[test]
    fn level_from_score_zero_is_none() {
        assert_eq!(level_from_score(0), RiskLevel::None);
    }

    #[test]
    fn level_from_score_boundaries() {
        assert_eq!(level_from_score(1), RiskLevel::Low);
        assert_eq!(level_from_score(25), RiskLevel::Low);
        assert_eq!(level_from_score(26), RiskLevel::Medium);
        assert_eq!(level_from_score(50), RiskLevel::Medium);
        assert_eq!(level_from_score(51), RiskLevel::High);
        assert_eq!(level_from_score(80), RiskLevel::High);
        assert_eq!(level_from_score(81), RiskLevel::Critical);
        assert_eq!(level_from_score(100), RiskLevel::Critical);
    }

    // ── score_for_pattern ─────────────────────────────────────────────────────

    #[test]
    fn score_for_rm_pattern() {
        assert_eq!(score_for_pattern(r"rm\s+-[rf]*r"), 50);
    }

    #[test]
    fn score_for_curl_bash_pattern() {
        assert_eq!(score_for_pattern(r"curl.*\|\s*bash"), 55);
    }

    #[test]
    fn score_for_wget_sh_pattern() {
        assert_eq!(score_for_pattern(r"wget.*\|\s*sh"), 55);
    }

    #[test]
    fn score_for_sudo_pattern() {
        assert_eq!(score_for_pattern(r"sudo\s+"), 30);
    }

    #[test]
    fn score_for_unknown_pattern_default() {
        assert_eq!(score_for_pattern("some-custom-pattern"), 30);
    }

    // ── category_for_pattern ──────────────────────────────────────────────────

    #[test]
    fn category_for_rm_pattern_is_destructive() {
        assert_eq!(
            category_for_pattern(r"rm\s+-[rf]*r"),
            RiskCategory::DestructiveFilesystem
        );
    }

    #[test]
    fn category_for_sudo_pattern_is_privilege() {
        assert_eq!(
            category_for_pattern(r"sudo\s+"),
            RiskCategory::PrivilegeElevation
        );
    }

    #[test]
    fn category_for_curl_pattern_is_network() {
        assert_eq!(
            category_for_pattern(r"curl.*\|\s*bash"),
            RiskCategory::Network
        );
    }

    #[test]
    fn category_for_wget_pattern_is_network() {
        assert_eq!(
            category_for_pattern(r"wget.*\|\s*sh"),
            RiskCategory::Network
        );
    }

    #[test]
    fn category_for_apt_pattern_is_package() {
        assert_eq!(
            category_for_pattern("apt install"),
            RiskCategory::PackageSystem
        );
    }

    #[test]
    fn category_for_pip_pattern_is_package() {
        assert_eq!(
            category_for_pattern("pip install"),
            RiskCategory::PackageSystem
        );
    }

    #[test]
    fn category_for_unknown_pattern_default_destructive() {
        assert_eq!(
            category_for_pattern("unknown"),
            RiskCategory::DestructiveFilesystem
        );
    }

    // ── category_for_prefix ───────────────────────────────────────────────────

    #[test]
    fn category_for_prefix_rm_is_destructive() {
        assert_eq!(
            category_for_prefix("rm -rf /"),
            RiskCategory::DestructiveFilesystem
        );
    }

    #[test]
    fn category_for_prefix_mkfs_is_destructive() {
        assert_eq!(
            category_for_prefix("mkfs"),
            RiskCategory::DestructiveFilesystem
        );
    }

    #[test]
    fn category_for_prefix_sudo_is_privilege() {
        assert_eq!(
            category_for_prefix("sudo rm"),
            RiskCategory::PrivilegeElevation
        );
    }

    #[test]
    fn category_for_prefix_curl_is_network() {
        assert_eq!(category_for_prefix("curl"), RiskCategory::Network);
    }

    #[test]
    fn category_for_prefix_apt_is_package() {
        assert_eq!(
            category_for_prefix("apt install"),
            RiskCategory::PackageSystem
        );
    }

    #[test]
    fn category_for_prefix_unknown_default_destructive() {
        assert_eq!(
            category_for_prefix("zzz"),
            RiskCategory::DestructiveFilesystem
        );
    }

    // ── Golden tests: empty argv ──────────────────────────────────────────────

    #[test]
    fn empty_argv_always_allow() {
        for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
            let engine = default_engine(mode);
            let (assessment, decision) = engine.evaluate(&[]);
            assert_eq!(decision, Decision::Allow);
            assert_eq!(assessment.level, RiskLevel::None);
            assert_eq!(assessment.score, 0);
        }
    }

    // ── Golden test: ls (safe command) ───────────────────────────────────────

    #[test]
    fn ls_is_allowed_in_all_modes() {
        for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
            let engine = default_engine(mode.clone());
            let (assessment, decision) = engine.evaluate(&["ls"]);
            assert_eq!(decision, Decision::Allow, "ls should Allow in {mode:?}");
            assert_eq!(assessment.level, RiskLevel::None);
            assert_eq!(assessment.score, 0);
            assert!(assessment.reasons.is_empty());
        }
    }

    #[test]
    fn ls_la_is_allowed_in_all_modes() {
        for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
            let engine = default_engine(mode);
            let (_, decision) = engine.evaluate(&["ls", "-la", "/tmp"]);
            assert_eq!(decision, Decision::Allow);
        }
    }

    // ── Golden test: rm -rf / (deny prefix) ──────────────────────────────────

    #[test]
    fn rm_rf_root_is_denied_in_all_modes() {
        for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
            let engine = default_engine(mode.clone());
            let (assessment, decision) = engine.evaluate(&["rm", "-rf", "/"]);
            assert_eq!(decision, Decision::Deny, "rm -rf / should Deny in {mode:?}");
            assert_eq!(assessment.level, RiskLevel::Critical);
            assert_eq!(assessment.score, 100);
            assert!(
                assessment.reasons[0].contains("explicitly denied"),
                "reason: {:?}",
                assessment.reasons
            );
        }
    }

    #[test]
    fn rm_rf_slash_prefix_also_catches_subdirectory() {
        // "rm -rf /home" starts with "rm -rf /" → deny prefix fires
        for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
            let engine = default_engine(mode);
            let (_, decision) = engine.evaluate(&["rm", "-rf", "/home"]);
            assert_eq!(decision, Decision::Deny);
        }
    }

    #[test]
    fn mkfs_prefix_is_denied_in_all_modes() {
        for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
            let engine = default_engine(mode);
            let (_, decision) = engine.evaluate(&["mkfs", "/dev/sda"]);
            assert_eq!(decision, Decision::Deny);
        }
    }

    #[test]
    fn dd_if_prefix_is_denied_in_all_modes() {
        for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
            let engine = default_engine(mode);
            let (_, decision) = engine.evaluate(&["dd", "if=/dev/zero", "of=/dev/sda"]);
            assert_eq!(decision, Decision::Deny);
        }
    }

    // ── Golden test: curl | bash (pipe to shell) ──────────────────────────────

    #[test]
    fn curl_pipe_bash_is_denied_in_strict() {
        let engine = default_engine(SafetyMode::Strict);
        let (assessment, decision) =
            engine.evaluate(&["curl", "http://example.com/install.sh", "|", "bash"]);
        assert_eq!(decision, Decision::Deny, "strict should deny curl|bash");
        assert!(
            assessment.score >= 51,
            "score should be High: {}",
            assessment.score
        );
    }

    #[test]
    fn curl_pipe_bash_needs_confirm_in_balanced() {
        let engine = default_engine(SafetyMode::Balanced);
        let (_, decision) =
            engine.evaluate(&["curl", "http://example.com/install.sh", "|", "bash"]);
        assert_eq!(
            decision,
            Decision::Confirm,
            "balanced should confirm curl|bash"
        );
    }

    #[test]
    fn curl_pipe_bash_needs_confirm_in_loose() {
        let engine = default_engine(SafetyMode::Loose);
        let (assessment, decision) =
            engine.evaluate(&["curl", "http://example.com/install.sh", "|", "bash"]);
        assert_eq!(
            decision,
            Decision::Confirm,
            "loose should confirm curl|bash; score={}",
            assessment.score
        );
    }

    #[test]
    fn wget_pipe_sh_is_high_risk() {
        let engine = default_engine(SafetyMode::Balanced);
        let (assessment, decision) =
            engine.evaluate(&["wget", "-qO-", "http://example.com/setup.sh", "|", "sh"]);
        assert!(
            decision == Decision::Confirm || decision == Decision::Deny,
            "wget|sh should not be allowed; decision={decision:?}, score={}",
            assessment.score
        );
        assert!(assessment.score >= 51);
    }

    // ── Golden test: sudo rm (privilege elevation) ────────────────────────────

    #[test]
    fn sudo_rm_needs_confirm_in_strict() {
        let engine = default_engine(SafetyMode::Strict);
        let (assessment, decision) = engine.evaluate(&["sudo", "rm", "/tmp/x"]);
        assert_eq!(
            decision,
            Decision::Confirm,
            "strict should confirm sudo rm; score={}",
            assessment.score
        );
        assert!(assessment
            .categories
            .contains(&RiskCategory::PrivilegeElevation));
    }

    #[test]
    fn sudo_rm_needs_confirm_in_balanced() {
        let engine = default_engine(SafetyMode::Balanced);
        let (_, decision) = engine.evaluate(&["sudo", "rm", "/tmp/x"]);
        assert_eq!(decision, Decision::Confirm);
    }

    #[test]
    fn sudo_rm_is_allowed_in_loose() {
        let engine = default_engine(SafetyMode::Loose);
        let (assessment, decision) = engine.evaluate(&["sudo", "rm", "/tmp/x"]);
        assert_eq!(
            decision,
            Decision::Allow,
            "loose should allow sudo rm (score={})",
            assessment.score
        );
    }

    #[test]
    fn sudo_rm_rf_is_high_risk() {
        // sudo -r and rm -rf together should reach High/Critical risk
        let engine = default_engine(SafetyMode::Strict);
        let (assessment, decision) = engine.evaluate(&["sudo", "rm", "-rf", "/tmp/x"]);
        assert!(
            decision == Decision::Deny || decision == Decision::Confirm,
            "sudo rm -rf should be Deny or Confirm in strict; got {decision:?}, score={}",
            assessment.score
        );
        assert!(assessment.score >= 30);
    }

    // ── Golden test: allowlisted patterns ────────────────────────────────────

    #[test]
    fn allowlisted_command_bypasses_all_checks_strict() {
        let engine = allow_engine(SafetyMode::Strict, vec!["git status"]);
        let (assessment, decision) = engine.evaluate(&["git", "status"]);
        assert_eq!(decision, Decision::Allow);
        assert_eq!(assessment.level, RiskLevel::None);
        assert!(assessment.reasons[0].contains("allowlisted"));
    }

    #[test]
    fn allowlisted_command_bypasses_all_modes() {
        for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
            let engine = allow_engine(mode, vec!["git "]);
            let (_, decision) = engine.evaluate(&["git", "push"]);
            assert_eq!(decision, Decision::Allow);
        }
    }

    #[test]
    fn non_allowlisted_command_still_evaluated() {
        let engine = allow_engine(SafetyMode::Strict, vec!["ls"]);
        let (_, decision) = engine.evaluate(&["rm", "-rf", "/"]);
        // deny prefix wins even when allow prefixes are configured
        assert_eq!(decision, Decision::Deny);
    }

    // ── Deny wins over allow (FR-33) ──────────────────────────────────────────

    #[test]
    fn deny_wins_over_allow_prefix() {
        // deny_prefix "rm -rf /" and allow_prefix "rm -rf /" — deny must win
        let mut cfg = SafetyConfig::default();
        cfg.deny_prefixes = vec!["rm -rf /".into()];
        cfg.allow_prefixes = vec!["rm -rf /".into()];
        let engine = SafetyPolicyEngine::new(SafetyMode::Balanced, &cfg);

        let (_, decision) = engine.evaluate(&["rm", "-rf", "/"]);
        assert_eq!(
            decision,
            Decision::Deny,
            "deny must win over allow per FR-33"
        );
    }

    // ── Custom deny prefixes ──────────────────────────────────────────────────

    #[test]
    fn custom_deny_prefix_blocks_command() {
        let engine = custom_engine(SafetyMode::Balanced, vec!["halt"], vec![]);
        let (assessment, decision) = engine.evaluate(&["halt"]);
        assert_eq!(decision, Decision::Deny);
        assert_eq!(assessment.level, RiskLevel::Critical);
        assert_eq!(assessment.score, 100);
    }

    #[test]
    fn custom_deny_prefix_partial_match_blocks() {
        let engine = custom_engine(SafetyMode::Balanced, vec!["dangerous-cmd"], vec![]);
        let (_, decision) = engine.evaluate(&["dangerous-cmd", "--all"]);
        assert_eq!(decision, Decision::Deny);
    }

    #[test]
    fn custom_deny_prefix_no_match_passes_through() {
        let engine = custom_engine(SafetyMode::Balanced, vec!["halt"], vec![]);
        let (_, decision) = engine.evaluate(&["uptime"]);
        assert_eq!(decision, Decision::Allow);
    }

    // ── Invalid regex in patterns (skipped gracefully) ────────────────────────

    #[test]
    fn invalid_regex_in_patterns_skipped() {
        let mut cfg = SafetyConfig::default();
        cfg.high_risk_patterns = vec!["[invalid-regex(".into()];
        let engine = SafetyPolicyEngine::new(SafetyMode::Balanced, &cfg);
        // Engine should construct without panic; evaluate should return Allow for ls
        let (_, decision) = engine.evaluate(&["ls"]);
        assert_eq!(decision, Decision::Allow);
    }

    #[test]
    fn mix_valid_and_invalid_patterns() {
        let mut cfg = SafetyConfig::default();
        cfg.high_risk_patterns = vec![
            "[bad-regex(".into(),
            r"sudo\s+".into(), // valid
        ];
        let engine = SafetyPolicyEngine::new(SafetyMode::Balanced, &cfg);
        let (assessment, _) = engine.evaluate(&["sudo", "ls"]);
        // The valid pattern should still contribute to the score
        assert!(assessment.score > 0);
    }

    // ── Score accumulation and cap ────────────────────────────────────────────

    #[test]
    fn multiple_patterns_accumulate_score() {
        // sudo rm -rf would match both sudo pattern and rm pattern
        let engine = default_engine(SafetyMode::Balanced);
        let (sudo_only, _) = engine.evaluate(&["sudo", "ls"]);
        let (both, _) = engine.evaluate(&["sudo", "rm", "-rf", "/tmp"]);
        // double pattern hit → higher score (rm -rf matches rm\s+-[rf]*r via substring)
        assert!(
            both.score > sudo_only.score,
            "sudo rm -rf should score higher than sudo ls"
        );
    }

    #[test]
    fn score_is_capped_at_100() {
        let mut cfg = SafetyConfig::default();
        // Add many high-score patterns to overflow
        cfg.high_risk_patterns = vec![
            r"sudo\s+".into(),
            r"rm\s+-[rf]*r".into(),
            r"curl.*\|\s*bash".into(),
            r"wget.*\|\s*sh".into(),
            r"\btrue\b".into(), // matches everything containing "true"
        ];
        let engine = SafetyPolicyEngine::new(SafetyMode::Balanced, &cfg);
        let cmd = "sudo rm -rf /tmp | curl http://x.com | bash | wget | sh true";
        let argv: Vec<&str> = cmd.split_whitespace().collect();
        let (assessment, _) = engine.evaluate(&argv);
        assert!(
            assessment.score <= 100,
            "score must not exceed 100; got {}",
            assessment.score
        );
    }

    // ── Mode-specific decisions ───────────────────────────────────────────────

    #[test]
    fn strict_mode_medium_risk_confirms() {
        // sudo alone → ~30 score → Medium → Confirm in strict
        let engine = default_engine(SafetyMode::Strict);
        let (assessment, decision) = engine.evaluate(&["sudo", "echo", "hi"]);
        assert_eq!(assessment.level, RiskLevel::Medium);
        assert_eq!(decision, Decision::Confirm);
    }

    #[test]
    fn balanced_mode_medium_risk_confirms() {
        let engine = default_engine(SafetyMode::Balanced);
        let (_, decision) = engine.evaluate(&["sudo", "echo", "hi"]);
        assert_eq!(decision, Decision::Confirm);
    }

    #[test]
    fn loose_mode_medium_risk_allows() {
        let engine = default_engine(SafetyMode::Loose);
        let (assessment, decision) = engine.evaluate(&["sudo", "echo", "hi"]);
        assert_eq!(assessment.level, RiskLevel::Medium);
        assert_eq!(decision, Decision::Allow);
    }

    // ── Reason and category content ───────────────────────────────────────────

    #[test]
    fn pattern_match_reason_includes_pattern_source() {
        let engine = default_engine(SafetyMode::Balanced);
        let (assessment, _) = engine.evaluate(&["sudo", "ls"]);
        assert!(
            assessment.reasons.iter().any(|r| r.contains("sudo")),
            "reason should mention the pattern; reasons={:?}",
            assessment.reasons
        );
    }

    #[test]
    fn deny_reason_mentions_prefix() {
        let engine = default_engine(SafetyMode::Balanced);
        let (assessment, _) = engine.evaluate(&["rm", "-rf", "/"]);
        assert!(
            assessment.reasons[0].contains("rm -rf /"),
            "deny reason should name the prefix; got: {:?}",
            assessment.reasons[0]
        );
    }

    #[test]
    fn allow_reason_mentions_prefix() {
        let engine = allow_engine(SafetyMode::Strict, vec!["git "]);
        let (assessment, _) = engine.evaluate(&["git", "pull"]);
        assert!(assessment.reasons[0].contains("allowlisted"));
        assert!(assessment.reasons[0].contains("git "));
    }

    #[test]
    fn sudo_pattern_match_sets_privilege_category() {
        let engine = default_engine(SafetyMode::Balanced);
        let (assessment, _) = engine.evaluate(&["sudo", "echo"]);
        assert!(assessment
            .categories
            .contains(&RiskCategory::PrivilegeElevation));
    }

    #[test]
    fn rm_pattern_match_sets_destructive_category() {
        // "rm -r /tmp/x" matches rm pattern
        let engine = default_engine(SafetyMode::Balanced);
        let (assessment, _) = engine.evaluate(&["rm", "-r", "/tmp/x"]);
        assert!(assessment
            .categories
            .contains(&RiskCategory::DestructiveFilesystem));
    }

    #[test]
    fn curl_bash_pattern_sets_network_category() {
        let engine = default_engine(SafetyMode::Balanced);
        let (assessment, _) = engine.evaluate(&["curl", "http://x.com", "|", "bash"]);
        assert!(assessment.categories.contains(&RiskCategory::Network));
    }

    // ── No-deny-prefix clear ──────────────────────────────────────────────────

    #[test]
    fn rm_without_recursive_flag_not_denied() {
        // "rm /tmp/file" should NOT match deny prefix "rm -rf /"
        let engine = default_engine(SafetyMode::Balanced);
        let (_, decision) = engine.evaluate(&["rm", "/tmp/file"]);
        // Not denied by prefix — may still trigger pattern (rm -r pattern won't match
        // because there's no -r flag), so should be Allow
        assert_eq!(decision, Decision::Allow);
    }

    #[test]
    fn git_command_is_safe() {
        let engine = default_engine(SafetyMode::Strict);
        let (assessment, decision) = engine.evaluate(&["git", "status"]);
        assert_eq!(decision, Decision::Allow);
        assert_eq!(assessment.level, RiskLevel::None);
    }

    #[test]
    fn echo_command_is_safe() {
        let engine = default_engine(SafetyMode::Strict);
        let (_, decision) = engine.evaluate(&["echo", "hello"]);
        assert_eq!(decision, Decision::Allow);
    }

    #[test]
    fn pwd_command_is_safe() {
        let engine = default_engine(SafetyMode::Strict);
        let (_, decision) = engine.evaluate(&["pwd"]);
        assert_eq!(decision, Decision::Allow);
    }

    // ── Empty config (no patterns, no prefixes) ───────────────────────────────

    #[test]
    fn engine_with_empty_config_allows_everything() {
        let empty = SafetyConfig {
            deny_prefixes: vec![],
            allow_prefixes: vec![],
            high_risk_patterns: vec![],
        };
        for mode in [SafetyMode::Strict, SafetyMode::Balanced, SafetyMode::Loose] {
            let engine = SafetyPolicyEngine::new(mode, &empty);
            let (_, decision) = engine.evaluate(&["rm", "-rf", "/"]);
            assert_eq!(decision, Decision::Allow, "empty config allows everything");
        }
    }

    // ── Sudo heuristic: first argv ────────────────────────────────────────────

    #[test]
    fn sudo_as_first_argv_raises_risk() {
        let engine = default_engine(SafetyMode::Balanced);
        let (no_sudo, _) = engine.evaluate(&["ls"]);
        let (with_sudo, _) = engine.evaluate(&["sudo", "ls"]);
        assert!(with_sudo.score > no_sudo.score, "sudo should raise score");
    }

    // ── Deny prefix with categories ───────────────────────────────────────────

    #[test]
    fn deny_prefix_result_has_one_category() {
        let engine = default_engine(SafetyMode::Balanced);
        let (assessment, decision) = engine.evaluate(&["rm", "-rf", "/"]);
        assert_eq!(decision, Decision::Deny);
        assert_eq!(assessment.categories.len(), 1);
    }

    // ── Loose mode high-risk reaches Confirm ─────────────────────────────────

    #[test]
    fn loose_mode_high_risk_confirms_not_denies() {
        let engine = default_engine(SafetyMode::Loose);
        // curl|bash is High risk (55) — in loose: High → Confirm
        let (assessment, decision) = engine.evaluate(&["curl", "http://x.com", "|", "bash"]);
        assert_eq!(assessment.level, RiskLevel::High);
        assert_eq!(decision, Decision::Confirm);
    }

    // ── Strict mode high-risk denies ──────────────────────────────────────────

    #[test]
    fn strict_mode_high_risk_denies() {
        let engine = default_engine(SafetyMode::Strict);
        let (assessment, decision) = engine.evaluate(&["curl", "http://x.com", "|", "bash"]);
        assert_eq!(assessment.level, RiskLevel::High);
        assert_eq!(decision, Decision::Deny);
    }

    // ── Multiple allow prefixes ───────────────────────────────────────────────

    #[test]
    fn first_matching_allow_prefix_wins() {
        let mut cfg = SafetyConfig::default();
        cfg.allow_prefixes = vec!["git ".into(), "ls".into()];
        let engine = SafetyPolicyEngine::new(SafetyMode::Strict, &cfg);
        let (_, decision) = engine.evaluate(&["ls", "-la"]);
        assert_eq!(decision, Decision::Allow);
    }
}
