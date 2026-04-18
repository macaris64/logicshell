// Configuration discovery — LOGICSHELL_OPERATIONS.md §Configuration discovery

use std::path::{Path, PathBuf};

use crate::{
    config::{load, Config},
    LogicShellError, Result,
};

/// Discover and load configuration using the standard search order.
///
/// Search order (first match wins, no merging):
/// 1. `LOGICSHELL_CONFIG` env var — must be an absolute path
/// 2. Walk up from `cwd` for `.logicshell.toml`
/// 3. `$XDG_CONFIG_HOME/logicshell/config.toml`
/// 4. `$XDG_CONFIG_HOME/logicshell/.logicshell.toml` (legacy; loses to `config.toml`)
/// 5. Built-in defaults when no file is found
pub fn discover(cwd: &Path) -> Result<Config> {
    let env_override = std::env::var("LOGICSHELL_CONFIG").ok();
    let xdg_home = std::env::var("XDG_CONFIG_HOME").ok();
    let home = std::env::var("HOME").ok();
    find_and_load(
        env_override.as_deref(),
        cwd,
        xdg_home.as_deref(),
        home.as_deref(),
    )
}

/// Return the resolved config file path without loading it, or `None` if defaults apply.
pub fn find_config_path(cwd: &Path) -> Result<Option<PathBuf>> {
    let env_override = std::env::var("LOGICSHELL_CONFIG").ok();
    let xdg_home = std::env::var("XDG_CONFIG_HOME").ok();
    let home = std::env::var("HOME").ok();
    find_path_impl(
        env_override.as_deref(),
        cwd,
        xdg_home.as_deref(),
        home.as_deref(),
    )
}

/// Testable inner loader — accepts explicit env values instead of reading the process env.
pub(crate) fn find_and_load(
    logicshell_config: Option<&str>,
    cwd: &Path,
    xdg_config_home: Option<&str>,
    home: Option<&str>,
) -> Result<Config> {
    match find_path_impl(logicshell_config, cwd, xdg_config_home, home)? {
        Some(path) => {
            let toml = std::fs::read_to_string(&path).map_err(LogicShellError::Io)?;
            load(&toml)
        }
        None => Ok(Config::default()),
    }
}

/// Testable inner path resolver — accepts explicit env values.
pub(crate) fn find_path_impl(
    logicshell_config: Option<&str>,
    cwd: &Path,
    xdg_config_home: Option<&str>,
    home: Option<&str>,
) -> Result<Option<PathBuf>> {
    // 1. LOGICSHELL_CONFIG env var
    if let Some(val) = logicshell_config {
        let p = PathBuf::from(val);
        if p.is_absolute() {
            return Ok(Some(p));
        }
        return Err(LogicShellError::Config(format!(
            "LOGICSHELL_CONFIG must be an absolute path, got: {val}"
        )));
    }

    // 2. Walk up from cwd
    if let Some(p) = walk_up(cwd) {
        return Ok(Some(p));
    }

    // 3 & 4. XDG config dir (config.toml wins over legacy .logicshell.toml)
    if let Some(xdg_dir) = resolve_xdg_config_dir(xdg_config_home, home) {
        let config_toml = xdg_dir.join("logicshell").join("config.toml");
        if config_toml.exists() {
            return Ok(Some(config_toml));
        }
        let legacy = xdg_dir.join("logicshell").join(".logicshell.toml");
        if legacy.exists() {
            return Ok(Some(legacy));
        }
    }

    // 5. No file found — caller uses built-in defaults
    Ok(None)
}

fn walk_up(start: &Path) -> Option<PathBuf> {
    let mut dir: &Path = start;
    loop {
        let candidate = dir.join(".logicshell.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return None,
        }
    }
}

fn resolve_xdg_config_dir(xdg_config_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    if let Some(xdg) = xdg_config_home {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg));
        }
    }
    home.map(|h| PathBuf::from(h).join(".config"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const MINIMAL_TOML: &str = "schema_version = 1\n";
    const CUSTOM_TOML: &str = "schema_version = 2\nsafety_mode = \"strict\"\n";

    fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, content).unwrap();
        p
    }

    // ── LOGICSHELL_CONFIG env var ─────────────────────────────────────────────

    #[test]
    fn env_var_absolute_path_wins() {
        let tmp = TempDir::new().unwrap();
        let cfg_file = write(tmp.path(), "my.toml", CUSTOM_TOML);
        let result =
            find_path_impl(Some(cfg_file.to_str().unwrap()), tmp.path(), None, None).unwrap();
        assert_eq!(result, Some(cfg_file));
    }

    #[test]
    fn env_var_wins_over_walk_up() {
        let tmp = TempDir::new().unwrap();
        // Walk-up file also present
        write(tmp.path(), ".logicshell.toml", MINIMAL_TOML);
        let cfg_file = write(tmp.path(), "override.toml", CUSTOM_TOML);

        let result =
            find_path_impl(Some(cfg_file.to_str().unwrap()), tmp.path(), None, None).unwrap();
        assert_eq!(result, Some(cfg_file));
    }

    #[test]
    fn env_var_relative_path_is_error() {
        let tmp = TempDir::new().unwrap();
        let result = find_path_impl(Some("relative/path.toml"), tmp.path(), None, None);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("absolute path"));
    }

    // ── walk-up ───────────────────────────────────────────────────────────────

    #[test]
    fn walk_up_finds_dotfile_in_cwd() {
        let tmp = TempDir::new().unwrap();
        let f = write(tmp.path(), ".logicshell.toml", MINIMAL_TOML);
        let result = find_path_impl(None, tmp.path(), None, None).unwrap();
        assert_eq!(result, Some(f));
    }

    #[test]
    fn walk_up_finds_dotfile_in_parent() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".logicshell.toml", MINIMAL_TOML);
        let child = tmp.path().join("a").join("b");
        fs::create_dir_all(&child).unwrap();

        let result = find_path_impl(None, &child, None, None).unwrap();
        assert_eq!(result, Some(tmp.path().join(".logicshell.toml")));
    }

    #[test]
    fn walk_up_prefers_closer_ancestor() {
        let tmp = TempDir::new().unwrap();
        // root-level file
        write(tmp.path(), ".logicshell.toml", MINIMAL_TOML);
        // child-level file (closer)
        let child = tmp.path().join("sub");
        fs::create_dir_all(&child).unwrap();
        let closer = write(&child, ".logicshell.toml", CUSTOM_TOML);

        let result = find_path_impl(None, &child, None, None).unwrap();
        assert_eq!(result, Some(closer));
    }

    #[test]
    fn walk_up_no_file_falls_through() {
        let tmp = TempDir::new().unwrap();
        // No .logicshell.toml anywhere under tmp; also no XDG/HOME set
        let result = find_path_impl(None, tmp.path(), None, None).unwrap();
        assert_eq!(result, None);
    }

    // ── XDG config.toml ──────────────────────────────────────────────────────

    #[test]
    fn xdg_config_toml_found() {
        let tmp = TempDir::new().unwrap();
        let xdg_dir = tmp.path().join("xdg");
        let ls_dir = xdg_dir.join("logicshell");
        fs::create_dir_all(&ls_dir).unwrap();
        write(&ls_dir, "config.toml", MINIMAL_TOML);

        let result =
            find_path_impl(None, tmp.path(), Some(xdg_dir.to_str().unwrap()), None).unwrap();
        assert_eq!(result, Some(ls_dir.join("config.toml")));
    }

    #[test]
    fn xdg_legacy_dotfile_found_when_no_config_toml() {
        let tmp = TempDir::new().unwrap();
        let xdg_dir = tmp.path().join("xdg");
        let ls_dir = xdg_dir.join("logicshell");
        fs::create_dir_all(&ls_dir).unwrap();
        write(&ls_dir, ".logicshell.toml", MINIMAL_TOML);

        let result =
            find_path_impl(None, tmp.path(), Some(xdg_dir.to_str().unwrap()), None).unwrap();
        assert_eq!(result, Some(ls_dir.join(".logicshell.toml")));
    }

    #[test]
    fn xdg_config_toml_wins_over_legacy() {
        let tmp = TempDir::new().unwrap();
        let xdg_dir = tmp.path().join("xdg");
        let ls_dir = xdg_dir.join("logicshell");
        fs::create_dir_all(&ls_dir).unwrap();
        write(&ls_dir, "config.toml", MINIMAL_TOML);
        write(&ls_dir, ".logicshell.toml", CUSTOM_TOML);

        let result =
            find_path_impl(None, tmp.path(), Some(xdg_dir.to_str().unwrap()), None).unwrap();
        assert_eq!(result, Some(ls_dir.join("config.toml")));
    }

    // ── HOME fallback ─────────────────────────────────────────────────────────

    #[test]
    fn home_dir_fallback_when_xdg_unset() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join(".config").join("logicshell");
        fs::create_dir_all(&config_dir).unwrap();
        write(&config_dir, "config.toml", MINIMAL_TOML);

        let result =
            find_path_impl(None, tmp.path(), None, Some(tmp.path().to_str().unwrap())).unwrap();
        assert_eq!(result, Some(config_dir.join("config.toml")));
    }

    #[test]
    fn empty_xdg_falls_back_to_home() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join(".config").join("logicshell");
        fs::create_dir_all(&config_dir).unwrap();
        write(&config_dir, "config.toml", MINIMAL_TOML);

        // XDG_CONFIG_HOME is empty string → treat as unset
        let result = find_path_impl(
            None,
            tmp.path(),
            Some(""),
            Some(tmp.path().to_str().unwrap()),
        )
        .unwrap();
        assert_eq!(result, Some(config_dir.join("config.toml")));
    }

    // ── precedence chain ──────────────────────────────────────────────────────

    #[test]
    fn walk_up_wins_over_xdg() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".logicshell.toml", MINIMAL_TOML);

        let xdg_dir = tmp.path().join("xdg");
        let ls_dir = xdg_dir.join("logicshell");
        fs::create_dir_all(&ls_dir).unwrap();
        write(&ls_dir, "config.toml", CUSTOM_TOML);

        let result =
            find_path_impl(None, tmp.path(), Some(xdg_dir.to_str().unwrap()), None).unwrap();
        assert_eq!(result, Some(tmp.path().join(".logicshell.toml")));
    }

    #[test]
    fn no_file_anywhere_returns_none() {
        let tmp = TempDir::new().unwrap();
        let result = find_path_impl(None, tmp.path(), None, None).unwrap();
        assert_eq!(result, None);
    }

    // ── find_and_load ─────────────────────────────────────────────────────────

    #[test]
    fn missing_file_uses_defaults() {
        let tmp = TempDir::new().unwrap();
        let cfg = find_and_load(None, tmp.path(), None, None).unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn loads_file_values_correctly() {
        let tmp = TempDir::new().unwrap();
        let f = write(tmp.path(), ".logicshell.toml", CUSTOM_TOML);
        let cfg = find_and_load(Some(f.to_str().unwrap()), tmp.path(), None, None).unwrap();
        assert_eq!(cfg.schema_version, 2);
        assert_eq!(cfg.safety_mode, crate::config::SafetyMode::Strict);
    }

    #[test]
    fn missing_env_var_file_returns_io_error() {
        let result = find_and_load(
            Some("/nonexistent/path/cfg.toml"),
            Path::new("/"),
            None,
            None,
        );
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), LogicShellError::Io(_)));
    }

    #[test]
    fn malformed_toml_in_file_returns_config_error() {
        let tmp = TempDir::new().unwrap();
        let f = write(tmp.path(), ".logicshell.toml", "= not valid toml ===");
        let result = find_and_load(None, tmp.path(), None, None);
        assert!(result.is_err());
        drop(f);
    }

    // ── public API surface (covers env-reading lines) ─────────────────────────

    #[test]
    fn discover_loads_dotfile_via_walk_up() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), ".logicshell.toml", CUSTOM_TOML);
        // Walk-up finds the file before any env-based path; result depends on whether
        // LOGICSHELL_CONFIG is set in the process env, so we only assert the strong
        // case when it is not set.
        if std::env::var("LOGICSHELL_CONFIG").is_err() {
            let cfg = discover(tmp.path()).unwrap();
            assert_eq!(cfg.schema_version, 2);
        } else {
            // LOGICSHELL_CONFIG is set — just verify the function runs without panic.
            let _ = discover(tmp.path());
        }
    }

    #[test]
    fn find_config_path_returns_dotfile_via_walk_up() {
        let tmp = TempDir::new().unwrap();
        let f = write(tmp.path(), ".logicshell.toml", MINIMAL_TOML);
        if std::env::var("LOGICSHELL_CONFIG").is_err() {
            let path = find_config_path(tmp.path()).unwrap();
            assert_eq!(path, Some(f));
        } else {
            let _ = find_config_path(tmp.path());
        }
    }
}
