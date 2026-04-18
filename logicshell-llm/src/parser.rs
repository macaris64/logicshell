// Command response parser — Phase 10, LLM Module PRD §5.5
//
// Converts raw LLM response text into an argv vec suitable for dispatch.
// The pipeline:
//   1. Strip optional markdown code fence (```bash...``` or ```...```)
//   2. Take the first non-empty line (models sometimes emit preamble)
//   3. Shell-tokenize: whitespace, single-quotes (literal), double-quotes
//      (backslash-escaped), backslash escapes outside quotes.
//
// Returns `LlmError::Parse` on empty/whitespace-only responses and on
// unterminated quoted strings.

use crate::error::LlmError;

/// Parse a raw LLM response into an argv vector.
///
/// Strips code fences, takes the first non-empty line, and tokenizes it
/// using POSIX-style shell quoting rules (single-quotes, double-quotes,
/// backslash escapes).
///
/// # Errors
///
/// - `LlmError::Parse` when the response is empty after stripping.
/// - `LlmError::Parse` when the response contains an unterminated quote.
pub fn parse_command_response(text: &str) -> Result<Vec<String>, LlmError> {
    let stripped = strip_code_fence(text);

    let first_line = stripped
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");

    if first_line.is_empty() {
        return Err(LlmError::Parse(
            "LLM response contained no parseable command".into(),
        ));
    }

    tokenize(first_line)
}

/// Strip a leading ```` ``` ```` (with optional language tag) and trailing
/// ```` ``` ```` from `text`. Returns a trimmed `&str` slice of the interior
/// content. If no fence is found the input is returned trimmed.
fn strip_code_fence(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        // Skip an optional language tag that runs to the first newline.
        let after_tag = if let Some(nl) = rest.find('\n') {
            &rest[nl + 1..]
        } else {
            rest
        };
        // Strip the trailing ``` if present.
        if let Some(end) = after_tag.rfind("```") {
            return after_tag[..end].trim();
        }
        // Malformed fence: return the raw interior anyway.
        return after_tag.trim();
    }
    trimmed
}

/// POSIX-style shell tokenizer for a single command line.
///
/// Supported:
/// - Unquoted tokens split on ASCII whitespace
/// - `'...'` single-quotes — literal content, no escaping inside
/// - `"..."` double-quotes — backslash-escape inside (`\\`, `\"`, `\n`, etc.)
/// - `\x` outside quotes — the character `x` is taken literally
///
/// Returns `Err(())` on unterminated quotes; callers map this to `LlmError::Parse`.
fn tokenize(s: &str) -> Result<Vec<String>, LlmError> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if in_token {
                    tokens.push(current.clone());
                    current.clear();
                    in_token = false;
                }
            }
            '\'' => {
                in_token = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(ch) => current.push(ch),
                        None => {
                            return Err(LlmError::Parse(
                                "unterminated single-quote in LLM response".into(),
                            ))
                        }
                    }
                }
            }
            '"' => {
                in_token = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(escaped) => current.push(escaped),
                            None => {
                                return Err(LlmError::Parse(
                                    "unterminated escape in double-quoted string".into(),
                                ))
                            }
                        },
                        Some(ch) => current.push(ch),
                        None => {
                            return Err(LlmError::Parse(
                                "unterminated double-quote in LLM response".into(),
                            ))
                        }
                    }
                }
            }
            '\\' => {
                in_token = true;
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            }
            _ => {
                in_token = true;
                current.push(c);
            }
        }
    }

    if in_token {
        tokens.push(current);
    }

    if tokens.is_empty() {
        return Err(LlmError::Parse(
            "LLM response tokenized to empty argv".into(),
        ));
    }

    Ok(tokens)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_code_fence ──────────────────────────────────────────────────────

    #[test]
    fn strip_fence_no_fence_returns_trimmed() {
        assert_eq!(strip_code_fence("ls -la"), "ls -la");
    }

    #[test]
    fn strip_fence_with_bash_tag() {
        let input = "```bash\nls -la\n```";
        assert_eq!(strip_code_fence(input), "ls -la");
    }

    #[test]
    fn strip_fence_without_language_tag() {
        let input = "```\nls -la\n```";
        assert_eq!(strip_code_fence(input), "ls -la");
    }

    #[test]
    fn strip_fence_multiline_takes_interior() {
        let input = "```bash\nls -la\necho done\n```";
        let result = strip_code_fence(input);
        assert!(result.contains("ls -la"));
        assert!(result.contains("echo done"));
    }

    #[test]
    fn strip_fence_trims_surrounding_whitespace() {
        assert_eq!(strip_code_fence("  ls -la  "), "ls -la");
    }

    #[test]
    fn strip_fence_empty_string() {
        assert_eq!(strip_code_fence(""), "");
    }

    #[test]
    fn strip_fence_only_whitespace() {
        assert_eq!(strip_code_fence("   "), "");
    }

    // ── tokenize ──────────────────────────────────────────────────────────────

    #[test]
    fn tokenize_simple_command() {
        assert_eq!(tokenize("ls").unwrap(), vec!["ls"]);
    }

    #[test]
    fn tokenize_command_with_args() {
        assert_eq!(tokenize("ls -la").unwrap(), vec!["ls", "-la"]);
    }

    #[test]
    fn tokenize_multiple_spaces_collapsed() {
        assert_eq!(tokenize("ls  -la").unwrap(), vec!["ls", "-la"]);
    }

    #[test]
    fn tokenize_single_quoted_space() {
        assert_eq!(
            tokenize("echo 'hello world'").unwrap(),
            vec!["echo", "hello world"]
        );
    }

    #[test]
    fn tokenize_double_quoted_space() {
        assert_eq!(
            tokenize(r#"echo "hello world""#).unwrap(),
            vec!["echo", "hello world"]
        );
    }

    #[test]
    fn tokenize_double_quote_with_backslash_escape() {
        assert_eq!(
            tokenize(r#"echo "hello \"world\"""#).unwrap(),
            vec!["echo", r#"hello "world""#]
        );
    }

    #[test]
    fn tokenize_backslash_space() {
        assert_eq!(
            tokenize(r#"echo hello\ world"#).unwrap(),
            vec!["echo", "hello world"]
        );
    }

    #[test]
    fn tokenize_tab_as_separator() {
        assert_eq!(tokenize("ls\t-la").unwrap(), vec!["ls", "-la"]);
    }

    #[test]
    fn tokenize_unterminated_single_quote_is_error() {
        assert!(tokenize("echo 'hello").is_err());
    }

    #[test]
    fn tokenize_unterminated_double_quote_is_error() {
        assert!(tokenize(r#"echo "hello"#).is_err());
    }

    #[test]
    fn tokenize_empty_string_is_error() {
        assert!(tokenize("").is_err());
    }

    #[test]
    fn tokenize_whitespace_only_is_error() {
        assert!(tokenize("   ").is_err());
    }

    #[test]
    fn tokenize_cmd_with_flags_and_path() {
        assert_eq!(
            tokenize("find /tmp -name '*.log' -type f").unwrap(),
            vec!["find", "/tmp", "-name", "*.log", "-type", "f"]
        );
    }

    #[test]
    fn tokenize_git_commit_with_message() {
        assert_eq!(
            tokenize(r#"git commit -m "fix: typo""#).unwrap(),
            vec!["git", "commit", "-m", "fix: typo"]
        );
    }

    // ── parse_command_response ────────────────────────────────────────────────

    #[test]
    fn parse_simple_command() {
        let result = parse_command_response("ls -la").unwrap();
        assert_eq!(result, vec!["ls", "-la"]);
    }

    #[test]
    fn parse_takes_first_non_empty_line() {
        let result = parse_command_response("\nls -la\necho done\n").unwrap();
        assert_eq!(result, vec!["ls", "-la"]);
    }

    #[test]
    fn parse_strips_bash_fence() {
        let result = parse_command_response("```bash\nls -la\n```").unwrap();
        assert_eq!(result, vec!["ls", "-la"]);
    }

    #[test]
    fn parse_strips_plain_fence() {
        let result = parse_command_response("```\nls -la\n```").unwrap();
        assert_eq!(result, vec!["ls", "-la"]);
    }

    #[test]
    fn parse_empty_response_is_error() {
        assert!(parse_command_response("").is_err());
    }

    #[test]
    fn parse_whitespace_only_is_error() {
        assert!(parse_command_response("   \n\n  ").is_err());
    }

    #[test]
    fn parse_error_is_parse_variant() {
        let e = parse_command_response("").unwrap_err();
        assert!(matches!(e, LlmError::Parse(_)));
    }

    #[test]
    fn parse_unterminated_quote_is_parse_error() {
        let e = parse_command_response("echo 'hello").unwrap_err();
        assert!(matches!(e, LlmError::Parse(_)));
    }

    #[test]
    fn parse_command_with_quoted_args() {
        let result = parse_command_response(r#"grep -r "TODO" src/"#).unwrap();
        assert_eq!(result, vec!["grep", "-r", "TODO", "src/"]);
    }

    #[test]
    fn parse_git_log_command() {
        let result = parse_command_response("git log --oneline -10").unwrap();
        assert_eq!(result, vec!["git", "log", "--oneline", "-10"]);
    }

    #[test]
    fn parse_single_word_command() {
        let result = parse_command_response("pwd").unwrap();
        assert_eq!(result, vec!["pwd"]);
    }

    #[test]
    fn parse_trims_leading_trailing_whitespace() {
        let result = parse_command_response("  ls -la  ").unwrap();
        assert_eq!(result, vec!["ls", "-la"]);
    }
}
