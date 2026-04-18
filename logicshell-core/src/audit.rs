// Audit log sink — §10.2, NFR-07
//
// Writes append-only NDJSON records to a configured file path.
// Each record contains timestamp, cwd, argv, safety decision, and an optional note.
// The internal BufWriter is flushed when AuditSink is dropped so records survive
// process termination without an explicit flush call.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{config::AuditConfig, LogicShellError, Result};

/// Safety policy outcome recorded in the audit log (§10.2, FR-30–33).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditDecision {
    /// Command was allowed by the safety policy.
    Allow,
    /// Command was denied by the safety policy.
    Deny,
    /// Command required (and received) user confirmation.
    Confirm,
}

/// A single record appended to the audit log (§10.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    /// Unix timestamp (seconds) when the record was created.
    pub timestamp_secs: u64,
    /// Working directory at dispatch time.
    pub cwd: String,
    /// Command argv; `argv[0]` is the executable.
    pub argv: Vec<String>,
    /// Safety policy decision for this invocation.
    pub decision: AuditDecision,
    /// Optional human-readable annotation (e.g. "user confirmed", "hook denied").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl AuditRecord {
    /// Create a record with the current Unix timestamp.
    pub fn new(cwd: impl Into<String>, argv: Vec<String>, decision: AuditDecision) -> Self {
        Self {
            timestamp_secs: unix_now(),
            cwd: cwd.into(),
            argv,
            decision,
            note: None,
        }
    }

    /// Attach a human-readable note to this record.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Append-only audit log sink (NFR-07, §10.2).
///
/// Each [`AuditSink::write`] call appends one JSON line terminated by `\n`.
/// The writer is flushed in [`Drop`] so that buffered records reach the OS
/// even if the caller never calls [`AuditSink::flush`] explicitly.
pub struct AuditSink {
    writer: Option<BufWriter<File>>,
}

impl AuditSink {
    /// Create a sink from [`AuditConfig`].
    ///
    /// Returns [`AuditSink::disabled`] when `enabled = false`; otherwise opens
    /// (or creates) the configured path, falling back to the OS temp directory.
    pub fn from_config(config: &AuditConfig) -> Result<Self> {
        if !config.enabled {
            return Ok(Self::disabled());
        }
        let path = config
            .path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(default_path);
        Self::open(&path)
    }

    /// Open (or create + append) a log file at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(LogicShellError::Io)?;
        Ok(Self {
            writer: Some(BufWriter::new(file)),
        })
    }

    /// A no-op sink for when audit logging is disabled.
    pub fn disabled() -> Self {
        Self { writer: None }
    }

    /// Returns `true` when this sink will write records to a file.
    pub fn is_enabled(&self) -> bool {
        self.writer.is_some()
    }

    /// Append `record` as a single JSON line (`\n`-terminated).
    ///
    /// Returns `Ok(())` on a disabled sink (no-op).
    pub fn write(&mut self, record: &AuditRecord) -> Result<()> {
        if let Some(ref mut w) = self.writer {
            let line = serde_json::to_string(record)
                .map_err(|e| LogicShellError::Audit(format!("serialization failed: {e}")))?;
            writeln!(w, "{line}").map_err(LogicShellError::Io)?;
        }
        Ok(())
    }

    /// Flush buffered bytes to the OS.
    ///
    /// Returns `Ok(())` on a disabled sink.
    pub fn flush(&mut self) -> Result<()> {
        if let Some(ref mut w) = self.writer {
            w.flush().map_err(LogicShellError::Io)?;
        }
        Ok(())
    }
}

impl Drop for AuditSink {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

fn default_path() -> PathBuf {
    std::env::temp_dir().join(".logicshell-audit.log")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuditConfig;
    use tempfile::TempDir;

    fn tmp_path(dir: &TempDir, name: &str) -> PathBuf {
        dir.path().join(name)
    }

    fn fixed_record(decision: AuditDecision) -> AuditRecord {
        AuditRecord {
            timestamp_secs: 1_700_000_000,
            cwd: "/home/user".to_string(),
            argv: vec!["ls".to_string(), "-la".to_string()],
            decision,
            note: None,
        }
    }

    // ── AuditDecision ─────────────────────────────────────────────────────────

    #[test]
    fn decision_variants_clone_debug_eq() {
        let all = [
            AuditDecision::Allow,
            AuditDecision::Deny,
            AuditDecision::Confirm,
        ];
        for v in &all {
            let c = v.clone();
            assert_eq!(&c, v);
            assert!(!format!("{v:?}").is_empty());
        }
    }

    #[test]
    fn decision_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&AuditDecision::Allow).unwrap(),
            "\"allow\""
        );
        assert_eq!(
            serde_json::to_string(&AuditDecision::Deny).unwrap(),
            "\"deny\""
        );
        assert_eq!(
            serde_json::to_string(&AuditDecision::Confirm).unwrap(),
            "\"confirm\""
        );
    }

    #[test]
    fn decision_roundtrips_json() {
        for d in [
            AuditDecision::Allow,
            AuditDecision::Deny,
            AuditDecision::Confirm,
        ] {
            let s = serde_json::to_string(&d).unwrap();
            let back: AuditDecision = serde_json::from_str(&s).unwrap();
            assert_eq!(back, d);
        }
    }

    // ── AuditRecord ──────────────────────────────────────────────────────────

    #[test]
    fn record_new_captures_current_timestamp() {
        let before = unix_now();
        let rec = AuditRecord::new("/tmp", vec!["ls".into()], AuditDecision::Allow);
        let after = unix_now();
        assert!(rec.timestamp_secs >= before);
        assert!(rec.timestamp_secs <= after);
    }

    #[test]
    fn record_with_note_sets_field() {
        let rec = AuditRecord::new("/tmp", vec!["ls".into()], AuditDecision::Allow).with_note("ok");
        assert_eq!(rec.note.as_deref(), Some("ok"));
    }

    #[test]
    fn record_without_note_omits_field_in_json() {
        let rec = fixed_record(AuditDecision::Allow);
        let s = serde_json::to_string(&rec).unwrap();
        assert!(!s.contains("note"));
    }

    #[test]
    fn record_clone_is_equal() {
        let rec = fixed_record(AuditDecision::Deny);
        assert_eq!(
            serde_json::to_string(&rec.clone()).unwrap(),
            serde_json::to_string(&rec).unwrap()
        );
    }

    // ── AuditSink: disabled ──────────────────────────────────────────────────

    #[test]
    fn disabled_sink_is_not_enabled() {
        assert!(!AuditSink::disabled().is_enabled());
    }

    #[test]
    fn disabled_write_is_noop_and_ok() {
        let mut sink = AuditSink::disabled();
        assert!(sink.write(&fixed_record(AuditDecision::Allow)).is_ok());
    }

    #[test]
    fn disabled_flush_is_noop_and_ok() {
        let mut sink = AuditSink::disabled();
        assert!(sink.flush().is_ok());
    }

    // ── AuditSink: open ───────────────────────────────────────────────────────

    #[test]
    fn open_creates_file_and_is_enabled() {
        let tmp = TempDir::new().unwrap();
        let path = tmp_path(&tmp, "audit.log");
        let sink = AuditSink::open(&path).unwrap();
        assert!(sink.is_enabled());
    }

    #[test]
    fn open_nonexistent_dir_returns_io_error() {
        let result = AuditSink::open(Path::new("/no/such/dir/audit.log"));
        assert!(matches!(result, Err(LogicShellError::Io(_))));
    }

    // ── write + flush ─────────────────────────────────────────────────────────

    #[test]
    fn write_and_flush_produces_json_line() {
        let tmp = TempDir::new().unwrap();
        let path = tmp_path(&tmp, "audit.log");
        let mut sink = AuditSink::open(&path).unwrap();

        sink.write(&fixed_record(AuditDecision::Allow)).unwrap();
        sink.flush().unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.is_empty());
        let v: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert!(v.is_object());
    }

    #[test]
    fn written_line_contains_all_expected_fields() {
        let tmp = TempDir::new().unwrap();
        let path = tmp_path(&tmp, "audit.log");
        let mut sink = AuditSink::open(&path).unwrap();

        let rec = AuditRecord {
            timestamp_secs: 1_700_000_000,
            cwd: "/home/user".to_string(),
            argv: vec!["rm".to_string(), "-rf".to_string(), "/tmp/x".to_string()],
            decision: AuditDecision::Deny,
            note: Some("denied by policy".to_string()),
        };
        sink.write(&rec).unwrap();
        sink.flush().unwrap();

        let line = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();

        assert_eq!(v["timestamp_secs"], 1_700_000_000u64);
        assert_eq!(v["cwd"], "/home/user");
        assert_eq!(v["decision"], "deny");
        assert_eq!(v["note"], "denied by policy");
        assert_eq!(v["argv"][0], "rm");
    }

    #[test]
    fn multiple_writes_produce_one_line_each() {
        let tmp = TempDir::new().unwrap();
        let path = tmp_path(&tmp, "audit.log");
        let mut sink = AuditSink::open(&path).unwrap();

        for decision in [
            AuditDecision::Allow,
            AuditDecision::Deny,
            AuditDecision::Confirm,
        ] {
            sink.write(&fixed_record(decision)).unwrap();
        }
        sink.flush().unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3);
        for line in &lines {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }
    }

    // ── append semantics ──────────────────────────────────────────────────────

    #[test]
    fn reopening_the_file_appends_rather_than_truncates() {
        let tmp = TempDir::new().unwrap();
        let path = tmp_path(&tmp, "audit.log");

        {
            let mut sink = AuditSink::open(&path).unwrap();
            sink.write(&fixed_record(AuditDecision::Allow)).unwrap();
        } // drop → flush

        {
            let mut sink = AuditSink::open(&path).unwrap();
            sink.write(&fixed_record(AuditDecision::Deny)).unwrap();
        } // drop → flush

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            content.lines().count(),
            2,
            "expected 2 lines after two reopen cycles"
        );
    }

    // ── flush on drop ─────────────────────────────────────────────────────────

    #[test]
    fn drop_flushes_buffered_bytes() {
        let tmp = TempDir::new().unwrap();
        let path = tmp_path(&tmp, "audit.log");

        {
            let mut sink = AuditSink::open(&path).unwrap();
            sink.write(&fixed_record(AuditDecision::Confirm)).unwrap();
            // No explicit flush — Drop must flush.
        }

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.is_empty(), "Drop should have flushed the write");
    }

    // ── from_config ──────────────────────────────────────────────────────────

    #[test]
    fn from_config_disabled_gives_disabled_sink() {
        let cfg = AuditConfig {
            enabled: false,
            path: None,
        };
        let sink = AuditSink::from_config(&cfg).unwrap();
        assert!(!sink.is_enabled());
    }

    #[test]
    fn from_config_enabled_with_explicit_path() {
        let tmp = TempDir::new().unwrap();
        let path = tmp_path(&tmp, "audit.log").to_str().unwrap().to_string();
        let cfg = AuditConfig {
            enabled: true,
            path: Some(path.clone()),
        };
        let sink = AuditSink::from_config(&cfg).unwrap();
        assert!(sink.is_enabled());
        drop(sink);
        assert!(Path::new(&path).exists());
    }

    #[test]
    fn from_config_enabled_no_path_uses_default() {
        let cfg = AuditConfig {
            enabled: true,
            path: None,
        };
        // Default path is under temp dir; just verify it succeeds.
        let sink = AuditSink::from_config(&cfg).unwrap();
        assert!(sink.is_enabled());
    }

    // ── JSON roundtrip ────────────────────────────────────────────────────────

    #[test]
    fn record_json_roundtrip_with_note() {
        let original = AuditRecord {
            timestamp_secs: 9_999,
            cwd: "/srv".to_string(),
            argv: vec!["echo".to_string(), "hi".to_string()],
            decision: AuditDecision::Confirm,
            note: Some("user said yes".to_string()),
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: AuditRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.timestamp_secs, 9_999);
        assert_eq!(parsed.cwd, "/srv");
        assert_eq!(parsed.decision, AuditDecision::Confirm);
        assert_eq!(parsed.note.as_deref(), Some("user said yes"));
    }

    #[test]
    fn record_json_roundtrip_without_note() {
        let original = fixed_record(AuditDecision::Allow);
        let json = serde_json::to_string(&original).unwrap();
        let parsed: AuditRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.decision, AuditDecision::Allow);
        assert!(parsed.note.is_none());
    }
}
