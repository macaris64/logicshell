/// Full-featured input line with cursor tracking and readline-like editing.
///
/// The cursor is a char-index in `0..=buffer.len()`:
///   - 0 means before the first character
///   - buffer.len() means after the last character
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputWidget {
    buffer: Vec<char>,
    /// Cursor position as a char index in `0..=buffer.len()`.
    pub cursor: usize,
}

impl Default for InputWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl InputWidget {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            cursor: 0,
        }
    }

    // ── read accessors ────────────────────────────────────────────────────────

    pub fn value(&self) -> String {
        self.buffer.iter().collect()
    }

    pub fn cursor_pos(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    // ── write helpers ─────────────────────────────────────────────────────────

    /// Replace the buffer with `s`, placing cursor at the end.
    pub fn set_value(&mut self, s: &str) {
        self.buffer = s.chars().collect();
        self.cursor = self.buffer.len();
    }

    /// Erase all content and reset cursor to 0.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
    }

    // ── editing ───────────────────────────────────────────────────────────────

    /// Insert `c` at the cursor position and advance the cursor by one.
    pub fn insert(&mut self, c: char) {
        self.buffer.insert(self.cursor, c);
        self.cursor += 1;
    }

    /// Delete the character immediately before the cursor (Backspace).
    pub fn delete_before_cursor(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buffer.remove(self.cursor);
        }
    }

    /// Delete the character immediately after the cursor (Delete key).
    pub fn delete_after_cursor(&mut self) {
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
        }
    }

    /// Remove all characters from the cursor to the end of the line (Ctrl-K).
    pub fn kill_to_end(&mut self) {
        self.buffer.truncate(self.cursor);
    }

    // ── movement ──────────────────────────────────────────────────────────────

    /// Move cursor one character to the left (Left arrow).
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor one character to the right (Right arrow).
    pub fn move_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor += 1;
        }
    }

    /// Move cursor to the beginning of the line (Home / Ctrl-A).
    pub fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to the end of the line (End / Ctrl-E).
    pub fn move_to_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    // ── rendering ─────────────────────────────────────────────────────────────

    /// Return the buffer as a string with an underscore `_` cursor marker
    /// inserted at the current cursor position.
    ///
    /// Examples:
    /// - cursor at end of "hello"  → `"hello_"`
    /// - cursor at position 2 of "hello" → `"he_llo"`
    /// - empty buffer, cursor 0   → `"_"`
    pub fn render_with_cursor(&self) -> String {
        let before: String = self.buffer[..self.cursor].iter().collect();
        let after: String = self.buffer[self.cursor..].iter().collect();
        format!("{before}_{after}")
    }
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── construction ──────────────────────────────────────────────────────────

    #[test]
    fn new_widget_is_empty_with_cursor_at_zero() {
        let w = InputWidget::new();
        assert!(w.is_empty());
        assert_eq!(w.len(), 0);
        assert_eq!(w.cursor_pos(), 0);
        assert_eq!(w.value(), "");
    }

    #[test]
    fn default_is_same_as_new() {
        let d = InputWidget::default();
        assert!(d.is_empty());
        assert_eq!(d.cursor_pos(), 0);
    }

    // ── insert ────────────────────────────────────────────────────────────────

    #[test]
    fn insert_appends_chars_and_advances_cursor() {
        let mut w = InputWidget::new();
        w.insert('h');
        w.insert('i');
        assert_eq!(w.value(), "hi");
        assert_eq!(w.cursor_pos(), 2);
    }

    #[test]
    fn insert_at_start_prepends() {
        let mut w = InputWidget::new();
        w.insert('b');
        w.move_to_start();
        w.insert('a');
        assert_eq!(w.value(), "ab");
        assert_eq!(w.cursor_pos(), 1);
    }

    #[test]
    fn insert_mid_buffer_shifts_chars() {
        let mut w = InputWidget::new();
        for c in "ac".chars() {
            w.insert(c);
        }
        w.move_left(); // cursor between 'a' and 'c'
        w.insert('b');
        assert_eq!(w.value(), "abc");
        assert_eq!(w.cursor_pos(), 2);
    }

    // ── delete_before_cursor (Backspace) ──────────────────────────────────────

    #[test]
    fn backspace_removes_char_before_cursor() {
        let mut w = InputWidget::new();
        w.insert('a');
        w.insert('b');
        w.delete_before_cursor();
        assert_eq!(w.value(), "a");
        assert_eq!(w.cursor_pos(), 1);
    }

    #[test]
    fn backspace_at_start_is_noop() {
        let mut w = InputWidget::new();
        w.insert('x');
        w.move_to_start();
        w.delete_before_cursor();
        assert_eq!(w.value(), "x");
        assert_eq!(w.cursor_pos(), 0);
    }

    #[test]
    fn backspace_on_empty_buffer_is_noop() {
        let mut w = InputWidget::new();
        w.delete_before_cursor();
        assert_eq!(w.value(), "");
        assert_eq!(w.cursor_pos(), 0);
    }

    #[test]
    fn backspace_mid_buffer_removes_correct_char() {
        let mut w = InputWidget::new();
        for c in "abc".chars() {
            w.insert(c);
        }
        w.move_left(); // cursor after 'b', before 'c'
        w.delete_before_cursor(); // removes 'b'
        assert_eq!(w.value(), "ac");
        assert_eq!(w.cursor_pos(), 1);
    }

    // ── delete_after_cursor (Delete key) ─────────────────────────────────────

    #[test]
    fn delete_after_cursor_removes_char_at_cursor() {
        let mut w = InputWidget::new();
        for c in "ab".chars() {
            w.insert(c);
        }
        w.move_to_start();
        w.delete_after_cursor(); // removes 'a'
        assert_eq!(w.value(), "b");
        assert_eq!(w.cursor_pos(), 0);
    }

    #[test]
    fn delete_after_cursor_at_end_is_noop() {
        let mut w = InputWidget::new();
        w.insert('x');
        w.delete_after_cursor();
        assert_eq!(w.value(), "x");
        assert_eq!(w.cursor_pos(), 1);
    }

    #[test]
    fn delete_after_cursor_on_empty_is_noop() {
        let mut w = InputWidget::new();
        w.delete_after_cursor();
        assert_eq!(w.value(), "");
        assert_eq!(w.cursor_pos(), 0);
    }

    // ── kill_to_end (Ctrl-K) ──────────────────────────────────────────────────

    #[test]
    fn kill_to_end_removes_from_cursor() {
        let mut w = InputWidget::new();
        for c in "hello world".chars() {
            w.insert(c);
        }
        w.set_value("hello world");
        w.cursor = 5; // after "hello"
        w.kill_to_end();
        assert_eq!(w.value(), "hello");
        assert_eq!(w.cursor_pos(), 5);
    }

    #[test]
    fn kill_to_end_from_start_clears_buffer() {
        let mut w = InputWidget::new();
        w.set_value("clear me");
        w.move_to_start();
        w.kill_to_end();
        assert_eq!(w.value(), "");
        assert_eq!(w.cursor_pos(), 0);
    }

    #[test]
    fn kill_to_end_at_end_is_noop() {
        let mut w = InputWidget::new();
        w.set_value("hello");
        w.kill_to_end();
        assert_eq!(w.value(), "hello");
        assert_eq!(w.cursor_pos(), 5);
    }

    // ── movement ──────────────────────────────────────────────────────────────

    #[test]
    fn move_left_decrements_cursor() {
        let mut w = InputWidget::new();
        w.set_value("ab");
        w.move_left();
        assert_eq!(w.cursor_pos(), 1);
        w.move_left();
        assert_eq!(w.cursor_pos(), 0);
    }

    #[test]
    fn move_left_at_start_is_noop() {
        let mut w = InputWidget::new();
        w.set_value("x");
        w.move_to_start();
        w.move_left();
        assert_eq!(w.cursor_pos(), 0);
    }

    #[test]
    fn move_right_increments_cursor() {
        let mut w = InputWidget::new();
        w.set_value("ab");
        w.move_to_start();
        w.move_right();
        assert_eq!(w.cursor_pos(), 1);
        w.move_right();
        assert_eq!(w.cursor_pos(), 2);
    }

    #[test]
    fn move_right_at_end_is_noop() {
        let mut w = InputWidget::new();
        w.set_value("x");
        w.move_right();
        assert_eq!(w.cursor_pos(), 1); // already at end
    }

    #[test]
    fn move_to_start_sets_cursor_to_zero() {
        let mut w = InputWidget::new();
        w.set_value("hello");
        w.move_to_start();
        assert_eq!(w.cursor_pos(), 0);
    }

    #[test]
    fn move_to_end_sets_cursor_to_len() {
        let mut w = InputWidget::new();
        w.set_value("hello");
        w.move_to_start();
        w.move_to_end();
        assert_eq!(w.cursor_pos(), 5);
    }

    // ── set_value and clear ───────────────────────────────────────────────────

    #[test]
    fn set_value_replaces_buffer_and_moves_cursor_to_end() {
        let mut w = InputWidget::new();
        w.set_value("hello");
        assert_eq!(w.value(), "hello");
        assert_eq!(w.cursor_pos(), 5);
    }

    #[test]
    fn clear_empties_buffer_and_resets_cursor() {
        let mut w = InputWidget::new();
        w.set_value("hello");
        w.clear();
        assert_eq!(w.value(), "");
        assert_eq!(w.cursor_pos(), 0);
        assert!(w.is_empty());
    }

    // ── render_with_cursor ────────────────────────────────────────────────────

    #[test]
    fn render_cursor_at_end_appends_underscore() {
        let mut w = InputWidget::new();
        w.set_value("hello");
        assert_eq!(w.render_with_cursor(), "hello_");
    }

    #[test]
    fn render_cursor_at_start_prepends_underscore() {
        let mut w = InputWidget::new();
        w.set_value("hello");
        w.move_to_start();
        assert_eq!(w.render_with_cursor(), "_hello");
    }

    #[test]
    fn render_cursor_mid_inserts_underscore_at_position() {
        let mut w = InputWidget::new();
        w.set_value("hello");
        w.cursor = 2;
        assert_eq!(w.render_with_cursor(), "he_llo");
    }

    #[test]
    fn render_empty_buffer_is_just_underscore() {
        let w = InputWidget::new();
        assert_eq!(w.render_with_cursor(), "_");
    }

    // ── unicode / multi-byte chars ────────────────────────────────────────────

    #[test]
    fn insert_unicode_chars_handled_correctly() {
        let mut w = InputWidget::new();
        w.insert('é');
        w.insert('ñ');
        assert_eq!(w.value(), "éñ");
        assert_eq!(w.len(), 2);
        assert_eq!(w.cursor_pos(), 2);
    }

    #[test]
    fn backspace_unicode_char_removes_one_grapheme() {
        let mut w = InputWidget::new();
        w.set_value("héllo");
        w.cursor = 2; // after 'é'
        w.delete_before_cursor();
        assert_eq!(w.value(), "hllo");
        assert_eq!(w.cursor_pos(), 1);
    }

    // ── cursor math invariants ────────────────────────────────────────────────

    #[test]
    fn cursor_never_exceeds_buffer_len_after_edits() {
        let mut w = InputWidget::new();
        w.set_value("abc");
        w.kill_to_end(); // value = "abc", cursor = 3, kill nothing
        assert_eq!(w.cursor_pos(), 3);
        w.set_value("abc");
        w.move_to_start();
        w.kill_to_end();
        assert_eq!(w.cursor_pos(), 0);
        assert_eq!(w.len(), 0);
    }

    #[test]
    fn repeated_backspace_eventually_empties_buffer() {
        let mut w = InputWidget::new();
        w.set_value("abc");
        w.delete_before_cursor();
        w.delete_before_cursor();
        w.delete_before_cursor();
        w.delete_before_cursor(); // extra, should be noop
        assert!(w.is_empty());
        assert_eq!(w.cursor_pos(), 0);
    }
}
