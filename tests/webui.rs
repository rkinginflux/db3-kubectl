#![forbid(unsafe_code)]
//! End-to-end tests for `kubectl db3 webui`.
//!
//! Each test starts the real binary against a temporary fake `kubectl`/`helm`
//! on PATH, then talks HTTP to the loopback listener. No cluster is contacted.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, os::unix::fs::PermissionsExt};

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write_fixture(dir: &Path, name: &str, body: &str) {
    fs::write(dir.join(name), body).unwrap();
}

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn kubectl_script() -> &'static str {
    // A tiny dispatcher: the collector calls kubectl with JSON output.
    r#"#!/bin/sh
DIR="$(dirname "$0")"
args="$*"
case "$args" in
  *"version -o json"*) cat "$DIR/version.json" ;;
  *"config view"*) cat "$DIR/kubeconfig.json" ;;
  *"get nodes"*) cat "$DIR/nodes.json" ;;
  *"get namespaces"*) cat "$DIR/namespaces.json" ;;
  *"get pods"*) cat "$DIR/pods.json" ;;
  *"get statefulsets,deployments,daemonsets"*) cat "$DIR/workloads.json" ;;
  *"get services"*) cat "$DIR/services.json" ;;
  *"get endpointslices"*) cat "$DIR/endpointslices.json" ;;
  *"get storageclasses"*) printf '{"items":[]}' ;;
  *"get persistentvolumes"*) printf '{"items":[]}' ;;
  *"get persistentvolumeclaims"*) printf '{"items":[]}' ;;
  *"get networkpolicies"*) printf '{"items":[]}' ;;
  *"get events"*) cat "$DIR/events.json" ;;
  *"get configmaps"*) cat "$DIR/configmaps.json" ;;
  *"get secrets"*) cat "$DIR/secrets.json" ;;
  *"logs "*) printf 'line one\nline two token=%s\n' "$INFLUXDB3_TOKEN" ;;
  *) echo "fake kubectl: unsupported args: $args" >&2; exit 1 ;;
esac
"#
}

fn helm_script() -> &'static str {
    r#"#!/bin/sh
DIR="$(dirname "$0")"
case "$*" in
  *"list -A -o json"*) cat "$DIR/helm.json" ;;
  *) echo "fake helm: unsupported args: $*" >&2; exit 1 ;;
esac
"#
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    script(dir.path(), "kubectl", kubectl_script());
    script(dir.path(), "helm", helm_script());
    write_fixture(
        dir.path(),
        "version.json",
        r#"{"serverVersion":{"gitVersion":"v1.35.4","platform":"linux/amd64"}}"#,
    );
    write_fixture(
        dir.path(),
        "kubeconfig.json",
        r#"{"current-context":"lab","clusters":[{"name":"lab","cluster":{"server":"https://192.0.2.10:6443"}}]}"#,
    );
    write_fixture(
        dir.path(),
        "nodes.json",
        r#"{"items":[{"metadata":{"name":"node-1","creationTimestamp":"2026-10-20T00:00:00Z","labels":{"node-role.kubernetes.io/control-plane":""}},
            "status":{"capacity":{"cpu":"8","memory":"16335564Ki","pods":"110"},
                      "allocatable":{"cpu":"7900m","memory":"15000000Ki","pods":"110"},
                      "conditions":[{"type":"Ready","status":"True"}],
                      "addresses":[{"type":"InternalIP","address":"192.0.2.10"}],
                      "nodeInfo":{"kubeletVersion":"v1.35.4","osImage":"Ubuntu 24.04","kernelVersion":"6.8"}}}]}"#,
    );
    write_fixture(
        dir.path(),
        "namespaces.json",
        r#"{"items":[{"metadata":{"name":"influxdb3","creationTimestamp":"2026-10-20T00:00:00Z"},"status":{"phase":"Active"}}]}"#,
    );
    // One healthy ingester and one crash-looping processor.
    write_fixture(
        dir.path(),
        "pods.json",
        r#"{"items":[
          {"metadata":{"name":"db3-ingester-0","namespace":"influxdb3","creationTimestamp":"2026-10-20T00:00:00Z",
                       "labels":{"app.kubernetes.io/component":"ingester"},
                       "ownerReferences":[{"kind":"StatefulSet","name":"db3-ingester"}]},
           "spec":{"nodeName":"node-1","containers":[{"name":"influxdb3","image":"docker.io/influxdb:3.11.0-enterprise",
             "args":["serve","--mode=ingest","--cluster-id=lab","--object-store=s3"],
             "env":[{"name":"INFLUXDB3_LICENSE_TYPE","value":"commercial"},
                    {"name":"INFLUXDB3_LICENSE_FILE","valueFrom":{"secretKeyRef":{"name":"db3-license","key":"license-file"}}},
                    {"name":"AWS_SECRET_ACCESS_KEY","valueFrom":{"secretKeyRef":{"name":"db3-s3","key":"secret-key"}}}]}]},
           "status":{"phase":"Running","conditions":[{"type":"Ready","status":"True"}],
                     "containerStatuses":[{"name":"influxdb3","ready":true,"restartCount":0,"state":{"running":{}}}],
                     "startTime":"2026-10-20T00:00:00Z"}},
          {"metadata":{"name":"db3-processor-0","namespace":"influxdb3","creationTimestamp":"2026-10-20T00:00:00Z",
                       "labels":{"app.kubernetes.io/component":"processor"},
                       "ownerReferences":[{"kind":"StatefulSet","name":"db3-processor"}]},
           "spec":{"nodeName":"node-1","containers":[{"name":"influxdb3","image":"docker.io/influxdb:3.11.0-enterprise",
             "args":["serve","--mode=process","--cluster-id=lab"],"env":[]}]},
           "status":{"phase":"Running","conditions":[{"type":"Ready","status":"False"}],
                     "containerStatuses":[{"name":"influxdb3","ready":false,"restartCount":9,
                       "state":{"waiting":{"reason":"CrashLoopBackOff"}}}],
                     "startTime":"2026-10-20T00:00:00Z"}}]}"#,
    );
    write_fixture(
        dir.path(),
        "workloads.json",
        r#"{"items":[
          {"kind":"StatefulSet","metadata":{"name":"db3-ingester","namespace":"influxdb3","creationTimestamp":"2026-10-20T00:00:00Z"},
           "spec":{"replicas":2,"template":{"metadata":{"labels":{"app.kubernetes.io/component":"ingester"}},"spec":{"containers":[{"name":"influxdb3","image":"docker.io/influxdb:3.11.0-enterprise"}]}}},
           "status":{"readyReplicas":1,"updatedReplicas":1}},
          {"kind":"StatefulSet","metadata":{"name":"db3-processor","namespace":"influxdb3","creationTimestamp":"2026-10-20T00:00:00Z"},
           "spec":{"replicas":1,"template":{"metadata":{"labels":{"app.kubernetes.io/component":"processor"}},"spec":{"containers":[{"name":"influxdb3","image":"docker.io/influxdb:3.11.0-enterprise"}]}}},
           "status":{"readyReplicas":0,"updatedReplicas":0}}]}"#,
    );
    write_fixture(
        dir.path(),
        "services.json",
        r#"{"items":[{"metadata":{"name":"db3-query","namespace":"influxdb3","creationTimestamp":"2026-10-20T00:00:00Z"},
            "spec":{"type":"ClusterIP","clusterIP":"10.43.0.10","selector":{"app.kubernetes.io/component":"ingester"},
                    "ports":[{"name":"http","port":8181,"targetPort":8181,"protocol":"TCP"}]}}]}"#,
    );
    write_fixture(
        dir.path(),
        "endpointslices.json",
        r#"{"items":[{"metadata":{"name":"db3-query-abc","namespace":"influxdb3","labels":{"kubernetes.io/service-name":"db3-query"}},
            "endpoints":[{"addresses":["10.42.0.9"],"conditions":{"ready":true},"targetRef":{"kind":"Pod","name":"db3-ingester-0"}}]}]}"#,
    );
    write_fixture(
        dir.path(),
        "events.json",
        r#"{"items":[{"metadata":{"name":"e1","namespace":"influxdb3","creationTimestamp":"2026-10-26T00:00:00Z"},
            "type":"Warning","reason":"BackOff","involvedObject":{"kind":"Pod","name":"db3-processor-0"},
            "message":"Back-off restarting failed container","count":9,"lastTimestamp":"2026-10-26T00:05:00Z"}]}"#,
    );
    write_fixture(
        dir.path(),
        "configmaps.json",
        r#"{"items":[{"metadata":{"name":"db3-config","namespace":"influxdb3","creationTimestamp":"2026-10-20T00:00:00Z"},
            "data":{"INFLUXDB3_BUCKET":"lab","INFLUXDB3_LICENSE_JWT":"super-secret-license-value"}}]}"#,
    );
    write_fixture(
        dir.path(),
        "secrets.json",
        r#"{"items":[{"metadata":{"name":"db3-s3","namespace":"influxdb3","creationTimestamp":"2026-10-20T00:00:00Z"},
            "type":"Opaque","data":{"AWS_ACCESS_KEY_ID":"c2VjcmV0LWFjY2Vzcw==","AWS_SECRET_ACCESS_KEY":"c2VjcmV0LWtleQ=="}}]}"#,
    );
    write_fixture(
        dir.path(),
        "helm.json",
        r#"[{"name":"db3","namespace":"influxdb3","revision":"3","updated":"2026-10-24 10:00:00 +0000 UTC",
            "status":"deployed","chart":"influxdb3-enterprise-0.4.2","app_version":"3.11.0"}]"#,
    );
    dir
}

fn env_command(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kubectl-db3"));
    // Keep the fixture first so it shadows any real kubectl, while coreutils
    // (cat, dirname) stay reachable for the tiny fake scripts.
    command
        .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
        .env_remove("INFLUXDB3_TOKEN")
        .current_dir(dir);
    command
}

fn start_with(dir: &Path, extra: &[&str], envs: &[(&str, &str)]) -> Server {
    let mut command = env_command(dir);
    command
        .args(["webui", "--port", "0", "--address", "127.0.0.1"])
        .args(extra)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take().expect("piped stdout");
    let mut reader = BufReader::new(stdout);
    let mut port = None;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        if let Some(rest) = line.split("URL").nth(1) {
            if let Some(address) = rest.split_whitespace().next() {
                if let Ok(value) = address.rsplit(':').next().unwrap_or_default().parse() {
                    port = Some(value);
                    break;
                }
            }
        }
    }
    // Keep the pipe drained so a chatty shutdown cannot block the child.
    std::thread::spawn(move || {
        let mut sink = String::new();
        let _ = reader.read_to_string(&mut sink);
    });
    let port = port.expect("server printed its listening URL");
    Server { child, port }
}

fn start(dir: &Path, extra: &[&str]) -> Server {
    start_with(dir, extra, &[])
}

fn http(port: u16, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to console");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    let status = raw
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let body = raw
        .split_once("\r\n\r\n")
        .map_or_else(String::new, |(_, body)| body.to_owned());
    (status, body)
}

fn snapshot(port: u16) -> Value {
    let (status, body) = http(port, "/api/snapshot");
    assert_eq!(status, 200, "snapshot should be served: {body}");
    serde_json::from_str(&body).expect("snapshot is JSON")
}

#[test]
fn serves_app_shell_and_redacted_snapshot() {
    let dir = fixture();
    let server = start(dir.path(), &[]);

    let (status, page) = http(server.port, "/");
    assert_eq!(status, 200);
    assert!(page.contains("Environment Console"));
    assert!(page.contains("id=\"root\""));

    let value = snapshot(server.port);
    assert_eq!(value["context"], "lab");
    assert_eq!(value["cluster"]["version"], "v1.35.4");
    assert_eq!(value["nodes"][0]["ready"], true);
    assert_eq!(value["nodes"][0]["pods_used"], 2);

    let clusters = value["db3"].as_array().unwrap();
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0]["cluster_id"], "lab");
    assert_eq!(clusters[0]["object_store"], "s3");
    assert_eq!(clusters[0]["license"]["token_present"], true);
    let roles = clusters[0]["roles"].as_array().unwrap();
    let ingester = roles
        .iter()
        .find(|role| role["role"] == "ingester")
        .unwrap();
    assert_eq!(ingester["desired"], 2);
    assert_eq!(ingester["ready"], 1);
    assert_eq!(ingester["workload"], "db3-ingester");

    // The degraded processor must surface as an actionable finding.
    let issues = value["issues"].as_array().unwrap();
    assert!(issues.iter().any(|issue| {
        issue["severity"] == "critical" && issue["title"] == "Pod db3-processor-0 is not Ready"
    }));
    assert!(issues.iter().any(|issue| {
        issue["title"]
            .as_str()
            .unwrap_or_default()
            .contains("restarted 9 times")
    }));

    // Service selector matching links the service to its component.
    assert_eq!(value["services"][0]["component"], "ingester");
    assert_eq!(value["services"][0]["endpoints"][0]["ready"], true);

    // Secret material must never be in the payload; key names are.
    let raw = value.to_string();
    assert!(!raw.contains("c2VjcmV0LWFjY2Vzcw"));
    assert!(!raw.contains("super-secret-license-value"));
    assert_eq!(value["secrets"][0]["keys"][0], "AWS_ACCESS_KEY_ID");
    assert_eq!(
        value["configmaps"][0]["data"]["INFLUXDB3_LICENSE_JWT"],
        "[redacted]"
    );
    assert_eq!(value["configmaps"][0]["data"]["INFLUXDB3_BUCKET"], "lab");
    assert_eq!(value["secret_values_exposed"], false);

    // Helm is read as its own program, not through kubectl.
    assert_eq!(value["helm"][0]["name"], "db3");
    assert_eq!(value["helm"][0]["chart"], "influxdb3-enterprise-0.4.2");
}

#[test]
fn logs_are_bounded_and_tokens_are_redacted() {
    let dir = fixture();
    let token = "apiv3_test-token-value";
    let server = start_with(dir.path(), &[], &[("INFLUXDB3_TOKEN", token)]);

    let (status, body) = http(
        server.port,
        "/api/logs?namespace=influxdb3&pod=db3-ingester-0&tail=50",
    );
    assert_eq!(status, 200);
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["pod"], "db3-ingester-0");
    assert_eq!(value["tail"], 50);
    assert_eq!(value["redacted"], true);
    let lines = value["lines"].as_str().unwrap();
    assert!(lines.contains("line one"), "{lines}");
    assert!(!lines.contains(token), "token must be redacted: {lines}");
    assert!(lines.contains("[REDACTED]"));

    // Argument injection and missing pods are rejected before kubectl runs.
    let (status, body) = http(server.port, "/api/logs?namespace=influxdb3&pod=--token");
    assert_eq!(status, 400);
    assert!(body.contains("invalid pod name"), "{body}");
    let (status, _) = http(server.port, "/api/logs?namespace=influxdb3");
    assert_eq!(status, 400);

    // Unknown API paths stay JSON, unknown pages serve the app shell.
    let (status, body) = http(server.port, "/api/nope");
    assert_eq!(status, 404);
    assert!(body.contains("unknown endpoint"));
    let (status, body) = http(server.port, "/some/deep/link");
    assert_eq!(status, 200);
    assert!(body.contains("Environment Console"));
}

#[test]
fn manifest_flag_prints_rbac_and_exits() {
    let dir = tempfile::tempdir().unwrap();
    let output = env_command(dir.path())
        .args(["webui", "--manifest"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let manifest = String::from_utf8_lossy(&output.stdout);
    assert!(manifest.contains("kind: ClusterRole"));
    assert!(manifest.contains("name: db3-webui-readonly"));
    assert!(manifest.contains("pods/log"));
    assert!(manifest.contains("kind: ServiceAccount"));

    // The same document is available over HTTP.
    let fixture = fixture();
    let server = start(fixture.path(), &[]);
    let (status, body) = http(server.port, "/api/manifest");
    assert_eq!(status, 200);
    assert!(body.contains("db3-webui-readonly"));
}

#[test]
fn collector_failures_degrade_without_failing_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    script(
        dir.path(),
        "kubectl",
        "#!/bin/sh\necho 'The connection to the server localhost:8080 was refused' >&2\nexit 1\n",
    );
    script(dir.path(), "helm", "#!/bin/sh\nexit 1\n");
    let server = start(dir.path(), &[]);

    let value = snapshot(server.port);
    let collectors = value["collectors"].as_array().unwrap();
    assert!(collectors.len() >= 14);
    assert!(collectors.iter().all(|collector| collector["ok"] == false));
    assert!(collectors.iter().any(|collector| {
        collector["error"]
            .as_str()
            .unwrap_or_default()
            .contains("exited with status 1")
    }));
    assert!(value["db3"].as_array().unwrap().is_empty());
    assert!(value["nodes"].as_array().unwrap().is_empty());

    // A second console on the same port must fail clearly, not silently.
    let second = env_command(dir.path())
        .args(["webui", "--port", &server.port.to_string()])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&second.stderr).to_lowercase();
    assert!(!second.status.success());
    assert!(
        stderr.contains("could not start the console"),
        "unexpected stderr: {stderr}"
    );
    assert!(stderr.contains("--port"));
}

#[test]
fn scoping_flags_and_health_are_reflected() {
    let dir = fixture();
    let server = start(
        dir.path(),
        &[
            "--no-secrets",
            "--namespace",
            "influxdb3",
            "--refresh",
            "30",
        ],
    );

    let (status, body) = http(server.port, "/api/health");
    assert_eq!(status, 200);
    let health: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(health["ok"], true);
    assert_eq!(health["version"], env!("CARGO_PKG_VERSION"));

    let value = snapshot(server.port);
    assert_eq!(value["namespace_filter"], "influxdb3");
    // --no-secrets means the collector never asks for Secret objects.
    assert!(value["secrets"].as_array().unwrap().is_empty());
    let config = value["collectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|collector| collector["name"] == "configuration")
        .unwrap();
    assert_eq!(config["ok"], true);
    assert_eq!(config["count"], 1);

    // Default port is documented as 9191 and is overridable.
    let help = env_command(dir.path())
        .args(["webui", "--help"])
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&help.stdout);
    assert!(help.contains("[default: 9191]"), "{help}");
    assert!(help.contains("--port <PORT>"));
}

#[test]
fn ctrl_c_stops_the_console_and_releases_the_port() {
    let dir = fixture();
    let mut server = start(dir.path(), &[]);
    let port = server.port;
    assert_eq!(http(port, "/api/health").0, 200);

    let signal = Command::new("kill")
        .args(["-INT", &server.child.id().to_string()])
        .status()
        .unwrap();
    assert!(signal.success(), "could not deliver SIGINT");

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut exited = false;
    while Instant::now() < deadline {
        if server.child.try_wait().unwrap().is_some() {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(exited, "console did not exit after SIGINT");

    let mut closed = false;
    for _ in 0..20 {
        if TcpStream::connect(("127.0.0.1", port)).is_err() {
            closed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        closed,
        "port {port} was still accepting connections after exit"
    );
}
