// Integration tests for Phase 5 — Process dispatcher (FR-01–04, NFR-06, NFR-08)
// Uses CARGO_BIN_EXE_* fixture binaries built from src/bin/fixture_*.rs.

use logicshell_core::dispatcher::{DispatchOptions, Dispatcher, StdinMode};

const ECHO_ARGV: &str = env!("CARGO_BIN_EXE_fixture_echo_argv");
const ECHO_CWD: &str = env!("CARGO_BIN_EXE_fixture_echo_cwd");
const ECHO_ENV: &str = env!("CARGO_BIN_EXE_fixture_echo_env");
const EXIT_CODE: &str = env!("CARGO_BIN_EXE_fixture_exit_code");
const FLOOD_STDOUT: &str = env!("CARGO_BIN_EXE_fixture_flood_stdout");
const STDIN_ECHO: &str = env!("CARGO_BIN_EXE_fixture_stdin_echo");

fn default_dispatcher() -> Dispatcher {
    Dispatcher::with_capture_limit(1_048_576)
}

/// FR-01: argv is passed through to the child process unchanged.
#[tokio::test]
async fn argv_passthrough() {
    let d = default_dispatcher();
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![ECHO_ARGV.into(), "hello".into(), "world".into()],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 0);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello"), "stdout: {stdout:?}");
    assert!(stdout.contains("world"), "stdout: {stdout:?}");
    assert!(!out.stdout_truncated);
}

/// FR-01: a child with no extra args produces no output.
#[tokio::test]
async fn no_args_produces_empty_stdout() {
    let d = default_dispatcher();
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![ECHO_ARGV.into()],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 0);
    assert!(out.stdout.is_empty() || out.stdout == b"\n");
}

/// FR-03: nonzero exit code is propagated; no panic occurs.
#[tokio::test]
async fn nonzero_exit_does_not_panic() {
    let d = default_dispatcher();
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![EXIT_CODE.into(), "42".into()],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 42);
}

/// FR-03: exit code 1 is propagated correctly.
#[tokio::test]
async fn exit_code_one_propagated() {
    let d = default_dispatcher();
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![EXIT_CODE.into(), "1".into()],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 1);
}

/// FR-03: exit code 0 is propagated correctly.
#[tokio::test]
async fn exit_code_zero_propagated() {
    let d = default_dispatcher();
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![EXIT_CODE.into(), "0".into()],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 0);
}

/// FR-04: `cwd` is respected; the child sees the configured working directory.
#[tokio::test]
async fn cwd_respected() {
    let d = default_dispatcher();
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![ECHO_CWD.into()],
            cwd: Some("/tmp".into()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 0);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // /tmp may be canonicalized differently across distros; just verify it is
    // non-empty and contains "tmp" so the cwd was applied.
    assert!(
        stdout.trim().contains("tmp"),
        "expected cwd to be /tmp-ish, got: {stdout:?}"
    );
}

/// FR-01: env vars from `env_extra` are visible to the child.
#[tokio::test]
async fn env_var_passthrough() {
    let d = default_dispatcher();
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![ECHO_ENV.into(), "LOGICSHELL_TEST_VAR".into()],
            env_extra: vec![("LOGICSHELL_TEST_VAR".into(), "hello_env_value".into())],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 0);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello_env_value"), "stdout: {stdout:?}");
}

/// FR-01: multiple env vars are all injected.
#[tokio::test]
async fn multiple_env_vars_injected() {
    let d = default_dispatcher();
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![ECHO_ENV.into(), "LS_VAR_B".into()],
            env_extra: vec![
                ("LS_VAR_A".into(), "aaa".into()),
                ("LS_VAR_B".into(), "bbb".into()),
            ],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 0);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("bbb"), "stdout: {stdout:?}");
}

/// NFR-08: stdout capture is truncated at the configured limit.
#[tokio::test]
async fn stdout_truncated_at_limit() {
    let limit = 100u64;
    let d = Dispatcher::with_capture_limit(limit);
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![FLOOD_STDOUT.into(), "1000000".into()],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 0);
    assert_eq!(
        out.stdout.len() as u64,
        limit,
        "captured {} bytes, expected {limit}",
        out.stdout.len()
    );
    assert!(out.stdout_truncated, "expected stdout_truncated = true");
}

/// NFR-08: stdout is NOT truncated when output is within the limit.
#[tokio::test]
async fn stdout_not_truncated_when_within_limit() {
    let d = Dispatcher::with_capture_limit(1_000_000);
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![FLOOD_STDOUT.into(), "200".into()],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 0);
    assert_eq!(out.stdout.len(), 200);
    assert!(!out.stdout_truncated, "expected stdout_truncated = false");
}

/// NFR-08: zero capture limit means stdout is always truncated (edge case).
#[tokio::test]
async fn zero_capture_limit_truncates_all_stdout() {
    let d = Dispatcher::with_capture_limit(0);
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![FLOOD_STDOUT.into(), "10".into()],
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 0);
    assert!(out.stdout.is_empty());
    assert!(out.stdout_truncated);
}

/// FR-02: `StdinMode::Piped` feeds bytes to the child's stdin.
#[tokio::test]
async fn stdin_piped_mode_feeds_data() {
    let d = default_dispatcher();
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![STDIN_ECHO.into()],
            stdin: StdinMode::Piped(b"hello from stdin\n".to_vec()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 0);
    assert_eq!(out.stdout, b"hello from stdin\n");
}

/// FR-02: `StdinMode::Null` gives the child an immediate EOF on stdin.
#[tokio::test]
async fn stdin_null_mode_gives_eof() {
    let d = default_dispatcher();
    let out = d
        .dispatch(DispatchOptions {
            argv: vec![STDIN_ECHO.into()],
            stdin: StdinMode::Null,
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(out.exit_code, 0);
    assert!(
        out.stdout.is_empty(),
        "expected empty stdout for null stdin"
    );
}

/// NFR-06: dispatching a nonexistent binary returns Err, not a panic.
#[tokio::test]
async fn nonexistent_binary_returns_error() {
    let d = default_dispatcher();
    let result = d
        .dispatch(DispatchOptions {
            argv: vec!["/no/such/binary/logicshell_fixture_xyz".into()],
            ..Default::default()
        })
        .await;

    assert!(
        result.is_err(),
        "expected Err for nonexistent binary, got Ok"
    );
    assert!(
        matches!(
            result.unwrap_err(),
            logicshell_core::error::LogicShellError::Dispatch(_)
        ),
        "expected Dispatch error variant"
    );
}

/// NFR-06: empty argv returns a structured Dispatch error.
#[tokio::test]
async fn empty_argv_returns_dispatch_error() {
    let d = default_dispatcher();
    let result = d
        .dispatch(DispatchOptions {
            argv: vec![],
            ..Default::default()
        })
        .await;

    assert!(matches!(
        result,
        Err(logicshell_core::error::LogicShellError::Dispatch(_))
    ));
}
