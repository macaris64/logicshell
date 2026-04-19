use std::collections::VecDeque;
use std::path::PathBuf;

const DEFAULT_CAP: usize = 1_000;

/// Persistent command history with in-memory ring buffer and arrow-key navigation.
///
/// Entries are stored oldest-first (`entries[0]` = oldest, `entries[len-1]` = newest).
/// Navigation with Up/Down arrows walks this list from newest → oldest and back.
#[derive(Debug, Clone)]
pub struct HistoryStore {
    entries: VecDeque<String>,
    cap: usize,
    path: PathBuf,
    /// `None` = live input; `Some(i)` = currently showing `entries[i]`.
    pub nav_index: Option<usize>,
    /// Input saved when navigation started, restored on Down past the newest entry.
    saved_input: String,
}

impl HistoryStore {
    /// Create an empty store that will persist to `path` with the default 1 000-entry cap.
    pub fn new(path: PathBuf) -> Self {
        Self::with_cap(path, DEFAULT_CAP)
    }

    /// Create an empty store with a custom cap.
    pub fn with_cap(path: PathBuf, cap: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            cap,
            path,
            nav_index: None,
            saved_input: String::new(),
        }
    }

    // ── read accessors ────────────────────────────────────────────────────────

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &VecDeque<String> {
        &self.entries
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    // ── mutation ──────────────────────────────────────────────────────────────

    /// Append `entry` to history, enforcing the cap and deduplication of consecutive
    /// identical entries.  Resets navigation state.
    pub fn push(&mut self, entry: String) {
        if entry.is_empty() {
            return;
        }
        // Skip consecutive duplicates.
        if self.entries.back().map(|e| e == &entry).unwrap_or(false) {
            self.reset_navigation();
            return;
        }
        self.entries.push_back(entry);
        while self.entries.len() > self.cap {
            self.entries.pop_front();
        }
        self.reset_navigation();
    }

    /// Reset navigation state without modifying entries.
    pub fn reset_navigation(&mut self) {
        self.nav_index = None;
        self.saved_input.clear();
    }

    // ── navigation ────────────────────────────────────────────────────────────

    /// Move to the previous (older) history entry.
    ///
    /// If called while not navigating, saves `current_input` and jumps to the
    /// newest entry.  Returns `Some(entry)` when a new entry is available,
    /// `None` when already at the oldest entry.
    pub fn navigate_prev(&mut self, current_input: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        match self.nav_index {
            None => {
                // Start navigation: save live input, jump to newest.
                self.saved_input = current_input.to_string();
                let idx = self.entries.len() - 1;
                self.nav_index = Some(idx);
                Some(self.entries[idx].clone())
            }
            Some(0) => {
                // Already at oldest — can't go further back.
                None
            }
            Some(i) => {
                let idx = i - 1;
                self.nav_index = Some(idx);
                Some(self.entries[idx].clone())
            }
        }
    }

    /// Move to the next (newer) history entry.
    ///
    /// When moving past the newest entry, restores the saved live input.
    /// Returns `None` when already at live input.
    pub fn navigate_next(&mut self) -> Option<String> {
        match self.nav_index {
            None => None,
            Some(i) if i + 1 < self.entries.len() => {
                let idx = i + 1;
                self.nav_index = Some(idx);
                Some(self.entries[idx].clone())
            }
            Some(_) => {
                // Past the newest entry — restore live input.
                let restored = self.saved_input.clone();
                self.nav_index = None;
                self.saved_input.clear();
                Some(restored)
            }
        }
    }

    // ── persistence ───────────────────────────────────────────────────────────

    /// Write all entries to `self.path`, one per line (oldest first).
    /// Creates parent directories if they don't exist.
    pub fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = self.entries.iter().cloned().collect::<Vec<_>>().join("\n");
        std::fs::write(&self.path, content)?;
        Ok(())
    }

    /// Load history from `path` with the default cap.  Returns an empty store
    /// if the file does not exist.
    pub fn load(path: PathBuf) -> std::io::Result<Self> {
        Self::load_with_cap(path, DEFAULT_CAP)
    }

    /// Load history from `path` with a custom cap.
    pub fn load_with_cap(path: PathBuf, cap: usize) -> std::io::Result<Self> {
        let mut store = Self::with_cap(path, cap);
        if store.path.exists() {
            let content = std::fs::read_to_string(&store.path)?;
            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    store.entries.push_back(trimmed.to_string());
                }
            }
            // Enforce cap on load (keep most recent).
            while store.entries.len() > store.cap {
                store.entries.pop_front();
            }
        }
        Ok(store)
    }
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn tmp_path() -> PathBuf {
        tempdir().unwrap().keep().join("history")
    }

    fn store() -> HistoryStore {
        HistoryStore::new(tmp_path())
    }

    // ── construction ──────────────────────────────────────────────────────────

    #[test]
    fn new_store_is_empty() {
        let s = store();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn with_cap_sets_custom_cap() {
        let s = HistoryStore::with_cap(tmp_path(), 5);
        assert_eq!(s.cap, 5);
    }

    // ── push ──────────────────────────────────────────────────────────────────

    #[test]
    fn push_adds_entry() {
        let mut s = store();
        s.push("ls".to_string());
        assert_eq!(s.len(), 1);
        assert_eq!(s.entries()[0], "ls");
    }

    #[test]
    fn push_empty_string_is_noop() {
        let mut s = store();
        s.push(String::new());
        assert!(s.is_empty());
    }

    #[test]
    fn push_consecutive_duplicate_is_skipped() {
        let mut s = store();
        s.push("ls".to_string());
        s.push("ls".to_string());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn push_non_consecutive_duplicate_is_kept() {
        let mut s = store();
        s.push("ls".to_string());
        s.push("pwd".to_string());
        s.push("ls".to_string());
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn push_enforces_cap() {
        let mut s = HistoryStore::with_cap(tmp_path(), 3);
        s.push("a".to_string());
        s.push("b".to_string());
        s.push("c".to_string());
        s.push("d".to_string()); // should evict "a"
        assert_eq!(s.len(), 3);
        let vals: Vec<_> = s.entries().iter().cloned().collect();
        assert_eq!(vals, vec!["b", "c", "d"]);
    }

    #[test]
    fn push_resets_navigation() {
        let mut s = store();
        s.push("ls".to_string());
        s.push("pwd".to_string());
        s.navigate_prev(""); // start navigating
        assert!(s.nav_index.is_some());
        s.push("whoami".to_string());
        assert!(s.nav_index.is_none());
    }

    // ── navigate_prev (Up arrow) ──────────────────────────────────────────────

    #[test]
    fn navigate_prev_empty_store_returns_none() {
        let mut s = store();
        assert_eq!(s.navigate_prev("current"), None);
    }

    #[test]
    fn navigate_prev_starts_at_newest_entry() {
        let mut s = store();
        s.push("ls".to_string());
        s.push("pwd".to_string());
        let entry = s.navigate_prev("live");
        assert_eq!(entry, Some("pwd".to_string()));
    }

    #[test]
    fn navigate_prev_saves_live_input() {
        let mut s = store();
        s.push("ls".to_string());
        s.navigate_prev("partial input");
        assert_eq!(s.saved_input, "partial input");
    }

    #[test]
    fn navigate_prev_walks_toward_oldest() {
        let mut s = store();
        s.push("cmd1".to_string());
        s.push("cmd2".to_string());
        s.push("cmd3".to_string());
        assert_eq!(s.navigate_prev(""), Some("cmd3".to_string())); // newest
        assert_eq!(s.navigate_prev(""), Some("cmd2".to_string()));
        assert_eq!(s.navigate_prev(""), Some("cmd1".to_string())); // oldest
        assert_eq!(s.navigate_prev(""), None); // already at oldest
    }

    #[test]
    fn navigate_prev_at_oldest_returns_none() {
        let mut s = store();
        s.push("only".to_string());
        s.navigate_prev(""); // goes to "only"
        assert_eq!(s.navigate_prev(""), None); // already at oldest
    }

    // ── navigate_next (Down arrow) ────────────────────────────────────────────

    #[test]
    fn navigate_next_when_not_navigating_returns_none() {
        let mut s = store();
        s.push("ls".to_string());
        assert_eq!(s.navigate_next(), None);
    }

    #[test]
    fn navigate_next_walks_toward_newest() {
        let mut s = store();
        s.push("cmd1".to_string());
        s.push("cmd2".to_string());
        s.push("cmd3".to_string());
        s.navigate_prev(""); // → cmd3
        s.navigate_prev(""); // → cmd2
        s.navigate_prev(""); // → cmd1
        assert_eq!(s.navigate_next(), Some("cmd2".to_string()));
        assert_eq!(s.navigate_next(), Some("cmd3".to_string()));
    }

    #[test]
    fn navigate_next_past_newest_restores_saved_input() {
        let mut s = store();
        s.push("ls".to_string());
        s.navigate_prev("typed so far"); // → "ls", saved = "typed so far"
        let restored = s.navigate_next();
        assert_eq!(restored, Some("typed so far".to_string()));
        assert!(s.nav_index.is_none());
    }

    #[test]
    fn navigate_next_after_full_round_trip_returns_none() {
        let mut s = store();
        s.push("ls".to_string());
        s.navigate_prev(""); // → "ls"
        s.navigate_next(); // → restored ""
        assert_eq!(s.navigate_next(), None); // already at live
    }

    // ── reset_navigation ──────────────────────────────────────────────────────

    #[test]
    fn reset_navigation_clears_nav_state() {
        let mut s = store();
        s.push("ls".to_string());
        s.navigate_prev("live");
        s.reset_navigation();
        assert!(s.nav_index.is_none());
        assert!(s.saved_input.is_empty());
    }

    // ── persistence round-trip ────────────────────────────────────────────────

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history");

        let mut s = HistoryStore::new(path.clone());
        s.push("ls".to_string());
        s.push("pwd".to_string());
        s.push("echo hello".to_string());
        s.save().unwrap();

        let loaded = HistoryStore::load(path).unwrap();
        let vals: Vec<_> = loaded.entries().iter().cloned().collect();
        assert_eq!(vals, vec!["ls", "pwd", "echo hello"]);
    }

    #[test]
    fn load_creates_empty_store_when_file_missing() {
        let path = tmp_path();
        let s = HistoryStore::load(path).unwrap();
        assert!(s.is_empty());
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("dirs").join("history");
        let mut s = HistoryStore::new(path.clone());
        s.push("ls".to_string());
        s.save().unwrap();
        assert!(path.exists());
    }

    #[test]
    fn load_respects_cap_by_dropping_oldest() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history");

        // Write 5 entries to file manually.
        std::fs::write(&path, "a\nb\nc\nd\ne").unwrap();

        // Load with cap=3 — should keep c, d, e.
        let s = HistoryStore::load_with_cap(path, 3).unwrap();
        let vals: Vec<_> = s.entries().iter().cloned().collect();
        assert_eq!(vals, vec!["c", "d", "e"]);
    }

    #[test]
    fn save_overwrites_previous_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history");

        let mut s = HistoryStore::new(path.clone());
        s.push("ls".to_string());
        s.save().unwrap();

        s.push("pwd".to_string());
        s.save().unwrap();

        let loaded = HistoryStore::load(path).unwrap();
        assert_eq!(loaded.len(), 2);
    }

    #[test]
    fn load_skips_blank_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history");
        std::fs::write(&path, "ls\n\npwd\n\n").unwrap();

        let s = HistoryStore::load(path).unwrap();
        assert_eq!(s.len(), 2);
    }

    // ── ring-buffer edge cases ────────────────────────────────────────────────

    #[test]
    fn ring_buffer_oldest_dropped_on_overflow() {
        let mut s = HistoryStore::with_cap(tmp_path(), 2);
        s.push("a".to_string());
        s.push("b".to_string());
        s.push("c".to_string());
        let vals: Vec<_> = s.entries().iter().cloned().collect();
        assert_eq!(vals, vec!["b", "c"]);
    }

    #[test]
    fn full_navigation_cycle_with_ring_buffer() {
        let mut s = HistoryStore::with_cap(tmp_path(), 3);
        s.push("a".to_string());
        s.push("b".to_string());
        s.push("c".to_string());
        s.push("d".to_string()); // evicts "a" → ["b", "c", "d"]

        // Navigate: None → d → c → b → None(oldest)
        assert_eq!(s.navigate_prev("live"), Some("d".to_string()));
        assert_eq!(s.navigate_prev("live"), Some("c".to_string()));
        assert_eq!(s.navigate_prev("live"), Some("b".to_string()));
        assert_eq!(s.navigate_prev("live"), None); // oldest
        assert_eq!(s.navigate_next(), Some("c".to_string()));
        assert_eq!(s.navigate_next(), Some("d".to_string()));
        assert_eq!(s.navigate_next(), Some("live".to_string())); // restored
    }
}
