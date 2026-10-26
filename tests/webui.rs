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
printf '%s\n' "$*" >> "$DIR/kubectl-calls"
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
  *"logs "*)
    printf 'line one\nline two token=%s\n' "$INFLUXDB3_TOKEN" ;;
  *"db3-query query"*)
    case "$args" in
      *FAILME*) echo "query failed: unknown measurement" >&2 ; exit 3 ;;
    esac
    cat "$DIR/query.json" ;;
  *) echo "fake kubectl: unsupported args: $args" >&2; exit 1 ;;
esac
"#
}

fn helm_script() -> &'static str {
    r#"#!/bin/sh
DIR="$(dirname "$0")"
printf '%s\n' "$*" >> "$DIR/helm-calls"
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
    write_fixture(
        dir.path(),
        "query.json",
        r#"[{"time":"2026-01-01T00:00:00Z","value":1.5},{"time":"2026-01-01T01:00:00Z","value":2.5}]"#,
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
    let host = format!("Host: 127.0.0.1:{port}\r\n");
    let (_, page) = raw_http(port, "/", &host);
    let session = page
        .split("name=\"db3-session\" content=\"")
        .nth(1)
        .expect("session bootstrap")
        .split('"')
        .next()
        .unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to console");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-DB3-Session: {session}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let headers = std::str::from_utf8(&raw[..split]).unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    let mut bytes = raw[split + 4..].to_vec();
    if headers
        .to_lowercase()
        .contains("transfer-encoding: chunked")
    {
        let mut rest = bytes.as_slice();
        let mut decoded = Vec::new();
        loop {
            let split = rest.windows(2).position(|w| w == b"\r\n").unwrap();
            let line = std::str::from_utf8(&rest[..split]).unwrap();
            let length = usize::from_str_radix(line.split(';').next().unwrap(), 16).unwrap();
            if length == 0 {
                break;
            }
            let data = &rest[split + 2..];
            decoded.extend_from_slice(&data[..length]);
            rest = &data[length + 2..];
        }
        bytes = decoded;
    }
    let body = String::from_utf8(bytes).unwrap();
    (status, body)
}

fn raw_http(port: u16, path: &str, headers: &str) -> (u16, String) {
    raw_method(port, "GET", path, headers)
}

fn raw_method(port: u16, method: &str, path: &str, headers: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\n{headers}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = String::new();
    stream.read_to_string(&mut raw).unwrap();
    (raw.split_whitespace().nth(1).unwrap().parse().unwrap(), raw)
}

#[test]
fn authority_and_session_guard_all_api_requests() {
    let dir = fixture();
    let server = start(dir.path(), &[]);
    let host = format!("Host: 127.0.0.1:{}\r\n", server.port);
    assert_eq!(raw_http(server.port, "/api/health", &host).0, 401);
    assert_eq!(
        raw_http(server.port, "/", "Host: attacker.example\r\n").0,
        403
    );
    assert_eq!(
        raw_http(
            server.port,
            "/",
            &format!("{host}Origin: https://attacker.example\r\n")
        )
        .0,
        403
    );
    let (_, page) = raw_http(server.port, "/", &host);
    assert!(page.contains("name=\"db3-session\""));
    assert!(!page.contains("__DB3_SESSION__"));
    assert!(page.contains("script-src 'nonce-"));
    assert!(!page.contains("script-src 'unsafe-inline'"));
    assert_eq!(
        page.matches("nonce=\"").count(),
        1,
        "inject nonce only into the real script element, not React's script-string literals"
    );
    let session = page
        .split("name=\"db3-session\" content=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert_eq!(
        page.matches(session).count(),
        1,
        "session must occur only in the bootstrap meta, never in JS comparisons"
    );
}

#[test]
fn default_never_collects_secrets_and_hides_opaque_config() {
    let dir = fixture();
    script(
        dir.path(),
        "kubectl",
        &kubectl_script().replace(
            "cat \"$DIR/secrets.json\"",
            "touch \"$DIR/secret-requested\"; cat \"$DIR/secrets.json\"",
        ),
    );
    let mut pods: Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("pods.json")).unwrap()).unwrap();
    pods["items"][0]["spec"]["containers"][0]["env"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"name":"INNOCENT", "value":"opaque-private-value"}));
    fs::write(dir.path().join("pods.json"), pods.to_string()).unwrap();
    let server = start(dir.path(), &[]);
    let value = snapshot(server.port);
    assert!(value["secrets"].as_array().unwrap().is_empty());
    assert!(!dir.path().join("secret-requested").exists());
    assert!(!value.to_string().contains("opaque-private-value"));
    assert_eq!(
        value["configmaps"][0]["data"]["INFLUXDB3_BUCKET"],
        "[redacted]"
    );
    assert_eq!(value["db3"][0]["object_store"], "s3");
    let output = env_command(dir.path())
        .args(["webui", "--address", "0.0.0.0", "--port", "0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn active_collection_shutdown_kills_child_groups_and_reaps() {
    let dir = fixture();
    script(
        dir.path(),
        "kubectl",
        "#!/bin/sh\nDIR=$(dirname \"$0\")\nsleep 60 &\necho \"$$ $!\" >> \"$DIR/pids\"\nwait\n",
    );
    let mut server = start(dir.path(), &[]);
    let port = server.port;
    let request = std::thread::spawn(move || http(port, "/api/snapshot"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !dir.path().join("pids").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(dir.path().join("pids").exists());
    let pids = fs::read_to_string(dir.path().join("pids")).unwrap();
    Command::new("kill")
        .args(["-TERM", &server.child.id().to_string()])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        server.child.try_wait().unwrap().is_some(),
        "shutdown must be bounded"
    );
    let _ = request.join();
    for pid in pids.split_whitespace() {
        let status = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        assert!(
            status.is_empty() || status.split_whitespace().nth(2) == Some("Z"),
            "child still running: {pid}: {status}"
        );
    }
}

#[test]
fn incomplete_http_headers_expire() {
    let dir = fixture();
    let server = start(dir.path(), &[]);
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\nHost:").unwrap();
    let mut bytes = [0; 256];
    assert!(
        stream.read(&mut bytes).is_ok(),
        "incomplete headers must time out server-side"
    );
}

#[test]
fn collector_output_is_bounded() {
    let dir = fixture();
    script(
        dir.path(),
        "kubectl",
        "#!/bin/sh\nhead -c 4194305 /dev/zero\n",
    );
    let server = start(dir.path(), &["--timeout", "0.5"]);
    let value = snapshot(server.port);
    assert!(
        value["collectors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["error"].as_str().unwrap_or_default().contains("limit"))
    );
    assert!(!value.to_string().contains("opaque-private-value"));
}

#[test]
fn successful_collectors_clean_up_background_descendants() {
    let dir = fixture();
    script(
        dir.path(),
        "kubectl",
        "#!/bin/sh\nDIR=$(dirname \"$0\")\nsleep 60 </dev/null >/dev/null 2>&1 &\necho $! >> \"$DIR/background-pids\"\nprintf '{\"items\":[]}'\n",
    );
    let server = start(dir.path(), &[]);
    let _ = snapshot(server.port);
    let pids = fs::read_to_string(dir.path().join("background-pids")).unwrap();
    for pid in pids.split_whitespace() {
        let status = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        assert!(
            status.is_empty() || status.split_whitespace().nth(2) == Some("Z"),
            "successful collector left descendant alive: {pid}"
        );
    }
}

#[test]
fn resource_budgets_are_visible_and_bound_derived_snapshot() {
    let dir = fixture();
    let pod: Value =
        serde_json::from_str::<Value>(&fs::read_to_string(dir.path().join("pods.json")).unwrap())
            .unwrap()["items"][0]
            .clone();
    fs::write(
        dir.path().join("pods.json"),
        serde_json::json!({"items": vec![pod; 1000]}).to_string(),
    )
    .unwrap();
    let server = start(dir.path(), &[]);
    let value = snapshot(server.port);
    assert!(value["pods"].as_array().unwrap().len() <= 500);
    assert!(value["issues"].as_array().unwrap().iter().any(|i| {
        i["title"]
            .as_str()
            .unwrap_or_default()
            .contains("truncated")
    }));
}

#[test]
fn timeout_kills_and_reaps_collector_group_without_stderr_leak() {
    let dir = fixture();
    script(
        dir.path(),
        "kubectl",
        "#!/bin/sh\nDIR=$(dirname \"$0\")\nsleep 60 &\necho \"$$ $!\" > \"$DIR/timeout-pids\"\necho sensitive-metadata-key-value >&2\nwait\n",
    );
    let server = start(dir.path(), &["--timeout", "0.1"]);
    let (status, body) = http(server.port, "/api/logs?pod=pod-0");
    assert_eq!(status, 200);
    assert!(body.contains("timed out"));
    assert!(!body.contains("sensitive-metadata"));
    let pids = fs::read_to_string(dir.path().join("timeout-pids")).unwrap();
    for (index, pid) in pids.split_whitespace().enumerate() {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
        if index == 0 {
            assert!(stat.is_empty(), "direct child not reaped");
        } else {
            assert!(stat.is_empty() || stat.split_whitespace().nth(2) == Some("Z"));
        }
    }
}

#[test]
fn all_api_routes_enforce_session_methods_and_actual_port() {
    let dir = fixture();
    let server = start(dir.path(), &[]);
    let host = format!("Host: 127.0.0.1:{}\r\n", server.port);
    let (_, page) = raw_http(server.port, "/", &host);
    let token = page
        .split("name=\"db3-session\" content=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert_eq!(token.len(), 64);
    assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
    for path in [
        "/api/health",
        "/api/snapshot",
        "/api/logs?pod=pod-0",
        "/api/manifest",
        "/api/unknown",
    ] {
        assert_eq!(raw_http(server.port, path, &host).0, 401);
        assert_eq!(
            raw_http(
                server.port,
                path,
                &format!("{host}X-DB3-Session: wrong\r\n")
            )
            .0,
            401
        );
    }
    let headers = format!(
        "{host}X-DB3-Session: {token}\r\nOrigin: http://127.0.0.1:{}\r\n",
        server.port
    );
    let (status, response) = raw_http(server.port, "/api/health", &headers);
    assert_eq!(status, 200);
    assert!(response.to_lowercase().contains("cache-control: no-store"));
    assert!(response.to_lowercase().contains("x-frame-options: deny"));
    assert!(
        !response
            .to_lowercase()
            .contains("access-control-allow-origin")
    );
    assert_eq!(
        raw_method(server.port, "POST", "/api/health", &headers).0,
        405
    );
    assert_eq!(
        raw_method(server.port, "HEAD", "/api/health", &headers).0,
        200
    );
    assert_eq!(raw_http(server.port, "/", "Host: 127.0.0.1:1\r\n").0, 403);
    let second = start(dir.path(), &[]);
    let (_, second_page) = raw_http(
        second.port,
        "/",
        &format!("Host: 127.0.0.1:{}\r\n", second.port),
    );
    assert!(
        !second_page.contains(token),
        "session must be distinct per run"
    );
}

fn snapshot(port: u16) -> Value {
    let (status, body) = http(port, "/api/snapshot");
    assert_eq!(status, 200, "snapshot should be served: {body}");
    serde_json::from_str(&body).expect("snapshot is JSON")
}

#[test]
fn serves_app_shell_and_redacted_snapshot() {
    let dir = fixture();
    let server = start(dir.path(), &["--include-secret-metadata", "--helm"]);

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
    assert_eq!(
        value["configmaps"][0]["data"]["INFLUXDB3_BUCKET"],
        "[redacted]"
    );
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
fn query_terminal_is_opt_in() {
    let dir = fixture();
    // A token alone must not enable it.
    let server = start_with(dir.path(), &[], &[("INFLUXDB3_TOKEN", "apiv3_off")]);
    let (status, body) = http(
        server.port,
        "/api/query?namespace=influxdb3&pod=db3-ingester-0&database=mydb&q=SELECT%201",
    );
    assert_eq!(status, 403, "{body}");
    assert!(body.contains("disabled"), "{body}");
}

#[test]
fn query_terminal_returns_rows_and_keeps_the_token_off_argv() {
    let dir = fixture();
    let token = "apiv3_query-secret-token";
    let server = start_with(dir.path(), &["--query"], &[("INFLUXDB3_TOKEN", token)]);

    let (status, body) = http(
        server.port,
        "/api/query?namespace=influxdb3&pod=db3-ingester-0&database=mydb&language=sql&q=SELECT%20*%20FROM%20home",
    );
    assert_eq!(status, 200, "{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["database"], "mydb");
    assert_eq!(value["language"], "sql");
    assert_eq!(value["row_count"], 2);
    assert_eq!(value["truncated"], false);
    assert_eq!(value["redacted"], true);
    assert_eq!(value["columns"][0], "time");
    assert_eq!(value["rows"][1]["value"], 2.5);
    assert!(
        !body.contains(token),
        "the admin token must never reach the payload"
    );

    // The remote command shape, and proof the token travels on stdin only.
    let calls = fs::read_to_string(dir.path().join("kubectl-calls")).unwrap();
    assert!(
        calls.contains("exec -i db3-ingester-0 -n influxdb3"),
        "{calls}"
    );
    assert!(calls.contains("--database mydb"), "{calls}");
    assert!(calls.contains("--language sql"), "{calls}");
    assert!(calls.contains("--format json"), "{calls}");
    assert!(
        !calls.contains(token),
        "the token must travel on stdin, never in argv: {calls}"
    );
}

#[test]
fn query_terminal_rejects_non_read_only_statements_before_running_kubectl() {
    let dir = fixture();
    let server = start_with(dir.path(), &["--query"], &[("INFLUXDB3_TOKEN", "apiv3_t")]);
    for statement in [
        "DROP%20TABLE%20home",
        "INSERT%20INTO%20home%20VALUES%20(1)",
        "SELECT%201%3B%20DROP%20TABLE%20home",
        "SELECT%20*%20INTO%20copy%20FROM%20home",
    ] {
        let (status, body) = http(
            server.port,
            &format!(
                "/api/query?namespace=influxdb3&pod=db3-ingester-0&database=mydb&q={statement}"
            ),
        );
        assert_eq!(status, 400, "should reject {statement}: {body}");
    }
    let calls = dir.path().join("kubectl-calls");
    assert!(
        !calls.exists() || !fs::read_to_string(&calls).unwrap().contains("db3-query"),
        "a rejected statement must not reach the cluster"
    );
}

#[test]
fn query_terminal_surfaces_the_database_error() {
    let dir = fixture();
    let token = "apiv3_error-token";
    let server = start_with(dir.path(), &["--query"], &[("INFLUXDB3_TOKEN", token)]);
    let (status, body) = http(
        server.port,
        "/api/query?namespace=influxdb3&pod=db3-ingester-0&database=mydb&q=SELECT%20FAILME",
    );
    assert_eq!(status, 200, "{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["row_count"], 0);
    let error = value["error"].as_str().unwrap_or_default();
    assert!(error.contains("unknown measurement"), "{error}");
    assert!(!body.contains(token), "{body}");
}

#[test]
fn query_terminal_requires_an_admin_token() {
    let dir = tempfile::tempdir().unwrap();
    let output = env_command(dir.path())
        .args(["webui", "--query", "--port", "0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("INFLUXDB3_TOKEN"), "{combined}");
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

#[test]
fn idle_listener_survives_socket_timeouts_and_accepts_new_requests() {
    let dir = fixture();
    let mut server = start(dir.path(), &[]);
    // No request at all for more than two inherited listener I/O timeouts.
    std::thread::sleep(Duration::from_millis(4500));
    assert!(
        server.child.try_wait().unwrap().is_none(),
        "console exited while idle rather than staying alive until Ctrl+C"
    );
    assert_eq!(http(server.port, "/api/health").0, 200);
}

#[test]
fn body_headers_are_rejected_before_readers_and_shutdown_is_bounded() {
    for signal in ["-INT", "-TERM"] {
        for header in [
            "Content-Length: 1",
            "Content-Length: 2048",
            "Content-Length: 18446744073709551615",
            "Content-Length: invalid",
            "Content-Length: -1",
            "Transfer-Encoding: chunked",
        ] {
            let dir = fixture();
            let mut server = start(dir.path(), &[]);
            let mut socket = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            write!(
                socket,
                "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{}\r\n\r\n{}",
                server.port,
                header,
                if header == "Content-Length: 1" {
                    ""
                } else {
                    "x"
                }
            )
            .unwrap();
            let mut drip = socket.try_clone().unwrap();
            let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
            let active = running.clone();
            let writer = std::thread::spawn(move || {
                while active.load(std::sync::atomic::Ordering::SeqCst) {
                    let _ = drip.write_all(b"x");
                    std::thread::sleep(Duration::from_millis(200));
                }
            });
            let mut response = [0; 128];
            let result = socket.read(&mut response);
            let pid = server.child.id().to_string();
            assert!(
                Command::new("kill")
                    .args([signal, &pid])
                    .status()
                    .unwrap()
                    .success()
            );
            let deadline = Instant::now() + Duration::from_secs(4);
            let mut exited = false;
            while Instant::now() < deadline {
                if server.child.try_wait().unwrap().is_some() {
                    exited = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            running.store(false, std::sync::atomic::Ordering::SeqCst);
            writer.join().unwrap();
            assert!(
                exited,
                "shutdown blocked while client dripped: {header} {signal}"
            );
            let size = result.expect("immediate HTTP rejection");
            assert!(
                String::from_utf8_lossy(&response[..size]).starts_with("HTTP/1.1 400"),
                "{header}: {:?}",
                String::from_utf8_lossy(&response[..size])
            );
        }
    }
}

#[test]
fn empty_body_requests_remain_supported() {
    let dir = fixture();
    let server = start(dir.path(), &[]);
    assert_eq!(
        raw_http(
            server.port,
            "/",
            &format!("Host: 127.0.0.1:{}\r\nContent-Length: 0\r\n", server.port)
        )
        .0,
        200
    );
    assert_eq!(http(server.port, "/api/health").0, 200);
    let host = format!("Host: 127.0.0.1:{}\r\n", server.port);
    assert_eq!(
        raw_http(server.port, "/", &format!("{host}Content-Length: 2048\r\n")).0,
        400
    );
    assert_eq!(
        raw_method(
            server.port,
            "HEAD",
            "/",
            &format!("{host}Content-Length: 0\r\n")
        )
        .0,
        200
    );
}

#[test]
fn request_header_buffers_are_bounded() {
    let dir = fixture();
    let server = start(dir.path(), &[]);
    for extra in [
        format!("X-Large: {}\r\n", "x".repeat(8192)),
        "X-A: a\r\n".repeat(65),
        format!("X-A: {}\r\n", "x".repeat(7000)).repeat(5),
    ] {
        assert_eq!(
            raw_http(
                server.port,
                "/",
                &format!("Host: 127.0.0.1:{}\r\n{extra}", server.port)
            )
            .0,
            400
        );
    }
}

#[test]
fn helm_is_never_invoked_without_explicit_opt_in() {
    for flags in [vec![], vec!["--include-secret-metadata"]] {
        let dir = fixture();
        let server = start(dir.path(), &flags);
        let value = snapshot(server.port);
        assert!(
            !dir.path().join("helm-calls").exists(),
            "default collection invoked Helm"
        );
        assert_eq!(value["helm_enabled"], false);
        assert_eq!(value["helm"], serde_json::json!([]));
    }
    let dir = fixture();
    let server = start(dir.path(), &["--helm"]);
    let value = snapshot(server.port);
    assert_eq!(value["helm_enabled"], true);
    assert_eq!(value["helm"][0]["name"], "db3");
    assert_eq!(value["secrets"], serde_json::json!([]));
    assert!(dir.path().join("helm-calls").exists());
}

#[test]
fn configuration_truncation_stays_partial_with_bounded_data() {
    for resource in ["configmaps", "secrets"] {
        let dir = fixture();
        let rows: Vec<_> = (0..501).map(|i| serde_json::json!({"metadata":{"name":format!("row-{i}"),"namespace":"influxdb3"},"data":{"key":"opaque"}})).collect();
        write_fixture(
            dir.path(),
            &format!("{resource}.json"),
            &serde_json::json!({"items":rows}).to_string(),
        );
        let server = start(dir.path(), &["--include-secret-metadata"]);
        let value = snapshot(server.port);
        assert!(value[resource].as_array().unwrap().len() <= 500);
        assert!(!value[resource].as_array().unwrap().is_empty());
        let status = value["collectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "configuration")
            .unwrap();
        assert_eq!(status["ok"], false, "{resource}: {status}");
        assert!(status["error"].as_str().unwrap().contains("partial"));
    }
}

#[test]
fn configuration_aggregate_budgets_limit_namespaces_rows_and_bytes() {
    for mode in ["namespaces", "rows", "bytes"] {
        let dir = fixture();
        let mut pods: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("pods.json")).unwrap())
                .unwrap();
        if mode == "namespaces" {
            let pod = pods["items"][0].clone();
            pods["items"] = serde_json::json!(
                (0..40)
                    .map(|i| {
                        let mut p = pod.clone();
                        p["metadata"]["namespace"] = serde_json::json!(format!("ns-{i:02}"));
                        p
                    })
                    .collect::<Vec<_>>()
            );
            write_fixture(dir.path(), "pods.json", &pods.to_string());
        } else {
            let count = if mode == "rows" { 400 } else { 2 };
            let data = if mode == "bytes" {
                "x".repeat(600_000)
            } else {
                "opaque".to_owned()
            };
            let rows: Vec<_> = (0..count).map(|i| serde_json::json!({"metadata":{"name":format!("row-{i}"),"namespace":"influxdb3"},"data":{data.clone():"opaque"}})).collect();
            for resource in ["configmaps", "secrets"] {
                write_fixture(
                    dir.path(),
                    &format!("{resource}.json"),
                    &serde_json::json!({"items":rows}).to_string(),
                );
            }
        }
        let server = start(dir.path(), &["--include-secret-metadata"]);
        let value = snapshot(server.port);
        let config = value["collectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "configuration")
            .unwrap();
        assert_eq!(config["ok"], false, "{mode}: {config}");
        assert!(config["error"].as_str().unwrap().contains("partial"));
        assert!(
            value["configmaps"].as_array().unwrap().len()
                + value["secrets"].as_array().unwrap().len()
                <= 500
        );
        assert!(
            serde_json::to_vec(&value["configmaps"]).unwrap().len()
                + serde_json::to_vec(&value["secrets"]).unwrap().len()
                <= 1_048_580
        );
        let calls = fs::read_to_string(dir.path().join("kubectl-calls")).unwrap();
        assert!(
            calls
                .lines()
                .filter(|l| l.contains("get configmaps") || l.contains("get secrets"))
                .count()
                <= 32
        );
    }
}

#[test]
fn database_roles_use_verified_pod_owners_not_component_labels() {
    let dir = fixture();
    let mut workloads: Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("workloads.json")).unwrap())
            .unwrap();
    let mut unrelated = workloads["items"][0].clone();
    unrelated["metadata"]["name"] = serde_json::json!("unrelated");
    unrelated["spec"]["replicas"] = serde_json::json!(99);
    workloads["items"]
        .as_array_mut()
        .unwrap()
        .insert(0, unrelated);
    write_fixture(dir.path(), "workloads.json", &workloads.to_string());
    let server = start(dir.path(), &[]);
    let value = snapshot(server.port);
    let role = value["db3"][0]["roles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["role"] == "ingester")
        .unwrap();
    assert_eq!(role["desired"], 2);
    assert_eq!(role["workload"], "db3-ingester");
}

#[test]
fn explicit_no_secrets_conflicts_with_helm_storage_consent() {
    let dir = fixture();
    let mut child = env_command(dir.path())
        .args(["webui", "--no-secrets", "--helm"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("conflicting flags unexpectedly started the server");
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used"));
}

#[test]
fn ambiguous_pod_owner_is_not_arbitrarily_selected() {
    let dir = fixture();
    let mut pods: Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("pods.json")).unwrap()).unwrap();
    pods["items"][0]["metadata"]["ownerReferences"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"kind":"StatefulSet","name":"other"}));
    write_fixture(dir.path(), "pods.json", &pods.to_string());
    let server = start(dir.path(), &[]);
    let value = snapshot(server.port);
    let role = value["db3"][0]["roles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["role"] == "ingester")
        .unwrap();
    assert!(role["workload"].is_null());
    assert!(role["desired"].is_null());
}

#[test]
fn verified_owners_are_deduplicated_and_unknown_ancestry_stays_unmapped() {
    for mode in ["deduplicate", "sum", "replicaset", "namespace"] {
        let dir = fixture();
        let mut pods: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("pods.json")).unwrap())
                .unwrap();
        let mut workloads: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("workloads.json")).unwrap())
                .unwrap();
        if mode == "replicaset" {
            pods["items"][0]["metadata"]["ownerReferences"][0]["kind"] =
                serde_json::json!("ReplicaSet");
        } else if mode == "namespace" {
            workloads["items"][0]["metadata"]["namespace"] = serde_json::json!("other");
        } else {
            let mut extra = pods["items"][0].clone();
            extra["metadata"]["name"] = serde_json::json!("extra-0");
            if mode == "sum" {
                extra["metadata"]["ownerReferences"][0]["name"] = serde_json::json!("second-owner");
                let mut owner = workloads["items"][0].clone();
                owner["metadata"]["name"] = serde_json::json!("second-owner");
                owner["spec"]["replicas"] = serde_json::json!(3);
                workloads["items"].as_array_mut().unwrap().push(owner);
            }
            pods["items"].as_array_mut().unwrap().push(extra);
        }
        write_fixture(dir.path(), "pods.json", &pods.to_string());
        write_fixture(dir.path(), "workloads.json", &workloads.to_string());
        let server = start(dir.path(), &[]);
        let value = snapshot(server.port);
        let role = value["db3"][0]["roles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["role"] == "ingester")
            .unwrap();
        match mode {
            "deduplicate" => {
                assert_eq!(role["desired"], 2);
                assert_eq!(role["workload"], "db3-ingester");
            }
            "sum" => {
                assert_eq!(role["desired"], 5);
                assert!(role["workload"].is_null());
            }
            _ => {
                assert!(role["desired"].is_null());
                assert!(role["workload"].is_null());
            }
        }
    }
}

#[test]
fn configuration_byte_budget_counts_final_masked_representation() {
    let dir = fixture();
    let data: serde_json::Map<String, Value> = (0..22000)
        .map(|i| (format!("k{i}"), serde_json::json!("")))
        .collect();
    write_fixture(dir.path(), "configmaps.json", &serde_json::json!({"items":[{"metadata":{"name":"many-keys","namespace":"influxdb3"},"data":data}]}).to_string());
    let server = start(dir.path(), &[]);
    let value = snapshot(server.port);
    let bytes = serde_json::to_vec(&value["configmaps"]).unwrap().len()
        + serde_json::to_vec(&value["secrets"]).unwrap().len();
    assert!(
        bytes <= 1_048_576,
        "final configuration grew after budget check: {bytes}"
    );
    let config = value["collectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "configuration")
        .unwrap();
    assert_eq!(config["ok"], false);
}
