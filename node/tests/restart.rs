#![cfg(target_os = "linux")]

use std::{fs, os::unix::fs::PermissionsExt, process::Command};

fn restart(args: &[&str], fail: bool) -> (std::process::Output, String) {
    let temp = tempfile::tempdir().unwrap();
    let systemctl = temp.path().join("systemctl");
    fs::write(
        &systemctl,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
    exit 0
fi
printf '%s\n' "$@" > "$RESTART_TEST_CALLS"
if [ "$RESTART_TEST_FAIL" = "1" ]; then
    echo 'restart denied' >&2
    exit 1
fi
"#,
    )
    .unwrap();
    fs::set_permissions(&systemctl, fs::Permissions::from_mode(0o755)).unwrap();
    let calls = temp.path().join("calls");
    let output = Command::new(env!("CARGO_BIN_EXE_node"))
        .arg("restart")
        .args(args)
        .env("PATH", temp.path())
        .env("RESTART_TEST_CALLS", &calls)
        .env("RESTART_TEST_FAIL", if fail { "1" } else { "0" })
        .output()
        .unwrap();
    (output, fs::read_to_string(calls).unwrap_or_default())
}

#[test]
fn restarts_default_systemd_unit() {
    let (output, calls) = restart(&[], false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(calls, "restart\n--\noxiproxy-node.service\n");
    assert!(String::from_utf8_lossy(&output.stdout).contains("已重启"));
}

#[test]
fn restarts_custom_systemd_unit_without_splitting_arguments() {
    let (output, calls) = restart(&["--service-name", "custom-node"], false);
    assert!(output.status.success());
    assert_eq!(calls, "restart\n--\ncustom-node.service\n");
}

#[test]
fn propagates_systemd_failure_without_reporting_success() {
    let (output, _) = restart(&[], true);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("restart denied"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("已重启"));
}

#[test]
fn missing_systemctl_fails_without_starting_another_process() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_node"))
        .arg("restart")
        .env("PATH", temp.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("未找到 systemctl"));
}
