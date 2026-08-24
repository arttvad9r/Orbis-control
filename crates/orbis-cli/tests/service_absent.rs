//! #119: executable service-absent integration coverage for `orbisctl status`.
//!
//! Runs the built binary with an explicitly unreachable session-bus address,
//! so no real session or system bus is ever contacted. The CLI must fail
//! closed: non-zero exit, the explicit connect error on stderr and no
//! synthesized status values on stdout.

use std::process::Command;

const ABSENT_BUS: &str = "unix:path=/nonexistent/orbisctl-service-absent-test";

fn run_status(args: &[&str]) -> (String, String, Option<i32>) {
    let output = Command::new(env!("CARGO_BIN_EXE_orbisctl"))
        .args(args)
        .env("DBUS_SESSION_BUS_ADDRESS", ABSENT_BUS)
        .env_remove("DBUS_STARTER_ADDRESS")
        .env_remove("DBUS_STARTER_BUS_TYPE")
        .output()
        .expect("spawn orbisctl");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

#[test]
fn status_fails_closed_when_session_bus_is_absent() {
    let (stdout, stderr, code) = run_status(&["status"]);
    assert_ne!(code, Some(0), "service-absent status must fail");
    assert!(
        stderr.contains("cannot connect to the user session bus"),
        "expected explicit connect error, got stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "no status values may be synthesized");

    let (stdout, stderr, code) = run_status(&["status", "--json"]);
    assert_ne!(code, Some(0), "service-absent --json must fail");
    assert!(stderr.contains("cannot connect to the user session bus"));
    assert!(
        !stdout.contains("schema_version"),
        "no versioned JSON may be synthesized"
    );
}
