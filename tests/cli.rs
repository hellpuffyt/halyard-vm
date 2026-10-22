//! The `halyard` binary end to end.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn halyard(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_halyard"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("halyard runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

/// Regression: `--trace` recorded the trace but only printed it when the
/// undocumented `HALYARD_TRACE` environment variable was also set.
#[test]
fn trace_flag_prints_the_trace() {
    let out = halyard(&["run", "examples/hello.hasm", "--trace"], "");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("trace: [0, 1, 2, 3]"), "{stderr}");
    let quiet = halyard(&["run", "examples/hello.hasm"], "");
    assert!(!String::from_utf8_lossy(&quiet.stderr).contains("trace:"));
}

/// Regression: `m A N` in the debugger computed `A + N` unchecked, which
/// panics (debug builds) for addresses near `usize::MAX`.
#[test]
fn debugger_memory_dump_handles_huge_addresses() {
    let out = halyard(
        &["debug", "examples/hello.hasm"],
        "m 18446744073709551615 16\nm 0 4\nq\n",
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("000000  "), "{stdout}");
}

#[test]
fn exit_codes_match_the_readme() {
    assert_eq!(
        halyard(&["run", "examples/hello.hasm"], "").status.code(),
        Some(0)
    );
    assert_eq!(
        halyard(&["run", "examples/guarded.hasm", "--gas", "5000"], "")
            .status
            .code(),
        Some(3)
    );
}
