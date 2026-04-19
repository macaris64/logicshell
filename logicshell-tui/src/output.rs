use std::collections::VecDeque;

pub const DEFAULT_LINE_CAP: usize = 500;

/// Scrollable output panel backed by a ring-buffer of text lines.
///
/// New lines are appended to the back (newest); when the cap is exceeded the
/// oldest front entry is evicted.  Scroll position is tracked as an offset
/// from the bottom: `0` = live tail (newest content visible).
#[derive(Debug, Clone)]
pub struct OutputPanel {
    lines: VecDeque<String>,
    cap: usize,
    /// Lines hidden at the bottom; 0 = live tail.
    scroll_offset: usize,
}

impl OutputPanel {
    pub fn new(cap: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            cap,
            scroll_offset: 0,
        }
    }

    pub fn with_default_cap() -> Self {
        Self::new(DEFAULT_LINE_CAP)
    }

    // ── read accessors ────────────────────────────────────────────────────────

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn lines(&self) -> &VecDeque<String> {
        &self.lines
    }

    // ── mutation ──────────────────────────────────────────────────────────────

    /// Append a line, evicting the oldest entry if the cap is exceeded.
    pub fn push_line(&mut self, line: impl Into<String>) {
        self.lines.push_back(line.into());
        if self.lines.len() > self.cap {
            self.lines.pop_front();
            // Shift offset to keep the visible window stable when at the top.
            self.scroll_offset = self.scroll_offset.saturating_sub(1);
        }
    }

    /// Erase all lines and reset the scroll position.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll_offset = 0;
    }

    // ── scroll ────────────────────────────────────────────────────────────────

    /// Show older content (scroll toward the top).
    pub fn scroll_up(&mut self) {
        let max = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + 1).min(max);
    }

    /// Show newer content (scroll toward the bottom).
    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    /// Jump to the live tail (newest content).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    /// Maximum useful scroll offset: one past the oldest visible line.
    fn max_scroll(&self) -> usize {
        self.lines.len().saturating_sub(1)
    }

    // ── rendering ─────────────────────────────────────────────────────────────

    /// Return the slice of lines that fit in a window of `height` rows.
    ///
    /// `scroll_offset = 0` returns the newest `height` lines.
    /// Increasing `scroll_offset` reveals older lines.
    pub fn visible_lines(&self, height: usize) -> Vec<&str> {
        if self.lines.is_empty() || height == 0 {
            return vec![];
        }
        let end = self.lines.len().saturating_sub(self.scroll_offset);
        let end = end.max(1); // always show at least 1 line if any exist
        let start = end.saturating_sub(height);
        self.lines
            .iter()
            .skip(start)
            .take(end - start)
            .map(|s| s.as_str())
            .collect()
    }
}

impl Default for OutputPanel {
    fn default() -> Self {
        Self::with_default_cap()
    }
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> OutputPanel {
        OutputPanel::new(10)
    }

    // ── construction ──────────────────────────────────────────────────────────

    #[test]
    fn new_panel_is_empty() {
        let p = panel();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
        assert_eq!(p.cap(), 10);
        assert_eq!(p.scroll_offset(), 0);
    }

    #[test]
    fn with_default_cap_uses_500() {
        let p = OutputPanel::with_default_cap();
        assert_eq!(p.cap(), DEFAULT_LINE_CAP);
    }

    #[test]
    fn default_uses_default_cap() {
        let p = OutputPanel::default();
        assert_eq!(p.cap(), DEFAULT_LINE_CAP);
    }

    // ── push_line ─────────────────────────────────────────────────────────────

    #[test]
    fn push_line_adds_to_buffer() {
        let mut p = panel();
        p.push_line("hello");
        assert_eq!(p.len(), 1);
        assert_eq!(p.lines()[0], "hello");
    }

    #[test]
    fn push_multiple_lines_ordered_oldest_to_newest() {
        let mut p = panel();
        p.push_line("first");
        p.push_line("second");
        p.push_line("third");
        let lines: Vec<_> = p.lines().iter().cloned().collect();
        assert_eq!(lines, vec!["first", "second", "third"]);
    }

    #[test]
    fn push_enforces_cap_evicts_oldest() {
        let mut p = OutputPanel::new(3);
        p.push_line("a");
        p.push_line("b");
        p.push_line("c");
        p.push_line("d"); // evicts "a"
        assert_eq!(p.len(), 3);
        let lines: Vec<_> = p.lines().iter().cloned().collect();
        assert_eq!(lines, vec!["b", "c", "d"]);
    }

    #[test]
    fn push_at_cap_keeps_cap_lines() {
        let mut p = OutputPanel::new(3);
        for i in 0..10 {
            p.push_line(i.to_string());
        }
        assert_eq!(p.len(), 3);
    }

    // ── scroll math ───────────────────────────────────────────────────────────

    #[test]
    fn scroll_up_increments_offset() {
        let mut p = panel();
        for i in 0..5 {
            p.push_line(i.to_string());
        }
        assert_eq!(p.scroll_offset(), 0);
        p.scroll_up();
        assert_eq!(p.scroll_offset(), 1);
        p.scroll_up();
        assert_eq!(p.scroll_offset(), 2);
    }

    #[test]
    fn scroll_up_capped_at_max_scroll() {
        let mut p = OutputPanel::new(5);
        for i in 0..5 {
            p.push_line(i.to_string());
        }
        // max_scroll = 5 - 1 = 4
        for _ in 0..20 {
            p.scroll_up();
        }
        assert_eq!(p.scroll_offset(), 4);
    }

    #[test]
    fn scroll_up_on_empty_is_noop() {
        let mut p = panel();
        p.scroll_up();
        assert_eq!(p.scroll_offset(), 0);
    }

    #[test]
    fn scroll_down_decrements_offset() {
        let mut p = panel();
        for i in 0..5 {
            p.push_line(i.to_string());
        }
        p.scroll_up();
        p.scroll_up();
        assert_eq!(p.scroll_offset(), 2);
        p.scroll_down();
        assert_eq!(p.scroll_offset(), 1);
    }

    #[test]
    fn scroll_down_at_bottom_is_noop() {
        let mut p = panel();
        p.push_line("a");
        assert_eq!(p.scroll_offset(), 0);
        p.scroll_down();
        assert_eq!(p.scroll_offset(), 0);
    }

    #[test]
    fn scroll_to_bottom_resets_offset() {
        let mut p = panel();
        for i in 0..5 {
            p.push_line(i.to_string());
        }
        p.scroll_up();
        p.scroll_up();
        p.scroll_up();
        p.scroll_to_bottom();
        assert_eq!(p.scroll_offset(), 0);
    }

    // ── visible_lines ─────────────────────────────────────────────────────────

    #[test]
    fn visible_lines_empty_panel_returns_empty() {
        let p = panel();
        assert!(p.visible_lines(5).is_empty());
    }

    #[test]
    fn visible_lines_zero_height_returns_empty() {
        let mut p = panel();
        p.push_line("a");
        assert!(p.visible_lines(0).is_empty());
    }

    #[test]
    fn visible_lines_at_bottom_shows_latest() {
        let mut p = panel();
        for i in 0..8 {
            p.push_line(i.to_string());
        }
        // height=5, offset=0 → show lines 3,4,5,6,7
        let visible = p.visible_lines(5);
        assert_eq!(visible, vec!["3", "4", "5", "6", "7"]);
    }

    #[test]
    fn visible_lines_fewer_than_height_shows_all() {
        let mut p = panel();
        p.push_line("a");
        p.push_line("b");
        let visible = p.visible_lines(10);
        assert_eq!(visible, vec!["a", "b"]);
    }

    #[test]
    fn visible_lines_with_scroll_shows_older_content() {
        let mut p = panel();
        for i in 0..8 {
            p.push_line(i.to_string());
        }
        // Scroll up 2: end = 8 - 2 = 6, start = 6 - 5 = 1 → lines 1,2,3,4,5
        p.scroll_up();
        p.scroll_up();
        let visible = p.visible_lines(5);
        assert_eq!(visible, vec!["1", "2", "3", "4", "5"]);
    }

    #[test]
    fn visible_lines_at_max_scroll_shows_oldest() {
        let mut p = OutputPanel::new(10);
        for i in 0..5 {
            p.push_line(i.to_string());
        }
        // Scroll to max (4): end = 5 - 4 = 1, start = 0 → just ["0"]
        for _ in 0..10 {
            p.scroll_up();
        }
        let visible = p.visible_lines(5);
        assert_eq!(visible, vec!["0"]);
    }

    #[test]
    fn visible_lines_height_equals_len_shows_all() {
        let mut p = panel();
        p.push_line("x");
        p.push_line("y");
        p.push_line("z");
        let visible = p.visible_lines(3);
        assert_eq!(visible, vec!["x", "y", "z"]);
    }

    // ── clear ─────────────────────────────────────────────────────────────────

    #[test]
    fn clear_empties_buffer_and_resets_scroll() {
        let mut p = panel();
        for i in 0..5 {
            p.push_line(i.to_string());
        }
        p.scroll_up();
        p.clear();
        assert!(p.is_empty());
        assert_eq!(p.scroll_offset(), 0);
    }

    // ── live-tail behaviour ───────────────────────────────────────────────────

    #[test]
    fn push_at_bottom_stays_at_bottom() {
        let mut p = OutputPanel::new(5);
        p.push_line("a");
        p.push_line("b");
        assert_eq!(p.scroll_offset(), 0);
        p.push_line("c");
        assert_eq!(p.scroll_offset(), 0);
    }

    #[test]
    fn scroll_then_new_lines_preserve_offset_direction() {
        let mut p = OutputPanel::new(10);
        for i in 0..5 {
            p.push_line(i.to_string());
        }
        p.scroll_up();
        let offset_before = p.scroll_offset();
        p.push_line("new");
        // scroll offset must not go negative and offset should still be > 0
        assert!(p.scroll_offset() <= offset_before);
    }
}
