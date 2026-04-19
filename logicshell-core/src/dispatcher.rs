// Process dispatcher — FR-01–04, NFR-06, NFR-08

use std::path::PathBuf;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::{config::LimitsConfig, LogicShellError, Result};

/// How to connect the child's stdin.
#[derive(Debug, Clone, Default)]
pub enum StdinMode {
    /// Connect stdin to `/dev/null`; the child sees immediate EOF.
    #[default]
    Null,
    /// Inherit the caller's stdin file descriptor.
    Inherit,
    /// Feed the supplied bytes to the child's stdin, then close the pipe.
    Piped(Vec<u8>),
}

/// Options for a single dispatch invocation.
#[derive(Debug, Clone, Default)]
pub struct DispatchOptions {
    /// `argv[0]` is the executable; remaining elements are arguments — FR-01.
    pub argv: Vec<String>,
    /// Additional environment variables to inject (overrides inherited env) — FR-01.
    pub env_extra: Vec<(String, String)>,
    /// Working directory for the child process (`None` = inherit) — FR-04.
    pub cwd: Option<PathBuf>,
    /// Stdin connection mode — FR-02.
    pub stdin: StdinMode,
}

/// Structured result of a completed child process — FR-03.
#[derive(Debug, Clone)]
pub struct DispatchOutput {
    /// The process exit code (`-1` if the OS does not surface one, e.g. signal kill).
    pub exit_code: i32,
    /// Captured stdout bytes, capped at `max_stdout_capture_bytes` — NFR-08.
    pub stdout: Vec<u8>,
    /// Captured stderr bytes (not capped).
    pub stderr: Vec<u8>,
    /// `true` when stdout was truncated because the limit was reached.
    pub stdout_truncated: bool,
}

/// Async process dispatcher wrapping `tokio::process::Command`.
///
/// Constructed with a byte-cap for stdout capture; all other limits come from
/// the caller's `DispatchOptions`.
#[derive(Debug, Clone)]
pub struct Dispatcher {
    max_stdout_capture_bytes: u64,
}

impl Dispatcher {
    /// Create a dispatcher using the limits from a loaded [`LimitsConfig`].
    pub fn new(limits: &LimitsConfig) -> Self {
        Self {
            max_stdout_capture_bytes: limits.max_stdout_capture_bytes,
        }
    }

    /// Create a dispatcher with an explicit stdout capture limit (useful in tests).
    pub fn with_capture_limit(max_bytes: u64) -> Self {
        Self {
            max_stdout_capture_bytes: max_bytes,
        }
    }

    /// Spawn a child process and return its structured output.
    ///
    /// - `argv` must be non-empty; `argv[0]` is the executable.
    /// - stdout is captured up to `max_stdout_capture_bytes`; any excess is discarded
    ///   and `stdout_truncated` is set to `true`.
    /// - stderr is captured without a byte cap.
    /// - A nonzero exit code is **not** an error; callers inspect `exit_code`.
    pub async fn dispatch(&self, opts: DispatchOptions) -> Result<DispatchOutput> {
        if opts.argv.is_empty() {
            return Err(LogicShellError::Dispatch("argv must not be empty".into()));
        }

        let mut cmd = Command::new(&opts.argv[0]);
        if opts.argv.len() > 1 {
            cmd.args(&opts.argv[1..]);
        }

        for (k, v) in &opts.env_extra {
            cmd.env(k, v);
        }

        if let Some(ref cwd) = opts.cwd {
            cmd.current_dir(cwd);
        }

        let piped_stdin_data: Option<Vec<u8>> = match opts.stdin {
            StdinMode::Null => {
                cmd.stdin(Stdio::null());
                None
            }
            StdinMode::Inherit => {
                cmd.stdin(Stdio::inherit());
                None
            }
            StdinMode::Piped(data) => {
                cmd.stdin(Stdio::piped());
                Some(data)
            }
        };

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| LogicShellError::Dispatch(format!("spawn failed: {e}")))?;

        // Spawn stdin writer as an independent task to prevent deadlock when the
        // child fills stdout before consuming all piped-in bytes.
        let stdin_task = if let Some(data) = piped_stdin_data {
            child.stdin.take().map(|mut stdin_handle| {
                tokio::spawn(async move {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin_handle.write_all(&data).await;
                    // Drop closes the pipe, signalling EOF to the child.
                })
            })
        } else {
            None
        };

        let stdout_handle = child.stdout.take().expect("stdout is piped");
        let stderr_handle = child.stderr.take().expect("stderr is piped");
        let max_bytes = self.max_stdout_capture_bytes as usize;

        // Read stdout (bounded) and stderr concurrently to avoid pipe-full deadlock.
        let stdout_fut = async move {
            let mut buf = Vec::new();
            let reader = tokio::io::BufReader::new(stdout_handle);
            // take() consumes the reader; into_inner() gives it back so we can
            // probe for a trailing byte to detect whether stdout was truncated.
            let mut take = reader.take(max_bytes as u64);
            let _ = take.read_to_end(&mut buf).await;
            let mut reader = take.into_inner();
            let mut extra = [0u8; 1];
            let truncated = reader.read(&mut extra).await.unwrap_or(0) > 0;
            (buf, truncated)
        };

        let stderr_fut = async move {
            let mut buf = Vec::new();
            let _ = tokio::io::BufReader::new(stderr_handle)
                .read_to_end(&mut buf)
                .await;
            buf
        };

        let ((stdout, stdout_truncated), stderr) = tokio::join!(stdout_fut, stderr_fut);

        if let Some(task) = stdin_task {
            let _ = task.await;
        }

        let status = child
            .wait()
            .await
            .map_err(|e| LogicShellError::Dispatch(format!("wait failed: {e}")))?;

        let exit_code = status.code().unwrap_or(-1);

        Ok(DispatchOutput {
            exit_code,
            stdout,
            stderr,
            stdout_truncated,
        })
    }

    /// Spawn a child process and stream its stdout line-by-line to `line_tx`.
    ///
    /// Each line is sent as it arrives; stderr is captured in full and returned
    /// in [`DispatchOutput`].  The stdout field of the returned output contains
    /// all bytes that were sent through the channel.
    pub async fn dispatch_streaming(
        &self,
        opts: DispatchOptions,
        line_tx: mpsc::UnboundedSender<String>,
    ) -> Result<DispatchOutput> {
        if opts.argv.is_empty() {
            return Err(LogicShellError::Dispatch("argv must not be empty".into()));
        }

        let mut cmd = Command::new(&opts.argv[0]);
        if opts.argv.len() > 1 {
            cmd.args(&opts.argv[1..]);
        }
        for (k, v) in &opts.env_extra {
            cmd.env(k, v);
        }
        if let Some(ref cwd) = opts.cwd {
            cmd.current_dir(cwd);
        }

        let piped_stdin_data: Option<Vec<u8>> = match opts.stdin {
            StdinMode::Null => {
                cmd.stdin(Stdio::null());
                None
            }
            StdinMode::Inherit => {
                cmd.stdin(Stdio::inherit());
                None
            }
            StdinMode::Piped(data) => {
                cmd.stdin(Stdio::piped());
                Some(data)
            }
        };

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| LogicShellError::Dispatch(format!("spawn failed: {e}")))?;

        let stdin_task = if let Some(data) = piped_stdin_data {
            child.stdin.take().map(|mut stdin_handle| {
                tokio::spawn(async move {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin_handle.write_all(&data).await;
                })
            })
        } else {
            None
        };

        let stdout_handle = child.stdout.take().expect("stdout is piped");
        let stderr_handle = child.stderr.take().expect("stderr is piped");

        let stdout_fut = async move {
            let reader = tokio::io::BufReader::new(stdout_handle);
            let mut lines = reader.lines();
            let mut all_bytes: Vec<u8> = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                all_bytes.extend_from_slice(line.as_bytes());
                all_bytes.push(b'\n');
                let _ = line_tx.send(line);
            }
            all_bytes
        };

        let stderr_fut = async move {
            let mut buf = Vec::new();
            let _ = tokio::io::BufReader::new(stderr_handle)
                .read_to_end(&mut buf)
                .await;
            buf
        };

        let (stdout_bytes, stderr) = tokio::join!(stdout_fut, stderr_fut);

        if let Some(task) = stdin_task {
            let _ = task.await;
        }

        let status = child
            .wait()
            .await
            .map_err(|e| LogicShellError::Dispatch(format!("wait failed: {e}")))?;

        let exit_code = status.code().unwrap_or(-1);

        Ok(DispatchOutput {
            exit_code,
            stdout: stdout_bytes,
            stderr,
            stdout_truncated: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LimitsConfig;

    fn default_dispatcher() -> Dispatcher {
        Dispatcher::new(&LimitsConfig::default())
    }

    /// Phase 5 smoke: Dispatcher is constructible — FR-01
    #[test]
    fn dispatcher_new() {
        let _d = default_dispatcher();
    }

    /// `with_capture_limit` sets the byte cap directly.
    #[test]
    fn dispatcher_with_capture_limit() {
        let d = Dispatcher::with_capture_limit(512);
        assert_eq!(d.max_stdout_capture_bytes, 512);
    }

    /// Empty argv returns a Dispatch error — NFR-06
    #[tokio::test]
    async fn empty_argv_returns_error() {
        let d = default_dispatcher();
        let result = d
            .dispatch(DispatchOptions {
                argv: vec![],
                ..Default::default()
            })
            .await;
        assert!(matches!(result, Err(LogicShellError::Dispatch(_))));
    }

    /// StdinMode variants are Clone + Debug — API completeness
    #[test]
    fn stdin_mode_clone_debug() {
        let modes: &[StdinMode] = &[
            StdinMode::Null,
            StdinMode::Inherit,
            StdinMode::Piped(b"hi".to_vec()),
        ];
        for m in modes {
            let _ = format!("{m:?}");
            let _ = m.clone();
        }
    }

    /// DispatchOptions default is well-formed.
    #[test]
    fn dispatch_options_default() {
        let o = DispatchOptions::default();
        assert!(o.argv.is_empty());
        assert!(o.env_extra.is_empty());
        assert!(o.cwd.is_none());
        assert!(matches!(o.stdin, StdinMode::Null));
    }

    /// FR-03: a process killed by signal has no exit code; we map it to -1.
    #[cfg(unix)]
    #[tokio::test]
    async fn signal_killed_process_returns_minus_one() {
        let d = default_dispatcher();
        // "kill -9 $$" sends SIGKILL to the shell itself → no exit code → -1.
        let out = d
            .dispatch(DispatchOptions {
                argv: vec!["sh".into(), "-c".into(), "kill -9 $$".into()],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(out.exit_code, -1);
    }

    /// dispatch_streaming delivers lines to the channel in order.
    #[tokio::test]
    async fn streaming_delivers_lines_in_order() {
        let d = default_dispatcher();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let out = d
            .dispatch_streaming(
                DispatchOptions {
                    argv: vec![
                        "sh".into(),
                        "-c".into(),
                        "echo line1; echo line2; echo line3".into(),
                    ],
                    ..Default::default()
                },
                tx,
            )
            .await
            .unwrap();
        assert_eq!(out.exit_code, 0);
        let mut lines = vec![];
        while let Ok(l) = rx.try_recv() {
            lines.push(l);
        }
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
    }

    /// dispatch_streaming with empty argv returns a Dispatch error.
    #[tokio::test]
    async fn streaming_empty_argv_returns_error() {
        let d = default_dispatcher();
        let (tx, _rx) = mpsc::unbounded_channel();
        let result = d.dispatch_streaming(DispatchOptions::default(), tx).await;
        assert!(matches!(result, Err(LogicShellError::Dispatch(_))));
    }

    /// dispatch_streaming captures exit code correctly.
    #[tokio::test]
    async fn streaming_exit_code_propagated() {
        let d = default_dispatcher();
        let (tx, _rx) = mpsc::unbounded_channel();
        let out = d
            .dispatch_streaming(
                DispatchOptions {
                    argv: vec!["false".into()],
                    ..Default::default()
                },
                tx,
            )
            .await
            .unwrap();
        assert_eq!(out.exit_code, 1);
    }

    /// dispatch_streaming stdout field contains all streamed bytes.
    #[tokio::test]
    async fn streaming_stdout_field_matches_channel() {
        let d = default_dispatcher();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let out = d
            .dispatch_streaming(
                DispatchOptions {
                    argv: vec!["echo".into(), "hello".into()],
                    ..Default::default()
                },
                tx,
            )
            .await
            .unwrap();
        assert!(!out.stdout.is_empty());
        let line = rx.try_recv().unwrap();
        assert_eq!(line, "hello");
    }
}
