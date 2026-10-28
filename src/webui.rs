//! `kubectl db3 webui` — a temporary console for the
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
    /// Opt in to the read-only query terminal (needs INFLUXDB3_TOKEN and pods/exec)
    #[arg(long)]
    query: bool,
    /// Opt in to the Basic Information panel (runs `influxdb3 show nodes` and
    /// `show license info` in a database pod; needs INFLUXDB3_TOKEN and pods/exec)
    #[arg(long)]
    info: bool,
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
    /// True when bound to loopback, where the Host header is pinned to the
    /// address we chose. A non-loopback bind cannot know the name or address a
    /// client will use, so the Host header is accepted as-is and the Origin is
    /// still required to be same-origin with it.
    pin_authority: bool,
    nonce: String,
    query_enabled: bool,
    /// True when the console was started with `--info`, which is the only path
    /// that reads `influxdb3 show nodes` / `show license info` from a pod.
    info_enabled: bool,
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

    let token = std::env::var("INFLUXDB3_TOKEN").unwrap_or_default();
    if args.query && token.trim().is_empty() {
        return Err("--query requires INFLUXDB3_TOKEN in the environment".into());
    }
    if args.info && token.trim().is_empty() {
        return Err("--info requires INFLUXDB3_TOKEN in the environment".into());
    }
    let bind = format!("{}:{}", bind_host(&args.address), args.port);
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
    println!(
        "  Query    {}",
        if args.query {
            "read-only SELECT/SHOW terminal enabled"
        } else {
            "disabled (enable with --query)"
        }
    );
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
            info: args.info,
            stop: Arc::clone(&stop),
        },
        cache_seconds: args.refresh,
        cache: Mutex::new(Cache {
            at: None,
            body: None,
        }),
        timeout: args.timeout,
        token,
        query_enabled: args.query,
        info_enabled: args.info,
        stop: Arc::clone(&stop),
        session: random_hex()?,
        nonce: random_hex()?,
        authority: format!("{host}:{actual}"),
        pin_authority: is_loopback(&args.address),
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

/// The address to bind. Brackets IPv6 literals so they can be joined with a port;
/// everything else is used verbatim, which matters because `0.0.0.0` and `::`
/// must not be normalised to `localhost` or the listener silently narrows to
/// loopback while the banner claims otherwise.
fn bind_host(address: &str) -> String {
    match address {
        value if value.contains(':') && !value.starts_with('[') => format!("[{value}]"),
        value => value.to_owned(),
    }
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
    // Exactly one Host header is required. When bound to loopback it must be the
    // authority we chose, which blocks DNS-rebinding; a non-loopback bind cannot
    // predict the name or address a client used, so it is accepted and the Origin
    // is still required to match it.
    let hosts = values("Host");
    let host = match hosts.as_slice() {
        [host] if !host.is_empty() && (!state.pin_authority || *host == state.authority) => *host,
        _ => {
            return respond(
                request,
                api(
                    json!({"error":"local same-origin authority required"}).to_string(),
                    403,
                ),
            );
        }
    };
    let origins = values("Origin");
    if !origins.is_empty() && origins != [format!("http://{host}").as_str()] {
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
        "/api/info" => match run_info(state, query) {
            Ok(value) => {
                let body = if head {
                    String::new()
                } else {
                    value.to_string()
                };
                respond(request, api(body, 200));
            }
            Err((status, error)) => {
                respond(request, api(json!({"error": error}).to_string(), status))
            }
        },
        "/api/query" => match run_query(state, query) {
            Ok(value) => {
                let body = if head {
                    String::new()
                } else {
                    value.to_string()
                };
                respond(request, api(body, 200));
            }
            Err((status, error)) => {
                respond(request, api(json!({"error": error}).to_string(), status))
            }
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

/// Decoded statement cap. tiny_http rejects request lines over 8 KiB and the
/// statement travels percent-encoded, so this leaves room for the other params.
const MAX_STATEMENT: usize = 2048;
/// Rows handed to the browser; the process output is separately capped at 4 MiB.
const MAX_ROWS: usize = 1000;

/// The two read-only views that `kubectl db3 support info` prints, surfaced for
/// the Basic Information panel. Both run in the selected pod through the same
/// exec helper the query panel uses, so the posture does not move: `--info` is
/// required, `pods/exec` comes from the operator's own kubeconfig, and the token
/// travels on stdin rather than in argv.
fn run_info(state: &State, query: &str) -> Result<Value, (u16, String)> {
    if !state.info_enabled {
        return Err((
            403,
            "the Basic Information panel is disabled; restart the console with --info to enable it"
                .to_owned(),
        ));
    }
    let namespace = param(query, "namespace").unwrap_or_else(|| "default".to_owned());
    let pod = param(query, "pod").ok_or((400, "pod is required".to_owned()))?;
    let container = param(query, "container").filter(|value| !value.is_empty());

    for (label, value) in [
        ("namespace", Some(&namespace)),
        ("pod", Some(&pod)),
        ("container", container.as_ref()),
    ] {
        if let Some(value) = value {
            if !valid_name(value) {
                return Err((400, format!("invalid {label} name")));
            }
        }
    }

    let mut base = Vec::new();
    if let Some(context) = state
        .options
        .context
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        base.push("--context".to_owned());
        base.push(context.to_owned());
    }
    base.extend(["exec", "-i", pod.as_str(), "-n", namespace.as_str()].map(str::to_owned));
    if let Some(container) = container.as_ref() {
        base.push("-c".to_owned());
        base.push(container.clone());
    }
    base.extend(["--", "sh", "-c", crate::REMOTE, "db3-info"].map(str::to_owned));

    let mut nodes = Value::Null;
    let mut license = Value::Null;
    let mut errors = serde_json::Map::new();

    for (name, command) in crate::COMMANDS
        .iter()
        .filter(|(name, _)| matches!(*name, "nodes" | "license-info"))
    {
        let argv: Vec<String> = base
            .iter()
            .map(String::as_str)
            .chain(command.iter().copied())
            .map(str::to_owned)
            .collect();
        let captured = crate::webui_process::capture_exec(
            "kubectl",
            &argv,
            &format!("{}\n", state.token),
            state.timeout,
            &state.stop,
        )
        .map_err(|error| (500, format!("could not run kubectl: {error}")))?;

        let stdout = crate::redact(&captured.stdout, &state.token);
        let stderr = crate::redact(captured.stderr.trim(), &state.token);
        let parsed = serde_json::from_str::<Value>(stdout.trim()).ok();
        if let Some(error) = captured.error.clone().or_else(|| {
            parsed
                .is_none()
                .then(|| "command returned invalid JSON".to_owned())
        }) {
            let detail = if stderr.is_empty() {
                error
            } else {
                format!("{error}: {stderr}")
            };
            errors.insert((*name).to_owned(), json!(detail));
        }
        match *name {
            "nodes" => nodes = parsed.unwrap_or(Value::Null),
            "license-info" => license = parsed.unwrap_or(Value::Null),
            _ => {}
        }
    }

    Ok(json!({
        "enabled": true,
        "namespace": namespace,
        "pod": pod,
        "container": container,
        "nodes": nodes,
        "license": license,
        "errors": Value::Object(errors),
    }))
}

fn run_query(state: &State, query: &str) -> Result<Value, (u16, String)> {
    if !state.query_enabled {
        return Err((
            403,
            "the query terminal is disabled; restart the console with --query to enable it"
                .to_owned(),
        ));
    }
    let namespace = param(query, "namespace").unwrap_or_else(|| "default".to_owned());
    let pod = param(query, "pod").ok_or((400, "pod is required".to_owned()))?;
    let container = param(query, "container").filter(|value| !value.is_empty());
    let database = param(query, "database").ok_or((400, "database is required".to_owned()))?;
    let language = param(query, "language").unwrap_or_else(|| "sql".to_owned());
    let statement = param(query, "q").ok_or((400, "q is required".to_owned()))?;

    if !matches!(language.as_str(), "sql" | "influxql") {
        return Err((400, "language must be sql or influxql".to_owned()));
    }
    for (label, value) in [
        ("namespace", Some(&namespace)),
        ("pod", Some(&pod)),
        ("container", container.as_ref()),
        ("database", Some(&database)),
    ] {
        if let Some(value) = value {
            if !valid_name(value) {
                return Err((400, format!("invalid {label} name")));
            }
        }
    }
    let statement = read_only_statement(&statement)?;

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
        "exec".to_owned(),
        "-i".to_owned(),
        pod.clone(),
        "-n".to_owned(),
        namespace.clone(),
    ]);
    if let Some(container) = container.as_ref() {
        argv.push("-c".to_owned());
        argv.push(container.clone());
    }
    argv.extend(["--", "sh", "-c", crate::REMOTE, "db3-query"].map(str::to_owned));
    argv.extend([
        "query".to_owned(),
        "--database".to_owned(),
        database.clone(),
        "--language".to_owned(),
        language.clone(),
        "--format".to_owned(),
        "json".to_owned(),
        statement.clone(),
    ]);

    let captured = crate::webui_process::capture_exec(
        "kubectl",
        &argv,
        &format!("{}\n", state.token),
        state.timeout,
        &state.stop,
    )
    .map_err(|error| (500, format!("could not run kubectl: {error}")))?;

    let stdout = crate::redact(&captured.stdout, &state.token);
    let stderr = crate::redact(captured.stderr.trim(), &state.token);

    let mut rows: Vec<Value> = Vec::new();
    let mut truncated = false;
    if captured.error.is_none() {
        match serde_json::from_str::<Value>(stdout.trim()) {
            Ok(Value::Array(items)) => {
                truncated = items.len() > MAX_ROWS;
                rows.extend(items.into_iter().take(MAX_ROWS));
            }
            Ok(_) => return Err((502, "query output was not a JSON array".to_owned())),
            Err(_) => return Err((502, "query output was not valid JSON".to_owned())),
        }
    }

    let mut columns: Vec<String> = Vec::new();
    for row in &rows {
        if let Value::Object(map) = row {
            for key in map.keys() {
                if !columns.iter().any(|existing| existing == key) {
                    columns.push(key.clone());
                }
            }
        }
    }
    let row_count = rows.len();

    let error = captured
        .error
        .map(|error| {
            if stderr.is_empty() {
                error
            } else {
                format!("{error}: {stderr}")
            }
        })
        .or_else(|| (row_count == 0 && !stderr.is_empty()).then(|| stderr.clone()));

    Ok(json!({
        "namespace": namespace,
        "pod": pod,
        "container": container,
        "database": database,
        "language": language,
        "statement": statement,
        "columns": columns,
        "rows": rows,
        "row_count": row_count,
        "truncated": truncated,
        "redacted": !state.token.is_empty(),
        "error": error,
    }))
}

/// Reject anything that is not a single read-only statement.
///
/// The console promises to write nothing to the cluster, and query languages can
/// mutate. This is a statement-shape check, not semantic analysis: the first
/// significant keyword must be `SELECT` or `SHOW`, a second statement is
/// rejected, and `INTO` is rejected because `SELECT ... INTO` writes. It is
/// deliberately paired with a read-scoped database token — this check reduces
/// the blast radius, it does not replace database-side authorization.
fn read_only_statement(statement: &str) -> Result<String, (u16, String)> {
    let trimmed = statement.trim();
    if trimmed.is_empty() {
        return Err((400, "q is empty".to_owned()));
    }
    if trimmed.len() > MAX_STATEMENT {
        return Err((
            400,
            format!("query is longer than {MAX_STATEMENT} characters"),
        ));
    }
    if trimmed
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err((400, "query contains control characters".to_owned()));
    }
    let tokens = significant_tokens(trimmed)?;
    let first = tokens
        .first()
        .ok_or((400, "query has no statement".to_owned()))?;
    if !matches!(first.as_str(), "SELECT" | "SHOW") {
        return Err((
            400,
            "only a single read-only SELECT or SHOW statement is allowed".to_owned(),
        ));
    }
    if tokens.iter().any(|token| token == "INTO") {
        return Err((
            400,
            "SELECT ... INTO writes data and is not allowed".to_owned(),
        ));
    }
    Ok(trimmed.to_owned())
}

/// Uppercase keywords that appear outside string literals, quoted identifiers
/// and comments. A statement separator followed by more keywords is an error.
fn significant_tokens(statement: &str) -> Result<Vec<String>, (u16, String)> {
    #[derive(PartialEq)]
    enum Mode {
        Normal,
        Single,
        Double,
        Backtick,
        Line,
        Block,
    }

    fn flush(
        word: &mut String,
        tokens: &mut Vec<String>,
        separator: bool,
    ) -> Result<(), (u16, String)> {
        if word.is_empty() {
            return Ok(());
        }
        if separator {
            return Err((400, "only a single statement is allowed".to_owned()));
        }
        tokens.push(std::mem::take(word));
        Ok(())
    }

    let chars: Vec<char> = statement.chars().collect();
    let mut mode = Mode::Normal;
    let mut tokens: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut separator = false;
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        let next = chars.get(index + 1).copied();
        match mode {
            Mode::Normal => match character {
                '\'' => {
                    flush(&mut word, &mut tokens, separator)?;
                    mode = Mode::Single;
                }
                '"' => {
                    flush(&mut word, &mut tokens, separator)?;
                    mode = Mode::Double;
                }
                '`' => {
                    flush(&mut word, &mut tokens, separator)?;
                    mode = Mode::Backtick;
                }
                '-' if next == Some('-') => {
                    flush(&mut word, &mut tokens, separator)?;
                    mode = Mode::Line;
                    index += 1;
                }
                '/' if next == Some('*') => {
                    flush(&mut word, &mut tokens, separator)?;
                    mode = Mode::Block;
                    index += 1;
                }
                ';' => {
                    flush(&mut word, &mut tokens, separator)?;
                    separator = true;
                }
                character if character.is_alphanumeric() || character == '_' => {
                    word.push(character.to_ascii_uppercase());
                }
                _ => flush(&mut word, &mut tokens, separator)?,
            },
            Mode::Single => {
                if character == '\'' {
                    if next == Some('\'') {
                        index += 1;
                    } else {
                        mode = Mode::Normal;
                    }
                }
            }
            Mode::Double => {
                if character == '"' {
                    mode = Mode::Normal;
                }
            }
            Mode::Backtick => {
                if character == '`' {
                    mode = Mode::Normal;
                }
            }
            Mode::Line => {
                if character == '\n' {
                    mode = Mode::Normal;
                }
            }
            Mode::Block => {
                if character == '*' && next == Some('/') {
                    mode = Mode::Normal;
                    index += 1;
                }
            }
        }
        index += 1;
    }
    match mode {
        Mode::Single => return Err((400, "unterminated string literal".to_owned())),
        Mode::Double | Mode::Backtick => {
            return Err((400, "unterminated quoted identifier".to_owned()));
        }
        Mode::Block => return Err((400, "unterminated comment".to_owned())),
        _ => {}
    }
    flush(&mut word, &mut tokens, separator)?;
    Ok(tokens)
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

    #[test]
    fn accepts_single_read_only_statements() {
        for statement in [
            "SELECT * FROM home",
            "select 1",
            "SHOW TABLES",
            "  SELECT time, value FROM cpu WHERE time > now() - 1h LIMIT 5  ",
            "SELECT * FROM t;",
            "SELECT 'drop table x; into' FROM t",
            "SELECT 1 -- insert into x\n",
            "SELECT /* delete from t */ 1",
            "SELECT \"from\" FROM t",
        ] {
            assert!(
                read_only_statement(statement).is_ok(),
                "should accept: {statement}"
            );
        }
    }

    #[test]
    fn rejects_writes_and_multiple_statements() {
        for statement in [
            "DROP TABLE home",
            "INSERT INTO home VALUES (1)",
            "DELETE FROM home",
            "CREATE TABLE x (a int)",
            "UPDATE t SET a = 1",
            "GRANT ALL ON x TO y",
            "SELECT * INTO target FROM home",
            "select 1; drop table home",
            "SELECT 1;SELECT 2",
            "SHOW TABLES; DROP TABLE x",
            ";",
            "-- only a comment",
            "",
            "   ",
        ] {
            assert!(
                read_only_statement(statement).is_err(),
                "should reject: {statement}"
            );
        }
    }

    #[test]
    fn rejects_unterminated_quoting_and_comments() {
        assert!(read_only_statement("SELECT 'abc").is_err());
        assert!(read_only_statement("SELECT \"abc").is_err());
        assert!(read_only_statement("SELECT `abc").is_err());
        assert!(read_only_statement("SELECT 1 /* trailing").is_err());
    }

    #[test]
    fn rejects_control_characters_and_oversized_queries() {
        assert!(read_only_statement("SELECT 1\u{0}").is_err());
        assert!(read_only_statement("SELECT 1\u{7}").is_err());
        assert!(read_only_statement(&format!("SELECT {}", "a".repeat(MAX_STATEMENT))).is_err());
        assert!(read_only_statement("SELECT 1\nFROM t").is_ok());
    }

    #[test]
    fn escape_sequences_stay_inside_string_literals() {
        // A doubled quote closes nothing, so the trailing content is still one
        // literal and the query remains a single statement.
        assert!(read_only_statement("SELECT 'it''s; DROP TABLE x'").is_ok());
    }
}
