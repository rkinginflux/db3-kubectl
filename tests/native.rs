#![forbid(unsafe_code)]
use serde_json::Value;
use std::process::Command;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

fn fixture(script: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kubectl");
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    dir
}
fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kubectl-db3"))
        .args(args)
        .env("PATH", dir)
        .env_remove("INFLUXDB3_TOKEN")
        .env_remove("INFLUXDB3_POD")
        .current_dir(dir)
        .output()
        .unwrap()
}
fn manifest(dir: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(dir.join("bundle/manifest.json")).unwrap()).unwrap()
}
#[test]
fn collects_private_json_bundle() {
    let dir = fixture("printf '{\"ok\":true}\\n'");
    let result = run(
        dir.path(),
        &["support", "--pod", "db3-0", "--output", "bundle"],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let m = manifest(dir.path());
    assert_eq!(m["complete"], true);
    assert_eq!(m["results"].as_array().unwrap().len(), 3);
    assert_eq!(
        fs::metadata(dir.path().join("bundle"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for entry in fs::read_dir(dir.path().join("bundle")).unwrap() {
        assert_eq!(
            entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn logs_defaults_to_all_and_preserves_shared_options() {
    let dir = fixture(
        "case \"$*\" in *'get pods'*) printf '{\"items\":[{\"metadata\":{\"name\":\"querier-0\"}},{\"metadata\":{\"name\":\"processor-0\"}}]}' ;; *) printf 'pod line\\n' ;; esac",
    );
    let result = run(
        dir.path(),
        &[
            "support", "-n", "chosen", "--output", "bundle", "logs", "--tail", "-1",
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let m = manifest(dir.path());
    assert_eq!(m["role"], "all");
    assert_eq!(m["namespace"], "chosen");
    assert_eq!(m["tail"], -1);
    assert_eq!(
        m["requested_roles"],
        serde_json::json!(["querier", "processor", "ingester", "compactor"])
    );
    assert_eq!(m["results"].as_array().unwrap().len(), 2);
}

#[test]
fn timeout_preserves_partial_streams_and_continues() {
    let dir = fixture(
        "case \"$*\" in *'show license info'*) printf partial; printf diagnostic >&2; /bin/sleep 2 ;; *) printf '{\"ok\":true}\\n' ;; esac",
    );
    let result = run(
        dir.path(),
        &[
            "support",
            "--pod",
            "p",
            "--output",
            "bundle",
            "--timeout",
            "0.05",
        ],
    );
    assert_eq!(result.status.code(), Some(1));
    let m = manifest(dir.path());
    assert_eq!(m["results"][0]["ok"], true);
    assert_eq!(m["results"][1]["returncode"], Value::Null);
    assert!(
        m["results"][1]["error"]
            .as_str()
            .unwrap()
            .contains("timed out")
    );
    assert_eq!(m["results"][2]["ok"], true);
    assert_eq!(
        fs::read_to_string(dir.path().join("bundle/license-info.stdout.txt")).unwrap(),
        "partial"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("bundle/license-info.stderr.txt")).unwrap(),
        "diagnostic"
    );
}

#[test]
fn invalid_json_does_not_prevent_other_results() {
    let dir = fixture(
        "case \"$*\" in *'show license info'*) printf 'not JSON' ;; *) printf 'null' ;; esac",
    );
    assert_eq!(
        run(dir.path(), &["support", "--pod", "p", "--output", "bundle"])
            .status
            .code(),
        Some(1)
    );
    let m = manifest(dir.path());
    assert_eq!(m["results"][1]["error"], "command returned invalid JSON");
    assert_eq!(m["results"][2]["ok"], true);
}

#[test]
fn existing_output_is_unchanged() {
    let dir = fixture("printf '{}'");
    fs::create_dir(dir.path().join("bundle")).unwrap();
    fs::write(dir.path().join("bundle/marker"), "original").unwrap();
    assert_eq!(
        run(dir.path(), &["support", "logs", "--output", "bundle"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("bundle/marker")).unwrap(),
        "original"
    );
    assert!(!dir.path().join("bundle/manifest.json").exists());
}

#[test]
fn options_after_logs_override_before_logs() {
    let dir = fixture("printf '{\"items\":[]}'");
    let result = run(
        dir.path(),
        &[
            "support",
            "-n",
            "first",
            "--output",
            "ignored",
            "--timeout",
            "10",
            "logs",
            "querier",
            "-n",
            "second",
            "--output",
            "bundle",
            "--timeout",
            "1",
        ],
    );
    assert_eq!(result.status.code(), Some(1));
    let m = manifest(dir.path());
    assert_eq!(m["namespace"], "second");
    assert_eq!(m["requested_roles"], serde_json::json!(["querier"]));
    assert!(!dir.path().join("ignored").exists());
}

#[test]
fn invalid_arguments_are_usage_errors_without_bundle() {
    let dir = fixture("exit 99");
    for args in [
        vec!["support", "--timeout", "nan", "logs"],
        vec!["support", "logs", "--timeout", "0"],
        vec!["support", "logs", "--tail", "-2"],
        vec!["support", "logs", "unknown"],
        vec!["support", "logs", "--pod", "p"],
    ] {
        assert_eq!(run(dir.path(), &args).status.code(), Some(2), "{args:?}");
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn large_token_with_unread_stdin_still_times_out() {
    let dir = fixture("printf partial; exec /bin/sleep 2");
    let result = Command::new(env!("CARGO_BIN_EXE_kubectl-db3"))
        .args([
            "support",
            "--pod",
            "p",
            "--output",
            "bundle",
            "--timeout",
            "0.03",
        ])
        .env("INFLUXDB3_TOKEN", "x".repeat(100_000))
        .env("PATH", dir.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let m = manifest(dir.path());
    assert!(
        m["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["error"].as_str().unwrap().contains("timed out"))
    );
}

#[test]
fn token_is_stdin_only_and_redacted_from_streams() {
    let dir = fixture(
        "printf '%s\\n' \"$@\" > argv; IFS= read -r token; printf '{\"echo\":\"%s\"}' \"$token\"; printf '%s' \"$token\" >&2",
    );
    let result = Command::new(env!("CARGO_BIN_EXE_kubectl-db3"))
        .args(["support", "--pod", "p", "--output", "bundle"])
        .env("INFLUXDB3_TOKEN", "apiv3_literal-$(not-executed)")
        .env("PATH", dir.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        !fs::read_to_string(dir.path().join("argv"))
            .unwrap()
            .contains("apiv3_literal")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("bundle/nodes.json")).unwrap(),
        "{\"echo\":\"[REDACTED]\"}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("bundle/nodes.stderr.txt")).unwrap(),
        "[REDACTED]"
    );
}

#[test]
fn missing_kubectl_is_recorded_for_each_command() {
    let dir = tempfile::tempdir().unwrap();
    let result = run(dir.path(), &["support", "--pod", "p", "--output", "bundle"]);
    assert_eq!(result.status.code(), Some(1));
    let m = manifest(dir.path());
    assert_eq!(m["results"].as_array().unwrap().len(), 3);
    assert!(
        m["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["returncode"].is_null() && r["ok"] == false)
    );
}

#[test]
fn missing_pod_is_usage_error() {
    let result = Command::new(env!("CARGO_BIN_EXE_kubectl-db3"))
        .arg("support")
        .env_remove("INFLUXDB3_POD")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("--pod"));
}
