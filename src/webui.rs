//! `kubectl db3 webui` — a temporary, loopback-only console for the
//! Kubernetes environment that hosts InfluxDB 3.
//!
//! The server lives exactly as long as this process: Ctrl+C stops it and the
//! port is released. It serves one embedded HTML document and a small JSON
//! API built from read-only `kubectl`/`helm` calls.

use crate::collect::{self, Options};
use serde_json::{Value, json};
use std::io::Write as _;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const INDEX: &str = include_str!("../webui/dist/index.html");
const MANIFEST: &str = include_str!("../deploy/webui/rbac.yaml");
const MIN_FORCED_REFRESH: Duration = Duration::from_secs(2);

#[derive(clap::Args)]
pub struct Webui {
    /// TCP port for the local console
    #[arg(long, default_value_t = 9191)]
    port: u16,
    /// Loopback address to bind; remote/unspecified addresses are rejected
    #[arg(long, default_value = "127.0.0.1")]
    address: String,
    /// Only collect this namespace instead of all namespaces
    #[arg(short = 'n', long)]
    namespace: Option<String>,
    /// kubeconfig context to read
    #[arg(long)]
    context: Option<String>,
    /// Seconds a snapshot stays cached before it is rebuilt
    #[arg(long, default_value_t = 10)]
    refresh: u64,
    /// Per-collector timeout in seconds
    #[arg(long, default_value_t = 60.0, allow_hyphen_values = true)]
    timeout: f64,
    /// Print the read-only RBAC manifest and exit
    #[arg(long)]
    manifest: bool,
    /// Do not read Secrets at all (names and keys are omitted)
    #[arg(long)]
    no_secrets: bool,
    /// Opt in to Secret names and key names (requires full Secret read permissions)
    #[arg(long, conflicts_with = "no_secrets")]
    include_secret_metadata: bool,
    /// Opt in to Helm releases; Helm may read storage Secrets using your credentials
    #[arg(long, conflicts_with = "no_secrets")]
    helm: bool,
}

struct Cache {
    at: Option<Instant>,
    body: Option<String>,
}

struct State {
    options: Options,
    cache_seconds: u64,
    cache: Mutex<Cache>,
    timeout: f64,
    token: String,
    session: String,
    authority: String,
    nonce: String,
    stop: Arc<AtomicBool>,
}

pub fn run(args: Webui) -> Result<(), Box<dyn std::error::Error>> {
    if args.manifest {
        print!("{MANIFEST}");
        std::io::stdout().flush()?;
        return Ok(());
    }
    if !args.timeout.is_finite() || args.timeout <= 0.0 {
        return Err("--timeout must be a finite positive number".into());
    }
    if args.refresh > 3600 {
        return Err("--refresh must be 3600 seconds or less".into());
    }

    if !is_loopback(&args.address) {
        return Err("--address must be a loopback IP address or localhost".into());
    }
    let bind = format!("{}:{}", display_host(&args.address), args.port);
    let listener = TcpListener::bind(&bind).map_err(|error| {
        format!(
            "could not start the console on {bind}: {error}. \
             Another process may already hold the port — choose another with --port."
        )
    })?;
    // Accepted sockets inherit these idle I/O bounds on supported Unix hosts.
    // This also bounds incomplete HTTP headers and clients that stop reading.
    let socket = socket2::SockRef::from(&listener);
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.set_write_timeout(Some(Duration::from_secs(2)))?;
    let server = Server::from_listener(listener, None).map_err(|_| "cannot start HTTP listener")?;
    let actual = server
        .server_addr()
        .to_ip()
        .map(|address| address.port())
        .unwrap_or(args.port);
    let host = display_host(&args.address);

    println!(
        "db3 webui {} — read-only environment console",
        env!("CARGO_PKG_VERSION")
    );
    println!("  URL      http://{host}:{actual}");
    println!(
        "  Context  {}",
        args.context.as_deref().unwrap_or("kubectl default")
    );
    println!(
        "  Scope    {}",
        args.namespace
            .as_deref()
            .map_or("all namespaces".to_owned(), |namespace| format!(
                "namespace {namespace}"
            ))
    );
    println!("  Cache    {}s per snapshot", args.refresh);
    println!("  Stop     Ctrl+C (the port is released when this process exits)");
    if !is_loopback(&args.address) {
        println!(
            "  Warning  bound to a non-loopback address: anyone who can reach port {actual} can read this snapshot."
        );
    }

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst))
            .map_err(|_| "cannot install WebUI shutdown handler")?;
    }

    let state = Arc::new(State {
        options: Options {
            namespace: args.namespace.clone(),
            context: args.context.clone(),
            timeout: args.timeout,
            no_secrets: !args.include_secret_metadata || args.no_secrets,
            helm: args.helm,
            stop: Arc::clone(&stop),
        },
        cache_seconds: args.refresh,
        cache: Mutex::new(Cache {
            at: None,
            body: None,
        }),
        timeout: args.timeout,
        token: std::env::var("INFLUXDB3_TOKEN").unwrap_or_default(),
        stop: Arc::clone(&stop),
        session: random_hex()?,
        nonce: random_hex()?,
        authority: format!("{host}:{actual}"),
    });

    let mut workers = Vec::new();
    while !stop.load(Ordering::SeqCst) {
        workers.retain(|worker: &std::thread::JoinHandle<()>| !worker.is_finished());
        match server.recv_timeout(Duration::from_millis(200)) {
            Ok(Some(request)) => {
                let state = Arc::clone(&state);
                if workers.len() >= 16 {
                    respond(
                        request,
                        api(json!({"error":"console busy"}).to_string(), 503),
                    );
                } else {
                    workers.push(std::thread::spawn(move || handle(&state, request)));
                }
            }
            Ok(None) => continue,
            Err(error) => {
                eprintln!("db3: console request failed: {error}");
                break;
            }
        }
    }
    stop.store(true, Ordering::SeqCst);
    for worker in workers {
        let _ = worker.join();
    }
    drop(server);
    println!("db3: console stopped; port {actual} released.");
    Ok(())
}

fn display_host(address: &str) -> String {
    match address {
        "0.0.0.0" => "localhost".to_owned(),
        "::" | "::0" => "[::1]".to_owned(),
        value if value.contains(':') => format!("[{value}]"),
        value => value.to_owned(),
    }
}

fn is_loopback(address: &str) -> bool {
    address == "localhost"
        || address
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn random_hex() -> Result<String, Box<dyn std::error::Error>> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "cannot generate session randomness")?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn shell(state: &State) -> String {
    INDEX
        .replace(
            "content=\"__DB3_SESSION__\"",
            &format!("content=\"{}\"", state.session),
        )
        // Only the actual singlefile module tag, never script-like JS strings.
        .replacen(
            "<script type=\"module\"",
            &format!("<script type=\"module\" nonce=\"{}\"", state.nonce),
            1,
        )
}

fn handle(state: &State, request: Request) {
    let values = |name: &'static str| {
        request
            .headers()
            .iter()
            .filter(|h| h.field.equiv(name))
            .map(|h| h.value.as_str())
            .collect::<Vec<_>>()
    };
    if values("Host") != [state.authority.as_str()] || {
        let origins = values("Origin");
        !origins.is_empty() && origins != [format!("http://{}", state.authority).as_str()]
    } {
        return respond(
            request,
            api(
                json!({"error":"local same-origin authority required"}).to_string(),
                403,
            ),
        );
    }
    if request
        .url()
        .split('?')
        .next()
        .is_some_and(|p| p.starts_with("/api/"))
        && values("X-DB3-Session") != [state.session.as_str()]
    {
        return respond(
            request,
            api(json!({"error":"session header required"}).to_string(), 401),
        );
    }
    let method = request.method().clone();
    if method != Method::Get && method != Method::Head {
        let mut response =
            Response::from_string("method not allowed").with_status_code(StatusCode(405));
        response.add_header(header("Allow", "GET, HEAD"));
        return respond(request, response);
    }
    let head = method == Method::Head;
    let url = request.url().to_owned();
    let (path, query) = match url.split_once('?') {
        Some((path, query)) => (path, query),
        None => (url.as_str(), ""),
    };

    match path {
        "/" | "/index.html" => {
            let body = if head { String::new() } else { shell(state) };
            let response = document(body, "text/html; charset=utf-8", &state.nonce);
            respond(request, response);
        }
        "/api/health" => {
            let body = if head {
                String::new()
            } else {
                json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}).to_string()
            };
            respond(request, api(body, 200));
        }
        "/api/snapshot" => {
            let force = param(query, "refresh").as_deref() == Some("1");
            match snapshot(state, force) {
                Ok(body) => {
                    let body = if head { String::new() } else { body };
                    respond(request, api(body, 200));
                }
                Err(error) => {
                    let body = json!({"error": error}).to_string();
                    respond(request, api(body, 503));
                }
            }
        }
        "/api/logs" => match logs(state, query) {
            Ok(value) => {
                let body = if head {
                    String::new()
                } else {
                    value.to_string()
                };
                respond(request, api(body, 200));
            }
            Err(error) => respond(request, api(json!({"error": error}).to_string(), 400)),
        },
        "/api/manifest" => {
            let body = if head {
                String::new()
            } else {
                MANIFEST.to_owned()
            };
            respond(
                request,
                document(body, "text/yaml; charset=utf-8", &state.nonce),
            );
        }
        "/favicon.ico" => {
            respond(
                request,
                Response::from_string(String::new()).with_status_code(StatusCode(204)),
            );
        }
        other if other.starts_with("/api/") => {
            respond(
                request,
                api(json!({"error": "unknown endpoint"}).to_string(), 404),
            );
        }
        _ => {
            // Hash-based routing means any other path is still the app shell.
            let body = if head { String::new() } else { shell(state) };
            respond(
                request,
                document(body, "text/html; charset=utf-8", &state.nonce),
            );
        }
    }
}

fn respond(request: Request, mut response: Response<std::io::Cursor<Vec<u8>>>) {
    for (key, value) in [
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
        ("X-Frame-Options", "DENY"),
    ] {
        response.add_header(header(key, value));
    }
    let _ = request.respond(response);
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("static header names and values")
}

fn api(body: String, status: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_string(body).with_status_code(StatusCode(status));
    response.add_header(header("Content-Type", "application/json; charset=utf-8"));
    response.add_header(header("Cache-Control", "no-store"));
    response.add_header(header("X-Content-Type-Options", "nosniff"));
    response.add_header(header("Referrer-Policy", "no-referrer"));
    response
}

fn document(body: String, content_type: &str, nonce: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_string(body).with_status_code(StatusCode(200));
    response.add_header(header("Content-Type", content_type));
    response.add_header(header("Cache-Control", "no-store"));
    response.add_header(header("X-Content-Type-Options", "nosniff"));
    response.add_header(header("X-Frame-Options", "DENY"));
    response.add_header(header("Referrer-Policy", "no-referrer"));
    response.add_header(header(
        "Content-Security-Policy",
        &format!("default-src 'none'; script-src 'nonce-{nonce}'; style-src 'unsafe-inline'; img-src data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"),
    ));
    response
}

fn snapshot(state: &State, force: bool) -> Result<String, String> {
    let mut cache = state
        .cache
        .lock()
        .map_err(|_| "snapshot cache is unavailable".to_owned())?;
    let age = cache.at.map(|at| at.elapsed());
    if let (Some(body), Some(age)) = (cache.body.as_ref(), age) {
        if !force && age < Duration::from_secs(state.cache_seconds) {
            return Ok(body.clone());
        }
        if force && age < MIN_FORCED_REFRESH {
            return Ok(body.clone());
        }
    }
    let value = collect::snapshot(&state.options);
    let body = crate::redact(
        &serde_json::to_string(&value).map_err(|_| "cannot serialize snapshot")?,
        &state.token,
    );
    cache.at = Some(Instant::now());
    cache.body = Some(body.clone());
    Ok(body)
}

fn logs(state: &State, query: &str) -> Result<Value, String> {
    let namespace = param(query, "namespace").unwrap_or_else(|| "default".to_owned());
    let pod = param(query, "pod").ok_or_else(|| "pod is required".to_owned())?;
    let container = param(query, "container").filter(|value| !value.is_empty());
    let tail = param(query, "tail")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(500)
        .clamp(1, 20_000);

    for (label, value) in [
        ("namespace", Some(&namespace)),
        ("pod", Some(&pod)),
        ("container", container.as_ref()),
    ] {
        if let Some(value) = value {
            if !valid_name(value) {
                return Err(format!("invalid {label} name"));
            }
        }
    }

    let mut argv = Vec::new();
    if let Some(context) = state
        .options
        .context
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        argv.push("--context".to_owned());
        argv.push(context.to_owned());
    }
    argv.extend([
        "logs".to_owned(),
        pod.clone(),
        "-n".to_owned(),
        namespace.clone(),
        "--tail".to_owned(),
        tail.to_string(),
    ]);
    if let Some(container) = container.as_ref() {
        argv.push("-c".to_owned());
        argv.push(container.clone());
    }

    let captured =
        crate::webui_process::capture("kubectl", &argv, state.timeout.min(30.0), &state.stop)
            .map_err(|error| format!("could not run kubectl: {error}"))?;
    let lines = crate::redact(&captured.stdout, &state.token);
    let stderr = crate::redact(&captured.stderr, &state.token);
    let error = captured.error.map(|error| {
        if stderr.trim().is_empty() {
            error
        } else {
            format!("{error}: {}", stderr.trim())
        }
    });
    Ok(json!({
        "namespace": namespace,
        "pod": pod,
        "container": container,
        "tail": tail,
        "redacted": !state.token.is_empty(),
        "error": error,
        "lines": lines,
    }))
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && !value.starts_with('-')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '.' | '_')
        })
}

fn param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (decode(name) == key).then(|| decode(value))
    })
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
                match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(byte) => {
                        output.push(byte);
                        index += 3;
                    }
                    None => {
                        output.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_argument_injection_in_names() {
        assert!(valid_name("db3-querier-0"));
        assert!(valid_name("influxdb3.querier-0_x"));
        assert!(!valid_name("--token"));
        assert!(!valid_name("ns;rm -rf"));
        assert!(!valid_name(""));
    }

    #[test]
    fn decodes_query_values() {
        assert_eq!(
            param("a=1&pod=db3%2D0&tail=5", "pod"),
            Some("db3-0".to_owned())
        );
        assert_eq!(param("tail=5", "tail"), Some("5".to_owned()));
        assert_eq!(param("tail=5", "missing"), None);
    }
}
