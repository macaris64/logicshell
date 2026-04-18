// System context capture — FR-10, FR-11, LLM Module PRD §5.2
//
// `SystemContextProvider` is the ONLY place in this crate that reads `std::env`
// or the filesystem. Everything downstream (`PromptComposer`, `LlmClient`)
// receives a `SystemContextSnapshot` instead of reading the environment directly.

/// A point-in-time snapshot of OS and shell context used to construct LLM prompts.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemContextSnapshot {
    /// OS family string, e.g. `"linux"`, `"macos"`, `"windows"`.
    pub os_family: String,
    /// CPU architecture, e.g. `"x86_64"`, `"aarch64"`.
    pub arch: String,
    /// Absolute path of the current working directory at snapshot time.
    pub cwd: String,
    /// First up to `MAX_PATH_DIRS` entries from the `PATH` environment variable.
    pub path_dirs: Vec<String>,
}

/// Maximum number of PATH directories included in the context snapshot.
const MAX_PATH_DIRS: usize = 10;

/// Reads OS state to produce a [`SystemContextSnapshot`].
///
/// Callers obtain a snapshot once and pass it to [`PromptComposer`]; the
/// composer itself never accesses `std::env` (FR-11).
///
/// [`PromptComposer`]: crate::prompt::PromptComposer
#[derive(Debug, Default)]
pub struct SystemContextProvider;

impl SystemContextProvider {
    /// Create a new provider.
    pub fn new() -> Self {
        Self
    }

    /// Capture a snapshot of the current OS and shell context.
    pub fn snapshot(&self) -> SystemContextSnapshot {
        SystemContextSnapshot {
            os_family: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "?".to_string()),
            path_dirs: Self::abbreviated_path(),
        }
    }

    /// Returns at most `MAX_PATH_DIRS` non-empty entries from `$PATH`.
    fn abbreviated_path() -> Vec<String> {
        let raw = std::env::var("PATH").unwrap_or_default();
        split_path_str(&raw)
    }
}

/// Pure helper: split a colon-separated PATH string into at most `MAX_PATH_DIRS`
/// non-empty entries. Extracted for deterministic unit testing without env mutation.
pub(crate) fn split_path_str(path_str: &str) -> Vec<String> {
    path_str
        .split(':')
        .filter(|s| !s.is_empty())
        .take(MAX_PATH_DIRS)
        .map(|s| s.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SystemContextSnapshot ─────────────────────────────────────────────────

    #[test]
    fn snapshot_fields_accessible() {
        let s = SystemContextSnapshot {
            os_family: "linux".into(),
            arch: "x86_64".into(),
            cwd: "/home/user".into(),
            path_dirs: vec!["/usr/bin".into(), "/bin".into()],
        };
        assert_eq!(s.os_family, "linux");
        assert_eq!(s.arch, "x86_64");
        assert_eq!(s.cwd, "/home/user");
        assert_eq!(s.path_dirs.len(), 2);
    }

    #[test]
    fn snapshot_clone_eq() {
        let s = SystemContextSnapshot {
            os_family: "linux".into(),
            arch: "x86_64".into(),
            cwd: "/tmp".into(),
            path_dirs: vec![],
        };
        assert_eq!(s.clone(), s);
    }

    #[test]
    fn snapshot_debug() {
        let s = SystemContextSnapshot {
            os_family: "linux".into(),
            arch: "x86_64".into(),
            cwd: "/tmp".into(),
            path_dirs: vec![],
        };
        let d = format!("{s:?}");
        assert!(d.contains("SystemContextSnapshot"));
        assert!(d.contains("linux"));
    }

    #[test]
    fn snapshot_partial_eq() {
        let a = SystemContextSnapshot {
            os_family: "linux".into(),
            arch: "x86_64".into(),
            cwd: "/tmp".into(),
            path_dirs: vec![],
        };
        let b = a.clone();
        let c = SystemContextSnapshot {
            os_family: "macos".into(),
            ..a.clone()
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // ── SystemContextProvider ─────────────────────────────────────────────────

    #[test]
    fn provider_default_works() {
        let _ = SystemContextProvider::default();
    }

    #[test]
    fn provider_new_works() {
        let _ = SystemContextProvider::new();
    }

    #[test]
    fn snapshot_os_family_non_empty() {
        let snap = SystemContextProvider::new().snapshot();
        assert!(!snap.os_family.is_empty(), "os_family must not be empty");
    }

    #[test]
    fn snapshot_arch_non_empty() {
        let snap = SystemContextProvider::new().snapshot();
        assert!(!snap.arch.is_empty(), "arch must not be empty");
    }

    #[test]
    fn snapshot_cwd_non_empty() {
        let snap = SystemContextProvider::new().snapshot();
        assert!(!snap.cwd.is_empty(), "cwd must not be empty");
    }

    #[test]
    fn snapshot_path_dirs_at_most_max() {
        let snap = SystemContextProvider::new().snapshot();
        assert!(
            snap.path_dirs.len() <= MAX_PATH_DIRS,
            "path_dirs exceeds MAX_PATH_DIRS: got {}",
            snap.path_dirs.len()
        );
    }

    #[test]
    fn snapshot_path_dirs_no_empty_entries() {
        let snap = SystemContextProvider::new().snapshot();
        for dir in &snap.path_dirs {
            assert!(
                !dir.is_empty(),
                "path_dirs should not contain empty strings"
            );
        }
    }

    // ── split_path_str (pure, no env mutation needed) ────────────────────────

    #[test]
    fn split_path_str_limits_to_max_dirs() {
        let many: String = (0..=20)
            .map(|i| format!("/dir{i}"))
            .collect::<Vec<_>>()
            .join(":");
        let dirs = split_path_str(&many);
        assert_eq!(dirs.len(), MAX_PATH_DIRS);
    }

    #[test]
    fn split_path_str_with_empty_string_returns_empty() {
        let dirs = split_path_str("");
        assert!(dirs.is_empty());
    }

    #[test]
    fn split_path_str_filters_empty_segments() {
        let dirs = split_path_str("/usr/bin::/bin");
        // The empty segment between two colons should be dropped
        assert!(dirs.iter().all(|d| !d.is_empty()));
        assert!(dirs.contains(&"/usr/bin".to_string()));
        assert!(dirs.contains(&"/bin".to_string()));
    }

    #[test]
    fn split_path_str_single_entry() {
        let dirs = split_path_str("/usr/bin");
        assert_eq!(dirs, vec!["/usr/bin"]);
    }

    #[test]
    fn split_path_str_exactly_max_entries() {
        let exact: String = (0..MAX_PATH_DIRS)
            .map(|i| format!("/dir{i}"))
            .collect::<Vec<_>>()
            .join(":");
        let dirs = split_path_str(&exact);
        assert_eq!(dirs.len(), MAX_PATH_DIRS);
    }

    #[test]
    fn provider_debug() {
        let p = SystemContextProvider::new();
        assert!(format!("{p:?}").contains("SystemContextProvider"));
    }
}
